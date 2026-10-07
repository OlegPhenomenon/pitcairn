use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use serde_json::json;
use sqlx::FromRow;

use crate::authz::Actor;
use crate::dto::{CompleteUploadResponse, CreateUploadRequest, UploadStateDto};
use crate::error::{AppError, AppResult};
use crate::util::{new_id, now_rfc3339, time_plus_secs};
use crate::validation::FieldErrors;
use crate::{AppState, files, jobs};

const CHUNK_BODY_LIMIT: usize = (files::CHUNK_SIZE + 1024 * 1024) as usize;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/uploads", post(create_upload))
        .route(
            "/uploads/{id}/chunks/{n}",
            put(put_chunk).route_layer(DefaultBodyLimit::max(CHUNK_BODY_LIMIT)),
        )
        .route("/uploads/{id}", get(get_upload).delete(delete_upload))
        .route("/uploads/{id}/complete", post(complete_upload))
}

#[derive(FromRow)]
#[allow(dead_code)]
struct UploadRow {
    id: String,
    user_id: String,
    filename: String,
    size: i64,
    sha256: String,
    mime: String,
    chunk_size: i64,
    chunks_total: i64,
    chunks_received_json: String,
    status: String,
    file_id: Option<String>,
    expires_at: String,
}

async fn load_upload(
    pool: &sqlx::SqlitePool,
    upload_id: &str,
    user_id: &str,
) -> AppResult<UploadRow> {
    let row: Option<UploadRow> = sqlx::query_as(
        "SELECT id, user_id, filename, size, sha256, mime, chunk_size, chunks_total,
                chunks_received_json, status, file_id, expires_at
         FROM upload_sessions WHERE id = ? AND user_id = ?",
    )
    .bind(upload_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await?;
    row.ok_or(AppError::NotFound)
}

fn parse_chunks(json: &str) -> AppResult<Vec<u64>> {
    serde_json::from_str(json).map_err(AppError::internal)
}

fn to_dto(row: &UploadRow) -> AppResult<UploadStateDto> {
    Ok(UploadStateDto {
        upload_id: row.id.clone(),
        chunk_size: row.chunk_size as u64,
        chunks_total: row.chunks_total as u64,
        chunks_received: parse_chunks(&row.chunks_received_json)?,
        status: row.status.clone(),
        file_id: row.file_id.clone(),
    })
}

fn require_open(status: &str) -> AppResult<()> {
    match status {
        "open" => Ok(()),
        "aborted" => Err(AppError::conflict(
            "upload_aborted",
            "upload has been aborted",
        )),
        _ => Err(AppError::conflict(
            "upload_finalizing",
            "upload is being finalized or is already complete",
        )),
    }
}

async fn create_upload(
    State(state): State<AppState>,
    actor: Actor,
    Json(req): Json<CreateUploadRequest>,
) -> AppResult<Json<UploadStateDto>> {
    let mut errors = FieldErrors::new();
    errors.require("filename", &req.filename, "filename is required");
    errors.max_len("filename", &req.filename, 255);
    errors.check("size", req.size >= 1, "size must be at least 1 byte");
    errors.check(
        "size",
        req.size <= state.config.max_upload_bytes,
        &format!(
            "size must be at most {} bytes",
            state.config.max_upload_bytes
        ),
    );
    errors.check(
        "sha256",
        req.sha256.len() == 64
            && req
                .sha256
                .chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
        "must be a 64-character lowercase hex sha256",
    );
    errors.finish()?;

    let chunk_size = files::CHUNK_SIZE;
    let chunks_total = req.size.div_ceil(chunk_size) as i64;
    let upload_id = new_id();
    let now = now_rfc3339();
    let expires = time_plus_secs(24 * 3600);
    sqlx::query(
        "INSERT INTO upload_sessions
         (id, user_id, filename, size, sha256, mime, chunk_size, chunks_total,
          chunks_received_json, status, file_id, expires_at, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, '[]', 'open', NULL, ?, ?)",
    )
    .bind(&upload_id)
    .bind(&actor.user_id)
    .bind(&req.filename)
    .bind(req.size as i64)
    .bind(&req.sha256)
    .bind(&req.mime)
    .bind(chunk_size as i64)
    .bind(chunks_total)
    .bind(&expires)
    .bind(&now)
    .execute(&state.pool)
    .await?;

    Ok(Json(UploadStateDto {
        upload_id,
        chunk_size,
        chunks_total: chunks_total as u64,
        chunks_received: vec![],
        status: "open".into(),
        file_id: None,
    }))
}

async fn put_chunk(
    State(state): State<AppState>,
    actor: Actor,
    Path((upload_id, n)): Path<(String, u64)>,
    body: Bytes,
) -> AppResult<Json<UploadStateDto>> {
    let row = load_upload(&state.pool, &upload_id, &actor.user_id).await?;
    require_open(&row.status)?;
    if n >= row.chunks_total as u64 {
        return Err(AppError::unprocessable(
            "bad_chunk_index",
            "chunk index out of range",
        ));
    }

    let expected_len = if n == row.chunks_total as u64 - 1 {
        let last = row.size - (row.chunks_total - 1) * row.chunk_size;
        last.max(0) as usize
    } else {
        row.chunk_size as usize
    };
    if body.len() != expected_len {
        return Err(AppError::unprocessable(
            "bad_chunk_length",
            format!("chunk {n} must be exactly {expected_len} bytes"),
        ));
    }

    let lock = state.upload_lock(&upload_id);
    let _guard = lock.lock().await;

    let row = load_upload(&state.pool, &upload_id, &actor.user_id).await?;
    require_open(&row.status)?;

    files::write_chunk(
        &state.config.data_dir,
        &upload_id,
        n,
        row.chunk_size as u64,
        &body,
    )
    .await?;

    let mut chunks = parse_chunks(&row.chunks_received_json)?;
    chunks.push(n);
    chunks.sort_unstable();
    chunks.dedup();
    let chunks_json = serde_json::to_string(&chunks).map_err(AppError::internal)?;
    sqlx::query("UPDATE upload_sessions SET chunks_received_json = ? WHERE id = ?")
        .bind(&chunks_json)
        .bind(&upload_id)
        .execute(&state.pool)
        .await?;

    let row = load_upload(&state.pool, &upload_id, &actor.user_id).await?;
    Ok(Json(to_dto(&row)?))
}

async fn get_upload(
    State(state): State<AppState>,
    actor: Actor,
    Path(upload_id): Path<String>,
) -> AppResult<Json<UploadStateDto>> {
    let row = load_upload(&state.pool, &upload_id, &actor.user_id).await?;
    Ok(Json(to_dto(&row)?))
}

async fn complete_upload(
    State(state): State<AppState>,
    actor: Actor,
    Path(upload_id): Path<String>,
) -> AppResult<Json<CompleteUploadResponse>> {
    let row = load_upload(&state.pool, &upload_id, &actor.user_id).await?;
    if row.status != "open" {
        return Err(AppError::conflict(
            "upload_finalizing",
            "upload is not open",
        ));
    }

    let chunks = parse_chunks(&row.chunks_received_json)?;
    let expected: Vec<u64> = (0..row.chunks_total as u64).collect();
    if chunks != expected {
        let missing = expected
            .into_iter()
            .find(|i| !chunks.contains(i))
            .unwrap_or(0);
        return Err(AppError::unprocessable(
            "missing_chunks",
            format!("missing chunk {missing}"),
        ));
    }

    let lock = state.upload_lock(&upload_id);
    let _guard = lock.lock().await;
    let row = load_upload(&state.pool, &upload_id, &actor.user_id).await?;
    if row.status != "open" {
        return Err(AppError::conflict(
            "upload_finalizing",
            "upload is not open",
        ));
    }
    sqlx::query("UPDATE upload_sessions SET status = 'finalizing' WHERE id = ?")
        .bind(&upload_id)
        .execute(&state.pool)
        .await?;
    drop(_guard);

    let part = files::part_path(&state.config.data_dir, &upload_id);
    let meta = tokio::fs::metadata(&part).await?;
    if meta.len() != row.size as u64 {
        sqlx::query("UPDATE upload_sessions SET status = 'open' WHERE id = ?")
            .bind(&upload_id)
            .execute(&state.pool)
            .await?;
        return Err(AppError::unprocessable(
            "checksum_mismatch",
            "file size does not match declared size",
        ));
    }

    let actual_sha = files::hash_file(&part).await?;
    if actual_sha != row.sha256 {
        sqlx::query("UPDATE upload_sessions SET status = 'open' WHERE id = ?")
            .bind(&upload_id)
            .execute(&state.pool)
            .await?;
        return Err(AppError::unprocessable(
            "checksum_mismatch",
            "sha256 does not match declared hash",
        ));
    }

    files::store_file(&state.config.data_dir, &part, &row.sha256).await?;

    let mut tx = state.pool.begin().await?;
    let now = now_rfc3339();
    let file_id = new_id();
    let storage_key = files::file_path(&state.config.data_dir, &row.sha256)
        .to_string_lossy()
        .to_string();
    sqlx::query(
        "INSERT INTO files (id, sha256, size, mime, storage_key, scan_status, scan_detail, uploaded_by, created_at)
         VALUES (?, ?, ?, ?, ?, 'pending', NULL, ?, ?)
         ON CONFLICT(sha256) DO NOTHING",
    )
    .bind(&file_id)
    .bind(&row.sha256)
    .bind(row.size)
    .bind(&row.mime)
    .bind(&storage_key)
    .bind(&actor.user_id)
    .bind(&now)
    .execute(&mut *tx)
    .await?;
    let file_id: String = sqlx::query_scalar("SELECT id FROM files WHERE sha256 = ?")
        .bind(&row.sha256)
        .fetch_one(&mut *tx)
        .await?;
    sqlx::query("UPDATE upload_sessions SET status = 'complete', file_id = ? WHERE id = ?")
        .bind(&file_id)
        .bind(&upload_id)
        .execute(&mut *tx)
        .await?;
    jobs::enqueue(
        &mut tx,
        jobs::KIND_SCAN_FILE,
        json!({"file_id": file_id}),
        None,
    )
    .await?;
    tx.commit().await?;

    Ok(Json(CompleteUploadResponse {
        file_id,
        scan_status: "pending".into(),
    }))
}

async fn delete_upload(
    State(state): State<AppState>,
    actor: Actor,
    Path(upload_id): Path<String>,
) -> AppResult<impl IntoResponse> {
    let row = load_upload(&state.pool, &upload_id, &actor.user_id).await?;
    if row.status == "complete" {
        return Err(AppError::conflict(
            "upload_complete",
            "upload is already complete",
        ));
    }
    sqlx::query("UPDATE upload_sessions SET status = 'aborted' WHERE id = ?")
        .bind(&upload_id)
        .execute(&state.pool)
        .await?;
    let part = files::part_path(&state.config.data_dir, &upload_id);
    tokio::fs::remove_file(part).await.ok();
    Ok(StatusCode::NO_CONTENT)
}
