mod common;

use common::{persona, spawn_app};

#[tokio::test]
async fn admin_cannot_grant_decision_maker_but_decision_maker_can() {
    let app = spawn_app(true).await;
    let admin = persona(&app, "admin").await;
    let helen = persona(&app, "helen").await;

    let ruth_id: (String,) = sqlx::query_as("SELECT id FROM users WHERE email = ?")
        .bind("ruth@demo.pitcairn.invalid")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    let ruth_id = ruth_id.0;

    let admin_grant = admin
        .post_json(
            &format!("/api/v1/admin/users/{ruth_id}/roles"),
            &serde_json::json!({"role": "decision_maker"}),
        )
        .await;
    assert_eq!(admin_grant.status(), 403);
    let err: serde_json::Value = admin.json(admin_grant).await;
    assert_eq!(err["error"]["code"], "cannot_grant_decision_maker");

    let helen_grant = helen
        .post_json(
            &format!("/api/v1/admin/users/{ruth_id}/roles"),
            &serde_json::json!({"role": "decision_maker"}),
        )
        .await;
    assert_eq!(helen_grant.status(), 200);

    let users: pitcairn::dto::ListResponse<pitcairn::dto::UserDto> = admin
        .json(admin.get("/api/v1/admin/users?q=ruth").await)
        .await;
    let ruth = users
        .items
        .into_iter()
        .find(|u| u.id == ruth_id)
        .expect("ruth in results");
    assert!(ruth.roles.contains(&"decision_maker".to_string()));

    let revoke = helen
        .delete(&format!(
            "/api/v1/admin/users/{ruth_id}/roles/decision_maker"
        ))
        .await;
    assert_eq!(revoke.status(), 200);
}

#[tokio::test]
async fn admin_cannot_patch_decision_maker() {
    let app = spawn_app(true).await;
    let admin = persona(&app, "admin").await;

    let helen_id: (String,) = sqlx::query_as("SELECT id FROM users WHERE email = ?")
        .bind("helen@demo.pitcairn.invalid")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    let helen_id = helen_id.0;

    let patch = admin
        .patch_json(
            &format!("/api/v1/admin/users/{helen_id}"),
            &serde_json::json!({"name": "X"}),
        )
        .await;
    assert_eq!(patch.status(), 409);
    let err: serde_json::Value = admin.json(patch).await;
    assert_eq!(err["error"]["code"], "protected_user");
}
