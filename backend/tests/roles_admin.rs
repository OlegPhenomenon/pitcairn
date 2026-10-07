mod common;

use common::a::{delete, get, persona_id, post};
use common::{persona, spawn_app};
use serde_json::{Value, json};

fn emails(list: &Value) -> Vec<String> {
    list["items"]
        .as_array()
        .expect("items")
        .iter()
        .map(|u| u["email"].as_str().unwrap().to_string())
        .collect()
}

#[tokio::test]
async fn decision_maker_lists_users_and_grants_decision_maker_from_the_users_screen() {
    let app = spawn_app(true).await;
    let helen = persona(&app, "helen").await;
    let admin = persona(&app, "admin").await;
    let sam_id = persona_id(&app, "sam").await;
    let ruth_id = persona_id(&app, "ruth").await;
    let helen_id = persona_id(&app, "helen").await;

    // Search works for the decision maker (Users screen).
    let (status, list) = get(&helen, "/admin/users?q=sam").await;
    assert_eq!(status, 200, "{list}");
    assert!(
        emails(&list).contains(&"sam@demo.pitcairn.invalid".to_string()),
        "{list}"
    );

    // Grant decision_maker to a staff user and revoke it again.
    let (status, user) = post(
        &helen,
        &format!("/admin/users/{sam_id}/roles"),
        json!({"role": "decision_maker"}),
    )
    .await;
    assert_eq!(status, 200, "{user}");
    assert!(
        user["roles"]
            .as_array()
            .unwrap()
            .contains(&json!("decision_maker"))
    );
    let (status, user) = delete(
        &helen,
        &format!("/admin/users/{sam_id}/roles/decision_maker"),
    )
    .await;
    assert_eq!(status, 200, "{user}");
    assert!(
        !user["roles"]
            .as_array()
            .unwrap()
            .contains(&json!("decision_maker"))
    );

    // The technical admin never obtains the power to issue permits.
    let (status, err) = post(
        &admin,
        &format!("/admin/users/{ruth_id}/roles"),
        json!({"role": "decision_maker"}),
    )
    .await;
    assert_eq!(
        (status, err["error"]["code"].as_str()),
        (403, Some("cannot_grant_decision_maker"))
    );
    // Nor does the decision maker get any other role-administration power.
    let (status, _) = post(
        &helen,
        &format!("/admin/users/{ruth_id}/roles"),
        json!({"role": "expert"}),
    )
    .await;
    assert_eq!(status, 403);
    // Nobody grants themselves a role.
    let (status, err) = post(
        &admin,
        &format!("/admin/users/{}/roles", persona_id(&app, "admin").await),
        json!({"role": "finance"}),
    )
    .await;
    assert_eq!(
        (status, err["error"]["code"].as_str()),
        (403, Some("cannot_grant_self"))
    );
    let (status, _) = post(
        &helen,
        &format!("/admin/users/{helen_id}/roles"),
        json!({"role": "decision_maker"}),
    )
    .await;
    assert_eq!(status, 403);
}

#[tokio::test]
async fn coordinator_manages_operational_roles_only() {
    let app = spawn_app(true).await;
    let maria = persona(&app, "maria").await;
    let lukas_id = persona_id(&app, "lukas").await;
    let maria_id = persona_id(&app, "maria").await;

    let (status, list) = get(&maria, "/admin/users?q=lukas").await;
    assert_eq!(status, 200, "{list}");
    assert!(
        emails(&list).contains(&"lukas@demo.pitcairn.invalid".to_string()),
        "{list}"
    );

    for role in ["expert", "provider"] {
        let (status, user) = post(
            &maria,
            &format!("/admin/users/{lukas_id}/roles"),
            json!({"role": role}),
        )
        .await;
        assert_eq!(status, 200, "{user}");
        assert!(user["roles"].as_array().unwrap().contains(&json!(role)));
    }
    let (status, _) = delete(&maria, &format!("/admin/users/{lukas_id}/roles/provider")).await;
    assert_eq!(status, 200);

    let (status, err) = post(
        &maria,
        &format!("/admin/users/{lukas_id}/roles"),
        json!({"role": "decision_maker"}),
    )
    .await;
    assert_eq!(
        (status, err["error"]["code"].as_str()),
        (403, Some("cannot_grant_decision_maker"))
    );
    for role in ["admin", "coordinator", "finance", "base_manager"] {
        let (status, _) = post(
            &maria,
            &format!("/admin/users/{lukas_id}/roles"),
            json!({"role": role}),
        )
        .await;
        assert_eq!(status, 403, "coordinator must not grant {role}");
    }
    let (status, _) = post(
        &maria,
        &format!("/admin/users/{maria_id}/roles"),
        json!({"role": "expert"}),
    )
    .await;
    assert_eq!(status, 403, "no self-grant");
    // Account administration stays with the technical admin.
    let (status, _) = post(
        &maria,
        "/admin/users",
        json!({"email": "x@example.invalid", "name": "X", "organisation": "", "password": "longenough"}),
    )
    .await;
    assert_eq!(status, 403);

    let roles: Vec<String> =
        sqlx::query_scalar("SELECT role FROM user_roles WHERE user_id = ? AND revoked_at IS NULL")
            .bind(&lukas_id)
            .fetch_all(&app.pool)
            .await
            .unwrap();
    assert_eq!(roles, vec!["expert".to_string()]);
}

#[tokio::test]
async fn users_list_is_closed_to_researchers_and_finance() {
    let app = spawn_app(true).await;
    for key in ["anna", "ruth"] {
        let client = persona(&app, key).await;
        let (status, _) = get(&client, "/admin/users").await;
        assert_eq!(status, 403, "{key}");
    }
}

#[tokio::test]
async fn base_manager_and_finance_edit_tariffs_researcher_cannot() {
    let app = spawn_app(true).await;
    let sam = persona(&app, "sam").await;
    let ruth = persona(&app, "ruth").await;
    let anna = persona(&app, "anna").await;

    // The base manager curates the resource catalogue.
    let (status, resource) = post(
        &sam,
        "/resources",
        json!({"kind": "equipment", "name": "Drone kit", "quantity": 1}),
    )
    .await;
    assert_eq!(status, 201, "{resource}");
    let rid = resource["id"].as_str().unwrap().to_string();
    let (status, _) = post(
        &ruth,
        "/resources",
        json!({"kind": "equipment", "name": "Finance kit", "quantity": 1}),
    )
    .await;
    assert_eq!(status, 403, "finance sets prices, not the catalogue");

    // Prices: append-only tariffs from an effective date.
    for (client, from, cents) in [(&sam, "2026-11-01", 5000), (&ruth, "2027-01-01", 6000)] {
        let (status, tariff) = post(
            client,
            &format!("/resources/{rid}/tariffs"),
            json!({"unit": "per_day", "price_cents": cents, "currency": "NZD", "effective_from": from}),
        )
        .await;
        assert_eq!(status, 201, "{tariff}");
    }
    let (status, _) = post(
        &anna,
        &format!("/resources/{rid}/tariffs"),
        json!({"unit": "per_day", "price_cents": 1, "currency": "NZD", "effective_from": "2027-02-01"}),
    )
    .await;
    assert_eq!(status, 403);

    let (status, tariffs) = get(&ruth, &format!("/resources/{rid}/tariffs")).await;
    assert_eq!(status, 200);
    let history: Vec<(String, i64)> = tariffs["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| {
            (
                t["effective_from"].as_str().unwrap().to_string(),
                t["price_cents"].as_i64().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        history,
        vec![
            ("2027-01-01".to_string(), 6000),
            ("2026-11-01".to_string(), 5000)
        ]
    );
}

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
