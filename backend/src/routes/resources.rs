//! Resources & tariffs (architecture §4, §5): admin manages; authenticated
//! researchers can read the catalog to request bookings.
//! Tariffs are append-only — a price change is a new row with a later
//! `effective_from`; old rows are never edited so issued invoice lines stay
//! consistent with the price in force at the booking's start date.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, patch};
use axum::{Json, Router};
use sqlx::FromRow;

use crate::AppState;
use crate::audit::{self, AuditEvent};
use crate::authz::{self, Actor};
use crate::db;
use crate::dto::{
    CreateResourceRequest, CreateTariffRequest, ListResponse, PatchResourceRequest, ResourceDto,
    TariffDto,
};
use crate::error::{AppError, AppResult};
use crate::util::{new_id, now_rfc3339};
use crate::validation::FieldErrors;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/resources", get(list_resources).post(create_resource))
        .route("/resources/{id}", patch(patch_resource))
        .route(
            "/resources/{id}/tariffs",
            get(list_tariffs).post(create_tariff),
        )
}

const RESOURCE_KINDS: &[&str] = &["room", "lab", "equipment", "boat", "service"];
/// `per_hour` exists in the schema, but bookings are day-granular (§4), so
/// hourly tariffs are never used and not accepted.
const TARIFF_UNITS: &[&str] = &["per_night", "per_day", "per_item"];

/// Resource catalogue editors (§7: Pitcairn staff configure it via the UI):
/// the technical admin and the base manager.
const RESOURCE_EDITORS: &[&str] = &["admin", "base_manager"];
/// Price editors: resource editors plus finance. Tariffs stay append-only.
const TARIFF_EDITORS: &[&str] = &["admin", "base_manager", "finance"];

#[derive(FromRow)]
#[allow(dead_code)]
struct ResourceRow {
    id: String,
    kind: String,
    name: String,
    description: String,
    quantity: i64,
    unit_label: String,
    provider_user_id: Option<String>,
    provider_name: Option<String>,
    active: i64,
    created_at: String,
}

const RESOURCE_SELECT: &str =
    "SELECT r.id, r.kind, r.name, r.description, r.quantity, r.unit_label,
            r.provider_user_id, u.name AS provider_name, r.active, r.created_at
     FROM resources r LEFT JOIN users u ON u.id = r.provider_user_id";

impl From<ResourceRow> for ResourceDto {
    fn from(r: ResourceRow) -> Self {
        ResourceDto {
            id: r.id,
            kind: r.kind,
            name: r.name,
            description: r.description,
            quantity: r.quantity,
            unit_label: r.unit_label,
            provider_user_id: r.provider_user_id,
            provider_name: r.provider_name,
            active: r.active != 0,
            created_at: r.created_at,
        }
    }
}

async fn load_resource_dto(pool: &sqlx::SqlitePool, id: &str) -> AppResult<ResourceDto> {
    let row: ResourceRow = sqlx::query_as(&format!("{RESOURCE_SELECT} WHERE r.id = ?"))
        .bind(id)
        .fetch_one(pool)
        .await?;
    Ok(row.into())
}

/// GET: any staff role; team members and providers do not list resources (§3
/// matrix: resources are an admin/staff domain).
async fn list_resources(
    State(state): State<AppState>,
    actor: Actor,
) -> AppResult<Json<ListResponse<ResourceDto>>> {
    let _ = actor; // Actor extractor requires an authenticated session.
    let rows: Vec<ResourceRow> =
        sqlx::query_as(&format!("{RESOURCE_SELECT} ORDER BY r.kind, r.name"))
            .fetch_all(&state.pool)
            .await?;
    let total = rows.len() as i64;
    let items = rows.into_iter().map(ResourceDto::from).collect();
    Ok(Json(ListResponse { items, total }))
}

async fn create_resource(
    State(state): State<AppState>,
    actor: Actor,
    Json(req): Json<CreateResourceRequest>,
) -> AppResult<impl IntoResponse> {
    authz::require_role(&actor, RESOURCE_EDITORS)?;

    let mut errors = FieldErrors::new();
    errors.check(
        "kind",
        RESOURCE_KINDS.contains(&req.kind.as_str()),
        "must be one of room, lab, equipment, boat, service",
    );
    errors.require("name", &req.name, "name is required");
    errors.max_len("name", &req.name, 200);
    errors.check("quantity", req.quantity >= 1, "must be at least 1");
    errors.finish()?;

    if let Some(provider_id) = &req.provider_user_id {
        let exists: Option<(String,)> = sqlx::query_as("SELECT id FROM users WHERE id = ?")
            .bind(provider_id)
            .fetch_optional(&state.pool)
            .await?;
        if exists.is_none() {
            return Err(AppError::unprocessable(
                "unknown_provider",
                "provider_user_id does not reference a user",
            ));
        }
    }

    let id = new_id();
    let now = now_rfc3339();
    let mut tx = db::begin_immediate(&state.pool).await?;
    sqlx::query(
        "INSERT INTO resources
         (id, kind, name, description, quantity, unit_label, provider_user_id, active, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&req.kind)
    .bind(&req.name)
    .bind(req.description.clone().unwrap_or_default())
    .bind(req.quantity)
    .bind(req.unit_label.clone().unwrap_or_default())
    .bind(&req.provider_user_id)
    .bind(req.active.unwrap_or(true) as i64)
    .bind(&now)
    .execute(&mut *tx)
    .await?;

    audit::record(
        &mut tx,
        AuditEvent {
            actor_id: Some(actor.user_id.clone()),
            actor_label: actor.name.clone(),
            action: "resource.created".into(),
            entity_type: "resource".into(),
            entity_id: id.clone(),
            project_id: None,
            visibility: "internal".into(),
            summary: format!("{} created resource '{}'", actor.name, req.name),
            before: None,
            after: Some(serde_json::json!({
                "name": req.name, "kind": req.kind, "quantity": req.quantity,
            })),
            reason: None,
        },
    )
    .await?;

    tx.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(load_resource_dto(&state.pool, &id).await?),
    ))
}

async fn patch_resource(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<String>,
    Json(req): Json<PatchResourceRequest>,
) -> AppResult<Json<ResourceDto>> {
    authz::require_role(&actor, RESOURCE_EDITORS)?;

    let before: ResourceRow = sqlx::query_as(&format!("{RESOURCE_SELECT} WHERE r.id = ?"))
        .bind(&id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or(AppError::NotFound)?;

    let mut errors = FieldErrors::new();
    if let Some(name) = &req.name {
        errors.require("name", name, "name is required");
        errors.max_len("name", name, 200);
    }
    if let Some(quantity) = req.quantity {
        errors.check("quantity", quantity >= 1, "must be at least 1");
    }
    errors.check(
        "clear_provider",
        !(req.clear_provider == Some(true) && req.provider_user_id.is_some()),
        "cannot set and clear the provider at once",
    );
    errors.finish()?;

    if let Some(provider_id) = &req.provider_user_id {
        let exists: Option<(String,)> = sqlx::query_as("SELECT id FROM users WHERE id = ?")
            .bind(provider_id)
            .fetch_optional(&state.pool)
            .await?;
        if exists.is_none() {
            return Err(AppError::unprocessable(
                "unknown_provider",
                "provider_user_id does not reference a user",
            ));
        }
    }

    let mut tx = db::begin_immediate(&state.pool).await?;
    if let Some(name) = &req.name {
        sqlx::query("UPDATE resources SET name = ? WHERE id = ?")
            .bind(name)
            .bind(&id)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(description) = &req.description {
        sqlx::query("UPDATE resources SET description = ? WHERE id = ?")
            .bind(description)
            .bind(&id)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(quantity) = req.quantity {
        sqlx::query("UPDATE resources SET quantity = ? WHERE id = ?")
            .bind(quantity)
            .bind(&id)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(unit_label) = &req.unit_label {
        sqlx::query("UPDATE resources SET unit_label = ? WHERE id = ?")
            .bind(unit_label)
            .bind(&id)
            .execute(&mut *tx)
            .await?;
    }
    let new_provider = if req.clear_provider == Some(true) {
        Some(None)
    } else {
        req.provider_user_id.clone().map(Some)
    };
    if let Some(provider) = &new_provider {
        sqlx::query("UPDATE resources SET provider_user_id = ? WHERE id = ?")
            .bind(provider)
            .bind(&id)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(active) = req.active {
        sqlx::query("UPDATE resources SET active = ? WHERE id = ?")
            .bind(active as i64)
            .bind(&id)
            .execute(&mut *tx)
            .await?;
    }

    audit::record(
        &mut tx,
        AuditEvent {
            actor_id: Some(actor.user_id.clone()),
            actor_label: actor.name.clone(),
            action: "resource.updated".into(),
            entity_type: "resource".into(),
            entity_id: id.clone(),
            project_id: None,
            visibility: "internal".into(),
            summary: format!("{} updated resource '{}'", actor.name, before.name),
            before: Some(serde_json::json!({
                "name": before.name, "quantity": before.quantity,
                "active": before.active != 0,
                "provider_user_id": before.provider_user_id,
            })),
            after: Some(serde_json::json!({
                "name": req.name, "quantity": req.quantity,
                "active": req.active,
                "provider_user_id": req.provider_user_id,
            })),
            reason: None,
        },
    )
    .await?;

    tx.commit().await?;
    Ok(Json(load_resource_dto(&state.pool, &id).await?))
}

#[derive(FromRow)]
#[allow(dead_code)]
struct TariffRow {
    id: String,
    resource_id: String,
    unit: String,
    price_cents: i64,
    currency: String,
    effective_from: String,
    created_by: String,
    created_at: String,
}

impl From<TariffRow> for TariffDto {
    fn from(r: TariffRow) -> Self {
        TariffDto {
            id: r.id,
            resource_id: r.resource_id,
            unit: r.unit,
            price_cents: r.price_cents,
            currency: r.currency,
            effective_from: r.effective_from,
            created_by: r.created_by,
            created_at: r.created_at,
        }
    }
}

async fn ensure_resource_exists(pool: &sqlx::SqlitePool, id: &str) -> AppResult<()> {
    let exists: Option<(String,)> = sqlx::query_as("SELECT id FROM resources WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await?;
    exists.map(|_| ()).ok_or(AppError::NotFound)
}

async fn list_tariffs(
    State(state): State<AppState>,
    actor: Actor,
    Path(resource_id): Path<String>,
) -> AppResult<Json<ListResponse<TariffDto>>> {
    let _ = actor; // Booking requesters need current tariff information.
    ensure_resource_exists(&state.pool, &resource_id).await?;
    let rows: Vec<TariffRow> = sqlx::query_as(
        "SELECT id, resource_id, unit, price_cents, currency, effective_from, created_by, created_at
         FROM tariffs WHERE resource_id = ? ORDER BY effective_from DESC",
    )
    .bind(&resource_id)
    .fetch_all(&state.pool)
    .await?;
    let total = rows.len() as i64;
    Ok(Json(ListResponse {
        items: rows.into_iter().map(TariffDto::from).collect(),
        total,
    }))
}

async fn create_tariff(
    State(state): State<AppState>,
    actor: Actor,
    Path(resource_id): Path<String>,
    Json(req): Json<CreateTariffRequest>,
) -> AppResult<impl IntoResponse> {
    authz::require_role(&actor, TARIFF_EDITORS)?;
    ensure_resource_exists(&state.pool, &resource_id).await?;

    let mut errors = FieldErrors::new();
    errors.check(
        "unit",
        TARIFF_UNITS.contains(&req.unit.as_str()),
        "must be one of per_night, per_day, per_item",
    );
    errors.check("price_cents", req.price_cents >= 0, "must be >= 0");
    errors.valid_date("effective_from", &req.effective_from);
    let currency = req.currency.clone().unwrap_or_else(|| "NZD".into());
    errors.check(
        "currency",
        currency.len() == 3 && currency.chars().all(|c| c.is_ascii_uppercase()),
        "must be a 3-letter currency code",
    );
    errors.finish()?;

    let id = new_id();
    let now = now_rfc3339();
    let mut tx = db::begin_immediate(&state.pool).await?;
    sqlx::query(
        "INSERT INTO tariffs
         (id, resource_id, unit, price_cents, currency, effective_from, created_by, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&resource_id)
    .bind(&req.unit)
    .bind(req.price_cents)
    .bind(&currency)
    .bind(&req.effective_from)
    .bind(&actor.user_id)
    .bind(&now)
    .execute(&mut *tx)
    .await?;

    audit::record(
        &mut tx,
        AuditEvent {
            actor_id: Some(actor.user_id.clone()),
            actor_label: actor.name.clone(),
            action: "tariff.created".into(),
            entity_type: "tariff".into(),
            entity_id: id.clone(),
            project_id: None,
            visibility: "internal".into(),
            summary: format!(
                "{} added {} {} {}/{} tariff effective {}",
                actor.name, req.unit, currency, req.price_cents, resource_id, req.effective_from
            ),
            before: None,
            after: Some(serde_json::json!({
                "resource_id": resource_id, "unit": req.unit,
                "price_cents": req.price_cents, "currency": currency,
                "effective_from": req.effective_from,
            })),
            reason: None,
        },
    )
    .await?;

    tx.commit().await?;
    let row: TariffRow = sqlx::query_as(
        "SELECT id, resource_id, unit, price_cents, currency, effective_from, created_by, created_at
         FROM tariffs WHERE id = ?",
    )
    .bind(&id)
    .fetch_one(&state.pool)
    .await?;
    Ok((StatusCode::CREATED, Json(TariffDto::from(row))))
}
