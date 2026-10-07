use std::future::Future;

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::error::{AppError, AppResult};

/// Idempotency-Key semantics per architecture §4 `idempotency_keys`:
/// the stored row is written in the SAME transaction as the effect.
///
/// - No row for (user, route, key): run `effect`, persist its response, return it.
/// - Row exists with the same `request_hash`: replay the stored response
///   (status + body) without re-running the effect.
/// - Row exists with a different `request_hash`: 422 `idempotency_key_reused`.
///
/// `request_hash` should be a stable hash of the request body (e.g. sha256 hex).
pub async fn run<T, F, Fut>(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    user_id: &str,
    route: &str,
    key: &str,
    request_hash: &str,
    effect: F,
) -> AppResult<(u16, T)>
where
    T: Serialize + DeserializeOwned,
    F: FnOnce() -> Fut,
    Fut: Future<Output = AppResult<(u16, T)>>,
{
    let existing: Option<(String, i64, String)> = sqlx::query_as(
        "SELECT request_hash, response_status, response_json FROM idempotency_keys
         WHERE user_id = ? AND route = ? AND key = ?",
    )
    .bind(user_id)
    .bind(route)
    .bind(key)
    .fetch_optional(&mut **tx)
    .await?;

    if let Some((stored_hash, status, body)) = existing {
        if stored_hash == request_hash {
            let parsed: T = serde_json::from_str(&body).map_err(AppError::internal)?;
            return Ok((status as u16, parsed));
        }
        return Err(AppError::unprocessable(
            "idempotency_key_reused",
            "Idempotency-Key was already used with a different request body",
        ));
    }

    let (status, response) = effect().await?;
    let body = serde_json::to_string(&response).map_err(AppError::internal)?;
    sqlx::query(
        "INSERT INTO idempotency_keys (user_id, route, key, request_hash, response_status, response_json, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(user_id)
    .bind(route)
    .bind(key)
    .bind(request_hash)
    .bind(status as i64)
    .bind(&body)
    .bind(crate::util::now_rfc3339())
    .execute(&mut **tx)
    .await?;
    Ok((status, response))
}

/// Result of [`lookup`]: how to proceed for an `Idempotency-Key` request.
pub enum Lookup {
    /// No stored key — run the effect, then call [`store`].
    Fresh,
    /// Same key + same request hash — replay the stored response verbatim
    /// (status, body JSON string) without running the effect.
    Replay(u16, String),
}

/// Check `idempotency_keys` inside `tx`. Call after `begin_immediate`, before
/// running the effect. Errors: 422 `idempotency_key_reused` when the key was
/// used with a different request body.
pub async fn lookup(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    user_id: &str,
    route: &str,
    key: &str,
    request_hash: &str,
) -> AppResult<Lookup> {
    let existing: Option<(String, i64, String)> = sqlx::query_as(
        "SELECT request_hash, response_status, response_json FROM idempotency_keys
         WHERE user_id = ? AND route = ? AND key = ?",
    )
    .bind(user_id)
    .bind(route)
    .bind(key)
    .fetch_optional(&mut **tx)
    .await?;

    match existing {
        Some((stored_hash, status, body)) if stored_hash == request_hash => {
            Ok(Lookup::Replay(status as u16, body))
        }
        Some(_) => Err(AppError::unprocessable(
            "idempotency_key_reused",
            "Idempotency-Key was already used with a different request body",
        )),
        None => Ok(Lookup::Fresh),
    }
}

/// Persist the effect's response under the key, inside the same transaction
/// as the effect itself.
pub async fn store(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    user_id: &str,
    route: &str,
    key: &str,
    request_hash: &str,
    status: u16,
    body: &str,
) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO idempotency_keys (user_id, route, key, request_hash, response_status, response_json, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(user_id)
    .bind(route)
    .bind(key)
    .bind(request_hash)
    .bind(status as i64)
    .bind(body)
    .bind(crate::util::now_rfc3339())
    .execute(&mut **tx)
    .await?;
    Ok(())
}
