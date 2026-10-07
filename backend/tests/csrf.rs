mod common;

use common::{Client, spawn_app};

#[tokio::test]
async fn missing_csrf_header_blocks_non_get() {
    let app = spawn_app(true).await;
    let client = Client::anonymous(&app);

    let no_csrf = client
        .request_no_csrf(reqwest::Method::POST, "/api/v1/auth/login")
        .json(&serde_json::json!({
            "email": "maria@demo.pitcairn.invalid",
            "password": pitcairn::seed::DEMO_PASSWORD,
        }))
        .send()
        .await
        .expect("send");
    assert_eq!(no_csrf.status(), 403);
    let err: serde_json::Value = client.json(no_csrf).await;
    assert_eq!(err["error"]["code"], "csrf");

    let with_csrf = client
        .post_json(
            "/api/v1/auth/login",
            &serde_json::json!({
                "email": "maria@demo.pitcairn.invalid",
                "password": pitcairn::seed::DEMO_PASSWORD,
            }),
        )
        .await;
    assert_eq!(with_csrf.status(), 200);
}
