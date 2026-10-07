use std::collections::HashMap;

use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{Request, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, patch, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::Value;
use sqlx::FromRow;

use crate::AppState;
use crate::audit::{self, AuditEvent};
use crate::authz::{self, Actor};
use crate::db;
use crate::dto::{
    AdminCreateUserRequest, AdminPatchUserRequest, AuditEventDto, GrantRoleRequest, JobDto,
    ListResponse, SettingsDto, UserDto,
};
use crate::error::{AppError, AppResult};
use crate::routes::auth::load_user_dto;
use crate::util::{new_id, now_rfc3339};
use crate::validation::FieldErrors;

const SETTINGS_KEYS: &[&str] = &[
    "mail_enabled",
    "organisation_name",
    "reference_prefix",
    "public_catalog_enabled",
];

const ROLES: &[&str] = &[
    "coordinator",
    "expert",
    "decision_maker",
    "base_manager",
    "finance",
    "admin",
    "provider",
];

pub fn router(state: AppState) -> Router<AppState> {
    let admin_only = Router::new()
        .route("/admin/settings", get(get_settings).put(put_settings))
        .route("/admin/users", get(list_users).post(create_user))
        .route("/admin/users/{id}", patch(patch_user))
        .route("/admin/jobs", get(list_jobs))
        .route("/admin/jobs/{id}/retry", post(retry_job))
        .route("/admin/audit", get(list_audit))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            require_admin,
        ));

    let role_routes = Router::new()
        .route("/admin/users/{id}/roles", post(grant_role))
        .route("/admin/users/{id}/roles/{role}", delete(revoke_role))
        .layer(axum::middleware::from_fn_with_state(
            state,
            require_admin_or_decision_maker,
        ));

    admin_only.merge(role_routes)
}

async fn require_admin(
    State(_state): State<AppState>,
    actor: Actor,
    req: Request<Body>,
    next: Next,
) -> AppResult<Response> {
    authz::require_role(&actor, &["admin"])?;
    Ok(next.run(req).await)
}

async fn require_admin_or_decision_maker(
    State(_state): State<AppState>,
    actor: Actor,
    req: Request<Body>,
    next: Next,
) -> AppResult<Response> {
    if !actor.has_role("admin") && !actor.has_role("decision_maker") {
        return Err(AppError::Forbidden {
            code: "forbidden".into(),
            message: "requires role: admin or decision_maker".into(),
        });
    }
    Ok(next.run(req).await)
}

async fn settings_dto(pool: &sqlx::SqlitePool) -> AppResult<SettingsDto> {
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT key, value FROM settings WHERE key IN (?, ?, ?, ?)")
            .bind(SETTINGS_KEYS[0])
            .bind(SETTINGS_KEYS[1])
            .bind(SETTINGS_KEYS[2])
            .bind(SETTINGS_KEYS[3])
            .fetch_all(pool)
            .await?;
    let mut map = HashMap::new();
    for (k, v) in rows {
        map.insert(k, v);
    }
    Ok(SettingsDto {
        mail_enabled: map.get("mail_enabled").map(|s| s == "true").unwrap_or(true),
        organisation_name: map.get("organisation_name").cloned().unwrap_or_default(),
        reference_prefix: map
            .get("reference_prefix")
            .cloned()
            .unwrap_or_else(|| "PIT".into()),
        public_catalog_enabled: map
            .get("public_catalog_enabled")
            .map(|s| s == "true")
            .unwrap_or(false),
    })
}

async fn get_settings(
    State(state): State<AppState>,
    _actor: Actor,
) -> AppResult<Json<SettingsDto>> {
    Ok(Json(settings_dto(&state.pool).await?))
}

async fn put_settings(
    State(state): State<AppState>,
    _actor: Actor,
    Json(req): Json<SettingsDto>,
) -> AppResult<Json<SettingsDto>> {
    let mut errors = FieldErrors::new();
    errors.check(
        "reference_prefix",
        !req.reference_prefix.is_empty()
            && req.reference_prefix.len() <= 8
            && req.reference_prefix.chars().all(|c| c.is_ascii_uppercase()),
        "must be 1–8 uppercase ASCII letters",
    );
    errors.finish()?;

    let values: [(&str, String); 4] = [
        (
            "mail_enabled",
            if req.mail_enabled {
                "true".into()
            } else {
                "false".into()
            },
        ),
        ("organisation_name", req.organisation_name.clone()),
        ("reference_prefix", req.reference_prefix.clone()),
        (
            "public_catalog_enabled",
            if req.public_catalog_enabled {
                "true".into()
            } else {
                "false".into()
            },
        ),
    ];
    for (key, value) in values {
        sqlx::query(
            "INSERT INTO settings (key, value) VALUES (?, ?)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        )
        .bind(key)
        .bind(value)
        .execute(&state.pool)
        .await?;
    }
    Ok(Json(settings_dto(&state.pool).await?))
}

#[derive(Deserialize)]
struct AdminUsersQuery {
    q: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
}

async fn list_users(
    State(state): State<AppState>,
    _actor: Actor,
    Query(query): Query<AdminUsersQuery>,
) -> AppResult<Json<ListResponse<UserDto>>> {
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    let offset = query.offset.unwrap_or(0).max(0);

    let (total, ids) = if let Some(q) = &query.q {
        let like = format!("%{q}%");
        let total: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM users WHERE email LIKE ? OR LOWER(name) LIKE LOWER(?)",
        )
        .bind(&like)
        .bind(&like)
        .fetch_one(&state.pool)
        .await?;
        let ids: Vec<String> = sqlx::query_scalar(
            "SELECT id FROM users
             WHERE email LIKE ? OR LOWER(name) LIKE LOWER(?)
             ORDER BY created_at DESC LIMIT ? OFFSET ?",
        )
        .bind(&like)
        .bind(&like)
        .bind(limit)
        .bind(offset)
        .fetch_all(&state.pool)
        .await?;
        (total, ids)
    } else {
        let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
            .fetch_one(&state.pool)
            .await?;
        let ids: Vec<String> =
            sqlx::query_scalar("SELECT id FROM users ORDER BY created_at DESC LIMIT ? OFFSET ?")
                .bind(limit)
                .bind(offset)
                .fetch_all(&state.pool)
                .await?;
        (total, ids)
    };

    let mut items = Vec::new();
    for id in ids {
        items.push(load_user_dto(&state.pool, &id).await?);
    }
    Ok(Json(ListResponse { items, total }))
}

async fn create_user(
    State(state): State<AppState>,
    _actor: Actor,
    Json(req): Json<AdminCreateUserRequest>,
) -> AppResult<impl IntoResponse> {
    let mut errors = FieldErrors::new();
    errors.check(
        "email",
        req.email.contains('@') && req.email.len() <= 254,
        "must be a valid email at most 254 characters",
    );
    errors.require("name", &req.name, "name is required");
    errors.max_len("name", &req.name, 200);
    errors.check(
        "password",
        (8..=200).contains(&req.password.len()),
        "must be 8–200 characters",
    );
    errors.finish()?;

    let existing: Option<(String,)> = sqlx::query_as("SELECT id FROM users WHERE email = ?")
        .bind(&req.email)
        .fetch_optional(&state.pool)
        .await?;
    if existing.is_some() {
        return Err(AppError::conflict(
            "email_taken",
            "an account with this email already exists",
        ));
    }

    let user_id = new_id();
    let now = now_rfc3339();
    let password_hash = crate::password::hash_password(&req.password)?;
    sqlx::query(
        "INSERT INTO users (id, email, name, organisation, password_hash, created_at)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(&user_id)
    .bind(&req.email)
    .bind(&req.name)
    .bind(&req.organisation)
    .bind(&password_hash)
    .bind(&now)
    .execute(&state.pool)
    .await?;

    let dto = load_user_dto(&state.pool, &user_id).await?;
    Ok((StatusCode::CREATED, Json(dto)))
}

async fn patch_user(
    State(state): State<AppState>,
    _actor: Actor,
    Path(user_id): Path<String>,
    Json(req): Json<AdminPatchUserRequest>,
) -> AppResult<Json<UserDto>> {
    let exists: Option<(String,)> = sqlx::query_as("SELECT id FROM users WHERE id = ?")
        .bind(&user_id)
        .fetch_optional(&state.pool)
        .await?;
    if exists.is_none() {
        return Err(AppError::NotFound);
    }

    let protected: Option<(String,)> = sqlx::query_as(
        "SELECT user_id FROM user_roles
         WHERE user_id = ? AND role = 'decision_maker' AND revoked_at IS NULL",
    )
    .bind(&user_id)
    .fetch_optional(&state.pool)
    .await?;
    if protected.is_some() {
        return Err(AppError::conflict(
            "protected_user",
            "decision_maker users are protected",
        ));
    }

    if let Some(email) = &req.email {
        let taken: Option<(String,)> =
            sqlx::query_as("SELECT id FROM users WHERE email = ? AND id != ?")
                .bind(email)
                .bind(&user_id)
                .fetch_optional(&state.pool)
                .await?;
        if taken.is_some() {
            return Err(AppError::conflict("email_taken", "email already in use"));
        }
        sqlx::query("UPDATE users SET email = ? WHERE id = ?")
            .bind(email)
            .bind(&user_id)
            .execute(&state.pool)
            .await?;
    }
    if let Some(name) = req.name {
        sqlx::query("UPDATE users SET name = ? WHERE id = ?")
            .bind(&name)
            .bind(&user_id)
            .execute(&state.pool)
            .await?;
    }
    if let Some(organisation) = req.organisation {
        sqlx::query("UPDATE users SET organisation = ? WHERE id = ?")
            .bind(&organisation)
            .bind(&user_id)
            .execute(&state.pool)
            .await?;
    }
    if let Some(disabled) = req.disabled {
        if disabled {
            let now = now_rfc3339();
            sqlx::query("UPDATE users SET disabled_at = ? WHERE id = ?")
                .bind(&now)
                .bind(&user_id)
                .execute(&state.pool)
                .await?;
            sqlx::query("DELETE FROM sessions WHERE user_id = ?")
                .bind(&user_id)
                .execute(&state.pool)
                .await?;
        } else {
            sqlx::query("UPDATE users SET disabled_at = NULL WHERE id = ?")
                .bind(&user_id)
                .execute(&state.pool)
                .await?;
        }
    }

    Ok(Json(load_user_dto(&state.pool, &user_id).await?))
}

fn validate_role(role: &str) -> AppResult<()> {
    if ROLES.contains(&role) {
        Ok(())
    } else {
        Err(AppError::unprocessable(
            "invalid_role",
            format!("role must be one of {}", ROLES.join(", ")),
        ))
    }
}

async fn grant_role(
    State(state): State<AppState>,
    actor: Actor,
    Path(user_id): Path<String>,
    Json(req): Json<GrantRoleRequest>,
) -> AppResult<impl IntoResponse> {
    validate_role(&req.role)?;

    let exists: Option<(String,)> = sqlx::query_as("SELECT id FROM users WHERE id = ?")
        .bind(&user_id)
        .fetch_optional(&state.pool)
        .await?;
    if exists.is_none() {
        return Err(AppError::NotFound);
    }

    if req.role == "decision_maker" && !actor.has_role("decision_maker") {
        return Err(AppError::Forbidden {
            code: "cannot_grant_decision_maker".into(),
            message: "only a decision_maker can grant decision_maker".into(),
        });
    }

    let active: Option<(String,)> = sqlx::query_as(
        "SELECT user_id FROM user_roles
         WHERE user_id = ? AND role = ? AND revoked_at IS NULL",
    )
    .bind(&user_id)
    .bind(&req.role)
    .fetch_optional(&state.pool)
    .await?;
    if active.is_some() {
        return Ok((
            StatusCode::OK,
            Json(load_user_dto(&state.pool, &user_id).await?),
        ));
    }

    let mut tx = db::begin_immediate(&state.pool).await?;
    let now = now_rfc3339();
    sqlx::query(
        "INSERT INTO user_roles (user_id, role, granted_by, granted_at)
         VALUES (?, ?, ?, ?)",
    )
    .bind(&user_id)
    .bind(&req.role)
    .bind(&actor.user_id)
    .bind(&now)
    .execute(&mut *tx)
    .await?;

    audit::record(
        &mut tx,
        AuditEvent {
            actor_id: Some(actor.user_id.clone()),
            actor_label: actor.name.clone(),
            action: "role.granted".into(),
            entity_type: "user".into(),
            entity_id: user_id.clone(),
            project_id: None,
            visibility: "internal".into(),
            summary: format!("{} granted role {}", actor.name, req.role),
            before: None,
            after: Some(Value::String(req.role.clone())),
            reason: None,
        },
    )
    .await?;

    tx.commit().await?;
    Ok((
        StatusCode::OK,
        Json(load_user_dto(&state.pool, &user_id).await?),
    ))
}

async fn revoke_role(
    State(state): State<AppState>,
    actor: Actor,
    Path((user_id, role)): Path<(String, String)>,
) -> AppResult<Json<UserDto>> {
    validate_role(&role)?;

    if role == "decision_maker" && !actor.has_role("decision_maker") {
        return Err(AppError::Forbidden {
            code: "protected_role".into(),
            message: "only a decision_maker can revoke decision_maker".into(),
        });
    }

    let mut tx = db::begin_immediate(&state.pool).await?;
    let now = now_rfc3339();
    let updated = sqlx::query(
        "UPDATE user_roles SET revoked_at = ?
         WHERE user_id = ? AND role = ? AND revoked_at IS NULL",
    )
    .bind(&now)
    .bind(&user_id)
    .bind(&role)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if updated == 0 {
        return Err(AppError::NotFound);
    }

    audit::record(
        &mut tx,
        AuditEvent {
            actor_id: Some(actor.user_id.clone()),
            actor_label: actor.name.clone(),
            action: "role.revoked".into(),
            entity_type: "user".into(),
            entity_id: user_id.clone(),
            project_id: None,
            visibility: "internal".into(),
            summary: format!("{} revoked role {}", actor.name, role),
            before: Some(Value::String(role.clone())),
            after: None,
            reason: None,
        },
    )
    .await?;

    tx.commit().await?;
    Ok(Json(load_user_dto(&state.pool, &user_id).await?))
}

#[derive(Deserialize)]
struct ListJobsQuery {
    status: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
}

#[derive(FromRow)]
#[allow(dead_code)]
struct JobRow {
    id: String,
    kind: String,
    payload_json: String,
    dedupe_key: Option<String>,
    status: String,
    attempts: i64,
    max_attempts: i64,
    run_after: String,
    last_error: Option<String>,
    locked_until: Option<String>,
    created_at: String,
}

fn job_dto(row: JobRow) -> AppResult<JobDto> {
    Ok(JobDto {
        id: row.id,
        kind: row.kind,
        payload: serde_json::from_str(&row.payload_json).map_err(AppError::internal)?,
        dedupe_key: row.dedupe_key,
        status: row.status,
        attempts: row.attempts,
        max_attempts: row.max_attempts,
        run_after: row.run_after,
        last_error: row.last_error,
        locked_until: row.locked_until,
        created_at: row.created_at,
    })
}

async fn load_job(pool: &sqlx::SqlitePool, id: &str) -> AppResult<JobDto> {
    let row: JobRow = sqlx::query_as(
        "SELECT id, kind, payload_json, dedupe_key, status, attempts, max_attempts,
                run_after, last_error, locked_until, created_at
         FROM jobs WHERE id = ?",
    )
    .bind(id)
    .fetch_one(pool)
    .await?;
    job_dto(row)
}

async fn list_jobs(
    State(state): State<AppState>,
    _actor: Actor,
    Query(query): Query<ListJobsQuery>,
) -> AppResult<Json<ListResponse<JobDto>>> {
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    let offset = query.offset.unwrap_or(0).max(0);

    let (total, rows) = if let Some(status) = &query.status {
        let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM jobs WHERE status = ?")
            .bind(status)
            .fetch_one(&state.pool)
            .await?;
        let rows: Vec<JobRow> = sqlx::query_as(
            "SELECT id, kind, payload_json, dedupe_key, status, attempts, max_attempts,
                    run_after, last_error, locked_until, created_at
             FROM jobs WHERE status = ?
             ORDER BY created_at DESC LIMIT ? OFFSET ?",
        )
        .bind(status)
        .bind(limit)
        .bind(offset)
        .fetch_all(&state.pool)
        .await?;
        (total, rows)
    } else {
        let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM jobs")
            .fetch_one(&state.pool)
            .await?;
        let rows: Vec<JobRow> = sqlx::query_as(
            "SELECT id, kind, payload_json, dedupe_key, status, attempts, max_attempts,
                    run_after, last_error, locked_until, created_at
             FROM jobs
             ORDER BY created_at DESC LIMIT ? OFFSET ?",
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(&state.pool)
        .await?;
        (total, rows)
    };

    let mut items = Vec::new();
    for row in rows {
        items.push(job_dto(row)?);
    }
    Ok(Json(ListResponse { items, total }))
}

async fn retry_job(
    State(state): State<AppState>,
    _actor: Actor,
    Path(id): Path<String>,
) -> AppResult<Json<JobDto>> {
    let status: Option<String> = sqlx::query_scalar("SELECT status FROM jobs WHERE id = ?")
        .bind(&id)
        .fetch_optional(&state.pool)
        .await?;
    let status = status.ok_or(AppError::NotFound)?;
    if !matches!(status.as_str(), "failed" | "dead") {
        return Err(AppError::conflict(
            "job_not_retryable",
            "job cannot be retried",
        ));
    }

    let now = now_rfc3339();
    sqlx::query(
        "UPDATE jobs SET status = 'queued', attempts = 0, run_after = ?, last_error = NULL,
         locked_until = NULL WHERE id = ?",
    )
    .bind(&now)
    .bind(&id)
    .execute(&state.pool)
    .await?;

    Ok(Json(load_job(&state.pool, &id).await?))
}

#[derive(Deserialize)]
struct ListAuditQuery {
    project_id: Option<String>,
    entity_type: Option<String>,
    entity_id: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
}

#[derive(FromRow)]
#[allow(dead_code)]
struct AuditEventRow {
    id: String,
    at: String,
    actor_id: Option<String>,
    actor_label: String,
    action: String,
    entity_type: String,
    entity_id: String,
    project_id: Option<String>,
    visibility: String,
    summary: String,
    before_json: Option<String>,
    after_json: Option<String>,
    reason: Option<String>,
}

fn opt_json(s: Option<String>) -> Option<Value> {
    s.and_then(|v| serde_json::from_str(&v).ok())
}

fn audit_event_dto(row: AuditEventRow) -> AuditEventDto {
    AuditEventDto {
        id: row.id,
        at: row.at,
        actor_id: row.actor_id,
        actor_label: row.actor_label,
        action: row.action,
        entity_type: row.entity_type,
        entity_id: row.entity_id,
        project_id: row.project_id,
        visibility: row.visibility,
        summary: row.summary,
        before: opt_json(row.before_json),
        after: opt_json(row.after_json),
        reason: row.reason,
    }
}

async fn list_audit(
    State(state): State<AppState>,
    _actor: Actor,
    Query(query): Query<ListAuditQuery>,
) -> AppResult<Json<ListResponse<AuditEventDto>>> {
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    let offset = query.offset.unwrap_or(0).max(0);

    let mut sql =
        "SELECT id, at, actor_id, actor_label, action, entity_type, entity_id, project_id,
                visibility, summary, before_json, after_json, reason
         FROM audit_events WHERE 1=1"
            .to_string();
    let mut count_sql = "SELECT COUNT(*) FROM audit_events WHERE 1=1".to_string();
    let mut binds: Vec<String> = Vec::new();
    if let Some(pid) = &query.project_id {
        sql.push_str(" AND project_id = ?");
        count_sql.push_str(" AND project_id = ?");
        binds.push(pid.clone());
    }
    if let Some(et) = &query.entity_type {
        sql.push_str(" AND entity_type = ?");
        count_sql.push_str(" AND entity_type = ?");
        binds.push(et.clone());
    }
    if let Some(eid) = &query.entity_id {
        sql.push_str(" AND entity_id = ?");
        count_sql.push_str(" AND entity_id = ?");
        binds.push(eid.clone());
    }
    sql.push_str(" ORDER BY at DESC LIMIT ? OFFSET ?");

    let mut total_query = sqlx::query_scalar::<_, i64>(&count_sql);
    for v in &binds {
        total_query = total_query.bind(v);
    }
    let total = total_query.fetch_one(&state.pool).await?;

    let mut rows_query = sqlx::query_as::<_, AuditEventRow>(&sql);
    for v in &binds {
        rows_query = rows_query.bind(v);
    }
    rows_query = rows_query.bind(limit).bind(offset);
    let rows: Vec<AuditEventRow> = rows_query.fetch_all(&state.pool).await?;

    let items = rows.into_iter().map(audit_event_dto).collect();
    Ok(Json(ListResponse { items, total }))
}
