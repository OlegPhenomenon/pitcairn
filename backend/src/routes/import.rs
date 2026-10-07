//! Import/export (§5 admin, §8): legacy CSV preview, project-archive preview,
//! commit of either batch kind, and the project export ZIP.
//!
//! Upload bodies are either the raw file bytes (CSV / ZIP) or JSON
//! `{"file_id": "..."}` naming a file uploaded through `/uploads` by the same
//! admin — that file must have passed the scan (`clean`) before it is parsed.

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, State};
use axum::http::{HeaderMap, HeaderValue, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::AppState;
use crate::audit::{self, AuditEvent};
use crate::authz::{self, Actor};
use crate::dto::{
    ArchiveImportPreviewResponse, ImportBatchDto, ImportCommitResponse, ImportRowPreviewDto,
    LegacyImportPreviewResponse,
};
use crate::error::{AppError, AppResult};
use crate::{archive, legacy};

pub fn router(state: AppState) -> Router<AppState> {
    let archive_limit = usize::try_from(state.config.max_upload_bytes).unwrap_or(usize::MAX);
    Router::new()
        .route(
            "/admin/import/legacy/preview",
            post(legacy_preview).route_layer(DefaultBodyLimit::max(32 * 1024 * 1024)),
        )
        .route(
            "/admin/import/project-archive",
            post(archive_preview).route_layer(DefaultBodyLimit::max(archive_limit)),
        )
        .route("/admin/import/{batch_id}/commit", post(commit_batch))
        .route("/projects/{id}/export", get(export_project))
}

#[derive(Deserialize)]
struct FileRef {
    file_id: String,
}

/// Resolve the upload body to bytes: raw body, or a clean uploaded file the
/// actor owns. Raw bodies go through the same mock scanner as uploads.
async fn body_bytes(
    state: &AppState,
    actor: &Actor,
    headers: &HeaderMap,
    body: Bytes,
    declared_mime: &str,
) -> AppResult<Vec<u8>> {
    let is_json = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("application/json"));
    if is_json {
        let file_ref: FileRef = serde_json::from_slice(&body)
            .map_err(|e| AppError::BadRequest(format!("expected {{\"file_id\"}}: {e}")))?;
        authz::ensure_file_owned_by(&state.pool, actor, &file_ref.file_id).await?;
        let (sha256, scan_status): (String, String) =
            sqlx::query_as("SELECT sha256, scan_status FROM files WHERE id = ?")
                .bind(&file_ref.file_id)
                .fetch_one(&state.pool)
                .await?;
        if scan_status != "clean" {
            return Err(AppError::conflict(
                "file_not_clean",
                "the uploaded file has not passed the scan yet",
            ));
        }
        return crate::files::read_file(&state.config.data_dir, &sha256).await;
    }
    if body.is_empty() {
        return Err(AppError::Validation {
            fields: [("file".to_string(), "file is required".to_string())].into(),
        });
    }
    if let Some(reason) = crate::jobs::scan_bytes(&body, declared_mime) {
        return Err(AppError::unprocessable("file_rejected", reason));
    }
    Ok(body.to_vec())
}

fn batch_dto(id: String, kind: &str, status: &str, created_at: String) -> ImportBatchDto {
    ImportBatchDto {
        id,
        kind: kind.into(),
        status: status.into(),
        created_at,
    }
}

// ---------------------------------------------------------------------------
// Legacy CSV
// ---------------------------------------------------------------------------

async fn legacy_preview(
    State(state): State<AppState>,
    actor: Actor,
    headers: HeaderMap,
    body: Bytes,
) -> AppResult<Json<LegacyImportPreviewResponse>> {
    authz::require_role(&actor, &["admin"])?;
    let bytes = body_bytes(&state, &actor, &headers, body, "text/csv").await?;
    let parsed = legacy::parse_csv(&bytes)?;
    let rows = legacy::preview_rows(&state.pool, parsed).await?;
    let preview = legacy::preview_json(&rows);
    let (id, created_at) =
        legacy::create_batch(&state.pool, legacy::KIND, &preview, &actor).await?;
    Ok(Json(LegacyImportPreviewResponse {
        batch: batch_dto(id, legacy::KIND, "previewed", created_at),
        rows: rows
            .into_iter()
            .map(|r| ImportRowPreviewDto {
                index: r.index,
                data: r.data,
                errors: r.errors,
                duplicate_of: r.duplicate_of,
            })
            .collect(),
    }))
}

// ---------------------------------------------------------------------------
// Project archive
// ---------------------------------------------------------------------------

/// Archive bytes are kept under `uploads/` until commit re-validates them.
fn staged_archive_path(state: &AppState, sha256: &str) -> std::path::PathBuf {
    state
        .config
        .data_dir
        .join("uploads")
        .join(format!("import-{sha256}.zip"))
}

async fn parse_blocking(bytes: Vec<u8>, max: u64) -> AppResult<(archive::ParsedArchive, Vec<u8>)> {
    tokio::task::spawn_blocking(move || {
        let parsed = archive::parse_archive(&bytes, max)?;
        Ok((parsed, bytes))
    })
    .await
    .map_err(AppError::internal)?
}

async fn archive_preview(
    State(state): State<AppState>,
    actor: Actor,
    headers: HeaderMap,
    body: Bytes,
) -> AppResult<Json<ArchiveImportPreviewResponse>> {
    authz::require_role(&actor, &["admin"])?;
    let bytes = body_bytes(&state, &actor, &headers, body, "application/zip").await?;
    let (parsed, bytes) = parse_blocking(bytes, state.config.max_upload_bytes).await?;
    let preview = archive::preview_archive(&state.pool, &parsed).await?;

    let sha256 = crate::util::sha256_hex(&bytes);
    let staged = staged_archive_path(&state, &sha256);
    if !staged.exists() {
        let tmp = staged.with_extension("tmp");
        tokio::fs::write(&tmp, &bytes).await?;
        tokio::fs::rename(&tmp, &staged).await?;
    }

    let preview_json = json!({
        "archive_sha256": sha256,
        "project_id": preview.project_id,
        "project_reference": preview.project_reference,
        "project_title": preview.project_title,
        "records": preview.record_counts,
        "files": preview.file_count,
        "conflicts": preview.conflicts,
        "matched_users": preview.matched_users,
        "new_users": preview.new_users,
    });
    let (id, created_at) =
        legacy::create_batch(&state.pool, archive::KIND, &preview_json, &actor).await?;
    Ok(Json(ArchiveImportPreviewResponse {
        batch: batch_dto(id, archive::KIND, "previewed", created_at),
        project_reference: preview.project_reference,
        project_title: preview.project_title,
        records: preview.record_counts,
        files: preview.file_count,
        conflicts: preview.conflicts,
        matched_users: preview.matched_users,
        new_users: preview.new_users,
    }))
}

// ---------------------------------------------------------------------------
// Commit (either kind)
// ---------------------------------------------------------------------------

async fn commit_batch(
    State(state): State<AppState>,
    actor: Actor,
    Path(batch_id): Path<String>,
) -> AppResult<Json<ImportCommitResponse>> {
    authz::require_role(&actor, &["admin"])?;
    let (kind, status, preview_json): (String, String, String) =
        sqlx::query_as("SELECT kind, status, preview_json FROM import_batches WHERE id = ?")
            .bind(&batch_id)
            .fetch_optional(&state.pool)
            .await?
            .ok_or(AppError::NotFound)?;
    if status != "previewed" {
        return Err(AppError::conflict(
            "batch_not_previewed",
            format!("this import batch is already {status}"),
        ));
    }
    let preview: Value = serde_json::from_str(&preview_json).map_err(AppError::internal)?;

    match kind.as_str() {
        legacy::KIND => {
            let rows = legacy::rows_from_preview(&preview)?;
            let (created, skipped, errors) =
                legacy::commit_rows(&state.pool, &batch_id, &rows, &actor).await?;
            Ok(Json(ImportCommitResponse {
                batch_id,
                status: "committed".into(),
                created,
                skipped,
                errors,
            }))
        }
        archive::KIND => {
            let sha256 = preview["archive_sha256"]
                .as_str()
                .ok_or_else(|| AppError::internal("archive batch without archive_sha256"))?;
            let staged = staged_archive_path(&state, sha256);
            let bytes = tokio::fs::read(&staged).await.map_err(|_| {
                AppError::conflict(
                    "archive_missing",
                    "the staged archive is gone; preview it again",
                )
            })?;
            // Re-validate everything at commit (§8).
            let (parsed, _) = parse_blocking(bytes, state.config.max_upload_bytes).await?;
            let outcome = archive::commit_archive(
                &state.pool,
                &state.config.data_dir,
                &parsed,
                Some(&actor.user_id),
                &actor.name,
                Some(&batch_id),
            )
            .await?;
            tokio::fs::remove_file(&staged).await.ok();
            let created = outcome.created_tables.values().sum();
            Ok(Json(ImportCommitResponse {
                batch_id,
                status: "committed".into(),
                created,
                skipped: i64::from(outcome.already_existed),
                errors: outcome.messages,
            }))
        }
        other => Err(AppError::internal(format!("unknown import kind {other}"))),
    }
}

// ---------------------------------------------------------------------------
// Export
// ---------------------------------------------------------------------------

async fn export_project(
    State(state): State<AppState>,
    actor: Actor,
    Path(project_id): Path<String>,
) -> AppResult<Response> {
    authz::require_role(&actor, &["coordinator", "admin"])?;
    let exists: Option<String> = sqlx::query_scalar("SELECT id FROM projects WHERE id = ?")
        .bind(&project_id)
        .fetch_optional(&state.pool)
        .await?;
    if exists.is_none() {
        return Err(AppError::NotFound);
    }
    let export = archive::export_project(&state.pool, &state.config.data_dir, &project_id).await?;

    let mut tx = state.pool.begin().await?;
    audit::record(
        &mut tx,
        AuditEvent {
            actor_id: Some(actor.user_id.clone()),
            actor_label: actor.name.clone(),
            action: "project.exported".into(),
            entity_type: "project".into(),
            entity_id: project_id.clone(),
            project_id: Some(project_id.clone()),
            visibility: "internal".into(),
            summary: "Project exported as ZIP archive".into(),
            before: None,
            after: None,
            reason: None,
        },
    )
    .await?;
    tx.commit().await?;

    let name = export
        .project_reference
        .clone()
        .unwrap_or_else(|| export.project_id.clone());
    let disposition = format!("attachment; filename=\"{name}.zip\"");
    let mut resp = export.bytes.into_response();
    let h = resp.headers_mut();
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/zip"),
    );
    h.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&disposition).map_err(AppError::internal)?,
    );
    h.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    h.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    Ok(resp)
}
