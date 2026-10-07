//! Slice C: submissions — files/links, request-changes, resubmit, accept,
//! measurement CSV parsing and external-link checking.

mod common;

use common::c::*;
use common::{persona, spawn_app};

const ANNA: &str = "anna@demo.pitcairn.invalid";
const MARIA: &str = "maria@demo.pitcairn.invalid";

async fn dataset_setup(app: &common::TestApp) -> (common::Client, common::Client, String, String) {
    let anna = persona(app, "anna").await;
    let maria = persona(app, "maria").await;
    let project = seeded_project(app).await;
    let anna_id = user_id(app, ANNA).await;
    let maria_id = user_id(app, MARIA).await;
    let d = agreed_deliverable(&anna, &maria, &project, "dataset", &anna_id, &maria_id).await;
    (anna, maria, project, d.id)
}

#[tokio::test]
async fn submit_request_changes_resubmit_accept_keeps_both() {
    let app = spawn_app(true).await;
    let (anna, maria, project, did) = dataset_setup(&app).await;

    let file_id = upload_clean_file(&anna, &app, b"data one", "text/csv").await;
    let v1 = create_document(&anna, &project, "Results v1", "result", &file_id).await;

    // Submit #1.
    let s1 = submit(
        &anna,
        &did,
        serde_json::json!({
            "note": "first cut",
            "document_version_ids": [v1],
            "links": [{"url": "https://example.org/data", "description": "mirror", "version_label": "v1"}],
        }),
    )
    .await;
    assert_eq!(s1.number, 1);
    assert_eq!(s1.status, "received");

    // Recipient (Maria) was notified.
    let n: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM notifications n JOIN users u ON u.id = n.user_id
         WHERE u.email = ? AND n.kind = 'submission.received'",
    )
    .bind(MARIA)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(n.0, 1);

    // Maria requests changes → deliverable + submission changes_requested,
    // shared thread message + open action item on the deliverable anchor.
    let resp = maria
        .post_json(
            &format!("/api/v1/submissions/{}/request-changes", s1.id),
            &serde_json::json!({"note": "add a description of observation sites"}),
        )
        .await;
    assert_eq!(resp.status(), 200);
    let s1: pitcairn::dto::SubmissionDto = maria.json(resp).await;
    assert_eq!(s1.status, "changes_requested");
    assert_eq!(
        s1.review_note.as_deref(),
        Some("add a description of observation sites")
    );

    let msg: (String,) = sqlx::query_as(
        "SELECT m.body FROM messages m
         JOIN threads t ON t.id = m.thread_id
         WHERE t.anchor_type = 'deliverable' AND t.anchor_key = ? AND t.visibility = 'shared'",
    )
    .bind(&did)
    .fetch_one(&app.pool)
    .await
    .expect("shared thread message");
    assert_eq!(msg.0, "add a description of observation sites");

    let open_items: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM action_items ai
         JOIN threads t ON t.id = ai.thread_id
         WHERE t.anchor_key = ? AND ai.addressed_to = 'team' AND ai.status = 'open'",
    )
    .bind(&did)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(open_items.0, 1);

    // The workspace headline shows it to the team ("Maria asks: ...").
    let resp = anna.get(&format!("/api/v1/projects/{project}")).await;
    let ws: pitcairn::dto::ProjectWorkspaceDto = anna.json(resp).await;
    let pm = ws.primary_message.expect("primary message");
    assert_eq!(pm.title, "add a description of observation sites");
    assert_eq!(pm.by_name, "Maria Ellis");

    // Resubmit #2 with a corrected file.
    let file2 = upload_clean_file(&anna, &app, b"data two", "text/csv").await;
    let v2 = create_document(&anna, &project, "Results v2", "result", &file2).await;
    let s2 = submit(
        &anna,
        &did,
        serde_json::json!({
            "note": "added site descriptions",
            "document_version_ids": [v2],
        }),
    )
    .await;
    assert_eq!(s2.number, 2);
    assert_eq!(s2.status, "received");

    // Accept #2 → deliverable accepted, both submissions kept, action item
    // resolved, team notified.
    let resp = maria
        .post(&format!("/api/v1/submissions/{}/accept", s2.id))
        .await;
    assert_eq!(resp.status(), 200);
    let accept: pitcairn::dto::AcceptSubmissionResponse = maria.json(resp).await;
    assert_eq!(accept.submission.status, "accepted");
    assert!(
        accept.message.contains("receipt"),
        "acceptance must say it marks receipt only"
    );

    let statuses: Vec<(i64, String)> = sqlx::query_as(
        "SELECT number, status FROM deliverable_submissions WHERE deliverable_id = ? ORDER BY number",
    )
    .bind(&did)
    .fetch_all(&app.pool)
    .await
    .unwrap();
    assert_eq!(
        statuses,
        vec![
            (1, "changes_requested".to_string()),
            (2, "accepted".to_string())
        ]
    );
    let d: (String,) = sqlx::query_as("SELECT status FROM deliverables WHERE id = ?")
        .bind(&did)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(d.0, "accepted");

    let resolved: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM action_items ai
         JOIN threads t ON t.id = ai.thread_id
         WHERE t.anchor_key = ? AND ai.status = 'resolved' AND ai.resolved_by IS NOT NULL",
    )
    .bind(&did)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(resolved.0, 1);

    // Workspace shows latest + accepted submission (both kept per §4).
    let resp = anna.get(&format!("/api/v1/projects/{project}")).await;
    let ws: pitcairn::dto::ProjectWorkspaceDto = anna.json(resp).await;
    let d = &ws.results.deliverables[0];
    assert_eq!(d.latest_submission.as_ref().unwrap().number, 2);
    assert_eq!(d.accepted_submission.as_ref().unwrap().number, 2);
}

#[tokio::test]
async fn link_only_submission_never_auto_accepts() {
    let app = spawn_app(true).await;
    let (anna, maria, _project, did) = dataset_setup(&app).await;

    let s = submit(
        &anna,
        &did,
        serde_json::json!({
            "links": [{"url": "https://zenodo.org/record/1", "description": "dataset", "version_label": "1.0"}],
        }),
    )
    .await;
    assert_eq!(s.status, "received");
    assert_eq!(s.links.len(), 1);

    // Runs the queued check_link job too; deliverable stays `submitted` —
    // a link alone never auto-accepts.
    run_jobs(&app).await;
    let d: (String,) = sqlx::query_as("SELECT status FROM deliverables WHERE id = ?")
        .bind(&did)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(d.0, "submitted");

    // Manual acceptance still works.
    let resp = maria
        .post(&format!("/api/v1/submissions/{}/accept", s.id))
        .await;
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn submission_requires_agreed_or_changes_requested_status() {
    let app = spawn_app(true).await;
    let anna = persona(&app, "anna").await;
    let project = seeded_project(&app).await;
    let anna_id = user_id(&app, ANNA).await;
    let maria_id = user_id(&app, MARIA).await;

    // Proposed (not yet agreed) → 409.
    let d = create_deliverable(&anna, &project, "report", "2030-01-01", &anna_id, &maria_id).await;
    let resp = anna
        .post_json(
            &format!("/api/v1/deliverables/{}/submissions", d.id),
            &serde_json::json!({"links": [{"url": "https://x.org", "description": "d", "version_label": "1"}]}),
        )
        .await;
    assert_eq!(resp.status(), 409);

    // Coordinators may not submit on the team's behalf.
    let maria = persona(&app, "maria").await;
    let d2 = agreed_deliverable(&anna, &maria, &project, "report", &anna_id, &maria_id).await;
    let resp = maria
        .post_json(
            &format!("/api/v1/deliverables/{}/submissions", d2.id),
            &serde_json::json!({"links": [{"url": "https://x.org", "description": "d", "version_label": "1"}]}),
        )
        .await;
    assert_eq!(resp.status(), 403);
}

#[tokio::test]
async fn submission_file_rules_enforced() {
    let app = spawn_app(true).await;
    let (anna, _maria, project, did) = dataset_setup(&app).await;
    let lukas = persona(&app, "lukas").await;

    // Another project's document version → 403 (never reveals existence).
    let other_project = create_project(&lukas, "Lukas project").await;
    let lf = upload_clean_file(&lukas, &app, b"foreign", "text/csv").await;
    let foreign_v = create_document(&lukas, &other_project, "Foreign", "result", &lf).await;
    let resp = anna
        .post_json(
            &format!("/api/v1/deliverables/{did}/submissions"),
            &serde_json::json!({"document_version_ids": [foreign_v]}),
        )
        .await;
    assert_eq!(resp.status(), 403);

    // Nonexistent version → 404.
    let resp = anna
        .post_json(
            &format!("/api/v1/deliverables/{did}/submissions"),
            &serde_json::json!({"document_version_ids": ["no-such-version"]}),
        )
        .await;
    assert_eq!(resp.status(), 404);

    // Same project but wrong category → 422.
    let af = upload_clean_file(&anna, &app, b"app doc", "text/plain").await;
    let app_v = create_document(
        &anna,
        &project,
        "Application attachment",
        "application",
        &af,
    )
    .await;
    let resp = anna
        .post_json(
            &format!("/api/v1/deliverables/{did}/submissions"),
            &serde_json::json!({"document_version_ids": [app_v]}),
        )
        .await;
    assert_eq!(resp.status(), 422);
    let err: serde_json::Value = anna.json(resp).await;
    assert!(err["error"]["fields"]["document_version_ids"].is_string());

    // Scan not clean yet → 422.
    let pending_f = upload_file(&anna, b"unscanned", "text/csv").await;
    let pending_v = create_document(&anna, &project, "Pending", "result", &pending_f).await;
    let resp = anna
        .post_json(
            &format!("/api/v1/deliverables/{did}/submissions"),
            &serde_json::json!({"document_version_ids": [pending_v]}),
        )
        .await;
    assert_eq!(resp.status(), 422);

    // Link without description/version_label → 422 field errors.
    let resp = anna
        .post_json(
            &format!("/api/v1/deliverables/{did}/submissions"),
            &serde_json::json!({"links": [{"url": "https://x.org", "description": "", "version_label": ""}]}),
        )
        .await;
    assert_eq!(resp.status(), 422);
    let err: serde_json::Value = anna.json(resp).await;
    assert!(err["error"]["fields"]["links[0].description"].is_string());
    assert!(err["error"]["fields"]["links[0].version_label"].is_string());

    // Non-http link → 422. Empty submission → 422.
    let resp = anna
        .post_json(
            &format!("/api/v1/deliverables/{did}/submissions"),
            &serde_json::json!({"links": [{"url": "ftp://x.org", "description": "d", "version_label": "1"}]}),
        )
        .await;
    assert_eq!(resp.status(), 422);
    let resp = anna
        .post_json(
            &format!("/api/v1/deliverables/{did}/submissions"),
            &serde_json::json!({}),
        )
        .await;
    assert_eq!(resp.status(), 422);
}

#[tokio::test]
async fn measurement_csv_parsed_on_accept_with_row_warnings() {
    let app = spawn_app(true).await;
    let (anna, maria, project, did) = dataset_setup(&app).await;

    let csv_bytes = b"site,date,variable,value,unit\n\
                      Bounty Bay,2030-02-01,temperature,18.4,degC\n\
                      Tedside,2030-02-02,temperature,17.9,degC\n\
                      Tedside,not-a-date,salinity,35.1,psu\n\
                      ,2030-02-03,depth,12,m\n";
    let file_id = upload_clean_file(&anna, &app, csv_bytes, "text/csv").await;
    let v = create_document(&anna, &project, "CTD casts", "result", &file_id).await;

    // A non-measurement CSV alongside: kept as a file, not parsed.
    let other = upload_clean_file(&anna, &app, b"a,b,c\n1,2,3\n", "text/csv").await;
    let v2 = create_document(&anna, &project, "Random table", "result", &other).await;

    let s = submit(
        &anna,
        &did,
        serde_json::json!({
            "note": "CTD data",
            "document_version_ids": [v, v2],
            "data_dictionary": [{"column": "temperature", "description": "SST", "unit": "degC", "method": "CTD"}],
        }),
    )
    .await;
    assert_eq!(s.data_dictionary.len(), 1);

    let resp = maria
        .post(&format!("/api/v1/submissions/{}/accept", s.id))
        .await;
    assert_eq!(resp.status(), 200);
    let accept: pitcairn::dto::AcceptSubmissionResponse = maria.json(resp).await;
    assert_eq!(
        accept.warnings.len(),
        2,
        "bad date + empty site → 2 warnings"
    );
    assert!(accept.warnings.iter().any(|w| w.contains("date")));
    assert!(accept.warnings.iter().any(|w| w.contains("site")));

    let rows: Vec<(String, String, String, f64, String, String)> = sqlx::query_as(
        "SELECT site_name, observed_on, variable_key, value, unit, source_label
         FROM measurements WHERE submission_id = ? ORDER BY observed_on",
    )
    .bind(&s.id)
    .fetch_all(&app.pool)
    .await
    .unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].0, "Bounty Bay");
    assert_eq!(rows[0].1, "2030-02-01");
    assert_eq!(rows[0].2, "temperature");
    assert!((rows[0].3 - 18.4).abs() < 1e-9);
    assert_eq!(rows[0].4, "degC");
    assert!(rows[0].5.contains("Final report (submission #1)"));
}

#[tokio::test]
async fn check_link_job_and_manual_check_flag_unavailable() {
    let app = spawn_app(true).await;
    let (anna, maria, _project, did) = dataset_setup(&app).await;

    let s = submit(
        &anna,
        &did,
        serde_json::json!({
            "links": [
                {"url": "https://data.example.org/files", "description": "good", "version_label": "1"},
                {"url": "https://repo.pitcairn.invalid/data", "description": "bad host", "version_label": "1"},
                {"url": "https://example.org/missing/dataset", "description": "missing path", "version_label": "1"},
            ],
        }),
    )
    .await;
    run_jobs(&app).await;

    let links: Vec<(String, String)> = sqlx::query_as(
        "SELECT url, last_status FROM external_links WHERE submission_id = ? ORDER BY url",
    )
    .bind(&s.id)
    .fetch_all(&app.pool)
    .await
    .unwrap();
    assert_eq!(
        links,
        vec![
            (
                "https://data.example.org/files".to_string(),
                "available".to_string()
            ),
            (
                "https://example.org/missing/dataset".to_string(),
                "unavailable".to_string()
            ),
            (
                "https://repo.pitcairn.invalid/data".to_string(),
                "unavailable".to_string()
            ),
        ]
    );

    // The coordinator dashboard query exposes the two unavailable ones.
    // Scoped to this deliverable: the demo seed has its own unavailable link.
    let bad: Vec<_> = pitcairn::deliverables::unavailable_links(&app.pool)
        .await
        .unwrap()
        .into_iter()
        .filter(|l| l.deliverable_id == did)
        .collect();
    assert_eq!(bad.len(), 2);
    assert!(bad.iter().any(|l| l.url.contains(".invalid")));

    // Maria was notified about the newly-unavailable links.
    let n: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM notifications n JOIN users u ON u.id = n.user_id
         WHERE u.email = ? AND n.kind = 'link.unavailable'",
    )
    .bind(MARIA)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(n.0, 2);

    // Manual re-check endpoint (coordinator) works and returns fresh status.
    let link_id: (String,) =
        sqlx::query_as("SELECT id FROM external_links WHERE url LIKE '%missing%' LIMIT 1")
            .fetch_one(&app.pool)
            .await
            .unwrap();
    let resp = maria
        .post(&format!("/api/v1/external-links/{}/check", link_id.0))
        .await;
    assert_eq!(resp.status(), 200);
    let link: pitcairn::dto::ExternalLinkDto = maria.json(resp).await;
    assert_eq!(link.last_status.as_deref(), Some("unavailable"));
    assert_eq!(link.available, Some(false));
    assert!(link.last_checked_at.is_some());

    // Non-coordinators cannot trigger checks.
    let resp = anna
        .post(&format!("/api/v1/external-links/{}/check", link_id.0))
        .await;
    assert_eq!(resp.status(), 403);
}
