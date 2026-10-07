//! Slice C: submissions — files/links, request-changes, resubmit, accept,
//! corrected versions of accepted results and measurement CSV parsing
//! (external-link checks: `c_link_check.rs`).

mod common;

use common::c::*;
use common::{persona, spawn_app};

const ANNA: &str = "anna@demo.pitcairn.invalid";
const MARIA: &str = "maria@demo.pitcairn.invalid";

#[tokio::test]
async fn researcher_can_choose_active_coordinator_for_first_deliverable() {
    let app = spawn_app(true).await;
    let anna = persona(&app, "anna").await;
    let response = anna.get("/api/v1/coordinators").await;
    assert_eq!(response.status(), 200);
    let list: pitcairn::dto::ListResponse<pitcairn::dto::CoordinatorDto> =
        anna.json(response).await;
    assert!(list.items.iter().any(|user| user.name == "Maria Ellis"));
}

#[tokio::test]
async fn submission_history_is_visible_to_project_team() {
    let app = spawn_app(true).await;
    let (anna, maria, _project, did) = dataset_setup(&app).await;
    let first = submit(&anna, &did, serde_json::json!({
        "links": [{"url": "https://example.org/first", "description": "First", "version_label": "v1"}]
    })).await;
    let response = maria
        .post_json(
            &format!("/api/v1/submissions/{}/request-changes", first.id),
            &serde_json::json!({"note": "Add a method note"}),
        )
        .await;
    assert_eq!(response.status(), 200);
    let second = submit(&anna, &did, serde_json::json!({
        "links": [{"url": "https://example.org/second", "description": "Second", "version_label": "v2"}]
    })).await;
    let response = anna
        .get(&format!("/api/v1/deliverables/{did}/submissions"))
        .await;
    assert_eq!(response.status(), 200);
    let history: pitcairn::dto::ListResponse<pitcairn::dto::SubmissionDto> =
        anna.json(response).await;
    assert_eq!(history.total, 2);
    assert_eq!(history.items[0].id, second.id);
    assert_eq!(
        history.items[1].review_note.as_deref(),
        Some("Add a method note")
    );
}

#[tokio::test]
async fn only_coordinator_can_read_publication_file_selection() {
    let app = spawn_app(true).await;
    let (anna, maria, _project, did) = dataset_setup(&app).await;
    let response = anna
        .get(&format!("/api/v1/deliverables/{did}/publication-files"))
        .await;
    assert_eq!(response.status(), 403);
    let response = maria
        .get(&format!("/api/v1/deliverables/{did}/publication-files"))
        .await;
    assert_eq!(response.status(), 200);
    let files: pitcairn::dto::PublicationFilesResponse = maria.json(response).await;
    assert!(files.document_version_ids.is_empty());
}

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
async fn submission_refused_before_agreement_and_for_coordinators() {
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

async fn deliverable(
    client: &common::Client,
    project: &str,
    did: &str,
) -> pitcairn::dto::DeliverableDto {
    let resp = client
        .get(&format!("/api/v1/projects/{project}/deliverables"))
        .await;
    assert_eq!(resp.status(), 200);
    let list: pitcairn::dto::ListResponse<pitcairn::dto::DeliverableDto> = client.json(resp).await;
    list.items
        .into_iter()
        .find(|d| d.id == did)
        .expect("deliverable listed")
}

async fn download(client: &common::Client, version_id: &str) -> Vec<u8> {
    let resp = client
        .get(&format!("/api/v1/document-versions/{version_id}/download"))
        .await;
    assert_eq!(resp.status(), 200, "accepted file stays downloadable");
    resp.bytes().await.unwrap().to_vec()
}

async fn measurement_values(app: &common::TestApp, did: &str) -> Vec<(String, f64)> {
    sqlx::query_as(
        "SELECT site_name, value FROM measurements WHERE deliverable_id = ? ORDER BY site_name",
    )
    .bind(did)
    .fetch_all(&app.pool)
    .await
    .unwrap()
}

/// §5 item 10: a corrected table is stored as a new version; the earlier
/// accepted one never disappears and stays in force until the correction is
/// accepted.
#[tokio::test]
async fn corrected_version_keeps_earlier_accepted_until_accepted() {
    let app = spawn_app(true).await;
    let (anna, maria, project, did) = dataset_setup(&app).await;

    // #1 accepted, its file selected for publication.
    let csv1: &[u8] =
        b"site,date,variable,value,unit\nBounty Bay,2030-02-01,temperature,18.4,degC\n";
    let f1 = upload_clean_file(&anna, &app, csv1, "text/csv").await;
    let v1 = create_document(&anna, &project, "CTD casts v1", "result", &f1).await;
    let s1 = submit(
        &anna,
        &did,
        serde_json::json!({"document_version_ids": [v1]}),
    )
    .await;
    let resp = maria
        .post(&format!("/api/v1/submissions/{}/accept", s1.id))
        .await;
    assert_eq!(resp.status(), 200);
    let resp = maria
        .put_json(
            &format!("/api/v1/deliverables/{did}/publication-files"),
            &serde_json::json!({"document_version_ids": [v1]}),
        )
        .await;
    assert_eq!(resp.status(), 200);
    assert_eq!(
        measurement_values(&app, &did).await,
        vec![("Bounty Bay".to_string(), 18.4)]
    );

    // The team submits corrected #2 on the accepted deliverable.
    let csv2: &[u8] =
        b"site,date,variable,value,unit\nBounty Bay,2030-02-01,temperature,18.9,degC\n";
    let f2 = upload_clean_file(&anna, &app, csv2, "text/csv").await;
    let v2 = create_document(&anna, &project, "CTD casts v2", "result", &f2).await;
    let s2 = submit(
        &anna,
        &did,
        serde_json::json!({"note": "fixed calibration", "document_version_ids": [v2]}),
    )
    .await;
    assert_eq!(s2.number, 2);
    assert_eq!(s2.status, "received");

    // #1 is still the accepted version: deliverable stays accepted, shows the
    // correction under review, #1 downloadable, published files unchanged,
    // measurements untouched.
    let d = deliverable(&anna, &project, &did).await;
    assert_eq!(d.status, "accepted");
    assert_eq!(d.correction_status.as_deref(), Some("under_review"));
    let accepted = d.accepted_submission.expect("accepted submission");
    assert_eq!(accepted.number, 1);
    assert_eq!(accepted.status, "accepted");
    assert_eq!(d.latest_submission.expect("latest").number, 2);
    assert_eq!(download(&anna, &v1).await, csv1);
    let resp = maria
        .get(&format!("/api/v1/deliverables/{did}/publication-files"))
        .await;
    let files: pitcairn::dto::PublicationFilesResponse = maria.json(resp).await;
    assert_eq!(files.document_version_ids, vec![v1.clone()]);
    assert_eq!(
        measurement_values(&app, &did).await,
        vec![("Bounty Bay".to_string(), 18.4)]
    );

    // Only one correction under review at a time.
    let resp = anna
        .post_json(
            &format!("/api/v1/deliverables/{did}/submissions"),
            &serde_json::json!({"document_version_ids": [v2]}),
        )
        .await;
    assert_eq!(resp.status(), 409);

    // Changes requested on the correction never un-accept #1.
    let resp = maria
        .post_json(
            &format!("/api/v1/submissions/{}/request-changes", s2.id),
            &serde_json::json!({"note": "units of the second cast are wrong"}),
        )
        .await;
    assert_eq!(resp.status(), 200);
    let d = deliverable(&anna, &project, &did).await;
    assert_eq!(d.status, "accepted");
    assert_eq!(d.correction_status.as_deref(), Some("changes_requested"));
    assert_eq!(d.accepted_submission.expect("accepted").number, 1);
    assert_eq!(
        measurement_values(&app, &did).await,
        vec![("Bounty Bay".to_string(), 18.4)]
    );

    // Resubmit #3 and accept it: #3 becomes the accepted submission and its
    // table replaces the measurement rows.
    let csv3: &[u8] =
        b"site,date,variable,value,unit\nBounty Bay,2030-02-01,temperature,18.7,degC\n";
    let f3 = upload_clean_file(&anna, &app, csv3, "text/csv").await;
    let v3 = create_document(&anna, &project, "CTD casts v3", "result", &f3).await;
    let s3 = submit(
        &anna,
        &did,
        serde_json::json!({"document_version_ids": [v3]}),
    )
    .await;
    assert_eq!(s3.number, 3);
    let resp = maria
        .post(&format!("/api/v1/submissions/{}/accept", s3.id))
        .await;
    assert_eq!(resp.status(), 200);

    let d = deliverable(&anna, &project, &did).await;
    assert_eq!(d.status, "accepted");
    assert_eq!(d.correction_status, None);
    let accepted = d.accepted_submission.expect("accepted");
    assert_eq!(accepted.number, 3);
    assert_eq!(accepted.files[0].document_version_id, v3);
    assert_eq!(
        measurement_values(&app, &did).await,
        vec![("Bounty Bay".to_string(), 18.7)]
    );

    // History lists every version with its status; #1 is kept (superseded)
    // and still downloadable.
    let resp = anna
        .get(&format!("/api/v1/deliverables/{did}/submissions"))
        .await;
    let history: pitcairn::dto::ListResponse<pitcairn::dto::SubmissionDto> = anna.json(resp).await;
    assert_eq!(history.total, 3);
    let statuses: Vec<(i64, &str)> = history
        .items
        .iter()
        .map(|s| (s.number, s.status.as_str()))
        .collect();
    assert_eq!(
        statuses,
        vec![(3, "accepted"), (2, "changes_requested"), (1, "superseded")]
    );
    assert_eq!(download(&anna, &v1).await, csv1);

    // An older version can no longer displace the accepted one.
    let resp = maria
        .post(&format!("/api/v1/submissions/{}/accept", s2.id))
        .await;
    assert_eq!(resp.status(), 409);
}

/// Accept #1 (one temperature row with `value`) and submit a correction with
/// `correction_value`; returns (anna, maria, project, deliverable, #2 id).
async fn accepted_with_correction(
    app: &common::TestApp,
    value: &str,
    correction_value: &str,
) -> (common::Client, common::Client, String, String, String) {
    let (anna, maria, project, did) = dataset_setup(app).await;
    let mut ids = Vec::new();
    for (i, v) in [value, correction_value].into_iter().enumerate() {
        let csv =
            format!("site,date,variable,value,unit\nBounty Bay,2030-02-01,temperature,{v},degC\n");
        let f = upload_clean_file(&anna, app, csv.as_bytes(), "text/csv").await;
        let dv = create_document(&anna, &project, &format!("CTD {i}"), "result", &f).await;
        let s = submit(
            &anna,
            &did,
            serde_json::json!({"document_version_ids": [dv]}),
        )
        .await;
        if i == 0 {
            let resp = maria
                .post(&format!("/api/v1/submissions/{}/accept", s.id))
                .await;
            assert_eq!(resp.status(), 200);
        }
        ids.push(s.id);
    }
    let correction = ids.pop().unwrap();
    (anna, maria, project, did, correction)
}

/// The measurement rows always come from the accepted submission.
async fn assert_measurements_mirror_accepted(app: &common::TestApp, accepted_id: &str, did: &str) {
    let sources: Vec<(String,)> =
        sqlx::query_as("SELECT DISTINCT submission_id FROM measurements WHERE deliverable_id = ?")
            .bind(did)
            .fetch_all(&app.pool)
            .await
            .unwrap();
    assert_eq!(sources, vec![(accepted_id.to_string(),)]);
}

/// Review decisions are validated inside the write transaction: once a
/// correction is accepted, a late request-changes on it is refused (409) and
/// cannot leave the accepted submission and the measurements disagreeing.
#[tokio::test]
async fn review_after_accept_is_refused_and_state_stays_consistent() {
    let app = spawn_app(true).await;
    let (anna, maria, project, did, s2) = accepted_with_correction(&app, "18.4", "18.9").await;

    // Accept #2, then request changes on #2 → 409.
    let resp = maria
        .post(&format!("/api/v1/submissions/{s2}/accept"))
        .await;
    assert_eq!(resp.status(), 200);
    let resp = maria
        .post_json(
            &format!("/api/v1/submissions/{s2}/request-changes"),
            &serde_json::json!({"note": "too late"}),
        )
        .await;
    assert_eq!(resp.status(), 409);
    let d = deliverable(&anna, &project, &did).await;
    assert_eq!(d.status, "accepted");
    assert_eq!(d.correction_status, None);
    assert_eq!(d.accepted_submission.as_ref().unwrap().id, s2);
    assert_eq!(
        measurement_values(&app, &did).await,
        vec![("Bounty Bay".to_string(), 18.9)]
    );
    assert_measurements_mirror_accepted(&app, &s2, &did).await;

    // A repeated accept is refused too.
    let resp = maria
        .post(&format!("/api/v1/submissions/{s2}/accept"))
        .await;
    assert_eq!(resp.status(), 409);
}

/// The other order: changes requested on a correction first; a repeated
/// request-changes is refused (409), #1 stays accepted with its measurements,
/// and a later explicit acceptance of the returned correction moves the
/// accepted submission and the measurements together.
#[tokio::test]
async fn review_after_request_changes_keeps_state_consistent() {
    let app = spawn_app(true).await;
    let (anna, maria, project, did, s2) = accepted_with_correction(&app, "18.4", "18.9").await;

    let resp = maria
        .post_json(
            &format!("/api/v1/submissions/{s2}/request-changes"),
            &serde_json::json!({"note": "check the calibration"}),
        )
        .await;
    assert_eq!(resp.status(), 200);
    let resp = maria
        .post_json(
            &format!("/api/v1/submissions/{s2}/request-changes"),
            &serde_json::json!({"note": "again"}),
        )
        .await;
    assert_eq!(resp.status(), 409);
    let d = deliverable(&anna, &project, &did).await;
    let s1 = d.accepted_submission.as_ref().unwrap().id.clone();
    assert_ne!(s1, s2);
    assert_eq!(d.status, "accepted");
    assert_eq!(d.correction_status.as_deref(), Some("changes_requested"));
    assert_measurements_mirror_accepted(&app, &s1, &did).await;

    let resp = maria
        .post(&format!("/api/v1/submissions/{s2}/accept"))
        .await;
    assert_eq!(resp.status(), 200);
    let d = deliverable(&anna, &project, &did).await;
    assert_eq!(d.accepted_submission.as_ref().unwrap().id, s2);
    assert_eq!(d.correction_status, None);
    assert_measurements_mirror_accepted(&app, &s2, &did).await;
}

/// Concurrent accept and request-changes on one correction: a held write lock
/// lets both requests pass their pre-transaction reads (both see `received`)
/// before either can write. Whichever order the writers then serialise in,
/// the accepted submission and the measurement rows never disagree.
#[tokio::test]
async fn concurrent_accept_and_request_changes_stay_consistent() {
    let app = spawn_app(true).await;
    let (anna, maria, project, did, s2) = accepted_with_correction(&app, "18.4", "18.9").await;

    let accept_path = format!("/api/v1/submissions/{s2}/accept");
    let changes_path = format!("/api/v1/submissions/{s2}/request-changes");
    let note = serde_json::json!({"note": "racing review"});
    let lock = pitcairn::db::begin_immediate(&app.pool).await.unwrap();
    let release = async {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        lock.commit().await.unwrap();
    };
    let (accept, changes, ()) = tokio::join!(
        maria.post(&accept_path),
        maria.post_json(&changes_path, &note),
        release
    );
    let (accept, changes) = (accept.status().as_u16(), changes.status().as_u16());
    // Accept-first → request-changes 409; request-changes-first → the
    // returned correction may still be accepted explicitly (both 200).
    assert_eq!(accept, 200, "accept never loses: {accept}/{changes}");
    assert!(matches!(changes, 200 | 409), "{changes}");

    let d = deliverable(&anna, &project, &did).await;
    let accepted = d.accepted_submission.expect("accepted");
    assert_eq!(accepted.id, s2);
    assert_eq!(accepted.status, "accepted");
    assert_eq!(d.status, "accepted");
    assert_eq!(d.correction_status, None);
    assert_measurements_mirror_accepted(&app, &s2, &did).await;
}
