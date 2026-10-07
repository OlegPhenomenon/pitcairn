//! Slice D: `deliverable_reminders` job — sender reminded when due ≤ 14 days
//! or overdue, recipient when overdue; one reminder per deliverable per day.

mod common;

use common::spawn_app;

async fn drain(app: &common::TestApp) {
    for _ in 0..500 {
        if !pitcairn::jobs::run_once(&app.state).await.unwrap() {
            return;
        }
    }
    panic!("job queue did not drain");
}

async fn reminders_for(app: &common::TestApp, email: &str) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT n.title FROM notifications n JOIN users u ON u.id = n.user_id
         WHERE u.email = ? AND n.kind = 'deliverable_reminder' ORDER BY n.title",
    )
    .bind(email)
    .fetch_all(&app.pool)
    .await
    .unwrap()
}

#[tokio::test]
async fn daily_scan_reminds_sender_and_recipient_once_per_day() {
    let app = spawn_app(true).await;
    // A deliverable due in 5 days (sender only) on the reef fish project.
    let (deliverable_id,): (String,) = sqlx::query_as(
        "SELECT d.id FROM deliverables d JOIN projects p ON p.id = d.project_id
         WHERE p.title = 'Reef fish biomass at Bounty Bay' AND d.kind = 'report'",
    )
    .fetch_one(&app.pool)
    .await
    .unwrap();
    let soon = (chrono::Utc::now().date_naive() + chrono::Duration::days(5))
        .format("%Y-%m-%d")
        .to_string();
    sqlx::query("UPDATE deliverables SET due_date = ? WHERE id = ?")
        .bind(&soon)
        .bind(&deliverable_id)
        .execute(&app.pool)
        .await
        .unwrap();

    pitcairn::jobs::reminders::enqueue_daily(&app.pool)
        .await
        .unwrap();
    pitcairn::jobs::reminders::enqueue_daily(&app.pool)
        .await
        .unwrap(); // deduped
    drain(&app).await;

    let anna = reminders_for(&app, "anna@demo.pitcairn.invalid").await;
    assert_eq!(
        anna,
        vec![
            "Due soon: Field report and community summary".to_string(),
            "Overdue: Census summary report".to_string(),
        ]
    );
    // Recipient only for the overdue one.
    let maria = reminders_for(&app, "maria@demo.pitcairn.invalid").await;
    assert_eq!(maria, vec!["Overdue: Census summary report".to_string()]);
    // Per-deliverable jobs carry the reminder:<id>:<date> dedupe key.
    let today = chrono::Utc::now()
        .date_naive()
        .format("%Y-%m-%d")
        .to_string();
    let key: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM jobs WHERE dedupe_key = ?")
        .bind(format!("reminder:{deliverable_id}:{today}"))
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(key, 1);

    // Running the scan again the same day (any path) sends nothing new.
    pitcairn::jobs::reminders::run(&app.pool, &serde_json::json!({}))
        .await
        .unwrap();
    pitcairn::jobs::reminders::run(
        &app.pool,
        &serde_json::json!({"deliverable_id": deliverable_id}),
    )
    .await
    .unwrap();
    drain(&app).await;
    assert_eq!(
        reminders_for(&app, "anna@demo.pitcairn.invalid")
            .await
            .len(),
        2
    );
    assert_eq!(
        reminders_for(&app, "maria@demo.pitcairn.invalid")
            .await
            .len(),
        1
    );

    // In-app notifications are mirrored by queued mail.
    let mails: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM mail_messages WHERE to_email = 'maria@demo.pitcairn.invalid'
         AND subject = 'Overdue: Census summary report'",
    )
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(mails, 1);
}
