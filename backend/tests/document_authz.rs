mod common;

use common::{Client, login, persona, spawn_app};

fn sha256(bytes: &[u8]) -> String {
    use sha2::Digest;
    hex::encode(sha2::Sha256::new().chain_update(bytes).finalize())
}

async fn upload_clean_file(client: &Client, app: &common::TestApp, bytes: &[u8]) -> String {
    let req = pitcairn::dto::CreateUploadRequest {
        filename: "clean.txt".into(),
        size: bytes.len() as u64,
        sha256: sha256(bytes),
        mime: "text/plain".into(),
    };
    let resp = client.post_json("/api/v1/uploads", &req).await;
    assert_eq!(resp.status(), 200, "create upload should succeed");
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

    while pitcairn::jobs::run_once(&app.state).await.expect("run job") {}

    let row: (String,) = sqlx::query_as("SELECT scan_status FROM files WHERE id = ?")
        .bind(&body.file_id)
        .fetch_one(&app.pool)
        .await
        .expect("file row");
    assert_eq!(row.0, "clean");

    body.file_id
}

#[tokio::test]
async fn team_member_can_download_lead_document() {
    let app = spawn_app(true).await;
    let anna = persona(&app, "anna").await;
    let liam = persona(&app, "liam").await;

    let project_id: (String,) =
        sqlx::query_as("SELECT id FROM projects WHERE title = 'Coral health around Pitcairn'")
            .fetch_one(&app.pool)
            .await
            .expect("seeded project");
    let project_id = project_id.0;

    let file_id = upload_clean_file(&anna, &app, b"secret team doc").await;

    let doc_resp = anna
        .post_json(
            &format!("/api/v1/projects/{project_id}/documents"),
            &serde_json::json!({
                "title": "Team doc",
                "category": "application",
                "file_id": file_id,
            }),
        )
        .await;
    assert_eq!(doc_resp.status(), 201);
    let doc: pitcairn::dto::DocumentDto = anna.json(doc_resp).await;
    let version_id = doc.latest_version.unwrap().id;

    let anna_download = anna
        .get(&format!("/api/v1/document-versions/{version_id}/download"))
        .await;
    assert_eq!(anna_download.status(), 200);

    let liam_download = liam
        .get(&format!("/api/v1/document-versions/{version_id}/download"))
        .await;
    assert_eq!(liam_download.status(), 200);

    let anonymous_download = Client::anonymous(&app)
        .get(&format!("/api/v1/document-versions/{version_id}/download"))
        .await;
    assert_eq!(anonymous_download.status(), 404);
}

#[tokio::test]
async fn outsider_cannot_download_or_view_project() {
    let app = spawn_app(true).await;
    let anna = persona(&app, "anna").await;
    let lukas = persona(&app, "lukas").await;

    let project_resp = anna
        .post_json(
            "/api/v1/projects",
            &serde_json::json!({"template_key": "base_use", "title": "Outsider test project"}),
        )
        .await;
    assert_eq!(project_resp.status(), 201);
    let project: pitcairn::dto::ProjectDto = anna.json(project_resp).await;

    let file_id = upload_clean_file(&anna, &app, b"private content").await;

    let doc_resp = anna
        .post_json(
            &format!("/api/v1/projects/{}/documents", project.id),
            &serde_json::json!({
                "title": "Private doc",
                "category": "application",
                "file_id": file_id,
            }),
        )
        .await;
    assert_eq!(doc_resp.status(), 201);
    let doc: pitcairn::dto::DocumentDto = anna.json(doc_resp).await;
    let version_id = doc.latest_version.unwrap().id;

    let project_get = lukas.get(&format!("/api/v1/projects/{}", project.id)).await;
    assert_eq!(project_get.status(), 403);

    let download = lukas
        .get(&format!("/api/v1/document-versions/{version_id}/download"))
        .await;
    assert_eq!(download.status(), 403);
}

#[tokio::test]
async fn removed_member_loses_download_access() {
    let app = spawn_app(true).await;
    let anna = persona(&app, "anna").await;
    let liam = login(
        &app,
        "liam@demo.pitcairn.invalid",
        pitcairn::seed::DEMO_PASSWORD,
    )
    .await;

    let project_id: (String,) =
        sqlx::query_as("SELECT id FROM projects WHERE title = 'Coral health around Pitcairn'")
            .fetch_one(&app.pool)
            .await
            .expect("seeded project");
    let project_id = project_id.0;

    let file_id = upload_clean_file(&anna, &app, b"remove me doc").await;

    let doc_resp = anna
        .post_json(
            &format!("/api/v1/projects/{project_id}/documents"),
            &serde_json::json!({
                "title": "Removable doc",
                "category": "application",
                "file_id": file_id,
            }),
        )
        .await;
    assert_eq!(doc_resp.status(), 201);
    let doc: pitcairn::dto::DocumentDto = anna.json(doc_resp).await;
    let version_id = doc.latest_version.unwrap().id;

    let before = liam
        .get(&format!("/api/v1/document-versions/{version_id}/download"))
        .await;
    assert_eq!(before.status(), 200, "liam should download before removal");

    let anna_id: (String,) = sqlx::query_as("SELECT id FROM users WHERE email = ?")
        .bind("anna@demo.pitcairn.invalid")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    let liam_id: (String,) = sqlx::query_as("SELECT id FROM users WHERE email = ?")
        .bind("liam@demo.pitcairn.invalid")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    let now = pitcairn::util::now_rfc3339();
    sqlx::query(
        "UPDATE project_members SET removed_at = ?, removed_by = ?
         WHERE project_id = ? AND user_id = ? AND removed_at IS NULL",
    )
    .bind(&now)
    .bind(&anna_id.0)
    .bind(&project_id)
    .bind(&liam_id.0)
    .execute(&app.pool)
    .await
    .expect("remove liam");

    let after = liam
        .get(&format!("/api/v1/document-versions/{version_id}/download"))
        .await;
    assert_eq!(after.status(), 403, "liam should be blocked after removal");
}
