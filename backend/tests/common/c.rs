//! Slice C test helpers: file+document plumbing for submissions, deliverable
//! builders and project-state shortcuts.

#![allow(dead_code)]

use super::{Client, TestApp};

pub fn sha256(bytes: &[u8]) -> String {
    use sha2::Digest;
    hex::encode(sha2::Sha256::new().chain_update(bytes).finalize())
}

/// Upload `bytes` through the chunked API, complete the upload and run the
/// job queue until the scan marks it `clean`. Returns `file_id`.
pub async fn upload_clean_file(client: &Client, app: &TestApp, bytes: &[u8], mime: &str) -> String {
    let file_id = upload_file(client, bytes, mime).await;
    while pitcairn::jobs::run_once(&app.state).await.expect("run job") {}
    let status: (String,) = sqlx::query_as("SELECT scan_status FROM files WHERE id = ?")
        .bind(&file_id)
        .fetch_one(&app.pool)
        .await
        .expect("file row");
    assert_eq!(status.0, "clean", "uploaded file should be scan-clean");
    file_id
}

/// Upload without running the scan job — file stays `pending`.
pub async fn upload_file(client: &Client, bytes: &[u8], mime: &str) -> String {
    let req = pitcairn::dto::CreateUploadRequest {
        filename: "test-file".into(),
        size: bytes.len() as u64,
        sha256: sha256(bytes),
        mime: mime.into(),
    };
    let resp = client.post_json("/api/v1/uploads", &req).await;
    assert_eq!(resp.status(), 200, "create upload");
    let state: pitcairn::dto::UploadStateDto = client.json(resp).await;

    let chunk = client
        .request(
            reqwest::Method::PUT,
            &format!("/api/v1/uploads/{}/chunks/0", state.upload_id),
        )
        .body(bytes.to_vec())
        .send()
        .await
        .expect("send chunk");
    assert_eq!(chunk.status(), 200);

    let complete = client
        .post(&format!("/api/v1/uploads/{}/complete", state.upload_id))
        .await;
    assert_eq!(complete.status(), 200);
    let body: pitcairn::dto::CompleteUploadResponse = client.json(complete).await;
    body.file_id
}

/// Create a document with one version; returns the `document_version_id`.
pub async fn create_document(
    client: &Client,
    project_id: &str,
    title: &str,
    category: &str,
    file_id: &str,
) -> String {
    let resp = client
        .post_json(
            &format!("/api/v1/projects/{project_id}/documents"),
            &serde_json::json!({
                "title": title,
                "category": category,
                "file_id": file_id,
            }),
        )
        .await;
    assert_eq!(resp.status(), 201, "create document");
    let doc: pitcairn::dto::DocumentDto = client.json(resp).await;
    doc.latest_version.expect("latest version").id
}

pub async fn user_id(app: &TestApp, email: &str) -> String {
    let row: (String,) = sqlx::query_as("SELECT id FROM users WHERE email = ?")
        .bind(email)
        .fetch_one(&app.pool)
        .await
        .expect("user");
    row.0
}

pub async fn create_project(client: &Client, title: &str) -> String {
    let resp = client
        .post_json(
            "/api/v1/projects",
            &serde_json::json!({"template_key": "base_use", "title": title}),
        )
        .await;
    assert_eq!(resp.status(), 201, "create project");
    let project: pitcairn::dto::ProjectDto = client.json(resp).await;
    project.id
}

/// The seeded "Coral health around Pitcairn" project (Anna lead, Liam/Priya
/// editors, Tomasi viewer).
pub async fn seeded_project(app: &TestApp) -> String {
    let row: (String,) =
        sqlx::query_as("SELECT id FROM projects WHERE title = 'Coral health around Pitcairn'")
            .fetch_one(&app.pool)
            .await
            .expect("seeded project");
    row.0
}

pub async fn set_project_status(app: &TestApp, project_id: &str, status: &str) {
    sqlx::query("UPDATE projects SET status = ? WHERE id = ?")
        .bind(status)
        .bind(project_id)
        .execute(&app.pool)
        .await
        .expect("set status");
}

pub async fn create_deliverable(
    client: &Client,
    project_id: &str,
    kind: &str,
    due_date: &str,
    sender_id: &str,
    recipient_id: &str,
) -> pitcairn::dto::DeliverableDto {
    let resp = client
        .post_json(
            &format!("/api/v1/projects/{project_id}/deliverables"),
            &serde_json::json!({
                "title": "Final report",
                "description": "What was found",
                "kind": kind,
                "due_date": due_date,
                "sender_id": sender_id,
                "recipient_id": recipient_id,
            }),
        )
        .await;
    assert_eq!(resp.status(), 201, "create deliverable");
    client.json(resp).await
}

/// Create a deliverable as `proposer` and have `agreer` acknowledge it so the
/// deliverable becomes `agreed` (proposer's side is acked at create time).
pub async fn agreed_deliverable(
    proposer: &Client,
    agreer: &Client,
    project_id: &str,
    kind: &str,
    sender_id: &str,
    recipient_id: &str,
) -> pitcairn::dto::DeliverableDto {
    let d = create_deliverable(
        proposer,
        project_id,
        kind,
        "2030-01-01",
        sender_id,
        recipient_id,
    )
    .await;
    let resp = agreer
        .post(&format!("/api/v1/deliverables/{}/agree", d.id))
        .await;
    assert_eq!(resp.status(), 200, "agree");
    let d: pitcairn::dto::DeliverableDto = agreer.json(resp).await;
    assert_eq!(d.status, "agreed", "deliverable should be agreed");
    d
}

/// Submit a result for `deliverable_id`; asserts 201 and returns the
/// submission DTO.
pub async fn submit(
    client: &Client,
    deliverable_id: &str,
    body: serde_json::Value,
) -> pitcairn::dto::SubmissionDto {
    let resp = client
        .post_json(
            &format!("/api/v1/deliverables/{deliverable_id}/submissions"),
            &body,
        )
        .await;
    assert_eq!(resp.status(), 201, "create submission");
    client.json(resp).await
}

pub async fn run_jobs(app: &TestApp) {
    while pitcairn::jobs::run_once(&app.state).await.expect("run job") {}
}
