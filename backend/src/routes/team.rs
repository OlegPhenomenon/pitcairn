//! Project team: members, invitations, role changes, lead transfer, removal
//! (§3 team roles, §5 team endpoints). Invitation acceptance lives in
//! `routes::auth` (`POST /invitations/{token}/accept`).

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use serde_json::json;
use sqlx::FromRow;

use crate::AppState;
use crate::audit::{self, AuditEvent};
use crate::authz::{Actor, ProjectAccess};
use crate::db;
use crate::dto::a::{InvitationDto, InviteRequest, MemberRoleRequest, MembersResponse};
use crate::error::{AppError, AppResult};
use crate::routes::projects::{require_project_access, require_team_lead, team_members};
use crate::util::{new_id, now_rfc3339, random_token, sha256_hex, time_plus_secs};
use crate::validation::FieldErrors;
use crate::{jobs, mail, notify};

const INVITATION_TTL_SECS: i64 = 14 * 24 * 3600;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/projects/{id}/members", get(list_members))
        .route("/projects/{id}/invitations", post(invite))
        .route(
            "/projects/{id}/members/{user_id}",
            patch(change_role).delete(remove_member),
        )
        .route(
            "/projects/{id}/members/{user_id}/make-lead",
            post(make_lead),
        )
}

fn member_event(
    actor: &Actor,
    action: &str,
    project_id: &str,
    entity_id: &str,
    summary: String,
    before: Option<serde_json::Value>,
    after: Option<serde_json::Value>,
) -> AuditEvent {
    AuditEvent {
        actor_id: Some(actor.user_id.clone()),
        actor_label: actor.name.clone(),
        action: action.into(),
        entity_type: "project_member".into(),
        entity_id: entity_id.into(),
        project_id: Some(project_id.into()),
        visibility: "shared".into(),
        summary,
        before,
        after,
        reason: None,
    }
}

async fn list_members(
    State(state): State<AppState>,
    actor: Actor,
    Path(project_id): Path<String>,
) -> AppResult<Json<MembersResponse>> {
    let access = require_project_access(&state, &actor, &project_id).await?;
    let members = team_members(&state.pool, &project_id, true).await?;
    let invitations = if access == ProjectAccess::TeamLead || actor.is_coordinator() {
        #[derive(FromRow)]
        struct Row {
            id: String,
            email: String,
            role: String,
            invited_by: String,
            invited_by_name: String,
            expires_at: String,
            accepted_at: Option<String>,
            revoked_at: Option<String>,
            created_at: String,
        }
        let rows: Vec<Row> = sqlx::query_as(
            "SELECT i.id, i.email, i.role, i.invited_by, u.name AS invited_by_name,
                    i.expires_at, i.accepted_at, i.revoked_at, i.created_at
             FROM invitations i JOIN users u ON u.id = i.invited_by
             WHERE i.project_id = ? ORDER BY i.created_at DESC",
        )
        .bind(&project_id)
        .fetch_all(&state.pool)
        .await?;
        rows.into_iter()
            .map(|r| InvitationDto {
                id: r.id,
                email: r.email,
                role: r.role,
                invited_by: r.invited_by,
                invited_by_name: r.invited_by_name,
                accept_url: None,
                expires_at: r.expires_at,
                accepted_at: r.accepted_at,
                revoked_at: r.revoked_at,
                created_at: r.created_at,
            })
            .collect()
    } else {
        Vec::new()
    };
    Ok(Json(MembersResponse {
        members,
        invitations,
    }))
}

fn validate_member_role(errors: &mut FieldErrors, role: &str) {
    errors.check(
        "role",
        matches!(role, "editor" | "viewer"),
        "must be editor or viewer (use make-lead to transfer the lead role)",
    );
}

async fn invite(
    State(state): State<AppState>,
    actor: Actor,
    Path(project_id): Path<String>,
    Json(req): Json<InviteRequest>,
) -> AppResult<impl IntoResponse> {
    let access = require_project_access(&state, &actor, &project_id).await?;
    require_team_lead(access)?;
    let email = req.email.trim().to_lowercase();
    let mut errors = FieldErrors::new();
    errors.check(
        "email",
        email.len() <= 254
            && email
                .split_once('@')
                .is_some_and(|(a, d)| !a.is_empty() && d.contains('.')),
        "must be a valid email address",
    );
    validate_member_role(&mut errors, &req.role);
    errors.finish()?;

    let mut tx = db::begin_immediate(&state.pool).await?;
    let already: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM project_members pm JOIN users u ON u.id = pm.user_id
         WHERE pm.project_id = ? AND pm.removed_at IS NULL AND u.email = ?",
    )
    .bind(&project_id)
    .bind(&email)
    .fetch_one(&mut *tx)
    .await?;
    if already > 0 {
        return Err(AppError::conflict(
            "already_member",
            "this person is already a member of the team",
        ));
    }
    // A new invitation replaces any pending one for the same address.
    let now = now_rfc3339();
    sqlx::query(
        "UPDATE invitations SET revoked_at = ?
         WHERE project_id = ? AND email = ? AND accepted_at IS NULL AND revoked_at IS NULL",
    )
    .bind(&now)
    .bind(&project_id)
    .bind(&email)
    .execute(&mut *tx)
    .await?;

    let token = random_token();
    let id = new_id();
    let expires_at = time_plus_secs(INVITATION_TTL_SECS);
    sqlx::query(
        "INSERT INTO invitations (id, project_id, email, role, token_hash, invited_by, expires_at, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&project_id)
    .bind(&email)
    .bind(&req.role)
    .bind(sha256_hex(token.as_bytes()))
    .bind(&actor.user_id)
    .bind(&expires_at)
    .bind(&now)
    .execute(&mut *tx)
    .await?;

    let title: String = sqlx::query_scalar("SELECT title FROM projects WHERE id = ?")
        .bind(&project_id)
        .fetch_one(&mut *tx)
        .await?;
    let accept_url = format!(
        "{}/invite/{token}",
        state.config.base_url.trim_end_matches('/')
    );
    let subject = format!("{} invited you to join \"{title}\"", actor.name);
    let body = format!(
        "{} invited you to join the project \"{title}\" as {}.\n\nSign in (or register) with this email address and open the link to accept. The invitation expires on {}.",
        actor.name,
        req.role,
        &expires_at[0..10]
    );
    let existing_user: Option<String> = sqlx::query_scalar("SELECT id FROM users WHERE email = ?")
        .bind(&email)
        .fetch_optional(&mut *tx)
        .await?;
    match existing_user {
        // In-app notification + mail (notify always writes both).
        Some(user_id) => {
            notify::notify(
                &mut tx,
                &user_id,
                "team_invitation",
                &subject,
                &body,
                &accept_url,
                Some(&project_id),
            )
            .await?
        }
        None => {
            let message_id = mail::queue_message(
                &mut tx,
                &email,
                &subject,
                &format!("{body}\n\n{accept_url}"),
            )
            .await?;
            jobs::enqueue(
                &mut tx,
                jobs::KIND_SEND_EMAIL,
                json!({"mail_message_id": message_id}),
                Some(&format!("send_email:{message_id}")),
            )
            .await?;
        }
    }
    audit::record(
        &mut tx,
        AuditEvent {
            entity_type: "invitation".into(),
            ..member_event(
                &actor,
                "team.invited",
                &project_id,
                &id,
                format!("{} invited {email} as {}", actor.name, req.role),
                None,
                Some(json!({"email": email, "role": req.role})),
            )
        },
    )
    .await?;
    tx.commit().await?;

    Ok((
        StatusCode::CREATED,
        Json(InvitationDto {
            id,
            email,
            role: req.role,
            invited_by: actor.user_id.clone(),
            invited_by_name: actor.name.clone(),
            accept_url: Some(accept_url),
            expires_at,
            accepted_at: None,
            revoked_at: None,
            created_at: now,
        }),
    ))
}

/// Active membership `(member_row_id, role, name)` or 404.
async fn active_member(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    project_id: &str,
    user_id: &str,
) -> AppResult<(String, String, String)> {
    let row: Option<(String, String, String)> = sqlx::query_as(
        "SELECT pm.id, pm.role, u.name FROM project_members pm JOIN users u ON u.id = pm.user_id
         WHERE pm.project_id = ? AND pm.user_id = ? AND pm.removed_at IS NULL",
    )
    .bind(project_id)
    .bind(user_id)
    .fetch_optional(&mut **tx)
    .await?;
    row.ok_or(AppError::NotFound)
}

async fn change_role(
    State(state): State<AppState>,
    actor: Actor,
    Path((project_id, user_id)): Path<(String, String)>,
    Json(req): Json<MemberRoleRequest>,
) -> AppResult<Json<MembersResponse>> {
    let access = require_project_access(&state, &actor, &project_id).await?;
    require_team_lead(access)?;
    let mut errors = FieldErrors::new();
    validate_member_role(&mut errors, &req.role);
    errors.finish()?;

    let mut tx = db::begin_immediate(&state.pool).await?;
    let (member_id, old_role, name) = active_member(&mut tx, &project_id, &user_id).await?;
    if old_role == "lead" {
        return Err(AppError::conflict(
            "lead_role_change",
            "transfer the lead role with make-lead first",
        ));
    }
    if old_role != req.role {
        sqlx::query("UPDATE project_members SET role = ? WHERE id = ?")
            .bind(&req.role)
            .bind(&member_id)
            .execute(&mut *tx)
            .await?;
        audit::record(
            &mut tx,
            member_event(
                &actor,
                "team.role_changed",
                &project_id,
                &member_id,
                format!(
                    "{} changed {name}'s role from {old_role} to {}",
                    actor.name, req.role
                ),
                Some(json!({"user_id": user_id, "role": old_role})),
                Some(json!({"user_id": user_id, "role": req.role})),
            ),
        )
        .await?;
    }
    tx.commit().await?;
    list_members(State(state), actor, Path(project_id)).await
}

async fn make_lead(
    State(state): State<AppState>,
    actor: Actor,
    Path((project_id, user_id)): Path<(String, String)>,
) -> AppResult<Json<MembersResponse>> {
    let access = require_project_access(&state, &actor, &project_id).await?;
    require_team_lead(access)?;

    let mut tx = db::begin_immediate(&state.pool).await?;
    let (member_id, old_role, name) = active_member(&mut tx, &project_id, &user_id).await?;
    if old_role == "lead" {
        return Err(AppError::conflict(
            "already_lead",
            "this member is already the lead",
        ));
    }
    // Exactly one active lead: every current lead becomes an editor.
    sqlx::query(
        "UPDATE project_members SET role = 'editor'
         WHERE project_id = ? AND role = 'lead' AND removed_at IS NULL",
    )
    .bind(&project_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query("UPDATE project_members SET role = 'lead' WHERE id = ?")
        .bind(&member_id)
        .execute(&mut *tx)
        .await?;
    audit::record(
        &mut tx,
        member_event(
            &actor,
            "team.lead_transferred",
            &project_id,
            &member_id,
            format!(
                "{} made {name} the project lead ({} is now an editor)",
                actor.name, actor.name
            ),
            Some(json!({"lead": actor.user_id, "new_lead_previous_role": old_role})),
            Some(json!({"lead": user_id, "previous_lead_role": "editor"})),
        ),
    )
    .await?;
    notify::notify(
        &mut tx,
        &user_id,
        "team_lead",
        "You are now the project lead",
        &format!("{} transferred the lead role to you.", actor.name),
        &format!("/app/projects/{project_id}/team"),
        Some(&project_id),
    )
    .await?;
    tx.commit().await?;
    list_members(State(state), actor, Path(project_id)).await
}

/// Lead removes a member, or a non-lead member leaves. The lead cannot
/// remove themselves without transferring the lead first. Access ends
/// immediately (every request re-checks membership).
async fn remove_member(
    State(state): State<AppState>,
    actor: Actor,
    Path((project_id, user_id)): Path<(String, String)>,
) -> AppResult<StatusCode> {
    let access = require_project_access(&state, &actor, &project_id).await?;
    let is_self = user_id == actor.user_id;
    if !is_self {
        require_team_lead(access)?;
    } else if !crate::routes::projects::is_team(access) {
        return Err(AppError::forbidden("only team members can leave a team"));
    }

    let mut tx = db::begin_immediate(&state.pool).await?;
    let (member_id, role, name) = active_member(&mut tx, &project_id, &user_id).await?;
    if role == "lead" {
        return Err(AppError::conflict(
            "lead_must_transfer",
            "the lead cannot be removed; transfer the lead role first",
        ));
    }
    let now = now_rfc3339();
    sqlx::query("UPDATE project_members SET removed_at = ?, removed_by = ? WHERE id = ?")
        .bind(&now)
        .bind(&actor.user_id)
        .bind(&member_id)
        .execute(&mut *tx)
        .await?;
    let summary = if is_self {
        format!("{name} left the team")
    } else {
        format!("{} removed {name} from the team", actor.name)
    };
    audit::record(
        &mut tx,
        member_event(
            &actor,
            "team.member_removed",
            &project_id,
            &member_id,
            summary,
            Some(json!({"user_id": user_id, "role": role})),
            None,
        ),
    )
    .await?;
    if !is_self {
        notify::notify(
            &mut tx,
            &user_id,
            "team_removed",
            "You were removed from a project team",
            &format!("{} removed you from the project team.", actor.name),
            "",
            None,
        )
        .await?;
    }
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
