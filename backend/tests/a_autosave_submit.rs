mod common;

use common::a::*;
use common::{persona, spawn_app};
use serde_json::json;

#[tokio::test]
async fn concurrent_autosaves_with_same_version_one_gets_stale_version() {
    let app = spawn_app(true).await;
    let anna = persona(&app, "anna").await;
    let liam = persona(&app, "liam").await;
    // Anna (lead) and Liam (editor) share the seeded draft.
    let pid: String =
        sqlx::query_scalar("SELECT id FROM projects WHERE title = 'Coral health around Pitcairn'")
            .fetch_one(&app.pool)
            .await
            .unwrap();
    let version = project_version(&anna, &pid).await;

    let path = format!("/projects/{pid}");
    let (a, b) = tokio::join!(
        patch(
            &anna,
            &path,
            json!({"version": version, "summary": "from anna"})
        ),
        patch(
            &liam,
            &path,
            json!({"version": version, "summary": "from liam"})
        ),
    );
    let mut statuses = [a.0, b.0];
    statuses.sort();
    assert_eq!(statuses, [200, 409], "anna {a:?} liam {b:?}");
    let loser = if a.0 == 409 { &a.1 } else { &b.1 };
    assert_eq!(loser["error"]["code"], "stale_version");
    assert_eq!(project_version(&anna, &pid).await, version + 1);

    // A later save with the old version is stale too; with the new one it works.
    let (status, err) = patch(&anna, &path, json!({"version": version, "title": "x"})).await;
    assert_eq!(
        (status, err["error"]["code"].as_str()),
        (409, Some("stale_version"))
    );
    let (status, ok) = patch(
        &anna,
        &path,
        json!({"version": version + 1, "start_date": null}),
    )
    .await;
    assert_eq!(status, 200, "{ok}");
}

#[tokio::test]
async fn autosave_validates_answers_and_editability() {
    let app = spawn_app(true).await;
    let anna = persona(&app, "anna").await;
    let tomasi = persona(&app, "tomasi").await;
    let maria = persona(&app, "maria").await;
    let pid = fieldwork_draft(&anna, "Validation").await;
    let path = format!("/projects/{pid}");
    let v = project_version(&anna, &pid).await;

    let (status, err) = patch(
        &anna,
        &path,
        json!({"version": v, "answers": {"team_size": "three", "nope": 1,
               "activities": ["flying"], "sites": ["00000000-0000-0000-0000-000000000000"]}}),
    )
    .await;
    assert_eq!(status, 422);
    let fields = &err["error"]["fields"];
    for f in [
        "answers.team_size",
        "answers.nope",
        "answers.activities",
        "answers.sites",
    ] {
        assert!(fields[f].is_string(), "missing {f}: {err}");
    }
    // Viewer (tomasi is viewer on Anna's seeded project, not on this one) and staff cannot edit.
    let (status, _) = patch(&tomasi, &path, json!({"version": v, "summary": "x"})).await;
    assert_eq!(status, 403);
    let (status, _) = patch(&maria, &path, json!({"version": v, "summary": "x"})).await;
    assert_eq!(status, 403);

    let (status, _) = submit(&anna, &pid, None).await;
    assert_eq!(status, 200);
    let v = project_version(&anna, &pid).await;
    let (status, err) = patch(&anna, &path, json!({"version": v, "summary": "late"})).await;
    assert_eq!(
        (status, err["error"]["code"].as_str()),
        (409, Some("not_editable"))
    );
}

/// The application form only sends answers; its date-range answer must
/// become the project's period, otherwise a new study is invisible to the
/// search-by-year filter (spec §5 item 14).
#[tokio::test]
async fn date_range_answer_sets_project_dates_and_year_search_finds_it() {
    let app = spawn_app(true).await;
    let anna = persona(&app, "anna").await;
    let maria = persona(&app, "maria").await;
    let pid = create_project(&anna, "fieldwork_permit", "Dates from the form").await;
    let v = project_version(&anna, &pid).await;
    let (status, saved) = patch(
        &anna,
        &format!("/projects/{pid}"),
        json!({"version": v, "answers": {"dates": {"start": "2031-02-03", "end": "2031-03-04"}}}),
    )
    .await;
    assert_eq!(status, 200, "{saved}");
    let (_, ws) = get(&anna, &format!("/projects/{pid}")).await;
    assert_eq!(ws["project"]["start_date"], "2031-02-03");
    assert_eq!(ws["project"]["end_date"], "2031-03-04");

    let (status, found) = get(&maria, "/search/projects?year=2031").await;
    assert_eq!(status, 200, "{found}");
    assert!(
        found["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["id"] == pid.as_str()),
        "{found}"
    );

    // An end before the start is rejected, not stored.
    let v = project_version(&anna, &pid).await;
    let (status, _) = patch(
        &anna,
        &format!("/projects/{pid}"),
        json!({"version": v, "answers": {"dates": {"start": "2031-05-01", "end": "2031-04-01"}}}),
    )
    .await;
    assert_eq!(status, 422);

    // Clearing the answer clears the project's period too.
    let v = project_version(&anna, &pid).await;
    let (status, _) = patch(
        &anna,
        &format!("/projects/{pid}"),
        json!({"version": v, "answers": {"dates": null}}),
    )
    .await;
    assert_eq!(status, 200);
    let (_, ws) = get(&anna, &format!("/projects/{pid}")).await;
    assert!(ws["project"]["start_date"].is_null(), "{ws}");
    assert!(ws["project"]["end_date"].is_null(), "{ws}");
}

#[tokio::test]
async fn idempotent_double_submit_creates_one_revision() {
    let app = spawn_app(true).await;
    let anna = persona(&app, "anna").await;
    let pid = fieldwork_draft(&anna, "Idempotent submit").await;

    let (s1, r1) = submit(&anna, &pid, Some("key-123")).await;
    let (s2, r2) = submit(&anna, &pid, Some("key-123")).await;
    assert_eq!((s1, s2), (200, 200), "{r1} / {r2}");
    assert_eq!(r1, r2, "replayed response is identical");
    let revisions: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM project_revisions WHERE project_id = ?")
            .bind(&pid)
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(revisions, 1);
    let refs: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM projects WHERE reference = ?")
        .bind(r1["reference"].as_str().unwrap())
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(refs, 1);

    // Same key, different body → 422; a fresh key on a submitted project → 409.
    let resp = anna
        .request(
            reqwest::Method::POST,
            &format!("/api/v1/projects/{pid}/submit"),
        )
        .header("Idempotency-Key", "key-123")
        .json(&json!({"different": true}))
        .send()
        .await
        .unwrap();
    let (status, err) = body(resp).await;
    assert_eq!(
        (status, err["error"]["code"].as_str()),
        (422, Some("idempotency_key_reused"))
    );
    let (status, err) = submit(&anna, &pid, Some("key-456")).await;
    assert_eq!(
        (status, err["error"]["code"].as_str()),
        (409, Some("invalid_transition"))
    );

    // Viewer cannot submit.
    let tomasi = persona(&app, "tomasi").await;
    let seeded: String =
        sqlx::query_scalar("SELECT id FROM projects WHERE title = 'Coral health around Pitcairn'")
            .fetch_one(&app.pool)
            .await
            .unwrap();
    let (status, _) = submit(&tomasi, &seeded, None).await;
    assert_eq!(status, 403);
}
