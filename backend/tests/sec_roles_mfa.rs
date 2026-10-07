//! Security regressions for role administration (§3): separation of duties
//! on role grants, and MFA on promotion to a staff/expert role.

mod common;

use common::c::user_id;
use common::{Client, persona, spawn_app};
use serde_json::{Value, json};

#[tokio::test]
async fn decision_maker_without_admin_cannot_manage_other_roles() {
    let app = spawn_app(true).await;
    let helen = persona(&app, "helen").await;
    let helen_id = user_id(&app, "helen@demo.pitcairn.invalid").await;
    let maria_id = user_id(&app, "maria@demo.pitcairn.invalid").await;
    let lukas_id = user_id(&app, "lukas@demo.pitcairn.invalid").await;

    // Self-promotion to admin.
    let resp = helen
        .post_json(
            &format!("/api/v1/admin/users/{helen_id}/roles"),
            &json!({"role": "admin"}),
        )
        .await;
    assert_eq!(resp.status(), 403);
    // Any other non-decision_maker role, granted or revoked.
    let resp = helen
        .post_json(
            &format!("/api/v1/admin/users/{lukas_id}/roles"),
            &json!({"role": "coordinator"}),
        )
        .await;
    assert_eq!(resp.status(), 403);
    let resp = helen
        .delete(&format!("/api/v1/admin/users/{maria_id}/roles/coordinator"))
        .await;
    assert_eq!(resp.status(), 403);
    let roles: Vec<String> = sqlx::query_scalar(
        "SELECT role FROM user_roles WHERE user_id IN (?, ?, ?) AND revoked_at IS NULL",
    )
    .bind(&helen_id)
    .bind(&maria_id)
    .bind(&lukas_id)
    .fetch_all(&app.pool)
    .await
    .unwrap();
    assert!(!roles.contains(&"admin".to_string()));
    assert!(roles.contains(&"coordinator".to_string()), "maria keeps it");
    assert_eq!(roles.iter().filter(|r| *r == "coordinator").count(), 1);

    // decision_maker remains helen's to grant; other roles remain admin's.
    let resp = helen
        .post_json(
            &format!("/api/v1/admin/users/{lukas_id}/roles"),
            &json!({"role": "decision_maker"}),
        )
        .await;
    assert_eq!(resp.status(), 200);
    let admin = persona(&app, "admin").await;
    let resp = admin
        .post_json(
            &format!("/api/v1/admin/users/{lukas_id}/roles"),
            &json!({"role": "finance"}),
        )
        .await;
    assert_eq!(resp.status(), 200);
    let resp = admin
        .delete(&format!(
            "/api/v1/admin/users/{lukas_id}/roles/decision_maker"
        ))
        .await;
    assert_eq!(resp.status(), 403);
}

async fn dashboard_status(client: &Client) -> (u16, Value) {
    let resp = client.get("/api/v1/dashboard").await;
    let status = resp.status().as_u16();
    (status, client.json(resp).await)
}

#[tokio::test]
async fn promotion_does_not_inherit_an_unverified_session() {
    let app = spawn_app(true).await;
    let admin = persona(&app, "admin").await;

    // A researcher session from a normal login (MFA not required then).
    let lukas = persona(&app, "lukas").await;
    let lukas_id = user_id(&app, "lukas@demo.pitcairn.invalid").await;
    assert_eq!(dashboard_status(&lukas).await.0, 200);
    let resp = admin
        .post_json(
            &format!("/api/v1/admin/users/{lukas_id}/roles"),
            &json!({"role": "coordinator"}),
        )
        .await;
    assert_eq!(resp.status(), 200);
    let (status, body) = dashboard_status(&lukas).await;
    assert_eq!(status, 403, "{body}");
    assert_eq!(body["error"]["code"], "mfa_required");
    let me: Value = lukas.json(lukas.get("/api/v1/auth/me").await).await;
    assert_eq!(me["mfa_verified"], false);

    // A demo-switch researcher session ("MFA satisfied") is reset as well.
    let anna = Client::anonymous(&app);
    let resp = anna
        .post_json("/api/v1/demo/switch", &json!({"persona_key": "anna"}))
        .await;
    assert_eq!(resp.status(), 200);
    assert_eq!(dashboard_status(&anna).await.0, 200);
    let anna_id = user_id(&app, "anna@demo.pitcairn.invalid").await;
    // Operational roles are the coordinator's to grant; MFA applies all the same.
    let maria = persona(&app, "maria").await;
    let resp = maria
        .post_json(
            &format!("/api/v1/admin/users/{anna_id}/roles"),
            &json!({"role": "expert"}),
        )
        .await;
    assert_eq!(resp.status(), 200);
    let (status, body) = dashboard_status(&anna).await;
    assert_eq!(status, 403, "{body}");
    assert_eq!(body["error"]["code"], "mfa_required");

    // Staff that already verified keep their session on a further grant.
    let maria_id = user_id(&app, "maria@demo.pitcairn.invalid").await;
    let resp = admin
        .post_json(
            &format!("/api/v1/admin/users/{maria_id}/roles"),
            &json!({"role": "finance"}),
        )
        .await;
    assert_eq!(resp.status(), 200);
    assert_eq!(dashboard_status(&maria).await.0, 200);
}
