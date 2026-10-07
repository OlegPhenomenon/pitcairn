//! Application form templates (architecture §4 Templates, §5).
//! Reading is open to any authenticated user (researchers pick a template to
//! start a project); creating drafts and publishing is admin-only.
//! Published versions are immutable — a revision always renders with the
//! schema it was submitted under.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use serde_json::Value;
use sqlx::FromRow;

use crate::AppState;
use crate::audit::{self, AuditEvent};
use crate::authz::{self, Actor};
use crate::db;
use crate::dto::ListResponse;
use crate::dto::a::{TemplateDto, TemplateSchemaRequest, TemplateVersionDto};
use crate::error::{AppError, AppResult};
use crate::util::{new_id, now_rfc3339};
use crate::validation::FieldErrors;

/// The fixed field-type palette for template schemas (§4).
pub const FIELD_TYPES: &[&str] = &[
    "text",
    "textarea",
    "date",
    "daterange",
    "number",
    "select",
    "multiselect",
    "people",
    "checkbox",
    "sites",
];

const DOCUMENT_CATEGORIES: &[&str] = &["application", "personal", "decision", "result", "other"];

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/templates", get(list_templates))
        .route(
            "/templates/{key}/versions",
            get(list_versions).post(create_version),
        )
        .route("/template-versions/{id}", put(update_version))
        .route("/template-versions/{id}/publish", post(publish_version))
}

/// Validate `schema_json` shape and the fixed field-type palette.
/// Returns 422 with per-field errors keyed `sections[i].fields[j].<attr>`.
/// Shared by the template routes; schema *reading* helpers used on submit
/// live in `template_schema` below.
pub fn validate_template_schema(schema: &Value) -> AppResult<()> {
    let mut errors = FieldErrors::new();
    let mut field_keys: Vec<String> = Vec::new();

    match schema.get("sections").and_then(|s| s.as_array()) {
        None => errors.check("sections", false, "must be an array of sections"),
        Some(sections) => {
            let mut section_keys: Vec<&str> = Vec::new();
            for (i, section) in sections.iter().enumerate() {
                let prefix = format!("sections[{i}]");
                let key = section.get("key").and_then(|k| k.as_str()).unwrap_or("");
                errors.require(&format!("{prefix}.key"), key, "key is required");
                if !key.is_empty() && section_keys.contains(&key) {
                    errors.check(
                        &format!("{prefix}.key"),
                        false,
                        "section keys must be unique",
                    );
                }
                section_keys.push(key);
                errors.require(
                    &format!("{prefix}.title"),
                    section.get("title").and_then(|t| t.as_str()).unwrap_or(""),
                    "title is required",
                );
                match section.get("fields").and_then(|f| f.as_array()) {
                    None => errors.check(
                        &format!("{prefix}.fields"),
                        false,
                        "must be an array of fields",
                    ),
                    Some(fields) => {
                        for (j, field) in fields.iter().enumerate() {
                            let fp = format!("{prefix}.fields[{j}]");
                            let fkey = field.get("key").and_then(|k| k.as_str()).unwrap_or("");
                            errors.require(&format!("{fp}.key"), fkey, "key is required");
                            if !fkey.is_empty() && field_keys.iter().any(|k| k == fkey) {
                                errors.check(
                                    &format!("{fp}.key"),
                                    false,
                                    "field keys must be unique across the schema",
                                );
                            }
                            field_keys.push(fkey.to_string());
                            errors.require(
                                &format!("{fp}.label"),
                                field.get("label").and_then(|l| l.as_str()).unwrap_or(""),
                                "label is required",
                            );
                            let ftype = field.get("type").and_then(|t| t.as_str()).unwrap_or("");
                            errors.check(
                                &format!("{fp}.type"),
                                FIELD_TYPES.contains(&ftype),
                                &format!("must be one of {}", FIELD_TYPES.join(", ")),
                            );
                            if let Some(req) = field.get("required") {
                                errors.check(
                                    &format!("{fp}.required"),
                                    req.is_boolean(),
                                    "must be a boolean",
                                );
                            }
                            if matches!(ftype, "select" | "multiselect") {
                                let ok = field
                                    .get("options")
                                    .and_then(|o| o.as_array())
                                    .map(|o| !o.is_empty() && o.iter().all(|v| v.is_string()))
                                    .unwrap_or(false);
                                errors.check(
                                    &format!("{fp}.options"),
                                    ok,
                                    "select/multiselect fields require a non-empty options array of strings",
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    match schema.get("required_documents") {
        None => {}
        Some(v) => match v.as_array() {
            None => errors.check(
                "required_documents",
                false,
                "must be an array of document slots",
            ),
            Some(docs) => {
                let mut keys: Vec<&str> = Vec::new();
                for (i, doc) in docs.iter().enumerate() {
                    let prefix = format!("required_documents[{i}]");
                    let key = doc.get("key").and_then(|k| k.as_str()).unwrap_or("");
                    errors.require(&format!("{prefix}.key"), key, "key is required");
                    if !key.is_empty() && keys.contains(&key) {
                        errors.check(
                            &format!("{prefix}.key"),
                            false,
                            "required document keys must be unique",
                        );
                    }
                    keys.push(key);
                    errors.require(
                        &format!("{prefix}.label"),
                        doc.get("label").and_then(|l| l.as_str()).unwrap_or(""),
                        "label is required",
                    );
                    let cat = doc
                        .get("category")
                        .and_then(|c| c.as_str())
                        .unwrap_or("application");
                    errors.check(
                        &format!("{prefix}.category"),
                        DOCUMENT_CATEGORIES.contains(&cat),
                        &format!("must be one of {}", DOCUMENT_CATEGORIES.join(", ")),
                    );
                }
            }
        },
    }

    errors.finish()
}

/// Iterate `(field_key, field_def)` over a template schema's fields in order.
pub fn schema_fields(schema: &Value) -> Vec<(String, &Value)> {
    let mut out = Vec::new();
    if let Some(sections) = schema.get("sections").and_then(|s| s.as_array()) {
        for section in sections {
            if let Some(fields) = section.get("fields").and_then(|f| f.as_array()) {
                for field in fields {
                    if let Some(key) = field.get("key").and_then(|k| k.as_str()) {
                        out.push((key.to_string(), field));
                    }
                }
            }
        }
    }
    out
}

/// `(key, label, category)` for each required document slot of a schema.
pub fn required_document_slots(schema: &Value) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    if let Some(docs) = schema.get("required_documents").and_then(|d| d.as_array()) {
        for doc in docs {
            let key = doc.get("key").and_then(|k| k.as_str()).unwrap_or_default();
            if key.is_empty() {
                continue;
            }
            let label = doc
                .get("label")
                .and_then(|l| l.as_str())
                .unwrap_or(key)
                .to_string();
            let category = doc
                .get("category")
                .and_then(|c| c.as_str())
                .unwrap_or("application")
                .to_string();
            out.push((key.to_string(), label, category));
        }
    }
    out
}

// ---------------------------------------------------------------------------

#[derive(FromRow)]
struct TemplateRow {
    id: String,
    key: String,
    name: String,
    description: String,
    created_at: String,
}

#[derive(FromRow)]
struct VersionRow {
    id: String,
    template_id: String,
    version: i64,
    schema_json: String,
    status: String,
    published_at: Option<String>,
    published_by: Option<String>,
    created_at: String,
}

fn version_dto(row: VersionRow, template_key: &str) -> AppResult<TemplateVersionDto> {
    Ok(TemplateVersionDto {
        id: row.id,
        template_id: row.template_id,
        template_key: template_key.to_string(),
        version: row.version,
        status: row.status,
        schema: serde_json::from_str(&row.schema_json).map_err(AppError::internal)?,
        published_at: row.published_at,
        published_by: row.published_by,
        created_at: row.created_at,
    })
}

async fn list_templates(
    State(state): State<AppState>,
    _actor: Actor,
) -> AppResult<Json<ListResponse<TemplateDto>>> {
    let rows: Vec<TemplateRow> =
        sqlx::query_as("SELECT id, key, name, description, created_at FROM templates ORDER BY key")
            .fetch_all(&state.pool)
            .await?;
    let mut items = Vec::new();
    for row in rows {
        let latest_published: Option<i64> = sqlx::query_scalar(
            "SELECT MAX(version) FROM template_versions
             WHERE template_id = ? AND status = 'published'",
        )
        .bind(&row.id)
        .fetch_one(&state.pool)
        .await?;
        let draft: Option<i64> = sqlx::query_scalar(
            "SELECT MAX(version) FROM template_versions
             WHERE template_id = ? AND status = 'draft'",
        )
        .bind(&row.id)
        .fetch_one(&state.pool)
        .await?;
        items.push(TemplateDto {
            id: row.id,
            key: row.key,
            name: row.name,
            description: row.description,
            latest_published_version: latest_published,
            draft_version: draft,
            created_at: row.created_at,
        });
    }
    let total = items.len() as i64;
    Ok(Json(ListResponse { items, total }))
}

async fn list_versions(
    State(state): State<AppState>,
    _actor: Actor,
    Path(key): Path<String>,
) -> AppResult<Json<ListResponse<TemplateVersionDto>>> {
    let template: Option<(String,)> = sqlx::query_as("SELECT id FROM templates WHERE key = ?")
        .bind(&key)
        .fetch_optional(&state.pool)
        .await?;
    let (template_id,) = template.ok_or(AppError::NotFound)?;

    let rows: Vec<VersionRow> = sqlx::query_as(
        "SELECT id, template_id, version, schema_json, status, published_at, published_by, created_at
         FROM template_versions WHERE template_id = ? ORDER BY version DESC",
    )
    .bind(&template_id)
    .fetch_all(&state.pool)
    .await?;
    let mut items = Vec::new();
    for row in rows {
        items.push(version_dto(row, &key)?);
    }
    let total = items.len() as i64;
    Ok(Json(ListResponse { items, total }))
}

async fn create_version(
    State(state): State<AppState>,
    actor: Actor,
    Path(key): Path<String>,
    Json(req): Json<TemplateSchemaRequest>,
) -> AppResult<impl IntoResponse> {
    authz::require_role(&actor, &["admin"])?;
    validate_template_schema(&req.schema)?;

    let template: Option<(String,)> = sqlx::query_as("SELECT id FROM templates WHERE key = ?")
        .bind(&key)
        .fetch_optional(&state.pool)
        .await?;
    let (template_id,) = template.ok_or(AppError::NotFound)?;

    let mut tx = db::begin_immediate(&state.pool).await?;
    let version: i64 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(version), 0) + 1 FROM template_versions WHERE template_id = ?",
    )
    .bind(&template_id)
    .fetch_one(&mut *tx)
    .await?;
    let id = new_id();
    let now = now_rfc3339();
    sqlx::query(
        "INSERT INTO template_versions
         (id, template_id, version, schema_json, status, created_at)
         VALUES (?, ?, ?, ?, 'draft', ?)",
    )
    .bind(&id)
    .bind(&template_id)
    .bind(version)
    .bind(req.schema.to_string())
    .bind(&now)
    .execute(&mut *tx)
    .await?;

    audit::record(
        &mut tx,
        AuditEvent {
            actor_id: Some(actor.user_id.clone()),
            actor_label: actor.name.clone(),
            action: "template_version.created".into(),
            entity_type: "template_version".into(),
            entity_id: id.clone(),
            project_id: None,
            visibility: "internal".into(),
            summary: format!("{} created draft v{version} of template {key}", actor.name),
            before: None,
            after: Some(req.schema.clone()),
            reason: None,
        },
    )
    .await?;

    tx.commit().await?;

    let row: VersionRow = sqlx::query_as(
        "SELECT id, template_id, version, schema_json, status, published_at, published_by, created_at
         FROM template_versions WHERE id = ?",
    )
    .bind(&id)
    .fetch_one(&state.pool)
    .await?;
    Ok((StatusCode::CREATED, Json(version_dto(row, &key)?)))
}

async fn update_version(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<String>,
    Json(req): Json<TemplateSchemaRequest>,
) -> AppResult<Json<TemplateVersionDto>> {
    authz::require_role(&actor, &["admin"])?;
    validate_template_schema(&req.schema)?;

    let mut tx = db::begin_immediate(&state.pool).await?;
    let row: Option<(String, String)> = sqlx::query_as(
        "SELECT tv.status, t.key FROM template_versions tv
         JOIN templates t ON t.id = tv.template_id WHERE tv.id = ?",
    )
    .bind(&id)
    .fetch_optional(&mut *tx)
    .await?;
    let (status, key) = row.ok_or(AppError::NotFound)?;
    if status != "draft" {
        return Err(AppError::conflict(
            "version_immutable",
            "only draft template versions can be edited",
        ));
    }
    sqlx::query("UPDATE template_versions SET schema_json = ? WHERE id = ?")
        .bind(req.schema.to_string())
        .bind(&id)
        .execute(&mut *tx)
        .await?;

    audit::record(
        &mut tx,
        AuditEvent {
            actor_id: Some(actor.user_id.clone()),
            actor_label: actor.name.clone(),
            action: "template_version.updated".into(),
            entity_type: "template_version".into(),
            entity_id: id.clone(),
            project_id: None,
            visibility: "internal".into(),
            summary: format!("{} edited draft of template {key}", actor.name),
            before: None,
            after: Some(req.schema.clone()),
            reason: None,
        },
    )
    .await?;
    tx.commit().await?;

    let row: VersionRow = sqlx::query_as(
        "SELECT id, template_id, version, schema_json, status, published_at, published_by, created_at
         FROM template_versions WHERE id = ?",
    )
    .bind(&id)
    .fetch_one(&state.pool)
    .await?;
    Ok(Json(version_dto(row, &key)?))
}

async fn publish_version(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<String>,
) -> AppResult<Json<TemplateVersionDto>> {
    authz::require_role(&actor, &["admin"])?;

    let mut tx = db::begin_immediate(&state.pool).await?;
    let row: Option<(String, String, i64)> = sqlx::query_as(
        "SELECT tv.template_id, tv.status, tv.version FROM template_versions tv WHERE tv.id = ?",
    )
    .bind(&id)
    .fetch_optional(&mut *tx)
    .await?;
    let (template_id, status, version) = row.ok_or(AppError::NotFound)?;
    if status != "draft" {
        return Err(AppError::conflict(
            "version_immutable",
            "only draft template versions can be published",
        ));
    }

    let now = now_rfc3339();
    // The previous published version stays readable but is retired for new
    // projects; it is never mutated (revisions render with their own schema).
    sqlx::query(
        "UPDATE template_versions SET status = 'retired'
         WHERE template_id = ? AND status = 'published'",
    )
    .bind(&template_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE template_versions SET status = 'published', published_at = ?, published_by = ?
         WHERE id = ?",
    )
    .bind(&now)
    .bind(&actor.user_id)
    .bind(&id)
    .execute(&mut *tx)
    .await?;

    audit::record(
        &mut tx,
        AuditEvent {
            actor_id: Some(actor.user_id.clone()),
            actor_label: actor.name.clone(),
            action: "template_version.published".into(),
            entity_type: "template_version".into(),
            entity_id: id.clone(),
            project_id: None,
            visibility: "internal".into(),
            summary: format!("{} published v{version} of a template", actor.name),
            before: None,
            after: None,
            reason: None,
        },
    )
    .await?;

    tx.commit().await?;

    let row: (VersionRow, String) = sqlx::query_as::<_, (String, String, i64, String, String, Option<String>, Option<String>, String, String)>(
        "SELECT tv.id, tv.template_id, tv.version, tv.schema_json, tv.status, tv.published_at, tv.published_by, tv.created_at, t.key
         FROM template_versions tv JOIN templates t ON t.id = tv.template_id WHERE tv.id = ?",
    )
    .bind(&id)
    .fetch_one(&state.pool)
    .await
    .map(|(id, template_id, version, schema_json, status, published_at, published_by, created_at, key)| {
        (
            VersionRow {
                id,
                template_id,
                version,
                schema_json,
                status,
                published_at,
                published_by,
                created_at,
            },
            key,
        )
    })?;
    Ok(Json(version_dto(row.0, &row.1)?))
}
