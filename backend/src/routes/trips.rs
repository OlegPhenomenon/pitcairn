//! Trips, bookings, the base calendar and the provider view
//! (architecture §4 "Trips, resources, bookings", §3 operation matrix).
//!
//! Invariants enforced here:
//! - Trip creation and booking requests open once the project is `in_review`
//!   (planning runs in parallel with review); confirmation needs `approved`.
//! - Booking intervals are half-open `[start_date, end_date)`; confirmation
//!   checks peak per-day usage inside `BEGIN IMMEDIATE`.
//! - Provider resources are decided only by their owning provider; all other
//!   resources by a `base_manager`.
//! - Trip dates are directly editable only while the trip has no `confirmed`
//!   bookings — after that they move via an approved `reschedule_trip` change
//!   request (§5); PATCH then answers 409 `use_change_request`.
//! - Cancelling a trip releases its live bookings (`requested`/`confirmed` →
//!   `released`); PATCH that shrinks dates releases `requested` bookings that
//!   would fall outside the new range so "booking within trip dates" always
//!   holds.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use sqlx::FromRow;

use crate::AppState;
use crate::audit::{self, AuditEvent};
use crate::authz::{self, Actor, ProjectAccess};
use crate::db;
use crate::dto::{
    BookingDto, CalendarBookingDto, CalendarDayDto, CalendarQuery, CalendarResourceDto,
    CalendarResponse, CreateBookingRequest, CreateTripRequest, DeclineBookingRequest, ListResponse,
    PatchTripRequest, ProviderBookingDto, TripDto,
};
use crate::error::{AppError, AppResult};
use crate::notify;
use crate::util::{new_id, now_rfc3339};
use crate::validation::FieldErrors;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/projects/{id}/trips", get(list_trips).post(create_trip))
        .route("/trips/{id}", patch(patch_trip))
        .route("/trips/{id}/cancel", post(cancel_trip))
        .route("/trips/{id}/complete", post(complete_trip))
        .route("/trips/{id}/bookings", post(create_booking))
        .route("/bookings/{id}/confirm", post(confirm_booking))
        .route("/bookings/{id}/decline", post(decline_booking))
        .route("/bookings/{id}/cancel", post(cancel_booking))
        .route("/calendar", get(calendar))
        .route("/provider/bookings", get(provider_bookings))
}

/// Project statuses in which trips/bookings may be planned (from `in_review`
/// onward so planning runs in parallel with the decision, §3 note).
const PLANNABLE_STATUSES: &[&str] = &["in_review", "changes_requested", "approved"];

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Team editors+ and the coordinator may create/edit trips & bookings.
fn can_edit_trips(access: ProjectAccess, actor: &Actor) -> bool {
    matches!(access, ProjectAccess::TeamEditor | ProjectAccess::TeamLead) || actor.is_coordinator()
}

fn parse_date(s: &str) -> Option<chrono::NaiveDate> {
    chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()
}

fn date_str(d: chrono::NaiveDate) -> String {
    d.format("%Y-%m-%d").to_string()
}

async fn role_user_ids<'e, E>(exec: E, role: &str) -> AppResult<Vec<String>>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    let ids: Vec<String> =
        sqlx::query_scalar("SELECT user_id FROM user_roles WHERE role = ? AND revoked_at IS NULL")
            .bind(role)
            .fetch_all(exec)
            .await?;
    Ok(ids)
}

async fn project_team_ids<'e, E>(exec: E, project_id: &str) -> AppResult<Vec<String>>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    let ids: Vec<String> = sqlx::query_scalar(
        "SELECT user_id FROM project_members
         WHERE project_id = ? AND removed_at IS NULL",
    )
    .bind(project_id)
    .fetch_all(exec)
    .await?;
    Ok(ids)
}

/// Notify whoever decides on bookings for a resource: its provider when it is
/// provider-owned, else every active base_manager.
async fn notify_decider(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    provider_user_id: Option<&str>,
    kind: &str,
    title: &str,
    body: &str,
    project_id: &str,
) -> AppResult<()> {
    if let Some(provider) = provider_user_id {
        notify::notify(
            tx,
            provider,
            kind,
            title,
            body,
            "/app/provider",
            Some(project_id),
        )
        .await?;
    } else {
        for uid in role_user_ids(&mut **tx, "base_manager").await? {
            notify::notify(
                tx,
                &uid,
                kind,
                title,
                body,
                "/app/calendar",
                Some(project_id),
            )
            .await?;
        }
    }
    Ok(())
}

async fn notify_team(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    project_id: &str,
    kind: &str,
    title: &str,
    body: &str,
    link: &str,
) -> AppResult<()> {
    for uid in project_team_ids(&mut **tx, project_id).await? {
        notify::notify(tx, &uid, kind, title, body, link, Some(project_id)).await?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Loading
// ---------------------------------------------------------------------------

#[derive(FromRow)]
#[allow(dead_code)]
struct TripRow {
    id: String,
    project_id: String,
    title: String,
    arrive_date: String,
    depart_date: String,
    participants_json: String,
    status: String,
    created_at: String,
}

#[derive(FromRow)]
#[allow(dead_code)]
struct BookingRow {
    id: String,
    trip_id: String,
    resource_id: String,
    resource_name: String,
    resource_kind: String,
    provider_user_id: Option<String>,
    start_date: String,
    end_date: String,
    quantity: i64,
    status: String,
    requested_by: String,
    decided_by: Option<String>,
    decided_at: Option<String>,
    decline_reason: Option<String>,
    created_at: String,
}

const BOOKING_SELECT: &str =
    "SELECT b.id, b.trip_id, b.resource_id, r.name AS resource_name, r.kind AS resource_kind,
            r.provider_user_id, b.start_date, b.end_date, b.quantity, b.status,
            b.requested_by, b.decided_by, b.decided_at, b.decline_reason, b.created_at
     FROM bookings b JOIN resources r ON r.id = b.resource_id";

fn booking_dto(row: BookingRow) -> BookingDto {
    BookingDto {
        id: row.id,
        trip_id: row.trip_id,
        resource_id: row.resource_id,
        resource_name: row.resource_name,
        resource_kind: row.resource_kind,
        start_date: row.start_date,
        end_date: row.end_date,
        quantity: row.quantity,
        status: row.status,
        requested_by: row.requested_by,
        decided_by: row.decided_by,
        decided_at: row.decided_at,
        decline_reason: row.decline_reason,
        created_at: row.created_at,
    }
}

async fn load_booking_row(pool: &sqlx::SqlitePool, booking_id: &str) -> AppResult<BookingRow> {
    sqlx::query_as(&format!("{BOOKING_SELECT} WHERE b.id = ?"))
        .bind(booking_id)
        .fetch_optional(pool)
        .await?
        .ok_or(AppError::NotFound)
}

async fn load_booking_dto(pool: &sqlx::SqlitePool, booking_id: &str) -> AppResult<BookingDto> {
    Ok(booking_dto(load_booking_row(pool, booking_id).await?))
}

fn trip_dto(row: TripRow, bookings: Vec<BookingDto>) -> AppResult<TripDto> {
    Ok(TripDto {
        id: row.id,
        project_id: row.project_id,
        title: row.title,
        arrive_date: row.arrive_date,
        depart_date: row.depart_date,
        participants: serde_json::from_str(&row.participants_json).unwrap_or_default(),
        status: row.status,
        bookings,
        created_at: row.created_at,
    })
}

async fn trips_for_project(pool: &sqlx::SqlitePool, project_id: &str) -> AppResult<Vec<TripDto>> {
    let trips: Vec<TripRow> = sqlx::query_as(
        "SELECT id, project_id, title, arrive_date, depart_date, participants_json, status, created_at
         FROM trips WHERE project_id = ? ORDER BY arrive_date, created_at",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await?;
    let mut out = Vec::new();
    for trip in trips {
        let bookings: Vec<BookingRow> = sqlx::query_as(&format!(
            "{BOOKING_SELECT} WHERE b.trip_id = ? ORDER BY b.start_date, b.created_at"
        ))
        .bind(&trip.id)
        .fetch_all(pool)
        .await?;
        out.push(trip_dto(
            trip,
            bookings.into_iter().map(booking_dto).collect(),
        )?);
    }
    Ok(out)
}

async fn load_trip(pool: &sqlx::SqlitePool, trip_id: &str) -> AppResult<TripRow> {
    sqlx::query_as(
        "SELECT id, project_id, title, arrive_date, depart_date, participants_json, status, created_at
         FROM trips WHERE id = ?",
    )
    .bind(trip_id)
    .fetch_optional(pool)
    .await?
    .ok_or(AppError::NotFound)
}

async fn project_status<'e, E>(exec: E, project_id: &str) -> AppResult<String>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    sqlx::query_scalar("SELECT status FROM projects WHERE id = ?")
        .bind(project_id)
        .fetch_optional(exec)
        .await?
        .ok_or(AppError::NotFound)
}

// ---------------------------------------------------------------------------
// Trips
// ---------------------------------------------------------------------------

async fn list_trips(
    State(state): State<AppState>,
    actor: Actor,
    Path(project_id): Path<String>,
) -> AppResult<Json<ListResponse<TripDto>>> {
    let access = authz::project_access(&state.pool, &actor, &project_id).await?;
    // Team members and staff see trips; experts/outsiders do not.
    if access < ProjectAccess::TeamViewer {
        return Err(AppError::forbidden(
            "you do not have access to this project",
        ));
    }
    let items = trips_for_project(&state.pool, &project_id).await?;
    let total = items.len() as i64;
    Ok(Json(ListResponse { items, total }))
}

fn validate_trip_fields(errors: &mut FieldErrors, title: &str, arrive: &str, depart: &str) {
    errors.require("title", title, "title is required");
    errors.max_len("title", title, 300);
    errors.valid_date("arrive_date", arrive);
    errors.valid_date("depart_date", depart);
    if let (Some(a), Some(d)) = (parse_date(arrive), parse_date(depart)) {
        errors.check(
            "depart_date",
            a < d,
            "depart_date must be after arrive_date",
        );
    }
}

async fn create_trip(
    State(state): State<AppState>,
    actor: Actor,
    Path(project_id): Path<String>,
    Json(req): Json<CreateTripRequest>,
) -> AppResult<impl IntoResponse> {
    let access = authz::project_access(&state.pool, &actor, &project_id).await?;
    if !can_edit_trips(access, &actor) {
        return Err(AppError::forbidden(
            "trip planning requires team editor or coordinator",
        ));
    }

    let status = project_status(&state.pool, &project_id).await?;
    if !PLANNABLE_STATUSES.contains(&status.as_str()) {
        return Err(AppError::conflict(
            "invalid_state",
            format!("trips can be planned once the project is in review (currently {status})"),
        ));
    }

    let mut errors = FieldErrors::new();
    validate_trip_fields(&mut errors, &req.title, &req.arrive_date, &req.depart_date);
    errors.finish()?;

    let participants =
        serde_json::to_string(&req.participants.unwrap_or_default()).map_err(AppError::internal)?;

    let id = new_id();
    let now = now_rfc3339();
    let mut tx = db::begin_immediate(&state.pool).await?;
    sqlx::query(
        "INSERT INTO trips (id, project_id, title, arrive_date, depart_date, participants_json, status, created_at)
         VALUES (?, ?, ?, ?, ?, ?, 'planned', ?)",
    )
    .bind(&id)
    .bind(&project_id)
    .bind(&req.title)
    .bind(&req.arrive_date)
    .bind(&req.depart_date)
    .bind(&participants)
    .bind(&now)
    .execute(&mut *tx)
    .await?;

    audit::record(
        &mut tx,
        AuditEvent {
            actor_id: Some(actor.user_id.clone()),
            actor_label: actor.name.clone(),
            action: "trip.created".into(),
            entity_type: "trip".into(),
            entity_id: id.clone(),
            project_id: Some(project_id.clone()),
            visibility: "shared".into(),
            summary: format!(
                "{} planned trip '{}' ({} → {})",
                actor.name, req.title, req.arrive_date, req.depart_date
            ),
            before: None,
            after: Some(serde_json::json!({
                "title": req.title,
                "arrive_date": req.arrive_date,
                "depart_date": req.depart_date,
            })),
            reason: None,
        },
    )
    .await?;

    tx.commit().await?;

    let trips = trips_for_project(&state.pool, &project_id).await?;
    let dto = trips
        .into_iter()
        .find(|t| t.id == id)
        .ok_or_else(|| AppError::internal("created trip missing"))?;
    Ok((StatusCode::CREATED, Json(dto)))
}

async fn patch_trip(
    State(state): State<AppState>,
    actor: Actor,
    Path(trip_id): Path<String>,
    Json(req): Json<PatchTripRequest>,
) -> AppResult<Json<TripDto>> {
    let trip = load_trip(&state.pool, &trip_id).await?;
    let access = authz::project_access(&state.pool, &actor, &trip.project_id).await?;
    if !can_edit_trips(access, &actor) {
        return Err(AppError::forbidden(
            "trip editing requires team editor or coordinator",
        ));
    }
    if !matches!(trip.status.as_str(), "planned" | "confirmed") {
        return Err(AppError::conflict(
            "invalid_state",
            format!("cannot edit a trip that is {}", trip.status),
        ));
    }

    let arrive = req
        .arrive_date
        .clone()
        .unwrap_or_else(|| trip.arrive_date.clone());
    let depart = req
        .depart_date
        .clone()
        .unwrap_or_else(|| trip.depart_date.clone());
    let dates_change = arrive != trip.arrive_date || depart != trip.depart_date;

    let mut errors = FieldErrors::new();
    if let Some(title) = &req.title {
        errors.require("title", title, "title is required");
        errors.max_len("title", title, 300);
    }
    if dates_change {
        errors.valid_date("arrive_date", &arrive);
        errors.valid_date("depart_date", &depart);
        if let (Some(a), Some(d)) = (parse_date(&arrive), parse_date(&depart)) {
            errors.check(
                "depart_date",
                a < d,
                "depart_date must be after arrive_date",
            );
        }
    }
    errors.finish()?;

    let mut tx = db::begin_immediate(&state.pool).await?;

    if dates_change {
        let confirmed: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM bookings WHERE trip_id = ? AND status = 'confirmed'",
        )
        .bind(&trip_id)
        .fetch_one(&mut *tx)
        .await?;
        if confirmed > 0 {
            return Err(AppError::conflict(
                "use_change_request",
                "trip dates can only change via an approved reschedule_trip change request once bookings are confirmed",
            ));
        }

        // Requested bookings that would fall outside the new range are
        // released so "booking within trip dates" keeps holding.
        let released: Vec<String> = sqlx::query_scalar(
            "SELECT id FROM bookings WHERE trip_id = ? AND status = 'requested'
             AND (start_date < ? OR end_date > ?)",
        )
        .bind(&trip_id)
        .bind(&arrive)
        .bind(&depart)
        .fetch_all(&mut *tx)
        .await?;
        let now = now_rfc3339();
        for booking_id in &released {
            sqlx::query(
                "UPDATE bookings SET status = 'released', decided_by = ?, decided_at = ?
                 WHERE id = ?",
            )
            .bind(&actor.user_id)
            .bind(&now)
            .bind(booking_id)
            .execute(&mut *tx)
            .await?;
        }

        sqlx::query("UPDATE trips SET arrive_date = ?, depart_date = ? WHERE id = ?")
            .bind(&arrive)
            .bind(&depart)
            .bind(&trip_id)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(title) = &req.title {
        sqlx::query("UPDATE trips SET title = ? WHERE id = ?")
            .bind(title)
            .bind(&trip_id)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(participants) = &req.participants {
        let json = serde_json::to_string(participants).map_err(AppError::internal)?;
        sqlx::query("UPDATE trips SET participants_json = ? WHERE id = ?")
            .bind(&json)
            .bind(&trip_id)
            .execute(&mut *tx)
            .await?;
    }

    audit::record(
        &mut tx,
        AuditEvent {
            actor_id: Some(actor.user_id.clone()),
            actor_label: actor.name.clone(),
            action: "trip.updated".into(),
            entity_type: "trip".into(),
            entity_id: trip_id.clone(),
            project_id: Some(trip.project_id.clone()),
            visibility: "shared".into(),
            summary: format!("{} updated trip '{}'", actor.name, trip.title),
            before: Some(serde_json::json!({
                "title": trip.title,
                "arrive_date": trip.arrive_date,
                "depart_date": trip.depart_date,
            })),
            after: Some(serde_json::json!({
                "title": req.title.unwrap_or_else(|| trip.title.clone()),
                "arrive_date": arrive,
                "depart_date": depart,
            })),
            reason: None,
        },
    )
    .await?;

    tx.commit().await?;

    let trips = trips_for_project(&state.pool, &trip.project_id).await?;
    Ok(Json(
        trips
            .into_iter()
            .find(|t| t.id == trip_id)
            .ok_or_else(|| AppError::internal("trip missing"))?,
    ))
}

async fn cancel_trip(
    State(state): State<AppState>,
    actor: Actor,
    Path(trip_id): Path<String>,
) -> AppResult<Json<TripDto>> {
    let trip = load_trip(&state.pool, &trip_id).await?;
    let access = authz::project_access(&state.pool, &actor, &trip.project_id).await?;
    if !can_edit_trips(access, &actor) {
        return Err(AppError::forbidden(
            "trip cancellation requires team editor or coordinator",
        ));
    }
    if !matches!(trip.status.as_str(), "planned" | "confirmed") {
        return Err(AppError::conflict(
            "invalid_state",
            format!("cannot cancel a trip that is {}", trip.status),
        ));
    }

    let mut tx = db::begin_immediate(&state.pool).await?;
    let now = now_rfc3339();
    sqlx::query("UPDATE trips SET status = 'cancelled' WHERE id = ?")
        .bind(&trip_id)
        .execute(&mut *tx)
        .await?;

    let released: Vec<BookingRow> = sqlx::query_as(&format!(
        "{BOOKING_SELECT} WHERE b.trip_id = ? AND b.status IN ('requested','confirmed')"
    ))
    .bind(&trip_id)
    .fetch_all(&mut *tx)
    .await?;
    for row in &released {
        sqlx::query(
            "UPDATE bookings SET status = 'released', decided_by = ?, decided_at = ? WHERE id = ?",
        )
        .bind(&actor.user_id)
        .bind(&now)
        .bind(&row.id)
        .execute(&mut *tx)
        .await?;
        notify_decider(
            &mut tx,
            row.provider_user_id.as_deref(),
            "booking.released",
            "Booking released",
            &format!(
                "{} on {} → {} was released because the trip '{}' was cancelled",
                row.resource_name, row.start_date, row.end_date, trip.title
            ),
            &trip.project_id,
        )
        .await?;
    }
    notify_team(
        &mut tx,
        &trip.project_id,
        "trip.cancelled",
        "Trip cancelled",
        &format!(
            "The trip '{}' was cancelled; its bookings were released",
            trip.title
        ),
        &format!("/app/projects/{}/trips", trip.project_id),
    )
    .await?;

    audit::record(
        &mut tx,
        AuditEvent {
            actor_id: Some(actor.user_id.clone()),
            actor_label: actor.name.clone(),
            action: "trip.cancelled".into(),
            entity_type: "trip".into(),
            entity_id: trip_id.clone(),
            project_id: Some(trip.project_id.clone()),
            visibility: "shared".into(),
            summary: format!(
                "{} cancelled trip '{}' ({} bookings released)",
                actor.name,
                trip.title,
                released.len()
            ),
            before: Some(serde_json::json!({ "status": trip.status })),
            after: Some(serde_json::json!({
                "status": "cancelled",
                "released_bookings": released.len(),
            })),
            reason: None,
        },
    )
    .await?;

    tx.commit().await?;
    let trips = trips_for_project(&state.pool, &trip.project_id).await?;
    Ok(Json(
        trips
            .into_iter()
            .find(|t| t.id == trip_id)
            .ok_or_else(|| AppError::internal("trip missing"))?,
    ))
}

async fn complete_trip(
    State(state): State<AppState>,
    actor: Actor,
    Path(trip_id): Path<String>,
) -> AppResult<Json<TripDto>> {
    authz::require_role(&actor, &["coordinator", "base_manager"])?;
    let trip = load_trip(&state.pool, &trip_id).await?;
    if !matches!(trip.status.as_str(), "planned" | "confirmed") {
        return Err(AppError::conflict(
            "invalid_state",
            format!("cannot complete a trip that is {}", trip.status),
        ));
    }

    let mut tx = db::begin_immediate(&state.pool).await?;
    sqlx::query("UPDATE trips SET status = 'completed' WHERE id = ?")
        .bind(&trip_id)
        .execute(&mut *tx)
        .await?;
    audit::record(
        &mut tx,
        AuditEvent {
            actor_id: Some(actor.user_id.clone()),
            actor_label: actor.name.clone(),
            action: "trip.completed".into(),
            entity_type: "trip".into(),
            entity_id: trip_id.clone(),
            project_id: Some(trip.project_id.clone()),
            visibility: "shared".into(),
            summary: format!("{} marked trip '{}' completed", actor.name, trip.title),
            before: Some(serde_json::json!({ "status": trip.status })),
            after: Some(serde_json::json!({ "status": "completed" })),
            reason: None,
        },
    )
    .await?;
    tx.commit().await?;

    let trips = trips_for_project(&state.pool, &trip.project_id).await?;
    Ok(Json(
        trips
            .into_iter()
            .find(|t| t.id == trip_id)
            .ok_or_else(|| AppError::internal("trip missing"))?,
    ))
}

// ---------------------------------------------------------------------------
// Bookings
// ---------------------------------------------------------------------------

async fn create_booking(
    State(state): State<AppState>,
    actor: Actor,
    Path(trip_id): Path<String>,
    Json(req): Json<CreateBookingRequest>,
) -> AppResult<impl IntoResponse> {
    let trip = load_trip(&state.pool, &trip_id).await?;
    let access = authz::project_access(&state.pool, &actor, &trip.project_id).await?;
    if !can_edit_trips(access, &actor) {
        return Err(AppError::forbidden(
            "booking requests require team editor or coordinator",
        ));
    }
    if !matches!(trip.status.as_str(), "planned" | "confirmed") {
        return Err(AppError::conflict(
            "invalid_state",
            format!("cannot add bookings to a trip that is {}", trip.status),
        ));
    }
    let status = project_status(&state.pool, &trip.project_id).await?;
    if !PLANNABLE_STATUSES.contains(&status.as_str()) {
        return Err(AppError::conflict(
            "invalid_state",
            format!("bookings can be requested once the project is in review (currently {status})"),
        ));
    }

    let resource: Option<(String, String, i64, Option<String>, i64)> = sqlx::query_as(
        "SELECT name, kind, quantity, provider_user_id, active FROM resources WHERE id = ?",
    )
    .bind(&req.resource_id)
    .fetch_optional(&state.pool)
    .await?;
    let (resource_name, _kind, capacity, provider_user_id, active) =
        resource.ok_or(AppError::NotFound)?;

    let mut errors = FieldErrors::new();
    errors.valid_date("start_date", &req.start_date);
    errors.valid_date("end_date", &req.end_date);
    if let (Some(s), Some(e)) = (parse_date(&req.start_date), parse_date(&req.end_date)) {
        errors.check("end_date", s < e, "end_date must be after start_date");
        errors.check(
            "start_date",
            date_str(s) >= trip.arrive_date && date_str(e) <= trip.depart_date,
            "booking must be within the trip dates",
        );
    }
    errors.check("quantity", req.quantity >= 1, "must be at least 1");
    errors.check(
        "quantity",
        req.quantity <= capacity,
        "exceeds the resource's total quantity",
    );
    errors.check("resource_id", active != 0, "resource is not active");
    errors.finish()?;

    let id = new_id();
    let now = now_rfc3339();
    let mut tx = db::begin_immediate(&state.pool).await?;
    sqlx::query(
        "INSERT INTO bookings
         (id, trip_id, resource_id, start_date, end_date, quantity, status, requested_by, created_at)
         VALUES (?, ?, ?, ?, ?, ?, 'requested', ?, ?)",
    )
    .bind(&id)
    .bind(&trip_id)
    .bind(&req.resource_id)
    .bind(&req.start_date)
    .bind(&req.end_date)
    .bind(req.quantity)
    .bind(&actor.user_id)
    .bind(&now)
    .execute(&mut *tx)
    .await?;

    notify_decider(
        &mut tx,
        provider_user_id.as_deref(),
        "booking.requested",
        "New booking request",
        &format!(
            "{} requested {} × {} on {} → {}",
            actor.name, resource_name, req.quantity, req.start_date, req.end_date
        ),
        &trip.project_id,
    )
    .await?;

    audit::record(
        &mut tx,
        AuditEvent {
            actor_id: Some(actor.user_id.clone()),
            actor_label: actor.name.clone(),
            action: "booking.requested".into(),
            entity_type: "booking".into(),
            entity_id: id.clone(),
            project_id: Some(trip.project_id.clone()),
            visibility: "shared".into(),
            summary: format!(
                "{} requested {} × {} ({} → {})",
                actor.name, resource_name, req.quantity, req.start_date, req.end_date
            ),
            before: None,
            after: Some(serde_json::json!({
                "resource_id": req.resource_id, "resource": resource_name,
                "start_date": req.start_date, "end_date": req.end_date,
                "quantity": req.quantity,
            })),
            reason: None,
        },
    )
    .await?;

    tx.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(load_booking_dto(&state.pool, &id).await?),
    ))
}

/// Peak-capacity check (§4): inside the caller's `BEGIN IMMEDIATE`, for every
/// day of the requested half-open interval, the sum of `quantity` over
/// `confirmed` bookings covering that day plus this request must not exceed
/// the resource's `quantity`. Returns 409 `capacity_conflict` naming the
/// first conflicting day.
async fn check_capacity(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    resource_id: &str,
    resource_name: &str,
    exclude_booking_id: &str,
    start: chrono::NaiveDate,
    end: chrono::NaiveDate,
    add_quantity: i64,
) -> AppResult<()> {
    let capacity: i64 = sqlx::query_scalar("SELECT quantity FROM resources WHERE id = ?")
        .bind(resource_id)
        .fetch_one(&mut **tx)
        .await?;
    let overlapping: Vec<(String, String, i64)> = sqlx::query_as(
        "SELECT start_date, end_date, quantity FROM bookings
         WHERE resource_id = ? AND status = 'confirmed' AND id != ?
           AND start_date < ? AND end_date > ?",
    )
    .bind(resource_id)
    .bind(exclude_booking_id)
    .bind(date_str(end))
    .bind(date_str(start))
    .fetch_all(&mut **tx)
    .await?;

    let mut day = start;
    while day < end {
        let ds = date_str(day);
        let used: i64 = overlapping
            .iter()
            .filter(|(s, e, _)| s.as_str() <= ds.as_str() && ds.as_str() < e.as_str())
            .map(|r| r.2)
            .sum();
        if used + add_quantity > capacity {
            return Err(AppError::conflict(
                "capacity_conflict",
                format!("insufficient capacity for '{resource_name}' on {ds}"),
            ));
        }
        day += chrono::Duration::days(1);
    }
    Ok(())
}

/// Who may decide a booking: the owning provider for provider resources,
/// else a base_manager.
fn can_decide(actor: &Actor, provider_user_id: Option<&str>) -> bool {
    match provider_user_id {
        Some(provider) => actor.user_id == provider,
        None => actor.has_role("base_manager"),
    }
}

async fn confirm_booking(
    State(state): State<AppState>,
    actor: Actor,
    Path(booking_id): Path<String>,
) -> AppResult<Json<BookingDto>> {
    let booking = load_booking_row(&state.pool, &booking_id).await?;
    let trip = load_trip(&state.pool, &booking.trip_id).await?;
    if !can_decide(&actor, booking.provider_user_id.as_deref()) {
        return Err(AppError::forbidden(
            "only the resource's provider or a base manager may confirm bookings",
        ));
    }

    let mut tx = db::begin_immediate(&state.pool).await?;

    // Re-check inside the write tx: two concurrent confirmations of
    // overlapping bookings must serialize here (§4, §12).
    let status: String = sqlx::query_scalar("SELECT status FROM bookings WHERE id = ?")
        .bind(&booking_id)
        .fetch_one(&mut *tx)
        .await?;
    if status != "requested" {
        return Err(AppError::conflict(
            "invalid_state",
            format!("booking is {status}, only requested bookings can be confirmed"),
        ));
    }
    let project_status: String = sqlx::query_scalar(
        "SELECT p.status FROM projects p JOIN trips t ON t.project_id = p.id
         WHERE t.id = ?",
    )
    .bind(&booking.trip_id)
    .fetch_one(&mut *tx)
    .await?;
    if project_status != "approved" {
        return Err(AppError::conflict(
            "project_not_approved",
            "bookings can be confirmed once the project is approved",
        ));
    }

    let start = parse_date(&booking.start_date)
        .ok_or_else(|| AppError::internal("bad booking start_date"))?;
    let end =
        parse_date(&booking.end_date).ok_or_else(|| AppError::internal("bad booking end_date"))?;
    check_capacity(
        &mut tx,
        &booking.resource_id,
        &booking.resource_name,
        &booking_id,
        start,
        end,
        booking.quantity,
    )
    .await?;

    let now = now_rfc3339();
    sqlx::query(
        "UPDATE bookings SET status = 'confirmed', decided_by = ?, decided_at = ? WHERE id = ?",
    )
    .bind(&actor.user_id)
    .bind(&now)
    .bind(&booking_id)
    .execute(&mut *tx)
    .await?;

    // A first confirmed booking flips the trip planned → confirmed.
    sqlx::query("UPDATE trips SET status = 'confirmed' WHERE id = ? AND status = 'planned'")
        .bind(&booking.trip_id)
        .execute(&mut *tx)
        .await?;

    notify_team(
        &mut tx,
        &trip.project_id,
        "booking.confirmed",
        "Booking confirmed",
        &format!(
            "{} confirmed {} × {} on {} → {}",
            actor.name,
            booking.resource_name,
            booking.quantity,
            booking.start_date,
            booking.end_date
        ),
        &format!("/app/projects/{}/trips", trip.project_id),
    )
    .await?;

    audit::record(
        &mut tx,
        AuditEvent {
            actor_id: Some(actor.user_id.clone()),
            actor_label: actor.name.clone(),
            action: "booking.confirmed".into(),
            entity_type: "booking".into(),
            entity_id: booking_id.clone(),
            project_id: Some(trip.project_id.clone()),
            visibility: "shared".into(),
            summary: format!(
                "{} confirmed {} × {} ({} → {})",
                actor.name,
                booking.resource_name,
                booking.quantity,
                booking.start_date,
                booking.end_date
            ),
            before: Some(serde_json::json!({ "status": "requested" })),
            after: Some(serde_json::json!({ "status": "confirmed" })),
            reason: None,
        },
    )
    .await?;

    tx.commit().await?;
    Ok(Json(load_booking_dto(&state.pool, &booking_id).await?))
}

async fn decline_booking(
    State(state): State<AppState>,
    actor: Actor,
    Path(booking_id): Path<String>,
    Json(req): Json<DeclineBookingRequest>,
) -> AppResult<Json<BookingDto>> {
    let booking = load_booking_row(&state.pool, &booking_id).await?;
    let trip = load_trip(&state.pool, &booking.trip_id).await?;
    if !can_decide(&actor, booking.provider_user_id.as_deref()) {
        return Err(AppError::forbidden(
            "only the resource's provider or a base manager may decline bookings",
        ));
    }
    let mut errors = FieldErrors::new();
    errors.require("reason", &req.reason, "a decline reason is required");
    errors.finish()?;

    let mut tx = db::begin_immediate(&state.pool).await?;
    let status: String = sqlx::query_scalar("SELECT status FROM bookings WHERE id = ?")
        .bind(&booking_id)
        .fetch_one(&mut *tx)
        .await?;
    if status != "requested" {
        return Err(AppError::conflict(
            "invalid_state",
            format!("booking is {status}, only requested bookings can be declined"),
        ));
    }
    let now = now_rfc3339();
    sqlx::query(
        "UPDATE bookings SET status = 'declined', decided_by = ?, decided_at = ?, decline_reason = ?
         WHERE id = ?",
    )
    .bind(&actor.user_id)
    .bind(&now)
    .bind(&req.reason)
    .bind(&booking_id)
    .execute(&mut *tx)
    .await?;

    notify_team(
        &mut tx,
        &trip.project_id,
        "booking.declined",
        "Booking declined",
        &format!(
            "{} declined {} × {} on {} → {}: {}",
            actor.name,
            booking.resource_name,
            booking.quantity,
            booking.start_date,
            booking.end_date,
            req.reason
        ),
        &format!("/app/projects/{}/trips", trip.project_id),
    )
    .await?;

    audit::record(
        &mut tx,
        AuditEvent {
            actor_id: Some(actor.user_id.clone()),
            actor_label: actor.name.clone(),
            action: "booking.declined".into(),
            entity_type: "booking".into(),
            entity_id: booking_id.clone(),
            project_id: Some(trip.project_id.clone()),
            visibility: "shared".into(),
            summary: format!(
                "{} declined {} × {} ({} → {})",
                actor.name,
                booking.resource_name,
                booking.quantity,
                booking.start_date,
                booking.end_date
            ),
            before: Some(serde_json::json!({ "status": "requested" })),
            after: Some(serde_json::json!({
                "status": "declined", "reason": req.reason,
            })),
            reason: Some(req.reason.clone()),
        },
    )
    .await?;

    tx.commit().await?;
    Ok(Json(load_booking_dto(&state.pool, &booking_id).await?))
}

async fn cancel_booking(
    State(state): State<AppState>,
    actor: Actor,
    Path(booking_id): Path<String>,
) -> AppResult<Json<BookingDto>> {
    let booking = load_booking_row(&state.pool, &booking_id).await?;
    let trip = load_trip(&state.pool, &booking.trip_id).await?;
    let access = authz::project_access(&state.pool, &actor, &trip.project_id).await?;
    if !can_edit_trips(access, &actor) {
        return Err(AppError::forbidden(
            "cancelling a booking requires team editor or coordinator",
        ));
    }
    if !matches!(booking.status.as_str(), "requested" | "confirmed") {
        return Err(AppError::conflict(
            "invalid_state",
            format!(
                "booking is {}, only requested or confirmed bookings can be cancelled",
                booking.status
            ),
        ));
    }

    let mut tx = db::begin_immediate(&state.pool).await?;
    sqlx::query("UPDATE bookings SET status = 'cancelled' WHERE id = ?")
        .bind(&booking_id)
        .execute(&mut *tx)
        .await?;

    notify_decider(
        &mut tx,
        booking.provider_user_id.as_deref(),
        "booking.cancelled",
        "Booking cancelled",
        &format!(
            "{} cancelled {} × {} on {} → {}",
            actor.name,
            booking.resource_name,
            booking.quantity,
            booking.start_date,
            booking.end_date
        ),
        &trip.project_id,
    )
    .await?;

    audit::record(
        &mut tx,
        AuditEvent {
            actor_id: Some(actor.user_id.clone()),
            actor_label: actor.name.clone(),
            action: "booking.cancelled".into(),
            entity_type: "booking".into(),
            entity_id: booking_id.clone(),
            project_id: Some(trip.project_id.clone()),
            visibility: "shared".into(),
            summary: format!(
                "{} cancelled booking {} × {} ({} → {})",
                actor.name,
                booking.resource_name,
                booking.quantity,
                booking.start_date,
                booking.end_date
            ),
            before: Some(serde_json::json!({ "status": booking.status })),
            after: Some(serde_json::json!({ "status": "cancelled" })),
            reason: None,
        },
    )
    .await?;

    tx.commit().await?;
    Ok(Json(load_booking_dto(&state.pool, &booking_id).await?))
}

// ---------------------------------------------------------------------------
// Calendar & provider view
// ---------------------------------------------------------------------------

const MAX_CALENDAR_DAYS: i64 = 400;

async fn calendar(
    State(state): State<AppState>,
    actor: Actor,
    Query(query): Query<CalendarQuery>,
) -> AppResult<Json<CalendarResponse>> {
    authz::require_role(&actor, &["base_manager", "coordinator"])?;

    let mut errors = FieldErrors::new();
    let from = query.from.clone().unwrap_or_default();
    let to = query.to.clone().unwrap_or_default();
    errors.valid_date("from", &from);
    errors.valid_date("to", &to);
    errors.finish()?;
    let (from, to) = (
        parse_date(&from).ok_or(AppError::BadRequest("bad from date".into()))?,
        parse_date(&to).ok_or(AppError::BadRequest("bad to date".into()))?,
    );
    let span = (to - from).num_days() + 1;
    if span < 1 {
        return Err(AppError::unprocessable(
            "bad_range",
            "from must be on or before to",
        ));
    }
    if span > MAX_CALENDAR_DAYS {
        return Err(AppError::unprocessable(
            "range_too_large",
            format!("calendar range is limited to {MAX_CALENDAR_DAYS} days"),
        ));
    }

    #[derive(FromRow)]
    struct CalResource {
        id: String,
        name: String,
        kind: String,
        quantity: i64,
        unit_label: String,
        provider_user_id: Option<String>,
    }
    let resources: Vec<CalResource> = if let Some(resource_id) = &query.resource_id {
        sqlx::query_as(
            "SELECT id, name, kind, quantity, unit_label, provider_user_id
             FROM resources WHERE id = ?",
        )
        .bind(resource_id)
        .fetch_all(&state.pool)
        .await?
    } else {
        sqlx::query_as(
            "SELECT id, name, kind, quantity, unit_label, provider_user_id
             FROM resources ORDER BY kind, name",
        )
        .fetch_all(&state.pool)
        .await?
    };

    // Live bookings overlapping [from, to] inclusive (interval [from, to+1)).
    #[derive(FromRow)]
    struct CalBooking {
        id: String,
        trip_id: String,
        resource_id: String,
        start_date: String,
        end_date: String,
        quantity: i64,
        status: String,
        project_id: String,
        project_reference: Option<String>,
        project_title: String,
    }
    let to_exclusive = to + chrono::Duration::days(1);
    let bookings: Vec<CalBooking> = if let Some(resource_id) = &query.resource_id {
        sqlx::query_as(
            "SELECT b.id, b.trip_id, b.resource_id, b.start_date, b.end_date, b.quantity,
                    b.status, t.project_id, p.reference AS project_reference, p.title AS project_title
             FROM bookings b
             JOIN trips t ON t.id = b.trip_id
             JOIN projects p ON p.id = t.project_id
             WHERE b.resource_id = ? AND b.status IN ('requested','confirmed')
               AND b.start_date < ? AND b.end_date > ?",
        )
        .bind(resource_id)
        .bind(date_str(to_exclusive))
        .bind(date_str(from))
        .fetch_all(&state.pool)
        .await?
    } else {
        sqlx::query_as(
            "SELECT b.id, b.trip_id, b.resource_id, b.start_date, b.end_date, b.quantity,
                    b.status, t.project_id, p.reference AS project_reference, p.title AS project_title
             FROM bookings b
             JOIN trips t ON t.id = b.trip_id
             JOIN projects p ON p.id = t.project_id
             WHERE b.status IN ('requested','confirmed')
               AND b.start_date < ? AND b.end_date > ?",
        )
        .bind(date_str(to_exclusive))
        .bind(date_str(from))
        .fetch_all(&state.pool)
        .await?
    };

    let mut out = Vec::new();
    for resource in resources {
        let mut days = Vec::new();
        let mut day = from;
        while day <= to {
            let ds = date_str(day);
            let covering: Vec<&CalBooking> = bookings
                .iter()
                .filter(|b| {
                    b.resource_id == resource.id
                        && b.start_date.as_str() <= ds.as_str()
                        && ds.as_str() < b.end_date.as_str()
                })
                .collect();
            let used: i64 = covering
                .iter()
                .filter(|b| b.status == "confirmed")
                .map(|b| b.quantity)
                .sum();
            days.push(CalendarDayDto {
                date: ds,
                used,
                bookings: covering
                    .into_iter()
                    .map(|b| CalendarBookingDto {
                        booking_id: b.id.clone(),
                        trip_id: b.trip_id.clone(),
                        status: b.status.clone(),
                        quantity: b.quantity,
                        start_date: b.start_date.clone(),
                        end_date: b.end_date.clone(),
                        project_id: b.project_id.clone(),
                        project_reference: b.project_reference.clone(),
                        project_title: b.project_title.clone(),
                    })
                    .collect(),
            });
            day += chrono::Duration::days(1);
        }
        out.push(CalendarResourceDto {
            resource_id: resource.id,
            name: resource.name,
            kind: resource.kind,
            capacity: resource.quantity,
            unit_label: resource.unit_label,
            provider_user_id: resource.provider_user_id,
            days,
        });
    }

    Ok(Json(CalendarResponse {
        from: date_str(from),
        to: date_str(to),
        resources: out,
    }))
}

/// Provider view (§3): only bookings on resources they own, with minimal
/// project info — title, reference, trip dates, team size, lead name.
async fn provider_bookings(
    State(state): State<AppState>,
    actor: Actor,
) -> AppResult<Json<ListResponse<ProviderBookingDto>>> {
    authz::require_role(&actor, &["provider"])?;

    #[derive(FromRow)]
    struct Row {
        booking_id: String,
        status: String,
        start_date: String,
        end_date: String,
        quantity: i64,
        resource_id: String,
        resource_name: String,
        trip_id: String,
        trip_title: String,
        trip_arrive_date: String,
        trip_depart_date: String,
        project_id: String,
        project_title: String,
        project_reference: Option<String>,
        team_size: i64,
        lead_name: Option<String>,
        requested_at: String,
        decline_reason: Option<String>,
    }
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT b.id AS booking_id, b.status, b.start_date, b.end_date, b.quantity,
                r.id AS resource_id, r.name AS resource_name,
                t.id AS trip_id, t.title AS trip_title,
                t.arrive_date AS trip_arrive_date, t.depart_date AS trip_depart_date,
                p.id AS project_id, p.title AS project_title, p.reference AS project_reference,
                (SELECT COUNT(*) FROM project_members pm
                  WHERE pm.project_id = p.id AND pm.removed_at IS NULL) AS team_size,
                (SELECT u.name FROM project_members pm JOIN users u ON u.id = pm.user_id
                  WHERE pm.project_id = p.id AND pm.role = 'lead' AND pm.removed_at IS NULL
                  LIMIT 1) AS lead_name,
                b.created_at AS requested_at, b.decline_reason
         FROM bookings b
         JOIN resources r ON r.id = b.resource_id
         JOIN trips t ON t.id = b.trip_id
         JOIN projects p ON p.id = t.project_id
         WHERE r.provider_user_id = ?
         ORDER BY (b.status = 'requested') DESC, b.start_date",
    )
    .bind(&actor.user_id)
    .fetch_all(&state.pool)
    .await?;

    let total = rows.len() as i64;
    let items = rows
        .into_iter()
        .map(|r| ProviderBookingDto {
            booking_id: r.booking_id,
            status: r.status,
            start_date: r.start_date,
            end_date: r.end_date,
            quantity: r.quantity,
            resource_id: r.resource_id,
            resource_name: r.resource_name,
            trip_id: r.trip_id,
            trip_title: r.trip_title,
            trip_arrive_date: r.trip_arrive_date,
            trip_depart_date: r.trip_depart_date,
            project_id: r.project_id,
            project_title: r.project_title,
            project_reference: r.project_reference,
            team_size: r.team_size,
            lead_name: r.lead_name.unwrap_or_default(),
            requested_at: r.requested_at,
            decline_reason: r.decline_reason,
        })
        .collect();
    Ok(Json(ListResponse { items, total }))
}

// ---------------------------------------------------------------------------
// Workspace section for GET /projects/{id} (one call site in routes/projects)
// ---------------------------------------------------------------------------

/// Trips (with bookings) + invoices for the project workspace. Empty for
/// viewers without at least team access (experts, public) — invoices honour
/// the list rule: finance sees all, everyone else issued + cancelled.
pub async fn workspace_section(
    pool: &sqlx::SqlitePool,
    actor: &Actor,
    project_id: &str,
    access: ProjectAccess,
) -> AppResult<(Vec<TripDto>, Vec<crate::dto::InvoiceDto>)> {
    if access < ProjectAccess::TeamViewer {
        return Ok((Vec::new(), Vec::new()));
    }
    let trips = trips_for_project(pool, project_id).await?;
    let invoices = crate::routes::money::invoices_for_project(pool, actor, project_id).await?;
    Ok((trips, invoices))
}
