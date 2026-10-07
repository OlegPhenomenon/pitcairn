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

/// Ids of all active members of a project team.
pub async fn team_member_ids<'e, E>(exec: E, project_id: &str) -> AppResult<Vec<String>>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    Ok(sqlx::query_scalar(
        "SELECT user_id FROM project_members WHERE project_id = ? AND removed_at IS NULL",
    )
    .bind(project_id)
    .fetch_all(exec)
    .await?)
}

/// Ids of all users holding the coordinator role.
pub async fn coordinator_ids<'e, E>(exec: E) -> AppResult<Vec<String>>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    Ok(sqlx::query_scalar(
        "SELECT DISTINCT user_id FROM user_roles WHERE role = 'coordinator' AND revoked_at IS NULL",
    )
    .fetch_all(exec)
    .await?)
}

/// The staff side of a project's internal conversation: every coordinator,
/// decision_maker, base_manager and finance user plus the experts assigned to
/// this project (non-declined). `exclude` drops the actor themselves.
pub async fn internal_audience_ids<'e, E>(
    exec: E,
    project_id: &str,
    exclude: Option<&str>,
) -> AppResult<Vec<String>>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    let mut ids: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT user_id FROM user_roles
         WHERE role IN ('coordinator','decision_maker','base_manager','finance')
           AND revoked_at IS NULL
         UNION
         SELECT expert_id FROM review_assignments
         WHERE project_id = ? AND status != 'declined'",
    )
    .bind(project_id)
    .fetch_all(exec)
    .await?;
    if let Some(me) = exclude {
        ids.retain(|id| id != me);
    }
    Ok(ids)
}

/// Notify each of `user_ids` (deduplicated) inside `tx`.
pub async fn notify_users(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    user_ids: Vec<String>,
    kind: &str,
    title: &str,
    body: &str,
    link: &str,
    project_id: Option<&str>,
) -> AppResult<()> {
    let mut seen = std::collections::HashSet::new();
    for user_id in user_ids {
        if seen.insert(user_id.clone()) {
            notify(tx, &user_id, kind, title, body, link, project_id).await?;
        }
    }
    Ok(())
}

/// Notify the whole team of `project_id` inside `tx`.
pub async fn notify_team(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    project_id: &str,
    kind: &str,
    title: &str,
    body: &str,
) -> AppResult<()> {
    let ids = team_member_ids(&mut **tx, project_id).await?;
    let link = format!("/app/projects/{project_id}");
    notify_users(tx, ids, kind, title, body, &link, Some(project_id)).await
}

/// Notify all coordinators inside `tx`.
pub async fn notify_coordinators(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    project_id: &str,
    kind: &str,
    title: &str,
    body: &str,
) -> AppResult<()> {
    let ids = coordinator_ids(&mut **tx).await?;
    let link = format!("/app/projects/{project_id}");
    notify_users(tx, ids, kind, title, body, &link, Some(project_id)).await
}
