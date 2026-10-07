//! Projects: create/list, the full workspace payload, draft autosave,
//! submit/resubmit (idempotent), template upgrade, withdraw, screening,
//! revisions + diffs, and the audit-filtered timeline (§4/§5).
//!
//! Status changes go through the shared state machine
//! `crate::projects::transition`; nothing else mutates `projects.status`.

use std::collections::{BTreeMap, BTreeSet};

use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use sqlx::FromRow;

use crate::AppState;
use crate::audit::{self, AuditEvent};
use crate::authz::{self, Actor, ProjectAccess};
use crate::db;
use crate::dto::a::{
    ApplicationSectionDto, DiffEntryDto, PatchProjectRequest, PatchProjectResponse,
    RevisionDiffDto, RevisionDto, SubmitResponse, TeamMemberDto, UpgradeTemplateResponse,
    WithdrawRequest, WorkspaceCountsDto,
};
use crate::dto::{
    AuditEventDto, CreateProjectRequest, ListQuery, ListResponse, PrimaryMessageDto, ProjectDto,
    ProjectListItemDto, ProjectWorkspaceDto,
};
use crate::error::{AppError, AppResult};
use crate::idempotency;
use crate::notify;
use crate::projects::{self, Action};
use crate::routes::{sites, templates};
use crate::util::{new_id, now_rfc3339, sha256_hex};
use crate::validation::FieldErrors;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/projects", post(create_project).get(list_projects))
        .route("/projects/{id}", get(get_project).patch(patch_project))
        .route("/projects/{id}/submit", post(submit_project))
        .route("/projects/{id}/upgrade-template", post(upgrade_template))
        .route("/projects/{id}/withdraw", post(withdraw_project))
        .route("/projects/{id}/screen", post(screen_project))
        .route("/projects/{id}/revisions", get(list_revisions))
        .route("/projects/{id}/revisions/{n}", get(get_revision))
        .route("/projects/{id}/revisions/{n}/diff", get(revision_diff))
        .route("/projects/{id}/timeline", get(project_timeline))
}

// ---------------------------------------------------------------------------
// Shared access helpers (pub — used by team/conversation/review/decision/
// change-request routes)
// ---------------------------------------------------------------------------

pub fn access_str(access: ProjectAccess) -> &'static str {
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

pub fn is_team(access: ProjectAccess) -> bool {
    matches!(
        access,
        ProjectAccess::TeamViewer | ProjectAccess::TeamEditor | ProjectAccess::TeamLead
    )
}

pub fn require_team_editor(access: ProjectAccess) -> AppResult<()> {
    if matches!(access, ProjectAccess::TeamEditor | ProjectAccess::TeamLead) {
        Ok(())
    } else {
        Err(AppError::forbidden(
            "requires a team editor or lead role on this project",
        ))
    }
}

pub fn require_team_lead(access: ProjectAccess) -> AppResult<()> {
    if access == ProjectAccess::TeamLead {
        Ok(())
    } else {
        Err(AppError::forbidden("requires the team lead role"))
    }
}

/// May the viewer read the application itself (answers, revisions)? Team,
/// assigned experts, and coordinator/decision_maker/admin staff. Base
/// managers and finance get the summary only (§3).
pub fn reads_application(actor: &Actor, access: ProjectAccess) -> bool {
    match access {
        ProjectAccess::Staff => ["coordinator", "decision_maker", "admin"]
            .iter()
            .any(|r| actor.has_role(r)),
        a => a >= ProjectAccess::Expert,
    }
}

/// Fresh access check: 404 for an unknown project, 403 without any access.
pub async fn require_project_access(
    state: &AppState,
    actor: &Actor,
    project_id: &str,
) -> AppResult<ProjectAccess> {
    let exists: Option<String> = sqlx::query_scalar("SELECT id FROM projects WHERE id = ?")
        .bind(project_id)
        .fetch_optional(&state.pool)
        .await?;
    exists.ok_or(AppError::NotFound)?;
    let access = authz::project_access(&state.pool, actor, project_id).await?;
    if access < ProjectAccess::Expert {
        return Err(AppError::forbidden(
            "you do not have access to this project",
        ));
    }
    Ok(access)
}

#[derive(FromRow, Clone)]
pub struct ProjectRow {
    pub id: String,
    pub reference: Option<String>,
    pub title: String,
    pub summary: String,
    pub keywords: String,
    pub organisation: String,
    pub status: String,
    pub template_version_id: String,
    pub answers_json: String,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
    pub version: i64,
    pub created_by: String,
    pub created_at: String,
}

const PROJECT_SELECT: &str = "SELECT id, reference, title, summary, keywords, organisation, status,
            template_version_id, answers_json, start_date, end_date, version,
            created_by, created_at
     FROM projects";

pub async fn load_project_row<'e, E>(exec: E, project_id: &str) -> AppResult<ProjectRow>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    let row: Option<ProjectRow> = sqlx::query_as(&format!("{PROJECT_SELECT} WHERE id = ?"))
        .bind(project_id)
        .fetch_optional(exec)
        .await?;
    row.ok_or(AppError::NotFound)
}

/// Project members, oldest first. Removed members keep their rows for
/// history and are included only when `include_removed`.
pub async fn team_members<'e, E>(
    exec: E,
    project_id: &str,
    include_removed: bool,
) -> AppResult<Vec<TeamMemberDto>>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    #[derive(FromRow)]
    struct Row {
        user_id: String,
        email: String,
        name: String,
        organisation: String,
        role: String,
        added_at: String,
        removed_at: Option<String>,
        removed_by: Option<String>,
    }
    let sql = format!(
        "SELECT pm.user_id, u.email, u.name, u.organisation, pm.role,
                pm.added_at, pm.removed_at, pm.removed_by
         FROM project_members pm JOIN users u ON u.id = pm.user_id
         WHERE pm.project_id = ? {} ORDER BY pm.added_at, pm.id",
        if include_removed {
            ""
        } else {
            "AND pm.removed_at IS NULL"
        }
    );
    let rows: Vec<Row> = sqlx::query_as(&sql)
        .bind(project_id)
        .fetch_all(exec)
        .await?;
    Ok(rows
        .into_iter()
        .map(|r| TeamMemberDto {
            user_id: r.user_id,
            email: r.email,
            name: r.name,
            organisation: r.organisation,
            role: r.role,
            added_at: r.added_at,
            removed_at: r.removed_at,
            removed_by: r.removed_by,
        })
        .collect())
}

async fn load_project_dto(
    state: &AppState,
    actor: &Actor,
    project_id: &str,
) -> AppResult<ProjectDto> {
    let row = load_project_row(&state.pool, project_id).await?;
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
        // Base managers / finance get the summary only (§3).
        answers: if reads_application(actor, access) {
            serde_json::from_str(&row.answers_json).map_err(AppError::internal)?
        } else {
            json!({})
        },
        start_date: row.start_date,
        end_date: row.end_date,
        version: row.version,
        created_by: row.created_by,
        created_at: row.created_at,
        my_access: access_str(access).into(),
    })
}

fn actor_event(
    actor: &Actor,
    action: &str,
    project_id: &str,
    visibility: &str,
    summary: String,
) -> AuditEvent {
    AuditEvent {
        actor_id: Some(actor.user_id.clone()),
        actor_label: actor.name.clone(),
        action: action.into(),
        entity_type: "project".into(),
        entity_id: project_id.into(),
        project_id: Some(project_id.into()),
        visibility: visibility.into(),
        summary,
        before: None,
        after: None,
        reason: None,
    }
}

// ---------------------------------------------------------------------------
// Template answers: validation on autosave, completeness on submit
// ---------------------------------------------------------------------------

async fn load_template_version<'e, E>(exec: E, id: &str) -> AppResult<(i64, String, Value)>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    let row: Option<(i64, String, String)> =
        sqlx::query_as("SELECT version, status, schema_json FROM template_versions WHERE id = ?")
            .bind(id)
            .fetch_optional(exec)
            .await?;
    let (version, status, schema) =
        row.ok_or_else(|| AppError::internal("project bound to missing template version"))?;
    Ok((
        version,
        status,
        serde_json::from_str(&schema).map_err(AppError::internal)?,
    ))
}

fn is_date(v: &str) -> bool {
    chrono::NaiveDate::parse_from_str(v, "%Y-%m-%d").is_ok()
}

fn field_options(def: &Value) -> Vec<&str> {
    def.get("options")
        .and_then(Value::as_array)
        .map(|o| o.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default()
}

/// Type-check every answer against the bound schema (fixed field-type
/// palette). Unknown keys are rejected. `null` clears an answer. Site ids in
/// `sites` answers must belong to `project_site_ids`.
fn validate_answers(
    schema: &Value,
    answers: &Value,
    project_site_ids: &BTreeSet<String>,
    errors: &mut FieldErrors,
) {
    let Some(map) = answers.as_object() else {
        errors.check("answers", false, "must be an object keyed by field key");
        return;
    };
    let fields: BTreeMap<String, &Value> = templates::schema_fields(schema).into_iter().collect();
    for (key, value) in map {
        let path = format!("answers.{key}");
        let Some(def) = fields.get(key) else {
            errors.check(&path, false, "unknown field for this template version");
            continue;
        };
        if value.is_null() {
            continue;
        }
        let ftype = def.get("type").and_then(Value::as_str).unwrap_or("");
        let (ok, msg) = match ftype {
            "text" | "textarea" => (
                value.as_str().is_some_and(|s| s.len() <= 20_000),
                "must be text (at most 20000 characters)",
            ),
            "date" => (
                value.as_str().is_some_and(|s| s.is_empty() || is_date(s)),
                "must be a date YYYY-MM-DD",
            ),
            "daterange" => {
                let part = |k: &str| match value.get(k) {
                    None | Some(Value::Null) => Some(None),
                    Some(Value::String(s)) if s.is_empty() => Some(None),
                    Some(Value::String(s)) if is_date(s) => Some(Some(s.clone())),
                    _ => None,
                };
                let ok = value.is_object()
                    && match (part("start"), part("end")) {
                        (Some(Some(a)), Some(Some(b))) => a <= b,
                        (Some(_), Some(_)) => true,
                        _ => false,
                    };
                (
                    ok,
                    "must be {start, end} dates YYYY-MM-DD with start <= end",
                )
            }
            "number" => (value.is_number(), "must be a number"),
            "select" => {
                let opts = field_options(def);
                (
                    value
                        .as_str()
                        .is_some_and(|s| s.is_empty() || opts.contains(&s)),
                    "must be one of the field's options",
                )
            }
            "multiselect" => {
                let opts = field_options(def);
                (
                    value.as_array().is_some_and(|a| {
                        a.iter()
                            .all(|v| v.as_str().is_some_and(|s| opts.contains(&s)))
                    }),
                    "must be a list of the field's options",
                )
            }
            "people" => (
                value
                    .as_array()
                    .is_some_and(|a| a.iter().all(|v| v.is_object() || v.is_string())),
                "must be a list of people",
            ),
            "checkbox" => (value.is_boolean(), "must be true or false"),
            "sites" => (
                value.as_array().is_some_and(|a| {
                    a.iter()
                        .all(|v| v.as_str().is_some_and(|s| project_site_ids.contains(s)))
                }),
                "must be a list of site ids of this project",
            ),
            _ => (false, "field has an unsupported type"),
        };
        errors.check(&path, ok, msg);
    }
}

fn answer_present(ftype: &str, value: Option<&Value>, has_sites: bool) -> bool {
    let present = match value {
        None | Some(Value::Null) => false,
        Some(Value::String(s)) => !s.trim().is_empty(),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Bool(b)) => *b,
        Some(Value::Object(o)) if ftype == "daterange" => {
            let set = |k: &str| {
                o.get(k)
                    .and_then(Value::as_str)
                    .is_some_and(|s| !s.is_empty())
            };
            set("start") && set("end")
        }
        Some(_) => true,
    };
    present || (ftype == "sites" && has_sites)
}

// ---------------------------------------------------------------------------
// Create / list
// ---------------------------------------------------------------------------

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

    let tv: Option<String> = sqlx::query_scalar(
        "SELECT tv.id FROM template_versions tv
         JOIN templates t ON t.id = tv.template_id
         WHERE t.key = ? AND tv.status = 'published'
         ORDER BY tv.version DESC LIMIT 1",
    )
    .bind(&req.template_key)
    .fetch_optional(&state.pool)
    .await?;
    let template_version_id = tv.ok_or_else(|| {
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
    .bind(req.title.trim())
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
    let mut event = actor_event(
        &actor,
        "project.created",
        &project_id,
        "shared",
        format!("{} created project {}", actor.name, req.title.trim()),
    );
    event.after = Some(json!({"title": req.title.trim()}));
    audit::record(&mut tx, event).await?;
    tx.commit().await?;

    let dto = load_project_dto(&state, &actor, &project_id).await?;
    Ok((StatusCode::CREATED, Json(dto)))
}

#[derive(Deserialize)]
pub struct ProjectListQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub status: Option<String>,
}

async fn list_projects(
    State(state): State<AppState>,
    actor: Actor,
    Query(query): Query<ProjectListQuery>,
) -> AppResult<Json<ListResponse<ProjectListItemDto>>> {
    let paging = ListQuery {
        limit: query.limit,
        offset: query.offset,
    };
    let status = query.status.unwrap_or_default();

    #[derive(FromRow)]
    struct Row {
        id: String,
        reference: Option<String>,
        title: String,
        status: String,
        organisation: String,
        created_at: String,
        my_role: Option<String>,
    }

    // Staff: every project. Experts: only assigned (non-declined).
    // Researchers: only projects where they are active members.
    let scope = if actor.is_staff() {
        "1 = 1"
    } else if actor.is_expert() {
        "(EXISTS (SELECT 1 FROM review_assignments ra WHERE ra.project_id = p.id
                  AND ra.expert_id = ?1x AND ra.status != 'declined')
          OR EXISTS (SELECT 1 FROM project_members pm WHERE pm.project_id = p.id
                  AND pm.user_id = ?1x AND pm.removed_at IS NULL))"
    } else {
        "EXISTS (SELECT 1 FROM project_members pm WHERE pm.project_id = p.id
                 AND pm.user_id = ?1x AND pm.removed_at IS NULL)"
    };
    // Bind the actor id through a CTE so the scope can reference it repeatedly.
    let scope = scope.replace("?1x", "(SELECT uid FROM me)");
    let total: i64 = sqlx::query_scalar(&format!(
        "WITH me(uid) AS (SELECT ?)
         SELECT COUNT(*) FROM projects p WHERE {scope} AND (? = '' OR p.status = ?)"
    ))
    .bind(&actor.user_id)
    .bind(&status)
    .bind(&status)
    .fetch_one(&state.pool)
    .await?;
    let rows: Vec<Row> = sqlx::query_as(&format!(
        "WITH me(uid) AS (SELECT ?)
         SELECT p.id, p.reference, p.title, p.status, p.organisation, p.created_at,
                (SELECT role FROM project_members
                  WHERE project_id = p.id AND user_id = (SELECT uid FROM me)
                    AND removed_at IS NULL) AS my_role
         FROM projects p
         WHERE {scope} AND (? = '' OR p.status = ?)
         ORDER BY p.created_at DESC, p.id
         LIMIT ? OFFSET ?"
    ))
    .bind(&actor.user_id)
    .bind(&status)
    .bind(&status)
    .bind(paging.limit())
    .bind(paging.offset())
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
    Ok(Json(ListResponse { items, total }))
}

// ---------------------------------------------------------------------------
// Workspace assembly (GET /projects/{id})
// ---------------------------------------------------------------------------

/// Most urgent open item for the viewer (§5 `primary_message`): the oldest
/// open action item addressed to their side — or, for an assigned expert, a
/// pending review invitation.
async fn primary_message(
    state: &AppState,
    actor: &Actor,
    access: ProjectAccess,
    project_id: &str,
) -> AppResult<Option<PrimaryMessageDto>> {
    let side = match access {
        ProjectAccess::TeamViewer | ProjectAccess::TeamEditor | ProjectAccess::TeamLead => "team",
        ProjectAccess::Staff => "staff",
        ProjectAccess::Expert => {
            let row: Option<(String, Option<String>, String, String)> = sqlx::query_as(
                "SELECT ra.id, ra.due_date, ra.created_at, u.name
                 FROM review_assignments ra JOIN users u ON u.id = ra.assigned_by
                 WHERE ra.project_id = ? AND ra.expert_id = ? AND ra.status IN ('invited','accepted')
                 ORDER BY ra.created_at DESC LIMIT 1",
            )
            .bind(project_id)
            .bind(&actor.user_id)
            .fetch_optional(&state.pool)
            .await?;
            return Ok(row.map(|(id, due, created_at, name)| PrimaryMessageDto {
                text: match &due {
                    Some(d) => format!("{name} asks you to review this application by {d}"),
                    None => format!("{name} asks you to review this application"),
                },
                title: "review this application".into(),
                by_name: name,
                action_item_id: None,
                thread_id: None,
                review_id: Some(id),
                created_at,
            }));
        }
        _ => return Ok(None),
    };
    let row: Option<(String, String, String, String, String)> = sqlx::query_as(
        "SELECT ai.id, ai.thread_id, ai.title, ai.created_at, u.name
         FROM action_items ai JOIN users u ON u.id = ai.created_by
         WHERE ai.project_id = ? AND ai.addressed_to = ? AND ai.status = 'open'
         ORDER BY ai.created_at, ai.id LIMIT 1",
    )
    .bind(project_id)
    .bind(side)
    .fetch_optional(&state.pool)
    .await?;
    Ok(row.map(|(id, thread_id, title, created_at, name)| {
        let first = name.split_whitespace().next().unwrap_or(&name).to_string();
        let ask = title.trim_end_matches('.');
        let text = if side == "team" {
            format!("{first} asks you to {}", lower_first(ask))
        } else {
            format!("{first}: {ask}")
        };
        PrimaryMessageDto {
            text,
            title,
            by_name: name,
            action_item_id: Some(id),
            thread_id: Some(thread_id),
            review_id: None,
            created_at,
        }
    }))
}

fn lower_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_lowercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// Per-tab counters, scoped to what the viewer can actually open.
async fn workspace_counts(
    state: &AppState,
    actor: &Actor,
    access: ProjectAccess,
    project_id: &str,
) -> AppResult<WorkspaceCountsDto> {
    let personal = authz::can_view_document_category(actor, access, "personal");
    let other_docs = authz::can_view_document_category(actor, access, "application");
    let internal = !is_team(access);
    let drafts = decision_drafts_visible(actor);
    let reviews_visible = !is_team(access);
    #[derive(FromRow)]
    struct Counts {
        documents: i64,
        team: i64,
        sites: i64,
        threads: i64,
        open_action_items: i64,
        reviews: i64,
        decisions: i64,
        change_requests: i64,
        trips: i64,
        invoices: i64,
        deliverables: i64,
        samples: i64,
        revisions: i64,
    }
    let c: Counts = sqlx::query_as(
        "WITH p(id, personal, other_docs, internal, drafts, reviews) AS (SELECT ?, ?, ?, ?, ?, ?)
         SELECT
          (SELECT COUNT(*) FROM documents d, p WHERE d.project_id = p.id
             AND ((d.category = 'personal' AND p.personal) OR (d.category != 'personal' AND p.other_docs))) AS documents,
          (SELECT COUNT(*) FROM project_members m, p WHERE m.project_id = p.id AND m.removed_at IS NULL) AS team,
          (SELECT COUNT(*) FROM project_sites s, p WHERE s.project_id = p.id) AS sites,
          (SELECT COUNT(*) FROM threads t, p WHERE t.project_id = p.id
             AND (t.visibility = 'shared' OR p.internal)) AS threads,
          (SELECT COUNT(*) FROM action_items ai JOIN threads t ON t.id = ai.thread_id, p
             WHERE ai.project_id = p.id AND ai.status = 'open'
               AND (t.visibility = 'shared' OR p.internal)) AS open_action_items,
          (SELECT COUNT(*) FROM review_assignments r, p WHERE r.project_id = p.id AND p.reviews) AS reviews,
          (SELECT COUNT(*) FROM decisions d, p WHERE d.project_id = p.id
             AND (d.status = 'issued' OR p.drafts)) AS decisions,
          (SELECT COUNT(*) FROM change_requests c, p WHERE c.project_id = p.id) AS change_requests,
          (SELECT COUNT(*) FROM trips t, p WHERE t.project_id = p.id) AS trips,
          (SELECT COUNT(*) FROM invoices i, p WHERE i.project_id = p.id AND i.status != 'draft') AS invoices,
          (SELECT COUNT(*) FROM deliverables d, p WHERE d.project_id = p.id) AS deliverables,
          (SELECT COUNT(*) FROM samples s, p WHERE s.project_id = p.id) AS samples,
          (SELECT COUNT(*) FROM project_revisions r, p WHERE r.project_id = p.id) AS revisions",
    )
    .bind(project_id)
    .bind(personal)
    .bind(other_docs)
    .bind(internal)
    .bind(drafts)
    .bind(reviews_visible)
    .fetch_one(&state.pool)
    .await?;
    Ok(WorkspaceCountsDto {
        documents: c.documents,
        team: c.team,
        sites: c.sites,
        threads: c.threads,
        open_action_items: c.open_action_items,
        reviews: c.reviews,
        decisions: c.decisions,
        change_requests: c.change_requests,
        trips: c.trips,
        invoices: c.invoices,
        deliverables: c.deliverables,
        samples: c.samples,
        revisions: c.revisions,
    })
}

/// Coordinators and decision makers see decision drafts; everyone else
/// only issued decisions.
pub fn decision_drafts_visible(actor: &Actor) -> bool {
    actor.is_coordinator() || actor.has_role("decision_maker")
}

async fn get_project(
    State(state): State<AppState>,
    actor: Actor,
    Path(project_id): Path<String>,
) -> AppResult<Json<ProjectWorkspaceDto>> {
    require_project_access(&state, &actor, &project_id).await?;
    Ok(Json(load_workspace(&state, &actor, &project_id).await?))
}

/// Full workspace payload for `GET /projects/{id}` (§5). Each slice adds ONE
/// section field to `ProjectWorkspaceDto` and one assembly line here.
/// Callers must have checked project access already.
pub async fn load_workspace(
    state: &AppState,
    actor: &Actor,
    project_id: &str,
) -> AppResult<ProjectWorkspaceDto> {
    let access = authz::project_access(&state.pool, actor, project_id).await?;
    let (trips, invoices) =
        super::trips::workspace_section(&state.pool, actor, project_id, access).await?;
    Ok(ProjectWorkspaceDto {
        project: load_project_dto(state, actor, project_id).await?,
        primary_message: primary_message(state, actor, access, project_id).await?,
        application: workspace_section(state, actor, access, project_id).await?,
        results: crate::routes::deliverables::workspace_section(&state.pool, project_id).await?,
        trips,
        invoices,
    })
}

/// Slice A's workspace section: bound template schema (+ outdated flag),
/// team, sites (generalized for non-precise viewers) and per-tab counts.
pub async fn workspace_section(
    state: &AppState,
    actor: &Actor,
    access: ProjectAccess,
    project_id: &str,
) -> AppResult<ApplicationSectionDto> {
    let template_version_id: String =
        sqlx::query_scalar("SELECT template_version_id FROM projects WHERE id = ?")
            .bind(project_id)
            .fetch_one(&state.pool)
            .await?;
    let (tv_number, tv_status, schema) =
        load_template_version(&state.pool, &template_version_id).await?;
    let latest: Option<String> = sqlx::query_scalar(
        "SELECT tv.id FROM template_versions tv
         JOIN template_versions bound ON bound.template_id = tv.template_id
         WHERE bound.id = ? AND tv.status = 'published'
         ORDER BY tv.version DESC LIMIT 1",
    )
    .bind(&template_version_id)
    .fetch_optional(&state.pool)
    .await?;
    let precise = authz::can_see_precise_location(access);
    Ok(ApplicationSectionDto {
        template_schema: schema,
        template_version_number: tv_number,
        template_status: tv_status,
        template_outdated: latest.as_ref().is_some_and(|id| id != &template_version_id),
        latest_template_version_id: latest,
        team: team_members(&state.pool, project_id, false).await?,
        sites: sites::project_sites(&state.pool, project_id)
            .await?
            .iter()
            .map(|r| sites::site_dto(r, precise))
            .collect::<AppResult<Vec<_>>>()?,
        counts: workspace_counts(state, actor, access, project_id).await?,
    })
}

// ---------------------------------------------------------------------------
// Autosave
// ---------------------------------------------------------------------------

async fn patch_project(
    State(state): State<AppState>,
    actor: Actor,
    Path(project_id): Path<String>,
    Json(req): Json<PatchProjectRequest>,
) -> AppResult<Json<PatchProjectResponse>> {
    let access = require_project_access(&state, &actor, &project_id).await?;
    require_team_editor(access)?;

    let mut tx = db::begin_immediate(&state.pool).await?;
    let row = load_project_row(&mut *tx, &project_id).await?;
    if !matches!(row.status.as_str(), "draft" | "changes_requested") {
        return Err(AppError::conflict(
            "not_editable",
            "the application can only be edited as a draft or when changes were requested",
        ));
    }
    if row.version != req.version {
        return Err(AppError::conflict(
            "stale_version",
            "the application was changed by someone else; reload to see the latest version",
        ));
    }

    let mut errors = FieldErrors::new();
    for (field, value, max) in [
        ("title", &req.title, 300),
        ("summary", &req.summary, 5000),
        ("keywords", &req.keywords, 500),
        ("organisation", &req.organisation, 300),
    ] {
        if let Some(v) = value {
            errors.max_len(field, v, max);
        }
    }
    if let Some(t) = &req.title {
        errors.require("title", t, "title must not be empty");
    }
    let mut start = req.start_date.clone().unwrap_or(row.start_date.clone());
    let mut end = req.end_date.clone().unwrap_or(row.end_date.clone());
    if let Some(Some(d)) = &req.start_date {
        errors.valid_date("start_date", d);
    }
    if let Some(Some(d)) = &req.end_date {
        errors.valid_date("end_date", d);
    }
    if let Some(answers) = &req.answers {
        let (_, _, schema) = load_template_version(&mut *tx, &row.template_version_id).await?;
        let site_ids: BTreeSet<String> =
            sqlx::query_scalar("SELECT id FROM project_sites WHERE project_id = ?")
                .bind(&project_id)
                .fetch_all(&mut *tx)
                .await?
                .into_iter()
                .collect();
        validate_answers(&schema, answers, &site_ids, &mut errors);
        // The application's own date range ("Proposed dates on Pitcairn")
        // is the project's period: search by year, overview, trip checks.
        if let Some((s, e)) = answered_date_range(&schema, answers) {
            if req.start_date.is_none() {
                start = s;
            }
            if req.end_date.is_none() {
                end = e;
            }
        }
    }
    if let (Some(s), Some(e)) = (&start, &end)
        && is_date(s)
        && is_date(e)
    {
        errors.check("end_date", s <= e, "must not be before the start date");
    }
    errors.finish()?;

    let answers_json = match &req.answers {
        Some(a) => a.to_string(),
        None => row.answers_json.clone(),
    };
    // Version-guarded write: a concurrent save with the same version loses.
    let result = sqlx::query(
        "UPDATE projects SET title = ?, summary = ?, keywords = ?, organisation = ?,
                start_date = ?, end_date = ?, answers_json = ?, version = version + 1
         WHERE id = ? AND version = ?",
    )
    .bind(req.title.as_deref().map(str::trim).unwrap_or(&row.title))
    .bind(req.summary.as_deref().unwrap_or(&row.summary))
    .bind(req.keywords.as_deref().unwrap_or(&row.keywords))
    .bind(req.organisation.as_deref().unwrap_or(&row.organisation))
    .bind(&start)
    .bind(&end)
    .bind(&answers_json)
    .bind(&project_id)
    .bind(req.version)
    .execute(&mut *tx)
    .await?;
    if result.rows_affected() != 1 {
        return Err(AppError::conflict(
            "stale_version",
            "the application was changed by someone else; reload to see the latest version",
        ));
    }
    let mut changed: Vec<&str> = Vec::new();
    for (name, present) in [
        ("title", req.title.is_some()),
        ("summary", req.summary.is_some()),
        ("keywords", req.keywords.is_some()),
        ("organisation", req.organisation.is_some()),
        ("dates", start != row.start_date || end != row.end_date),
        ("answers", req.answers.is_some()),
    ] {
        if present {
            changed.push(name);
        }
    }
    let mut event = actor_event(
        &actor,
        "project.autosaved",
        &project_id,
        "shared",
        format!("{} edited the application", actor.name),
    );
    event.after = Some(json!({"fields": changed, "version": req.version + 1}));
    audit::record(&mut tx, event).await?;
    tx.commit().await?;

    Ok(Json(PatchProjectResponse {
        version: req.version + 1,
        saved_at: now_rfc3339(),
    }))
}

/// Start and end of the template's date-range answer. Answers are replaced
/// as a whole, so a missing or null answer clears both dates; empty or
/// malformed parts become None. None when the template has no date range.
fn answered_date_range(
    schema: &Value,
    answers: &Value,
) -> Option<(Option<String>, Option<String>)> {
    let (key, _) = templates::schema_fields(schema)
        .into_iter()
        .find(|(_, def)| def.get("type").and_then(Value::as_str) == Some("daterange"))?;
    let Some(range) = answers.get(&key).and_then(Value::as_object) else {
        return Some((None, None));
    };
    let part = |k: &str| {
        range
            .get(k)
            .and_then(Value::as_str)
            .filter(|s| is_date(s))
            .map(str::to_string)
    };
    Some((part("start"), part("end")))
}

// ---------------------------------------------------------------------------
// Submit / resubmit (Idempotency-Key)
// ---------------------------------------------------------------------------

/// Self-contained snapshot for `project_revisions.snapshot_json` (§4).
async fn build_snapshot(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    row: &ProjectRow,
    reference: &str,
    template_version: i64,
) -> AppResult<Value> {
    let team = team_members(&mut **tx, &row.id, false).await?;
    let site_values = sites::project_sites(&mut **tx, &row.id)
        .await?
        .iter()
        .map(sites::site_snapshot_value)
        .collect::<AppResult<Vec<_>>>()?;
    #[derive(FromRow)]
    struct Doc {
        document_id: String,
        slot_key: Option<String>,
        title: String,
        category: String,
        version_id: String,
        version_number: i64,
        sha256: String,
    }
    let docs: Vec<Doc> = sqlx::query_as(
        "SELECT d.id AS document_id, d.slot_key, d.title, d.category,
                dv.id AS version_id, dv.number AS version_number, f.sha256
         FROM documents d
         JOIN document_versions dv ON dv.document_id = d.id
              AND dv.number = (SELECT MAX(number) FROM document_versions WHERE document_id = d.id)
         JOIN files f ON f.id = dv.file_id
         WHERE d.project_id = ? AND d.category IN ('application', 'personal', 'other')
         ORDER BY d.created_at, d.id",
    )
    .bind(&row.id)
    .fetch_all(&mut **tx)
    .await?;
    let answers: Value = serde_json::from_str(&row.answers_json).map_err(AppError::internal)?;
    Ok(json!({
        "reference": reference,
        "title": row.title,
        "summary": row.summary,
        "keywords": row.keywords,
        "organisation": row.organisation,
        "start_date": row.start_date,
        "end_date": row.end_date,
        "template_version_id": row.template_version_id,
        "template_version": template_version,
        "answers": answers,
        "team": team.iter().map(|m| json!({
            "user_id": m.user_id, "name": m.name, "email": m.email,
            "organisation": m.organisation, "role": m.role,
        })).collect::<Vec<_>>(),
        "sites": site_values,
        "documents": docs.iter().map(|d| json!({
            "document_id": d.document_id, "slot_key": d.slot_key, "title": d.title,
            "category": d.category, "version_id": d.version_id,
            "version_number": d.version_number, "sha256": d.sha256,
        })).collect::<Vec<_>>(),
    }))
}

async fn submit_project(
    State(state): State<AppState>,
    actor: Actor,
    Path(project_id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> AppResult<(StatusCode, Json<SubmitResponse>)> {
    let access = require_project_access(&state, &actor, &project_id).await?;
    require_team_editor(access)?;

    let idem_key = headers
        .get("idempotency-key")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|k| !k.is_empty())
        .map(str::to_string);
    let route = format!("POST /projects/{project_id}/submit");
    let request_hash = sha256_hex(&body);

    let mut tx = db::begin_immediate(&state.pool).await?;
    if let Some(key) = &idem_key
        && let Some((status, stored)) = idempotency::replay::<SubmitResponse>(
            &mut tx,
            &actor.user_id,
            &route,
            key,
            &request_hash,
        )
        .await?
    {
        let status = StatusCode::from_u16(status).map_err(AppError::internal)?;
        return Ok((status, Json(stored)));
    }

    let row = load_project_row(&mut *tx, &project_id).await?;
    let action = match row.status.as_str() {
        "draft" => Action::Submit,
        "changes_requested" => Action::Resubmit,
        other => {
            return Err(AppError::conflict(
                "invalid_transition",
                format!(
                    "Cannot submit a project that is {}",
                    other.replace('_', " ")
                ),
            ));
        }
    };

    let (tv_number, tv_status, schema) =
        load_template_version(&mut *tx, &row.template_version_id).await?;
    if tv_status != "published" {
        return Err(AppError::conflict(
            "template_outdated",
            "a newer version of this form was published; upgrade the application before submitting",
        ));
    }

    // Completeness: required template fields and required documents.
    let answers: Value = serde_json::from_str(&row.answers_json).map_err(AppError::internal)?;
    let has_sites: bool =
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM project_sites WHERE project_id = ?")
            .bind(&project_id)
            .fetch_one(&mut *tx)
            .await?
            > 0;
    let mut errors = FieldErrors::new();
    errors.require("title", &row.title, "title is required");
    for (key, def) in templates::schema_fields(&schema) {
        let required = def
            .get("required")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let ftype = def.get("type").and_then(Value::as_str).unwrap_or("");
        if required && !answer_present(ftype, answers.get(&key), has_sites) {
            let label = def.get("label").and_then(Value::as_str).unwrap_or(&key);
            errors.check(
                &format!("answers.{key}"),
                false,
                &format!("{label} is required"),
            );
        }
    }
    for (slot, label, _category) in templates::required_document_slots(&schema) {
        let present: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM documents d
             JOIN document_versions dv ON dv.document_id = d.id
             JOIN files f ON f.id = dv.file_id
             WHERE d.project_id = ? AND d.slot_key = ? AND f.scan_status != 'rejected'",
        )
        .bind(&project_id)
        .bind(&slot)
        .fetch_one(&mut *tx)
        .await?;
        errors.check(
            &format!("documents.{slot}"),
            present > 0,
            &format!("{label} must be uploaded"),
        );
    }
    errors.finish()?;

    let now = now_rfc3339();
    let reference = match &row.reference {
        Some(r) => r.clone(),
        None => {
            let prefix: String = sqlx::query_scalar(
                "SELECT COALESCE((SELECT value FROM settings WHERE key = 'reference_prefix'), 'PIT')",
            )
            .fetch_one(&mut *tx)
            .await?;
            let year: i64 = now[0..4].parse().map_err(AppError::internal)?;
            let reference = crate::refs::next(&mut tx, &prefix, year).await?;
            sqlx::query("UPDATE projects SET reference = ? WHERE id = ?")
                .bind(&reference)
                .bind(&project_id)
                .execute(&mut *tx)
                .await?;
            reference
        }
    };

    let snapshot = build_snapshot(&mut tx, &row, &reference, tv_number).await?;
    let number: i64 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(number), 0) + 1 FROM project_revisions WHERE project_id = ?",
    )
    .bind(&project_id)
    .fetch_one(&mut *tx)
    .await?;
    let revision_id = new_id();
    sqlx::query(
        "INSERT INTO project_revisions
         (id, project_id, number, template_version_id, snapshot_json, submitted_by, submitted_at, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&revision_id)
    .bind(&project_id)
    .bind(number)
    .bind(&row.template_version_id)
    .bind(snapshot.to_string())
    .bind(&actor.user_id)
    .bind(&now)
    .bind(&now)
    .execute(&mut *tx)
    .await?;

    let (_, to) = projects::transition(&mut tx, &project_id, action, &actor, None).await?;

    if action == Action::Resubmit {
        // Resolve open team action items that the team answered (a team
        // member's message in the item's thread after it was raised).
        sqlx::query(
            "UPDATE action_items SET status = 'resolved', resolved_by = ?, resolved_at = ?
             WHERE project_id = ? AND addressed_to = 'team' AND status = 'open'
               AND EXISTS (
                 SELECT 1 FROM messages m
                 JOIN project_members pm ON pm.user_id = m.author_id
                      AND pm.project_id = action_items.project_id
                 WHERE m.thread_id = action_items.thread_id
                   AND m.created_at >= action_items.created_at)",
        )
        .bind(&actor.user_id)
        .bind(&now)
        .bind(&project_id)
        .execute(&mut *tx)
        .await?;
    }

    let mut event = AuditEvent {
        entity_type: "project_revision".into(),
        entity_id: revision_id.clone(),
        ..actor_event(
            &actor,
            "project.revision_created",
            &project_id,
            "shared",
            format!("{} submitted revision {number} ({reference})", actor.name),
        )
    };
    event.after = Some(json!({"number": number, "reference": reference}));
    audit::record(&mut tx, event).await?;

    let verb = if action == Action::Submit {
        "submitted"
    } else {
        "resubmitted"
    };
    notify::notify_coordinators(
        &mut tx,
        &project_id,
        "project_submitted",
        &format!("{reference} {verb}: {}", row.title),
        &format!("{} {verb} revision {number}.", actor.name),
    )
    .await?;

    let version: i64 = sqlx::query_scalar("SELECT version FROM projects WHERE id = ?")
        .bind(&project_id)
        .fetch_one(&mut *tx)
        .await?;
    let response = SubmitResponse {
        project_id: project_id.clone(),
        reference,
        revision_number: number,
        status: to.to_string(),
        version,
    };
    if let Some(key) = &idem_key {
        idempotency::store(
            &mut tx,
            &actor.user_id,
            &route,
            key,
            &request_hash,
            200,
            &response,
        )
        .await?;
    }
    tx.commit().await?;
    Ok((StatusCode::OK, Json(response)))
}

// ---------------------------------------------------------------------------
// Template upgrade, withdraw, screen
// ---------------------------------------------------------------------------

async fn upgrade_template(
    State(state): State<AppState>,
    actor: Actor,
    Path(project_id): Path<String>,
) -> AppResult<Json<UpgradeTemplateResponse>> {
    let access = require_project_access(&state, &actor, &project_id).await?;
    require_team_editor(access)?;

    let mut tx = db::begin_immediate(&state.pool).await?;
    let row = load_project_row(&mut *tx, &project_id).await?;
    if !matches!(row.status.as_str(), "draft" | "changes_requested") {
        return Err(AppError::conflict(
            "not_editable",
            "the form can only be upgraded while the application is editable",
        ));
    }
    let latest: Option<(String, i64, String)> = sqlx::query_as(
        "SELECT tv.id, tv.version, tv.schema_json FROM template_versions tv
         JOIN template_versions bound ON bound.template_id = tv.template_id
         WHERE bound.id = ? AND tv.status = 'published'
         ORDER BY tv.version DESC LIMIT 1",
    )
    .bind(&row.template_version_id)
    .fetch_optional(&mut *tx)
    .await?;
    let (new_id_, new_number, new_schema) = latest
        .ok_or_else(|| AppError::conflict("template_current", "no published form version"))?;
    if new_id_ == row.template_version_id {
        return Err(AppError::conflict(
            "template_current",
            "the application already uses the current form version",
        ));
    }
    let new_schema: Value = serde_json::from_str(&new_schema).map_err(AppError::internal)?;
    let keys: BTreeSet<String> = templates::schema_fields(&new_schema)
        .into_iter()
        .map(|(k, _)| k)
        .collect();
    let old_answers: Map<String, Value> =
        serde_json::from_str(&row.answers_json).map_err(AppError::internal)?;
    let mut answers = Map::new();
    let mut dropped = Vec::new();
    for (k, v) in old_answers {
        if keys.contains(&k) {
            answers.insert(k, v);
        } else {
            dropped.push(k);
        }
    }
    sqlx::query(
        "UPDATE projects SET template_version_id = ?, answers_json = ?, version = version + 1
         WHERE id = ?",
    )
    .bind(&new_id_)
    .bind(Value::Object(answers).to_string())
    .bind(&project_id)
    .execute(&mut *tx)
    .await?;
    let mut event = actor_event(
        &actor,
        "project.template_upgraded",
        &project_id,
        "shared",
        format!(
            "{} upgraded the application form to version {new_number}",
            actor.name
        ),
    );
    event.before = Some(json!({"template_version_id": row.template_version_id}));
    event.after = Some(json!({"template_version_id": new_id_, "dropped_keys": dropped}));
    audit::record(&mut tx, event).await?;
    tx.commit().await?;

    Ok(Json(UpgradeTemplateResponse {
        template_version_id: new_id_,
        template_version_number: new_number,
        version: row.version + 1,
        dropped_keys: dropped,
    }))
}

async fn withdraw_project(
    State(state): State<AppState>,
    actor: Actor,
    Path(project_id): Path<String>,
    body: Bytes,
) -> AppResult<Json<ProjectDto>> {
    let access = require_project_access(&state, &actor, &project_id).await?;
    require_team_lead(access)?;
    let req: WithdrawRequest = if body.is_empty() {
        WithdrawRequest { reason: None }
    } else {
        serde_json::from_slice(&body).map_err(|e| AppError::BadRequest(e.to_string()))?
    };
    let reason = req
        .reason
        .map(|r| r.trim().to_string())
        .filter(|r| !r.is_empty());

    let mut tx = db::begin_immediate(&state.pool).await?;
    let (from, _) = projects::transition(
        &mut tx,
        &project_id,
        Action::Withdraw,
        &actor,
        reason.clone(),
    )
    .await?;
    if from != "draft" {
        let title: String = sqlx::query_scalar("SELECT title FROM projects WHERE id = ?")
            .bind(&project_id)
            .fetch_one(&mut *tx)
            .await?;
        notify::notify_coordinators(
            &mut tx,
            &project_id,
            "project_withdrawn",
            &format!("Application withdrawn: {title}"),
            reason
                .as_deref()
                .unwrap_or("The applicant withdrew the application."),
        )
        .await?;
    }
    tx.commit().await?;
    Ok(Json(load_project_dto(&state, &actor, &project_id).await?))
}

async fn screen_project(
    State(state): State<AppState>,
    actor: Actor,
    Path(project_id): Path<String>,
) -> AppResult<Json<ProjectDto>> {
    require_project_access(&state, &actor, &project_id).await?;
    authz::require_role(&actor, &["coordinator"])?;
    let mut tx = db::begin_immediate(&state.pool).await?;
    projects::transition(&mut tx, &project_id, Action::Screen, &actor, None).await?;
    let title: String = sqlx::query_scalar("SELECT title FROM projects WHERE id = ?")
        .bind(&project_id)
        .fetch_one(&mut *tx)
        .await?;
    notify::notify_team(
        &mut tx,
        &project_id,
        "project_in_review",
        &format!("Your application is in review: {title}"),
        &format!("{} opened your application for screening.", actor.name),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(load_project_dto(&state, &actor, &project_id).await?))
}

// ---------------------------------------------------------------------------
// Revisions, diff, timeline
// ---------------------------------------------------------------------------

/// The viewer's copy of a revision snapshot: documents the viewer may not
/// read are removed, sensitive sites generalized for non-precise viewers.
pub fn view_snapshot(mut snapshot: Value, actor: &Actor, access: ProjectAccess) -> Value {
    if let Some(docs) = snapshot.get_mut("documents").and_then(Value::as_array_mut) {
        docs.retain(|d| {
            let cat = d.get("category").and_then(Value::as_str).unwrap_or("other");
            authz::can_view_document_category(actor, access, cat)
        });
    }
    if let Some(sites) = snapshot.get_mut("sites") {
        sites::generalize_site_list(sites, authz::can_see_precise_location(access));
    }
    snapshot
}

async fn require_application_reader(
    state: &AppState,
    actor: &Actor,
    project_id: &str,
) -> AppResult<ProjectAccess> {
    let access = require_project_access(state, actor, project_id).await?;
    if !reads_application(actor, access) {
        return Err(AppError::forbidden(
            "your role sees only the project summary",
        ));
    }
    Ok(access)
}

#[derive(FromRow)]
struct RevisionRow {
    id: String,
    number: i64,
    template_version_id: String,
    template_version: i64,
    schema_json: String,
    snapshot_json: String,
    submitted_by: String,
    submitted_by_name: String,
    submitted_at: String,
}

const REVISION_SELECT: &str =
    "SELECT r.id, r.number, r.template_version_id, tv.version AS template_version,
            tv.schema_json, r.snapshot_json, r.submitted_by, u.name AS submitted_by_name,
            r.submitted_at
     FROM project_revisions r
     JOIN template_versions tv ON tv.id = r.template_version_id
     JOIN users u ON u.id = r.submitted_by";

fn revision_dto(r: RevisionRow, actor: &Actor, access: ProjectAccess) -> AppResult<RevisionDto> {
    let snapshot: Value = serde_json::from_str(&r.snapshot_json).map_err(AppError::internal)?;
    Ok(RevisionDto {
        id: r.id,
        number: r.number,
        template_version_id: r.template_version_id,
        template_version: r.template_version,
        snapshot: view_snapshot(snapshot, actor, access),
        template_schema: serde_json::from_str(&r.schema_json).map_err(AppError::internal)?,
        submitted_by: r.submitted_by,
        submitted_by_name: r.submitted_by_name,
        submitted_at: r.submitted_at,
    })
}

async fn list_revisions(
    State(state): State<AppState>,
    actor: Actor,
    Path(project_id): Path<String>,
) -> AppResult<Json<ListResponse<RevisionDto>>> {
    let access = require_application_reader(&state, &actor, &project_id).await?;
    let rows: Vec<RevisionRow> = sqlx::query_as(&format!(
        "{REVISION_SELECT} WHERE r.project_id = ? ORDER BY r.number"
    ))
    .bind(&project_id)
    .fetch_all(&state.pool)
    .await?;
    let items = rows
        .into_iter()
        .map(|r| revision_dto(r, &actor, access))
        .collect::<AppResult<Vec<_>>>()?;
    let total = items.len() as i64;
    Ok(Json(ListResponse { items, total }))
}

async fn load_revision(state: &AppState, project_id: &str, number: i64) -> AppResult<RevisionRow> {
    let row: Option<RevisionRow> = sqlx::query_as(&format!(
        "{REVISION_SELECT} WHERE r.project_id = ? AND r.number = ?"
    ))
    .bind(project_id)
    .bind(number)
    .fetch_optional(&state.pool)
    .await?;
    row.ok_or(AppError::NotFound)
}

async fn get_revision(
    State(state): State<AppState>,
    actor: Actor,
    Path((project_id, number)): Path<(String, i64)>,
) -> AppResult<Json<RevisionDto>> {
    let access = require_application_reader(&state, &actor, &project_id).await?;
    let row = load_revision(&state, &project_id, number).await?;
    Ok(Json(revision_dto(row, &actor, access)?))
}

#[derive(Deserialize)]
pub struct DiffQuery {
    pub against: Option<i64>,
}

fn diff_entry(path: String, before: Option<&Value>, after: Option<&Value>) -> Option<DiffEntryDto> {
    let norm = |v: Option<&Value>| v.filter(|v| !v.is_null()).cloned();
    let (b, a) = (norm(before), norm(after));
    let kind = match (&b, &a) {
        (None, None) => return None,
        (None, Some(_)) => "added",
        (Some(_), None) => "removed",
        (Some(x), Some(y)) if x == y => return None,
        _ => "changed",
    };
    Some(DiffEntryDto {
        path,
        kind: kind.into(),
        before: b,
        after: a,
        redacted: false,
    })
}

fn keyed<'a>(list: Option<&'a Value>, id_key: &str) -> BTreeMap<String, &'a Value> {
    list.and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|v| Some((v.get(id_key)?.as_str()?.to_string(), v)))
                .collect()
        })
        .unwrap_or_default()
}

/// Structural diff between two revision snapshots. Entries about documents
/// the viewer may not read are kept but redacted (no values).
pub fn diff_snapshots(
    old: &Value,
    new: &Value,
    actor: &Actor,
    access: ProjectAccess,
) -> Vec<DiffEntryDto> {
    let mut out = Vec::new();
    for key in [
        "title",
        "summary",
        "keywords",
        "organisation",
        "start_date",
        "end_date",
        "template_version",
    ] {
        out.extend(diff_entry(key.into(), old.get(key), new.get(key)));
    }
    let empty = Map::new();
    let oa = old
        .get("answers")
        .and_then(Value::as_object)
        .unwrap_or(&empty);
    let na = new
        .get("answers")
        .and_then(Value::as_object)
        .unwrap_or(&empty);
    let answer_keys: BTreeSet<&String> = oa.keys().chain(na.keys()).collect();
    for k in answer_keys {
        out.extend(diff_entry(format!("answers.{k}"), oa.get(k), na.get(k)));
    }
    let precise = authz::can_see_precise_location(access);
    let (os, ns) = (keyed(old.get("sites"), "id"), keyed(new.get("sites"), "id"));
    let site_ids: BTreeSet<&String> = os.keys().chain(ns.keys()).collect();
    for id in site_ids {
        let view = |v: Option<&&Value>| {
            v.map(|v| {
                let mut v = (*v).clone();
                sites::generalize_site_value(&mut v, precise);
                v
            })
        };
        let (b, a) = (view(os.get(id)), view(ns.get(id)));
        out.extend(diff_entry(format!("sites.{id}"), b.as_ref(), a.as_ref()));
    }
    let (od, nd) = (
        keyed(old.get("documents"), "document_id"),
        keyed(new.get("documents"), "document_id"),
    );
    let doc_ids: BTreeSet<&String> = od.keys().chain(nd.keys()).collect();
    for id in doc_ids {
        let (b, a) = (od.get(id).copied(), nd.get(id).copied());
        let category = a
            .or(b)
            .and_then(|d| d.get("category"))
            .and_then(Value::as_str)
            .unwrap_or("other");
        if let Some(mut entry) = diff_entry(format!("documents.{id}"), b, a) {
            if !authz::can_view_document_category(actor, access, category) {
                entry.before = None;
                entry.after = None;
                entry.redacted = true;
            }
            out.push(entry);
        }
    }
    let (ot, nt) = (
        keyed(old.get("team"), "user_id"),
        keyed(new.get("team"), "user_id"),
    );
    let member_ids: BTreeSet<&String> = ot.keys().chain(nt.keys()).collect();
    for id in member_ids {
        out.extend(diff_entry(
            format!("team.{id}"),
            ot.get(id).copied(),
            nt.get(id).copied(),
        ));
    }
    out
}

async fn revision_diff(
    State(state): State<AppState>,
    actor: Actor,
    Path((project_id, number)): Path<(String, i64)>,
    Query(query): Query<DiffQuery>,
) -> AppResult<Json<RevisionDiffDto>> {
    let access = require_application_reader(&state, &actor, &project_id).await?;
    let against = query.against.unwrap_or(number - 1);
    let new = load_revision(&state, &project_id, number).await?;
    let old = load_revision(&state, &project_id, against).await?;
    let parse = |s: &str| serde_json::from_str::<Value>(s).map_err(AppError::internal);
    let changes = diff_snapshots(
        &parse(&old.snapshot_json)?,
        &parse(&new.snapshot_json)?,
        &actor,
        access,
    );
    Ok(Json(RevisionDiffDto {
        revision: number,
        against,
        changes,
    }))
}

/// Timeline = audit events of the project; the team sees only `shared`
/// events, staff and assigned experts see internal ones too.
async fn project_timeline(
    State(state): State<AppState>,
    actor: Actor,
    Path(project_id): Path<String>,
    Query(query): Query<ListQuery>,
) -> AppResult<Json<ListResponse<AuditEventDto>>> {
    let access = require_project_access(&state, &actor, &project_id).await?;
    let shared_only = is_team(access);
    let total: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_events
         WHERE project_id = ? AND (? = 0 OR visibility = 'shared')",
    )
    .bind(&project_id)
    .bind(shared_only)
    .fetch_one(&state.pool)
    .await?;
    #[derive(FromRow)]
    struct Row {
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
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT id, at, actor_id, actor_label, action, entity_type, entity_id, project_id,
                visibility, summary, before_json, after_json, reason
         FROM audit_events
         WHERE project_id = ? AND (? = 0 OR visibility = 'shared')
         ORDER BY at DESC, rowid DESC LIMIT ? OFFSET ?",
    )
    .bind(&project_id)
    .bind(shared_only)
    .bind(query.limit())
    .bind(query.offset())
    .fetch_all(&state.pool)
    .await?;
    let parse = |s: Option<String>| s.and_then(|s| serde_json::from_str(&s).ok());
    let items = rows
        .into_iter()
        .map(|r| AuditEventDto {
            id: r.id,
            at: r.at,
            actor_id: r.actor_id,
            actor_label: r.actor_label,
            action: r.action,
            entity_type: r.entity_type,
            entity_id: r.entity_id,
            project_id: r.project_id,
            visibility: r.visibility,
            summary: r.summary,
            before: parse(r.before_json),
            after: parse(r.after_json),
            reason: r.reason,
        })
        .collect();
    Ok(Json(ListResponse { items, total }))
}
