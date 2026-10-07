//! Server-side authorization. Pure functions over an `Actor` plus rows loaded
//! fresh from the database on EVERY call — membership removal takes effect
//! immediately, old links stop working at once.

use axum::extract::FromRequestParts;
use axum::http::request::Parts;

use crate::error::{AppError, AppResult};

pub const STAFF_ROLES: [&str; 5] = [
    "coordinator",
    "decision_maker",
    "base_manager",
    "finance",
    "admin",
];

#[derive(Debug, Clone)]
pub struct Actor {
    pub user_id: String,
    pub email: String,
    pub name: String,
    pub roles: Vec<String>,
    pub mfa_verified: bool,
    pub demo: bool,
}

impl Actor {
    pub fn has_role(&self, role: &str) -> bool {
        self.roles.iter().any(|r| r == role)
    }

    pub fn is_staff(&self) -> bool {
        self.roles.iter().any(|r| STAFF_ROLES.contains(&r.as_str()))
    }

    pub fn is_expert(&self) -> bool {
        self.has_role("expert")
    }

    pub fn is_coordinator(&self) -> bool {
        self.has_role("coordinator")
    }

    /// Staff members and experts must complete TOTP MFA before any non-auth
    /// endpoint works (enforced by the MFA gate middleware).
    pub fn needs_mfa(&self) -> bool {
        self.roles.iter().any(|r| role_requires_mfa(r)) && !self.mfa_verified
    }
}

/// Staff roles and `expert` require TOTP MFA (§3).
pub fn role_requires_mfa(role: &str) -> bool {
    role == "expert" || STAFF_ROLES.contains(&role)
}

/// Require one of the given roles; 403 otherwise.
pub fn require_role(actor: &Actor, roles: &[&str]) -> AppResult<()> {
    if roles.iter().any(|r| actor.has_role(r)) {
        Ok(())
    } else {
        Err(AppError::Forbidden {
            code: "forbidden".into(),
            message: format!("requires role: {}", roles.join(" or ")),
        })
    }
}

/// Access level of an actor to a project, ordered from most to least
/// privileged. The highest applicable level wins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ProjectAccess {
    None,
    /// Public catalog view (generalized sites, published results only).
    Public,
    /// Assigned expert (non-declined assignment): application + non-personal docs.
    Expert,
    TeamViewer,
    TeamEditor,
    TeamLead,
    /// Any staff role. Fine-grained differences (e.g. base_manager sees no
    /// documents) are handled by `can_view_document_category`.
    Staff,
}

/// Load the actor's access to a project fresh from the database. Call on
/// every request; never cache.
pub async fn project_access<'e, E>(
    exec: E,
    actor: &Actor,
    project_id: &str,
) -> AppResult<ProjectAccess>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    // One round trip: active membership + non-declined expert assignment.
    // An assignment only counts while the user still holds the expert role.
    let (member_role, expert_count): (Option<String>, i64) = sqlx::query_as(
        "SELECT (SELECT role FROM project_members
                  WHERE project_id = ? AND user_id = ? AND removed_at IS NULL LIMIT 1),
                (SELECT COUNT(*) FROM review_assignments
                  WHERE project_id = ? AND expert_id = ? AND status != 'declined')",
    )
    .bind(project_id)
    .bind(&actor.user_id)
    .bind(project_id)
    .bind(&actor.user_id)
    .fetch_one(exec)
    .await?;
    if let Some(role) = member_role {
        return Ok(match role.as_str() {
            "lead" => ProjectAccess::TeamLead,
            "editor" => ProjectAccess::TeamEditor,
            _ => ProjectAccess::TeamViewer,
        });
    }
    if actor.is_staff() {
        return Ok(ProjectAccess::Staff);
    }
    if expert_count > 0 && actor.is_expert() {
        return Ok(ProjectAccess::Expert);
    }
    Ok(ProjectAccess::None)
}

/// May the actor read documents of `category` on a project where they hold
/// `access`? `personal` (passports, insurance, CVs): team editors+ and the
/// coordinator only — never experts, never base managers. Non-personal:
/// team viewers+, assigned experts, and staff EXCEPT a base_manager without
/// other staff roles (summary only, no documents).
pub fn can_view_document_category(actor: &Actor, access: ProjectAccess, category: &str) -> bool {
    match access {
        ProjectAccess::TeamLead | ProjectAccess::TeamEditor => true,
        ProjectAccess::TeamViewer => category != "personal",
        ProjectAccess::Expert => category != "personal",
        ProjectAccess::Staff => {
            if category == "personal" {
                actor.is_coordinator()
            } else {
                // base_manager-only staff get a summary view, no documents.
                actor
                    .roles
                    .iter()
                    .any(|r| r != "base_manager" && STAFF_ROLES.contains(&r.as_str()))
            }
        }
        ProjectAccess::Public | ProjectAccess::None => false,
    }
}

/// May the actor see precise (non-generalized) sensitive site geometries?
/// Only active team members, staff and assigned experts; everyone else gets
/// the 0.1°-grid generalized bbox.
pub fn can_see_precise_location(access: ProjectAccess) -> bool {
    access >= ProjectAccess::Expert
}

/// Ensure a document version belongs to a document of `project_id`
/// (a guessed ID from another project must never grant access).
pub async fn ensure_document_version_in_project<'e, E>(
    exec: E,
    document_version_id: &str,
    project_id: &str,
) -> AppResult<()>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    let owner: Option<String> = sqlx::query_scalar(
        "SELECT d.project_id FROM document_versions dv
         JOIN documents d ON d.id = dv.document_id WHERE dv.id = ?",
    )
    .bind(document_version_id)
    .fetch_optional(exec)
    .await?;
    match owner {
        Some(pid) if pid == project_id => Ok(()),
        Some(_) => Err(AppError::forbidden(
            "document version belongs to a different project",
        )),
        None => Err(AppError::NotFound),
    }
}

/// Only the uploader (or staff) may attach a `file_id` to a document.
pub async fn ensure_file_owned_by<'e, E>(exec: E, actor: &Actor, file_id: &str) -> AppResult<()>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    let uploader: Option<String> = sqlx::query_scalar("SELECT uploaded_by FROM files WHERE id = ?")
        .bind(file_id)
        .fetch_optional(exec)
        .await?;
    match uploader {
        None => Err(AppError::NotFound),
        Some(u) if u == actor.user_id || actor.is_staff() => Ok(()),
        Some(_) => Err(AppError::forbidden("file belongs to another user")),
    }
}

// ---------------------------------------------------------------------------
// Extractor
// ---------------------------------------------------------------------------

pub const SESSION_COOKIE: &str = "pitcairn_session";

impl FromRequestParts<crate::AppState> for Actor {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &crate::AppState,
    ) -> Result<Self, Self::Rejection> {
        let token = parts
            .headers
            .get(axum::http::header::COOKIE)
            .and_then(|v| v.to_str().ok())
            .and_then(|cookies| {
                cookies.split(';').map(str::trim).find_map(|c| {
                    c.strip_prefix(&format!("{SESSION_COOKIE}="))
                        .map(str::to_string)
                })
            })
            .ok_or(AppError::Unauthorized)?;
        load_actor(state, &token).await
    }
}

/// Resolve a raw session token to an Actor (401 on invalid/expired/disabled).
#[allow(clippy::type_complexity)]
pub async fn load_actor(state: &crate::AppState, token: &str) -> AppResult<Actor> {
    let session_id = crate::util::sha256_hex(token.as_bytes());
    let row: Option<(String, String, String, String, i64, String, Option<String>)> =
        sqlx::query_as(
            "SELECT s.user_id, u.email, u.name, u.organisation, s.mfa_verified, s.expires_at, u.disabled_at
             FROM sessions s JOIN users u ON u.id = s.user_id WHERE s.id = ?",
        )
        .bind(&session_id)
        .fetch_optional(&state.pool)
        .await?;
    let (user_id, email, name, _org, mfa, expires_at, disabled_at) =
        row.ok_or(AppError::Unauthorized)?;
    if disabled_at.is_some() {
        return Err(AppError::Unauthorized);
    }
    let expired = crate::util::parse_time(&expires_at)
        .map(|t| t < chrono::Utc::now())
        .unwrap_or(true);
    if expired {
        return Err(AppError::Unauthorized);
    }
    let roles: Vec<String> =
        sqlx::query_scalar("SELECT role FROM user_roles WHERE user_id = ? AND revoked_at IS NULL")
            .bind(&user_id)
            .fetch_all(&state.pool)
            .await?;
    Ok(Actor {
        user_id,
        email,
        name,
        roles,
        mfa_verified: mfa != 0,
        demo: state.config.demo_mode,
    })
}

// ---------------------------------------------------------------------------
// Tests: every deny path
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn actor(roles: &[&str]) -> Actor {
        Actor {
            user_id: "u1".into(),
            email: "u@example.invalid".into(),
            name: "U".into(),
            roles: roles.iter().map(|s| s.to_string()).collect(),
            mfa_verified: true,
            demo: false,
        }
    }

    #[test]
    fn require_role_denies_without_role() {
        assert!(require_role(&actor(&["finance"]), &["admin"]).is_err());
        assert!(require_role(&actor(&[]), &["coordinator"]).is_err());
        assert!(require_role(&actor(&["admin"]), &["admin"]).is_ok());
    }

    #[test]
    fn personal_documents_denied_to_experts_viewers_and_non_coordinator_staff() {
        let expert = actor(&["expert"]);
        assert!(!can_view_document_category(
            &expert,
            ProjectAccess::Expert,
            "personal"
        ));
        let viewer = actor(&[]);
        assert!(!can_view_document_category(
            &viewer,
            ProjectAccess::TeamViewer,
            "personal"
        ));
        let base_manager = actor(&["base_manager"]);
        assert!(!can_view_document_category(
            &base_manager,
            ProjectAccess::Staff,
            "personal"
        ));
        let finance = actor(&["finance"]);
        assert!(!can_view_document_category(
            &finance,
            ProjectAccess::Staff,
            "personal"
        ));
        let admin = actor(&["admin"]);
        assert!(!can_view_document_category(
            &admin,
            ProjectAccess::Staff,
            "personal"
        ));
        // allowed: team editor, team lead, coordinator
        assert!(can_view_document_category(
            &actor(&[]),
            ProjectAccess::TeamEditor,
            "personal"
        ));
        assert!(can_view_document_category(
            &actor(&[]),
            ProjectAccess::TeamLead,
            "personal"
        ));
        assert!(can_view_document_category(
            &actor(&["coordinator"]),
            ProjectAccess::Staff,
            "personal"
        ));
    }

    #[test]
    fn base_manager_only_staff_sees_no_documents() {
        let bm = actor(&["base_manager"]);
        assert!(!can_view_document_category(
            &bm,
            ProjectAccess::Staff,
            "application"
        ));
        // base_manager + coordinator does
        let both = actor(&["base_manager", "coordinator"]);
        assert!(can_view_document_category(
            &both,
            ProjectAccess::Staff,
            "application"
        ));
    }

    #[test]
    fn non_personal_documents_denied_to_outsiders_and_public() {
        let anyone = actor(&[]);
        assert!(!can_view_document_category(
            &anyone,
            ProjectAccess::None,
            "application"
        ));
        assert!(!can_view_document_category(
            &anyone,
            ProjectAccess::Public,
            "result"
        ));
        assert!(can_view_document_category(
            &anyone,
            ProjectAccess::TeamViewer,
            "application"
        ));
        assert!(can_view_document_category(
            &actor(&["expert"]),
            ProjectAccess::Expert,
            "result"
        ));
    }

    #[test]
    fn precise_location_denied_to_public_and_outsiders() {
        assert!(!can_see_precise_location(ProjectAccess::None));
        assert!(!can_see_precise_location(ProjectAccess::Public));
        assert!(can_see_precise_location(ProjectAccess::Expert));
        assert!(can_see_precise_location(ProjectAccess::TeamViewer));
        assert!(can_see_precise_location(ProjectAccess::Staff));
    }

    #[test]
    fn staff_and_expert_need_mfa() {
        let mut a = actor(&["finance"]);
        a.mfa_verified = false;
        assert!(a.needs_mfa());
        let mut e = actor(&["expert"]);
        e.mfa_verified = false;
        assert!(e.needs_mfa());
        let researcher = actor(&[]);
        assert!(!researcher.needs_mfa());
        let mut verified = actor(&["admin"]);
        verified.mfa_verified = true;
        assert!(!verified.needs_mfa());
    }
}
