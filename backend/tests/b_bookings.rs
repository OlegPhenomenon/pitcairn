mod common;

use common::b::*;
use common::persona;
use pitcairn::dto::{BookingDto, CalendarResponse, ListResponse, ProviderBookingDto, TripDto};
use serde_json::json;

#[tokio::test]
async fn concurrent_confirmations_of_single_unit_resource_one_wins() {
    let fx = fixture().await;
    let lab = resource_id(&fx.app, LAB).await;
    let trip = create_trip(&fx.anna, &fx.project_id, "2027-03-01", "2027-03-10").await;
    let a = request_booking(&fx.anna, &trip.id, &lab, "2027-03-02", "2027-03-05", 1).await;
    let b = request_booking(&fx.anna, &trip.id, &lab, "2027-03-04", "2027-03-07", 1).await;

    // Two independent base-manager sessions against the file-backed DB.
    let sam1 = persona(&fx.app, "sam").await;
    let sam2 = persona(&fx.app, "sam").await;
    let (r1, r2) = tokio::join!(confirm(&sam1, &a.id), confirm(&sam2, &b.id));
    let mut statuses = [r1.status().as_u16(), r2.status().as_u16()];
    statuses.sort();
    assert_eq!(statuses, [200, 409], "exactly one confirmation succeeds");

    let loser = if r1.status() == 409 { r1 } else { r2 };
    let (_, code, message) = error_of(loser).await;
    assert_eq!(code, "capacity_conflict");
    assert!(
        message.contains("2027-03-04"),
        "names first conflicting day: {message}"
    );

    let confirmed: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM bookings WHERE resource_id = ? AND status = 'confirmed'",
    )
    .bind(&lab)
    .fetch_one(&fx.app.pool)
    .await
    .unwrap();
    assert_eq!(confirmed, 1);
}

#[tokio::test]
async fn peak_capacity_allows_non_simultaneous_overlaps() {
    let fx = fixture().await;
    let sam = persona(&fx.app, "sam").await;
    let tanks = resource_id(&fx.app, TANKS).await; // quantity 6
    let trip = create_trip(&fx.anna, &fx.project_id, "2027-04-01", "2027-04-20").await;

    // A [1,4) and C [4,8) never overlap each other; B [3,6) overlaps both.
    // The sum over everything overlapping B is 9 > 6, but the peak on any
    // single day is 6, so all three fit.
    let a = request_booking(&fx.anna, &trip.id, &tanks, "2027-04-01", "2027-04-04", 3).await;
    let c = request_booking(&fx.anna, &trip.id, &tanks, "2027-04-04", "2027-04-08", 3).await;
    let b = request_booking(&fx.anna, &trip.id, &tanks, "2027-04-03", "2027-04-06", 3).await;
    for id in [&a.id, &c.id, &b.id] {
        assert_eq!(confirm(&sam, id).await.status(), 200);
    }

    // One more set on 2027-04-05 exceeds the peak there (B + C = 6).
    let d = request_booking(&fx.anna, &trip.id, &tanks, "2027-04-05", "2027-04-07", 1).await;
    let (status, code, message) = error_of(confirm(&sam, &d.id).await).await;
    assert_eq!((status, code.as_str()), (409, "capacity_conflict"));
    assert!(message.contains("2027-04-05"), "{message}");

    // Half-open intervals: a booking starting on C's end day fits.
    let e = request_booking(&fx.anna, &trip.id, &tanks, "2027-04-08", "2027-04-10", 6).await;
    assert_eq!(confirm(&sam, &e.id).await.status(), 200);

    let cal: CalendarResponse = sam
        .json(
            sam.get(&format!(
                "/api/v1/calendar?from=2027-04-03&to=2027-04-08&resource_id={tanks}"
            ))
            .await,
        )
        .await;
    let used: Vec<i64> = cal.resources[0].days.iter().map(|d| d.used).collect();
    assert_eq!(used, vec![6, 6, 6, 3, 3, 6]);
    assert_eq!(cal.resources[0].capacity, 6);
}

#[tokio::test]
async fn provider_confirms_only_own_boat_and_base_manager_cannot() {
    let fx = fixture().await;
    let sam = persona(&fx.app, "sam").await;
    let david = persona(&fx.app, "david").await;
    let admin = persona(&fx.app, "admin").await;
    let boat = resource_id(&fx.app, BOAT).await;
    let room = resource_id(&fx.app, ROOM).await;
    // A boat owned by a different provider.
    let lukas_id = user_id(&fx.app, "lukas").await;
    let other_boat =
        admin_create_resource(&admin, "boat", "Other boat (test)", 1, Some(&lukas_id)).await;

    let trip = create_trip(&fx.anna, &fx.project_id, "2027-05-01", "2027-05-10").await;
    let boat_booking =
        request_booking(&fx.anna, &trip.id, &boat, "2027-05-02", "2027-05-04", 1).await;
    let other_booking = request_booking(
        &fx.anna,
        &trip.id,
        &other_boat.id,
        "2027-05-02",
        "2027-05-04",
        1,
    )
    .await;
    let room_booking =
        request_booking(&fx.anna, &trip.id, &room, "2027-05-01", "2027-05-10", 1).await;

    // Base manager cannot confirm the provider's boat.
    assert_eq!(confirm(&sam, &boat_booking.id).await.status(), 403);
    // Provider cannot confirm someone else's boat nor a base resource.
    assert_eq!(confirm(&david, &other_booking.id).await.status(), 403);
    assert_eq!(confirm(&david, &room_booking.id).await.status(), 403);
    // Provider confirms their own boat.
    let resp = confirm(&david, &boat_booking.id).await;
    assert_eq!(resp.status(), 200);
    let confirmed: BookingDto = david.json(resp).await;
    assert_eq!(confirmed.status, "confirmed");

    // Provider view: only own resources, minimal project info.
    let resp = david.get("/api/v1/provider/bookings").await;
    assert_eq!(resp.status(), 200);
    let raw: serde_json::Value = david.json(resp).await;
    let list: ListResponse<ProviderBookingDto> = serde_json::from_value(raw.clone()).unwrap();
    assert_eq!(list.total, 1);
    assert_eq!(list.items[0].booking_id, boat_booking.id);
    assert_eq!(list.items[0].lead_name, "Dr Anna Hart");
    assert_eq!(list.items[0].team_size, 4);
    let text = raw.to_string();
    assert!(!text.contains("document"), "no documents in provider view");

    // The team was notified about the confirmation.
    let notified: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM notifications WHERE kind = 'booking.confirmed' AND project_id = ?",
    )
    .bind(&fx.project_id)
    .fetch_one(&fx.app.pool)
    .await
    .unwrap();
    assert!(notified >= 1);

    // Non-providers are refused the provider view; the team cannot use the calendar.
    assert_eq!(sam.get("/api/v1/provider/bookings").await.status(), 403);
    assert_eq!(
        fx.anna
            .get("/api/v1/calendar?from=2027-05-01&to=2027-05-02")
            .await
            .status(),
        403
    );
}

#[tokio::test]
async fn trip_date_patch_with_confirmed_booking_needs_change_request() {
    let fx = fixture().await;
    let sam = persona(&fx.app, "sam").await;
    let room = resource_id(&fx.app, ROOM).await;
    let trip = create_trip(&fx.anna, &fx.project_id, "2027-06-01", "2027-06-10").await;

    // Without confirmed bookings the dates may move.
    let resp = fx
        .anna
        .patch_json(
            &format!("/api/v1/trips/{}", trip.id),
            &json!({"depart_date": "2027-06-12"}),
        )
        .await;
    assert_eq!(resp.status(), 200);
    let moved: TripDto = fx.anna.json(resp).await;
    assert_eq!(moved.depart_date, "2027-06-12");

    let booking = request_booking(&fx.anna, &trip.id, &room, "2027-06-01", "2027-06-05", 1).await;
    assert_eq!(confirm(&sam, &booking.id).await.status(), 200);

    let resp = fx
        .anna
        .patch_json(
            &format!("/api/v1/trips/{}", trip.id),
            &json!({"arrive_date": "2027-06-02"}),
        )
        .await;
    let (status, code, _) = error_of(resp).await;
    assert_eq!((status, code.as_str()), (409, "use_change_request"));

    // Title-only edits are still fine.
    let resp = fx
        .anna
        .patch_json(
            &format!("/api/v1/trips/{}", trip.id),
            &json!({"title": "Reef survey (main)"}),
        )
        .await;
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn cancel_trip_releases_bookings_and_frees_capacity() {
    let fx = fixture().await;
    let sam = persona(&fx.app, "sam").await;
    let lab = resource_id(&fx.app, LAB).await;

    let trip1 = create_trip(&fx.anna, &fx.project_id, "2027-07-01", "2027-07-10").await;
    let first = request_booking(&fx.anna, &trip1.id, &lab, "2027-07-02", "2027-07-05", 1).await;
    assert_eq!(confirm(&sam, &first.id).await.status(), 200);

    let trip2 = create_trip(&fx.anna, &fx.project_id, "2027-07-01", "2027-07-10").await;
    let second = request_booking(&fx.anna, &trip2.id, &lab, "2027-07-03", "2027-07-04", 1).await;
    let (status, code, _) = error_of(confirm(&sam, &second.id).await).await;
    assert_eq!((status, code.as_str()), (409, "capacity_conflict"));

    let resp = fx
        .anna
        .post(&format!("/api/v1/trips/{}/cancel", trip1.id))
        .await;
    assert_eq!(resp.status(), 200);
    let cancelled: TripDto = fx.anna.json(resp).await;
    assert_eq!(cancelled.status, "cancelled");
    assert!(cancelled.bookings.iter().all(|b| b.status == "released"));

    // Capacity is free again.
    assert_eq!(confirm(&sam, &second.id).await.status(), 200);

    // Workspace payload carries the trips section.
    let project: serde_json::Value = fx
        .anna
        .json(
            fx.anna
                .get(&format!("/api/v1/projects/{}", fx.project_id))
                .await,
        )
        .await;
    assert_eq!(project["trips"].as_array().map(Vec::len), Some(2));
}

#[tokio::test]
async fn bookings_confirm_only_on_approved_projects_and_within_trip_dates() {
    let fx = fixture_with_status("in_review").await;
    let sam = persona(&fx.app, "sam").await;
    let room = resource_id(&fx.app, ROOM).await;
    // Planning may start while in review…
    let trip = create_trip(&fx.anna, &fx.project_id, "2027-08-01", "2027-08-10").await;
    let booking = request_booking(&fx.anna, &trip.id, &room, "2027-08-01", "2027-08-03", 1).await;
    // …but confirmation waits for approval.
    let (status, code, _) = error_of(confirm(&sam, &booking.id).await).await;
    assert_eq!((status, code.as_str()), (409, "project_not_approved"));

    // Booking outside the trip dates is a field error.
    let resp = fx
        .anna
        .post_json(
            &format!("/api/v1/trips/{}/bookings", trip.id),
            &json!({"resource_id": room, "start_date": "2027-07-30", "end_date": "2027-08-02", "quantity": 1}),
        )
        .await;
    assert_eq!(resp.status(), 422);

    // A viewer (tomasi) may not plan trips.
    let tomasi = persona(&fx.app, "tomasi").await;
    let resp = tomasi
        .post_json(
            &format!("/api/v1/projects/{}/trips", fx.project_id),
            &json!({"title": "x", "arrive_date": "2027-08-01", "depart_date": "2027-08-02"}),
        )
        .await;
    assert_eq!(resp.status(), 403);
    // Another team's researcher cannot see the trips.
    let lukas = persona(&fx.app, "lukas").await;
    let resp = lukas
        .get(&format!("/api/v1/projects/{}/trips", fx.project_id))
        .await;
    assert_eq!(resp.status(), 403);
}

#[tokio::test]
async fn researcher_can_read_resource_catalog_and_tariffs_for_booking_picker() {
    let fx = fixture().await;
    let resources = fx.anna.get("/api/v1/resources").await;
    assert_eq!(resources.status(), 200);
    let room = resource_id(&fx.app, ROOM).await;
    let tariffs = fx
        .anna
        .get(&format!("/api/v1/resources/{room}/tariffs"))
        .await;
    assert_eq!(tariffs.status(), 200);
    let anonymous = common::Client::anonymous(&fx.app);
    assert_eq!(anonymous.get("/api/v1/resources").await.status(), 401);
    assert_eq!(fx.anna.post_json("/api/v1/resources", &json!({"kind":"room","name":"No","quantity":1,"description":null,"unit_label":null,"provider_user_id":null,"active":true})).await.status(), 403);
}
