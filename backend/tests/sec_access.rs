//! Security regressions for project access: a revoked expert loses project
//! access and review actions at once, and `/assist/summary` follows the
//! workspace's application-read rule.

mod common;

use common::c::{seeded_project, user_id};
use common::{persona, spawn_app};
use serde_json::json;

#[tokio::test]
async fn revoked_expert_loses_assigned_projects_and_reviews() {
    let app = spawn_app(true).await;
    let james = persona(&app, "james").await;
    let james_id = user_id(&app, "james@demo.pitcairn.invalid").await;
    let (review_id, project_id): (String, String) = sqlx::query_as(
        "SELECT id, project_id FROM review_assignments
         WHERE expert_id = ? AND status = 'accepted' LIMIT 1",
    )
    .bind(&james_id)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    let project = format!("/api/v1/projects/{project_id}");
    let threads = format!("/api/v1/projects/{project_id}/threads");
    assert_eq!(james.get(&project).await.status(), 200);
    assert_eq!(james.get(&threads).await.status(), 200);

    let admin = persona(&app, "admin").await;
    let resp = admin
        .delete(&format!("/api/v1/admin/users/{james_id}/roles/expert"))
        .await;
    assert_eq!(resp.status(), 200);

    // Same session, assignment still on record: no access any more.
    assert_eq!(james.get(&project).await.status(), 403);
    assert_eq!(james.get(&threads).await.status(), 403);
    assert_eq!(
        james
            .get(&format!("/api/v1/projects/{project_id}/documents"))
            .await
            .status(),
        403
    );
    let resp = james
        .post_json(
            &format!("/api/v1/reviews/{review_id}/submit"),
            &json!({"opinion": "Looks fine to me.", "recommendation": "approve"}),
        )
        .await;
    assert_eq!(resp.status(), 403);
    let resp = james
        .post_json(
            &format!("/api/v1/reviews/{review_id}/decline"),
            &json!({"reason": "no longer an expert"}),
        )
        .await;
    assert_eq!(resp.status(), 403);
    let status: String = sqlx::query_scalar("SELECT status FROM review_assignments WHERE id = ?")
        .bind(&review_id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(status, "accepted", "review untouched");
}

#[tokio::test]
async fn assist_summary_requires_application_read_access() {
    let app = spawn_app(true).await;
    let pid = seeded_project(&app).await;
    // Base manager and finance see the project summary only (§3).
    for key in ["sam", "ruth"] {
        let client = persona(&app, key).await;
        let resp = client
            .post_json("/api/v1/assist/summary", &json!({"project_id": pid}))
            .await;
        assert_eq!(resp.status(), 403, "{key} must not read the application");
    }
    let maria = persona(&app, "maria").await;
    let resp = maria
        .post_json("/api/v1/assist/summary", &json!({"project_id": pid}))
        .await;
    assert_eq!(resp.status(), 200);
}
