//! Expert review (§3 expert, §4 `review_assignments`). The coordinator
//! assigns an expert to the latest submitted revision; the expert accepts or
//! declines (with a reason, e.g. conflict of interest) and submits an
//! opinion + recommendation. Opinions are internal: never shown to the team.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;
use sqlx::FromRow;

use crate::AppState;
use crate::audit::{self, AuditEvent};
use crate::authz::Actor;
use crate::db;
use crate::dto::a::{
    CreateReviewRequest, DeclineRequest, ReviewAssignmentDto, SubmitOpinionRequest,
};
use crate::dto::{ListResponse, UserDto};
use crate::error::{AppError, AppResult};
use crate::notify;
use crate::routes::projects::require_project_access;
use crate::util::{new_id, now_rfc3339};
use crate::validation::FieldErrors;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/projects/{id}/reviews",
            post(assign_expert).get(project_reviews),
        )
        .route("/reviews", get(list_reviews))
        .route("/review-experts", get(list_available_experts))
        .route("/reviews/{id}/accept", post(accept_review))
        .route("/reviews/{id}/decline", post(decline_review))
        .route("/reviews/{id}/submit", post(submit_review))
}

/// Coordinators need a safe picker of active experts to assign a review.
async fn list_available_experts(
    State(state): State<AppState>,
    actor: Actor,
) -> AppResult<Json<ListResponse<UserDto>>> {
    if !actor.is_coordinator() {
        return Err(AppError::forbidden(
            "only coordinators can list available experts",
        ));
    }
    let ids: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT u.id FROM users u JOIN user_roles r ON r.user_id = u.id
         WHERE r.role = 'expert' AND r.revoked_at IS NULL AND u.disabled_at IS NULL
         ORDER BY u.name",
    )
    .fetch_all(&state.pool)
    .await?;
    let total = ids.len() as i64;
    let mut items = Vec::with_capacity(ids.len());
    for id in ids {
        items.push(crate::routes::auth::load_user_dto(&state.pool, &id).await?);
    }
    Ok(Json(ListResponse { items, total }))
}

const RECOMMENDATIONS: &[&str] = &[
    "approve",
    "approve_with_conditions",
    "reject",
    "need_more_info",
];

#[derive(FromRow)]
struct ReviewRow {
    id: String,
    project_id: String,
    project_title: String,
    project_reference: Option<String>,
    project_revision_id: String,
    revision_number: i64,
    expert_id: String,
    expert_name: String,
    assigned_by: String,
    assigned_by_name: String,
    due_date: Option<String>,
    status: String,
    decline_reason: Option<String>,
    opinion: Option<String>,
    recommendation: Option<String>,
    submitted_at: Option<String>,
    created_at: String,
}

const REVIEW_SELECT: &str = "SELECT ra.id, ra.project_id, p.title AS project_title, p.reference AS project_reference,
            ra.project_revision_id, r.number AS revision_number, ra.expert_id, eu.name AS expert_name,
            ra.assigned_by, au.name AS assigned_by_name, ra.due_date, ra.status, ra.decline_reason,
            ra.opinion, ra.recommendation, ra.submitted_at, ra.created_at
     FROM review_assignments ra
     JOIN projects p ON p.id = ra.project_id
     JOIN project_revisions r ON r.id = ra.project_revision_id
     JOIN users eu ON eu.id = ra.expert_id
     JOIN users au ON au.id = ra.assigned_by";

impl From<ReviewRow> for ReviewAssignmentDto {
    fn from(r: ReviewRow) -> Self {
        ReviewAssignmentDto {
            id: r.id,
            project_id: r.project_id,
            project_title: r.project_title,
            project_reference: r.project_reference,
            project_revision_id: r.project_revision_id,
            revision_number: r.revision_number,
            expert_id: r.expert_id,
            expert_name: r.expert_name,
            assigned_by: r.assigned_by,
            assigned_by_name: r.assigned_by_name,
            due_date: r.due_date,
            status: r.status,
            decline_reason: r.decline_reason,
            opinion: r.opinion,
            recommendation: r.recommendation,
            submitted_at: r.submitted_at,
            created_at: r.created_at,
        }
    }
}

async fn load_review(state: &AppState, id: &str) -> AppResult<ReviewAssignmentDto> {
    let row: Option<ReviewRow> = sqlx::query_as(&format!("{REVIEW_SELECT} WHERE ra.id = ?"))
        .bind(id)
        .fetch_optional(&state.pool)
        .await?;
    row.map(Into::into).ok_or(AppError::NotFound)
}

fn review_event(
    actor: &Actor,
    action: &str,
    id: &str,
    project_id: &str,
    summary: String,
) -> AuditEvent {
    AuditEvent {
        actor_id: Some(actor.user_id.clone()),
        actor_label: actor.name.clone(),
        action: action.into(),
        entity_type: "review_assignment".into(),
        entity_id: id.into(),
        project_id: Some(project_id.into()),
        visibility: "internal".into(),
        summary,
        before: None,
        after: None,
        reason: None,
    }
}

async fn assign_expert(
    State(state): State<AppState>,
    actor: Actor,
    Path(project_id): Path<String>,
    Json(req): Json<CreateReviewRequest>,
) -> AppResult<impl IntoResponse> {
    require_project_access(&state, &actor, &project_id).await?;
    if !actor.is_coordinator() {
        return Err(AppError::forbidden("only the coordinator assigns experts"));
    }
    let mut errors = FieldErrors::new();
    errors.require("expert_id", &req.expert_id, "expert_id is required");
    if let Some(d) = &req.due_date {
        errors.valid_date("due_date", d);
    }
    errors.finish()?;

    let mut tx = db::begin_immediate(&state.pool).await?;
    let (status, title): (String, String) =
        sqlx::query_as("SELECT status, title FROM projects WHERE id = ?")
            .bind(&project_id)
            .fetch_one(&mut *tx)
            .await?;
    if !matches!(
        status.as_str(),
        "submitted" | "in_review" | "changes_requested"
    ) {
        return Err(AppError::conflict(
            "invalid_state",
            "experts can only be assigned while the application is under review",
        ));
    }
    let is_expert: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM user_roles ur JOIN users u ON u.id = ur.user_id
         WHERE ur.user_id = ? AND ur.role = 'expert' AND ur.revoked_at IS NULL AND u.disabled_at IS NULL",
    )
    .bind(&req.expert_id)
    .fetch_one(&mut *tx)
    .await?;
    if is_expert == 0 {
        let mut fields = std::collections::HashMap::new();
        fields.insert("expert_id".into(), "must be an active expert".into());
        return Err(AppError::Validation { fields });
    }
    let is_member: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM project_members WHERE project_id = ? AND user_id = ? AND removed_at IS NULL",
    )
    .bind(&project_id)
    .bind(&req.expert_id)
    .fetch_one(&mut *tx)
    .await?;
    if is_member > 0 {
        return Err(AppError::conflict(
            "conflict_of_interest",
            "a member of the project team cannot review it",
        ));
    }
    // Bound to the latest submitted revision.
    let revision_id: Option<String> = sqlx::query_scalar(
        "SELECT id FROM project_revisions WHERE project_id = ? ORDER BY number DESC LIMIT 1",
    )
    .bind(&project_id)
    .fetch_optional(&mut *tx)
    .await?;
    let revision_id = revision_id.ok_or_else(|| {
        AppError::conflict("invalid_state", "the application has not been submitted")
    })?;
    let existing: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM review_assignments
         WHERE project_revision_id = ? AND expert_id = ? AND status != 'declined'",
    )
    .bind(&revision_id)
    .bind(&req.expert_id)
    .fetch_one(&mut *tx)
    .await?;
    if existing > 0 {
        return Err(AppError::conflict(
            "already_assigned",
            "this expert is already assigned to the current revision",
        ));
    }
    let id = new_id();
    let now = now_rfc3339();
    sqlx::query(
        "INSERT INTO review_assignments
         (id, project_id, project_revision_id, expert_id, assigned_by, due_date, status, created_at)
         VALUES (?, ?, ?, ?, ?, ?, 'invited', ?)",
    )
    .bind(&id)
    .bind(&project_id)
    .bind(&revision_id)
    .bind(&req.expert_id)
    .bind(&actor.user_id)
    .bind(&req.due_date)
    .bind(&now)
    .execute(&mut *tx)
    .await?;
    let mut event = review_event(
        &actor,
        "review.assigned",
        &id,
        &project_id,
        format!("{} invited an expert to review", actor.name),
    );
    event.after = Some(json!({"expert_id": req.expert_id, "due_date": req.due_date}));
    audit::record(&mut tx, event).await?;
    notify::notify(
        &mut tx,
        &req.expert_id,
        "review_invited",
        &format!("Review requested: {title}"),
        &format!(
            "{} asks you to review this application{}.",
            actor.name,
            req.due_date
                .as_deref()
                .map(|d| format!(" by {d}"))
                .unwrap_or_default()
        ),
        &format!("/app/projects/{project_id}/review"),
        Some(&project_id),
    )
    .await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(load_review(&state, &id).await?)))
}

#[derive(Deserialize)]
pub struct ReviewListQuery {
    pub project_id: Option<String>,
    pub status: Option<String>,
}

/// `GET /reviews`: an expert sees their own assignments; staff see all.
async fn list_reviews(
    State(state): State<AppState>,
    actor: Actor,
    Query(query): Query<ReviewListQuery>,
) -> AppResult<Json<ListResponse<ReviewAssignmentDto>>> {
    let staff = actor.is_staff();
    if !staff && !actor.is_expert() {
        return Err(AppError::forbidden(
            "reviews are visible to staff and experts",
        ));
    }
    let rows: Vec<ReviewRow> = sqlx::query_as(&format!(
        "{REVIEW_SELECT}
         WHERE (? = 1 OR ra.expert_id = ?)
           AND (? IS NULL OR ra.project_id = ?)
           AND (? IS NULL OR ra.status = ?)
         ORDER BY ra.created_at DESC"
    ))
    .bind(staff)
    .bind(&actor.user_id)
    .bind(&query.project_id)
    .bind(&query.project_id)
    .bind(&query.status)
    .bind(&query.status)
    .fetch_all(&state.pool)
    .await?;
    let items: Vec<ReviewAssignmentDto> = rows.into_iter().map(Into::into).collect();
    let total = items.len() as i64;
    Ok(Json(ListResponse { items, total }))
}

/// `GET /projects/{id}/reviews`: staff see all assignments of the project,
/// an assigned expert only their own; the team never (opinions are internal).
async fn project_reviews(
    State(state): State<AppState>,
    actor: Actor,
    Path(project_id): Path<String>,
) -> AppResult<Json<ListResponse<ReviewAssignmentDto>>> {
    let access = require_project_access(&state, &actor, &project_id).await?;
    if crate::routes::projects::is_team(access) {
        return Err(AppError::forbidden("expert reviews are internal"));
    }
    let staff = access == crate::authz::ProjectAccess::Staff;
    let rows: Vec<ReviewRow> = sqlx::query_as(&format!(
        "{REVIEW_SELECT} WHERE ra.project_id = ? AND (? = 1 OR ra.expert_id = ?)
         ORDER BY ra.created_at DESC"
    ))
    .bind(&project_id)
    .bind(staff)
    .bind(&actor.user_id)
    .fetch_all(&state.pool)
    .await?;
    let items: Vec<ReviewAssignmentDto> = rows.into_iter().map(Into::into).collect();
    let total = items.len() as i64;
    Ok(Json(ListResponse { items, total }))
}

/// Load `(project_id, status)` of the actor's OWN assignment; anyone else's
/// assignment is 404 (do not reveal it).
async fn own_assignment(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    actor: &Actor,
    id: &str,
) -> AppResult<(String, String, String)> {
    let row: Option<(String, String, String, String)> = sqlx::query_as(
        "SELECT ra.project_id, ra.status, ra.expert_id, p.title
         FROM review_assignments ra JOIN projects p ON p.id = ra.project_id WHERE ra.id = ?",
    )
    .bind(id)
    .fetch_optional(&mut **tx)
    .await?;
    match row {
        Some((project_id, status, expert_id, title)) if expert_id == actor.user_id => {
            Ok((project_id, status, title))
        }
        Some(_) => Err(AppError::forbidden(
            "this review is assigned to someone else",
        )),
        None => Err(AppError::NotFound),
    }
}

fn wrong_state(status: &str) -> AppError {
    AppError::conflict(
        "invalid_review_state",
        format!("this review is already {status}"),
    )
}

async fn accept_review(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<String>,
) -> AppResult<Json<ReviewAssignmentDto>> {
    let mut tx = db::begin_immediate(&state.pool).await?;
    let (project_id, status, title) = own_assignment(&mut tx, &actor, &id).await?;
    if status != "invited" {
        return Err(wrong_state(&status));
    }
    sqlx::query("UPDATE review_assignments SET status = 'accepted' WHERE id = ?")
        .bind(&id)
        .execute(&mut *tx)
        .await?;
    audit::record(
        &mut tx,
        review_event(
            &actor,
            "review.accepted",
            &id,
            &project_id,
            format!("{} accepted the review", actor.name),
        ),
    )
    .await?;
    notify::notify_coordinators(
        &mut tx,
        &project_id,
        "review_accepted",
        &format!("{} accepted the review of {title}", actor.name),
        "",
    )
    .await?;
    tx.commit().await?;
    Ok(Json(load_review(&state, &id).await?))
}

async fn decline_review(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<String>,
    Json(req): Json<DeclineRequest>,
) -> AppResult<Json<ReviewAssignmentDto>> {
    let mut errors = FieldErrors::new();
    errors.require(
        "reason",
        &req.reason,
        "please give a reason (e.g. conflict of interest)",
    );
    errors.max_len("reason", &req.reason, 2000);
    errors.finish()?;

    let mut tx = db::begin_immediate(&state.pool).await?;
    let (project_id, status, title) = own_assignment(&mut tx, &actor, &id).await?;
    if !matches!(status.as_str(), "invited" | "accepted") {
        return Err(wrong_state(&status));
    }
    sqlx::query(
        "UPDATE review_assignments SET status = 'declined', decline_reason = ? WHERE id = ?",
    )
    .bind(req.reason.trim())
    .bind(&id)
    .execute(&mut *tx)
    .await?;
    let mut event = review_event(
        &actor,
        "review.declined",
        &id,
        &project_id,
        format!("{} declined the review", actor.name),
    );
    event.reason = Some(req.reason.trim().to_string());
    audit::record(&mut tx, event).await?;
    notify::notify_coordinators(
        &mut tx,
        &project_id,
        "review_declined",
        &format!("{} declined the review of {title}", actor.name),
        req.reason.trim(),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(load_review(&state, &id).await?))
}

async fn submit_review(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<String>,
    Json(req): Json<SubmitOpinionRequest>,
) -> AppResult<Json<ReviewAssignmentDto>> {
    let mut errors = FieldErrors::new();
    errors.require("opinion", &req.opinion, "opinion is required");
    errors.max_len("opinion", &req.opinion, 50_000);
    errors.check(
        "recommendation",
        RECOMMENDATIONS.contains(&req.recommendation.as_str()),
        &format!("must be one of {}", RECOMMENDATIONS.join(", ")),
    );
    errors.finish()?;

    let mut tx = db::begin_immediate(&state.pool).await?;
    let (project_id, status, title) = own_assignment(&mut tx, &actor, &id).await?;
    if status != "accepted" {
        return Err(if status == "invited" {
            AppError::conflict(
                "invalid_review_state",
                "accept the review before submitting",
            )
        } else {
            wrong_state(&status)
        });
    }
    let now = now_rfc3339();
    sqlx::query(
        "UPDATE review_assignments SET status = 'submitted', opinion = ?, recommendation = ?, submitted_at = ?
         WHERE id = ?",
    )
    .bind(req.opinion.trim())
    .bind(&req.recommendation)
    .bind(&now)
    .bind(&id)
    .execute(&mut *tx)
    .await?;
    let mut event = review_event(
        &actor,
        "review.submitted",
        &id,
        &project_id,
        format!(
            "{} submitted an opinion ({})",
            actor.name,
            req.recommendation.replace('_', " ")
        ),
    );
    event.after = Some(json!({"recommendation": req.recommendation}));
    audit::record(&mut tx, event).await?;
    // Coordinators and decision makers receive the (internal) opinion.
    let recipients: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT user_id FROM user_roles
         WHERE role IN ('coordinator', 'decision_maker') AND revoked_at IS NULL",
    )
    .fetch_all(&mut *tx)
    .await?;
    notify::notify_users(
        &mut tx,
        recipients,
        "review_submitted",
        &format!("Expert opinion received: {title}"),
        &format!(
            "{} recommends: {}.",
            actor.name,
            req.recommendation.replace('_', " ")
        ),
        &format!("/app/projects/{project_id}/review"),
        Some(&project_id),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(load_review(&state, &id).await?))
}
