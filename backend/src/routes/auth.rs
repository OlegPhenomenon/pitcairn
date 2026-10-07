#![allow(clippy::type_complexity)]

use axum::extract::{ConnectInfo, Path, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use rand::RngCore;
use sqlx::SqlitePool;

use crate::AppState;
use crate::authz::{self, Actor};
use crate::dto::{
    AcceptInvitationResponse, LoginRequest, LoginResponse, MeResponse, MfaCodeRequest,
    MfaEnrollResponse, RegisterRequest, UserDto,
};
use crate::error::{AppError, AppResult};
use crate::util::{new_id, now_rfc3339, random_token, sha256_hex, time_plus_secs};
use crate::validation::FieldErrors;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/auth/register", post(register))
        .route("/auth/login", post(login))
        .route("/auth/logout", post(logout))
        .route("/auth/me", get(me))
        .route("/auth/mfa/enroll", post(mfa_enroll))
        .route("/auth/mfa/enroll/confirm", post(mfa_enroll_confirm))
        .route("/auth/mfa/verify", post(mfa_verify))
        .route("/invitations/{token}/accept", post(accept_invitation))
}

pub async fn load_user_dto(pool: &SqlitePool, user_id: &str) -> AppResult<UserDto> {
    let row: Option<(
        String,
        String,
        String,
        String,
        Option<String>,
        Option<String>,
    )> = sqlx::query_as(
        "SELECT id, email, name, organisation, totp_secret, disabled_at FROM users WHERE id = ?",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await?;
    let (id, email, name, organisation, totp_secret, disabled_at) =
        row.ok_or(AppError::NotFound)?;
    let roles: Vec<String> =
        sqlx::query_scalar("SELECT role FROM user_roles WHERE user_id = ? AND revoked_at IS NULL")
            .bind(user_id)
            .fetch_all(pool)
            .await?;
    Ok(UserDto {
        id,
        email,
        name,
        organisation,
        roles,
        totp_enrolled: totp_secret.is_some(),
        disabled: disabled_at.is_some(),
    })
}

fn cookie_header(token: &str, secure: bool, max_age: i64) -> String {
    let mut cookie = format!(
        "{}={}; HttpOnly; SameSite=Lax; Path=/; Max-Age={}",
        authz::SESSION_COOKIE,
        token,
        max_age
    );
    if secure {
        cookie.push_str("; Secure");
    }
    cookie
}

fn clear_cookie_header() -> String {
    format!(
        "{}=; HttpOnly; SameSite=Lax; Path=/; Max-Age=0",
        authz::SESSION_COOKIE
    )
}

async fn create_session(pool: &SqlitePool, user_id: &str, mfa_verified: i64) -> AppResult<String> {
    let token = random_token();
    let session_id = sha256_hex(token.as_bytes());
    let now = now_rfc3339();
    let expires = time_plus_secs(30 * 24 * 3600);
    sqlx::query(
        "INSERT INTO sessions (id, user_id, created_at, expires_at, mfa_verified, via_demo_switch)
         VALUES (?, ?, ?, ?, ?, 0)",
    )
    .bind(&session_id)
    .bind(user_id)
    .bind(&now)
    .bind(&expires)
    .bind(mfa_verified)
    .execute(pool)
    .await?;
    Ok(token)
}

async fn register(
    State(state): State<AppState>,
    Json(req): Json<RegisterRequest>,
) -> AppResult<impl IntoResponse> {
    let mut errors = FieldErrors::new();
    errors.check(
        "email",
        req.email.contains('@') && req.email.len() <= 254,
        "must be a valid email at most 254 characters",
    );
    errors.require("name", &req.name, "name is required");
    errors.max_len("name", &req.name, 200);
    errors.check(
        "password",
        (8..=200).contains(&req.password.len()),
        "must be 8–200 characters",
    );
    errors.finish()?;

    let existing: Option<(String,)> = sqlx::query_as("SELECT id FROM users WHERE email = ?")
        .bind(&req.email)
        .fetch_optional(&state.pool)
        .await?;
    if existing.is_some() {
        return Err(AppError::conflict(
            "email_taken",
            "an account with this email already exists",
        ));
    }

    let user_id = new_id();
    let now = now_rfc3339();
    let password_hash = crate::password::hash_password(&req.password)?;
    sqlx::query(
        "INSERT INTO users (id, email, name, organisation, password_hash, created_at)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(&user_id)
    .bind(&req.email)
    .bind(&req.name)
    .bind(&req.organisation)
    .bind(&password_hash)
    .bind(&now)
    .execute(&state.pool)
    .await?;

    let token = create_session(&state.pool, &user_id, 1).await?;
    let user = load_user_dto(&state.pool, &user_id).await?;
    Ok((
        StatusCode::OK,
        [(
            header::SET_COOKIE,
            cookie_header(&token, state.config.secure_cookies, 2_592_000),
        )],
        Json(LoginResponse {
            user,
            mfa_required: false,
            mfa_enrollment_required: false,
        }),
    ))
}

async fn login(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<std::net::SocketAddr>,
    Json(req): Json<LoginRequest>,
) -> AppResult<impl IntoResponse> {
    let key = format!("{}|{}", addr.ip(), req.email);
    if !state.login_limiter.allow(&key) {
        return Err(AppError::TooManyRequests(
            "too many login attempts, try again later".into(),
        ));
    }

    let row: Option<(
        String,
        String,
        String,
        String,
        String,
        Option<String>,
        Option<String>,
    )> = sqlx::query_as(
        "SELECT id, email, name, organisation, password_hash, totp_secret, disabled_at
             FROM users WHERE email = ?",
    )
    .bind(&req.email)
    .fetch_optional(&state.pool)
    .await?;
    let (user_id, _email, _name, _organisation, password_hash, totp_secret, disabled_at) = match row
    {
        Some(r) => r,
        None => {
            return Err(AppError::auth_failed(
                "invalid_credentials",
                "invalid email or password",
            ));
        }
    };

    if disabled_at.is_some() {
        return Err(AppError::Forbidden {
            code: "account_disabled".into(),
            message: "account disabled".into(),
        });
    }
    if !crate::password::verify_password(&password_hash, &req.password) {
        return Err(AppError::auth_failed(
            "invalid_credentials",
            "invalid email or password",
        ));
    }

    let roles: Vec<String> =
        sqlx::query_scalar("SELECT role FROM user_roles WHERE user_id = ? AND revoked_at IS NULL")
            .bind(&user_id)
            .fetch_all(&state.pool)
            .await?;
    let requires_mfa = roles.iter().any(|r| {
        matches!(
            r.as_str(),
            "coordinator" | "decision_maker" | "base_manager" | "finance" | "admin" | "expert"
        )
    });
    let mfa_verified = if requires_mfa { 0 } else { 1 };
    let token = create_session(&state.pool, &user_id, mfa_verified).await?;
    let user = load_user_dto(&state.pool, &user_id).await?;

    Ok((
        StatusCode::OK,
        [(
            header::SET_COOKIE,
            cookie_header(&token, state.config.secure_cookies, 2_592_000),
        )],
        Json(LoginResponse {
            user,
            mfa_required: requires_mfa,
            mfa_enrollment_required: requires_mfa && totp_secret.is_none(),
        }),
    ))
}

async fn logout(
    State(state): State<AppState>,
    _actor: Actor,
    headers: HeaderMap,
) -> AppResult<impl IntoResponse> {
    if let Some(token) = crate::session_token(&headers) {
        let session_id = sha256_hex(token.as_bytes());
        sqlx::query("DELETE FROM sessions WHERE id = ?")
            .bind(&session_id)
            .execute(&state.pool)
            .await?;
    }
    Ok((
        StatusCode::NO_CONTENT,
        [(header::SET_COOKIE, clear_cookie_header())],
    ))
}

async fn me(State(state): State<AppState>, actor: Actor) -> AppResult<Json<MeResponse>> {
    let user = load_user_dto(&state.pool, &actor.user_id).await?;
    Ok(Json(MeResponse {
        user,
        mfa_verified: actor.mfa_verified,
        demo_mode: actor.demo,
    }))
}

async fn mfa_enroll(
    State(state): State<AppState>,
    actor: Actor,
) -> AppResult<Json<MfaEnrollResponse>> {
    let row: Option<(Option<String>, Option<String>)> =
        sqlx::query_as("SELECT totp_secret, totp_enabled_at FROM users WHERE id = ?")
            .bind(&actor.user_id)
            .fetch_optional(&state.pool)
            .await?;
    let (secret_opt, enabled_opt) = row.ok_or(AppError::NotFound)?;
    if secret_opt.is_some() && enabled_opt.is_some() {
        return Err(AppError::conflict(
            "already_enrolled",
            "MFA is already enrolled",
        ));
    }

    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    let secret = totp_rs::Secret::Raw(bytes.to_vec());
    let base32 = secret.to_encoded().to_string();
    let secret_bytes = secret.to_bytes().map_err(AppError::internal)?;
    sqlx::query("UPDATE users SET totp_secret = ? WHERE id = ?")
        .bind(&base32)
        .bind(&actor.user_id)
        .execute(&state.pool)
        .await?;

    let totp = totp_rs::TOTP::new(
        totp_rs::Algorithm::SHA1,
        6,
        1,
        30,
        secret_bytes,
        Some("Pitcairn Research Hub".to_string()),
        actor.email,
    )
    .map_err(AppError::internal)?;

    Ok(Json(MfaEnrollResponse {
        secret: base32,
        otpauth_url: totp.get_url(),
    }))
}

async fn mfa_enroll_confirm(
    State(state): State<AppState>,
    actor: Actor,
    headers: HeaderMap,
    Json(req): Json<MfaCodeRequest>,
) -> AppResult<Json<MeResponse>> {
    let row: Option<(Option<String>, Option<String>)> =
        sqlx::query_as("SELECT totp_secret, totp_enabled_at FROM users WHERE id = ?")
            .bind(&actor.user_id)
            .fetch_optional(&state.pool)
            .await?;
    let (secret_opt, enabled_opt) = row.ok_or(AppError::NotFound)?;
    if secret_opt.is_none() || enabled_opt.is_some() {
        return Err(AppError::conflict(
            "already_enrolled",
            "MFA is already enrolled or was not started",
        ));
    }
    let secret_str = secret_opt.unwrap();
    let secret = totp_rs::Secret::Encoded(secret_str);
    let secret_bytes = secret.to_bytes().map_err(AppError::internal)?;
    let totp = totp_rs::TOTP::new(
        totp_rs::Algorithm::SHA1,
        6,
        1,
        30,
        secret_bytes,
        Some("Pitcairn Research Hub".to_string()),
        actor.email.clone(),
    )
    .map_err(AppError::internal)?;
    if !totp.check_current(&req.code).map_err(AppError::internal)? {
        return Err(AppError::auth_failed(
            "invalid_totp",
            "invalid verification code",
        ));
    }

    let now = now_rfc3339();
    sqlx::query("UPDATE users SET totp_enabled_at = ? WHERE id = ?")
        .bind(&now)
        .bind(&actor.user_id)
        .execute(&state.pool)
        .await?;
    verify_current_session(&state, &headers).await?;

    let user = load_user_dto(&state.pool, &actor.user_id).await?;
    Ok(Json(MeResponse {
        user,
        mfa_verified: true,
        demo_mode: actor.demo,
    }))
}

async fn mfa_verify(
    State(state): State<AppState>,
    actor: Actor,
    headers: HeaderMap,
    Json(req): Json<MfaCodeRequest>,
) -> AppResult<Json<MeResponse>> {
    let row: Option<(Option<String>, Option<String>)> =
        sqlx::query_as("SELECT totp_secret, totp_enabled_at FROM users WHERE id = ?")
            .bind(&actor.user_id)
            .fetch_optional(&state.pool)
            .await?;
    let (secret_opt, enabled_opt) = row.ok_or(AppError::NotFound)?;
    if secret_opt.is_none() || enabled_opt.is_none() {
        return Err(AppError::conflict("not_enrolled", "MFA is not enrolled"));
    }
    let secret_str = secret_opt.unwrap();
    let secret = totp_rs::Secret::Encoded(secret_str);
    let secret_bytes = secret.to_bytes().map_err(AppError::internal)?;
    let totp = totp_rs::TOTP::new(
        totp_rs::Algorithm::SHA1,
        6,
        1,
        30,
        secret_bytes,
        Some("Pitcairn Research Hub".to_string()),
        actor.email.clone(),
    )
    .map_err(AppError::internal)?;
    if !totp.check_current(&req.code).map_err(AppError::internal)? {
        return Err(AppError::auth_failed(
            "invalid_totp",
            "invalid verification code",
        ));
    }

    verify_current_session(&state, &headers).await?;

    let user = load_user_dto(&state.pool, &actor.user_id).await?;
    Ok(Json(MeResponse {
        user,
        mfa_verified: true,
        demo_mode: actor.demo,
    }))
}

async fn verify_current_session(state: &AppState, headers: &HeaderMap) -> AppResult<()> {
    if let Some(token) = crate::session_token(headers) {
        let session_id = sha256_hex(token.as_bytes());
        sqlx::query("UPDATE sessions SET mfa_verified = 1 WHERE id = ?")
            .bind(&session_id)
            .execute(&state.pool)
            .await?;
    }
    Ok(())
}

async fn accept_invitation(
    State(state): State<AppState>,
    actor: Actor,
    Path(token): Path<String>,
) -> AppResult<Json<AcceptInvitationResponse>> {
    let token_hash = sha256_hex(token.as_bytes());
    let row: Option<(
        String,
        String,
        String,
        String,
        String,
        Option<String>,
        Option<String>,
    )> = sqlx::query_as(
        "SELECT id, project_id, email, role, expires_at, accepted_at, revoked_at
             FROM invitations WHERE token_hash = ?",
    )
    .bind(&token_hash)
    .fetch_optional(&state.pool)
    .await?;
    let (inv_id, project_id, email, role, expires_at, accepted_at, revoked_at) =
        row.ok_or(AppError::NotFound)?;

    if accepted_at.is_some() || revoked_at.is_some() {
        return Err(AppError::conflict(
            "invitation_invalid",
            "invitation has already been used or revoked",
        ));
    }
    if crate::util::parse_time(&expires_at)
        .map(|t| t < chrono::Utc::now())
        .unwrap_or(true)
    {
        return Err(AppError::conflict(
            "invitation_invalid",
            "invitation has expired",
        ));
    }
    if email.to_lowercase() != actor.email.to_lowercase() {
        return Err(AppError::forbidden(
            "invitation is for a different email address",
        ));
    }

    let now = now_rfc3339();
    let mut conn = state.pool.acquire().await?;
    sqlx::query("BEGIN IMMEDIATE").execute(&mut *conn).await?;
    sqlx::query("UPDATE invitations SET accepted_at = ? WHERE id = ?")
        .bind(&now)
        .bind(&inv_id)
        .execute(&mut *conn)
        .await?;

    let active: Option<String> = sqlx::query_scalar(
        "SELECT id FROM project_members
         WHERE project_id = ? AND user_id = ? AND removed_at IS NULL",
    )
    .bind(&project_id)
    .bind(&actor.user_id)
    .fetch_optional(&mut *conn)
    .await?;
    if let Some(member_id) = active {
        sqlx::query("UPDATE project_members SET role = ? WHERE id = ?")
            .bind(&role)
            .bind(&member_id)
            .execute(&mut *conn)
            .await?;
    } else {
        sqlx::query(
            "INSERT INTO project_members (id, project_id, user_id, role, added_by, added_at)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(new_id())
        .bind(&project_id)
        .bind(&actor.user_id)
        .bind(&role)
        .bind(&actor.user_id)
        .bind(&now)
        .execute(&mut *conn)
        .await?;
    }
    sqlx::query("COMMIT").execute(&mut *conn).await?;

    Ok(Json(AcceptInvitationResponse { project_id }))
}
