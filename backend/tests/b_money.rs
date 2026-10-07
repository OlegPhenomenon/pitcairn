mod common;

use common::b::*;
use common::persona;
use pitcairn::dto::{
    BankNotificationResponse, InvoiceDto, ListResponse, PayTestCardResponse, PaymentDto,
};
use serde_json::json;

async fn reload(client: &common::Client, project_id: &str, invoice_id: &str) -> InvoiceDto {
    let list: ListResponse<InvoiceDto> = client
        .json(
            client
                .get(&format!("/api/v1/projects/{project_id}/invoices"))
                .await,
        )
        .await;
    list.items
        .into_iter()
        .find(|i| i.id == invoice_id)
        .expect("invoice listed")
}

async fn payment_count(app: &common::TestApp) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM payments")
        .fetch_one(&app.pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn tariff_change_after_line_creation_does_not_change_invoice() {
    let fx = fixture().await;
    let sam = persona(&fx.app, "sam").await;
    let ruth = persona(&fx.app, "ruth").await;
    let admin = persona(&fx.app, "admin").await;
    let room = resource_id(&fx.app, ROOM).await;

    let trip = create_trip(&fx.anna, &fx.project_id, "2027-03-01", "2027-03-10").await;
    let booking = request_booking(&fx.anna, &trip.id, &room, "2027-03-01", "2027-03-04", 2).await;
    assert_eq!(confirm(&sam, &booking.id).await.status(), 200);

    let draft = create_invoice(&ruth, &fx.project_id, &[&booking.id]).await;
    assert_eq!(draft.status, "draft");
    assert_eq!(draft.lines.len(), 1);
    let line = &draft.lines[0];
    // per_night: 3 nights × 2 rooms at the seeded demo price.
    assert_eq!(line.unit, "per_night");
    assert_eq!(line.quantity, 6.0);
    assert_eq!(line.unit_price_cents, 9_500);
    assert_eq!(draft.total_cents, 57_000);

    // New tariff effective before the booking start.
    let resp = admin
        .post_json(
            &format!("/api/v1/resources/{room}/tariffs"),
            &json!({"unit": "per_night", "price_cents": 20_000, "effective_from": "2026-01-01"}),
        )
        .await;
    assert_eq!(resp.status(), 201);

    let issued = issue(&ruth, &draft.id).await;
    assert_eq!(issued.total_cents, 57_000, "lines are snapshots");
    assert_eq!(issued.lines[0].unit_price_cents, 9_500);
    let number = issued.number.clone().expect("number assigned");
    assert!(number.starts_with("INV-") && number.len() == "INV-2026-0001".len());

    // Issued lines are frozen.
    let resp = ruth
        .put_json(
            &format!("/api/v1/invoices/{}/lines", issued.id),
            &json!({"lines": [{"description": "x", "quantity": 1.0, "unit": "per_item", "unit_price_cents": 1}]}),
        )
        .await;
    assert_eq!(error_of(resp).await.0, 409);

    // A fresh invoice for a new booking prices from the new tariff.
    let booking2 = request_booking(&fx.anna, &trip.id, &room, "2027-03-05", "2027-03-06", 1).await;
    assert_eq!(confirm(&sam, &booking2.id).await.status(), 200);
    let second = create_invoice(&ruth, &fx.project_id, &[&booking2.id]).await;
    assert_eq!(second.lines[0].unit_price_cents, 20_000);

    // A booking already on a live invoice cannot be billed twice.
    let resp = ruth
        .post_json(
            &format!("/api/v1/projects/{}/invoices", fx.project_id),
            &json!({"booking_ids": [booking.id]}),
        )
        .await;
    assert_eq!(resp.status(), 422);

    // Team sees issued invoices only; finance sees drafts too.
    let team: ListResponse<InvoiceDto> = fx
        .anna
        .json(
            fx.anna
                .get(&format!("/api/v1/projects/{}/invoices", fx.project_id))
                .await,
        )
        .await;
    assert_eq!(team.total, 1);
    let fin: ListResponse<InvoiceDto> = ruth
        .json(
            ruth.get(&format!("/api/v1/projects/{}/invoices", fx.project_id))
                .await,
        )
        .await;
    assert_eq!(fin.total, 2);
    // Only finance creates invoices.
    let resp = sam
        .post_json(
            &format!("/api/v1/projects/{}/invoices", fx.project_id),
            &json!({"booking_ids": [booking2.id]}),
        )
        .await;
    assert_eq!(resp.status(), 403);
}

#[tokio::test]
async fn issue_is_idempotent_with_key() {
    let fx = fixture().await;
    let sam = persona(&fx.app, "sam").await;
    let ruth = persona(&fx.app, "ruth").await;
    let room = resource_id(&fx.app, ROOM).await;
    let trip = create_trip(&fx.anna, &fx.project_id, "2027-03-01", "2027-03-10").await;
    let booking = request_booking(&fx.anna, &trip.id, &room, "2027-03-01", "2027-03-02", 1).await;
    assert_eq!(confirm(&sam, &booking.id).await.status(), 200);
    let draft = create_invoice(&ruth, &fx.project_id, &[&booking.id]).await;

    let send = |body: serde_json::Value| {
        ruth.request(
            reqwest::Method::POST,
            &format!("/api/v1/invoices/{}/issue", draft.id),
        )
        .header("Idempotency-Key", "issue-1")
        .json(&body)
        .send()
    };
    let first: InvoiceDto = ruth.json(send(json!({})).await.unwrap()).await;
    let replay = send(json!({})).await.unwrap();
    assert_eq!(replay.status(), 200);
    let replay: InvoiceDto = ruth.json(replay).await;
    assert_eq!(first.number, replay.number);
    let reused = send(json!({"due_date": "2027-12-31"})).await.unwrap();
    let (status, code, _) = error_of(reused).await;
    assert_eq!((status, code.as_str()), (422, "idempotency_key_reused"));
}

#[tokio::test]
async fn partial_payment_refund_then_cancel() {
    let fx = fixture().await;
    let (ruth, invoice) = issued_room_invoice(&fx).await;
    assert_eq!(invoice.total_cents, 57_000);
    assert_eq!(invoice.settlement, "unpaid");

    // Partial manual payment → pending until verified.
    let resp = ruth
        .post_json(
            &format!("/api/v1/invoices/{}/payments", invoice.id),
            &json!({"amount_cents": 20_000, "method": "manual"}),
        )
        .await;
    assert_eq!(resp.status(), 201);
    let payment: PaymentDto = ruth.json(resp).await;
    assert_eq!(payment.status, "pending_verification");
    assert_eq!(
        reload(&ruth, &fx.project_id, &invoice.id).await.settlement,
        "unpaid"
    );
    let resp = ruth
        .post(&format!("/api/v1/payments/{}/verify", payment.id))
        .await;
    assert_eq!(resp.status(), 200);
    let inv = reload(&ruth, &fx.project_id, &invoice.id).await;
    assert_eq!(inv.settlement, "partially_paid");
    assert_eq!(inv.net_verified_cents, 20_000);

    // Cancel is refused while receipts remain.
    let resp = ruth
        .post_json(
            &format!("/api/v1/invoices/{}/cancel", invoice.id),
            &json!({"reason": "trip shortened"}),
        )
        .await;
    assert_eq!(error_of(resp).await.0, 409);

    // Refund all receipts, then cancel.
    let resp = ruth
        .post_json(
            &format!("/api/v1/invoices/{}/refunds", invoice.id),
            &json!({"amount_cents": 20_000, "note": "returned"}),
        )
        .await;
    assert_eq!(resp.status(), 201);
    let inv = reload(&ruth, &fx.project_id, &invoice.id).await;
    assert_eq!(inv.net_verified_cents, 0);
    assert_eq!(inv.settlement, "unpaid");

    let resp = ruth
        .post_json(
            &format!("/api/v1/invoices/{}/cancel", invoice.id),
            &json!({"reason": "trip shortened"}),
        )
        .await;
    assert_eq!(resp.status(), 200);
    let cancelled: InvoiceDto = ruth.json(resp).await;
    assert_eq!(cancelled.status, "cancelled");
    assert_eq!(
        cancelled.cancelled_reason.as_deref(),
        Some("trip shortened")
    );

    // Cancelled is terminal: no further payments.
    let resp = ruth
        .post_json(
            &format!("/api/v1/invoices/{}/payments", invoice.id),
            &json!({"amount_cents": 100, "method": "manual"}),
        )
        .await;
    assert_eq!(error_of(resp).await.0, 409);
}

#[tokio::test]
async fn refund_larger_than_receipts_is_rejected() {
    let fx = fixture().await;
    let (ruth, invoice) = issued_room_invoice(&fx).await;
    let resp = ruth
        .post_json(
            &format!("/api/v1/invoices/{}/payments", invoice.id),
            &json!({"amount_cents": 10_000, "method": "bank_transfer"}),
        )
        .await;
    let payment: PaymentDto = ruth.json(resp).await;
    ruth.post(&format!("/api/v1/payments/{}/verify", payment.id))
        .await;

    let before = payment_count(&fx.app).await;
    let resp = ruth
        .post_json(
            &format!("/api/v1/invoices/{}/refunds", invoice.id),
            &json!({"amount_cents": 10_001}),
        )
        .await;
    let (status, code, _) = error_of(resp).await;
    assert_eq!((status, code.as_str()), (422, "refund_exceeds_receipts"));
    assert_eq!(payment_count(&fx.app).await, before);

    // Team members cannot record refunds.
    let resp = fx
        .anna
        .post_json(
            &format!("/api/v1/invoices/{}/refunds", invoice.id),
            &json!({"amount_cents": 1}),
        )
        .await;
    assert_eq!(resp.status(), 403);
}

#[tokio::test]
async fn duplicate_bank_notification_is_counted_once() {
    let fx = fixture().await;
    let (ruth, invoice) = issued_room_invoice(&fx).await;
    let body = serde_json::to_vec(&json!({
        "invoice_number": invoice.number,
        "amount_cents": 57_000,
        "external_ref": "BANK-REF-1",
    }))
    .unwrap();
    let sig = sign(&body);

    let first = bank_post(&fx.app, &body, &sig).await;
    assert_eq!(first.status(), 201);
    let first: BankNotificationResponse = ruth.json(first).await;
    assert!(!first.duplicate);

    let second = bank_post(&fx.app, &body, &sig).await;
    assert_eq!(second.status(), 200);
    let second: BankNotificationResponse = ruth.json(second).await;
    assert!(second.duplicate);
    assert_eq!(second.payment_id, first.payment_id);

    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM payments WHERE external_ref = ?")
        .bind("BANK-REF-1")
        .fetch_one(&fx.app.pool)
        .await
        .unwrap();
    assert_eq!(rows, 1);

    // Pending until finance verifies; then paid.
    let inv = reload(&ruth, &fx.project_id, &invoice.id).await;
    assert_eq!(inv.payments[0].status, "pending_verification");
    assert_eq!(inv.payments[0].method, "bank_transfer");
    let resp = ruth
        .post(&format!(
            "/api/v1/payments/{}/verify",
            first.payment_id.unwrap()
        ))
        .await;
    assert_eq!(resp.status(), 200);
    assert_eq!(
        reload(&ruth, &fx.project_id, &invoice.id).await.settlement,
        "paid"
    );
}

#[tokio::test]
async fn same_external_ref_with_different_payload_conflicts() {
    let fx = fixture().await;
    let (_ruth, invoice) = issued_room_invoice(&fx).await;
    let body = serde_json::to_vec(&json!({
        "invoice_number": invoice.number, "amount_cents": 1_000, "external_ref": "BANK-REF-2",
    }))
    .unwrap();
    assert_eq!(bank_post(&fx.app, &body, &sign(&body)).await.status(), 201);

    let other = serde_json::to_vec(&json!({
        "invoice_number": invoice.number, "amount_cents": 2_000, "external_ref": "BANK-REF-2",
    }))
    .unwrap();
    let (status, code, _) = error_of(bank_post(&fx.app, &other, &sign(&other)).await).await;
    assert_eq!((status, code.as_str()), (409, "external_ref_conflict"));
    assert_eq!(payment_count(&fx.app).await, 1);
}

#[tokio::test]
async fn bad_bank_signature_is_rejected_and_nothing_stored() {
    let fx = fixture().await;
    let (_ruth, invoice) = issued_room_invoice(&fx).await;
    let body = serde_json::to_vec(&json!({
        "invoice_number": invoice.number, "amount_cents": 1_000, "external_ref": "BANK-REF-3",
    }))
    .unwrap();
    let wrong = pitcairn::routes::money::bank_signature("not-the-secret", &body);
    assert_eq!(bank_post(&fx.app, &body, &wrong).await.status(), 401);
    assert_eq!(bank_post(&fx.app, &body, "zz-not-hex").await.status(), 401);
    // Signature over a different body.
    let tampered = sign(b"{}");
    assert_eq!(bank_post(&fx.app, &body, &tampered).await.status(), 401);
    assert_eq!(payment_count(&fx.app).await, 0);
}

#[tokio::test]
async fn demo_bank_notify_and_test_card_payment() {
    let fx = fixture().await;
    let (ruth, invoice) = issued_room_invoice(&fx).await;

    // Demo bank simulator (finance only) signs and goes through the webhook path.
    let req = json!({"invoice_id": invoice.id, "amount_cents": 7_000, "external_ref": "SIM-1"});
    assert_eq!(
        fx.anna
            .post_json("/api/v1/demo/bank/notify", &req)
            .await
            .status(),
        403
    );
    let resp = ruth.post_json("/api/v1/demo/bank/notify", &req).await;
    assert_eq!(resp.status(), 201);
    let resp = ruth.post_json("/api/v1/demo/bank/notify", &req).await;
    let dup: BankNotificationResponse = ruth.json(resp).await;
    assert!(dup.duplicate);

    // Test card: team editor+ pays the outstanding amount, TEST MODE.
    let tomasi = persona(&fx.app, "tomasi").await; // viewer
    assert_eq!(
        tomasi
            .post(&format!("/api/v1/invoices/{}/pay-test-card", invoice.id))
            .await
            .status(),
        403
    );
    let resp = fx
        .anna
        .post(&format!("/api/v1/invoices/{}/pay-test-card", invoice.id))
        .await;
    assert_eq!(resp.status(), 201);
    let paid: PayTestCardResponse = fx.anna.json(resp).await;
    assert!(!paid.duplicate);
    let payment = paid.payment.expect("payment recorded");
    assert_eq!(payment.method, "test_card");
    assert_eq!(payment.status, "verified");
    assert_eq!(payment.amount_cents, 57_000);
    assert!(payment.note.contains("TEST MODE"));
    assert_eq!(paid.invoice.settlement, "paid");

    // Idempotent: a second call records nothing new.
    let before = payment_count(&fx.app).await;
    let resp = fx
        .anna
        .post(&format!("/api/v1/invoices/{}/pay-test-card", invoice.id))
        .await;
    assert_eq!(resp.status(), 200);
    let again: PayTestCardResponse = fx.anna.json(resp).await;
    assert!(again.duplicate);
    assert_eq!(payment_count(&fx.app).await, before);

    // Workspace payload shows the invoice with its settlement.
    let project: serde_json::Value = fx
        .anna
        .json(
            fx.anna
                .get(&format!("/api/v1/projects/{}", fx.project_id))
                .await,
        )
        .await;
    assert_eq!(project["invoices"][0]["settlement"], "paid");
}

#[tokio::test]
async fn demo_bank_notify_is_hidden_outside_demo_mode() {
    let app = common::spawn_app(false).await;
    let anna = common::login(
        &app,
        "anna@demo.pitcairn.invalid",
        pitcairn::seed::DEMO_PASSWORD,
    )
    .await;
    let resp = anna
        .post_json(
            "/api/v1/demo/bank/notify",
            &json!({"invoice_id": "x", "amount_cents": 1, "external_ref": "y"}),
        )
        .await;
    assert_eq!(resp.status(), 404);
}
