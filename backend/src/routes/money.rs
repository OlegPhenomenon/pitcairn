//! Invoices, payments, refunds and the bank webhook (architecture §4 "Money",
//! §5 endpoints).
//!
//! - Invoice lines are priced at creation from the tariff in force at the
//!   booking's start date (`latest effective_from <= start_date`); tariff
//!   changes never alter existing lines, and lines are editable only while
//!   the invoice is `draft`.
//! - `settlement` is derived (never stored): verified payments minus verified
//!   refunds vs the line total → unpaid | partially_paid | paid | overpaid.
//! - Payments attach only to `issued` invoices; overpayment is allowed.
//! - Refunds are capped at net verified receipts (422 `refund_exceeds_receipts`).
//! - `/invoices/{id}/pay-test-card` records a verified `test_card` payment
//!   labelled TEST MODE — a demo affordance, no money moves; it is idempotent
//!   (a fully covered invoice answers `duplicate: true` without new rows).
//! - `/integrations/bank/notifications` is unauthenticated: it verifies
//!   HMAC-SHA256 of the raw body (`X-Bank-Signature`, hex) against
//!   `PITCAIRN_BANK_WEBHOOK_SECRET`, is exempt from CSRF (lib.rs), and is
//!   idempotent on `external_ref` — same payload replays 200 `{duplicate:true}`,
//!   a different payload with the same ref conflicts 409 `external_ref_conflict`.

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use chrono::Datelike;
use sqlx::{FromRow, SqliteConnection};

use crate::AppState;
use crate::audit::{self, AuditEvent};
use crate::authz::{self, Actor, ProjectAccess};
use crate::db;
use crate::dto::{
    BankNotificationPayload, BankNotificationResponse, CancelInvoiceRequest, CreateInvoiceRequest,
    CreatePaymentRequest, CreateRefundRequest, DemoBankNotifyRequest, InvoiceDto, InvoiceLineDto,
    IssueInvoiceRequest, ListResponse, PayTestCardResponse, PaymentDto, RejectPaymentRequest,
    ReplaceInvoiceLinesRequest,
};
use crate::error::{AppError, AppResult};
use crate::idempotency;
use crate::notify;
use crate::refs;
use crate::util::{new_id, now_rfc3339, sha256_hex};
use crate::validation::FieldErrors;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/projects/{id}/invoices",
            get(list_invoices).post(create_invoice),
        )
        .route("/invoices/{id}/lines", put(replace_lines))
        .route("/invoices/{id}/issue", post(issue_invoice))
        .route("/invoices/{id}/cancel", post(cancel_invoice))
        .route("/invoices/{id}/payments", post(create_payment))
        .route("/invoices/{id}/refunds", post(create_refund))
        .route("/invoices/{id}/pay-test-card", post(pay_test_card))
        .route("/payments/{id}/verify", post(verify_payment))
        .route("/payments/{id}/reject", post(reject_payment))
        .route("/integrations/bank/notifications", post(bank_notifications))
        .route("/demo/bank/notify", post(demo_bank_notify))
}

// ---------------------------------------------------------------------------
// Loading / derivation
// ---------------------------------------------------------------------------

#[derive(FromRow)]
#[allow(dead_code)]
struct InvoiceRow {
    id: String,
    project_id: String,
    number: Option<String>,
    status: String,
    currency: String,
    issued_at: Option<String>,
    due_date: Option<String>,
    cancelled_reason: Option<String>,
    created_by: String,
    created_at: String,
}

#[derive(FromRow)]
#[allow(dead_code)]
struct LineRow {
    id: String,
    invoice_id: String,
    booking_id: Option<String>,
    description: String,
    quantity: f64,
    unit: String,
    unit_price_cents: i64,
    amount_cents: i64,
}

#[derive(FromRow)]
#[allow(dead_code)]
struct PaymentRow {
    id: String,
    invoice_id: String,
    kind: String,
    amount_cents: i64,
    currency: String,
    method: String,
    external_ref: Option<String>,
    status: String,
    received_at: String,
    verified_by: Option<String>,
    note: String,
    created_at: String,
}

fn line_dto(r: LineRow) -> InvoiceLineDto {
    InvoiceLineDto {
        id: r.id,
        invoice_id: r.invoice_id,
        booking_id: r.booking_id,
        description: r.description,
        quantity: r.quantity,
        unit: r.unit,
        unit_price_cents: r.unit_price_cents,
        amount_cents: r.amount_cents,
    }
}

fn payment_dto(r: PaymentRow) -> PaymentDto {
    PaymentDto {
        id: r.id,
        invoice_id: r.invoice_id,
        kind: r.kind,
        amount_cents: r.amount_cents,
        currency: r.currency,
        method: r.method,
        external_ref: r.external_ref,
        status: r.status,
        received_at: r.received_at,
        verified_by: r.verified_by,
        note: r.note,
        created_at: r.created_at,
    }
}

/// Derived settlement status (§4): never stored.
fn settlement(total_cents: i64, net_verified_cents: i64) -> &'static str {
    if net_verified_cents > total_cents {
        "overpaid"
    } else if net_verified_cents == total_cents {
        "paid"
    } else if net_verified_cents > 0 {
        "partially_paid"
    } else {
        "unpaid"
    }
}

async fn load_invoice_dto(conn: &mut SqliteConnection, invoice_id: &str) -> AppResult<InvoiceDto> {
    let inv = load_invoice_row(conn, invoice_id).await?;
    let lines = invoice_lines(conn, invoice_id).await?;
    let payments = invoice_payments(conn, invoice_id).await?;
    Ok(invoice_dto_from_parts(inv, lines, payments))
}

fn invoice_dto_from_parts(
    inv: InvoiceRow,
    lines: Vec<LineRow>,
    payments: Vec<PaymentRow>,
) -> InvoiceDto {
    let total_cents: i64 = lines.iter().map(|l| l.amount_cents).sum();
    let net_verified_cents: i64 = payments
        .iter()
        .filter(|p| p.status == "verified")
        .map(|p| {
            if p.kind == "refund" {
                -p.amount_cents
            } else {
                p.amount_cents
            }
        })
        .sum();
    InvoiceDto {
        id: inv.id,
        project_id: inv.project_id,
        number: inv.number,
        status: inv.status,
        currency: inv.currency,
        total_cents,
        net_verified_cents,
        settlement: settlement(total_cents, net_verified_cents).into(),
        issued_at: inv.issued_at,
        due_date: inv.due_date,
        cancelled_reason: inv.cancelled_reason,
        created_by: inv.created_by,
        created_at: inv.created_at,
        lines: lines.into_iter().map(line_dto).collect(),
        payments: payments.into_iter().map(payment_dto).collect(),
    }
}

async fn invoice_lines(conn: &mut SqliteConnection, invoice_id: &str) -> AppResult<Vec<LineRow>> {
    Ok(sqlx::query_as(
        "SELECT id, invoice_id, booking_id, description, quantity, unit,
                unit_price_cents, amount_cents
         FROM invoice_lines WHERE invoice_id = ? ORDER BY created_at, id",
    )
    .bind(invoice_id)
    .fetch_all(conn)
    .await?)
}

async fn invoice_payments(
    conn: &mut SqliteConnection,
    invoice_id: &str,
) -> AppResult<Vec<PaymentRow>> {
    Ok(sqlx::query_as(
        "SELECT id, invoice_id, kind, amount_cents, currency, method, external_ref,
                status, received_at, verified_by, note, created_at
         FROM payments WHERE invoice_id = ? ORDER BY created_at, id",
    )
    .bind(invoice_id)
    .fetch_all(conn)
    .await?)
}

/// Net verified receipts (verified payments minus verified refunds).
async fn net_verified(conn: &mut SqliteConnection, invoice_id: &str) -> AppResult<i64> {
    let net: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(CASE kind WHEN 'payment' THEN amount_cents
                              ELSE -amount_cents END), 0)
         FROM payments WHERE invoice_id = ? AND status = 'verified'",
    )
    .bind(invoice_id)
    .fetch_one(conn)
    .await?;
    Ok(net)
}

/// Invoices of a project honouring the list rule (§5): finance sees all,
/// everyone else with project access sees issued + cancelled only.
pub async fn invoices_for_project(
    pool: &sqlx::SqlitePool,
    actor: &Actor,
    project_id: &str,
) -> AppResult<Vec<InvoiceDto>> {
    let all = actor.has_role("finance");
    let ids: Vec<String> = if all {
        sqlx::query_scalar("SELECT id FROM invoices WHERE project_id = ? ORDER BY created_at")
            .bind(project_id)
            .fetch_all(pool)
            .await?
    } else {
        sqlx::query_scalar(
            "SELECT id FROM invoices WHERE project_id = ? AND status != 'draft'
             ORDER BY created_at",
        )
        .bind(project_id)
        .fetch_all(pool)
        .await?
    };
    let mut conn = pool.acquire().await?;
    let mut out = Vec::new();
    for id in ids {
        out.push(load_invoice_dto(&mut conn, &id).await?);
    }
    Ok(out)
}

async fn notify_project_team(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    project_id: &str,
    kind: &str,
    title: &str,
    body: &str,
    link: &str,
) -> AppResult<()> {
    let ids: Vec<String> = sqlx::query_scalar(
        "SELECT user_id FROM project_members WHERE project_id = ? AND removed_at IS NULL",
    )
    .bind(project_id)
    .fetch_all(&mut **tx)
    .await?;
    for uid in ids {
        notify::notify(tx, &uid, kind, title, body, link, Some(project_id)).await?;
    }
    Ok(())
}

fn idempotency_key(headers: &HeaderMap) -> Option<String> {
    headers
        .get("idempotency-key")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
        .filter(|k| !k.is_empty() && k.len() <= 200)
}

// ---------------------------------------------------------------------------
// Invoices
// ---------------------------------------------------------------------------

async fn list_invoices(
    State(state): State<AppState>,
    actor: Actor,
    Path(project_id): Path<String>,
) -> AppResult<Json<ListResponse<InvoiceDto>>> {
    let access = authz::project_access(&state.pool, &actor, &project_id).await?;
    if access < ProjectAccess::TeamViewer {
        return Err(AppError::forbidden(
            "you do not have access to this project",
        ));
    }
    let items = invoices_for_project(&state.pool, &actor, &project_id).await?;
    let total = items.len() as i64;
    Ok(Json(ListResponse { items, total }))
}

/// Price of a booking under the tariff in force at its start date: returns
/// `(line quantity, unit, unit_price_cents, currency)`. `None` when the
/// resource has no applicable tariff.
#[derive(Debug)]
struct PricedLine {
    quantity: f64,
    unit: String,
    unit_price_cents: i64,
    currency: String,
    description: String,
}

async fn price_booking(
    conn: &mut SqliteConnection,
    booking_id: &str,
) -> AppResult<Option<PricedLine>> {
    let row: Option<(String, String, String, i64, String)> = sqlx::query_as(
        "SELECT r.name, b.start_date, b.end_date, b.quantity, b.id
         FROM bookings b JOIN resources r ON r.id = b.resource_id
         WHERE b.id = ?",
    )
    .bind(booking_id)
    .fetch_optional(&mut *conn)
    .await?;
    let Some((resource_name, start, end, booking_qty, _)) = row else {
        return Ok(None);
    };
    let tariff: Option<(String, i64, String)> = sqlx::query_as(
        "SELECT unit, price_cents, currency FROM tariffs
         WHERE resource_id = (SELECT resource_id FROM bookings WHERE id = ?)
           AND effective_from <= ?
         ORDER BY effective_from DESC LIMIT 1",
    )
    .bind(booking_id)
    .bind(&start)
    .fetch_optional(&mut *conn)
    .await?;
    let Some((unit, price_cents, currency)) = tariff else {
        return Ok(None);
    };
    let days = (chrono::NaiveDate::parse_from_str(&end, "%Y-%m-%d").map_err(AppError::internal)?
        - chrono::NaiveDate::parse_from_str(&start, "%Y-%m-%d").map_err(AppError::internal)?)
    .num_days();
    // §4: per_night → nights, per_day → days, per_item → quantity. Nights and
    // days of a half-open [start,end) interval are the same count.
    let (quantity, span_label) = match unit.as_str() {
        "per_night" => ((days * booking_qty) as f64, "nights"),
        "per_day" => ((days * booking_qty) as f64, "days"),
        _ => (booking_qty as f64, "items"),
    };
    Ok(Some(PricedLine {
        quantity,
        unit,
        unit_price_cents: price_cents,
        currency,
        description: format!("{resource_name} — {start} → {end} ({span_label})"),
    }))
}

async fn create_invoice(
    State(state): State<AppState>,
    actor: Actor,
    Path(project_id): Path<String>,
    Json(req): Json<CreateInvoiceRequest>,
) -> AppResult<impl IntoResponse> {
    authz::require_role(&actor, &["finance"])?;

    let mut errors = FieldErrors::new();
    errors.check(
        "booking_ids",
        !req.booking_ids.is_empty() && req.booking_ids.len() <= 100,
        "provide 1–100 confirmed bookings",
    );
    errors.finish()?;

    // Every referenced booking must be a confirmed booking of THIS project and
    // not already billed on a live invoice (a guessed id never slips through).
    // Checked inside BEGIN IMMEDIATE so a booking cannot be billed twice.
    let mut tx = db::begin_immediate(&state.pool).await?;
    let mut priced = Vec::new();
    for booking_id in &req.booking_ids {
        let row: Option<(String, String)> = sqlx::query_as(
            "SELECT t.project_id, b.status FROM bookings b
             JOIN trips t ON t.id = b.trip_id WHERE b.id = ?",
        )
        .bind(booking_id)
        .fetch_optional(&mut *tx)
        .await?;
        let ok = matches!(&row, Some((pid, status)) if pid == &project_id && status == "confirmed");
        if !ok {
            return Err(AppError::Validation {
                fields: std::collections::HashMap::from([(
                    "booking_ids".into(),
                    format!("booking {booking_id} is not a confirmed booking of this project"),
                )]),
            });
        }
        let already: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM invoice_lines il
             JOIN invoices i ON i.id = il.invoice_id
             WHERE il.booking_id = ? AND i.status != 'cancelled'",
        )
        .bind(booking_id)
        .fetch_one(&mut *tx)
        .await?;
        if already > 0 {
            return Err(AppError::Validation {
                fields: std::collections::HashMap::from([(
                    "booking_ids".into(),
                    format!("booking {booking_id} is already on an invoice"),
                )]),
            });
        }
        let line = price_booking(&mut tx, booking_id).await?;
        let Some(line) = line else {
            return Err(AppError::Validation {
                fields: std::collections::HashMap::from([(
                    "booking_ids".into(),
                    format!("booking {booking_id} has no tariff effective at its start date"),
                )]),
            });
        };
        priced.push((booking_id.clone(), line));
    }

    let currency = priced
        .first()
        .map(|(_, l)| l.currency.clone())
        .unwrap_or_else(|| "NZD".into());
    if priced.iter().any(|(_, l)| l.currency != currency) {
        return Err(AppError::Validation {
            fields: std::collections::HashMap::from([(
                "booking_ids".into(),
                "bookings price in mixed currencies; split into separate invoices".to_string(),
            )]),
        });
    }

    let invoice_id = new_id();
    let now = now_rfc3339();
    sqlx::query(
        "INSERT INTO invoices (id, project_id, status, currency, created_by, created_at)
         VALUES (?, ?, 'draft', ?, ?, ?)",
    )
    .bind(&invoice_id)
    .bind(&project_id)
    .bind(&currency)
    .bind(&actor.user_id)
    .bind(&now)
    .execute(&mut *tx)
    .await?;

    for (booking_id, line) in &priced {
        let amount = (line.quantity * line.unit_price_cents as f64).round() as i64;
        sqlx::query(
            "INSERT INTO invoice_lines
             (id, invoice_id, booking_id, description, quantity, unit, unit_price_cents, amount_cents, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(new_id())
        .bind(&invoice_id)
        .bind(booking_id)
        .bind(&line.description)
        .bind(line.quantity)
        .bind(&line.unit)
        .bind(line.unit_price_cents)
        .bind(amount)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
    }

    audit::record(
        &mut tx,
        AuditEvent {
            actor_id: Some(actor.user_id.clone()),
            actor_label: actor.name.clone(),
            action: "invoice.created".into(),
            entity_type: "invoice".into(),
            entity_id: invoice_id.clone(),
            project_id: Some(project_id.clone()),
            visibility: "internal".into(),
            summary: format!(
                "{} drafted an invoice with {} lines",
                actor.name,
                priced.len()
            ),
            before: None,
            after: Some(serde_json::json!({ "lines": priced.len() })),
            reason: None,
        },
    )
    .await?;

    let dto = load_invoice_dto(&mut tx, &invoice_id).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(dto)))
}

async fn load_invoice_row(conn: &mut SqliteConnection, id: &str) -> AppResult<InvoiceRow> {
    sqlx::query_as(
        "SELECT id, project_id, number, status, currency, issued_at, due_date,
                cancelled_reason, created_by, created_at
         FROM invoices WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(conn)
    .await?
    .ok_or(AppError::NotFound)
}

/// Whole-sale replacement of an invoice's lines; only while `draft` (§4:
/// "lines are editable only while the invoice is draft").
async fn replace_lines(
    State(state): State<AppState>,
    actor: Actor,
    Path(invoice_id): Path<String>,
    Json(req): Json<ReplaceInvoiceLinesRequest>,
) -> AppResult<Json<InvoiceDto>> {
    authz::require_role(&actor, &["finance"])?;
    let mut tx = db::begin_immediate(&state.pool).await?;
    let invoice = load_invoice_row(&mut tx, &invoice_id).await?;
    if invoice.status != "draft" {
        return Err(AppError::conflict(
            "invalid_state",
            "invoice lines are editable only while the invoice is draft",
        ));
    }

    let mut errors = FieldErrors::new();
    for (i, line) in req.lines.iter().enumerate() {
        let f = |name: &str| format!("lines.{i}.{name}");
        errors.require(
            &f("description"),
            &line.description,
            "description is required",
        );
        errors.check(&f("quantity"), line.quantity > 0.0, "must be > 0");
        errors.check(
            &f("unit_price_cents"),
            line.unit_price_cents >= 0,
            "must be >= 0",
        );
        errors.require(&f("unit"), &line.unit, "unit is required");
        errors.max_len(&f("unit"), &line.unit, 30);
    }
    errors.finish()?;

    // Referenced bookings must be confirmed bookings of this project.
    for line in &req.lines {
        if let Some(booking_id) = &line.booking_id {
            let ok: Option<(String,)> = sqlx::query_as(
                "SELECT t.project_id FROM bookings b JOIN trips t ON t.id = b.trip_id
                 WHERE b.id = ? AND b.status = 'confirmed'",
            )
            .bind(booking_id)
            .fetch_optional(&mut *tx)
            .await?;
            if ok.as_ref().map(|(pid,)| pid) != Some(&invoice.project_id) {
                return Err(AppError::Validation {
                    fields: std::collections::HashMap::from([(
                        "booking_id".into(),
                        format!("booking {booking_id} is not a confirmed booking of this project"),
                    )]),
                });
            }
        }
    }

    let now = now_rfc3339();
    sqlx::query("DELETE FROM invoice_lines WHERE invoice_id = ?")
        .bind(&invoice_id)
        .execute(&mut *tx)
        .await?;
    for line in &req.lines {
        let amount = (line.quantity * line.unit_price_cents as f64).round() as i64;
        sqlx::query(
            "INSERT INTO invoice_lines
             (id, invoice_id, booking_id, description, quantity, unit, unit_price_cents, amount_cents, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(line.id.clone().unwrap_or_else(new_id))
        .bind(&invoice_id)
        .bind(&line.booking_id)
        .bind(&line.description)
        .bind(line.quantity)
        .bind(&line.unit)
        .bind(line.unit_price_cents)
        .bind(amount)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
    }

    audit::record(
        &mut tx,
        AuditEvent {
            actor_id: Some(actor.user_id.clone()),
            actor_label: actor.name.clone(),
            action: "invoice.lines_updated".into(),
            entity_type: "invoice".into(),
            entity_id: invoice_id.clone(),
            project_id: Some(invoice.project_id.clone()),
            visibility: "internal".into(),
            summary: format!(
                "{} edited invoice lines ({} lines)",
                actor.name,
                req.lines.len()
            ),
            before: None,
            after: Some(serde_json::json!({ "lines": req.lines.len() })),
            reason: None,
        },
    )
    .await?;

    let dto = load_invoice_dto(&mut tx, &invoice_id).await?;
    tx.commit().await?;
    Ok(Json(dto))
}

async fn issue_invoice(
    State(state): State<AppState>,
    actor: Actor,
    Path(invoice_id): Path<String>,
    headers: HeaderMap,
    Json(req): Json<IssueInvoiceRequest>,
) -> AppResult<Json<InvoiceDto>> {
    authz::require_role(&actor, &["finance"])?;

    if let Some(due) = &req.due_date {
        let mut errors = FieldErrors::new();
        errors.valid_date("due_date", due);
        errors.finish()?;
    }

    let request_hash = sha256_hex(
        serde_json::to_string(&req)
            .map_err(AppError::internal)?
            .as_bytes(),
    );
    let route = format!("POST /invoices/{invoice_id}/issue");

    let key = idempotency_key(&headers);
    let mut tx = db::begin_immediate(&state.pool).await?;
    if let Some(key) = &key
        && let Some((_, dto)) =
            idempotency::replay::<InvoiceDto>(&mut tx, &actor.user_id, &route, key, &request_hash)
                .await?
    {
        tx.commit().await?;
        return Ok(Json(dto));
    }
    let dto = issue_invoice_inner(&mut tx, &actor, &invoice_id, req.due_date.clone()).await?;
    if let Some(key) = &key {
        idempotency::store(
            &mut tx,
            &actor.user_id,
            &route,
            key,
            &request_hash,
            200,
            &dto,
        )
        .await?;
    }
    tx.commit().await?;
    Ok(Json(dto))
}

async fn issue_invoice_inner(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    actor: &Actor,
    invoice_id: &str,
    due_date: Option<String>,
) -> AppResult<InvoiceDto> {
    let invoice = load_invoice_row(tx, invoice_id).await?;
    if invoice.status != "draft" {
        return Err(AppError::conflict(
            "invalid_state",
            format!("cannot issue an invoice that is {}", invoice.status),
        ));
    }
    let line_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM invoice_lines WHERE invoice_id = ?")
            .bind(invoice_id)
            .fetch_one(&mut **tx)
            .await?;
    if line_count == 0 {
        return Err(AppError::Validation {
            fields: std::collections::HashMap::from([(
                "lines".into(),
                "invoice has no lines".to_string(),
            )]),
        });
    }

    let now = now_rfc3339();
    let year = chrono::Utc::now().year() as i64;
    let number = refs::next(tx, "INV", year).await?;
    let due = due_date.unwrap_or_else(|| {
        (chrono::Utc::now() + chrono::Duration::days(30))
            .format("%Y-%m-%d")
            .to_string()
    });
    sqlx::query(
        "UPDATE invoices SET status = 'issued', number = ?, issued_at = ?, due_date = ?
         WHERE id = ?",
    )
    .bind(&number)
    .bind(&now)
    .bind(&due)
    .bind(invoice_id)
    .execute(&mut **tx)
    .await?;

    notify_project_team(
        tx,
        &invoice.project_id,
        "invoice.issued",
        &format!("Invoice {number} issued"),
        &format!("Invoice {number} was issued; please see the Invoices tab"),
        &format!("/app/projects/{}/invoices", invoice.project_id),
    )
    .await?;

    audit::record(
        tx,
        AuditEvent {
            actor_id: Some(actor.user_id.clone()),
            actor_label: actor.name.clone(),
            action: "invoice.issued".into(),
            entity_type: "invoice".into(),
            entity_id: invoice_id.to_string(),
            project_id: Some(invoice.project_id.clone()),
            visibility: "shared".into(),
            summary: format!("{} issued invoice {number}", actor.name),
            before: Some(serde_json::json!({ "status": "draft" })),
            after: Some(serde_json::json!({
                "status": "issued", "number": number, "due_date": due,
            })),
            reason: None,
        },
    )
    .await?;

    load_invoice_dto(tx, invoice_id).await
}

async fn cancel_invoice(
    State(state): State<AppState>,
    actor: Actor,
    Path(invoice_id): Path<String>,
    Json(req): Json<CancelInvoiceRequest>,
) -> AppResult<Json<InvoiceDto>> {
    authz::require_role(&actor, &["finance"])?;
    let mut errors = FieldErrors::new();
    errors.require("reason", &req.reason, "a cancellation reason is required");
    errors.finish()?;

    let mut tx = db::begin_immediate(&state.pool).await?;
    let invoice = load_invoice_row(&mut tx, &invoice_id).await?;
    if invoice.status == "cancelled" {
        return Err(AppError::conflict(
            "invalid_state",
            "invoice is already cancelled",
        ));
    }
    // §4: an invoice with net verified receipts > 0 can be cancelled only
    // after refunding them.
    let net = net_verified(&mut tx, &invoice_id).await?;
    if net > 0 {
        return Err(AppError::conflict(
            "invoice_has_receipts",
            "invoice has verified receipts; refund them before cancelling",
        ));
    }
    sqlx::query("UPDATE invoices SET status = 'cancelled', cancelled_reason = ? WHERE id = ?")
        .bind(&req.reason)
        .bind(&invoice_id)
        .execute(&mut *tx)
        .await?;
    // Pending reports die with the invoice.
    sqlx::query(
        "UPDATE payments SET status = 'rejected'
         WHERE invoice_id = ? AND status = 'pending_verification'",
    )
    .bind(&invoice_id)
    .execute(&mut *tx)
    .await?;

    notify_project_team(
        &mut tx,
        &invoice.project_id,
        "invoice.cancelled",
        "Invoice cancelled",
        &format!(
            "Invoice {} was cancelled: {}",
            invoice.number.as_deref().unwrap_or("draft"),
            req.reason
        ),
        &format!("/app/projects/{}/invoices", invoice.project_id),
    )
    .await?;

    audit::record(
        &mut tx,
        AuditEvent {
            actor_id: Some(actor.user_id.clone()),
            actor_label: actor.name.clone(),
            action: "invoice.cancelled".into(),
            entity_type: "invoice".into(),
            entity_id: invoice_id.clone(),
            project_id: Some(invoice.project_id.clone()),
            visibility: "shared".into(),
            summary: format!(
                "{} cancelled invoice {} ({})",
                actor.name,
                invoice.number.as_deref().unwrap_or("draft"),
                req.reason
            ),
            before: Some(serde_json::json!({ "status": invoice.status })),
            after: Some(serde_json::json!({ "status": "cancelled" })),
            reason: Some(req.reason.clone()),
        },
    )
    .await?;

    let dto = load_invoice_dto(&mut tx, &invoice_id).await?;
    tx.commit().await?;
    Ok(Json(dto))
}

// ---------------------------------------------------------------------------
// Payments & refunds
// ---------------------------------------------------------------------------

async fn create_payment(
    State(state): State<AppState>,
    actor: Actor,
    Path(invoice_id): Path<String>,
    headers: HeaderMap,
    Json(req): Json<CreatePaymentRequest>,
) -> AppResult<impl IntoResponse> {
    authz::require_role(&actor, &["finance"])?;

    let mut errors = FieldErrors::new();
    errors.check("amount_cents", req.amount_cents > 0, "must be > 0");
    errors.check(
        "method",
        matches!(req.method.as_str(), "bank_transfer" | "manual"),
        "must be bank_transfer or manual",
    );
    errors.finish()?;

    let request_hash = sha256_hex(
        serde_json::to_string(&req)
            .map_err(AppError::internal)?
            .as_bytes(),
    );
    let route = format!("POST /invoices/{invoice_id}/payments");

    let key = idempotency_key(&headers);
    let mut tx = db::begin_immediate(&state.pool).await?;
    if let Some(key) = &key
        && let Some((status, dto)) =
            idempotency::replay::<PaymentDto>(&mut tx, &actor.user_id, &route, key, &request_hash)
                .await?
    {
        tx.commit().await?;
        return Ok((
            StatusCode::from_u16(status).unwrap_or(StatusCode::CREATED),
            Json(dto),
        ));
    }
    let (status, dto) = create_payment_inner(&mut tx, &actor, &invoice_id, req).await?;
    if let Some(key) = &key {
        idempotency::store(
            &mut tx,
            &actor.user_id,
            &route,
            key,
            &request_hash,
            status,
            &dto,
        )
        .await?;
    }
    tx.commit().await?;
    Ok((
        StatusCode::from_u16(status).unwrap_or(StatusCode::CREATED),
        Json(dto),
    ))
}

async fn create_payment_inner(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    actor: &Actor,
    invoice_id: &str,
    req: CreatePaymentRequest,
) -> AppResult<(u16, PaymentDto)> {
    let invoice = load_invoice_row(tx, invoice_id).await?;
    if invoice.status != "issued" {
        return Err(AppError::conflict(
            "invoice_not_issued",
            "payments can be recorded only on issued invoices",
        ));
    }
    if let Some(external_ref) = &req.external_ref {
        let exists: Option<(String,)> =
            sqlx::query_as("SELECT id FROM payments WHERE external_ref = ?")
                .bind(external_ref)
                .fetch_optional(&mut **tx)
                .await?;
        if exists.is_some() {
            return Err(AppError::conflict(
                "external_ref_conflict",
                "a payment with this external_ref already exists",
            ));
        }
    }
    let payment_id = new_id();
    let now = now_rfc3339();
    sqlx::query(
        "INSERT INTO payments
         (id, invoice_id, kind, amount_cents, currency, method, external_ref,
          status, received_at, note, created_at)
         VALUES (?, ?, 'payment', ?, ?, ?, ?, 'pending_verification', ?, ?, ?)",
    )
    .bind(&payment_id)
    .bind(invoice_id)
    .bind(req.amount_cents)
    .bind(&invoice.currency)
    .bind(&req.method)
    .bind(&req.external_ref)
    .bind(&now)
    .bind(req.note.clone().unwrap_or_default())
    .bind(&now)
    .execute(&mut **tx)
    .await?;

    audit::record(
        tx,
        AuditEvent {
            actor_id: Some(actor.user_id.clone()),
            actor_label: actor.name.clone(),
            action: "payment.recorded".into(),
            entity_type: "payment".into(),
            entity_id: payment_id.clone(),
            project_id: Some(invoice.project_id.clone()),
            visibility: "internal".into(),
            summary: format!(
                "{} recorded {} {:.2} {} on {}",
                actor.name,
                req.method,
                req.amount_cents as f64 / 100.0,
                invoice.currency,
                invoice.number.as_deref().unwrap_or("invoice")
            ),
            before: None,
            after: Some(serde_json::json!({
                "amount_cents": req.amount_cents, "method": req.method,
            })),
            reason: None,
        },
    )
    .await?;

    let row: PaymentRow = sqlx::query_as(
        "SELECT id, invoice_id, kind, amount_cents, currency, method, external_ref,
                status, received_at, verified_by, note, created_at
         FROM payments WHERE id = ?",
    )
    .bind(&payment_id)
    .fetch_one(&mut **tx)
    .await?;
    Ok((201, payment_dto(row)))
}

/// Decide a payment (verify / reject); only `pending_verification` moves.
async fn decide_payment(
    state: &AppState,
    actor: &Actor,
    payment_id: &str,
    decision: &str,
    note: Option<&str>,
) -> AppResult<Json<PaymentDto>> {
    authz::require_role(actor, &["finance"])?;

    let mut tx = db::begin_immediate(&state.pool).await?;
    let row: Option<(String, String, String)> = sqlx::query_as(
        "SELECT p.status, p.invoice_id, i.project_id FROM payments p
         JOIN invoices i ON i.id = p.invoice_id WHERE p.id = ?",
    )
    .bind(payment_id)
    .fetch_optional(&mut *tx)
    .await?;
    let (status, invoice_id, project_id) = row.ok_or(AppError::NotFound)?;
    if status != "pending_verification" {
        return Err(AppError::conflict(
            "invalid_state",
            format!("payment is {status}, only pending payments can be decided"),
        ));
    }

    sqlx::query("UPDATE payments SET status = ?, verified_by = ? WHERE id = ?")
        .bind(decision)
        .bind(&actor.user_id)
        .bind(payment_id)
        .execute(&mut *tx)
        .await?;

    let kind = format!("payment.{decision}");
    notify_project_team(
        &mut tx,
        &project_id,
        &kind,
        &format!("Payment {decision}"),
        &format!(
            "A payment on invoice {} was {decision}{}",
            invoice_id,
            note.map(|n| format!(": {n}")).unwrap_or_default()
        ),
        &format!("/app/projects/{project_id}/invoices"),
    )
    .await?;

    audit::record(
        &mut tx,
        AuditEvent {
            actor_id: Some(actor.user_id.clone()),
            actor_label: actor.name.clone(),
            action: kind.clone(),
            entity_type: "payment".into(),
            entity_id: payment_id.to_string(),
            project_id: Some(project_id.clone()),
            visibility: "internal".into(),
            summary: format!("{} marked a payment {decision}", actor.name),
            before: Some(serde_json::json!({ "status": "pending_verification" })),
            after: Some(serde_json::json!({ "status": decision })),
            reason: note.map(str::to_string),
        },
    )
    .await?;

    tx.commit().await?;
    let row: PaymentRow = sqlx::query_as(
        "SELECT id, invoice_id, kind, amount_cents, currency, method, external_ref,
                status, received_at, verified_by, note, created_at
         FROM payments WHERE id = ?",
    )
    .bind(payment_id)
    .fetch_one(&state.pool)
    .await?;
    Ok(Json(payment_dto(row)))
}

async fn verify_payment(
    State(state): State<AppState>,
    actor: Actor,
    Path(payment_id): Path<String>,
) -> AppResult<Json<PaymentDto>> {
    decide_payment(&state, &actor, &payment_id, "verified", None).await
}

async fn reject_payment(
    State(state): State<AppState>,
    actor: Actor,
    Path(payment_id): Path<String>,
    Json(req): Json<RejectPaymentRequest>,
) -> AppResult<Json<PaymentDto>> {
    decide_payment(&state, &actor, &payment_id, "rejected", req.note.as_deref()).await
}

async fn create_refund(
    State(state): State<AppState>,
    actor: Actor,
    Path(invoice_id): Path<String>,
    Json(req): Json<CreateRefundRequest>,
) -> AppResult<impl IntoResponse> {
    authz::require_role(&actor, &["finance"])?;

    let method = req.method.clone().unwrap_or_else(|| "bank_transfer".into());
    let mut errors = FieldErrors::new();
    errors.check("amount_cents", req.amount_cents > 0, "must be > 0");
    errors.check(
        "method",
        matches!(method.as_str(), "bank_transfer" | "manual"),
        "must be bank_transfer or manual",
    );
    errors.finish()?;

    let mut tx = db::begin_immediate(&state.pool).await?;
    let invoice = load_invoice_row(&mut tx, &invoice_id).await?;
    if invoice.status != "issued" {
        return Err(AppError::conflict(
            "invoice_not_issued",
            "refunds can be recorded only on issued invoices",
        ));
    }
    let net = net_verified(&mut tx, &invoice_id).await?;
    if req.amount_cents > net {
        return Err(AppError::unprocessable(
            "refund_exceeds_receipts",
            format!(
                "refund of {} exceeds net verified receipts of {net}",
                req.amount_cents
            ),
        ));
    }

    let payment_id = new_id();
    let now = now_rfc3339();
    sqlx::query(
        "INSERT INTO payments
         (id, invoice_id, kind, amount_cents, currency, method, status, received_at,
          verified_by, note, created_at)
         VALUES (?, ?, 'refund', ?, ?, ?, 'verified', ?, ?, ?, ?)",
    )
    .bind(&payment_id)
    .bind(&invoice_id)
    .bind(req.amount_cents)
    .bind(&invoice.currency)
    .bind(&method)
    .bind(&now)
    .bind(&actor.user_id)
    .bind(req.note.clone().unwrap_or_default())
    .bind(&now)
    .execute(&mut *tx)
    .await?;

    notify_project_team(
        &mut tx,
        &invoice.project_id,
        "refund.recorded",
        "Refund recorded",
        &format!(
            "A refund of {:.2} {} was recorded on invoice {}",
            req.amount_cents as f64 / 100.0,
            invoice.currency,
            invoice.number.as_deref().unwrap_or("invoice")
        ),
        &format!("/app/projects/{}/invoices", invoice.project_id),
    )
    .await?;

    audit::record(
        &mut tx,
        AuditEvent {
            actor_id: Some(actor.user_id.clone()),
            actor_label: actor.name.clone(),
            action: "refund.recorded".into(),
            entity_type: "payment".into(),
            entity_id: payment_id.clone(),
            project_id: Some(invoice.project_id.clone()),
            visibility: "internal".into(),
            summary: format!(
                "{} recorded a refund of {:.2} {}",
                actor.name,
                req.amount_cents as f64 / 100.0,
                invoice.currency
            ),
            before: None,
            after: Some(serde_json::json!({ "amount_cents": req.amount_cents })),
            reason: req.note.clone(),
        },
    )
    .await?;

    tx.commit().await?;
    let row: PaymentRow = sqlx::query_as(
        "SELECT id, invoice_id, kind, amount_cents, currency, method, external_ref,
                status, received_at, verified_by, note, created_at
         FROM payments WHERE id = ?",
    )
    .bind(&payment_id)
    .fetch_one(&state.pool)
    .await?;
    Ok((StatusCode::CREATED, Json(payment_dto(row))))
}

/// TEST MODE payment (§5): a team editor+ "pays" the outstanding amount with
/// a test card. Records a verified `test_card` payment clearly labelled TEST
/// MODE — no money ever moves. Idempotent: when the invoice is already fully
/// covered the call is a no-op (`duplicate: true`).
async fn pay_test_card(
    State(state): State<AppState>,
    actor: Actor,
    Path(invoice_id): Path<String>,
    headers: HeaderMap,
) -> AppResult<(StatusCode, Json<PayTestCardResponse>)> {
    let mut tx = db::begin_immediate(&state.pool).await?;
    let invoice = load_invoice_row(&mut tx, &invoice_id).await?;
    let access = authz::project_access(&mut *tx, &actor, &invoice.project_id).await?;
    if !matches!(access, ProjectAccess::TeamEditor | ProjectAccess::TeamLead) {
        return Err(AppError::forbidden(
            "test-card payment requires team editor or lead",
        ));
    }

    let request_hash = sha256_hex(invoice_id.as_bytes());
    let route = format!("POST /invoices/{invoice_id}/pay-test-card");
    let key = idempotency_key(&headers);
    if let Some(key) = &key
        && let Some((status, resp)) = idempotency::replay::<PayTestCardResponse>(
            &mut tx,
            &actor.user_id,
            &route,
            key,
            &request_hash,
        )
        .await?
    {
        tx.commit().await?;
        return Ok((
            StatusCode::from_u16(status).unwrap_or(StatusCode::OK),
            Json(resp),
        ));
    }
    let (status, resp) = pay_test_card_inner(&mut tx, &actor, &invoice_id).await?;
    if let Some(key) = &key {
        idempotency::store(
            &mut tx,
            &actor.user_id,
            &route,
            key,
            &request_hash,
            status,
            &resp,
        )
        .await?;
    }
    tx.commit().await?;
    Ok((
        StatusCode::from_u16(status).unwrap_or(StatusCode::OK),
        Json(resp),
    ))
}

async fn pay_test_card_inner(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    actor: &Actor,
    invoice_id: &str,
) -> AppResult<(u16, PayTestCardResponse)> {
    let invoice = load_invoice_row(tx, invoice_id).await?;
    if invoice.status != "issued" {
        return Err(AppError::conflict(
            "invoice_not_issued",
            "test-card payment applies only to issued invoices",
        ));
    }
    let lines = invoice_lines(tx, invoice_id).await?;
    let total: i64 = lines.iter().map(|l| l.amount_cents).sum();
    let net = net_verified(tx, invoice_id).await?;
    let outstanding = total - net;
    if outstanding <= 0 {
        let latest: Option<PaymentRow> = sqlx::query_as(
            "SELECT id, invoice_id, kind, amount_cents, currency, method, external_ref,
                    status, received_at, verified_by, note, created_at
             FROM payments WHERE invoice_id = ? AND method = 'test_card'
             ORDER BY created_at DESC LIMIT 1",
        )
        .bind(invoice_id)
        .fetch_optional(&mut **tx)
        .await?;
        let dto = load_invoice_dto(tx, invoice_id).await?;
        return Ok((
            200,
            PayTestCardResponse {
                payment: latest.map(payment_dto),
                duplicate: true,
                invoice: dto,
            },
        ));
    }

    // Unique per call so a refund reopening the balance can be re-paid.
    let seq: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM payments WHERE invoice_id = ? AND method = 'test_card'",
    )
    .bind(invoice_id)
    .fetch_one(&mut **tx)
    .await?;
    let external_ref = format!("test-card:{invoice_id}:{}", seq + 1);
    let payment_id = new_id();
    let now = now_rfc3339();
    sqlx::query(
        "INSERT INTO payments
         (id, invoice_id, kind, amount_cents, currency, method, external_ref,
          status, received_at, verified_by, note, created_at)
         VALUES (?, ?, 'payment', ?, ?, 'test_card', ?, 'verified', ?, ?, ?, ?)",
    )
    .bind(&payment_id)
    .bind(invoice_id)
    .bind(outstanding)
    .bind(&invoice.currency)
    .bind(&external_ref)
    .bind(&now)
    .bind(&actor.user_id)
    .bind("TEST MODE — demo test card payment; no money moved")
    .bind(&now)
    .execute(&mut **tx)
    .await?;

    notify_project_team(
        tx,
        &invoice.project_id,
        "payment.verified",
        "Invoice paid (test mode)",
        &format!(
            "Invoice {} was paid in TEST MODE — no real money moved",
            invoice.number.as_deref().unwrap_or("invoice")
        ),
        &format!("/app/projects/{}/invoices", invoice.project_id),
    )
    .await?;

    audit::record(
        tx,
        AuditEvent {
            actor_id: Some(actor.user_id.clone()),
            actor_label: actor.name.clone(),
            action: "payment.test_card".into(),
            entity_type: "payment".into(),
            entity_id: payment_id.clone(),
            project_id: Some(invoice.project_id.clone()),
            visibility: "internal".into(),
            summary: format!(
                "{} paid invoice {} in TEST MODE ({:.2} {})",
                actor.name,
                invoice.number.as_deref().unwrap_or("invoice"),
                outstanding as f64 / 100.0,
                invoice.currency
            ),
            before: None,
            after: Some(serde_json::json!({
                "amount_cents": outstanding, "method": "test_card",
            })),
            reason: None,
        },
    )
    .await?;

    let row: PaymentRow = sqlx::query_as(
        "SELECT id, invoice_id, kind, amount_cents, currency, method, external_ref,
                status, received_at, verified_by, note, created_at
         FROM payments WHERE id = ?",
    )
    .bind(&payment_id)
    .fetch_one(&mut **tx)
    .await?;
    let dto = load_invoice_dto(tx, invoice_id).await?;
    Ok((
        201,
        PayTestCardResponse {
            payment: Some(payment_dto(row)),
            duplicate: false,
            invoice: dto,
        },
    ))
}

// ---------------------------------------------------------------------------
// Bank webhook (HMAC-authenticated, no session, no CSRF — exempted in lib.rs)
// ---------------------------------------------------------------------------

/// HMAC-SHA256 hex of `body` under the webhook secret.
pub fn bank_signature(secret: &str, body: &[u8]) -> String {
    use hmac::{Hmac, Mac};
    let mut mac = <Hmac<sha2::Sha256> as Mac>::new_from_slice(secret.as_bytes())
        .expect("HMAC accepts any key length");
    mac.update(body);
    hex::encode(mac.finalize().into_bytes())
}

fn bank_signature_valid(secret: &str, body: &[u8], signature_hex: &str) -> bool {
    use hmac::{Hmac, Mac};
    let Ok(expected_bytes) = hex::decode(signature_hex) else {
        return false;
    };
    let mut mac = <Hmac<sha2::Sha256> as Mac>::new_from_slice(secret.as_bytes())
        .expect("HMAC accepts any key length");
    mac.update(body);
    mac.verify_slice(&expected_bytes).is_ok()
}

/// Shared implementation for the real webhook route and the demo bank
/// simulator (`POST /demo/bank/notify` signs and calls this).
pub async fn apply_bank_notification(
    state: &AppState,
    raw_body: &[u8],
    signature_hex: Option<&str>,
) -> AppResult<(StatusCode, Json<BankNotificationResponse>)> {
    let Some(signature) = signature_hex else {
        return Err(AppError::auth_failed(
            "bad_signature",
            "missing X-Bank-Signature",
        ));
    };
    if !bank_signature_valid(&state.config.bank_webhook_secret, raw_body, signature) {
        return Err(AppError::auth_failed(
            "bad_signature",
            "invalid X-Bank-Signature",
        ));
    }

    let payload: BankNotificationPayload = serde_json::from_slice(raw_body).map_err(|_| {
        AppError::unprocessable("bad_payload", "body is not a valid bank notification")
    })?;

    let mut errors = FieldErrors::new();
    errors.require("invoice_number", &payload.invoice_number, "required");
    errors.require("external_ref", &payload.external_ref, "required");
    errors.check("amount_cents", payload.amount_cents > 0, "must be > 0");
    errors.finish()?;

    let request_hash = sha256_hex(raw_body);

    let mut tx = db::begin_immediate(&state.pool).await?;
    // Idempotent on external_ref (§4): same ref + same payload → duplicate;
    // same ref + different payload → 409 external_ref_conflict.
    let existing: Option<(String, Option<String>, String)> =
        sqlx::query_as("SELECT id, request_hash, invoice_id FROM payments WHERE external_ref = ?")
            .bind(&payload.external_ref)
            .fetch_optional(&mut *tx)
            .await?;
    if let Some((payment_id, stored_hash, invoice_id)) = existing {
        if stored_hash.as_deref() == Some(request_hash.as_str()) {
            tx.commit().await?;
            return Ok((
                StatusCode::OK,
                Json(BankNotificationResponse {
                    payment_id: Some(payment_id),
                    invoice_id: Some(invoice_id),
                    duplicate: true,
                }),
            ));
        }
        return Err(AppError::conflict(
            "external_ref_conflict",
            "external_ref was already used with a different payload",
        ));
    }

    let invoice: Option<(String, String, String)> =
        sqlx::query_as("SELECT id, status, currency FROM invoices WHERE number = ?")
            .bind(&payload.invoice_number)
            .fetch_optional(&mut *tx)
            .await?;
    let (invoice_id, invoice_status, invoice_currency) = invoice
        .ok_or_else(|| AppError::unprocessable("unknown_invoice", "no invoice with that number"))?;
    if invoice_status != "issued" {
        return Err(AppError::conflict(
            "invoice_not_issued",
            "payments can be recorded only on issued invoices",
        ));
    }
    if let Some(currency) = &payload.currency
        && currency != &invoice_currency
    {
        return Err(AppError::unprocessable(
            "currency_mismatch",
            "notification currency does not match the invoice currency",
        ));
    }

    let payment_id = new_id();
    let now = now_rfc3339();
    sqlx::query(
        "INSERT INTO payments
         (id, invoice_id, kind, amount_cents, currency, method, external_ref,
          request_hash, status, received_at, note, created_at)
         VALUES (?, ?, 'payment', ?, ?, 'bank_transfer', ?, ?, 'pending_verification', ?, ?, ?)",
    )
    .bind(&payment_id)
    .bind(&invoice_id)
    .bind(payload.amount_cents)
    .bind(&invoice_currency)
    .bind(&payload.external_ref)
    .bind(&request_hash)
    .bind(&now)
    .bind(payload.note.clone().unwrap_or_default())
    .bind(&now)
    .execute(&mut *tx)
    .await?;

    // Finance verifies the reported transfer.
    let finance_ids: Vec<String> = sqlx::query_scalar(
        "SELECT user_id FROM user_roles WHERE role = 'finance' AND revoked_at IS NULL",
    )
    .fetch_all(&mut *tx)
    .await?;
    let project_id: String = sqlx::query_scalar("SELECT project_id FROM invoices WHERE id = ?")
        .bind(&invoice_id)
        .fetch_one(&mut *tx)
        .await?;
    for uid in finance_ids {
        notify::notify(
            &mut tx,
            &uid,
            "payment.bank_reported",
            "Bank transfer reported",
            &format!(
                "A bank transfer of {:.2} {} was reported for {} (ref {})",
                payload.amount_cents as f64 / 100.0,
                invoice_currency,
                payload.invoice_number,
                payload.external_ref
            ),
            "/app/finance",
            Some(&project_id),
        )
        .await?;
    }

    audit::record(
        &mut tx,
        AuditEvent {
            actor_id: None,
            actor_label: "bank-webhook".into(),
            action: "payment.bank_reported".into(),
            entity_type: "payment".into(),
            entity_id: payment_id.clone(),
            project_id: Some(project_id.clone()),
            visibility: "internal".into(),
            summary: format!(
                "Bank transfer reported for {} ({:.2} {}, ref {})",
                payload.invoice_number,
                payload.amount_cents as f64 / 100.0,
                invoice_currency,
                payload.external_ref
            ),
            before: None,
            after: Some(serde_json::json!({
                "amount_cents": payload.amount_cents,
                "external_ref": payload.external_ref,
            })),
            reason: None,
        },
    )
    .await?;

    tx.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(BankNotificationResponse {
            payment_id: Some(payment_id),
            invoice_id: Some(invoice_id),
            duplicate: false,
        }),
    ))
}

async fn bank_notifications(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> AppResult<(StatusCode, Json<BankNotificationResponse>)> {
    let signature = headers
        .get("x-bank-signature")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    apply_bank_notification(&state, &body, signature.as_deref()).await
}

/// Demo bank simulator (§5, §9; 404 unless demo mode, finance only): builds
/// the bank's payload for an issued invoice, signs it with the webhook secret
/// and runs it through the same path as the real webhook.
async fn demo_bank_notify(
    State(state): State<AppState>,
    actor: Actor,
    Json(req): Json<DemoBankNotifyRequest>,
) -> AppResult<(StatusCode, Json<BankNotificationResponse>)> {
    if !state.config.demo_mode {
        return Err(AppError::NotFound);
    }
    authz::require_role(&actor, &["finance"])?;
    let mut errors = FieldErrors::new();
    errors.require("external_ref", &req.external_ref, "required");
    errors.max_len("external_ref", &req.external_ref, 200);
    errors.check("amount_cents", req.amount_cents > 0, "must be > 0");
    errors.finish()?;

    let invoice: Option<(Option<String>, String)> =
        sqlx::query_as("SELECT number, currency FROM invoices WHERE id = ?")
            .bind(&req.invoice_id)
            .fetch_optional(&state.pool)
            .await?;
    let (number, currency) = invoice.ok_or(AppError::NotFound)?;
    let Some(invoice_number) = number else {
        return Err(AppError::conflict(
            "invoice_not_issued",
            "only issued invoices have a number the bank can reference",
        ));
    };
    let payload = BankNotificationPayload {
        invoice_number,
        amount_cents: req.amount_cents,
        currency: Some(currency),
        external_ref: req.external_ref,
        note: req.note,
    };
    let body = serde_json::to_vec(&payload).map_err(AppError::internal)?;
    let signature = bank_signature(&state.config.bank_webhook_secret, &body);
    apply_bank_notification(&state, &body, Some(&signature)).await
}
