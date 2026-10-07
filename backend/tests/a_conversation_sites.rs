mod common;

use common::a::*;
use common::{persona, spawn_app};
use serde_json::json;

#[tokio::test]
async fn internal_thread_invisible_to_team() {
    let app = spawn_app(true).await;
    let anna = persona(&app, "anna").await;
    let maria = persona(&app, "maria").await;
    let james = persona(&app, "james").await;
    let pid = in_review_project(&anna, &maria, "Internal threads").await;
    let (status, _) = post(
        &maria,
        &format!("/projects/{pid}/reviews"),
        json!({"expert_id": persona_id(&app, "james").await}),
    )
    .await;
    assert_eq!(status, 201);

    let (status, internal) = post(
        &maria,
        &format!("/projects/{pid}/threads"),
        json!({"anchor_type": "project", "visibility": "internal",
               "body": "STAFF ONLY: applicant had a permit breach in 2019"}),
    )
    .await;
    assert_eq!(status, 201, "{internal}");
    let internal_id = internal["id"].as_str().unwrap();
    let (status, _) = post(
        &maria,
        &format!("/projects/{pid}/threads"),
        json!({"anchor_type": "field", "anchor_key": "purpose", "visibility": "shared",
               "body": "Could you clarify the purpose?"}),
    )
    .await;
    assert_eq!(status, 201);

    let (_, team_view) = get(&anna, &format!("/projects/{pid}/threads")).await;
    assert_eq!(team_view["total"], 1);
    assert!(!team_view.to_string().contains("STAFF ONLY"));
    let (status, _) = post(
        &anna,
        &format!("/threads/{internal_id}/messages"),
        json!({"body": "hi"}),
    )
    .await;
    assert_eq!(status, 404, "internal thread hidden from the team");
    let (_, timeline) = get(&anna, &format!("/projects/{pid}/timeline")).await;
    assert!(
        timeline["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|e| e["visibility"] == "shared")
    );
    let (_, ws) = get(&anna, &format!("/projects/{pid}")).await;
    assert_eq!(ws["application"]["counts"]["threads"], 1);

    // Team cannot open internal threads; expert sees and writes internal only.
    let (status, _) = post(
        &anna,
        &format!("/projects/{pid}/threads"),
        json!({"anchor_type": "project", "visibility": "internal", "body": "x"}),
    )
    .await;
    assert_eq!(status, 403);
    let (_, expert_view) = get(&james, &format!("/projects/{pid}/threads")).await;
    assert_eq!(expert_view["total"], 2);
    let (status, _) = post(
        &james,
        &format!("/threads/{internal_id}/messages"),
        json!({"body": "Noted."}),
    )
    .await;
    assert_eq!(status, 201);

    // Anchors must belong to this project; only the coordinator raises action items.
    let (status, _) = post(&maria, &format!("/projects/{pid}/threads"),
        json!({"anchor_type": "field", "anchor_key": "no_such_field", "visibility": "shared", "body": "x"})).await;
    assert_eq!(status, 422);
    let (status, _) = post(
        &anna,
        &format!("/projects/{pid}/threads"),
        json!({"anchor_type": "project", "visibility": "shared", "body": "x",
               "action_item": {"addressed_to": "staff", "title": "Do it"}}),
    )
    .await;
    assert_eq!(status, 403);

    // The other side is notified of new messages.
    let anna_id = persona_id(&app, "anna").await;
    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM notifications WHERE user_id = ? AND kind = 'conversation_message'",
    )
    .bind(&anna_id)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(n, 1, "only the shared message notifies the team");
}

#[tokio::test]
async fn sensitive_site_generalized_for_unrelated_expert() {
    let app = spawn_app(true).await;
    let anna = persona(&app, "anna").await;
    let maria = persona(&app, "maria").await;
    let james = persona(&app, "james").await;
    let pid = in_review_project(&anna, &maria, "Nesting sites").await;

    // Not in the catalog yet: an unrelated expert gets nothing.
    let (status, _) = get(&james, &format!("/projects/{pid}/sites.geojson")).await;
    assert_eq!(status, 403);

    // Publish a deliverable (catalog-visible project) directly in the DB.
    let anna_id = persona_id(&app, "anna").await;
    let maria_id = persona_id(&app, "maria").await;
    let now = pitcairn::util::now_rfc3339();
    sqlx::query(
        "INSERT INTO deliverables (id, project_id, title, kind, due_date, sender_id, recipient_id, status,
             publish_level, published_at, published_by, created_by, created_at)
         VALUES (?, ?, 'Report', 'report', '2027-01-01', ?, ?, 'accepted', 'metadata', ?, ?, ?, ?)",
    )
    .bind(pitcairn::util::new_id())
    .bind(&pid)
    .bind(&anna_id)
    .bind(&maria_id)
    .bind(&now)
    .bind(&maria_id)
    .bind(&maria_id)
    .bind(&now)
    .execute(&app.pool)
    .await
    .unwrap();

    let (status, geo) = get(&james, &format!("/projects/{pid}/sites.geojson")).await;
    assert_eq!(status, 200, "{geo}");
    let feature = &geo["features"][0];
    assert_eq!(feature["properties"]["generalized"], true);
    assert_eq!(feature["geometry"]["type"], "Polygon");
    let text = geo.to_string();
    assert!(
        !text.contains("25.0661") && !text.contains("130.1043"),
        "{text}"
    );
    let (_, list) = get(&james, &format!("/projects/{pid}/sites")).await;
    assert_eq!(list["items"][0]["generalized"], true);
    assert_eq!(list["items"][0]["min_lat"], -25.1);
    // The unrelated expert still cannot open the project itself.
    let (status, _) = get(&james, &format!("/projects/{pid}")).await;
    assert_eq!(status, 403);

    // Team and staff see the precise point; so does the expert once assigned.
    let (_, precise) = get(&anna, &format!("/projects/{pid}/sites.geojson")).await;
    assert_eq!(precise["features"][0]["properties"]["generalized"], false);
    assert_eq!(
        precise["features"][0]["geometry"]["coordinates"],
        json!([-130.1043, -25.0661])
    );
    let (_, staff) = get(&maria, "/sites/search?bbox=-131,-26,-129,-24").await;
    assert_eq!(staff["items"][0]["site"]["generalized"], false);
    let (status, _) = get(&anna, "/sites/search?bbox=-131,-26,-129,-24").await;
    assert_eq!(status, 403);
    let (status, _) = post(
        &maria,
        &format!("/projects/{pid}/reviews"),
        json!({"expert_id": persona_id(&app, "james").await}),
    )
    .await;
    assert_eq!(status, 201);
    let (_, assigned) = get(&james, &format!("/projects/{pid}/sites.geojson")).await;
    assert_eq!(assigned["features"][0]["properties"]["generalized"], false);

    // Sites are part of the application: not editable once submitted.
    let site_id = list["items"][0]["id"].as_str().unwrap();
    let (status, err) = patch(&anna, &format!("/sites/{site_id}"), json!({"name": "x"})).await;
    assert_eq!(
        (status, err["error"]["code"].as_str()),
        (409, Some("not_editable"))
    );
}
