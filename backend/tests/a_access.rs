mod common;

use common::a::*;
use common::{persona, spawn_app};
use serde_json::json;

#[tokio::test]
async fn expert_of_another_project_gets_403() {
    let app = spawn_app(true).await;
    let anna = persona(&app, "anna").await;
    let lukas = persona(&app, "lukas").await;
    let maria = persona(&app, "maria").await;
    let james = persona(&app, "james").await;
    let james_id = persona_id(&app, "james").await;

    let assigned = in_review_project(&anna, &maria, "Assigned to James").await;
    let other = in_review_project(&lukas, &maria, "Not assigned").await;
    let (status, review) = post(
        &maria,
        &format!("/projects/{assigned}/reviews"),
        json!({"expert_id": james_id}),
    )
    .await;
    assert_eq!(status, 201, "{review}");

    let (status, _) = get(&james, &format!("/projects/{assigned}")).await;
    assert_eq!(status, 200);
    for path in [
        format!("/projects/{other}"),
        format!("/projects/{other}/threads"),
        format!("/projects/{other}/revisions"),
        format!("/projects/{other}/decisions"),
        format!("/projects/{other}/members"),
    ] {
        let (status, _) = get(&james, &path).await;
        assert_eq!(status, 403, "{path}");
    }
    let (_, list) = get(&james, "/projects").await;
    let ids: Vec<&str> = list["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, vec![assigned.as_str()]);
    let (_, reviews) = get(&james, "/reviews").await;
    assert_eq!(reviews["total"], 1);

    // Another expert's review cannot be acted on; declining ends access.
    let review_id = review["id"].as_str().unwrap();
    let (status, _) = post(&anna, &format!("/reviews/{review_id}/accept"), json!({})).await;
    assert_eq!(status, 403);
    let (status, err) = post(
        &james,
        &format!("/reviews/{review_id}/submit"),
        json!({"opinion": "x", "recommendation": "approve"}),
    )
    .await;
    assert_eq!(
        (status, err["error"]["code"].as_str()),
        (409, Some("invalid_review_state"))
    );
    let (status, _) = post(
        &james,
        &format!("/reviews/{review_id}/decline"),
        json!({"reason": ""}),
    )
    .await;
    assert_eq!(status, 422);
    let (status, _) = post(
        &james,
        &format!("/reviews/{review_id}/decline"),
        json!({"reason": "Conflict of interest: co-author"}),
    )
    .await;
    assert_eq!(status, 200);
    let (status, _) = get(&james, &format!("/projects/{assigned}")).await;
    assert_eq!(status, 403);
}

#[tokio::test]
async fn second_team_cannot_read_and_removed_member_loses_access() {
    let app = spawn_app(true).await;
    let anna = persona(&app, "anna").await;
    let liam = persona(&app, "liam").await;
    let lukas = persona(&app, "lukas").await;
    let pid: String =
        sqlx::query_scalar("SELECT id FROM projects WHERE title = 'Coral health around Pitcairn'")
            .fetch_one(&app.pool)
            .await
            .unwrap();
    add_site(&anna, &pid, "Secret reef", true).await;

    for path in [
        format!("/projects/{pid}"),
        format!("/projects/{pid}/threads"),
        format!("/projects/{pid}/sites"),
        format!("/projects/{pid}/sites.geojson"),
        format!("/projects/{pid}/timeline"),
        format!("/projects/{pid}/members"),
        format!("/projects/{pid}/change-requests"),
    ] {
        let (status, _) = get(&lukas, &path).await;
        assert_eq!(status, 403, "{path}");
    }
    let (status, _) = patch(
        &lukas,
        &format!("/projects/{pid}"),
        json!({"version": 1, "title": "x"}),
    )
    .await;
    assert_eq!(status, 403);

    // Liam can read until removed; afterwards every request is denied.
    let (status, _) = get(&liam, &format!("/projects/{pid}")).await;
    assert_eq!(status, 200);
    let liam_id = persona_id(&app, "liam").await;
    let (status, _) = delete(
        &liam,
        &format!(
            "/projects/{pid}/members/{}",
            persona_id(&app, "priya").await
        ),
    )
    .await;
    assert_eq!(status, 403, "only the lead removes others");
    let (status, _) = delete(&anna, &format!("/projects/{pid}/members/{liam_id}")).await;
    assert_eq!(status, 204);
    let (status, _) = get(&liam, &format!("/projects/{pid}")).await;
    assert_eq!(status, 403);
    let (_, members) = get(&anna, &format!("/projects/{pid}/members")).await;
    let liam_row = members["members"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["user_id"] == liam_id.as_str())
        .expect("history kept");
    assert!(liam_row["removed_at"].is_string());

    // Lead cannot remove self; make-lead transfers (previous lead → editor).
    let anna_id = persona_id(&app, "anna").await;
    let (status, err) = delete(&anna, &format!("/projects/{pid}/members/{anna_id}")).await;
    assert_eq!(
        (status, err["error"]["code"].as_str()),
        (409, Some("lead_must_transfer"))
    );
    let priya_id = persona_id(&app, "priya").await;
    let (status, members) = post(
        &anna,
        &format!("/projects/{pid}/members/{priya_id}/make-lead"),
        json!({}),
    )
    .await;
    assert_eq!(status, 200, "{members}");
    let roles: Vec<(String, String)> = members["members"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|m| m["removed_at"].is_null())
        .map(|m| {
            (
                m["user_id"].as_str().unwrap().into(),
                m["role"].as_str().unwrap().into(),
            )
        })
        .collect();
    assert!(roles.contains(&(priya_id.clone(), "lead".into())));
    assert!(roles.contains(&(anna_id.clone(), "editor".into())));
    assert_eq!(roles.iter().filter(|(_, r)| r == "lead").count(), 1);
    let (status, _) = post(
        &anna,
        &format!("/projects/{pid}/invitations"),
        json!({"email": "x@example.invalid", "role": "viewer"}),
    )
    .await;
    assert_eq!(status, 403, "anna is no longer lead");
}

#[tokio::test]
async fn invitation_accept_requires_matching_email() {
    let app = spawn_app(true).await;
    let anna = persona(&app, "anna").await;
    let pid = create_project(&anna, "fieldwork_permit", "Invite flow").await;
    let (status, err) = post(
        &anna,
        &format!("/projects/{pid}/invitations"),
        json!({"email": "newbie@fjord.invalid", "role": "lead"}),
    )
    .await;
    assert_eq!(status, 422, "{err}");
    let (status, inv) = post(
        &anna,
        &format!("/projects/{pid}/invitations"),
        json!({"email": "newbie@fjord.invalid", "role": "editor"}),
    )
    .await;
    assert_eq!(status, 201, "{inv}");
    let url = inv["accept_url"].as_str().unwrap();
    let token = url.rsplit('/').next().unwrap();
    let mail: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM mail_messages WHERE to_email = 'newbie@fjord.invalid' AND body_text LIKE ?",
    )
    .bind(format!("%{token}%"))
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(mail, 1, "token emailed");

    let lukas = persona(&app, "lukas").await;
    let (status, _) = post(&lukas, &format!("/invitations/{token}/accept"), json!({})).await;
    assert_eq!(status, 403);
    let newbie = register(&app, "newbie@fjord.invalid", "New Bie").await;
    let (status, _) = post(&newbie, &format!("/invitations/{token}/accept"), json!({})).await;
    assert_eq!(status, 200);
    let (status, ws) = get(&newbie, &format!("/projects/{pid}")).await;
    assert_eq!(status, 200);
    assert_eq!(ws["project"]["my_access"], "team_editor");
}
