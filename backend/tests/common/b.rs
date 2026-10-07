//! Slice B test helpers: an approved project with Anna's team, seeded MSB
//! resources, trip/booking/invoice shortcuts and bank-webhook signing.

use pitcairn::dto::{BookingDto, InvoiceDto, ResourceDto, TripDto};
use serde_json::json;

use super::{Client, TestApp, persona, spawn_app};

pub const TEST_BANK_SECRET: &str = "test-bank-webhook-secret-000000000000000000000000";

pub struct Fixture {
    pub app: TestApp,
    pub project_id: String,
    pub anna: Client,
}

/// Demo app with Anna's seeded project moved to `status` (fixture shortcut:
/// the decision workflow belongs to another slice).
pub async fn fixture_with_status(status: &str) -> Fixture {
    let app = spawn_app(true).await;
    let project_id: String =
        sqlx::query_scalar("SELECT id FROM projects WHERE title = 'Coral health around Pitcairn'")
            .fetch_one(&app.pool)
            .await
            .expect("seeded project");
    sqlx::query("UPDATE projects SET status = ?, reference = 'PIT-2026-0001' WHERE id = ?")
        .bind(status)
        .bind(&project_id)
        .execute(&app.pool)
        .await
        .expect("set project status");
    let anna = persona(&app, "anna").await;
    Fixture {
        app,
        project_id,
        anna,
    }
}

pub async fn fixture() -> Fixture {
    fixture_with_status("approved").await
}

pub async fn resource_id(app: &TestApp, name: &str) -> String {
    sqlx::query_scalar("SELECT id FROM resources WHERE name = ?")
        .bind(name)
        .fetch_one(&app.pool)
        .await
        .unwrap_or_else(|_| panic!("seeded resource {name}"))
}

pub const LAB: &str = "MSB wet laboratory";
pub const ROOM: &str = "MSB twin bedroom";
pub const TANKS: &str = "SCUBA tank set";
pub const BOAT: &str = "Boat charter — Bounty Bay Boat Hire (fictional)";

/// `(status, error.code, error.message)` of an error response.
pub async fn error_of(resp: reqwest::Response) -> (u16, String, String) {
    let status = resp.status().as_u16();
    let text = resp.text().await.expect("error body");
    let body: serde_json::Value = serde_json::from_str(&text).expect("error json");
    (
        status,
        body["error"]["code"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
        body["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
    )
}

pub async fn create_trip(client: &Client, project_id: &str, arrive: &str, depart: &str) -> TripDto {
    let resp = client
        .post_json(
            &format!("/api/v1/projects/{project_id}/trips"),
            &json!({"title": "Reef survey", "arrive_date": arrive, "depart_date": depart}),
        )
        .await;
    assert_eq!(resp.status(), 201, "create trip");
    client.json(resp).await
}

pub async fn request_booking(
    client: &Client,
    trip_id: &str,
    resource_id: &str,
    start: &str,
    end: &str,
    quantity: i64,
) -> BookingDto {
    let resp = client
        .post_json(
            &format!("/api/v1/trips/{trip_id}/bookings"),
            &json!({"resource_id": resource_id, "start_date": start, "end_date": end, "quantity": quantity}),
        )
        .await;
    assert_eq!(resp.status(), 201, "request booking");
    client.json(resp).await
}

pub async fn confirm(client: &Client, booking_id: &str) -> reqwest::Response {
    client
        .post(&format!("/api/v1/bookings/{booking_id}/confirm"))
        .await
}

pub async fn create_invoice(client: &Client, project_id: &str, booking_ids: &[&str]) -> InvoiceDto {
    let resp = client
        .post_json(
            &format!("/api/v1/projects/{project_id}/invoices"),
            &json!({ "booking_ids": booking_ids }),
        )
        .await;
    assert_eq!(resp.status(), 201, "create invoice");
    client.json(resp).await
}

pub async fn issue(client: &Client, invoice_id: &str) -> InvoiceDto {
    let resp = client
        .post_json(&format!("/api/v1/invoices/{invoice_id}/issue"), &json!({}))
        .await;
    assert_eq!(resp.status(), 200, "issue invoice");
    client.json(resp).await
}

/// Confirmed room booking (2 rooms × 3 nights) on an approved project and an
/// issued invoice for it. Returns `(finance client, invoice)`.
pub async fn issued_room_invoice(fx: &Fixture) -> (Client, InvoiceDto) {
    let sam = persona(&fx.app, "sam").await;
    let ruth = persona(&fx.app, "ruth").await;
    let room = resource_id(&fx.app, ROOM).await;
    let trip = create_trip(&fx.anna, &fx.project_id, "2027-03-01", "2027-03-10").await;
    let booking = request_booking(&fx.anna, &trip.id, &room, "2027-03-01", "2027-03-04", 2).await;
    assert_eq!(confirm(&sam, &booking.id).await.status(), 200);
    let invoice = create_invoice(&ruth, &fx.project_id, &[&booking.id]).await;
    let invoice = issue(&ruth, &invoice.id).await;
    (ruth, invoice)
}

pub fn sign(body: &[u8]) -> String {
    pitcairn::routes::money::bank_signature(TEST_BANK_SECRET, body)
}

/// POST the raw body to the bank webhook (no session, no CSRF header).
pub async fn bank_post(app: &TestApp, body: &[u8], signature: &str) -> reqwest::Response {
    Client::anonymous(app)
        .request_no_csrf(
            reqwest::Method::POST,
            "/api/v1/integrations/bank/notifications",
        )
        .header("content-type", "application/json")
        .header("x-bank-signature", signature)
        .body(body.to_vec())
        .send()
        .await
        .expect("send bank notification")
}

pub async fn admin_create_resource(
    admin: &Client,
    kind: &str,
    name: &str,
    quantity: i64,
    provider_user_id: Option<&str>,
) -> ResourceDto {
    let resp = admin
        .post_json(
            "/api/v1/resources",
            &json!({"kind": kind, "name": name, "quantity": quantity, "provider_user_id": provider_user_id}),
        )
        .await;
    assert_eq!(resp.status(), 201, "create resource");
    admin.json(resp).await
}

pub async fn user_id(app: &TestApp, key: &str) -> String {
    let email = pitcairn::seed::PERSONAS
        .iter()
        .find(|p| p.key == key)
        .expect("persona")
        .email;
    sqlx::query_scalar("SELECT id FROM users WHERE email = ?")
        .bind(email)
        .fetch_one(&app.pool)
        .await
        .expect("user id")
}
