//! Project status state machine (architecture §4 transition table).
//!
//! Every slice changes `projects.status` ONLY through [`transition`]; who may
//! perform an action is checked by the handler via `authz` before calling it.

use crate::audit::{self, AuditEvent};
use crate::authz::Actor;
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Team editor+: first submission of a draft.
    Submit,
    /// Coordinator: opens a submitted application for screening/review.
    Screen,
    /// Coordinator: an action item addressed to the team was created.
    RequestChanges,
    /// Team editor+: resubmission after changes were requested.
    Resubmit,
    /// Decision maker: permit issued.
    Approve,
    /// Decision maker: refusal issued.
    Refuse,
    /// Coordinator: all deliverables resolved.
    Close,
    /// Team lead: withdraw before a decision.
    Withdraw,
}

impl Action {
    pub fn as_str(self) -> &'static str {
        match self {
            Action::Submit => "submit",
            Action::Screen => "screen",
            Action::RequestChanges => "request_changes",
            Action::Resubmit => "resubmit",
            Action::Approve => "approve",
            Action::Refuse => "refuse",
            Action::Close => "close",
            Action::Withdraw => "withdraw",
        }
    }
}

/// Pure transition table. `None` = not allowed from `from`.
pub fn next_status(from: &str, action: Action) -> Option<&'static str> {
    use Action::*;
    match (from, action) {
        ("draft", Submit) => Some("submitted"),
        ("submitted", Screen) => Some("in_review"),
        ("submitted" | "in_review", RequestChanges) => Some("changes_requested"),
        ("changes_requested", Resubmit) => Some("in_review"),
        ("in_review", Approve) => Some("approved"),
        ("in_review", Refuse) => Some("refused"),
        ("approved", Close) => Some("closed"),
        ("draft" | "submitted" | "in_review" | "changes_requested", Withdraw) => Some("withdrawn"),
        _ => None,
    }
}

/// Applies `action` to the project inside `tx`: checks the current status,
/// updates `status`, bumps `version`, and records a shared audit event.
/// Returns `(from, to)`. Errors: 404 unknown project, 409 `invalid_transition`.
pub async fn transition(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    project_id: &str,
    action: Action,
    actor: &Actor,
    reason: Option<String>,
) -> AppResult<(String, &'static str)> {
    let from: String = sqlx::query_scalar("SELECT status FROM projects WHERE id = ?")
        .bind(project_id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or(AppError::NotFound)?;
    let to = next_status(&from, action).ok_or_else(|| {
        AppError::conflict(
            "invalid_transition",
            format!(
                "Cannot {} a project that is {}",
                action.as_str().replace('_', " "),
                from.replace('_', " ")
            ),
        )
    })?;
    sqlx::query("UPDATE projects SET status = ?, version = version + 1 WHERE id = ?")
        .bind(to)
        .bind(project_id)
        .execute(&mut **tx)
        .await?;
    audit::record(
        tx,
        AuditEvent {
            actor_id: Some(actor.user_id.clone()),
            actor_label: actor.name.clone(),
            action: format!("project.{}", action.as_str()),
            entity_type: "project".into(),
            entity_id: project_id.to_string(),
            project_id: Some(project_id.to_string()),
            visibility: "shared".into(),
            summary: format!(
                "Status changed from {} to {}",
                from.replace('_', " "),
                to.replace('_', " ")
            ),
            before: Some(serde_json::json!({ "status": from })),
            after: Some(serde_json::json!({ "status": to })),
            reason,
        },
    )
    .await?;
    Ok((from, to))
}

#[cfg(test)]
mod tests {
    use super::*;
    use Action::*;

    #[test]
    fn allowed_transitions_follow_the_table() {
        assert_eq!(next_status("draft", Submit), Some("submitted"));
        assert_eq!(next_status("submitted", Screen), Some("in_review"));
        assert_eq!(
            next_status("in_review", RequestChanges),
            Some("changes_requested")
        );
        assert_eq!(
            next_status("changes_requested", Resubmit),
            Some("in_review")
        );
        assert_eq!(next_status("in_review", Approve), Some("approved"));
        assert_eq!(next_status("in_review", Refuse), Some("refused"));
        assert_eq!(next_status("approved", Close), Some("closed"));
        assert_eq!(
            next_status("changes_requested", Withdraw),
            Some("withdrawn")
        );
    }

    #[test]
    fn decisions_and_closure_cannot_skip_review() {
        assert_eq!(next_status("submitted", Approve), None);
        assert_eq!(next_status("changes_requested", Approve), None);
        assert_eq!(next_status("in_review", Close), None);
        assert_eq!(next_status("approved", Withdraw), None);
        assert_eq!(next_status("refused", Resubmit), None);
        assert_eq!(next_status("closed", Submit), None);
    }
}
