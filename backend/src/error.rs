use std::collections::HashMap;

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;

pub type AppResult<T> = Result<T, AppError>;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    BadRequest(String),
    #[error("not authenticated")]
    Unauthorized,
    /// 401 with a specific snake_case code, e.g. "invalid_credentials", "invalid_totp".
    #[error("{message}")]
    AuthFailed { code: String, message: String },
    /// 403 with a specific snake_case code, e.g. "forbidden", "mfa_required", "protected_user"... (409 codes use Conflict).
    #[error("{message}")]
    Forbidden { code: String, message: String },
    #[error("not found")]
    NotFound,
    /// 409 with a specific code, e.g. "invalid_transition", "stale_version", "capacity_conflict".
    #[error("{message}")]
    Conflict { code: String, message: String },
    /// 422 with per-field messages.
    #[error("validation failed")]
    Validation { fields: HashMap<String, String> },
    /// 422 with a specific code, e.g. "checksum_mismatch", "bad_chunk_length", "idempotency_key_reused".
    #[error("{message}")]
    Unprocessable { code: String, message: String },
    /// 503 with a specific code, e.g. "ai_unavailable".
    #[error("{message}")]
    Unavailable { code: String, message: String },
    /// 429, e.g. login rate limit.
    #[error("{0}")]
    TooManyRequests(String),
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

impl AppError {
    pub fn forbidden(message: impl Into<String>) -> Self {
        AppError::Forbidden {
            code: "forbidden".into(),
            message: message.into(),
        }
    }

    pub fn conflict(code: &str, message: impl Into<String>) -> Self {
        AppError::Conflict {
            code: code.into(),
            message: message.into(),
        }
    }

    pub fn unprocessable(code: &str, message: impl Into<String>) -> Self {
        AppError::Unprocessable {
            code: code.into(),
            message: message.into(),
        }
    }

    pub fn auth_failed(code: &str, message: impl Into<String>) -> Self {
        AppError::AuthFailed {
            code: code.into(),
            message: message.into(),
        }
    }

    pub fn internal(err: impl std::fmt::Display) -> Self {
        AppError::Internal(anyhow::Error::msg(err.to_string()))
    }

    fn status_code(&self) -> StatusCode {
        match self {
            AppError::BadRequest(_) => StatusCode::BAD_REQUEST,
            AppError::Unauthorized | AppError::AuthFailed { .. } => StatusCode::UNAUTHORIZED,
            AppError::Forbidden { .. } => StatusCode::FORBIDDEN,
            AppError::NotFound => StatusCode::NOT_FOUND,
            AppError::Conflict { .. } => StatusCode::CONFLICT,
            AppError::Validation { .. } | AppError::Unprocessable { .. } => {
                StatusCode::UNPROCESSABLE_ENTITY
            }
            AppError::Unavailable { .. } => StatusCode::SERVICE_UNAVAILABLE,
            AppError::TooManyRequests(_) => StatusCode::TOO_MANY_REQUESTS,
            AppError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn code(&self) -> &str {
        match self {
            AppError::BadRequest(_) => "bad_request",
            AppError::Unauthorized => "unauthorized",
            AppError::AuthFailed { code, .. } => code,
            AppError::Forbidden { code, .. } => code,
            AppError::NotFound => "not_found",
            AppError::Conflict { code, .. } => code,
            AppError::Validation { .. } => "validation_failed",
            AppError::Unprocessable { code, .. } => code,
            AppError::Unavailable { code, .. } => code,
            AppError::TooManyRequests(_) => "rate_limited",
            AppError::Internal(_) => "internal_error",
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = self.status_code();
        let mut body = json!({
            "error": {
                "code": self.code(),
                "message": self.to_string(),
            }
        });
        if let AppError::Validation { fields } = &self {
            body["error"]["fields"] = json!(fields);
        }
        if matches!(self, AppError::Internal(_)) {
            tracing::error!(error = %self, "internal error");
            body["error"]["message"] = json!("internal server error");
        }
        (status, Json(body)).into_response()
    }
}

impl From<sqlx::Error> for AppError {
    fn from(err: sqlx::Error) -> Self {
        match err {
            sqlx::Error::RowNotFound => AppError::NotFound,
            other => AppError::internal(other),
        }
    }
}

impl From<std::io::Error> for AppError {
    fn from(err: std::io::Error) -> Self {
        AppError::internal(err)
    }
}
