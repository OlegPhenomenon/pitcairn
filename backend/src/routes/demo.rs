use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{Request, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};

use crate::AppState;
use crate::authz;
use crate::dto::{
    DemoSwitchRequest, DemoTotpResponse, ListQuery, ListResponse, LoginResponse, MailMessageDto,
    PersonaDto, PersonasResponse,
};
use crate::error::{AppError, AppResult};
use crate::routes::auth::load_user_dto;
use crate::util::{now_rfc3339, random_token, sha256_hex, time_plus_secs};

pub fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/demo/personas", get(list_personas))
        .route("/demo/switch", post(switch_persona))
        .route("/demo/mailbox", get(mailbox))
        .route("/demo/totp/{user_id}", get(totp_code))
        .layer(axum::middleware::from_fn_with_state(state, demo_only))
}

async fn demo_only(
    State(state): State<AppState>,
    req: Request<Body>,
    next: Next,
) -> AppResult<Response> {
    if !state.config.demo_mode {
        return Err(AppError::NotFound);
    }
    Ok(next.run(req).await)
}

async fn list_personas(State(state): State<AppState>) -> AppResult<Json<PersonasResponse>> {
    let mut personas = Vec::new();
    for p in crate::seed::PERSONAS {
        let user_id: Option<String> = sqlx::query_scalar("SELECT id FROM users WHERE email = ?")
            .bind(p.email)
            .fetch_optional(&state.pool)
            .await?;
        if let Some(user_id) = user_id {
            personas.push(PersonaDto {
                key: p.key.into(),
                user_id,
                name: p.name.into(),
                email: p.email.into(),
                organisation: p.organisation.into(),
                role: p.role.map(Into::into),
            });
        }
    }
    Ok(Json(PersonasResponse { personas }))
}

async fn switch_persona(
    State(state): State<AppState>,
    Json(req): Json<DemoSwitchRequest>,
) -> AppResult<impl IntoResponse> {
    let persona = crate::seed::PERSONAS
        .iter()
        .find(|p| p.key == req.persona_key)
        .ok_or(AppError::NotFound)?;
    let user_id: String = sqlx::query_scalar("SELECT id FROM users WHERE email = ?")
        .bind(persona.email)
        .fetch_one(&state.pool)
        .await?;

    let token = random_token();
    let session_id = sha256_hex(token.as_bytes());
    let now = now_rfc3339();
    let expires = time_plus_secs(12 * 3600);
    sqlx::query(
        "INSERT INTO sessions (id, user_id, created_at, expires_at, mfa_verified, via_demo_switch)
         VALUES (?, ?, ?, ?, 1, 1)",
    )
    .bind(&session_id)
    .bind(&user_id)
    .bind(&now)
    .bind(&expires)
    .execute(&state.pool)
    .await?;

    let user = load_user_dto(&state.pool, &user_id).await?;
    let mut cookie = format!(
        "{}={}; HttpOnly; SameSite=Lax; Path=/; Max-Age={}",
        authz::SESSION_COOKIE,
        token,
        12 * 3600
    );
    if state.config.secure_cookies {
        cookie.push_str("; Secure");
    }

    Ok((
        axum::http::StatusCode::OK,
        [(header::SET_COOKIE, cookie)],
        Json(LoginResponse {
            user,
            mfa_required: false,
            mfa_enrollment_required: false,
        }),
    ))
}

#[derive(sqlx::FromRow)]
#[allow(dead_code)]
struct MailRow {
    rowid: i64,
    to_email: String,
    subject: String,
    body_text: String,
    status: String,
    error: Option<String>,
    created_at: String,
    sent_at: Option<String>,
}

async fn mailbox(
    State(state): State<AppState>,
    Query(query): Query<ListQuery>,
) -> AppResult<Json<ListResponse<MailMessageDto>>> {
    let limit = query.limit();
    let offset = query.offset();

    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM mail_messages")
        .fetch_one(&state.pool)
        .await?;
    let rows: Vec<MailRow> = sqlx::query_as(
        "SELECT rowid, to_email, subject, body_text, status, error, created_at, sent_at
         FROM mail_messages ORDER BY created_at DESC LIMIT ? OFFSET ?",
    )
    .bind(limit)
    .bind(offset)
    .fetch_all(&state.pool)
    .await?;

    let items = rows
        .into_iter()
        .map(|r| MailMessageDto {
            id: r.rowid,
            to_email: r.to_email,
            subject: r.subject,
            body_text: r.body_text,
            status: r.status,
            error: r.error,
            created_at: r.created_at,
            sent_at: r.sent_at,
        })
        .collect();
    Ok(Json(ListResponse { items, total }))
}

async fn totp_code(
    State(state): State<AppState>,
    Path(user_id): Path<String>,
) -> AppResult<Json<DemoTotpResponse>> {
    let row: Option<(String, Option<String>)> =
        sqlx::query_as("SELECT email, totp_secret FROM users WHERE id = ?")
            .bind(&user_id)
            .fetch_optional(&state.pool)
            .await?;
    let (email, secret_str) = row.ok_or(AppError::NotFound)?;
    let secret_str = secret_str.ok_or(AppError::NotFound)?;

    let secret = totp_rs::Secret::Encoded(secret_str);
    let secret_bytes = secret.to_bytes().map_err(AppError::internal)?;
    let totp = totp_rs::TOTP::new(
        totp_rs::Algorithm::SHA1,
        6,
        1,
        30,
        secret_bytes,
        Some("Pitcairn Research Hub".to_string()),
        email,
    )
    .map_err(AppError::internal)?;
    let code = totp.generate_current().map_err(AppError::internal)?;

    Ok(Json(DemoTotpResponse { user_id, code }))
}
