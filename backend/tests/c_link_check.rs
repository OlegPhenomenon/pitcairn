//! External-link checks (§5 item 11): a real HTTP check against a local
//! repository server (live mode, private addresses allowed for the test), the
//! deterministic mock for reserved demo hosts, the SSRF guard, and archives
//! exported before the status migration.

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Redirect;
use axum::routing::{get, head};
use pitcairn::dto::{DashboardResponse, ExternalLinkDto, ListResponse, SubmissionDto};
use pitcairn::linkcheck::{self, LinkStatus};

use common::c::*;
use common::d::{empty_install, project_id, unzip, zip_with};
use common::{Client, TestApp, persona, spawn_app, spawn_app_with};

const ANNA: &str = "anna@demo.pitcairn.invalid";
const MARIA: &str = "maria@demo.pitcairn.invalid";

/// A stand-in university repository: `/data.csv` exists until `present` is
/// cleared; `hits` counts requests that reached it.
#[derive(Clone, Default)]
struct Repo {
    present: Arc<AtomicBool>,
    hits: Arc<AtomicUsize>,
}

async fn data_csv(State(repo): State<Repo>) -> (StatusCode, &'static str) {
    repo.hits.fetch_add(1, Ordering::SeqCst);
    if repo.present.load(Ordering::SeqCst) {
        (StatusCode::OK, "site,date,variable,value,unit\n")
    } else {
        (StatusCode::NOT_FOUND, "")
    }
}

async fn spawn_repo() -> (String, Repo) {
    let repo = Repo::default();
    repo.present.store(true, Ordering::SeqCst);
    let app = axum::Router::new()
        .route("/data.csv", get(data_csv))
        // Ordinary URL whose text merely contains "missing": must be checked
        // for real (it exists).
        .route("/missing/data.csv", get(|| async { "present" }))
        .route("/private", get(|| async { StatusCode::UNAUTHORIZED }))
        .route("/forbidden", get(|| async { StatusCode::FORBIDDEN }))
        .route(
            "/sso",
            get(|| async { Redirect::temporary("/idp/login?return=/sso") }),
        )
        .route("/moved", get(|| async { Redirect::permanent("/data.csv") }))
        .route(
            "/no-head",
            head(|| async { StatusCode::METHOD_NOT_ALLOWED }).get(|| async { "ok" }),
        )
        .route("/broken", get(|| async { StatusCode::SERVICE_UNAVAILABLE }))
        // Redirect targets whose names merely contain a login word are
        // followed (no route → 404); real sign-in targets are not.
        .route(
            "/catalog-moved",
            get(|| async { Redirect::temporary("/cataloging.csv") }),
        )
        .route(
            "/needs-login",
            get(|| async { Redirect::temporary("/login?next=/data.csv") }),
        )
        .route(
            "/federated",
            get(|| async {
                Redirect::temporary("https://idp.example.ac.uk/profile/start?target=data")
            }),
        )
        .with_state(repo.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://127.0.0.1:{port}"), repo)
}

/// A port nothing listens on.
async fn closed_port() -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    listener.local_addr().unwrap().port()
}

async fn live_app(allow_private: bool) -> TestApp {
    spawn_app_with(true, |config| {
        config.link_check_mode = "live".into();
        config.link_check_allow_private = allow_private;
    })
    .await
}

async fn dataset_setup(app: &TestApp) -> (Client, Client, String) {
    let anna = persona(app, "anna").await;
    let maria = persona(app, "maria").await;
    let project = seeded_project(app).await;
    let anna_id = user_id(app, ANNA).await;
    let maria_id = user_id(app, MARIA).await;
    let d = agreed_deliverable(&anna, &maria, &project, "dataset", &anna_id, &maria_id).await;
    (anna, maria, d.id)
}

fn link(url: &str) -> serde_json::Value {
    serde_json::json!({"url": url, "description": "Repository copy", "version_label": "v1"})
}

/// Submit `urls`, run the queued `check_link` jobs, return the links as the
/// coordinator sees them on the deliverable page.
async fn submit_and_check(
    app: &TestApp,
    anna: &Client,
    maria: &Client,
    did: &str,
    urls: &[&str],
) -> Vec<ExternalLinkDto> {
    let links: Vec<_> = urls.iter().map(|u| link(u)).collect();
    let s = submit(anna, did, serde_json::json!({ "links": links })).await;
    run_jobs(app).await;
    let resp = maria
        .get(&format!("/api/v1/deliverables/{did}/submissions"))
        .await;
    assert_eq!(resp.status(), 200);
    let history: ListResponse<SubmissionDto> = maria.json(resp).await;
    history
        .items
        .into_iter()
        .find(|x| x.id == s.id)
        .expect("submission listed")
        .links
}

fn by_url<'a>(links: &'a [ExternalLinkDto], url: &str) -> &'a ExternalLinkDto {
    links.iter().find(|l| l.url == url).expect("link present")
}

async fn lost_urls(app: &TestApp, did: &str) -> Vec<String> {
    pitcairn::deliverables::unavailable_links(&app.pool)
        .await
        .unwrap()
        .into_iter()
        .filter(|l| l.deliverable_id == did)
        .map(|l| l.url)
        .collect()
}

async fn notifications_about(app: &TestApp, url: &str) -> i64 {
    sqlx::query_scalar(
        "SELECT COUNT(*) FROM notifications n JOIN users u ON u.id = n.user_id
         WHERE u.email = ? AND n.kind = 'link.unavailable' AND n.body LIKE ?",
    )
    .bind(MARIA)
    .bind(format!("%{url}"))
    .fetch_one(&app.pool)
    .await
    .unwrap()
}

#[tokio::test]
async fn live_check_available_then_missing_after_file_removed() {
    let app = live_app(true).await;
    let (anna, maria, did) = dataset_setup(&app).await;
    let (repo_base, repo) = spawn_repo().await;
    let url = format!("{repo_base}/data.csv");

    let links = submit_and_check(&app, &anna, &maria, &did, &[&url]).await;
    let l = by_url(&links, &url);
    assert_eq!(l.check_status, "available");
    assert_eq!(l.check_http_status, Some(200));
    assert_eq!(l.check_reason, "Reachable (HTTP 200)");
    assert!(l.last_checked_at.is_some());
    assert!(
        repo.hits.load(Ordering::SeqCst) >= 1,
        "the URL was really fetched"
    );
    assert!(lost_urls(&app, &did).await.is_empty());

    // The repository deletes the file; the coordinator re-checks.
    repo.present.store(false, Ordering::SeqCst);
    sqlx::query("UPDATE external_links SET last_checked_at = '2000-01-01T00:00:00Z' WHERE id = ?")
        .bind(&l.id)
        .execute(&app.pool)
        .await
        .unwrap();
    let resp = maria
        .post(&format!("/api/v1/external-links/{}/check", l.id))
        .await;
    assert_eq!(resp.status(), 200);
    let checked: ExternalLinkDto = maria.json(resp).await;
    assert_eq!(checked.check_status, "missing");
    assert_eq!(checked.check_http_status, Some(404));
    assert_eq!(checked.check_reason, "Not found (HTTP 404)");
    let at = checked.last_checked_at.expect("check time stored");
    assert!(
        at.as_str() > "2000-01-01T00:00:00Z",
        "last_checked_at updated"
    );

    // Maria sees it among the lost links, on her dashboard, and was notified.
    assert_eq!(lost_urls(&app, &did).await, vec![url.clone()]);
    let resp = maria.get("/api/v1/dashboard").await;
    let dash: DashboardResponse = maria.json(resp).await;
    let item = dash.sections["overdue_results"]
        .iter()
        .find(|i| i.kind == "external_link" && i.title.contains(&url))
        .expect("lost link on the coordinator dashboard");
    assert!(item.title.starts_with("Data link missing"));
    assert!(item.subtitle.contains("Not found (HTTP 404)"));
    assert_eq!(item.checked_at.as_deref(), Some(at.as_str()));
    assert_eq!(notifications_about(&app, &url).await, 1);

    // Still missing on the next check: no duplicate notification.
    let resp = maria
        .post(&format!("/api/v1/external-links/{}/check", l.id))
        .await;
    assert_eq!(resp.status(), 200);
    assert_eq!(notifications_about(&app, &url).await, 1);

    // Only the coordinator may trigger checks.
    let resp = anna
        .post(&format!("/api/v1/external-links/{}/check", l.id))
        .await;
    assert_eq!(resp.status(), 403);
}

#[tokio::test]
async fn live_check_login_required_is_not_data_loss() {
    let app = live_app(true).await;
    let (anna, maria, did) = dataset_setup(&app).await;
    let (repo_base, _repo) = spawn_repo().await;
    let private = format!("{repo_base}/private");
    let forbidden = format!("{repo_base}/forbidden");
    let sso = format!("{repo_base}/sso");

    let links = submit_and_check(&app, &anna, &maria, &did, &[&private, &forbidden, &sso]).await;
    let l = by_url(&links, &private);
    assert_eq!(
        (l.check_status.as_str(), l.check_http_status),
        ("login_required", Some(401))
    );
    let l = by_url(&links, &forbidden);
    assert_eq!(
        (l.check_status.as_str(), l.check_http_status),
        ("login_required", Some(403))
    );
    let l = by_url(&links, &sso);
    assert_eq!(l.check_status, "login_required");
    assert_eq!(l.check_reason, "Redirects to a login page");

    assert!(
        lost_urls(&app, &did).await.is_empty(),
        "not in the lost list"
    );
    let resp = maria.get("/api/v1/dashboard").await;
    let dash: DashboardResponse = maria.json(resp).await;
    assert!(
        !dash.sections["overdue_results"]
            .iter()
            .any(|i| i.title.contains(&repo_base)),
        "login-required links are not flagged as lost"
    );
    for url in [&private, &forbidden, &sso] {
        assert_eq!(notifications_about(&app, url).await, 0);
    }
}

/// Regression: a redirect target whose name merely contains a login word
/// (`/cataloging.csv`) is followed, so the 404 behind it is reported missing
/// and alerted; only real sign-in targets count as login required.
#[tokio::test]
async fn redirect_login_detection_uses_auth_segments_not_substrings() {
    let app = live_app(true).await;
    let (anna, maria, did) = dataset_setup(&app).await;
    let (repo_base, _repo) = spawn_repo().await;
    let catalog = format!("{repo_base}/catalog-moved");
    let needs_login = format!("{repo_base}/needs-login");
    let federated = format!("{repo_base}/federated");

    let links = submit_and_check(
        &app,
        &anna,
        &maria,
        &did,
        &[&catalog, &needs_login, &federated],
    )
    .await;
    let l = by_url(&links, &catalog);
    assert_eq!(
        (l.check_status.as_str(), l.check_http_status),
        ("missing", Some(404))
    );
    for url in [&needs_login, &federated] {
        let l = by_url(&links, url);
        assert_eq!(l.check_status, "login_required", "{url}");
        assert_eq!(l.check_reason, "Redirects to a login page", "{url}");
    }

    assert_eq!(lost_urls(&app, &did).await, vec![catalog.clone()]);
    assert_eq!(notifications_about(&app, &catalog).await, 1);
    assert_eq!(notifications_about(&app, &needs_login).await, 0);
    assert_eq!(notifications_about(&app, &federated).await, 0);
}

#[tokio::test]
async fn live_check_unreachable_redirects_and_head_fallback() {
    let app = live_app(true).await;
    let (anna, maria, did) = dataset_setup(&app).await;
    let (repo_base, _repo) = spawn_repo().await;
    let refused = format!("http://127.0.0.1:{}/data.csv", closed_port().await);
    let broken = format!("{repo_base}/broken");
    let moved = format!("{repo_base}/moved");
    let no_head = format!("{repo_base}/no-head");
    // URL text no longer decides the result for ordinary URLs.
    let missing_in_text = format!("{repo_base}/missing/data.csv");

    let links = submit_and_check(
        &app,
        &anna,
        &maria,
        &did,
        &[&refused, &broken, &moved, &no_head, &missing_in_text],
    )
    .await;
    let l = by_url(&links, &refused);
    assert_eq!(l.check_status, "unreachable");
    assert_eq!(l.check_http_status, None);
    assert_eq!(l.check_reason, "Connection refused");
    let l = by_url(&links, &broken);
    assert_eq!(
        (l.check_status.as_str(), l.check_http_status),
        ("unreachable", Some(503))
    );
    assert_eq!(by_url(&links, &moved).check_status, "available");
    assert_eq!(by_url(&links, &no_head).check_status, "available");
    assert_eq!(by_url(&links, &missing_in_text).check_status, "available");

    let mut lost = lost_urls(&app, &did).await;
    lost.sort();
    let mut expected = vec![broken.clone(), refused.clone()];
    expected.sort();
    assert_eq!(lost, expected);
    assert_eq!(notifications_about(&app, &refused).await, 1);
    assert_eq!(notifications_about(&app, &broken).await, 1);
}

#[tokio::test]
async fn demo_hosts_keep_the_deterministic_mock_in_live_mode() {
    let app = live_app(false).await;
    let (anna, maria, did) = dataset_setup(&app).await;
    let good = "https://data.example.org/files";
    let bad_host = "https://repo.pitcairn.invalid/data";
    let missing = "https://example.org/missing/dataset";
    let restricted = "https://repository.example.org/restricted/tracks";

    let links = submit_and_check(
        &app,
        &anna,
        &maria,
        &did,
        &[good, bad_host, missing, restricted],
    )
    .await;
    assert_eq!(by_url(&links, good).check_status, "available");
    assert_eq!(by_url(&links, bad_host).check_status, "unreachable");
    assert_eq!(by_url(&links, missing).check_status, "missing");
    assert_eq!(by_url(&links, restricted).check_status, "login_required");
    assert!(
        links
            .iter()
            .all(|l| l.check_reason.ends_with("(demo check)"))
    );

    let mut lost = lost_urls(&app, &did).await;
    lost.sort();
    assert_eq!(lost, vec![missing.to_string(), bad_host.to_string()]);
    assert_eq!(notifications_about(&app, bad_host).await, 1);
    assert_eq!(notifications_about(&app, missing).await, 1);
    assert_eq!(notifications_about(&app, restricted).await, 0);

    // The seeded demo link stays deterministic too.
    let seeded: (String,) = sqlx::query_as(
        "SELECT check_status FROM external_links
         WHERE url = 'https://data.example.invalid/missing/henderson-seabird-photos'",
    )
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(seeded.0, "unreachable");
}

#[tokio::test]
async fn ssrf_guard_refuses_private_addresses_without_the_test_flag() {
    let app = live_app(false).await;
    let (anna, maria, did) = dataset_setup(&app).await;
    let (repo_base, repo) = spawn_repo().await;
    let port = repo_base.rsplit(':').next().unwrap();
    let loopback = format!("{repo_base}/data.csv");
    let by_name = format!("http://localhost:{port}/data.csv");

    let links = submit_and_check(&app, &anna, &maria, &did, &[&loopback, &by_name]).await;
    for url in [&loopback, &by_name] {
        let l = by_url(&links, url);
        assert_eq!(l.check_status, "unreachable", "{url}");
        assert!(
            l.check_reason.starts_with("Refused"),
            "{url}: {}",
            l.check_reason
        );
    }
    assert_eq!(repo.hits.load(Ordering::SeqCst), 0, "no request reached it");

    // Same guard on the checker itself, including IPv6 and metadata IPs.
    for url in [
        "http://[::1]/x",
        "http://169.254.169.254/latest/meta-data/",
        "http://10.0.0.1/",
        "http://[::ffff:127.0.0.1]/",
    ] {
        let outcome = linkcheck::check(url, linkcheck::MODE_LIVE, false).await;
        assert_eq!(outcome.status, LinkStatus::Unreachable, "{url}");
        assert!(outcome.reason.starts_with("Refused"), "{url}");
    }
}

#[tokio::test]
async fn archive_exported_before_status_migration_still_imports() {
    let app = spawn_app(true).await;
    let pid = project_id(&app.pool, pitcairn::seed::history::SEABIRDS).await;
    let export = pitcairn::archive::export_project(&app.pool, app._dir.path(), &pid)
        .await
        .unwrap();

    // Rewrite the link rows into the pre-0510 shape (boolean check).
    let mut entries = unzip(&export.bytes);
    let rows: Vec<serde_json::Value> =
        serde_json::from_slice(&entries["records/external_links.json"]).unwrap();
    assert!(!rows.is_empty(), "seeded project has external links");
    let old: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|mut r| {
            let o = r.as_object_mut().unwrap();
            let available = o["check_status"] == "available";
            for k in ["check_status", "check_http_status", "check_reason"] {
                o.remove(k);
            }
            o.insert("available".into(), serde_json::json!(available as i64));
            o.insert("last_status".into(), serde_json::json!("unavailable"));
            r
        })
        .collect();
    entries.insert(
        "records/external_links.json".into(),
        serde_json::to_vec(&old).unwrap(),
    );
    let named: Vec<(&str, &[u8])> = entries
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_slice()))
        .collect();
    let bytes = zip_with(&named);

    let fresh_dir = tempfile::TempDir::new().unwrap();
    let fresh = empty_install(fresh_dir.path()).await;
    pitcairn::archive::import_bytes(&fresh, fresh_dir.path(), &bytes, 2_147_483_648)
        .await
        .expect("old archive imports");
    let statuses: Vec<String> =
        sqlx::query_scalar("SELECT check_status FROM external_links ORDER BY check_status")
            .fetch_all(&fresh)
            .await
            .unwrap();
    assert_eq!(statuses, vec!["unreachable", "unreachable"]);
}
