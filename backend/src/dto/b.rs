//! Slice B DTOs: resources & tariffs, trips & bookings, calendar, provider
//! view, invoices, payments, refunds and the bank webhook. Kept in their own
//! module so the shared `export_all!` list in `mod.rs` stays untouched.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

macro_rules! export_all {
    ($($t:ty),* $(,)?) => {
        /// Export this slice's TypeScript bindings (called from dto::export_all).
        pub fn export() -> Result<(), ts_rs::ExportError> {
            $( <$t as TS>::export()?; )*
            Ok(())
        }
    };
}

// ---------------------------------------------------------------------------
// Slice B: resources & tariffs
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct ResourceDto {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub description: String,
    pub quantity: i64,
    pub unit_label: String,
    pub provider_user_id: Option<String>,
    pub provider_name: Option<String>,
    pub active: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct CreateResourceRequest {
    pub kind: String,
    pub name: String,
    pub description: Option<String>,
    pub quantity: i64,
    pub unit_label: Option<String>,
    pub provider_user_id: Option<String>,
    pub active: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct PatchResourceRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub quantity: Option<i64>,
    pub unit_label: Option<String>,
    /// Sets the owning provider (user id).
    pub provider_user_id: Option<String>,
    /// `true` removes the provider (resource becomes base-managed).
    pub clear_provider: Option<bool>,
    pub active: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct TariffDto {
    pub id: String,
    pub resource_id: String,
    pub unit: String,
    pub price_cents: i64,
    pub currency: String,
    pub effective_from: String,
    pub created_by: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct CreateTariffRequest {
    pub unit: String,
    pub price_cents: i64,
    pub currency: Option<String>,
    pub effective_from: String,
}

// ---------------------------------------------------------------------------
// Slice B: trips & bookings
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct TripDto {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub arrive_date: String,
    pub depart_date: String,
    pub participants: Vec<String>,
    pub status: String,
    pub bookings: Vec<BookingDto>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct CreateTripRequest {
    pub title: String,
    pub arrive_date: String,
    pub depart_date: String,
    pub participants: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct PatchTripRequest {
    pub title: Option<String>,
    pub arrive_date: Option<String>,
    pub depart_date: Option<String>,
    pub participants: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct BookingDto {
    pub id: String,
    pub trip_id: String,
    pub resource_id: String,
    pub resource_name: String,
    pub resource_kind: String,
    pub start_date: String,
    pub end_date: String,
    pub quantity: i64,
    pub status: String,
    pub requested_by: String,
    pub decided_by: Option<String>,
    pub decided_at: Option<String>,
    pub decline_reason: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct CreateBookingRequest {
    pub resource_id: String,
    pub start_date: String,
    pub end_date: String,
    pub quantity: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct DeclineBookingRequest {
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct CalendarQuery {
    /// YYYY-MM-DD, required.
    pub from: Option<String>,
    /// YYYY-MM-DD, required, inclusive.
    pub to: Option<String>,
    pub resource_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct CalendarBookingDto {
    pub booking_id: String,
    pub trip_id: String,
    pub status: String,
    pub quantity: i64,
    pub start_date: String,
    pub end_date: String,
    pub project_id: String,
    pub project_reference: Option<String>,
    pub project_title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct CalendarDayDto {
    pub date: String,
    /// Confirmed units in use that day (half-open intervals [start, end)).
    pub used: i64,
    pub bookings: Vec<CalendarBookingDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct CalendarResourceDto {
    pub resource_id: String,
    pub name: String,
    pub kind: String,
    pub capacity: i64,
    pub unit_label: String,
    pub provider_user_id: Option<String>,
    pub days: Vec<CalendarDayDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct CalendarResponse {
    pub from: String,
    pub to: String,
    pub resources: Vec<CalendarResourceDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct ProviderBookingDto {
    pub booking_id: String,
    pub status: String,
    pub start_date: String,
    pub end_date: String,
    pub quantity: i64,
    pub resource_id: String,
    pub resource_name: String,
    pub trip_id: String,
    pub trip_title: String,
    pub trip_arrive_date: String,
    pub trip_depart_date: String,
    pub project_id: String,
    pub project_title: String,
    pub project_reference: Option<String>,
    pub team_size: i64,
    pub lead_name: String,
    pub requested_at: String,
    pub decline_reason: Option<String>,
}

// ---------------------------------------------------------------------------
// Slice B: money (invoices, lines, payments)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct InvoiceLineDto {
    pub id: String,
    pub invoice_id: String,
    pub booking_id: Option<String>,
    pub description: String,
    /// Units charged (nights/days/items); REAL in the schema.
    pub quantity: f64,
    pub unit: String,
    pub unit_price_cents: i64,
    pub amount_cents: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct InvoiceLineInput {
    /// Present = keep/update an existing line id; absent = new line.
    pub id: Option<String>,
    /// Optional link to a confirmed booking of the same project.
    pub booking_id: Option<String>,
    pub description: String,
    pub quantity: f64,
    pub unit: String,
    pub unit_price_cents: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct PaymentDto {
    pub id: String,
    pub invoice_id: String,
    pub kind: String,
    pub amount_cents: i64,
    pub currency: String,
    pub method: String,
    pub external_ref: Option<String>,
    pub status: String,
    pub received_at: String,
    pub verified_by: Option<String>,
    pub note: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct InvoiceDto {
    pub id: String,
    pub project_id: String,
    pub number: Option<String>,
    pub status: String,
    pub currency: String,
    pub total_cents: i64,
    /// verified payments minus verified refunds.
    pub net_verified_cents: i64,
    /// unpaid | partially_paid | paid | overpaid (derived, never stored).
    pub settlement: String,
    pub issued_at: Option<String>,
    pub due_date: Option<String>,
    pub cancelled_reason: Option<String>,
    pub created_by: String,
    pub created_at: String,
    pub lines: Vec<InvoiceLineDto>,
    pub payments: Vec<PaymentDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct CreateInvoiceRequest {
    /// Confirmed bookings of this project; each becomes one priced line.
    pub booking_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct ReplaceInvoiceLinesRequest {
    pub lines: Vec<InvoiceLineInput>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct IssueInvoiceRequest {
    /// YYYY-MM-DD; defaults to 30 days after issue.
    pub due_date: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct CancelInvoiceRequest {
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct CreatePaymentRequest {
    pub amount_cents: i64,
    /// bank_transfer | manual (test_card is only created via pay-test-card).
    pub method: String,
    pub external_ref: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct RejectPaymentRequest {
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct CreateRefundRequest {
    pub amount_cents: i64,
    /// bank_transfer | manual; defaults to bank_transfer.
    pub method: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct PayTestCardResponse {
    /// The recorded verified test payment; null when the invoice was already
    /// fully covered (duplicate call).
    pub payment: Option<PaymentDto>,
    pub duplicate: bool,
    pub invoice: InvoiceDto,
}

/// Body of the bank webhook `POST /integrations/bank/notifications`
/// (also what the demo bank simulator signs and sends).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct BankNotificationPayload {
    /// Invoice number, e.g. INV-2026-0003.
    pub invoice_number: String,
    pub amount_cents: i64,
    pub currency: Option<String>,
    pub external_ref: String,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct BankNotificationResponse {
    pub payment_id: Option<String>,
    pub invoice_id: Option<String>,
    pub duplicate: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct DemoBankNotifyRequest {
    pub invoice_id: String,
    pub amount_cents: i64,
    pub external_ref: String,
    pub note: Option<String>,
}

export_all!(
    ResourceDto,
    CreateResourceRequest,
    PatchResourceRequest,
    TariffDto,
    CreateTariffRequest,
    TripDto,
    CreateTripRequest,
    PatchTripRequest,
    BookingDto,
    CreateBookingRequest,
    DeclineBookingRequest,
    CalendarQuery,
    CalendarBookingDto,
    CalendarDayDto,
    CalendarResourceDto,
    CalendarResponse,
    ProviderBookingDto,
    InvoiceLineDto,
    InvoiceLineInput,
    PaymentDto,
    InvoiceDto,
    CreateInvoiceRequest,
    ReplaceInvoiceLinesRequest,
    IssueInvoiceRequest,
    CancelInvoiceRequest,
    CreatePaymentRequest,
    RejectPaymentRequest,
    CreateRefundRequest,
    PayTestCardResponse,
    BankNotificationPayload,
    BankNotificationResponse,
    DemoBankNotifyRequest,
);
