//! Deliverables, submissions, publication, samples-facing close flow and the
//! results workspace section (architecture §4 "Deliverables", §5).
//!
//! Acknowledgement model: each side's `*_agreed_at/by` fields record
//! acknowledgement of the CURRENT `terms_version`; a material change bumps
//! `terms_version` and clears the OTHER side's acknowledgement (the changer
//! implicitly agrees to their own terms), so "agreed" always means both
//! sides accepted the current terms.

use axum::Router;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json};
use axum::routing::{get, patch, post, put};
use serde_json::{Value, json};
use sqlx::{FromRow, SqlitePool};

use crate::audit::{self, AuditEvent};
use crate::authz::{self, Actor, ProjectAccess};
use crate::db;
use crate::deliverables::{self, OPEN_STATUSES, PUBLICATION_WARNING};
use crate::dto::{
    AcceptSubmissionResponse, CloseProjectRequest, CreateDeliverableRequest,
    CreateSubmissionRequest, DeliverableDto, ExternalLinkDto, ListQuery, ListResponse, NoteRequest,
    PublicationFilesResponse, PublicationUpdateResponse, ResultsSectionDto, SampleDto,
    SetPublicationFilesRequest, SubmissionDto, SubmissionFileDto, UnresolvedDeliverableDto,
    UpdateDeliverableRequest, UpdatePublicationRequest,
};
use crate::error::{AppError, AppResult};
use crate::util::{new_id, now_rfc3339};
use crate::validation::FieldErrors;
use crate::{AppState, jobs, notify, projects};

const KINDS: [&str; 5] = ["report", "dataset", "media", "samples", "other"];

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/projects/{id}/deliverables",
            get(list_deliverables).post(create_deliverable),
        )
        .route("/projects/{id}/close", post(close_project))
        .route("/deliverables/{id}", patch(patch_deliverable))
        .route("/deliverables/{id}/agree", post(agree_deliverable))
        .route("/deliverables/{id}/waive", post(waive_deliverable))
        .route("/deliverables/{id}/cancel", post(cancel_deliverable))
        .route("/deliverables/{id}/submissions", post(create_submission))
        .route("/deliverables/{id}/publication", patch(update_publication))
        .route(
            "/deliverables/{id}/publication-files",
            put(set_publication_files),
        )
        .route("/submissions/{id}/request-changes", post(request_changes))
        .route("/submissions/{id}/accept", post(accept_submission))
        .route("/external-links/{id}/check", post(check_link_now))
        .route(
            "/projects/{id}/samples",
            get(list_samples).post(create_sample),
        )
        .route("/samples/{id}", patch(patch_sample).delete(delete_sample))
}

// ---------------------------------------------------------------------------
// Shared loaders / authz helpers
// ---------------------------------------------------------------------------

#[derive(FromRow)]
#[allow(dead_code)]
struct DeliverableRow {
    id: String,
    project_id: String,
    title: String,
    description: String,
    kind: String,
    due_date: String,
    sender_id: String,
    sender_name: String,
    recipient_id: String,
    recipient_name: String,
    status: String,
    terms_version: i64,
    team_agreed_at: Option<String>,
    team_agreed_by: Option<String>,
    staff_agreed_at: Option<String>,
    staff_agreed_by: Option<String>,
    resolution_note: Option<String>,
    publish_level: String,
    embargo_until: Option<String>,
    published_at: Option<String>,
    published_by: Option<String>,
    created_by: String,
    created_at: String,
}

const DELIVERABLE_SELECT: &str =
    "SELECT d.id, d.project_id, d.title, d.description, d.kind, d.due_date,
            d.sender_id, su.name AS sender_name,
            d.recipient_id, ru.name AS recipient_name,
            d.status, d.terms_version, d.team_agreed_at, d.team_agreed_by,
            d.staff_agreed_at, d.staff_agreed_by, d.resolution_note,
            d.publish_level, d.embargo_until, d.published_at, d.published_by,
            d.created_by, d.created_at
     FROM deliverables d
     JOIN users su ON su.id = d.sender_id
     JOIN users ru ON ru.id = d.recipient_id";

async fn load_deliverable(pool: &SqlitePool, id: &str) -> AppResult<DeliverableRow> {
    let row: Option<DeliverableRow> =
        sqlx::query_as(&format!("{DELIVERABLE_SELECT} WHERE d.id = ?"))
            .bind(id)
            .fetch_optional(pool)
            .await?;
    row.ok_or(AppError::NotFound)
}

#[derive(FromRow)]
#[allow(dead_code)]
struct SubmissionRow {
    id: String,
    deliverable_id: String,
    number: i64,
    submitted_by: String,
    note: String,
    data_dictionary_json: String,
    status: String,
    reviewed_by: Option<String>,
    review_note: Option<String>,
    reviewed_at: Option<String>,
    created_at: String,
}

async fn load_submission_dto(pool: &SqlitePool, id: &str) -> AppResult<SubmissionDto> {
    let row: SubmissionRow = sqlx::query_as(
        "SELECT id, deliverable_id, number, submitted_by, note, data_dictionary_json,
                status, reviewed_by, review_note, reviewed_at, created_at
         FROM deliverable_submissions WHERE id = ?",
    )
    .bind(id)
    .fetch_one(pool)
    .await?;
    submission_dto(pool, row).await
}

#[derive(FromRow)]
#[allow(dead_code)]
struct ExternalLinkRow {
    id: String,
    submission_id: String,
    url: String,
    description: String,
    version_label: String,
    access_notes: String,
    last_checked_at: Option<String>,
    last_status: Option<String>,
    available: Option<i64>,
}

impl From<ExternalLinkRow> for ExternalLinkDto {
    fn from(r: ExternalLinkRow) -> Self {
        ExternalLinkDto {
            id: r.id,
            submission_id: r.submission_id,
            url: r.url,
            description: r.description,
            version_label: r.version_label,
            access_notes: r.access_notes,
            last_checked_at: r.last_checked_at,
            last_status: r.last_status,
            available: r.available.map(|v| v != 0),
        }
    }
}

#[derive(FromRow)]
#[allow(dead_code)]
struct SubmissionFileRow {
    document_version_id: String,
    document_id: String,
    title: String,
    number: i64,
    mime: String,
    size: i64,
}

async fn submission_dto(pool: &SqlitePool, row: SubmissionRow) -> AppResult<SubmissionDto> {
    let files: Vec<SubmissionFileDto> = sqlx::query_as::<_, SubmissionFileRow>(
        "SELECT sf.document_version_id, d.id AS document_id, d.title, dv.number,
                f.mime, f.size
         FROM submission_files sf
         JOIN document_versions dv ON dv.id = sf.document_version_id
         JOIN documents d ON d.id = dv.document_id
         JOIN files f ON f.id = dv.file_id
         WHERE sf.submission_id = ?
         ORDER BY d.title, dv.number",
    )
    .bind(&row.id)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|r| SubmissionFileDto {
        document_version_id: r.document_version_id,
        document_id: r.document_id,
        title: r.title,
        number: r.number,
        mime: r.mime,
        size: r.size,
    })
    .collect();

    let links: Vec<ExternalLinkDto> = sqlx::query_as::<_, ExternalLinkRow>(
        "SELECT id, submission_id, url, description, version_label, access_notes,
                last_checked_at, last_status, available
         FROM external_links WHERE submission_id = ? ORDER BY url",
    )
    .bind(&row.id)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(ExternalLinkDto::from)
    .collect();

    Ok(SubmissionDto {
        id: row.id,
        deliverable_id: row.deliverable_id,
        number: row.number,
        status: row.status,
        note: row.note,
        data_dictionary: serde_json::from_str(&row.data_dictionary_json).unwrap_or_default(),
        submitted_by: row.submitted_by,
        submitted_at: row.created_at,
        reviewed_by: row.reviewed_by,
        review_note: row.review_note,
        reviewed_at: row.reviewed_at,
        files,
        links,
    })
}

async fn deliverable_dto(pool: &SqlitePool, row: DeliverableRow) -> AppResult<DeliverableDto> {
    let latest: Option<SubmissionRow> = sqlx::query_as(
        "SELECT id, deliverable_id, number, submitted_by, note, data_dictionary_json,
                status, reviewed_by, review_note, reviewed_at, created_at
         FROM deliverable_submissions WHERE deliverable_id = ?
         ORDER BY number DESC LIMIT 1",
    )
    .bind(&row.id)
    .fetch_optional(pool)
    .await?;
    let accepted: Option<SubmissionRow> = sqlx::query_as(
        "SELECT id, deliverable_id, number, submitted_by, note, data_dictionary_json,
                status, reviewed_by, review_note, reviewed_at, created_at
         FROM deliverable_submissions WHERE deliverable_id = ? AND status = 'accepted'
         ORDER BY number DESC LIMIT 1",
    )
    .bind(&row.id)
    .fetch_optional(pool)
    .await?;
    let latest_submission = match latest {
        Some(r) => Some(submission_dto(pool, r).await?),
        None => None,
    };
    let accepted_submission = match accepted {
        Some(r) => Some(submission_dto(pool, r).await?),
        None => None,
    };
    let agreement_state = match (&row.team_agreed_at, &row.staff_agreed_at) {
        (Some(_), Some(_)) => "agreed",
        (Some(_), None) => "team_only",
        (None, Some(_)) => "staff_only",
        (None, None) => "pending",
    };
    Ok(DeliverableDto {
        id: row.id,
        project_id: row.project_id,
        title: row.title,
        description: row.description,
        kind: row.kind,
        due_date: row.due_date,
        sender_id: row.sender_id,
        sender_name: row.sender_name,
        recipient_id: row.recipient_id,
        recipient_name: row.recipient_name,
        status: row.status,
        terms_version: row.terms_version,
        team_agreed_at: row.team_agreed_at,
        team_agreed_by: row.team_agreed_by,
        staff_agreed_at: row.staff_agreed_at,
        staff_agreed_by: row.staff_agreed_by,
        agreement_state: agreement_state.into(),
        resolution_note: row.resolution_note,
        publish_level: row.publish_level,
        embargo_until: row.embargo_until,
        published_at: row.published_at,
        published_by: row.published_by,
        created_by: row.created_by,
        created_at: row.created_at,
        latest_submission,
        accepted_submission,
    })
}

/// May the actor edit/propose/submit on the team side, or act as the staff
/// side (coordinator) for this project? Returns the side: "staff" for a
/// coordinator, "team" for an active team editor/lead; `None` when the actor
/// may do neither (callers return 403).
async fn deliverable_side(
    pool: &SqlitePool,
    actor: &Actor,
    project_id: &str,
) -> AppResult<Option<&'static str>> {
    if actor.is_coordinator() {
        // A coordinator acts on the staff side even when they also sit on the
        // project team.
        return Ok(Some("staff"));
    }
    let access = authz::project_access(pool, actor, project_id).await?;
    Ok(match access {
        ProjectAccess::TeamEditor | ProjectAccess::TeamLead => Some("team"),
        _ => None,
    })
}

/// Every active member's user_id on the project (for team-wide notifications).
async fn team_member_ids<'e, E>(exec: E, project_id: &str) -> AppResult<Vec<String>>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    Ok(sqlx::query_scalar(
        "SELECT user_id FROM project_members
         WHERE project_id = ? AND removed_at IS NULL",
    )
    .bind(project_id)
    .fetch_all(exec)
    .await?)
}

/// Notify all active project members (used for review outcomes).
async fn notify_team(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    project_id: &str,
    kind: &str,
    title: &str,
    body: &str,
    link: &str,
) -> AppResult<()> {
    for user_id in team_member_ids(&mut **tx, project_id).await? {
        notify::notify(tx, &user_id, kind, title, body, link, Some(project_id)).await?;
    }
    Ok(())
}

fn require_open(row: &DeliverableRow) -> AppResult<()> {
    if OPEN_STATUSES.contains(&row.status.as_str()) {
        Ok(())
    } else {
        Err(AppError::conflict(
            "invalid_transition",
            format!("deliverable is {}", row.status.replace('_', " ")),
        ))
    }
}

async fn require_coordinator(actor: &Actor) -> AppResult<()> {
    authz::require_role(actor, &["coordinator"])
}

/// Active project member? (for `sender_id` validation)
async fn is_active_member<'e, E>(exec: E, project_id: &str, user_id: &str) -> AppResult<bool>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM project_members
         WHERE project_id = ? AND user_id = ? AND removed_at IS NULL",
    )
    .bind(project_id)
    .bind(user_id)
    .fetch_one(exec)
    .await?;
    Ok(n > 0)
}

/// Holds the coordinator role?
async fn is_coordinator_user<'e, E>(exec: E, user_id: &str) -> AppResult<bool>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM user_roles
         WHERE user_id = ? AND role = 'coordinator' AND revoked_at IS NULL",
    )
    .bind(user_id)
    .fetch_one(exec)
    .await?;
    Ok(n > 0)
}

// ---------------------------------------------------------------------------
// GET /projects/{id}/deliverables
// ---------------------------------------------------------------------------

async fn list_deliverables(
    State(state): State<AppState>,
    actor: Actor,
    Path(project_id): Path<String>,
    axum::extract::Query(query): axum::extract::Query<ListQuery>,
) -> AppResult<Json<ListResponse<DeliverableDto>>> {
    let access = authz::project_access(&state.pool, &actor, &project_id).await?;
    if access == ProjectAccess::None {
        return Err(AppError::forbidden(
            "you do not have access to this project",
        ));
    }
    let limit = query.limit();
    let offset = query.offset();
    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM deliverables WHERE project_id = ?")
        .bind(&project_id)
        .fetch_one(&state.pool)
        .await?;
    let rows: Vec<DeliverableRow> = sqlx::query_as(&format!(
        "{DELIVERABLE_SELECT} WHERE d.project_id = ?
         ORDER BY d.due_date ASC, d.created_at ASC LIMIT ? OFFSET ?"
    ))
    .bind(&project_id)
    .bind(limit)
    .bind(offset)
    .fetch_all(&state.pool)
    .await?;
    let mut items = Vec::new();
    for row in rows {
        items.push(deliverable_dto(&state.pool, row).await?);
    }
    Ok(Json(ListResponse { items, total }))
}

// ---------------------------------------------------------------------------
// POST /projects/{id}/deliverables — propose (C or team editor+)
// ---------------------------------------------------------------------------

async fn create_deliverable(
    State(state): State<AppState>,
    actor: Actor,
    Path(project_id): Path<String>,
    Json(req): Json<CreateDeliverableRequest>,
) -> AppResult<impl IntoResponse> {
    let Some(side) = deliverable_side(&state.pool, &actor, &project_id).await? else {
        return Err(AppError::forbidden(
            "only team editors and coordinators may propose deliverables",
        ));
    };
    // Project must exist (access already implies it for members/staff, but a
    // coordinator reaches non-existent ids too — check explicitly).
    let exists: Option<(String,)> = sqlx::query_as("SELECT id FROM projects WHERE id = ?")
        .bind(&project_id)
        .fetch_optional(&state.pool)
        .await?;
    if exists.is_none() {
        return Err(AppError::NotFound);
    }

    let mut errors = FieldErrors::new();
    errors.require("title", &req.title, "title is required");
    errors.max_len("title", &req.title, 300);
    errors.check(
        "kind",
        KINDS.contains(&req.kind.as_str()),
        "must be one of report, dataset, media, samples, other",
    );
    errors.require("due_date", &req.due_date, "due_date is required");
    if !req.due_date.is_empty() {
        errors.valid_date("due_date", &req.due_date);
    }
    if !is_active_member(&state.pool, &project_id, &req.sender_id).await? {
        errors.check(
            "sender_id",
            false,
            "sender must be an active project member",
        );
    }
    if !is_coordinator_user(&state.pool, &req.recipient_id).await? {
        errors.check("recipient_id", false, "recipient must be a coordinator");
    }
    errors.finish()?;

    let mut tx = db::begin_immediate(&state.pool).await?;
    let id = new_id();
    let now = now_rfc3339();
    // The proposer implicitly agrees to the terms they propose: stamp their
    // side's acknowledgement now; the deliverable turns `agreed` when the
    // other side acknowledges the same terms_version.
    let (team_at, team_by, staff_at, staff_by) = if side == "staff" {
        (None, None, Some(now.clone()), Some(actor.user_id.clone()))
    } else {
        (Some(now.clone()), Some(actor.user_id.clone()), None, None)
    };
    sqlx::query(
        "INSERT INTO deliverables
         (id, project_id, title, description, kind, due_date, sender_id, recipient_id,
          status, terms_version, team_agreed_at, team_agreed_by, staff_agreed_at,
          staff_agreed_by, created_by, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, 'proposed', 1, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&project_id)
    .bind(&req.title)
    .bind(req.description.unwrap_or_default())
    .bind(&req.kind)
    .bind(&req.due_date)
    .bind(&req.sender_id)
    .bind(&req.recipient_id)
    .bind(&team_at)
    .bind(&team_by)
    .bind(&staff_at)
    .bind(&staff_by)
    .bind(&actor.user_id)
    .bind(&now)
    .execute(&mut *tx)
    .await?;

    audit::record(
        &mut tx,
        AuditEvent {
            actor_id: Some(actor.user_id.clone()),
            actor_label: actor.name.clone(),
            action: "deliverable.proposed".into(),
            entity_type: "deliverable".into(),
            entity_id: id.clone(),
            project_id: Some(project_id.clone()),
            visibility: "shared".into(),
            summary: format!("{} proposed deliverable \"{}\"", actor.name, req.title),
            before: None,
            after: Some(Value::String(req.title.clone())),
            reason: None,
        },
    )
    .await?;

    // Notify the other side of the proposal.
    let (notify_id, notify_body) = if side == "staff" {
        (
            req.sender_id.clone(),
            format!(
                "{} proposed a new deliverable: \"{}\"",
                actor.name, req.title
            ),
        )
    } else {
        (
            req.recipient_id.clone(),
            format!(
                "{} proposed a new deliverable: \"{}\"",
                actor.name, req.title
            ),
        )
    };
    notify::notify(
        &mut tx,
        &notify_id,
        "deliverable.proposed",
        "New deliverable proposed",
        &notify_body,
        &format!("/app/projects/{project_id}/results"),
        Some(&project_id),
    )
    .await?;

    tx.commit().await?;

    let row = load_deliverable(&state.pool, &id).await?;
    Ok((
        StatusCode::CREATED,
        Json(deliverable_dto(&state.pool, row).await?),
    ))
}

// ---------------------------------------------------------------------------
// PATCH /deliverables/{id} — material change bumps terms_version
// ---------------------------------------------------------------------------

async fn patch_deliverable(
    State(state): State<AppState>,
    actor: Actor,
    Path(deliverable_id): Path<String>,
    Json(req): Json<UpdateDeliverableRequest>,
) -> AppResult<impl IntoResponse> {
    let row = load_deliverable(&state.pool, &deliverable_id).await?;
    let Some(side) = deliverable_side(&state.pool, &actor, &row.project_id).await? else {
        return Err(AppError::forbidden(
            "only team editors and coordinators may change deliverables",
        ));
    };
    require_open(&row)?;

    let new_title = req.title.clone().unwrap_or_else(|| row.title.clone());
    let new_description = req
        .description
        .clone()
        .unwrap_or_else(|| row.description.clone());
    let new_kind = req.kind.clone().unwrap_or_else(|| row.kind.clone());
    let new_due = req.due_date.clone().unwrap_or_else(|| row.due_date.clone());
    let new_sender = req
        .sender_id
        .clone()
        .unwrap_or_else(|| row.sender_id.clone());
    let new_recipient = req
        .recipient_id
        .clone()
        .unwrap_or_else(|| row.recipient_id.clone());

    let mut errors = FieldErrors::new();
    errors.require("title", &new_title, "title is required");
    errors.max_len("title", &new_title, 300);
    errors.check(
        "kind",
        KINDS.contains(&new_kind.as_str()),
        "must be one of report, dataset, media, samples, other",
    );
    errors.valid_date("due_date", &new_due);
    let due_changed = new_due != row.due_date;
    if due_changed {
        errors.check(
            "reason",
            req.reason
                .as_deref()
                .map(|r| !r.trim().is_empty())
                .unwrap_or(false),
            "reason is required when the due date changes",
        );
    }
    if new_sender != row.sender_id
        && !is_active_member(&state.pool, &row.project_id, &new_sender).await?
    {
        errors.check(
            "sender_id",
            false,
            "sender must be an active project member",
        );
    }
    if new_recipient != row.recipient_id
        && !is_coordinator_user(&state.pool, &new_recipient).await?
    {
        errors.check("recipient_id", false, "recipient must be a coordinator");
    }
    errors.finish()?;

    let material = new_title != row.title
        || new_description != row.description
        || new_kind != row.kind
        || due_changed
        || new_sender != row.sender_id
        || new_recipient != row.recipient_id;

    let mut tx = db::begin_immediate(&state.pool).await?;
    let now = now_rfc3339();
    if material {
        // Bump terms_version, clear the OTHER side's acknowledgement and
        // stamp the changer on their own side (they agree to their terms).
        let (clear_col, set_col_at, set_col_by) = if side == "staff" {
            ("team_agreed_at", "staff_agreed_at", "staff_agreed_by")
        } else {
            ("staff_agreed_at", "team_agreed_at", "team_agreed_by")
        };
        let clear_by = if side == "staff" {
            "team_agreed_by"
        } else {
            "staff_agreed_by"
        };
        let new_status = if row.status == "agreed" {
            "proposed"
        } else {
            row.status.as_str()
        };
        sqlx::query(&format!(
            "UPDATE deliverables SET title = ?, description = ?, kind = ?, due_date = ?,
                    sender_id = ?, recipient_id = ?, terms_version = terms_version + 1,
                    {clear_col} = NULL, {clear_by} = NULL,
                    {set_col_at} = ?, {set_col_by} = ?, status = ?
             WHERE id = ?"
        ))
        .bind(&new_title)
        .bind(&new_description)
        .bind(&new_kind)
        .bind(&new_due)
        .bind(&new_sender)
        .bind(&new_recipient)
        .bind(&now)
        .bind(&actor.user_id)
        .bind(new_status)
        .bind(&deliverable_id)
        .execute(&mut *tx)
        .await?;
    }

    if due_changed {
        sqlx::query(
            "INSERT INTO deliverable_due_changes
             (id, deliverable_id, old_due, new_due, reason, changed_by, changed_at, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(new_id())
        .bind(&deliverable_id)
        .bind(&row.due_date)
        .bind(&new_due)
        .bind(req.reason.clone().unwrap_or_default())
        .bind(&actor.user_id)
        .bind(&now)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
    }

    if material {
        audit::record(
            &mut tx,
            AuditEvent {
                actor_id: Some(actor.user_id.clone()),
                actor_label: actor.name.clone(),
                action: "deliverable.terms_changed".into(),
                entity_type: "deliverable".into(),
                entity_id: deliverable_id.clone(),
                project_id: Some(row.project_id.clone()),
                visibility: "shared".into(),
                summary: format!(
                    "{} changed terms of deliverable \"{}\"",
                    actor.name, new_title
                ),
                before: Some(json!({
                    "title": row.title, "due_date": row.due_date,
                    "terms_version": row.terms_version,
                })),
                after: Some(json!({
                    "title": new_title, "due_date": new_due,
                    "terms_version": row.terms_version + 1,
                })),
                reason: req.reason.clone(),
            },
        )
        .await?;
    }

    tx.commit().await?;
    let row = load_deliverable(&state.pool, &deliverable_id).await?;
    Ok(Json(deliverable_dto(&state.pool, row).await?))
}

// ---------------------------------------------------------------------------
// POST /deliverables/{id}/agree — ack current terms_version
// ---------------------------------------------------------------------------

async fn agree_deliverable(
    State(state): State<AppState>,
    actor: Actor,
    Path(deliverable_id): Path<String>,
) -> AppResult<impl IntoResponse> {
    let row = load_deliverable(&state.pool, &deliverable_id).await?;
    let Some(side) = deliverable_side(&state.pool, &actor, &row.project_id).await? else {
        return Err(AppError::forbidden(
            "only team editors and coordinators can agree deliverables",
        ));
    };
    require_open(&row)?;

    let mut tx = db::begin_immediate(&state.pool).await?;
    let now = now_rfc3339();
    if side == "staff" {
        sqlx::query("UPDATE deliverables SET staff_agreed_at = ?, staff_agreed_by = ? WHERE id = ?")
    } else {
        sqlx::query("UPDATE deliverables SET team_agreed_at = ?, team_agreed_by = ? WHERE id = ?")
    }
    .bind(&now)
    .bind(&actor.user_id)
    .bind(&deliverable_id)
    .execute(&mut *tx)
    .await?;

    // Both sides acknowledged the current terms → `agreed` (from `proposed`;
    // submitted/changes_requested keep their status).
    let fresh = load_deliverable_tx(&mut tx, &deliverable_id).await?;
    let became_agreed = fresh.status == "proposed"
        && fresh.team_agreed_at.is_some()
        && fresh.staff_agreed_at.is_some();
    if became_agreed {
        sqlx::query("UPDATE deliverables SET status = 'agreed' WHERE id = ?")
            .bind(&deliverable_id)
            .execute(&mut *tx)
            .await?;
    }

    audit::record(
        &mut tx,
        AuditEvent {
            actor_id: Some(actor.user_id.clone()),
            actor_label: actor.name.clone(),
            action: "deliverable.agreed".into(),
            entity_type: "deliverable".into(),
            entity_id: deliverable_id.clone(),
            project_id: Some(row.project_id.clone()),
            visibility: "shared".into(),
            summary: format!(
                "{} agreed to deliverable \"{}\" (terms v{})",
                actor.name, row.title, row.terms_version
            ),
            before: None,
            after: Some(json!({"side": side, "terms_version": row.terms_version})),
            reason: None,
        },
    )
    .await?;

    if became_agreed {
        let other = if side == "staff" {
            row.sender_id.clone()
        } else {
            row.recipient_id.clone()
        };
        notify::notify(
            &mut tx,
            &other,
            "deliverable.agreed",
            "Deliverable agreed",
            &format!("Deliverable \"{}\" is now agreed by both sides.", row.title),
            &format!("/app/projects/{}/results", row.project_id),
            Some(&row.project_id),
        )
        .await?;
    }

    tx.commit().await?;
    let row = load_deliverable(&state.pool, &deliverable_id).await?;
    Ok(Json(deliverable_dto(&state.pool, row).await?))
}

async fn load_deliverable_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    id: &str,
) -> AppResult<DeliverableRow> {
    let row: Option<DeliverableRow> =
        sqlx::query_as(&format!("{DELIVERABLE_SELECT} WHERE d.id = ?"))
            .bind(id)
            .fetch_optional(&mut **tx)
            .await?;
    row.ok_or(AppError::NotFound)
}

// ---------------------------------------------------------------------------
// POST /deliverables/{id}/waive and /cancel (coordinator)
// ---------------------------------------------------------------------------

async fn resolve_deliverable(
    state: &AppState,
    actor: &Actor,
    deliverable_id: &str,
    to: &'static str,
    note: &str,
) -> AppResult<DeliverableDto> {
    let row = load_deliverable(&state.pool, deliverable_id).await?;
    require_coordinator(actor).await?;
    require_open(&row)?;

    let mut tx = db::begin_immediate(&state.pool).await?;
    sqlx::query("UPDATE deliverables SET status = ?, resolution_note = ? WHERE id = ?")
        .bind(to)
        .bind(note)
        .bind(deliverable_id)
        .execute(&mut *tx)
        .await?;

    audit::record(
        &mut tx,
        AuditEvent {
            actor_id: Some(actor.user_id.clone()),
            actor_label: actor.name.clone(),
            action: format!("deliverable.{to}"),
            entity_type: "deliverable".into(),
            entity_id: deliverable_id.to_string(),
            project_id: Some(row.project_id.clone()),
            visibility: "shared".into(),
            summary: format!("{} {to} deliverable \"{}\"", actor.name, row.title),
            before: Some(Value::String(row.status.clone())),
            after: Some(Value::String(to.to_string())),
            reason: Some(note.to_string()),
        },
    )
    .await?;

    notify::notify(
        &mut tx,
        &row.sender_id,
        format!("deliverable.{to}").as_str(),
        &format!("Deliverable {to}"),
        &format!("Deliverable \"{}\" was {to}: {note}", row.title),
        &format!("/app/projects/{}/results", row.project_id),
        Some(&row.project_id),
    )
    .await?;

    tx.commit().await?;
    let row = load_deliverable(&state.pool, deliverable_id).await?;
    deliverable_dto(&state.pool, row).await
}

async fn waive_deliverable(
    State(state): State<AppState>,
    actor: Actor,
    Path(deliverable_id): Path<String>,
    Json(req): Json<NoteRequest>,
) -> AppResult<impl IntoResponse> {
    let mut errors = FieldErrors::new();
    errors.require("note", &req.note, "a note is required");
    errors.finish()?;
    let dto = resolve_deliverable(&state, &actor, &deliverable_id, "waived", &req.note).await?;
    Ok(Json(dto))
}

async fn cancel_deliverable(
    State(state): State<AppState>,
    actor: Actor,
    Path(deliverable_id): Path<String>,
    Json(req): Json<NoteRequest>,
) -> AppResult<impl IntoResponse> {
    let mut errors = FieldErrors::new();
    errors.require("note", &req.note, "a note is required");
    errors.finish()?;
    let dto = resolve_deliverable(&state, &actor, &deliverable_id, "cancelled", &req.note).await?;
    Ok(Json(dto))
}

// ---------------------------------------------------------------------------
// POST /deliverables/{id}/submissions — team editor+
// ---------------------------------------------------------------------------

async fn create_submission(
    State(state): State<AppState>,
    actor: Actor,
    Path(deliverable_id): Path<String>,
    Json(req): Json<CreateSubmissionRequest>,
) -> AppResult<impl IntoResponse> {
    let row = load_deliverable(&state.pool, &deliverable_id).await?;

    let access = authz::project_access(&state.pool, &actor, &row.project_id).await?;
    if !matches!(access, ProjectAccess::TeamEditor | ProjectAccess::TeamLead) {
        return Err(AppError::forbidden("only team editors may submit results"));
    }
    if !matches!(row.status.as_str(), "agreed" | "changes_requested") {
        return Err(AppError::conflict(
            "invalid_transition",
            format!("deliverable is {}", row.status.replace('_', " ")),
        ));
    }

    let doc_version_ids = req.document_version_ids.clone().unwrap_or_default();
    let links = req.links.clone().unwrap_or_default();
    let data_dictionary = req.data_dictionary.clone().unwrap_or_default();

    let mut errors = FieldErrors::new();
    if doc_version_ids.is_empty() && links.is_empty() {
        errors.check("submission", false, "attach files or links");
    }
    for (i, l) in links.iter().enumerate() {
        if !deliverables::is_http_url(&l.url) {
            errors.check(&format!("links[{i}].url"), false, "must be an http(s) URL");
        }
        errors.check(
            &format!("links[{i}].description"),
            !l.description.trim().is_empty(),
            "description is required",
        );
        errors.check(
            &format!("links[{i}].version_label"),
            !l.version_label.trim().is_empty(),
            "version_label is required",
        );
    }
    for (i, e) in data_dictionary.iter().enumerate() {
        errors.check(
            &format!("data_dictionary[{i}].column"),
            !e.column.trim().is_empty(),
            "column is required",
        );
    }
    errors.finish()?;

    // Every referenced document version: belongs to THIS project (403/404 for
    // guessed ids), is a `result` document and its file is scan-clean (422
    // field errors for same-project-but-invalid).
    let mut field_errors = FieldErrors::new();
    for vid in &doc_version_ids {
        authz::ensure_document_version_in_project(&state.pool, vid, &row.project_id).await?;
        let info: Option<(String, String)> = sqlx::query_as(
            "SELECT d.category, f.scan_status
             FROM document_versions dv
             JOIN documents d ON d.id = dv.document_id
             JOIN files f ON f.id = dv.file_id
             WHERE dv.id = ?",
        )
        .bind(vid)
        .fetch_optional(&state.pool)
        .await?;
        if let Some((category, scan_status)) = info {
            field_errors.check(
                "document_version_ids",
                category == "result",
                "files must belong to result documents",
            );
            field_errors.check(
                "document_version_ids",
                scan_status == "clean",
                "files must have passed the antivirus scan",
            );
        }
    }
    field_errors.finish()?;

    let mut tx = db::begin_immediate(&state.pool).await?;
    let now = now_rfc3339();
    let number: i64 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(number), 0) + 1 FROM deliverable_submissions WHERE deliverable_id = ?",
    )
    .bind(&deliverable_id)
    .fetch_one(&mut *tx)
    .await?;

    let submission_id = new_id();
    sqlx::query(
        "INSERT INTO deliverable_submissions
         (id, deliverable_id, number, submitted_by, note, data_dictionary_json, status, created_at)
         VALUES (?, ?, ?, ?, ?, ?, 'received', ?)",
    )
    .bind(&submission_id)
    .bind(&deliverable_id)
    .bind(number)
    .bind(&actor.user_id)
    .bind(req.note.clone().unwrap_or_default())
    .bind(serde_json::to_string(&data_dictionary).map_err(AppError::internal)?)
    .bind(&now)
    .execute(&mut *tx)
    .await?;

    for vid in &doc_version_ids {
        sqlx::query(
            "INSERT INTO submission_files (id, submission_id, document_version_id, created_at)
             VALUES (?, ?, ?, ?)",
        )
        .bind(new_id())
        .bind(&submission_id)
        .bind(vid)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
    }

    let today = deliverables::today();
    for l in &links {
        let link_id = new_id();
        sqlx::query(
            "INSERT INTO external_links
             (id, submission_id, url, description, version_label, access_notes, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&link_id)
        .bind(&submission_id)
        .bind(&l.url)
        .bind(&l.description)
        .bind(&l.version_label)
        .bind(l.access_notes.clone().unwrap_or_default())
        .bind(&now)
        .execute(&mut *tx)
        .await?;
        jobs::enqueue(
            &mut tx,
            deliverables::KIND_CHECK_LINK,
            json!({"external_link_id": link_id}),
            Some(&format!("check_link:{link_id}:{today}")),
        )
        .await?;
    }

    sqlx::query("UPDATE deliverables SET status = 'submitted' WHERE id = ?")
        .bind(&deliverable_id)
        .execute(&mut *tx)
        .await?;

    audit::record(
        &mut tx,
        AuditEvent {
            actor_id: Some(actor.user_id.clone()),
            actor_label: actor.name.clone(),
            action: "submission.received".into(),
            entity_type: "deliverable_submission".into(),
            entity_id: submission_id.clone(),
            project_id: Some(row.project_id.clone()),
            visibility: "shared".into(),
            summary: format!(
                "{} submitted results for \"{}\" (#{number})",
                actor.name, row.title
            ),
            before: None,
            after: Some(json!({"number": number})),
            reason: None,
        },
    )
    .await?;

    notify::notify(
        &mut tx,
        &row.recipient_id,
        "submission.received",
        "Results submitted",
        &format!(
            "{} submitted results for deliverable \"{}\".",
            actor.name, row.title
        ),
        &format!("/app/projects/{}/results", row.project_id),
        Some(&row.project_id),
    )
    .await?;

    tx.commit().await?;
    let dto = load_submission_dto(&state.pool, &submission_id).await?;
    Ok((StatusCode::CREATED, Json(dto)))
}

// ---------------------------------------------------------------------------
// POST /submissions/{id}/request-changes and /accept (coordinator)
// ---------------------------------------------------------------------------

async fn load_submission_with_project(
    pool: &SqlitePool,
    submission_id: &str,
) -> AppResult<(SubmissionRow, DeliverableRow)> {
    let sub: Option<SubmissionRow> = sqlx::query_as(
        "SELECT id, deliverable_id, number, submitted_by, note, data_dictionary_json,
                status, reviewed_by, review_note, reviewed_at, created_at
         FROM deliverable_submissions WHERE id = ?",
    )
    .bind(submission_id)
    .fetch_optional(pool)
    .await?;
    let sub = sub.ok_or(AppError::NotFound)?;
    let del = load_deliverable(pool, &sub.deliverable_id).await?;
    Ok((sub, del))
}

async fn request_changes(
    State(state): State<AppState>,
    actor: Actor,
    Path(submission_id): Path<String>,
    Json(req): Json<NoteRequest>,
) -> AppResult<impl IntoResponse> {
    require_coordinator(&actor).await?;
    let mut errors = FieldErrors::new();
    errors.require("note", &req.note, "a note is required");
    errors.finish()?;

    let (sub, del) = load_submission_with_project(&state.pool, &submission_id).await?;
    if sub.status != "received" {
        return Err(AppError::conflict(
            "invalid_transition",
            format!("submission is {}", sub.status.replace('_', " ")),
        ));
    }

    let mut tx = db::begin_immediate(&state.pool).await?;
    let now = now_rfc3339();
    sqlx::query(
        "UPDATE deliverable_submissions
         SET status = 'changes_requested', reviewed_by = ?, review_note = ?, reviewed_at = ?
         WHERE id = ?",
    )
    .bind(&actor.user_id)
    .bind(&req.note)
    .bind(&now)
    .bind(&submission_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query("UPDATE deliverables SET status = 'changes_requested' WHERE id = ?")
        .bind(&del.id)
        .execute(&mut *tx)
        .await?;

    // Shared thread anchored to the deliverable so the team sees the request
    // ("Maria asks: add a description of observation sites").
    let thread_id: Option<String> = sqlx::query_scalar(
        "SELECT id FROM threads
         WHERE project_id = ? AND anchor_type = 'deliverable' AND anchor_key = ?
           AND visibility = 'shared' LIMIT 1",
    )
    .bind(&del.project_id)
    .bind(&del.id)
    .fetch_optional(&mut *tx)
    .await?;
    let thread_id = match thread_id {
        Some(t) => t,
        None => {
            let t = new_id();
            sqlx::query(
                "INSERT INTO threads (id, project_id, anchor_type, anchor_key, visibility, created_at)
                 VALUES (?, ?, 'deliverable', ?, 'shared', ?)",
            )
            .bind(&t)
            .bind(&del.project_id)
            .bind(&del.id)
            .bind(&now)
            .execute(&mut *tx)
            .await?;
            t
        }
    };
    sqlx::query(
        "INSERT INTO messages (id, thread_id, author_id, body, created_at)
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(new_id())
    .bind(&thread_id)
    .bind(&actor.user_id)
    .bind(&req.note)
    .bind(&now)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO action_items
         (id, project_id, thread_id, addressed_to, title, status, created_by, created_at)
         VALUES (?, ?, ?, 'team', ?, 'open', ?, ?)",
    )
    .bind(new_id())
    .bind(&del.project_id)
    .bind(&thread_id)
    .bind(&req.note)
    .bind(&actor.user_id)
    .bind(&now)
    .execute(&mut *tx)
    .await?;

    audit::record(
        &mut tx,
        AuditEvent {
            actor_id: Some(actor.user_id.clone()),
            actor_label: actor.name.clone(),
            action: "submission.changes_requested".into(),
            entity_type: "deliverable_submission".into(),
            entity_id: submission_id.clone(),
            project_id: Some(del.project_id.clone()),
            visibility: "shared".into(),
            summary: format!(
                "{} requested changes on submission #{} for \"{}\"",
                actor.name, sub.number, del.title
            ),
            before: None,
            after: Some(Value::String(req.note.clone())),
            reason: Some(req.note.clone()),
        },
    )
    .await?;

    notify_team(
        &mut tx,
        &del.project_id,
        "submission.changes_requested",
        "Changes requested on results",
        &format!("{} asks: {}", actor.name, req.note),
        &format!("/app/projects/{}/results", del.project_id),
    )
    .await?;

    tx.commit().await?;
    Ok(Json(
        load_submission_dto(&state.pool, &submission_id).await?,
    ))
}

async fn accept_submission(
    State(state): State<AppState>,
    actor: Actor,
    Path(submission_id): Path<String>,
) -> AppResult<impl IntoResponse> {
    require_coordinator(&actor).await?;
    let (sub, del) = load_submission_with_project(&state.pool, &submission_id).await?;
    if !matches!(sub.status.as_str(), "received" | "changes_requested") {
        return Err(AppError::conflict(
            "invalid_transition",
            format!("submission is {}", sub.status.replace('_', " ")),
        ));
    }

    let mut tx = db::begin_immediate(&state.pool).await?;
    let now = now_rfc3339();
    sqlx::query(
        "UPDATE deliverable_submissions
         SET status = 'accepted', reviewed_by = ?, reviewed_at = ?
         WHERE id = ?",
    )
    .bind(&actor.user_id)
    .bind(&now)
    .bind(&submission_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query("UPDATE deliverables SET status = 'accepted' WHERE id = ?")
        .bind(&del.id)
        .execute(&mut *tx)
        .await?;

    // The coordinator's acceptance resolves the team's open ask on this
    // deliverable's shared thread.
    sqlx::query(
        "UPDATE action_items SET status = 'resolved', resolved_by = ?, resolved_at = ?
         WHERE status = 'open' AND addressed_to = 'team' AND thread_id IN (
            SELECT id FROM threads
            WHERE project_id = ? AND anchor_type = 'deliverable' AND anchor_key = ?)",
    )
    .bind(&actor.user_id)
    .bind(&now)
    .bind(&del.project_id)
    .bind(&del.id)
    .execute(&mut *tx)
    .await?;

    audit::record(
        &mut tx,
        AuditEvent {
            actor_id: Some(actor.user_id.clone()),
            actor_label: actor.name.clone(),
            action: "submission.accepted".into(),
            entity_type: "deliverable_submission".into(),
            entity_id: submission_id.clone(),
            project_id: Some(del.project_id.clone()),
            visibility: "shared".into(),
            summary: format!(
                "{} accepted submission #{} for \"{}\" (receipt check)",
                actor.name, sub.number, del.title
            ),
            before: None,
            after: Some(Value::String("accepted".into())),
            reason: None,
        },
    )
    .await?;

    notify_team(
        &mut tx,
        &del.project_id,
        "submission.accepted",
        "Results accepted",
        &format!(
            "{} accepted submission #{} for deliverable \"{}\".",
            actor.name, sub.number, del.title
        ),
        &format!("/app/projects/{}/results", del.project_id),
    )
    .await?;

    tx.commit().await?;

    // Measurement CSVs: read files OUTSIDE any DB transaction (never hold a
    // tx across file IO), then insert valid rows. Invalid rows become
    // warnings and never fail the acceptance.
    let mut warnings = Vec::new();
    if del.kind == "dataset" {
        let (rows, warns) = deliverables::submission_measurements(
            &state.pool,
            &state.config.data_dir,
            &submission_id,
        )
        .await?;
        warnings = warns;
        if !rows.is_empty() {
            let reference: Option<String> =
                sqlx::query_scalar("SELECT reference FROM projects WHERE id = ?")
                    .bind(&del.project_id)
                    .fetch_one(&state.pool)
                    .await?;
            let source_label = format!(
                "{} — {} (submission #{})",
                reference.unwrap_or_else(|| "unreferenced project".into()),
                del.title,
                sub.number
            );
            deliverables::insert_measurements(
                &state.pool,
                &del.project_id,
                &del.id,
                &submission_id,
                &source_label,
                &rows,
            )
            .await?;
        }
    }

    let dto = load_submission_dto(&state.pool, &submission_id).await?;
    Ok(Json(AcceptSubmissionResponse {
        submission: dto,
        message: "Submission marked as received. Acceptance confirms receipt of the \
                  results, not their scientific validity."
            .into(),
        warnings,
    }))
}

// ---------------------------------------------------------------------------
// Publication (coordinator)
// ---------------------------------------------------------------------------

async fn update_publication(
    State(state): State<AppState>,
    actor: Actor,
    Path(deliverable_id): Path<String>,
    Json(req): Json<UpdatePublicationRequest>,
) -> AppResult<impl IntoResponse> {
    require_coordinator(&actor).await?;
    let row = load_deliverable(&state.pool, &deliverable_id).await?;
    if row.status != "accepted" {
        return Err(AppError::conflict(
            "invalid_transition",
            "only an accepted deliverable can be published",
        ));
    }

    let mut errors = FieldErrors::new();
    errors.check(
        "publish_level",
        matches!(
            req.publish_level.as_str(),
            "none" | "metadata" | "metadata_and_files"
        ),
        "must be one of none, metadata, metadata_and_files",
    );
    if let Some(e) = &req.embargo_until {
        errors.valid_date("embargo_until", e);
    }
    errors.finish()?;

    let mut tx = db::begin_immediate(&state.pool).await?;
    let now = now_rfc3339();
    let publishing = req.publish_level != "none";
    sqlx::query(
        "UPDATE deliverables
         SET publish_level = ?, embargo_until = ?,
             published_at = CASE WHEN ? THEN ? ELSE published_at END,
             published_by = CASE WHEN ? THEN ? ELSE published_by END
         WHERE id = ?",
    )
    .bind(&req.publish_level)
    .bind(&req.embargo_until)
    .bind(publishing)
    .bind(&now)
    .bind(publishing)
    .bind(&actor.user_id)
    .bind(&deliverable_id)
    .execute(&mut *tx)
    .await?;

    audit::record(
        &mut tx,
        AuditEvent {
            actor_id: Some(actor.user_id.clone()),
            actor_label: actor.name.clone(),
            action: "deliverable.publication_changed".into(),
            entity_type: "deliverable".into(),
            entity_id: deliverable_id.clone(),
            project_id: Some(row.project_id.clone()),
            visibility: "shared".into(),
            summary: format!(
                "{} set publication of \"{}\" to {}",
                actor.name,
                row.title,
                req.publish_level.replace('_', " ")
            ),
            before: Some(json!({"publish_level": row.publish_level})),
            after: Some(
                json!({"publish_level": req.publish_level, "embargo_until": req.embargo_until}),
            ),
            reason: None,
        },
    )
    .await?;

    tx.commit().await?;
    let row = load_deliverable(&state.pool, &deliverable_id).await?;
    Ok(Json(PublicationUpdateResponse {
        deliverable: deliverable_dto(&state.pool, row).await?,
        warning: PUBLICATION_WARNING.into(),
    }))
}

async fn set_publication_files(
    State(state): State<AppState>,
    actor: Actor,
    Path(deliverable_id): Path<String>,
    Json(req): Json<SetPublicationFilesRequest>,
) -> AppResult<impl IntoResponse> {
    require_coordinator(&actor).await?;
    let row = load_deliverable(&state.pool, &deliverable_id).await?;
    if row.status != "accepted" {
        return Err(AppError::conflict(
            "invalid_transition",
            "only an accepted deliverable can publish files",
        ));
    }

    // The accepted submission (latest accepted per §4) defines which files
    // may be published.
    let accepted: Option<(String,)> = sqlx::query_as(
        "SELECT id FROM deliverable_submissions
         WHERE deliverable_id = ? AND status = 'accepted'
         ORDER BY number DESC LIMIT 1",
    )
    .bind(&deliverable_id)
    .fetch_optional(&state.pool)
    .await?;
    let Some((accepted_id,)) = accepted else {
        return Err(AppError::conflict(
            "invalid_transition",
            "no accepted submission for this deliverable",
        ));
    };
    let allowed: std::collections::HashSet<String> = sqlx::query_scalar(
        "SELECT document_version_id FROM submission_files WHERE submission_id = ?",
    )
    .bind(&accepted_id)
    .fetch_all(&state.pool)
    .await?
    .into_iter()
    .collect();
    let mut errors = FieldErrors::new();
    for vid in &req.document_version_ids {
        errors.check(
            "document_version_ids",
            allowed.contains(vid),
            "files must come from the accepted submission",
        );
    }
    errors.finish()?;

    let mut tx = db::begin_immediate(&state.pool).await?;
    let now = now_rfc3339();
    sqlx::query("DELETE FROM publication_files WHERE deliverable_id = ?")
        .bind(&deliverable_id)
        .execute(&mut *tx)
        .await?;
    for vid in &req.document_version_ids {
        sqlx::query(
            "INSERT INTO publication_files
             (id, deliverable_id, document_version_id, approved_by, approved_at, created_at)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(new_id())
        .bind(&deliverable_id)
        .bind(vid)
        .bind(&actor.user_id)
        .bind(&now)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
    }

    audit::record(
        &mut tx,
        AuditEvent {
            actor_id: Some(actor.user_id.clone()),
            actor_label: actor.name.clone(),
            action: "deliverable.publication_files_set".into(),
            entity_type: "deliverable".into(),
            entity_id: deliverable_id.clone(),
            project_id: Some(row.project_id.clone()),
            visibility: "internal".into(),
            summary: format!(
                "{} selected {} file(s) of \"{}\" for publication",
                actor.name,
                req.document_version_ids.len(),
                row.title
            ),
            before: None,
            after: Some(json!({"document_version_ids": req.document_version_ids})),
            reason: None,
        },
    )
    .await?;

    tx.commit().await?;
    Ok(Json(PublicationFilesResponse {
        document_version_ids: req.document_version_ids,
        warning: PUBLICATION_WARNING.into(),
    }))
}

// ---------------------------------------------------------------------------
// POST /external-links/{id}/check (coordinator)
// ---------------------------------------------------------------------------

async fn check_link_now(
    State(state): State<AppState>,
    actor: Actor,
    Path(link_id): Path<String>,
) -> AppResult<impl IntoResponse> {
    require_coordinator(&actor).await?;
    let exists: Option<(String,)> = sqlx::query_as("SELECT id FROM external_links WHERE id = ?")
        .bind(&link_id)
        .fetch_optional(&state.pool)
        .await?;
    if exists.is_none() {
        return Err(AppError::NotFound);
    }
    deliverables::run_link_check(&state, &link_id).await?;
    let row: ExternalLinkRow = sqlx::query_as(
        "SELECT id, submission_id, url, description, version_label, access_notes,
                last_checked_at, last_status, available
         FROM external_links WHERE id = ?",
    )
    .bind(&link_id)
    .fetch_one(&state.pool)
    .await?;
    Ok(Json(ExternalLinkDto::from(row)))
}

// ---------------------------------------------------------------------------
// POST /projects/{id}/close (coordinator)
// ---------------------------------------------------------------------------

async fn close_project(
    State(state): State<AppState>,
    actor: Actor,
    Path(project_id): Path<String>,
    Json(req): Json<CloseProjectRequest>,
) -> AppResult<impl IntoResponse> {
    require_coordinator(&actor).await?;

    let exists: Option<(String,)> = sqlx::query_as("SELECT id FROM projects WHERE id = ?")
        .bind(&project_id)
        .fetch_optional(&state.pool)
        .await?;
    if exists.is_none() {
        return Err(AppError::NotFound);
    }

    let mut errors = FieldErrors::new();
    for (i, r) in req.deliverable_resolutions.iter().enumerate() {
        errors.check(
            &format!("deliverable_resolutions[{i}].action"),
            matches!(r.action.as_str(), "waive" | "cancel"),
            "must be waive or cancel",
        );
        errors.check(
            &format!("deliverable_resolutions[{i}].note"),
            !r.note.trim().is_empty(),
            "a note is required",
        );
    }
    errors.finish()?;

    let mut tx = db::begin_immediate(&state.pool).await?;

    // Open deliverables not covered by a resolution block closing.
    let open: Vec<(String, String, String, String)> = sqlx::query_as(
        "SELECT id, title, status, due_date FROM deliverables
         WHERE project_id = ? AND status IN ('proposed','agreed','submitted','changes_requested')",
    )
    .bind(&project_id)
    .fetch_all(&mut *tx)
    .await?;
    let resolved_ids: std::collections::HashSet<&str> = req
        .deliverable_resolutions
        .iter()
        .map(|r| r.deliverable_id.as_str())
        .collect();
    let unresolved: Vec<UnresolvedDeliverableDto> = open
        .iter()
        .filter(|(id, ..)| !resolved_ids.contains(id.as_str()))
        .map(|(id, title, status, due)| UnresolvedDeliverableDto {
            id: id.clone(),
            title: title.clone(),
            status: status.clone(),
            due_date: due.clone(),
        })
        .collect();
    if !unresolved.is_empty() {
        let body = json!({"error": {
            "code": "unresolved_deliverables",
            "message": format!("{} deliverable(s) still need a resolution", unresolved.len()),
            "deliverables": unresolved,
        }});
        return Ok((StatusCode::CONFLICT, Json(body)).into_response());
    }

    // Every resolution must target an open deliverable of THIS project.
    let open_ids: std::collections::HashSet<&str> =
        open.iter().map(|(id, ..)| id.as_str()).collect();
    for r in &req.deliverable_resolutions {
        if !open_ids.contains(r.deliverable_id.as_str()) {
            return Err(AppError::conflict(
                "invalid_transition",
                format!(
                    "deliverable {} is not an open deliverable of this project",
                    r.deliverable_id
                ),
            ));
        }
        let to = if r.action == "waive" {
            "waived"
        } else {
            "cancelled"
        };
        sqlx::query("UPDATE deliverables SET status = ?, resolution_note = ? WHERE id = ?")
            .bind(to)
            .bind(&r.note)
            .bind(&r.deliverable_id)
            .execute(&mut *tx)
            .await?;
        audit::record(
            &mut tx,
            AuditEvent {
                actor_id: Some(actor.user_id.clone()),
                actor_label: actor.name.clone(),
                action: format!("deliverable.{to}"),
                entity_type: "deliverable".into(),
                entity_id: r.deliverable_id.clone(),
                project_id: Some(project_id.clone()),
                visibility: "shared".into(),
                summary: format!("{} {to} deliverable at close: {}", actor.name, r.note),
                before: None,
                after: Some(Value::String(to.to_string())),
                reason: Some(r.note.clone()),
            },
        )
        .await?;
        let sender: Option<String> =
            sqlx::query_scalar("SELECT sender_id FROM deliverables WHERE id = ?")
                .bind(&r.deliverable_id)
                .fetch_optional(&mut *tx)
                .await?;
        if let Some(sender) = sender {
            notify::notify(
                &mut tx,
                &sender,
                "deliverable.resolved",
                "Deliverable resolved",
                &format!("A deliverable was {to} during project close: {}", r.note),
                &format!("/app/projects/{project_id}/results"),
                Some(&project_id),
            )
            .await?;
        }
    }

    projects::transition(&mut tx, &project_id, projects::Action::Close, &actor, None).await?;
    tx.commit().await?;

    Ok(
        Json(crate::routes::projects::load_workspace(&state, &actor, &project_id).await?)
            .into_response(),
    )
}

// ---------------------------------------------------------------------------
// Workspace section for GET /projects/{id} (slice C §9)
// ---------------------------------------------------------------------------

/// Results section: deliverables with due dates, agreement state,
/// latest/accepted submission and publication state; samples count.
pub async fn workspace_section(
    pool: &SqlitePool,
    project_id: &str,
) -> AppResult<ResultsSectionDto> {
    let rows: Vec<DeliverableRow> = sqlx::query_as(&format!(
        "{DELIVERABLE_SELECT} WHERE d.project_id = ?
         ORDER BY d.due_date ASC, d.created_at ASC"
    ))
    .bind(project_id)
    .fetch_all(pool)
    .await?;
    let mut deliverables = Vec::new();
    for row in rows {
        deliverables.push(deliverable_dto(pool, row).await?);
    }
    let samples_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM samples WHERE project_id = ?")
            .bind(project_id)
            .fetch_one(pool)
            .await?;
    Ok(ResultsSectionDto {
        deliverables,
        samples_count,
    })
}

// ---------------------------------------------------------------------------
// Samples
// ---------------------------------------------------------------------------

#[derive(FromRow)]
#[allow(dead_code)]
struct SampleRow {
    id: String,
    project_id: String,
    code: String,
    site_id: Option<String>,
    collected_on: Option<String>,
    material: String,
    custodian_org: String,
    storage_location: String,
    notes: String,
    related_deliverable_ids_json: String,
    created_at: String,
}

fn sample_dto(row: SampleRow) -> SampleDto {
    SampleDto {
        id: row.id,
        project_id: row.project_id,
        code: row.code,
        site_id: row.site_id,
        collected_on: row.collected_on,
        material: row.material,
        custodian_org: row.custodian_org,
        storage_location: row.storage_location,
        notes: row.notes,
        related_deliverable_ids: serde_json::from_str(&row.related_deliverable_ids_json)
            .unwrap_or_default(),
        created_at: row.created_at,
    }
}

async fn load_sample(pool: &SqlitePool, id: &str) -> AppResult<SampleRow> {
    let row: Option<SampleRow> = sqlx::query_as(
        "SELECT id, project_id, code, site_id, collected_on, material, custodian_org,
                storage_location, notes, related_deliverable_ids_json, created_at
         FROM samples WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    row.ok_or(AppError::NotFound)
}

/// May the actor manage samples on this project? Team editor+ or coordinator.
async fn can_manage_samples(pool: &SqlitePool, actor: &Actor, project_id: &str) -> AppResult<bool> {
    Ok(deliverable_side(pool, actor, project_id).await?.is_some())
}

async fn validate_sample_refs(
    pool: &SqlitePool,
    project_id: &str,
    site_id: Option<&String>,
    deliverable_ids: &[String],
) -> AppResult<()> {
    let mut errors = FieldErrors::new();
    if let Some(site_id) = site_id {
        let ok: Option<(String,)> =
            sqlx::query_as("SELECT id FROM project_sites WHERE id = ? AND project_id = ?")
                .bind(site_id)
                .bind(project_id)
                .fetch_optional(pool)
                .await?;
        errors.check("site_id", ok.is_some(), "site not found in this project");
    }
    for did in deliverable_ids {
        let ok: Option<(String,)> =
            sqlx::query_as("SELECT id FROM deliverables WHERE id = ? AND project_id = ?")
                .bind(did)
                .bind(project_id)
                .fetch_optional(pool)
                .await?;
        errors.check(
            "related_deliverable_ids",
            ok.is_some(),
            "deliverables must belong to this project",
        );
    }
    errors.finish()
}

async fn list_samples(
    State(state): State<AppState>,
    actor: Actor,
    Path(project_id): Path<String>,
    axum::extract::Query(query): axum::extract::Query<ListQuery>,
) -> AppResult<Json<ListResponse<SampleDto>>> {
    let access = authz::project_access(&state.pool, &actor, &project_id).await?;
    if access == ProjectAccess::None {
        return Err(AppError::forbidden(
            "you do not have access to this project",
        ));
    }
    let limit = query.limit();
    let offset = query.offset();
    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM samples WHERE project_id = ?")
        .bind(&project_id)
        .fetch_one(&state.pool)
        .await?;
    let rows: Vec<SampleRow> = sqlx::query_as(
        "SELECT id, project_id, code, site_id, collected_on, material, custodian_org,
                storage_location, notes, related_deliverable_ids_json, created_at
         FROM samples WHERE project_id = ? ORDER BY code LIMIT ? OFFSET ?",
    )
    .bind(&project_id)
    .bind(limit)
    .bind(offset)
    .fetch_all(&state.pool)
    .await?;
    let items = rows.into_iter().map(sample_dto).collect();
    Ok(Json(ListResponse { items, total }))
}

async fn create_sample(
    State(state): State<AppState>,
    actor: Actor,
    Path(project_id): Path<String>,
    Json(req): Json<crate::dto::CreateSampleRequest>,
) -> AppResult<impl IntoResponse> {
    if !can_manage_samples(&state.pool, &actor, &project_id).await? {
        return Err(AppError::forbidden(
            "only team editors and coordinators may manage samples",
        ));
    }
    let exists: Option<(String,)> = sqlx::query_as("SELECT id FROM projects WHERE id = ?")
        .bind(&project_id)
        .fetch_optional(&state.pool)
        .await?;
    if exists.is_none() {
        return Err(AppError::NotFound);
    }

    let mut errors = FieldErrors::new();
    errors.require("code", &req.code, "code is required");
    errors.max_len("code", &req.code, 100);
    if let Some(d) = &req.collected_on {
        errors.valid_date("collected_on", d);
    }
    errors.finish()?;
    let related = req.related_deliverable_ids.clone().unwrap_or_default();
    validate_sample_refs(&state.pool, &project_id, req.site_id.as_ref(), &related).await?;

    let mut tx = db::begin_immediate(&state.pool).await?;
    let id = new_id();
    let now = now_rfc3339();
    sqlx::query(
        "INSERT INTO samples
         (id, project_id, code, site_id, collected_on, material, custodian_org,
          storage_location, notes, related_deliverable_ids_json, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&project_id)
    .bind(&req.code)
    .bind(&req.site_id)
    .bind(&req.collected_on)
    .bind(req.material.unwrap_or_default())
    .bind(req.custodian_org.unwrap_or_default())
    .bind(req.storage_location.unwrap_or_default())
    .bind(req.notes.unwrap_or_default())
    .bind(serde_json::to_string(&related).map_err(AppError::internal)?)
    .bind(&now)
    .execute(&mut *tx)
    .await?;

    audit::record(
        &mut tx,
        AuditEvent {
            actor_id: Some(actor.user_id.clone()),
            actor_label: actor.name.clone(),
            action: "sample.created".into(),
            entity_type: "sample".into(),
            entity_id: id.clone(),
            project_id: Some(project_id.clone()),
            visibility: "shared".into(),
            summary: format!("{} registered sample {}", actor.name, req.code),
            before: None,
            after: Some(Value::String(req.code.clone())),
            reason: None,
        },
    )
    .await?;

    tx.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(sample_dto(load_sample(&state.pool, &id).await?)),
    ))
}

async fn patch_sample(
    State(state): State<AppState>,
    actor: Actor,
    Path(sample_id): Path<String>,
    Json(req): Json<crate::dto::UpdateSampleRequest>,
) -> AppResult<impl IntoResponse> {
    let row = load_sample(&state.pool, &sample_id).await?;
    if !can_manage_samples(&state.pool, &actor, &row.project_id).await? {
        return Err(AppError::forbidden(
            "only team editors and coordinators may manage samples",
        ));
    }

    let mut errors = FieldErrors::new();
    if let Some(code) = &req.code {
        errors.require("code", code, "code is required");
        errors.max_len("code", code, 100);
    }
    if let Some(Some(d)) = &req.collected_on {
        errors.valid_date("collected_on", d);
    }
    errors.finish()?;

    let new_site: Option<String> = match &req.site_id {
        Some(v) => v.clone(),
        None => row.site_id.clone(),
    };
    let new_related = req.related_deliverable_ids.clone().unwrap_or_else(|| {
        serde_json::from_str(&row.related_deliverable_ids_json).unwrap_or_default()
    });
    validate_sample_refs(
        &state.pool,
        &row.project_id,
        new_site.as_ref(),
        &new_related,
    )
    .await?;

    let new_collected: Option<String> = match &req.collected_on {
        Some(v) => v.clone(),
        None => row.collected_on.clone(),
    };

    let mut tx = db::begin_immediate(&state.pool).await?;
    sqlx::query(
        "UPDATE samples SET code = ?, site_id = ?, collected_on = ?, material = ?,
                custodian_org = ?, storage_location = ?, notes = ?,
                related_deliverable_ids_json = ?
         WHERE id = ?",
    )
    .bind(req.code.clone().unwrap_or_else(|| row.code.clone()))
    .bind(&new_site)
    .bind(&new_collected)
    .bind(req.material.clone().unwrap_or_else(|| row.material.clone()))
    .bind(
        req.custodian_org
            .clone()
            .unwrap_or_else(|| row.custodian_org.clone()),
    )
    .bind(
        req.storage_location
            .clone()
            .unwrap_or_else(|| row.storage_location.clone()),
    )
    .bind(req.notes.clone().unwrap_or_else(|| row.notes.clone()))
    .bind(serde_json::to_string(&new_related).map_err(AppError::internal)?)
    .bind(&sample_id)
    .execute(&mut *tx)
    .await?;

    audit::record(
        &mut tx,
        AuditEvent {
            actor_id: Some(actor.user_id.clone()),
            actor_label: actor.name.clone(),
            action: "sample.updated".into(),
            entity_type: "sample".into(),
            entity_id: sample_id.clone(),
            project_id: Some(row.project_id.clone()),
            visibility: "shared".into(),
            summary: format!("{} updated sample {}", actor.name, row.code),
            before: None,
            after: None,
            reason: None,
        },
    )
    .await?;

    tx.commit().await?;
    Ok(Json(sample_dto(
        load_sample(&state.pool, &sample_id).await?,
    )))
}

async fn delete_sample(
    State(state): State<AppState>,
    actor: Actor,
    Path(sample_id): Path<String>,
) -> AppResult<impl IntoResponse> {
    let row = load_sample(&state.pool, &sample_id).await?;
    if !can_manage_samples(&state.pool, &actor, &row.project_id).await? {
        return Err(AppError::forbidden(
            "only team editors and coordinators may manage samples",
        ));
    }

    let mut tx = db::begin_immediate(&state.pool).await?;
    audit::record(
        &mut tx,
        AuditEvent {
            actor_id: Some(actor.user_id.clone()),
            actor_label: actor.name.clone(),
            action: "sample.deleted".into(),
            entity_type: "sample".into(),
            entity_id: sample_id.clone(),
            project_id: Some(row.project_id.clone()),
            visibility: "shared".into(),
            summary: format!("{} deleted sample {}", actor.name, row.code),
            before: Some(Value::String(row.code.clone())),
            after: None,
            reason: None,
        },
    )
    .await?;
    sqlx::query("DELETE FROM samples WHERE id = ?")
        .bind(&sample_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
