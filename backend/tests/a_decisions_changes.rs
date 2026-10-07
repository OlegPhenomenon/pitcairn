mod common;

use common::a::*;
use common::{persona, spawn_app};
use serde_json::json;

#[tokio::test]
async fn admin_cannot_issue_decision_maker_can() {
    let app = spawn_app(true).await;
    let anna = persona(&app, "anna").await;
    let maria = persona(&app, "maria").await;
    let admin = persona(&app, "admin").await;
    let helen = persona(&app, "helen").await;
    let pid = in_review_project(&anna, &maria, "Separation of duties").await;
    let revision = latest_revision_id(&app, &pid).await;

    let draft = json!({"kind": "permit", "project_revision_id": revision, "basis": "OK",
                       "valid_from": "2026-11-01", "valid_to": "2026-11-30"});
    let (status, _) = post(&admin, &format!("/projects/{pid}/decisions"), draft.clone()).await;
    assert_eq!(status, 403, "admin cannot even draft");
    let (status, _) = post(&anna, &format!("/projects/{pid}/decisions"), draft.clone()).await;
    assert_eq!(status, 403);
    // Coordinator drafts; drafts are invisible to the team.
    let (status, d) = post(&maria, &format!("/projects/{pid}/decisions"), draft).await;
    assert_eq!(status, 201, "{d}");
    let id = d["id"].as_str().unwrap();
    let (_, team_list) = get(&anna, &format!("/projects/{pid}/decisions")).await;
    assert_eq!(team_list["total"], 0);

    for who in [&admin, &maria, &anna] {
        let (status, _) = post(who, &format!("/decisions/{id}/issue"), json!({})).await;
        assert_eq!(status, 403);
    }
    let (status, issued) = post(&helen, &format!("/decisions/{id}/issue"), json!({})).await;
    assert_eq!(status, 200, "{issued}");
    let (status, err) = patch(
        &helen,
        &format!("/decisions/{id}"),
        json!({"basis": "changed"}),
    )
    .await;
    assert_eq!(
        (status, err["error"]["code"].as_str()),
        (409, Some("decision_issued"))
    );
    let (_, ws) = get(&anna, &format!("/projects/{pid}")).await;
    assert_eq!(ws["project"]["status"], "approved");
}

#[tokio::test]
async fn amendment_supersedes_old_decision_both_visible() {
    let app = spawn_app(true).await;
    let anna = persona(&app, "anna").await;
    let maria = persona(&app, "maria").await;
    let helen = persona(&app, "helen").await;
    let (pid, permit) = approved_project(&app, &anna, &maria, &helen, "Amend me").await;
    let permit_id = permit["id"].as_str().unwrap();
    let revision = latest_revision_id(&app, &pid).await;

    // An amendment must supersede the permit in force.
    let (status, d) = post(
        &helen,
        &format!("/projects/{pid}/decisions"),
        json!({"kind": "amendment", "project_revision_id": revision, "basis": "Extra site",
               "valid_from": "2026-11-01", "valid_to": "2026-12-15"}),
    )
    .await;
    assert_eq!(status, 201);
    let (status, err) = post(
        &helen,
        &format!("/decisions/{}/issue", d["id"].as_str().unwrap()),
        json!({}),
    )
    .await;
    assert_eq!(
        (status, err["error"]["code"].as_str()),
        (409, Some("supersedes_required"))
    );

    let amendment = issue_decision(
        &helen,
        &pid,
        json!({
            "kind": "amendment", "project_revision_id": revision, "basis": "Extended field season",
            "valid_from": "2026-11-01", "valid_to": "2026-12-15", "supersedes_id": permit_id,
            "conditions": ["No anchoring on live coral", "Daily dive log"],
        }),
    )
    .await;
    let amendment_id = amendment["id"].as_str().unwrap();
    assert_eq!(amendment["supersedes_id"], permit_id);

    let (_, list) = get(&anna, &format!("/projects/{pid}/decisions")).await;
    assert_eq!(list["total"], 2, "both decisions remain visible");
    let old = list["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["id"] == permit_id)
        .unwrap();
    assert_eq!(old["superseded_by_id"], amendment_id);
    assert_eq!(old["status"], "issued");
    let html = anna
        .get(&format!("/api/v1/decisions/{permit_id}/document"))
        .await
        .text()
        .await
        .unwrap();
    assert!(html.contains("superseded"));

    // The superseded permit cannot be superseded a second time.
    let (status, d) = post(
        &helen,
        &format!("/projects/{pid}/decisions"),
        json!({"kind": "extension", "project_revision_id": revision, "basis": "x",
               "valid_from": "2026-11-01", "valid_to": "2027-01-15", "supersedes_id": permit_id}),
    )
    .await;
    assert_eq!(status, 201);
    let (status, _) = post(
        &helen,
        &format!("/decisions/{}/issue", d["id"].as_str().unwrap()),
        json!({}),
    )
    .await;
    assert_eq!(status, 409);
}

#[tokio::test]
async fn reschedule_impact_and_approval_release_bookings_only() {
    let app = spawn_app(true).await;
    let anna = persona(&app, "anna").await;
    let lukas = persona(&app, "lukas").await;
    let maria = persona(&app, "maria").await;
    let helen = persona(&app, "helen").await;
    let (pid, permit) = approved_project(&app, &anna, &maria, &helen, "Reschedule me").await;
    let (other_pid, _) = approved_project(&app, &lukas, &maria, &helen, "Other team").await;

    // Trips/resources/bookings/deliverables are slice B/C tables: insert directly.
    let anna_id = persona_id(&app, "anna").await;
    let lukas_id = persona_id(&app, "lukas").await;
    let maria_id = persona_id(&app, "maria").await;
    let now = pitcairn::util::now_rfc3339();
    let room = pitcairn::util::new_id();
    sqlx::query("INSERT INTO resources (id, kind, name, quantity, created_at) VALUES (?, 'room', 'Lab bunk room', 1, ?)")
        .bind(&room).bind(&now).execute(&app.pool).await.unwrap();
    let trip = pitcairn::util::new_id();
    let other_trip = pitcairn::util::new_id();
    for (id, project, arrive, depart) in [
        (&trip, &pid, "2026-11-05", "2026-11-15"),
        (&other_trip, &other_pid, "2026-12-03", "2026-12-05"),
    ] {
        sqlx::query("INSERT INTO trips (id, project_id, title, arrive_date, depart_date, status, created_at)
                     VALUES (?, ?, 'Field trip', ?, ?, 'confirmed', ?)")
            .bind(id).bind(project).bind(arrive).bind(depart).bind(&now)
            .execute(&app.pool).await.unwrap();
    }
    let booking = pitcairn::util::new_id();
    let other_booking = pitcairn::util::new_id();
    for (id, trip_id, start, end, by) in [
        (&booking, &trip, "2026-11-05", "2026-11-15", &anna_id),
        (
            &other_booking,
            &other_trip,
            "2026-12-03",
            "2026-12-05",
            &lukas_id,
        ),
    ] {
        sqlx::query("INSERT INTO bookings (id, trip_id, resource_id, start_date, end_date, quantity, status,
                         requested_by, decided_by, decided_at, created_at)
                     VALUES (?, ?, ?, ?, ?, 1, 'confirmed', ?, ?, ?, ?)")
            .bind(id).bind(trip_id).bind(&room).bind(start).bind(end).bind(by)
            .bind(&maria_id).bind(&now).bind(&now)
            .execute(&app.pool).await.unwrap();
    }
    let deliverable = pitcairn::util::new_id();
    sqlx::query(
        "INSERT INTO deliverables (id, project_id, title, kind, due_date, sender_id, recipient_id,
                     status, created_by, created_at)
                 VALUES (?, ?, 'Cruise report', 'report', '2026-12-05', ?, ?, 'agreed', ?, ?)",
    )
    .bind(&deliverable)
    .bind(&pid)
    .bind(&anna_id)
    .bind(&maria_id)
    .bind(&maria_id)
    .bind(&now)
    .execute(&app.pool)
    .await
    .unwrap();

    // Another project's trip cannot be referenced.
    let (status, _) = post(&anna, &format!("/projects/{pid}/change-requests"), json!({
        "kind": "reschedule_trip", "description": "x",
        "payload": {"trip_id": other_trip, "new_arrive_date": "2026-12-01", "new_depart_date": "2026-12-10"}
    })).await;
    assert_eq!(status, 403);
    let (status, cr) = post(&anna, &format!("/projects/{pid}/change-requests"), json!({
        "kind": "reschedule_trip", "description": "Boat engine failure; move trip by four weeks",
        "payload": {"trip_id": trip, "new_arrive_date": "2026-12-01", "new_depart_date": "2026-12-10"}
    })).await;
    assert_eq!(status, 201, "{cr}");
    let cr_id = cr["id"].as_str().unwrap();

    let (status, impact) = get(&anna, &format!("/change-requests/{cr_id}/impact")).await;
    assert_eq!(status, 200, "{impact}");
    let kinds: Vec<(String, String)> = impact["bookings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| {
            (
                b["booking_id"].as_str().unwrap().into(),
                b["kind"].as_str().unwrap().into(),
            )
        })
        .collect();
    assert!(kinds.contains(&(booking.clone(), "trip".into())));
    assert!(
        kinds.contains(&(other_booking.clone(), "conflict".into())),
        "{impact}"
    );
    assert_eq!(impact["deliverables"][0]["id"], deliverable.as_str());
    assert_eq!(impact["decisions"][0]["id"], permit["id"]);
    assert_eq!(impact["requires_new_decision"], true);

    // Team cannot approve; coordinator can.
    let (status, _) = post(
        &anna,
        &format!("/change-requests/{cr_id}/approve"),
        json!({}),
    )
    .await;
    assert_eq!(status, 403);
    let (status, approved) = post(
        &maria,
        &format!("/change-requests/{cr_id}/approve"),
        json!({"note": "Approved; bookings re-requested"}),
    )
    .await;
    assert_eq!(status, 200, "{approved}");
    assert_eq!(approved["status"], "approved");

    let (arrive, depart): (String, String) =
        sqlx::query_as("SELECT arrive_date, depart_date FROM trips WHERE id = ?")
            .bind(&trip)
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(
        (arrive.as_str(), depart.as_str()),
        ("2026-12-01", "2026-12-10")
    );
    let old_status: String = sqlx::query_scalar("SELECT status FROM bookings WHERE id = ?")
        .bind(&booking)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(old_status, "released");
    let new: Vec<(String, String, String, String)> = sqlx::query_as(
        "SELECT resource_id, start_date, end_date, status FROM bookings WHERE trip_id = ? AND id != ?")
        .bind(&trip).bind(&booking).fetch_all(&app.pool).await.unwrap();
    assert_eq!(
        new,
        vec![(
            room.clone(),
            "2026-12-01".into(),
            "2026-12-10".into(),
            "requested".into()
        )]
    );
    let other_status: String = sqlx::query_scalar("SELECT status FROM bookings WHERE id = ?")
        .bind(&other_booking)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(
        other_status, "confirmed",
        "other project's booking untouched"
    );

    // Decisions and deliverables are NOT altered silently.
    let (valid_to, superseded): (String, Option<String>) =
        sqlx::query_as("SELECT valid_to, superseded_by_id FROM decisions WHERE id = ?")
            .bind(permit["id"].as_str().unwrap())
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!((valid_to.as_str(), superseded), ("2026-11-30", None));
    let due: String = sqlx::query_scalar("SELECT due_date FROM deliverables WHERE id = ?")
        .bind(&deliverable)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(due, "2026-12-05");
    // ...and still show up as needing action.
    let (_, impact) = get(&maria, &format!("/change-requests/{cr_id}/impact")).await;
    assert_eq!(impact["decisions"].as_array().unwrap().len(), 1);
    assert_eq!(impact["deliverables"].as_array().unwrap().len(), 1);

    let (status, err) = post(
        &maria,
        &format!("/change-requests/{cr_id}/reject"),
        json!({}),
    )
    .await;
    assert_eq!(
        (status, err["error"]["code"].as_str()),
        (409, Some("invalid_state"))
    );

    // expand_scope needs a resulting decision before approval.
    let (_, scope) = post(
        &anna,
        &format!("/projects/{pid}/change-requests"),
        json!({"kind": "expand_scope", "description": "Add sediment cores", "payload": {}}),
    )
    .await;
    let scope_id = scope["id"].as_str().unwrap();
    let (_, impact) = get(&maria, &format!("/change-requests/{scope_id}/impact")).await;
    assert_eq!(impact["requires_new_decision"], true);
    let (status, _) = post(
        &maria,
        &format!("/change-requests/{scope_id}/approve"),
        json!({}),
    )
    .await;
    assert_eq!(status, 422);
    let (status, w) = post(
        &anna,
        &format!("/change-requests/{scope_id}/withdraw"),
        json!({}),
    )
    .await;
    assert_eq!((status, w["status"].as_str()), (200, Some("withdrawn")));
}
