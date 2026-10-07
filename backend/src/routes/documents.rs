use axum::extract::{Path, State};
use axum::http::{HeaderValue, Response, StatusCode, header};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::Value;
use sqlx::FromRow;

use crate::AppState;
use crate::audit::{self, AuditEvent};
use crate::authz::{self, Actor, ProjectAccess};
use crate::db;
use crate::dto::{
    CreateDocumentRequest, DocumentDto, DocumentVersionDto, ListResponse, NewVersionRequest,
};
use crate::error::{AppError, AppResult};
use crate::util::{new_id, now_rfc3339};
use crate::validation::FieldErrors;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/projects/{id}/documents",
            get(list_documents).post(create_document),
        )
        .route("/documents/{id}/versions", post(add_version))
        .route("/document-versions/{id}/download", get(download_version))
}

fn can_upload(access: ProjectAccess, actor: &Actor) -> bool {
    matches!(access, ProjectAccess::TeamEditor | ProjectAccess::TeamLead) || actor.is_coordinator()
}

fn safe_filename(name: &str) -> String {
    let mut out: String = name
        .chars()
        .filter(|c| c.is_ascii())
        .map(|c| {
            if matches!(c, '"' | '\\' | '/') {
                '_'
            } else {
                c
            }
        })
        .collect();
    if out.is_empty() {
        out = "download".into();
    }
    out
}

#[derive(FromRow)]
#[allow(dead_code)]
struct DocumentRow {
    id: String,
    project_id: String,
    slot_key: Option<String>,
    title: String,
    category: String,
    created_by: String,
    created_at: String,
}

#[derive(FromRow)]
#[allow(dead_code)]
struct VersionRow {
    id: String,
    document_id: String,
    number: i64,
    file_id: String,
    note: String,
    uploaded_by: String,
    uploaded_at: String,
    scan_status: String,
    size: i64,
    mime: String,
}

#[derive(FromRow)]
#[allow(dead_code)]
struct VersionDownloadRow {
    document_id: String,
    project_id: String,
    category: String,
    title: String,
    sha256: String,
    size: i64,
    mime: String,
    scan_status: String,
}

/// All versions of a document, newest first. Old versions stay accessible (§4).
async fn load_versions(
    pool: &sqlx::SqlitePool,
    document_id: &str,
) -> AppResult<Vec<DocumentVersionDto>> {
    let rows: Vec<VersionRow> = sqlx::query_as(
        "SELECT dv.id, dv.document_id, dv.number, dv.file_id, dv.note, dv.uploaded_by,
                dv.uploaded_at, f.scan_status, f.size, f.mime
         FROM document_versions dv
         JOIN files f ON f.id = dv.file_id
         WHERE dv.document_id = ?
         ORDER BY dv.number DESC",
    )
    .bind(document_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| DocumentVersionDto {
            id: r.id,
            document_id: r.document_id,
            number: r.number,
            file_id: r.file_id,
            note: r.note,
            uploaded_by: r.uploaded_by,
            uploaded_at: r.uploaded_at,
            scan_status: r.scan_status,
            size: r.size,
            mime: r.mime,
        })
        .collect())
}

async fn list_documents(
    State(state): State<AppState>,
    actor: Actor,
    Path(project_id): Path<String>,
) -> AppResult<Json<ListResponse<DocumentDto>>> {
    let access = authz::project_access(&state.pool, &actor, &project_id).await?;
    if access == ProjectAccess::None {
        return Err(AppError::forbidden(
            "you do not have access to this project",
        ));
    }

    let rows: Vec<DocumentRow> = sqlx::query_as(
        "SELECT id, project_id, slot_key, title, category, created_by, created_at
         FROM documents WHERE project_id = ? ORDER BY created_at DESC",
    )
    .bind(&project_id)
    .fetch_all(&state.pool)
    .await?;

    let mut items = Vec::new();
    for row in rows {
        if !authz::can_view_document_category(&actor, access, &row.category) {
            continue;
        }
        let versions = load_versions(&state.pool, &row.id).await?;
        items.push(DocumentDto {
            id: row.id,
            project_id: row.project_id,
            slot_key: row.slot_key,
            title: row.title,
            category: row.category,
            created_by: row.created_by,
            created_at: row.created_at,
            latest_version: versions.first().cloned(),
            versions,
        });
    }
    let total = items.len() as i64;
    Ok(Json(ListResponse { items, total }))
}

async fn create_document(
    State(state): State<AppState>,
    actor: Actor,
    Path(project_id): Path<String>,
    Json(req): Json<CreateDocumentRequest>,
) -> AppResult<impl IntoResponse> {
    let access = authz::project_access(&state.pool, &actor, &project_id).await?;
    if !can_upload(access, &actor) {
        return Err(AppError::forbidden(
            "you do not have permission to add documents",
        ));
    }

    let mut errors = FieldErrors::new();
    errors.require("title", &req.title, "title is required");
    errors.max_len("title", &req.title, 500);
    errors.check(
        "category",
        matches!(
            req.category.as_str(),
            "application" | "personal" | "decision" | "result" | "other"
        ),
        "must be one of application, personal, decision, result, other",
    );
    errors.finish()?;

    authz::ensure_file_owned_by(&state.pool, &actor, &req.file_id).await?;

    let mut tx = db::begin_immediate(&state.pool).await?;
    let doc_id = new_id();
    let now = now_rfc3339();
    let note = req.note.unwrap_or_default();
    sqlx::query(
        "INSERT INTO documents (id, project_id, slot_key, title, category, created_by, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&doc_id)
    .bind(&project_id)
    .bind(&req.slot_key)
    .bind(&req.title)
    .bind(&req.category)
    .bind(&actor.user_id)
    .bind(&now)
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        "INSERT INTO document_versions
         (id, document_id, number, file_id, note, uploaded_by, uploaded_at, created_at)
         VALUES (?, ?, 1, ?, ?, ?, ?, ?)",
    )
    .bind(new_id())
    .bind(&doc_id)
    .bind(&req.file_id)
    .bind(&note)
    .bind(&actor.user_id)
    .bind(&now)
    .bind(&now)
    .execute(&mut *tx)
    .await?;

    audit::record(
        &mut tx,
        AuditEvent {
            actor_id: Some(actor.user_id.clone()),
            actor_label: actor.name.clone(),
            action: "document.created".into(),
            entity_type: "document".into(),
            entity_id: doc_id.clone(),
            project_id: Some(project_id.clone()),
            visibility: "shared".into(),
            summary: format!("{} added document '{}'", actor.name, req.title),
            before: None,
            after: Some(Value::String(req.title.clone())),
            reason: None,
        },
    )
    .await?;

    tx.commit().await?;

    let versions = load_versions(&state.pool, &doc_id).await?;
    Ok((
        StatusCode::CREATED,
        Json(DocumentDto {
            id: doc_id,
            project_id,
            slot_key: req.slot_key,
            title: req.title,
            category: req.category,
            created_by: actor.user_id,
            created_at: now,
            latest_version: versions.first().cloned(),
            versions,
        }),
    ))
}

async fn add_version(
    State(state): State<AppState>,
    actor: Actor,
    Path(document_id): Path<String>,
    Json(req): Json<NewVersionRequest>,
) -> AppResult<impl IntoResponse> {
    let doc: Option<(String, String)> =
        sqlx::query_as("SELECT id, project_id FROM documents WHERE id = ?")
            .bind(&document_id)
            .fetch_optional(&state.pool)
            .await?;
    let (doc_id, project_id) = doc.ok_or(AppError::NotFound)?;

    let access = authz::project_access(&state.pool, &actor, &project_id).await?;
    if !can_upload(access, &actor) {
        return Err(AppError::forbidden(
            "you do not have permission to add versions",
        ));
    }

    authz::ensure_file_owned_by(&state.pool, &actor, &req.file_id).await?;

    let mut tx = db::begin_immediate(&state.pool).await?;
    let now = now_rfc3339();
    let note = req.note.unwrap_or_default();
    let next_number: i64 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(number), 0) + 1 FROM document_versions WHERE document_id = ?",
    )
    .bind(&doc_id)
    .fetch_one(&mut *tx)
    .await?;

    let version_id = new_id();
    sqlx::query(
        "INSERT INTO document_versions
         (id, document_id, number, file_id, note, uploaded_by, uploaded_at, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&version_id)
    .bind(&doc_id)
    .bind(next_number)
    .bind(&req.file_id)
    .bind(&note)
    .bind(&actor.user_id)
    .bind(&now)
    .bind(&now)
    .execute(&mut *tx)
    .await?;

    audit::record(
        &mut tx,
        AuditEvent {
            actor_id: Some(actor.user_id.clone()),
            actor_label: actor.name.clone(),
            action: "document.version_added".into(),
            entity_type: "document_version".into(),
            entity_id: version_id.clone(),
            project_id: Some(project_id.clone()),
            visibility: "shared".into(),
            summary: format!(
                "{} added version {} to document {}",
                actor.name, next_number, doc_id
            ),
            before: None,
            after: Some(Value::Number(next_number.into())),
            reason: None,
        },
    )
    .await?;

    tx.commit().await?;

    let row: VersionRow = sqlx::query_as(
        "SELECT dv.id, dv.document_id, dv.number, dv.file_id, dv.note, dv.uploaded_by,
                dv.uploaded_at, f.scan_status, f.size, f.mime
         FROM document_versions dv
         JOIN files f ON f.id = dv.file_id
         WHERE dv.id = ?",
    )
    .bind(&version_id)
    .fetch_one(&state.pool)
    .await?;

    Ok((
        StatusCode::CREATED,
        Json(DocumentVersionDto {
            id: row.id,
            document_id: row.document_id,
            number: row.number,
            file_id: row.file_id,
            note: row.note,
            uploaded_by: row.uploaded_by,
            uploaded_at: row.uploaded_at,
            scan_status: row.scan_status,
            size: row.size,
            mime: row.mime,
        }),
    ))
}

async fn download_version(
    State(state): State<AppState>,
    actor: Actor,
    Path(version_id): Path<String>,
) -> AppResult<impl IntoResponse> {
    let row: Option<VersionDownloadRow> = sqlx::query_as(
        "SELECT d.id as document_id, d.project_id, d.category, d.title,
                f.sha256, f.size, f.mime, f.scan_status
         FROM document_versions dv
         JOIN documents d ON d.id = dv.document_id
         JOIN files f ON f.id = dv.file_id
         WHERE dv.id = ?",
    )
    .bind(&version_id)
    .fetch_optional(&state.pool)
    .await?;
    let row = row.ok_or(AppError::NotFound)?;

    let access = authz::project_access(&state.pool, &actor, &row.project_id).await?;
    if !authz::can_view_document_category(&actor, access, &row.category) {
        return Err(AppError::forbidden(
            "you do not have permission to download this document",
        ));
    }
    if row.scan_status != "clean" {
        return Err(AppError::conflict(
            "scan_not_clean",
            "file is pending antivirus scan or was rejected",
        ));
    }

    let (body, len) = crate::files::stream_file(&state.config.data_dir, &row.sha256).await?;
    let filename = safe_filename(&row.title);
    let disposition = format!("attachment; filename=\"{}\"", filename);

    let mut resp = Response::new(body);
    resp.headers_mut()
        .insert(header::CONTENT_LENGTH, HeaderValue::from(len));
    *resp.status_mut() = StatusCode::OK;
    resp.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(&row.mime).map_err(AppError::internal)?,
    );
    resp.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&disposition).map_err(AppError::internal)?,
    );
    resp.headers_mut().insert(
        header::HeaderName::from_static("x-content-type-options"),
        HeaderValue::from_static("nosniff"),
    );
    resp.headers_mut().insert(
        header::HeaderName::from_static("cache-control"),
        HeaderValue::from_static("private, no-store"),
    );
    Ok(resp)
}
