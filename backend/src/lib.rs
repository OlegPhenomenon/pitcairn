pub mod archive;
pub mod assist;
pub mod audit;
pub mod authz;
pub mod backup;
pub mod config;
pub mod db;
pub mod deliverables;
pub mod dto;
pub mod error;
pub mod files;
pub mod idempotency;
pub mod jobs;
pub mod legacy;
pub mod mail;
pub mod notify;
pub mod password;
pub mod projects;
pub mod ratelimit;
pub mod refs;
pub mod routes;
pub mod seed;
pub mod util;
pub mod validation;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, Method, Request, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Json, Response};
use sqlx::SqlitePool;
use tower_http::compression::CompressionLayer;
use tower_http::trace::TraceLayer;

use crate::config::Config;
use crate::error::{AppError, AppResult};
use crate::mail::MailTransport;

/// Per-upload mutex map: `complete` takes the lock so chunk writes race
/// against finalization deterministically (chunk write while finalizing → 409).
pub type UploadLocks = Arc<Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>>;

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub config: Arc<Config>,
    pub mail: Arc<dyn MailTransport>,
    pub upload_locks: UploadLocks,
    pub login_limiter: Arc<ratelimit::RateLimiter>,
    /// Admin imports buffer whole CSV/ZIP bodies: one at a time.
    pub import_slots: Arc<tokio::sync::Semaphore>,
}

impl AppState {
    pub fn new(pool: SqlitePool, config: Arc<Config>, mail: Arc<dyn MailTransport>) -> Self {
        AppState {
            pool,
            config,
            mail,
            upload_locks: Arc::new(Mutex::new(HashMap::new())),
            login_limiter: Arc::new(ratelimit::RateLimiter::new(
                std::time::Duration::from_secs(60),
                10,
            )),
            import_slots: Arc::new(tokio::sync::Semaphore::new(1)),
        }
    }

    pub fn upload_lock(&self, upload_id: &str) -> Arc<tokio::sync::Mutex<()>> {
        self.upload_locks
            .lock()
            .expect("upload locks poisoned")
            .entry(upload_id.to_string())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone()
    }
}

pub fn build_app(state: AppState) -> axum::Router {
    let api = routes::api_router(state.clone())
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            mfa_gate,
        ))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            csrf_guard,
        ));

    let static_dir = state.config.static_dir.clone();
    let spa = tower_http::services::ServeDir::new(&static_dir).not_found_service(
        tower_http::services::ServeFile::new(static_dir.join("index.html")),
    );
    let spa_fallback = tower::service_fn(move |req: Request<Body>| {
        let mut spa = spa.clone();
        async move {
            if req.uri().path().starts_with("/api/") {
                return Ok(AppError::NotFound.into_response());
            }
            use tower::Service;
            let resp = spa.call(req).await.expect("ServeDir is infallible");
            Ok(resp.map(Body::new))
        }
    });

    axum::Router::new()
        .route("/up", axum::routing::get(up))
        .nest("/api/v1", api)
        .fallback_service(spa_fallback)
        .layer(CompressionLayer::new().gzip(true))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

async fn up() -> Json<serde_json::Value> {
    Json(serde_json::json!({"status": "ok"}))
}

/// Every non-GET request must carry `X-Pitcairn-Csrf: 1` (§5), except the
/// bank webhook which authenticates by HMAC signature instead.
async fn csrf_guard(
    State(_state): State<AppState>,
    req: Request<Body>,
    next: Next,
) -> AppResult<Response> {
    let method = req.method().clone();
    let path = req.uri().path().to_string();
    if matches!(method, Method::GET | Method::HEAD | Method::OPTIONS)
        // Nested under /api/v1, so middleware sees the stripped inner path.
        || path == "/integrations/bank/notifications"
    {
        return Ok(next.run(req).await);
    }
    let ok = req
        .headers()
        .get("x-pitcairn-csrf")
        .and_then(|v| v.to_str().ok())
        == Some("1");
    if ok {
        Ok(next.run(req).await)
    } else {
        Err(AppError::Forbidden {
            code: "csrf".into(),
            message: "missing X-Pitcairn-Csrf: 1 header".into(),
        })
    }
}

/// Staff and experts get 403 `mfa_required` on every non-auth endpoint until
/// the session is MFA-verified (§3). Unauthenticated requests pass through
/// (the endpoint's own extractor returns 401).
async fn mfa_gate(State(state): State<AppState>, req: Request<Body>, next: Next) -> Response {
    let path = req.uri().path().to_string();
    let exempt = path.starts_with("/auth/")
        || path.starts_with("/invitations/")
        || path.starts_with("/public/")
        || path.starts_with("/integrations/")
        || path.starts_with("/demo/totp");
    if exempt {
        return next.run(req).await;
    }
    if let Some(token) = session_token(req.headers())
        && let Ok(actor) = authz::load_actor(&state, &token).await
        && actor.needs_mfa()
    {
        return AppError::Forbidden {
            code: "mfa_required".into(),
            message: "multi-factor verification required".into(),
        }
        .into_response();
    }
    next.run(req).await
}

pub fn session_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get(axum::http::header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .and_then(|cookies| {
            cookies.split(';').map(str::trim).find_map(|c| {
                c.strip_prefix(&format!("{}=", authz::SESSION_COOKIE))
                    .map(str::to_string)
            })
        })
}

/// 404 JSON body for unknown API paths.
pub fn api_not_found() -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({"error": {"code": "not_found", "message": "not found"}})),
    )
}
