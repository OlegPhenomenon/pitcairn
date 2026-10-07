mod common;

use common::{persona, spawn_app};

#[tokio::test]
async fn role_grant_queues_email_and_notification_survives_mail_outage() {
    let app = spawn_app(true).await;
    let admin = persona(&app, "admin").await;
    let helen = persona(&app, "helen").await;

    let settings: pitcairn::dto::SettingsDto =
        admin.json(admin.get("/api/v1/admin/settings").await).await;
    let mut updated = settings.clone();
    updated.mail_enabled = false;
    let put = admin.put_json("/api/v1/admin/settings", &updated).await;
    assert_eq!(put.status(), 200);

    let ruth_id: (String,) = sqlx::query_as("SELECT id FROM users WHERE email = ?")
        .bind("ruth@demo.pitcairn.invalid")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    let ruth_id = ruth_id.0;

    let grant = helen
        .post_json(
            &format!("/api/v1/admin/users/{ruth_id}/roles"),
            &serde_json::json!({"role": "provider"}),
        )
        .await;
    assert_eq!(grant.status(), 200);

    let jobs: pitcairn::dto::ListResponse<pitcairn::dto::JobDto> = admin
        .json(admin.get("/api/v1/admin/jobs?status=queued").await)
        .await;
    let send_email_job = jobs
        .items
        .iter()
        .find(|j| j.kind == pitcairn::jobs::KIND_SEND_EMAIL)
        .expect("send_email job queued")
        .clone();

    while pitcairn::jobs::run_once(&app.state).await.expect("run job") {}

    let job: (String, Option<String>) =
        sqlx::query_as("SELECT status, last_error FROM jobs WHERE id = ?")
            .bind(&send_email_job.id)
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(job.0, "failed");
    let last_error = job.1.expect("last_error should be set");
    assert!(
        last_error.to_lowercase().contains("mail"),
        "error should mention mail: {last_error}"
    );

    let ruth = persona(&app, "ruth").await;
    let notifications: pitcairn::dto::ListResponse<pitcairn::dto::NotificationDto> =
        ruth.json(ruth.get("/api/v1/notifications").await).await;
    let role_note = notifications
        .items
        .iter()
        .find(|n| n.kind == "role.granted")
        .expect("role.granted notification exists");
    assert!(role_note.body.contains("provider"));

    let mut re_enabled = settings.clone();
    re_enabled.mail_enabled = true;
    let put = admin.put_json("/api/v1/admin/settings", &re_enabled).await;
    assert_eq!(put.status(), 200);

    let retry = admin
        .post(&format!("/api/v1/admin/jobs/{}/retry", send_email_job.id))
        .await;
    assert_eq!(retry.status(), 200);

    while pitcairn::jobs::run_once(&app.state).await.expect("run job") {}

    let job: (String,) = sqlx::query_as("SELECT status FROM jobs WHERE id = ?")
        .bind(&send_email_job.id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(job.0, "done");

    let mail: (String,) = sqlx::query_as(
        "SELECT status FROM mail_messages
         WHERE to_email = 'ruth@demo.pitcairn.invalid'
         ORDER BY created_at DESC LIMIT 1",
    )
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(mail.0, "sent");
}
