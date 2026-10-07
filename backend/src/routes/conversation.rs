//! Conversation: threads anchored to project / field / document slot /
//! deliverable / decision / change request, shared vs internal visibility,
//! messages, and action items (§3 message visibility, §4 Conversation).
//!
//! Internal threads are never visible to the team. An action item addressed
//! to the team moves a submitted/in-review project to `changes_requested`
//! through the shared state machine.

use std::collections::HashMap;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{Value, json};
use sqlx::FromRow;

use crate::AppState;
use crate::audit::{self, AuditEvent};
use crate::authz::{Actor, ProjectAccess};
use crate::db;
use crate::dto::ListResponse;
use crate::dto::a::{
    ActionItemDto, CreateThreadRequest, MessageDto, PostMessageRequest, ThreadDto,
};
use crate::error::{AppError, AppResult};
use crate::notify;
use crate::projects::{self, Action};
use crate::routes::projects::{is_team, load_project_row, require_project_access};
use crate::routes::templates;
use crate::util::{new_id, now_rfc3339};
use crate::validation::FieldErrors;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/projects/{id}/threads",
            get(list_threads).post(create_thread),
        )
        .route("/threads/{id}/messages", post(post_message))
        .route("/action-items/{id}/resolve", post(resolve_action_item))
}

const ANCHOR_TYPES: &[&str] = &[
    "project",
    "field",
    "document",
    "deliverable",
    "decision",
    "change_request",
];

/// Which side of the conversation the author speaks for, if they may write a
/// message with `visibility` at all. Team editors+ write shared only;
/// assigned experts internal only; coordinator, decision maker and base
/// manager either. Team viewers, finance and admin read only.
fn writer_side(actor: &Actor, access: ProjectAccess, visibility: &str) -> AppResult<&'static str> {
    match access {
        ProjectAccess::TeamEditor | ProjectAccess::TeamLead => {
            if visibility == "shared" {
                Ok("team")
            } else {
                Err(AppError::forbidden("the team cannot write internal notes"))
            }
        }
        ProjectAccess::Expert => {
            if visibility == "internal" {
                Ok("staff")
            } else {
                Err(AppError::forbidden(
                    "experts write in internal threads only",
                ))
            }
        }
        ProjectAccess::Staff
            if ["coordinator", "decision_maker", "base_manager"]
                .iter()
                .any(|r| actor.has_role(r)) =>
        {
            Ok("staff")
        }
        _ => Err(AppError::forbidden(
            "your role may read but not write messages",
        )),
    }
}

/// Validate that the anchor exists in THIS project (a guessed id from
/// another project is rejected).
async fn validate_anchor(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    project_id: &str,
    template_version_id: &str,
    anchor_type: &str,
    anchor_key: &str,
) -> AppResult<()> {
    let ok = match anchor_type {
        "project" => true,
        "field" | "document" => {
            let schema: String =
                sqlx::query_scalar("SELECT schema_json FROM template_versions WHERE id = ?")
                    .bind(template_version_id)
                    .fetch_one(&mut **tx)
                    .await?;
            let schema: Value =
                serde_json::from_str(&schema).map_err(crate::error::AppError::internal)?;
            if anchor_type == "field" {
                templates::schema_fields(&schema)
                    .iter()
                    .any(|(k, _)| k == anchor_key)
            } else {
                let is_slot = templates::required_document_slots(&schema)
                    .iter()
                    .any(|(k, _, _)| k == anchor_key);
                is_slot || row_in_project(tx, "documents", anchor_key, project_id).await?
            }
        }
        "deliverable" => row_in_project(tx, "deliverables", anchor_key, project_id).await?,
        "decision" => row_in_project(tx, "decisions", anchor_key, project_id).await?,
        "change_request" => row_in_project(tx, "change_requests", anchor_key, project_id).await?,
        _ => false,
    };
    if ok {
        Ok(())
    } else {
        let mut fields = HashMap::new();
        fields.insert(
            "anchor_key".to_string(),
            "does not identify an item of this project".to_string(),
        );
        Err(AppError::Validation { fields })
    }
}

async fn row_in_project(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    table: &'static str,
    id: &str,
    project_id: &str,
) -> AppResult<bool> {
    let n: i64 = sqlx::query_scalar(&format!(
        "SELECT COUNT(*) FROM {table} WHERE id = ? AND project_id = ?"
    ))
    .bind(id)
    .bind(project_id)
    .fetch_one(&mut **tx)
    .await?;
    Ok(n > 0)
}

// ---------------------------------------------------------------------------
// Reading
// ---------------------------------------------------------------------------

#[derive(FromRow)]
struct ThreadRow {
    id: String,
    project_id: String,
    anchor_type: String,
    anchor_key: String,
    visibility: String,
    created_at: String,
}

#[derive(FromRow)]
struct MessageRow {
    id: String,
    thread_id: String,
    author_id: String,
    author_name: String,
    body: String,
    created_at: String,
    edited_at: Option<String>,
}

#[derive(FromRow)]
struct ActionRow {
    id: String,
    thread_id: String,
    addressed_to: String,
    title: String,
    status: String,
    created_by: String,
    created_by_name: String,
    resolved_by: Option<String>,
    resolved_by_name: Option<String>,
    resolved_at: Option<String>,
    created_at: String,
}

/// Threads of a project visible to the viewer (team: shared only), with
/// messages and action items. `only_thread` narrows to one thread.
async fn load_threads(
    state: &AppState,
    project_id: &str,
    shared_only: bool,
    only_thread: Option<&str>,
) -> AppResult<Vec<ThreadDto>> {
    let filter =
        "t.project_id = ? AND (? = 0 OR t.visibility = 'shared') AND (? IS NULL OR t.id = ?)";
    let threads: Vec<ThreadRow> = sqlx::query_as(&format!(
        "SELECT t.id, t.project_id, t.anchor_type, t.anchor_key, t.visibility, t.created_at
         FROM threads t WHERE {filter} ORDER BY t.created_at, t.id"
    ))
    .bind(project_id)
    .bind(shared_only)
    .bind(only_thread)
    .bind(only_thread)
    .fetch_all(&state.pool)
    .await?;
    let messages: Vec<MessageRow> = sqlx::query_as(&format!(
        "SELECT m.id, m.thread_id, m.author_id, u.name AS author_name, m.body, m.created_at, m.edited_at
         FROM messages m JOIN threads t ON t.id = m.thread_id JOIN users u ON u.id = m.author_id
         WHERE {filter} ORDER BY m.created_at, m.rowid"
    ))
    .bind(project_id)
    .bind(shared_only)
    .bind(only_thread)
    .bind(only_thread)
    .fetch_all(&state.pool)
    .await?;
    let actions: Vec<ActionRow> = sqlx::query_as(&format!(
        "SELECT ai.id, ai.thread_id, ai.addressed_to, ai.title, ai.status, ai.created_by,
                cu.name AS created_by_name, ai.resolved_by, ru.name AS resolved_by_name,
                ai.resolved_at, ai.created_at
         FROM action_items ai JOIN threads t ON t.id = ai.thread_id
         JOIN users cu ON cu.id = ai.created_by
         LEFT JOIN users ru ON ru.id = ai.resolved_by
         WHERE {filter} ORDER BY ai.created_at, ai.rowid"
    ))
    .bind(project_id)
    .bind(shared_only)
    .bind(only_thread)
    .bind(only_thread)
    .fetch_all(&state.pool)
    .await?;

    let mut by_thread: HashMap<String, ThreadDto> = HashMap::new();
    let order: Vec<String> = threads.iter().map(|t| t.id.clone()).collect();
    for t in threads {
        by_thread.insert(
            t.id.clone(),
            ThreadDto {
                id: t.id,
                project_id: t.project_id,
                anchor_type: t.anchor_type,
                anchor_key: t.anchor_key,
                visibility: t.visibility,
                created_at: t.created_at,
                messages: Vec::new(),
                action_items: Vec::new(),
            },
        );
    }
    for m in messages {
        if let Some(t) = by_thread.get_mut(&m.thread_id) {
            t.messages.push(MessageDto {
                id: m.id,
                author_id: m.author_id,
                author_name: m.author_name,
                body: m.body,
                created_at: m.created_at,
                edited_at: m.edited_at,
            });
        }
    }
    for a in actions {
        if let Some(t) = by_thread.get_mut(&a.thread_id) {
            t.action_items.push(ActionItemDto {
                id: a.id,
                thread_id: a.thread_id,
                addressed_to: a.addressed_to,
                title: a.title,
                status: a.status,
                created_by: a.created_by,
                created_by_name: a.created_by_name,
                resolved_by: a.resolved_by,
                resolved_by_name: a.resolved_by_name,
                resolved_at: a.resolved_at,
                created_at: a.created_at,
            });
        }
    }
    Ok(order
        .into_iter()
        .filter_map(|id| by_thread.remove(&id))
        .collect())
}

async fn list_threads(
    State(state): State<AppState>,
    actor: Actor,
    Path(project_id): Path<String>,
) -> AppResult<Json<ListResponse<ThreadDto>>> {
    let access = require_project_access(&state, &actor, &project_id).await?;
    let items = load_threads(&state, &project_id, is_team(access), None).await?;
    let total = items.len() as i64;
    Ok(Json(ListResponse { items, total }))
}

// ---------------------------------------------------------------------------
// Writing
// ---------------------------------------------------------------------------

fn validate_body(errors: &mut FieldErrors, body: &str) {
    errors.require("body", body, "message must not be empty");
    errors.max_len("body", body, 20_000);
}

/// Notify the other side about a new message / action item.
#[allow(clippy::too_many_arguments)]
async fn notify_other_side(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    actor: &Actor,
    project_id: &str,
    project_title: &str,
    thread_id: &str,
    side: &str,
    visibility: &str,
    headline: &str,
) -> AppResult<()> {
    let link = format!("/app/projects/{project_id}/messages?thread={thread_id}");
    let title = format!("{headline} — {project_title}");
    let body = format!("{} wrote in the project conversation.", actor.name);
    let recipients = if visibility == "internal" {
        notify::internal_audience_ids(&mut **tx, project_id, Some(&actor.user_id)).await?
    } else if side == "team" {
        notify::coordinator_ids(&mut **tx).await?
    } else {
        notify::team_member_ids(&mut **tx, project_id).await?
    };
    let recipients = recipients
        .into_iter()
        .filter(|id| id != &actor.user_id)
        .collect();
    notify::notify_users(
        tx,
        recipients,
        "conversation_message",
        &title,
        &body,
        &link,
        Some(project_id),
    )
    .await
}

fn message_event(
    actor: &Actor,
    action: &str,
    entity_type: &str,
    entity_id: &str,
    project_id: &str,
    visibility: &str,
    summary: String,
) -> AuditEvent {
    AuditEvent {
        actor_id: Some(actor.user_id.clone()),
        actor_label: actor.name.clone(),
        action: action.into(),
        entity_type: entity_type.into(),
        entity_id: entity_id.into(),
        project_id: Some(project_id.into()),
        visibility: visibility.into(),
        summary,
        before: None,
        after: None,
        reason: None,
    }
}

async fn insert_message(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    thread_id: &str,
    author_id: &str,
    body: &str,
    now: &str,
) -> AppResult<String> {
    let id = new_id();
    sqlx::query(
        "INSERT INTO messages (id, thread_id, author_id, body, created_at) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(thread_id)
    .bind(author_id)
    .bind(body.trim())
    .bind(now)
    .execute(&mut **tx)
    .await?;
    Ok(id)
}

async fn create_thread(
    State(state): State<AppState>,
    actor: Actor,
    Path(project_id): Path<String>,
    Json(req): Json<CreateThreadRequest>,
) -> AppResult<impl IntoResponse> {
    let access = require_project_access(&state, &actor, &project_id).await?;
    let mut errors = FieldErrors::new();
    errors.check(
        "anchor_type",
        ANCHOR_TYPES.contains(&req.anchor_type.as_str()),
        &format!("must be one of {}", ANCHOR_TYPES.join(", ")),
    );
    errors.check(
        "visibility",
        matches!(req.visibility.as_str(), "shared" | "internal"),
        "must be shared or internal",
    );
    validate_body(&mut errors, &req.body);
    let anchor_key = if req.anchor_type == "project" {
        String::new()
    } else {
        let key = req.anchor_key.clone().unwrap_or_default();
        errors.require(
            "anchor_key",
            &key,
            "anchor_key is required for this anchor type",
        );
        key
    };
    if let Some(item) = &req.action_item {
        errors.check(
            "action_item.addressed_to",
            matches!(item.addressed_to.as_str(), "team" | "staff"),
            "must be team or staff",
        );
        errors.require("action_item.title", &item.title, "title is required");
        errors.max_len("action_item.title", &item.title, 300);
        errors.check(
            "visibility",
            !(item.addressed_to == "team" && req.visibility == "internal"),
            "an action item for the team must be in a shared thread",
        );
    }
    errors.finish()?;
    let side = writer_side(&actor, access, &req.visibility)?;
    if req.action_item.is_some() && !actor.is_coordinator() {
        return Err(AppError::forbidden(
            "only the coordinator creates action items",
        ));
    }

    let mut tx = db::begin_immediate(&state.pool).await?;
    let project = load_project_row(&mut *tx, &project_id).await?;
    validate_anchor(
        &mut tx,
        &project_id,
        &project.template_version_id,
        &req.anchor_type,
        &anchor_key,
    )
    .await?;
    let now = now_rfc3339();
    let thread_id = new_id();
    sqlx::query(
        "INSERT INTO threads (id, project_id, anchor_type, anchor_key, visibility, created_at)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(&thread_id)
    .bind(&project_id)
    .bind(&req.anchor_type)
    .bind(&anchor_key)
    .bind(&req.visibility)
    .bind(&now)
    .execute(&mut *tx)
    .await?;
    insert_message(&mut tx, &thread_id, &actor.user_id, &req.body, &now).await?;
    audit::record(
        &mut tx,
        message_event(
            &actor,
            "thread.created",
            "thread",
            &thread_id,
            &project_id,
            &req.visibility,
            format!(
                "{} started a {} conversation on {} {}",
                actor.name, req.visibility, req.anchor_type, anchor_key
            ),
        ),
    )
    .await?;

    let mut headline = format!("New message from {}", actor.name);
    if let Some(item) = &req.action_item {
        let item_id = new_id();
        sqlx::query(
            "INSERT INTO action_items (id, project_id, thread_id, addressed_to, title, status, created_by, created_at)
             VALUES (?, ?, ?, ?, ?, 'open', ?, ?)",
        )
        .bind(&item_id)
        .bind(&project_id)
        .bind(&thread_id)
        .bind(&item.addressed_to)
        .bind(item.title.trim())
        .bind(&actor.user_id)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
        let mut event = message_event(
            &actor,
            "action_item.created",
            "action_item",
            &item_id,
            &project_id,
            &req.visibility,
            format!(
                "{} asked the {} to: {}",
                actor.name,
                item.addressed_to,
                item.title.trim()
            ),
        );
        event.after = Some(json!({"addressed_to": item.addressed_to, "title": item.title.trim()}));
        audit::record(&mut tx, event).await?;
        if item.addressed_to == "team"
            && matches!(project.status.as_str(), "submitted" | "in_review")
        {
            projects::transition(&mut tx, &project_id, Action::RequestChanges, &actor, None)
                .await?;
        }
        headline = format!("{} asks: {}", actor.name, item.title.trim());
    }
    notify_other_side(
        &mut tx,
        &actor,
        &project_id,
        &project.title,
        &thread_id,
        side,
        &req.visibility,
        &headline,
    )
    .await?;
    tx.commit().await?;

    let mut threads = load_threads(&state, &project_id, false, Some(&thread_id)).await?;
    let thread = threads.pop().ok_or(AppError::NotFound)?;
    Ok((StatusCode::CREATED, Json(thread)))
}

async fn post_message(
    State(state): State<AppState>,
    actor: Actor,
    Path(thread_id): Path<String>,
    Json(req): Json<PostMessageRequest>,
) -> AppResult<impl IntoResponse> {
    let thread: Option<(String, String)> =
        sqlx::query_as("SELECT project_id, visibility FROM threads WHERE id = ?")
            .bind(&thread_id)
            .fetch_optional(&state.pool)
            .await?;
    let (project_id, visibility) = thread.ok_or(AppError::NotFound)?;
    let access = require_project_access(&state, &actor, &project_id).await?;
    if is_team(access) && visibility == "internal" {
        // Do not reveal that the internal thread exists.
        return Err(AppError::NotFound);
    }
    let mut errors = FieldErrors::new();
    validate_body(&mut errors, &req.body);
    errors.finish()?;
    let side = writer_side(&actor, access, &visibility)?;

    let mut tx = db::begin_immediate(&state.pool).await?;
    let project = load_project_row(&mut *tx, &project_id).await?;
    let now = now_rfc3339();
    let message_id = insert_message(&mut tx, &thread_id, &actor.user_id, &req.body, &now).await?;
    audit::record(
        &mut tx,
        message_event(
            &actor,
            "message.posted",
            "message",
            &message_id,
            &project_id,
            &visibility,
            format!("{} replied in a {visibility} conversation", actor.name),
        ),
    )
    .await?;
    notify_other_side(
        &mut tx,
        &actor,
        &project_id,
        &project.title,
        &thread_id,
        side,
        &visibility,
        &format!("New message from {}", actor.name),
    )
    .await?;
    tx.commit().await?;

    let mut threads = load_threads(&state, &project_id, false, Some(&thread_id)).await?;
    let thread = threads.pop().ok_or(AppError::NotFound)?;
    Ok((StatusCode::CREATED, Json(thread)))
}

async fn resolve_action_item(
    State(state): State<AppState>,
    actor: Actor,
    Path(item_id): Path<String>,
) -> AppResult<Json<ActionItemDto>> {
    let item: Option<(String, String, String, String, String)> = sqlx::query_as(
        "SELECT ai.project_id, ai.thread_id, ai.status, ai.addressed_to, t.visibility
         FROM action_items ai JOIN threads t ON t.id = ai.thread_id WHERE ai.id = ?",
    )
    .bind(&item_id)
    .fetch_optional(&state.pool)
    .await?;
    let (project_id, thread_id, _, addressed_to, visibility) = item.ok_or(AppError::NotFound)?;
    require_project_access(&state, &actor, &project_id).await?;
    if !actor.is_coordinator() {
        return Err(AppError::forbidden(
            "only the coordinator resolves action items",
        ));
    }

    let mut tx = db::begin_immediate(&state.pool).await?;
    let now = now_rfc3339();
    let updated = sqlx::query(
        "UPDATE action_items SET status = 'resolved', resolved_by = ?, resolved_at = ?
         WHERE id = ? AND status = 'open'",
    )
    .bind(&actor.user_id)
    .bind(&now)
    .bind(&item_id)
    .execute(&mut *tx)
    .await?;
    if updated.rows_affected() != 1 {
        return Err(AppError::conflict(
            "action_item_closed",
            "this action item is no longer open",
        ));
    }
    let title: String = sqlx::query_scalar("SELECT title FROM action_items WHERE id = ?")
        .bind(&item_id)
        .fetch_one(&mut *tx)
        .await?;
    audit::record(
        &mut tx,
        message_event(
            &actor,
            "action_item.resolved",
            "action_item",
            &item_id,
            &project_id,
            &visibility,
            format!("{} resolved: {title}", actor.name),
        ),
    )
    .await?;
    if addressed_to == "team" {
        let team = notify::team_member_ids(&mut *tx, &project_id).await?;
        notify::notify_users(
            &mut tx,
            team,
            "action_item_resolved",
            &format!("Resolved: {title}"),
            &format!("{} marked this request as resolved.", actor.name),
            &format!("/app/projects/{project_id}/messages?thread={thread_id}"),
            Some(&project_id),
        )
        .await?;
    }
    tx.commit().await?;

    let threads = load_threads(&state, &project_id, false, Some(&thread_id)).await?;
    threads
        .into_iter()
        .flat_map(|t| t.action_items)
        .find(|a| a.id == item_id)
        .map(Json)
        .ok_or(AppError::NotFound)
}

/// Ids of open team action items on a project (decision prerequisites).
pub async fn open_team_action_items<'e, E>(exec: E, project_id: &str) -> AppResult<Vec<String>>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    Ok(sqlx::query_scalar(
        "SELECT id FROM action_items WHERE project_id = ? AND addressed_to = 'team' AND status = 'open'",
    )
    .bind(project_id)
    .fetch_all(exec)
    .await?)
}
