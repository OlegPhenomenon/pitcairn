//! `deliverable_reminders` job (§7): daily, one reminder per deliverable per
//! day. The sender (team member) is reminded when a deliverable is due within
//! 14 days or overdue; the recipient (staff) when it is overdue.
//!
//! Payload `{}` = daily scan: enqueues one `deliverable_reminders` job per
//! deliverable that needs a reminder, deduped by
//! `reminder:<deliverable_id>:<date>`. Payload `{"deliverable_id": ...}` =
//! remind for that deliverable now (no-op if it no longer qualifies or was
//! already reminded today).

use chrono::NaiveDate;
use serde_json::{Value, json};
use sqlx::SqlitePool;

use crate::error::AppResult;

pub const KIND: &str = "deliverable_reminders";
pub const NOTIFICATION_KIND: &str = "deliverable_reminder";
const REMIND_WITHIN_DAYS: i64 = 14;
/// Statuses where the team still owes the result.
const OPEN_STATUSES: &str = "('agreed','changes_requested')";

fn today() -> NaiveDate {
    chrono::Utc::now().date_naive()
}

/// Enqueue the daily scan (deduped per date, so repeated calls are no-ops).
pub async fn enqueue_daily(pool: &SqlitePool) -> AppResult<()> {
    let date = today().format("%Y-%m-%d").to_string();
    let mut tx = pool.begin().await?;
    super::enqueue(&mut tx, KIND, json!({}), Some(&format!("{KIND}:{date}"))).await?;
    tx.commit().await?;
    Ok(())
}

pub async fn run(pool: &SqlitePool, payload: &Value) -> AppResult<()> {
    match payload["deliverable_id"].as_str() {
        Some(id) => remind_one(pool, id).await,
        None => scan(pool).await,
    }
}

async fn scan(pool: &SqlitePool) -> AppResult<()> {
    let today = today();
    let horizon = (today + chrono::Duration::days(REMIND_WITHIN_DAYS))
        .format("%Y-%m-%d")
        .to_string();
    let date = today.format("%Y-%m-%d").to_string();
    let ids: Vec<String> = sqlx::query_scalar(&format!(
        "SELECT d.id FROM deliverables d JOIN projects p ON p.id = d.project_id
         WHERE d.status IN {OPEN_STATUSES} AND d.due_date <= ?
           AND p.status NOT IN ('withdrawn','refused')
         ORDER BY d.due_date"
    ))
    .bind(&horizon)
    .fetch_all(pool)
    .await?;
    let mut tx = pool.begin().await?;
    for id in ids {
        super::enqueue(
            &mut tx,
            KIND,
            json!({ "deliverable_id": id }),
            Some(&format!("reminder:{id}:{date}")),
        )
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

#[derive(sqlx::FromRow)]
struct Due {
    project_id: String,
    title: String,
    due_date: String,
    status: String,
    sender_id: String,
    recipient_id: String,
    project_title: String,
    reference: Option<String>,
    project_status: String,
}

async fn remind_one(pool: &SqlitePool, deliverable_id: &str) -> AppResult<()> {
    let row: Option<Due> = sqlx::query_as(
        "SELECT d.project_id, d.title, d.due_date, d.status, d.sender_id, d.recipient_id,
                p.title AS project_title, p.reference, p.status AS project_status
         FROM deliverables d JOIN projects p ON p.id = d.project_id WHERE d.id = ?",
    )
    .bind(deliverable_id)
    .fetch_optional(pool)
    .await?;
    let Some(d) = row else {
        return Ok(()); // deleted meanwhile: nothing to remind
    };
    let today = today();
    let Ok(due) = NaiveDate::parse_from_str(&d.due_date, "%Y-%m-%d") else {
        return Ok(());
    };
    let days_left = (due - today).num_days();
    let open = matches!(d.status.as_str(), "agreed" | "changes_requested");
    if !open
        || days_left > REMIND_WITHIN_DAYS
        || matches!(d.project_status.as_str(), "withdrawn" | "refused")
    {
        return Ok(());
    }
    let overdue = days_left < 0;
    let link = format!("/app/projects/{}/results", d.project_id);
    let day_start = format!("{}T00:00:00Z", today.format("%Y-%m-%d"));
    let project_label = d
        .reference
        .clone()
        .unwrap_or_else(|| d.project_title.clone());

    let mut tx = crate::db::begin_immediate(pool).await?;
    // One reminder per deliverable per user per day, whichever path enqueued it.
    let already: Vec<String> = sqlx::query_scalar(
        "SELECT user_id FROM notifications
         WHERE kind = ? AND link = ? AND created_at >= ? AND body LIKE ?",
    )
    .bind(NOTIFICATION_KIND)
    .bind(&link)
    .bind(&day_start)
    .bind(format!("%[{deliverable_id}]%"))
    .fetch_all(&mut *tx)
    .await?;

    let (title, body) = if overdue {
        (
            format!("Overdue: {}", d.title),
            format!(
                "\"{}\" for {project_label} was due on {} ({} days ago). [{deliverable_id}]",
                d.title, d.due_date, -days_left
            ),
        )
    } else {
        (
            format!("Due soon: {}", d.title),
            format!(
                "\"{}\" for {project_label} is due on {} (in {days_left} days). [{deliverable_id}]",
                d.title, d.due_date
            ),
        )
    };
    if !already.contains(&d.sender_id) {
        crate::notify::notify(
            &mut tx,
            &d.sender_id,
            NOTIFICATION_KIND,
            &title,
            &body,
            &link,
            Some(&d.project_id),
        )
        .await?;
    }
    if overdue && d.recipient_id != d.sender_id && !already.contains(&d.recipient_id) {
        crate::notify::notify(
            &mut tx,
            &d.recipient_id,
            NOTIFICATION_KIND,
            &title,
            &body,
            &link,
            Some(&d.project_id),
        )
        .await?;
    }
    tx.commit().await?;
    Ok(())
}
