mod common;

use common::{Client, spawn_app};

#[tokio::test]
async fn register_login_logout_me_cycle() {
    let app = spawn_app(true).await;
    let client = Client::anonymous(&app);

    let resp = client
        .post_json(
            "/api/v1/auth/register",
            &serde_json::json!({
                "email": "new.user@example.invalid",
                "name": "New User",
                "organisation": "Example Org",
                "password": "password123"
            }),
        )
        .await;
    assert_eq!(resp.status(), 200, "register should succeed");

    let me: pitcairn::dto::MeResponse = client.json(client.get("/api/v1/auth/me").await).await;
    assert_eq!(me.user.email, "new.user@example.invalid");

    let logout = client.post("/api/v1/auth/logout").await;
    assert_eq!(logout.status(), 204, "logout should succeed");

    let me_after = client.get("/api/v1/auth/me").await;
    assert_eq!(
        me_after.status(),
        401,
        "me should be unauthorized after logout"
    );

    let login_resp = client
        .post_json(
            "/api/v1/auth/login",
            &serde_json::json!({"email": "new.user@example.invalid", "password": "password123"}),
        )
        .await;
    assert_eq!(login_resp.status(), 200, "login should succeed");
}

#[tokio::test]
async fn wrong_password_returns_invalid_credentials() {
    let app = spawn_app(true).await;
    let client = Client::anonymous(&app);

    let resp = client
        .post_json(
            "/api/v1/auth/login",
            &serde_json::json!({"email": "maria@demo.pitcairn.invalid", "password": "wrong"}),
        )
        .await;
    assert_eq!(resp.status(), 401);
    let err: serde_json::Value = client.json(resp).await;
    assert_eq!(err["error"]["code"], "invalid_credentials");
}

#[tokio::test]
async fn staff_blocked_until_mfa_then_can_access() {
    let app = spawn_app(true).await;
    let client = Client::anonymous(&app);
    let login_resp = client
        .post_json(
            "/api/v1/auth/login",
            &serde_json::json!({"email": "maria@demo.pitcairn.invalid", "password": pitcairn::seed::DEMO_PASSWORD}),
        )
        .await;
    assert_eq!(login_resp.status(), 200);
    let login: pitcairn::dto::LoginResponse = client.json(login_resp).await;
    assert!(login.mfa_required, "maria should require MFA");

    let notifications = client.get("/api/v1/notifications").await;
    assert_eq!(notifications.status(), 403);
    let err: serde_json::Value = client.json(notifications).await;
    assert_eq!(err["error"]["code"], "mfa_required");

    let user_id = login.user.id.clone();
    let totp: pitcairn::dto::DemoTotpResponse = client
        .json(client.get(&format!("/api/v1/demo/totp/{user_id}")).await)
        .await;

    let verify = client
        .post_json(
            "/api/v1/auth/mfa/verify",
            &serde_json::json!({"code": totp.code}),
        )
        .await;
    assert_eq!(verify.status(), 200);

    let notifications = client.get("/api/v1/notifications").await;
    assert_eq!(notifications.status(), 200);
}

#[tokio::test]
async fn demo_endpoints_404_when_demo_off() {
    let app = spawn_app(false).await;
    let client = Client::anonymous(&app);

    let personas = client.get("/api/v1/demo/personas").await;
    assert_eq!(personas.status(), 404);

    let switch = client
        .post_json(
            "/api/v1/demo/switch",
            &serde_json::json!({"persona_key": "anna"}),
        )
        .await;
    assert_eq!(switch.status(), 404);
}
