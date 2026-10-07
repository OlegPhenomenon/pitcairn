mod common;

use common::{Client, login, spawn_app};

fn sha256(bytes: &[u8]) -> String {
    use sha2::Digest;
    hex::encode(sha2::Sha256::new().chain_update(bytes).finalize())
}

async fn create_upload(
    client: &Client,
    filename: &str,
    bytes: &[u8],
    mime: &str,
) -> pitcairn::dto::UploadStateDto {
    let req = pitcairn::dto::CreateUploadRequest {
        filename: filename.into(),
        size: bytes.len() as u64,
        sha256: sha256(bytes),
        mime: mime.into(),
    };
    let resp = client.post_json("/api/v1/uploads", &req).await;
    assert_eq!(resp.status(), 200, "create upload should succeed");
    client.json(resp).await
}

async fn put_chunk(client: &Client, upload_id: &str, n: u64, chunk: &[u8]) {
    let resp = client
        .request(
            reqwest::Method::PUT,
            &format!("/api/v1/uploads/{upload_id}/chunks/{n}"),
        )
        .body(chunk.to_vec())
        .send()
        .await
        .expect("send chunk");
    assert_eq!(resp.status(), 200, "chunk {n} should be accepted");
}

#[tokio::test]
async fn small_file_upload_and_complete() {
    let app = spawn_app(true).await;
    let client = login(
        &app,
        "anna@demo.pitcairn.invalid",
        pitcairn::seed::DEMO_PASSWORD,
    )
    .await;

    let bytes = b"hello integration tests";
    let state = create_upload(&client, "hello.txt", bytes, "text/plain").await;
    assert_eq!(state.chunks_total, 1);

    put_chunk(&client, &state.upload_id, 0, bytes).await;

    let complete = client
        .post(&format!("/api/v1/uploads/{}/complete", state.upload_id))
        .await;
    assert_eq!(complete.status(), 200);
    let body: pitcairn::dto::CompleteUploadResponse = client.json(complete).await;
    assert!(!body.file_id.is_empty());
}

#[tokio::test]
async fn multi_chunk_upload_tracks_missing_chunks() {
    let app = spawn_app(true).await;
    let client = login(
        &app,
        "anna@demo.pitcairn.invalid",
        pitcairn::seed::DEMO_PASSWORD,
    )
    .await;

    let chunk_size = pitcairn::files::CHUNK_SIZE as usize;
    let total = chunk_size * 2 + 10;
    let mut bytes = vec![b'a'; total];
    bytes[chunk_size * 2..].fill(b'b');

    let state = create_upload(&client, "big.bin", &bytes, "application/octet-stream").await;
    assert_eq!(state.chunks_total, 3);

    put_chunk(&client, &state.upload_id, 0, &vec![b'a'; chunk_size]).await;
    put_chunk(&client, &state.upload_id, 2, &bytes[chunk_size * 2..]).await;

    let state: pitcairn::dto::UploadStateDto = client
        .json(
            client
                .get(&format!("/api/v1/uploads/{}", state.upload_id))
                .await,
        )
        .await;
    assert_eq!(state.chunks_received, vec![0, 2]);

    put_chunk(&client, &state.upload_id, 1, &vec![b'a'; chunk_size]).await;

    let complete = client
        .post(&format!("/api/v1/uploads/{}/complete", state.upload_id))
        .await;
    assert_eq!(complete.status(), 200);
}

#[tokio::test]
async fn complete_with_wrong_sha256_fails() {
    let app = spawn_app(true).await;
    let client = login(
        &app,
        "anna@demo.pitcairn.invalid",
        pitcairn::seed::DEMO_PASSWORD,
    )
    .await;

    let bytes = b"wrong checksum content";
    let mut req = pitcairn::dto::CreateUploadRequest {
        filename: "wrong.txt".into(),
        size: bytes.len() as u64,
        sha256: "0".repeat(64),
        mime: "text/plain".into(),
    };
    req.sha256 = "0".repeat(64);

    let resp = client.post_json("/api/v1/uploads", &req).await;
    assert_eq!(resp.status(), 200);
    let state: pitcairn::dto::UploadStateDto = client.json(resp).await;

    put_chunk(&client, &state.upload_id, 0, bytes).await;

    let complete = client
        .post(&format!("/api/v1/uploads/{}/complete", state.upload_id))
        .await;
    assert_eq!(complete.status(), 422);
    let err: serde_json::Value = client.json(complete).await;
    assert_eq!(err["error"]["code"], "checksum_mismatch");
}

#[tokio::test]
async fn eicar_file_is_rejected_by_scan() {
    let app = spawn_app(true).await;
    let anna = login(
        &app,
        "anna@demo.pitcairn.invalid",
        pitcairn::seed::DEMO_PASSWORD,
    )
    .await;

    let eicar = br"X5O!P%@AP[4\PZX54(P^)7CC)7}$EICAR-STANDARD-ANTIVIRUS-TEST-FILE!$H+H*";
    let state = create_upload(&anna, "eicar.txt", eicar, "text/plain").await;
    put_chunk(&anna, &state.upload_id, 0, eicar).await;

    let complete = anna
        .post(&format!("/api/v1/uploads/{}/complete", state.upload_id))
        .await;
    assert_eq!(complete.status(), 200);
    let body: pitcairn::dto::CompleteUploadResponse = anna.json(complete).await;
    let file_id = body.file_id;

    while pitcairn::jobs::run_once(&app.state).await.expect("run job") {}

    let row: (String,) = sqlx::query_as("SELECT scan_status FROM files WHERE id = ?")
        .bind(&file_id)
        .fetch_one(&app.pool)
        .await
        .expect("file row");
    assert_eq!(row.0, "rejected", "EICAR should be rejected");

    let project_resp = anna
        .post_json(
            "/api/v1/projects",
            &serde_json::json!({"template_key": "base_use", "title": "EICAR project"}),
        )
        .await;
    assert_eq!(project_resp.status(), 201);
    let project: pitcairn::dto::ProjectDto = anna.json(project_resp).await;

    let doc_resp = anna
        .post_json(
            &format!("/api/v1/projects/{}/documents", project.id),
            &serde_json::json!({
                "title": "EICAR doc",
                "category": "other",
                "file_id": file_id,
            }),
        )
        .await;
    assert_eq!(doc_resp.status(), 201);
    let doc: pitcairn::dto::DocumentDto = anna.json(doc_resp).await;
    let version_id = doc.latest_version.unwrap().id;

    let download = anna
        .get(&format!("/api/v1/document-versions/{version_id}/download"))
        .await;
    assert_eq!(download.status(), 409);
    let err: serde_json::Value = anna.json(download).await;
    assert_eq!(err["error"]["code"], "scan_not_clean");
}

#[tokio::test]
async fn wrong_chunk_length_is_rejected() {
    let app = spawn_app(true).await;
    let client = login(
        &app,
        "anna@demo.pitcairn.invalid",
        pitcairn::seed::DEMO_PASSWORD,
    )
    .await;

    let bytes = b"tiny";
    let state = create_upload(&client, "tiny.txt", bytes, "text/plain").await;

    let resp = client
        .request(
            reqwest::Method::PUT,
            &format!("/api/v1/uploads/{}/chunks/0", state.upload_id),
        )
        .body(vec![b'x'; 100])
        .send()
        .await
        .expect("send chunk");
    assert_eq!(resp.status(), 422);
    let err: serde_json::Value = client.json(resp).await;
    assert_eq!(err["error"]["code"], "bad_chunk_length");
}
