use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use sqlx::FromRow;

use crate::AppState;
use crate::authz::Actor;
use crate::dto::{ListQuery, ListResponse, NotificationDto, ReadAllResponse};
use crate::error::{AppError, AppResult};
use crate::util::now_rfc3339;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/notifications", get(list_notifications))
        .route("/notifications/{id}/read", post(mark_read))
        .route("/notifications/read-all", post(mark_all_read))
}

#[derive(FromRow)]
#[allow(dead_code)]
struct NotificationRow {
    id: String,
    kind: String,
    title: String,
    body: String,
    link: String,
    project_id: Option<String>,
    read_at: Option<String>,
    created_at: String,
}

impl From<NotificationRow> for NotificationDto {
    fn from(row: NotificationRow) -> Self {
        NotificationDto {
            id: row.id,
            kind: row.kind,
            title: row.title,
            body: row.body,
            link: row.link,
            project_id: row.project_id,
            read_at: row.read_at,
            created_at: row.created_at,
        }
    }
}

async fn list_notifications(
    State(state): State<AppState>,
    actor: Actor,
    Query(query): Query<ListQuery>,
) -> AppResult<Json<ListResponse<NotificationDto>>> {
    let limit = query.limit();
    let offset = query.offset();

    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM notifications WHERE user_id = ?")
        .bind(&actor.user_id)
        .fetch_one(&state.pool)
        .await?;

    let rows: Vec<NotificationRow> = sqlx::query_as(
        "SELECT id, kind, title, body, link, project_id, read_at, created_at
         FROM notifications WHERE user_id = ?
         ORDER BY created_at DESC
         LIMIT ? OFFSET ?",
    )
    .bind(&actor.user_id)
    .bind(limit)
    .bind(offset)
    .fetch_all(&state.pool)
    .await?;

    let items = rows.into_iter().map(NotificationDto::from).collect();
    Ok(Json(ListResponse { items, total }))
}

async fn mark_read(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<String>,
) -> AppResult<Json<NotificationDto>> {
    let now = now_rfc3339();
    let updated = sqlx::query(
        "UPDATE notifications SET read_at = COALESCE(read_at, ?)
         WHERE id = ? AND user_id = ?",
    )
    .bind(&now)
    .bind(&id)
    .bind(&actor.user_id)
    .execute(&state.pool)
    .await?
    .rows_affected();
    if updated == 0 {
        return Err(AppError::NotFound);
    }

    let row: NotificationRow = sqlx::query_as(
        "SELECT id, kind, title, body, link, project_id, read_at, created_at
         FROM notifications WHERE id = ? AND user_id = ?",
    )
    .bind(&id)
    .bind(&actor.user_id)
    .fetch_one(&state.pool)
    .await?;
    Ok(Json(row.into()))
}

async fn mark_all_read(
    State(state): State<AppState>,
    actor: Actor,
) -> AppResult<Json<ReadAllResponse>> {
    let now = now_rfc3339();
    let marked = sqlx::query(
        "UPDATE notifications SET read_at = ?
         WHERE user_id = ? AND read_at IS NULL",
    )
    .bind(&now)
    .bind(&actor.user_id)
    .execute(&state.pool)
    .await?
    .rows_affected() as i64;
    Ok(Json(ReadAllResponse { marked }))
}
