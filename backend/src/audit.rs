use serde_json::Value;

use crate::error::AppResult;

/// One audit record. `before`/`after` are optional JSON snapshots; `reason` is
/// required by convention (enforced by callers) for corrections of issued data.
pub struct AuditEvent {
    pub actor_id: Option<String>,
    pub actor_label: String,
    pub action: String,
    pub entity_type: String,
    pub entity_id: String,
    pub project_id: Option<String>,
    pub visibility: String, // "shared" | "internal"
    pub summary: String,
    pub before: Option<Value>,
    pub after: Option<Value>,
    pub reason: Option<String>,
}

/// Record an audit event inside an existing transaction. Every significant
/// change writes business rows + audit + notifications + jobs in ONE tx.
pub async fn record(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    event: AuditEvent,
) -> AppResult<()> {
    let id = crate::util::new_id();
    let now = crate::util::now_rfc3339();
    sqlx::query(
        "INSERT INTO audit_events
         (id, at, actor_id, actor_label, action, entity_type, entity_id, project_id,
          visibility, summary, before_json, after_json, reason, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&now)
    .bind(&event.actor_id)
    .bind(&event.actor_label)
    .bind(&event.action)
    .bind(&event.entity_type)
    .bind(&event.entity_id)
    .bind(&event.project_id)
    .bind(&event.visibility)
    .bind(&event.summary)
    .bind(event.before.as_ref().map(|v| v.to_string()))
    .bind(event.after.as_ref().map(|v| v.to_string()))
    .bind(&event.reason)
    .bind(&now)
    .execute(&mut **tx)
    .await?;
    Ok(())
}
