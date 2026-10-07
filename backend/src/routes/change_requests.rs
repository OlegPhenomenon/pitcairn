//! Change requests (§4 `change_requests`): the team asks for a reschedule,
//! permit extension or scope expansion; `impact` lists what is affected
//! (bookings, deliverables, decisions) by reading those tables directly;
//! approving a `reschedule_trip` moves the trip, releases its bookings and
//! re-requests them at the new dates — and touches nothing else.

use std::collections::HashMap;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::NaiveDate;
use serde_json::{Value, json};
use sqlx::FromRow;

use crate::AppState;
use crate::audit::{self, AuditEvent};
use crate::authz::{self, Actor, ProjectAccess};
use crate::db;
use crate::dto::ListResponse;
use crate::dto::a::{
    ChangeRequestDto, ChangeRequestImpactDto, CreateChangeRequestRequest, ImpactBookingDto,
    ImpactDecisionDto, ImpactDeliverableDto, RejectChangeRequestRequest,
    ResolveChangeRequestRequest,
};
use crate::error::{AppError, AppResult};
use crate::notify;
use crate::routes::decisions::PERMIT_CHAIN;
use crate::routes::projects::{require_project_access, require_team_editor};
use crate::util::{new_id, now_rfc3339};
use crate::validation::FieldErrors;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/projects/{id}/change-requests",
            get(list_change_requests).post(create_change_request),
        )
        .route("/change-requests/{id}", get(get_change_request))
        .route("/change-requests/{id}/impact", get(change_request_impact))
        .route(
            "/change-requests/{id}/approve",
            post(approve_change_request),
        )
        .route("/change-requests/{id}/reject", post(reject_change_request))
        .route(
            "/change-requests/{id}/withdraw",
            post(withdraw_change_request),
        )
}

const KINDS: &[&str] = &["reschedule_trip", "extend_permit", "expand_scope", "other"];

#[derive(FromRow)]
struct CrRow {
    id: String,
    project_id: String,
    kind: String,
    description: String,
    payload_json: String,
    status: String,
    requested_by: String,
    requested_by_name: String,
    resolved_by: Option<String>,
    resolved_by_name: Option<String>,
    resolution_note: Option<String>,
    resulting_decision_id: Option<String>,
    created_at: String,
}

const CR_SELECT: &str = "SELECT c.id, c.project_id, c.kind, c.description, c.payload_json, c.status,
            c.requested_by, ru.name AS requested_by_name, c.resolved_by, su.name AS resolved_by_name,
            c.resolution_note, c.resulting_decision_id, c.created_at
     FROM change_requests c
     JOIN users ru ON ru.id = c.requested_by
     LEFT JOIN users su ON su.id = c.resolved_by";

impl CrRow {
    fn payload(&self) -> AppResult<Value> {
        serde_json::from_str(&self.payload_json).map_err(AppError::internal)
    }

    fn into_dto(self) -> AppResult<ChangeRequestDto> {
        let payload = self.payload()?;
        Ok(ChangeRequestDto {
            id: self.id,
            project_id: self.project_id,
            kind: self.kind,
            description: self.description,
            payload,
            status: self.status,
            requested_by: self.requested_by,
            requested_by_name: self.requested_by_name,
            resolved_by: self.resolved_by,
            resolved_by_name: self.resolved_by_name,
            resolution_note: self.resolution_note,
            resulting_decision_id: self.resulting_decision_id,
            created_at: self.created_at,
        })
    }
}

async fn load_cr<'e, E>(exec: E, id: &str) -> AppResult<CrRow>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    let row: Option<CrRow> = sqlx::query_as(&format!("{CR_SELECT} WHERE c.id = ?"))
        .bind(id)
        .fetch_optional(exec)
        .await?;
    row.ok_or(AppError::NotFound)
}

fn date(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()
}

fn fmt(d: NaiveDate) -> String {
    d.format("%Y-%m-%d").to_string()
}

/// Reschedule payload `(trip_id, new_arrive, new_depart)`.
fn reschedule_payload(payload: &Value) -> Option<(String, NaiveDate, NaiveDate)> {
    Some((
        payload.get("trip_id")?.as_str()?.to_string(),
        date(payload.get("new_arrive_date")?.as_str()?)?,
        date(payload.get("new_depart_date")?.as_str()?)?,
    ))
}

fn cr_event(
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
        entity_type: "change_request".into(),
        entity_id: id.into(),
        project_id: Some(project_id.into()),
        visibility: "shared".into(),
        summary,
        before: None,
        after: None,
        reason: None,
    }
}

// ---------------------------------------------------------------------------
// Create / read
// ---------------------------------------------------------------------------

async fn create_change_request(
    State(state): State<AppState>,
    actor: Actor,
    Path(project_id): Path<String>,
    Json(req): Json<CreateChangeRequestRequest>,
) -> AppResult<impl IntoResponse> {
    let access = require_project_access(&state, &actor, &project_id).await?;
    require_team_editor(access)?;

    let mut errors = FieldErrors::new();
    errors.check(
        "kind",
        KINDS.contains(&req.kind.as_str()),
        &format!("must be one of {}", KINDS.join(", ")),
    );
    errors.require(
        "description",
        &req.description,
        "please describe the change",
    );
    errors.max_len("description", &req.description, 10_000);
    errors.check("payload", req.payload.is_object(), "must be an object");
    match req.kind.as_str() {
        "reschedule_trip" => match reschedule_payload(&req.payload) {
            Some((_, a, d)) => errors.check(
                "payload.new_depart_date",
                a < d,
                "must be after the new arrival date",
            ),
            None => errors.check(
                "payload",
                false,
                "requires trip_id, new_arrive_date and new_depart_date (YYYY-MM-DD)",
            ),
        },
        "extend_permit" => {
            errors.check(
                "payload.decision_id",
                req.payload
                    .get("decision_id")
                    .and_then(Value::as_str)
                    .is_some_and(|s| !s.trim().is_empty()),
                "choose the permit to extend",
            );
            errors.check(
                "payload.new_valid_to",
                req.payload
                    .get("new_valid_to")
                    .and_then(Value::as_str)
                    .and_then(date)
                    .is_some(),
                "must be a date YYYY-MM-DD",
            );
        }
        _ => {}
    }
    errors.finish()?;

    let mut tx = db::begin_immediate(&state.pool).await?;
    let (status, title): (String, String) =
        sqlx::query_as("SELECT status, title FROM projects WHERE id = ?")
            .bind(&project_id)
            .fetch_one(&mut *tx)
            .await?;
    if matches!(
        status.as_str(),
        "draft" | "withdrawn" | "refused" | "closed"
    ) {
        return Err(AppError::conflict(
            "invalid_state",
            format!("no changes can be requested while the project is {status}"),
        ));
    }
    if req.kind == "reschedule_trip"
        && let Some((trip_id, _, _)) = reschedule_payload(&req.payload)
    {
        let trip: Option<(String, String)> =
            sqlx::query_as("SELECT project_id, status FROM trips WHERE id = ?")
                .bind(&trip_id)
                .fetch_optional(&mut *tx)
                .await?;
        match trip {
            Some((pid, _)) if pid != project_id => {
                return Err(AppError::forbidden("the trip belongs to another project"));
            }
            None => return Err(AppError::NotFound),
            Some((_, s)) if matches!(s.as_str(), "cancelled" | "completed") => {
                return Err(AppError::conflict(
                    "invalid_state",
                    format!("the trip is {s}"),
                ));
            }
            Some(_) => {}
        }
    }
    if req.kind == "extend_permit"
        && let Some(decision_id) = req.payload.get("decision_id").and_then(Value::as_str)
    {
        let in_force: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM decisions
             WHERE id = ? AND project_id = ? AND status = 'issued' AND superseded_by_id IS NULL
               AND chain_id IS NOT NULL AND kind <> 'revocation'",
        )
        .bind(decision_id)
        .bind(&project_id)
        .fetch_one(&mut *tx)
        .await?;
        if in_force == 0 {
            let mut fields = HashMap::new();
            fields.insert(
                "payload.decision_id".to_string(),
                "must be a permit of this project that is currently in force".to_string(),
            );
            return Err(AppError::Validation { fields });
        }
    }
    let id = new_id();
    sqlx::query(
        "INSERT INTO change_requests (id, project_id, kind, description, payload_json, status, requested_by, created_at)
         VALUES (?, ?, ?, ?, ?, 'open', ?, ?)",
    )
    .bind(&id)
    .bind(&project_id)
    .bind(&req.kind)
    .bind(req.description.trim())
    .bind(req.payload.to_string())
    .bind(&actor.user_id)
    .bind(now_rfc3339())
    .execute(&mut *tx)
    .await?;
    let mut event = cr_event(
        &actor,
        "change_request.created",
        &id,
        &project_id,
        format!(
            "{} requested a change ({})",
            actor.name,
            req.kind.replace('_', " ")
        ),
    );
    event.after = Some(json!({"kind": req.kind, "payload": req.payload}));
    audit::record(&mut tx, event).await?;
    notify::notify_coordinators(
        &mut tx,
        &project_id,
        "change_requested",
        &format!("Change requested: {title}"),
        &format!("{} requested: {}", actor.name, req.description.trim()),
    )
    .await?;
    tx.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(load_cr(&state.pool, &id).await?.into_dto()?),
    ))
}

async fn list_change_requests(
    State(state): State<AppState>,
    actor: Actor,
    Path(project_id): Path<String>,
) -> AppResult<Json<ListResponse<ChangeRequestDto>>> {
    require_project_access(&state, &actor, &project_id).await?;
    let rows: Vec<CrRow> = sqlx::query_as(&format!(
        "{CR_SELECT} WHERE c.project_id = ? ORDER BY c.created_at DESC"
    ))
    .bind(&project_id)
    .fetch_all(&state.pool)
    .await?;
    let items = rows
        .into_iter()
        .map(CrRow::into_dto)
        .collect::<AppResult<Vec<_>>>()?;
    let total = items.len() as i64;
    Ok(Json(ListResponse { items, total }))
}

async fn readable_cr(
    state: &AppState,
    actor: &Actor,
    id: &str,
) -> AppResult<(CrRow, ProjectAccess)> {
    let row = load_cr(&state.pool, id).await?;
    let access = require_project_access(state, actor, &row.project_id).await?;
    Ok((row, access))
}

async fn get_change_request(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<String>,
) -> AppResult<Json<ChangeRequestDto>> {
    let (row, _) = readable_cr(&state, &actor, &id).await?;
    Ok(Json(row.into_dto()?))
}

// ---------------------------------------------------------------------------
// Impact
// ---------------------------------------------------------------------------

#[derive(FromRow)]
struct BookingRow {
    id: String,
    trip_id: String,
    resource_id: String,
    resource_name: String,
    resource_quantity: i64,
    status: String,
    start_date: String,
    end_date: String,
    quantity: i64,
}

const BOOKING_SELECT: &str = "SELECT b.id, b.trip_id, b.resource_id, r.name AS resource_name,
            r.quantity AS resource_quantity, b.status, b.start_date, b.end_date, b.quantity
     FROM bookings b JOIN resources r ON r.id = b.resource_id";

/// Where a booking of the rescheduled trip lands: whole-trip bookings take
/// the whole new trip; partial ones shift by the arrival delta, clamped to
/// the new trip (half-open intervals).
fn shifted_interval(
    (start, end): (NaiveDate, NaiveDate),
    (old_arrive, old_depart): (NaiveDate, NaiveDate),
    (new_arrive, new_depart): (NaiveDate, NaiveDate),
) -> (NaiveDate, NaiveDate) {
    if start <= old_arrive && end >= old_depart {
        return (new_arrive, new_depart);
    }
    let delta = new_arrive - old_arrive;
    let s = (start + delta).max(new_arrive);
    let e = (end + delta).min(new_depart);
    if s < e {
        (s, e)
    } else {
        (new_arrive, new_depart)
    }
}

/// Bookings of the trip itself, plus confirmed bookings of other trips that
/// would push a resource over capacity at the new dates.
async fn booking_impact(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    trip_id: &str,
    new_arrive: NaiveDate,
    new_depart: NaiveDate,
) -> AppResult<(Vec<ImpactBookingDto>, Vec<String>)> {
    let (old_arrive, old_depart): (String, String) =
        sqlx::query_as("SELECT arrive_date, depart_date FROM trips WHERE id = ?")
            .bind(trip_id)
            .fetch_one(&mut **tx)
            .await?;
    let (Some(old_arrive), Some(old_depart)) = (date(&old_arrive), date(&old_depart)) else {
        return Err(AppError::internal("trip has invalid dates"));
    };
    let own: Vec<BookingRow> = sqlx::query_as(&format!(
        "{BOOKING_SELECT} WHERE b.trip_id = ? AND b.status IN ('requested', 'confirmed')
         ORDER BY b.start_date, b.id"
    ))
    .bind(trip_id)
    .fetch_all(&mut **tx)
    .await?;
    let to_dto = |b: &BookingRow, kind: &str| ImpactBookingDto {
        booking_id: b.id.clone(),
        trip_id: b.trip_id.clone(),
        resource_id: b.resource_id.clone(),
        resource_name: b.resource_name.clone(),
        kind: kind.into(),
        status: b.status.clone(),
        start_date: b.start_date.clone(),
        end_date: b.end_date.clone(),
        quantity: b.quantity,
    };
    let mut out: Vec<ImpactBookingDto> = own.iter().map(|b| to_dto(b, "trip")).collect();
    let mut notes = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for b in &own {
        let (Some(s), Some(e)) = (date(&b.start_date), date(&b.end_date)) else {
            continue;
        };
        let (ns, ne) = shifted_interval((s, e), (old_arrive, old_depart), (new_arrive, new_depart));
        let others: Vec<BookingRow> = sqlx::query_as(&format!(
            "{BOOKING_SELECT} WHERE b.resource_id = ? AND b.trip_id != ? AND b.status = 'confirmed'
               AND b.start_date < ? AND b.end_date > ? ORDER BY b.start_date, b.id"
        ))
        .bind(&b.resource_id)
        .bind(trip_id)
        .bind(fmt(ne))
        .bind(fmt(ns))
        .fetch_all(&mut **tx)
        .await?;
        let mut first_conflict = None;
        let mut day = ns;
        while day < ne {
            let used: i64 = others
                .iter()
                .filter(|o| {
                    date(&o.start_date).is_some_and(|os| os <= day)
                        && date(&o.end_date).is_some_and(|oe| oe > day)
                })
                .map(|o| o.quantity)
                .sum();
            if used + b.quantity > b.resource_quantity {
                first_conflict = Some(day);
                break;
            }
            day = day.succ_opt().unwrap_or(ne);
        }
        if let Some(day) = first_conflict {
            notes.push(format!(
                "{} is fully booked on {} — the re-requested booking will conflict",
                b.resource_name,
                fmt(day)
            ));
            for o in &others {
                if seen.insert(o.id.clone()) {
                    out.push(to_dto(o, "conflict"));
                }
            }
        }
    }
    Ok((out, notes))
}

async fn compute_impact(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    cr: &CrRow,
) -> AppResult<ChangeRequestImpactDto> {
    let payload = cr.payload()?;
    let mut impact = ChangeRequestImpactDto {
        bookings: Vec::new(),
        deliverables: Vec::new(),
        decisions: Vec::new(),
        requires_new_decision: matches!(cr.kind.as_str(), "expand_scope" | "extend_permit"),
        notes: Vec::new(),
    };
    // Window that the permit in force must cover after the change.
    let window: Option<(Option<NaiveDate>, NaiveDate)> = match cr.kind.as_str() {
        "reschedule_trip" => {
            let (trip_id, arrive, depart) = reschedule_payload(&payload)
                .ok_or_else(|| AppError::internal("invalid reschedule payload"))?;
            let (bookings, notes) = booking_impact(tx, &trip_id, arrive, depart).await?;
            impact.bookings = bookings;
            impact.notes.extend(notes);
            let deliverables: Vec<(String, String, String, String)> = sqlx::query_as(
                "SELECT id, title, due_date, status FROM deliverables
                 WHERE project_id = ? AND due_date < ?
                   AND status NOT IN ('accepted', 'waived', 'cancelled')
                 ORDER BY due_date, id",
            )
            .bind(&cr.project_id)
            .bind(fmt(depart))
            .fetch_all(&mut **tx)
            .await?;
            impact.deliverables = deliverables
                .into_iter()
                .map(|(id, title, due_date, status)| ImpactDeliverableDto {
                    id,
                    title,
                    due_date,
                    status,
                })
                .collect();
            if !impact.deliverables.is_empty() {
                impact.notes.push(format!(
                    "{} deliverable(s) are due before the new trip end; agree new due dates",
                    impact.deliverables.len()
                ));
            }
            Some((Some(arrive), depart))
        }
        "extend_permit" => payload
            .get("new_valid_to")
            .and_then(Value::as_str)
            .and_then(date)
            .map(|d| (None, d)),
        _ => None,
    };
    if let Some((from, to)) = window {
        #[derive(FromRow)]
        struct DecRow {
            id: String,
            kind: String,
            title: String,
            status: String,
            valid_from: Option<String>,
            valid_to: Option<String>,
        }
        // Every permit in force is checked against a rescheduled trip; an
        // extension request concerns only the one permit it names.
        let only: Option<&str> = match cr.kind.as_str() {
            "extend_permit" => payload.get("decision_id").and_then(Value::as_str),
            _ => None,
        };
        let decisions: Vec<DecRow> = sqlx::query_as(&format!(
            "SELECT id, kind, title, status, valid_from, valid_to FROM decisions
             WHERE project_id = ? AND status = 'issued' AND superseded_by_id IS NULL
               AND chain_id IS NOT NULL AND kind IN ({}) AND (? IS NULL OR id = ?)
             ORDER BY issued_at",
            PERMIT_CHAIN
                .iter()
                .map(|k| format!("'{k}'"))
                .collect::<Vec<_>>()
                .join(",")
        ))
        .bind(&cr.project_id)
        .bind(only)
        .bind(only)
        .fetch_all(&mut **tx)
        .await?;
        for d in decisions {
            let starts_late = match (from, d.valid_from.as_deref().and_then(date)) {
                (Some(f), Some(vf)) => vf > f,
                _ => false,
            };
            let ends_early = d
                .valid_to
                .as_deref()
                .and_then(date)
                .is_some_and(|vt| vt < to);
            if starts_late || ends_early || d.kind == "revocation" {
                impact.decisions.push(ImpactDecisionDto {
                    id: d.id,
                    kind: d.kind,
                    title: d.title,
                    status: d.status,
                    valid_from: d.valid_from,
                    valid_to: d.valid_to,
                });
            }
        }
        if !impact.decisions.is_empty() {
            impact.requires_new_decision = true;
            let names = impact
                .decisions
                .iter()
                .map(|d| {
                    if d.title.is_empty() {
                        format!("{} {}", d.kind, &d.id[..8.min(d.id.len())])
                    } else {
                        d.title.clone()
                    }
                })
                .collect::<Vec<_>>()
                .join(", ");
            impact.notes.push(format!(
                "{} permit(s) do not cover the new dates ({names}); each needs its own amendment or extension. Other permits are unchanged",
                impact.decisions.len()
            ));
        }
    }
    if cr.kind == "expand_scope" {
        impact
            .notes
            .push("Expanding the scope requires a new decision".into());
    }
    Ok(impact)
}

async fn change_request_impact(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<String>,
) -> AppResult<Json<ChangeRequestImpactDto>> {
    let (row, _) = readable_cr(&state, &actor, &id).await?;
    // Read-only, but consistent across the several tables it reads.
    let mut tx = state.pool.begin().await?;
    let impact = compute_impact(&mut tx, &row).await?;
    tx.commit().await?;
    Ok(Json(impact))
}

// ---------------------------------------------------------------------------
// Resolve
// ---------------------------------------------------------------------------

fn require_resolver(actor: &Actor) -> AppResult<()> {
    authz::require_role(actor, &["coordinator", "decision_maker"])
}

async fn open_cr(tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>, id: &str) -> AppResult<CrRow> {
    let row = load_cr(&mut **tx, id).await?;
    if row.status != "open" {
        return Err(AppError::conflict(
            "invalid_state",
            format!("this change request is already {}", row.status),
        ));
    }
    Ok(row)
}

/// Approving a reschedule: move the trip, release its active bookings and
/// re-request the same resources at the new dates. Decisions and
/// deliverables are NOT touched (they appear in the impact instead).
async fn apply_reschedule(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    actor: &Actor,
    cr: &CrRow,
) -> AppResult<Vec<String>> {
    let payload = cr.payload()?;
    let (trip_id, new_arrive, new_depart) = reschedule_payload(&payload)
        .ok_or_else(|| AppError::internal("invalid reschedule payload"))?;
    let trip: Option<(String, String, String, String)> = sqlx::query_as(
        "SELECT project_id, status, arrive_date, depart_date FROM trips WHERE id = ?",
    )
    .bind(&trip_id)
    .fetch_optional(&mut **tx)
    .await?;
    let (project_id, status, old_arrive, old_depart) = trip.ok_or(AppError::NotFound)?;
    if project_id != cr.project_id {
        return Err(AppError::forbidden("the trip belongs to another project"));
    }
    if matches!(status.as_str(), "cancelled" | "completed") {
        return Err(AppError::conflict(
            "invalid_state",
            format!("the trip is {status}"),
        ));
    }
    let (Some(old_arrive), Some(old_depart)) = (date(&old_arrive), date(&old_depart)) else {
        return Err(AppError::internal("trip has invalid dates"));
    };
    let now = now_rfc3339();
    sqlx::query(
        "UPDATE trips SET arrive_date = ?, depart_date = ?,
                status = CASE WHEN status = 'confirmed' THEN 'planned' ELSE status END
         WHERE id = ?",
    )
    .bind(fmt(new_arrive))
    .bind(fmt(new_depart))
    .bind(&trip_id)
    .execute(&mut **tx)
    .await?;
    let bookings: Vec<(String, String, String, String, i64, String)> = sqlx::query_as(
        "SELECT id, resource_id, start_date, end_date, quantity, requested_by FROM bookings
         WHERE trip_id = ? AND status IN ('requested', 'confirmed') ORDER BY start_date, id",
    )
    .bind(&trip_id)
    .fetch_all(&mut **tx)
    .await?;
    let mut resource_ids = Vec::new();
    for (booking_id, resource_id, start, end, quantity, requested_by) in bookings {
        let (Some(s), Some(e)) = (date(&start), date(&end)) else {
            continue;
        };
        let (ns, ne) = shifted_interval((s, e), (old_arrive, old_depart), (new_arrive, new_depart));
        sqlx::query(
            "UPDATE bookings SET status = 'released', decided_by = ?, decided_at = ? WHERE id = ?",
        )
        .bind(&actor.user_id)
        .bind(&now)
        .bind(&booking_id)
        .execute(&mut **tx)
        .await?;
        sqlx::query(
            "INSERT INTO bookings (id, trip_id, resource_id, start_date, end_date, quantity, status, requested_by, created_at)
             VALUES (?, ?, ?, ?, ?, ?, 'requested', ?, ?)",
        )
        .bind(new_id())
        .bind(&trip_id)
        .bind(&resource_id)
        .bind(fmt(ns))
        .bind(fmt(ne))
        .bind(quantity)
        .bind(&requested_by)
        .bind(&now)
        .execute(&mut **tx)
        .await?;
        resource_ids.push(resource_id);
    }
    let mut event = AuditEvent {
        entity_type: "trip".into(),
        entity_id: trip_id.clone(),
        ..cr_event(
            actor,
            "trip.rescheduled",
            &trip_id,
            &cr.project_id,
            format!(
                "Trip moved to {} – {}; {} booking(s) released and re-requested",
                fmt(new_arrive),
                fmt(new_depart),
                resource_ids.len()
            ),
        )
    };
    event.before = Some(json!({"arrive_date": fmt(old_arrive), "depart_date": fmt(old_depart)}));
    event.after = Some(json!({"arrive_date": fmt(new_arrive), "depart_date": fmt(new_depart)}));
    audit::record(tx, event).await?;
    Ok(resource_ids)
}

async fn approve_change_request(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<String>,
    Json(req): Json<ResolveChangeRequestRequest>,
) -> AppResult<Json<ChangeRequestDto>> {
    let (row, _) = readable_cr(&state, &actor, &id).await?;
    require_resolver(&actor)?;
    let project_id = row.project_id.clone();

    let mut tx = db::begin_immediate(&state.pool).await?;
    let cr = open_cr(&mut tx, &id).await?;
    let needs_decision = matches!(cr.kind.as_str(), "extend_permit" | "expand_scope");
    let resulting = req
        .resulting_decision_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    if let Some(dec) = &resulting {
        // An extension must be issued on the permit the team asked about,
        // so the other permits of the project stay as they were.
        let target = match cr.kind.as_str() {
            "extend_permit" => cr
                .payload()?
                .get("decision_id")
                .and_then(Value::as_str)
                .map(str::to_string),
            _ => None,
        };
        // For an extension: a current (not superseded), non-revoking successor
        // of exactly that permit — a revocation cannot "implement" it.
        let ok: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM decisions WHERE id = ? AND project_id = ? AND status = 'issued'
               AND (? IS NULL OR (supersedes_id = ? AND kind <> 'revocation'
                                  AND superseded_by_id IS NULL))",
        )
        .bind(dec)
        .bind(&project_id)
        .bind(&target)
        .bind(&target)
        .fetch_one(&mut *tx)
        .await?;
        if ok == 0 {
            let mut fields = HashMap::new();
            fields.insert(
                "resulting_decision_id".to_string(),
                if target.is_some() {
                    "must be the amendment or extension now in force for the permit named in the request"
                } else {
                    "must be an issued decision of this project"
                }
                .to_string(),
            );
            return Err(AppError::Validation { fields });
        }
    } else if needs_decision {
        let mut fields = HashMap::new();
        fields.insert(
            "resulting_decision_id".to_string(),
            "this change needs a new issued decision (amendment or extension)".to_string(),
        );
        return Err(AppError::Validation { fields });
    }

    let mut bm_and_providers: Vec<String> = Vec::new();
    if cr.kind == "reschedule_trip" {
        let resource_ids = apply_reschedule(&mut tx, &actor, &cr).await?;
        bm_and_providers = sqlx::query_scalar(
            "SELECT DISTINCT user_id FROM user_roles WHERE role = 'base_manager' AND revoked_at IS NULL",
        )
        .fetch_all(&mut *tx)
        .await?;
        for rid in resource_ids {
            let provider: Option<String> =
                sqlx::query_scalar("SELECT provider_user_id FROM resources WHERE id = ?")
                    .bind(&rid)
                    .fetch_optional(&mut *tx)
                    .await?
                    .flatten();
            bm_and_providers.extend(provider);
        }
    }
    let note = req
        .note
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty());
    sqlx::query(
        "UPDATE change_requests SET status = 'approved', resolved_by = ?, resolution_note = ?,
                resulting_decision_id = ?
         WHERE id = ?",
    )
    .bind(&actor.user_id)
    .bind(&note)
    .bind(&resulting)
    .bind(&id)
    .execute(&mut *tx)
    .await?;
    let mut event = cr_event(
        &actor,
        "change_request.approved",
        &id,
        &project_id,
        format!(
            "{} approved the change request ({})",
            actor.name,
            cr.kind.replace('_', " ")
        ),
    );
    event.reason = note.clone();
    event.after = Some(json!({"resulting_decision_id": resulting}));
    audit::record(&mut tx, event).await?;

    let title: String = sqlx::query_scalar("SELECT title FROM projects WHERE id = ?")
        .bind(&project_id)
        .fetch_one(&mut *tx)
        .await?;
    let link = format!("/app/projects/{project_id}/changes");
    let team = notify::team_member_ids(&mut *tx, &project_id).await?;
    notify::notify_users(
        &mut tx,
        team,
        "change_request_approved",
        &format!("Change approved: {title}"),
        note.as_deref()
            .unwrap_or("Your change request was approved."),
        &link,
        Some(&project_id),
    )
    .await?;
    if !bm_and_providers.is_empty() {
        notify::notify_users(
            &mut tx,
            bm_and_providers,
            "bookings_rerequested",
            &format!("Bookings re-requested for new trip dates: {title}"),
            "A trip was rescheduled; its bookings were released and requested again for the new dates.",
            "/app/calendar",
            Some(&project_id),
        )
        .await?;
    }
    tx.commit().await?;
    Ok(Json(load_cr(&state.pool, &id).await?.into_dto()?))
}

async fn reject_change_request(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<String>,
    Json(req): Json<RejectChangeRequestRequest>,
) -> AppResult<Json<ChangeRequestDto>> {
    let (row, _) = readable_cr(&state, &actor, &id).await?;
    require_resolver(&actor)?;
    let project_id = row.project_id.clone();
    let note = req
        .note
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty());

    let mut tx = db::begin_immediate(&state.pool).await?;
    open_cr(&mut tx, &id).await?;
    sqlx::query(
        "UPDATE change_requests SET status = 'rejected', resolved_by = ?, resolution_note = ? WHERE id = ?",
    )
    .bind(&actor.user_id)
    .bind(&note)
    .bind(&id)
    .execute(&mut *tx)
    .await?;
    let mut event = cr_event(
        &actor,
        "change_request.rejected",
        &id,
        &project_id,
        format!("{} rejected the change request", actor.name),
    );
    event.reason = note.clone();
    audit::record(&mut tx, event).await?;
    notify::notify_team(
        &mut tx,
        &project_id,
        "change_request_rejected",
        "Change request rejected",
        note.as_deref()
            .unwrap_or("Your change request was rejected."),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(load_cr(&state.pool, &id).await?.into_dto()?))
}

/// The requester or the team lead withdraws an open change request.
async fn withdraw_change_request(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<String>,
) -> AppResult<Json<ChangeRequestDto>> {
    let (row, access) = readable_cr(&state, &actor, &id).await?;
    let is_requester = row.requested_by == actor.user_id
        && matches!(access, ProjectAccess::TeamEditor | ProjectAccess::TeamLead);
    if !is_requester && access != ProjectAccess::TeamLead {
        return Err(AppError::forbidden(
            "only the requester or the team lead can withdraw a change request",
        ));
    }
    let mut tx = db::begin_immediate(&state.pool).await?;
    open_cr(&mut tx, &id).await?;
    sqlx::query("UPDATE change_requests SET status = 'withdrawn', resolved_by = ? WHERE id = ?")
        .bind(&actor.user_id)
        .bind(&id)
        .execute(&mut *tx)
        .await?;
    audit::record(
        &mut tx,
        cr_event(
            &actor,
            "change_request.withdrawn",
            &id,
            &row.project_id,
            format!("{} withdrew the change request", actor.name),
        ),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(load_cr(&state.pool, &id).await?.into_dto()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> NaiveDate {
        date(s).unwrap()
    }

    #[test]
    fn whole_trip_bookings_follow_the_trip_partial_ones_shift() {
        let old = (d("2026-11-01"), d("2026-11-10"));
        let new = (d("2026-12-01"), d("2026-12-12"));
        assert_eq!(shifted_interval(old, old, new), new);
        assert_eq!(
            shifted_interval((d("2026-11-03"), d("2026-11-05")), old, new),
            (d("2026-12-03"), d("2026-12-05"))
        );
        // Clamped to the new (shorter) trip.
        let short = (d("2026-12-01"), d("2026-12-04"));
        assert_eq!(
            shifted_interval((d("2026-11-03"), d("2026-11-08")), old, short),
            (d("2026-12-03"), d("2026-12-04"))
        );
    }
}
