use std::future::Future;
use std::pin::Pin;

use sqlx::SqlitePool;

use crate::error::AppResult;

/// Outbound mail transport. All integrations are mocked; the demo transport
/// records every message in the `mail_messages` table (visible on the demo
/// Mailbox page) and fails while the `mail_enabled` setting is `false` so the
/// `send_email` job retries and shows up in Admin → Delivery issues.
pub trait MailTransport: Send + Sync {
    /// Deliver a previously queued `mail_messages` row: mark it `sent` (with
    /// `sent_at`) or `failed` (with `error`) and report the outcome.
    fn deliver<'a>(
        &'a self,
        message_id: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>>;
}

pub struct DemoMailbox {
    pool: SqlitePool,
}

impl DemoMailbox {
    pub fn new(pool: SqlitePool) -> Self {
        DemoMailbox { pool }
    }
}

impl MailTransport for DemoMailbox {
    fn deliver<'a>(
        &'a self,
        message_id: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
        Box::pin(async move {
            let enabled: Option<String> =
                sqlx::query_scalar("SELECT value FROM settings WHERE key = 'mail_enabled'")
                    .fetch_optional(&self.pool)
                    .await
                    .map_err(|e| format!("settings lookup failed: {e}"))?;
            if enabled.as_deref() == Some("false") {
                let now = crate::util::now_rfc3339();
                let _ = sqlx::query(
                    "UPDATE mail_messages SET status = 'failed', error = 'mail_enabled=false (demo mail outage)' WHERE id = ?",
                )
                .bind(message_id)
                .bind(&now)
                .execute(&self.pool)
                .await;
                return Err("mail transport disabled by setting mail_enabled=false".into());
            }
            let now = crate::util::now_rfc3339();
            sqlx::query(
                "UPDATE mail_messages SET status = 'sent', sent_at = ?, error = NULL WHERE id = ?",
            )
            .bind(&now)
            .bind(message_id)
            .execute(&self.pool)
            .await
            .map_err(|e| format!("db error: {e}"))?;
            Ok(())
        })
    }
}

/// Insert a queued `mail_messages` row inside an existing transaction and
/// return its id. The `send_email` job delivering it is enqueued in the same
/// transaction by the caller.
pub async fn queue_message(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    to_email: &str,
    subject: &str,
    body_text: &str,
) -> AppResult<String> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = crate::util::now_rfc3339();
    sqlx::query(
        "INSERT INTO mail_messages (id, to_email, subject, body_text, status, created_at)
         VALUES (?, ?, ?, ?, 'queued', ?)",
    )
    .bind(&id)
    .bind(to_email)
    .bind(subject)
    .bind(body_text)
    .bind(&now)
    .execute(&mut **tx)
    .await?;
    Ok(id)
}
