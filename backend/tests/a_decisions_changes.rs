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

/// Spec §5 item 5 / §6 "Проверка изменений": one study holds several
/// independent permits. Extending A replaces only A's current decision;
/// B keeps its number, dates, conditions and state. Revoking B leaves A alone.
#[tokio::test]
async fn independent_permits_change_separately() {
    let app = spawn_app(true).await;
    let anna = persona(&app, "anna").await;
    let maria = persona(&app, "maria").await;
    let helen = persona(&app, "helen").await;
    let (pid, a) = approved_project(&app, &anna, &maria, &helen, "Two permits").await;
    let a_id = a["id"].as_str().unwrap().to_string();
    assert_eq!(
        a["chain_id"],
        a_id.as_str(),
        "a permit starts its own chain"
    );
    let revision = latest_revision_id(&app, &pid).await;

    // B: a second permit on the already approved project, no supersession.
    let b = issue_decision(
        &helen,
        &pid,
        json!({"kind": "permit", "title": "Coral tissue sampling", "project_revision_id": revision,
               "basis": "Separate activity", "valid_from": "2026-11-05", "valid_to": "2026-11-20",
               "permitted_activities": ["Collect 10 coral fragments"],
               "conditions": ["One fragment per colony"]}),
    )
    .await;
    let b_id = b["id"].as_str().unwrap().to_string();
    let (_, ws) = get(&anna, &format!("/projects/{pid}")).await;
    assert_eq!(ws["project"]["status"], "approved");

    // Anna asks to extend A only; the request must name the permit.
    let (status, err) = post(
        &anna,
        &format!("/projects/{pid}/change-requests"),
        json!({"kind": "extend_permit", "description": "Longer season",
               "payload": {"new_valid_to": "2027-01-31"}}),
    )
    .await;
    assert_eq!(status, 422, "{err}");
    assert!(err["error"]["fields"]["payload.decision_id"].is_string());
    let (status, cr) = post(
        &anna,
        &format!("/projects/{pid}/change-requests"),
        json!({"kind": "extend_permit", "description": "Longer season",
               "payload": {"decision_id": a_id, "new_valid_to": "2027-01-31"}}),
    )
    .await;
    assert_eq!(status, 201, "{cr}");
    let cr_id = cr["id"].as_str().unwrap();
    let (_, impact) = get(&maria, &format!("/change-requests/{cr_id}/impact")).await;
    let affected: Vec<&str> = impact["decisions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["id"].as_str().unwrap())
        .collect();
    assert_eq!(affected, vec![a_id.as_str()], "only A is affected");

    // An extension without naming A is refused; it may not touch B either.
    let (_, d) = post(
        &helen,
        &format!("/projects/{pid}/decisions"),
        json!({"kind": "extension", "project_revision_id": revision, "basis": "x",
               "valid_from": "2026-11-01", "valid_to": "2027-01-31"}),
    )
    .await;
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
    let a2 = issue_decision(
        &helen,
        &pid,
        json!({"kind": "extension", "project_revision_id": revision, "basis": "Season extended",
               "valid_from": "2026-11-01", "valid_to": "2027-01-31", "supersedes_id": a_id,
               "change_request_id": cr_id}),
    )
    .await;
    let a2_id = a2["id"].as_str().unwrap().to_string();
    assert_eq!(a2["chain_id"], a_id.as_str());

    // Approving the request with B's decision is rejected: it is not A's chain.
    let (status, _) = post(
        &maria,
        &format!("/change-requests/{cr_id}/approve"),
        json!({"resulting_decision_id": b_id}),
    )
    .await;
    assert_eq!(status, 422);
    let (status, body) = post(
        &maria,
        &format!("/change-requests/{cr_id}/approve"),
        json!({"resulting_decision_id": a2_id}),
    )
    .await;
    assert_eq!(status, 200, "{body}");

    let in_force = |list: &serde_json::Value| -> Vec<String> {
        list["items"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|d| d["status"] == "issued" && d["superseded_by_id"].is_null())
            .map(|d| d["id"].as_str().unwrap().to_string())
            .collect()
    };
    let find = |list: &serde_json::Value, id: &str| -> serde_json::Value {
        list["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|d| d["id"] == id)
            .unwrap()
            .clone()
    };
    let (_, list) = get(&anna, &format!("/projects/{pid}/decisions")).await;
    let mut current = in_force(&list);
    current.sort();
    let mut expected = vec![a2_id.clone(), b_id.clone()];
    expected.sort();
    assert_eq!(current, expected, "A2 and B are both in force");
    assert_eq!(
        find(&list, &a_id)["superseded_by_id"],
        a2_id.as_str(),
        "A kept in history"
    );
    assert_eq!(find(&list, &b_id), b, "B is untouched");

    // Revoke only B: A2 stays in force.
    let revocation = issue_decision(
        &helen,
        &pid,
        json!({"kind": "revocation", "project_revision_id": revision,
               "basis": "Sampling no longer needed", "supersedes_id": b_id}),
    )
    .await;
    assert_eq!(revocation["chain_id"], b_id.as_str());
    assert_eq!(
        revocation["title"], "Coral tissue sampling",
        "title inherited"
    );
    let (_, list) = get(&anna, &format!("/projects/{pid}/decisions")).await;
    assert!(
        find(&list, &a2_id)["superseded_by_id"].is_null(),
        "A2 still in force"
    );

    // A revoked permit cannot be amended back to life.
    let (_, d) = post(
        &helen,
        &format!("/projects/{pid}/decisions"),
        json!({"kind": "amendment", "project_revision_id": revision, "basis": "x",
               "valid_from": "2026-11-01", "valid_to": "2026-12-01",
               "supersedes_id": revocation["id"]}),
    )
    .await;
    let (status, err) = post(
        &helen,
        &format!("/decisions/{}/issue", d["id"].as_str().unwrap()),
        json!({}),
    )
    .await;
    assert_eq!(
        (status, err["error"]["code"].as_str()),
        (409, Some("invalid_supersession"))
    );
}

/// An extension request is implemented only by the extension or amendment of
/// the named permit that is now in force — never by its revocation.
#[tokio::test]
async fn extension_request_cannot_be_approved_with_a_revocation() {
    let app = spawn_app(true).await;
    let anna = persona(&app, "anna").await;
    let maria = persona(&app, "maria").await;
    let helen = persona(&app, "helen").await;
    let (pid, a) = approved_project(&app, &anna, &maria, &helen, "Revoked instead").await;
    let a_id = a["id"].as_str().unwrap();
    let revision = latest_revision_id(&app, &pid).await;
    let (status, cr) = post(
        &anna,
        &format!("/projects/{pid}/change-requests"),
        json!({"kind": "extend_permit", "description": "Longer season",
               "payload": {"decision_id": a_id, "new_valid_to": "2027-01-31"}}),
    )
    .await;
    assert_eq!(status, 201, "{cr}");
    let revocation = issue_decision(
        &helen,
        &pid,
        json!({"kind": "revocation", "project_revision_id": revision,
               "basis": "Season cancelled", "supersedes_id": a_id}),
    )
    .await;
    let (status, err) = post(
        &maria,
        &format!("/change-requests/{}/approve", cr["id"].as_str().unwrap()),
        json!({"resulting_decision_id": revocation["id"]}),
    )
    .await;
    assert_eq!(status, 422, "{err}");
    assert!(err["error"]["fields"]["resulting_decision_id"].is_string());
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

    // Everything below goes through the API: trips, a single-unit lab booked
    // and confirmed by the base manager, and a deliverable agreed by both sides.
    let sam = persona(&app, "sam").await;
    let anna_id = persona_id(&app, "anna").await;
    let maria_id = persona_id(&app, "maria").await;
    // Single-unit resource the demo seed never books.
    let lab = common::b::resource_id(&app, "Underwater camera housing").await;
    let trip = common::b::create_trip(&anna, &pid, "2026-11-05", "2026-11-15")
        .await
        .id;
    let other_trip = common::b::create_trip(&lukas, &other_pid, "2026-12-03", "2026-12-05")
        .await
        .id;
    let booking = common::b::request_booking(&anna, &trip, &lab, "2026-11-05", "2026-11-15", 1)
        .await
        .id;
    let other_booking =
        common::b::request_booking(&lukas, &other_trip, &lab, "2026-12-03", "2026-12-05", 1)
            .await
            .id;
    for b in [&booking, &other_booking] {
        let resp = common::b::confirm(&sam, b).await;
        assert_eq!(resp.status(), 200, "base manager confirms the lab booking");
    }
    let deliverable =
        common::c::create_deliverable(&maria, &pid, "report", "2026-12-05", &anna_id, &maria_id)
            .await
            .id;
    let resp = anna
        .post(&format!("/api/v1/deliverables/{deliverable}/agree"))
        .await;
    assert_eq!(resp.status(), 200, "team agrees the deliverable");

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
            lab.clone(),
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
