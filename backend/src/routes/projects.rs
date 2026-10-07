use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::Value;
use sqlx::FromRow;

use crate::AppState;
use crate::audit::{self, AuditEvent};
use crate::authz::{self, Actor, ProjectAccess};
use crate::db;
use crate::dto::{CreateProjectRequest, ListQuery, ListResponse, ProjectDto, ProjectListItemDto};
use crate::error::{AppError, AppResult};
use crate::util::{new_id, now_rfc3339};
use crate::validation::FieldErrors;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/projects", post(create_project).get(list_projects))
        .route("/projects/{id}", get(get_project))
}

fn access_str(access: ProjectAccess) -> &'static str {
    match access {
        ProjectAccess::None => "none",
        ProjectAccess::Public => "public",
        ProjectAccess::Expert => "expert",
        ProjectAccess::TeamViewer => "team_viewer",
        ProjectAccess::TeamEditor => "team_editor",
        ProjectAccess::TeamLead => "team_lead",
        ProjectAccess::Staff => "staff",
    }
}

#[derive(FromRow)]
#[allow(dead_code)]
struct ProjectRow {
    id: String,
    reference: Option<String>,
    title: String,
    summary: String,
    keywords: String,
    organisation: String,
    status: String,
    template_version_id: String,
    answers_json: String,
    start_date: Option<String>,
    end_date: Option<String>,
    version: i64,
    created_by: String,
    created_at: String,
}

#[derive(FromRow)]
#[allow(dead_code)]
struct ProjectListRow {
    id: String,
    reference: Option<String>,
    title: String,
    status: String,
    organisation: String,
    created_at: String,
    my_role: Option<String>,
}

async fn load_project_dto(
    state: &AppState,
    actor: &Actor,
    project_id: &str,
) -> AppResult<ProjectDto> {
    let row: ProjectRow = sqlx::query_as(
        "SELECT id, reference, title, summary, keywords, organisation, status,
                template_version_id, answers_json, start_date, end_date, version,
                created_by, created_at
         FROM projects WHERE id = ?",
    )
    .bind(project_id)
    .fetch_one(&state.pool)
    .await?;
    let access = authz::project_access(&state.pool, actor, project_id).await?;
    Ok(ProjectDto {
        id: row.id,
        reference: row.reference,
        title: row.title,
        summary: row.summary,
        keywords: row.keywords,
        organisation: row.organisation,
        status: row.status,
        template_version_id: row.template_version_id,
        answers: serde_json::from_str(&row.answers_json).map_err(AppError::internal)?,
        start_date: row.start_date,
        end_date: row.end_date,
        version: row.version,
        created_by: row.created_by,
        created_at: row.created_at,
        my_access: access_str(access).into(),
    })
}

async fn create_project(
    State(state): State<AppState>,
    actor: Actor,
    Json(req): Json<CreateProjectRequest>,
) -> AppResult<impl IntoResponse> {
    let mut errors = FieldErrors::new();
    errors.require(
        "template_key",
        &req.template_key,
        "template_key is required",
    );
    errors.require("title", &req.title, "title is required");
    errors.max_len("title", &req.title, 300);
    errors.finish()?;

    let tv: Option<(String,)> = sqlx::query_as(
        "SELECT tv.id FROM template_versions tv
         JOIN templates t ON t.id = tv.template_id
         WHERE t.key = ? AND tv.status = 'published'
         ORDER BY tv.version DESC LIMIT 1",
    )
    .bind(&req.template_key)
    .fetch_optional(&state.pool)
    .await?;
    let (template_version_id,) = tv.ok_or_else(|| {
        AppError::unprocessable("unknown_template", "no published version for template")
    })?;

    let actor_org: String = sqlx::query_scalar("SELECT organisation FROM users WHERE id = ?")
        .bind(&actor.user_id)
        .fetch_one(&state.pool)
        .await?;

    let mut tx = db::begin_immediate(&state.pool).await?;
    let project_id = new_id();
    let now = now_rfc3339();
    sqlx::query(
        "INSERT INTO projects
         (id, title, summary, keywords, organisation, template_version_id, answers_json,
          status, start_date, end_date, version, created_by, created_at)
         VALUES (?, ?, '', '', ?, ?, '{}', 'draft', NULL, NULL, 1, ?, ?)",
    )
    .bind(&project_id)
    .bind(&req.title)
    .bind(&actor_org)
    .bind(&template_version_id)
    .bind(&actor.user_id)
    .bind(&now)
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        "INSERT INTO project_members (id, project_id, user_id, role, added_by, added_at)
         VALUES (?, ?, ?, 'lead', ?, ?)",
    )
    .bind(new_id())
    .bind(&project_id)
    .bind(&actor.user_id)
    .bind(&actor.user_id)
    .bind(&now)
    .execute(&mut *tx)
    .await?;

    audit::record(
        &mut tx,
        AuditEvent {
            actor_id: Some(actor.user_id.clone()),
            actor_label: actor.name.clone(),
            action: "project.created".into(),
            entity_type: "project".into(),
            entity_id: project_id.clone(),
            project_id: Some(project_id.clone()),
            visibility: "shared".into(),
            summary: format!("{} created project {}", actor.name, req.title),
            before: None,
            after: Some(Value::String(req.title.clone())),
            reason: None,
        },
    )
    .await?;

    tx.commit().await?;
    let dto = load_project_dto(&state, &actor, &project_id).await?;
    Ok((StatusCode::CREATED, Json(dto)))
}

async fn list_projects(
    State(state): State<AppState>,
    actor: Actor,
    Query(query): Query<ListQuery>,
) -> AppResult<Json<ListResponse<ProjectListItemDto>>> {
    let limit = query.limit();
    let offset = query.offset();

    let (total, items) = if actor.is_staff() {
        let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM projects")
            .fetch_one(&state.pool)
            .await?;
        let rows: Vec<ProjectListRow> = sqlx::query_as(
            "SELECT p.id, p.reference, p.title, p.status, p.organisation, p.created_at,
                    (SELECT role FROM project_members
                     WHERE project_id = p.id AND user_id = ? AND removed_at IS NULL) as my_role
             FROM projects p
             ORDER BY p.created_at DESC
             LIMIT ? OFFSET ?",
        )
        .bind(&actor.user_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(&state.pool)
        .await?;
        let items = rows
            .into_iter()
            .map(|r| ProjectListItemDto {
                id: r.id,
                reference: r.reference,
                title: r.title,
                status: r.status,
                organisation: r.organisation,
                created_at: r.created_at,
                my_role: r.my_role,
            })
            .collect();
        (total, items)
    } else {
        let total: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM project_members pm
             JOIN projects p ON p.id = pm.project_id
             WHERE pm.user_id = ? AND pm.removed_at IS NULL",
        )
        .bind(&actor.user_id)
        .fetch_one(&state.pool)
        .await?;
        let rows: Vec<ProjectListRow> = sqlx::query_as(
            "SELECT p.id, p.reference, p.title, p.status, p.organisation, p.created_at, pm.role as my_role
             FROM projects p
             JOIN project_members pm ON pm.project_id = p.id
             WHERE pm.user_id = ? AND pm.removed_at IS NULL
             ORDER BY p.created_at DESC
             LIMIT ? OFFSET ?",
        )
        .bind(&actor.user_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(&state.pool)
        .await?;
        let items = rows
            .into_iter()
            .map(|r| ProjectListItemDto {
                id: r.id,
                reference: r.reference,
                title: r.title,
                status: r.status,
                organisation: r.organisation,
                created_at: r.created_at,
                my_role: r.my_role,
            })
            .collect();
        (total, items)
    };

    Ok(Json(ListResponse { items, total }))
}

async fn get_project(
    State(state): State<AppState>,
    actor: Actor,
    Path(project_id): Path<String>,
) -> AppResult<Json<ProjectDto>> {
    let access = authz::project_access(&state.pool, &actor, &project_id).await?;
    if access == ProjectAccess::None {
        return Err(AppError::forbidden(
            "you do not have access to this project",
        ));
    }
    Ok(Json(load_project_dto(&state, &actor, &project_id).await?))
}
