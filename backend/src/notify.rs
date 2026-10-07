//! In-app notification + email fan-out. Always writes the in-app
//! notification row (independent of mail), queues a `mail_messages` row and
//! enqueues a `send_email` job — all in the caller's transaction (§7).

use crate::error::AppResult;
use crate::{jobs, mail};

#[allow(clippy::too_many_arguments)]
pub async fn notify(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    user_id: &str,
    kind: &str,
    title: &str,
    body: &str,
    link: &str,
    project_id: Option<&str>,
) -> AppResult<()> {
    let id = crate::util::new_id();
    let now = crate::util::now_rfc3339();
    sqlx::query(
        "INSERT INTO notifications (id, user_id, project_id, kind, title, body, link, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(user_id)
    .bind(project_id)
    .bind(kind)
    .bind(title)
    .bind(body)
    .bind(link)
    .bind(&now)
    .execute(&mut **tx)
    .await?;

    let email: Option<String> = sqlx::query_scalar("SELECT email FROM users WHERE id = ?")
        .bind(user_id)
        .fetch_optional(&mut **tx)
        .await?;
    if let Some(email) = email {
        let mail_body = if link.is_empty() {
            body.to_string()
        } else {
            format!("{body}\n\n{link}")
        };
        let message_id = mail::queue_message(tx, &email, title, &mail_body).await?;
        jobs::enqueue(
            tx,
            jobs::KIND_SEND_EMAIL,
            serde_json::json!({"mail_message_id": message_id}),
            Some(&format!("send_email:{message_id}")),
        )
        .await?;
    }
    Ok(())
}
