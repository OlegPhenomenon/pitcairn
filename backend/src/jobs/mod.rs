//! In-process background job worker (architecture §7).
//!
//! One tokio task polls `jobs` every 2s. `run_once` is public so tests can
//! drive the queue deterministically. A `running` job whose `locked_until`
//! lease expired is reclaimed (crash recovery). Backoff is 30s × 2^attempts,
//! `max_attempts` (6) failures → `dead`. Business actions never roll back
//! because a job fails: job rows are written in the same transaction as the
//! business change, and the worker only mutates job/side-effect rows.
//!
//! Slice C adds kinds `check_link` and `deliverable_reminders` (in
//! `crate::deliverables`) plus the once-per-day enqueue pass below.

use serde_json::Value;

use crate::AppState;
use crate::error::{AppError, AppResult};

pub const KIND_SEND_EMAIL: &str = "send_email";
pub const KIND_SCAN_FILE: &str = "scan_file";
pub const KIND_CLEANUP_UPLOADS: &str = "cleanup_uploads";

const LOCK_SECS: i64 = 120;
const BASE_BACKOFF_SECS: i64 = 30;

/// Enqueue a job inside an existing transaction. `dedupe_key` (unique) makes
/// enqueueing idempotent: a second enqueue with the same key is a no-op.
pub async fn enqueue(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    kind: &str,
    payload: Value,
    dedupe_key: Option<&str>,
) -> AppResult<String> {
    let id = crate::util::new_id();
    let now = crate::util::now_rfc3339();
    sqlx::query(
        "INSERT INTO jobs (id, kind, payload_json, dedupe_key, status, attempts, max_attempts, run_after, created_at)
         VALUES (?, ?, ?, ?, 'queued', 0, 6, ?, ?)
         ON CONFLICT (dedupe_key) DO NOTHING",
    )
    .bind(&id)
    .bind(kind)
    .bind(payload.to_string())
    .bind(dedupe_key)
    .bind(&now)
    .bind(&now)
    .execute(&mut **tx)
    .await?;
    Ok(id)
}

struct JobRow {
    id: String,
    kind: String,
    payload_json: String,
    attempts: i64,
    max_attempts: i64,
}

/// Reclaim expired leases, then claim and run at most one job.
/// Returns Ok(true) if a job ran.
pub async fn run_once(state: &AppState) -> AppResult<bool> {
    let now = crate::util::now_rfc3339();
    // Crash recovery: a `running` job whose lease expired goes back to queued.
    sqlx::query(
        "UPDATE jobs SET status = 'queued', locked_until = NULL
         WHERE status = 'running' AND locked_until < ?",
    )
    .bind(&now)
    .execute(&state.pool)
    .await?;

    let candidate: Option<(String,)> = sqlx::query_as(
        "SELECT id FROM jobs WHERE status IN ('queued','failed') AND run_after <= ?
         ORDER BY created_at LIMIT 1",
    )
    .bind(&now)
    .fetch_optional(&state.pool)
    .await?;
    let Some((id,)) = candidate else {
        return Ok(false);
    };

    let lock_until = crate::util::time_plus_secs(LOCK_SECS);
    // Claim atomically; another worker may have taken it.
    let claimed = sqlx::query(
        "UPDATE jobs SET status = 'running', attempts = attempts + 1, locked_until = ?
         WHERE id = ? AND status IN ('queued','failed')",
    )
    .bind(&lock_until)
    .bind(&id)
    .execute(&state.pool)
    .await?
    .rows_affected();
    if claimed == 0 {
        return Ok(false);
    }

    let job: JobRow = sqlx::query_as::<_, (String, String, String, i64, i64)>(
        "SELECT id, kind, payload_json, attempts, max_attempts FROM jobs WHERE id = ?",
    )
    .bind(&id)
    .fetch_one(&state.pool)
    .await
    .map(|(id, kind, payload_json, attempts, max_attempts)| JobRow {
        id,
        kind,
        payload_json,
        attempts,
        max_attempts,
    })?;

    let result = execute(state, &job).await;
    match result {
        Ok(()) => {
            sqlx::query(
                "UPDATE jobs SET status = 'done', last_error = NULL, locked_until = NULL WHERE id = ?",
            )
            .bind(&job.id)
            .execute(&state.pool)
            .await?;
        }
        Err(e) => {
            let msg = e.to_string();
            if job.attempts >= job.max_attempts {
                sqlx::query(
                    "UPDATE jobs SET status = 'dead', last_error = ?, locked_until = NULL WHERE id = ?",
                )
                .bind(&msg)
                .bind(&job.id)
                .execute(&state.pool)
                .await?;
            } else {
                let backoff = BASE_BACKOFF_SECS * 2i64.pow(job.attempts.saturating_sub(1) as u32);
                let run_after = crate::util::time_plus_secs(backoff);
                sqlx::query(
                    "UPDATE jobs SET status = 'failed', last_error = ?, run_after = ?, locked_until = NULL WHERE id = ?",
                )
                .bind(&msg)
                .bind(&run_after)
                .bind(&job.id)
                .execute(&state.pool)
                .await?;
            }
        }
    }
    Ok(true)
}

async fn execute(state: &AppState, job: &JobRow) -> AppResult<()> {
    let payload: Value = serde_json::from_str(&job.payload_json).unwrap_or(Value::Null);
    match job.kind.as_str() {
        KIND_SEND_EMAIL => {
            let message_id = payload["mail_message_id"].as_str().ok_or_else(|| {
                AppError::BadRequest("send_email payload missing mail_message_id".into())
            })?;
            state
                .mail
                .deliver(message_id)
                .await
                .map_err(|e| AppError::Unavailable {
                    code: "mail_failed".into(),
                    message: e,
                })
        }
        KIND_SCAN_FILE => {
            let file_id = payload["file_id"]
                .as_str()
                .ok_or_else(|| AppError::BadRequest("scan_file payload missing file_id".into()))?;
            scan_file(state, file_id).await
        }
        KIND_CLEANUP_UPLOADS => cleanup_uploads(state).await,
        crate::deliverables::KIND_CHECK_LINK => {
            let link_id = payload["external_link_id"].as_str().ok_or_else(|| {
                AppError::BadRequest("check_link payload missing external_link_id".into())
            })?;
            crate::deliverables::run_link_check(state, link_id).await
        }
        crate::deliverables::KIND_DELIVERABLE_REMINDERS => {
            let deliverable_id = payload["deliverable_id"].as_str().ok_or_else(|| {
                AppError::BadRequest("deliverable_reminders payload missing deliverable_id".into())
            })?;
            crate::deliverables::run_deliverable_reminder(state, deliverable_id).await
        }
        other => Err(AppError::BadRequest(format!("unknown job kind: {other}"))),
    }
}

/// Mock antivirus (§6): rejects the EICAR test string, executables (MZ/ELF
/// magic), and declared-vs-sniffed mime mismatches for common types.
async fn scan_file(state: &AppState, file_id: &str) -> AppResult<()> {
    let row: Option<(String, String)> =
        sqlx::query_as("SELECT sha256, mime FROM files WHERE id = ?")
            .bind(file_id)
            .fetch_optional(&state.pool)
            .await?;
    let Some((sha256, declared_mime)) = row else {
        // File vanished; nothing to scan, treat as done.
        return Ok(());
    };
    let bytes = crate::files::read_file(&state.config.data_dir, &sha256).await?;
    let detail = scan_bytes(&bytes, &declared_mime);
    let (status, detail) = match detail {
        None => ("clean", None),
        Some(d) => ("rejected", Some(d)),
    };
    sqlx::query("UPDATE files SET scan_status = ?, scan_detail = ? WHERE id = ?")
        .bind(status)
        .bind(&detail)
        .bind(file_id)
        .execute(&state.pool)
        .await?;
    Ok(())
}

/// Returns Some(reason) when the bytes are rejected.
pub fn scan_bytes(bytes: &[u8], declared_mime: &str) -> Option<String> {
    const EICAR: &str = "X5O!P%@AP[4\\PZX54(P^)7CC)7}$EICAR-STANDARD-ANTIVIRUS-TEST-FILE!$H+H*";
    if bytes.windows(EICAR.len()).any(|w| w == EICAR.as_bytes()) {
        return Some("EICAR antivirus test string detected".into());
    }
    if bytes.starts_with(b"MZ") {
        return Some("Windows executable (MZ) not allowed".into());
    }
    if bytes.starts_with(b"\x7fELF") {
        return Some("ELF executable not allowed".into());
    }
    let sniffed = sniff_mime(bytes);
    if let (Some(sniffed), false) = (sniffed, bytes.is_empty()) {
        // Compare broad type families; only reject clear mismatches.
        let declared_family = declared_mime.split('/').next().unwrap_or("");
        let sniffed_family = sniffed.split('/').next().unwrap_or("");
        let exact = declared_mime == sniffed;
        let family_ok = declared_family == sniffed_family;
        // text/* is a superset family (csv, plain, markdown...).
        let text_ok = sniffed == "text/plain" && declared_family == "text";
        let zip_ok = sniffed == "application/zip"
            && matches!(
                declared_mime,
                "application/zip"
                    | "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
                    | "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
            );
        if !exact && !family_ok && !text_ok && !zip_ok {
            return Some(format!(
                "declared mime '{declared_mime}' does not match sniffed '{sniffed}'"
            ));
        }
    }
    None
}

fn sniff_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"%PDF") {
        Some("application/pdf")
    } else if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        Some("image/jpeg")
    } else if bytes.starts_with(b"GIF8") {
        Some("image/gif")
    } else if bytes.starts_with(b"PK\x03\x04") {
        Some("application/zip")
    } else if bytes.iter().all(|b| {
        b.is_ascii() && (!b.is_ascii_control() || *b == b'\n' || *b == b'\r' || *b == b'\t')
    }) {
        Some("text/plain")
    } else {
        None
    }
}

/// Remove expired/aborted upload sessions and their part files.
async fn cleanup_uploads(state: &AppState) -> AppResult<()> {
    let now = crate::util::now_rfc3339();
    let stale: Vec<(String,)> = sqlx::query_as(
        "SELECT id FROM upload_sessions
         WHERE (status IN ('open','finalizing') AND expires_at < ?) OR status = 'aborted'",
    )
    .bind(&now)
    .fetch_all(&state.pool)
    .await?;
    for (id,) in stale {
        let part = crate::files::part_path(&state.config.data_dir, &id);
        tokio::fs::remove_file(part).await.ok();
        sqlx::query("DELETE FROM upload_sessions WHERE id = ?")
            .bind(&id)
            .execute(&state.pool)
            .await?;
    }
    Ok(())
}

/// Run due jobs until the queue is drained or shutdown fires; used by `serve`.
/// Once per UTC day it also enqueues the daily batch (link checks and
/// deliverable reminders, deduped by date — re-runs are no-ops).
pub async fn worker_loop(state: AppState, mut shutdown: tokio::sync::watch::Receiver<bool>) {
    let mut last_daily: Option<String> = None;
    loop {
        // Finish the current job before honoring shutdown (graceful stop).
        match run_once(&state).await {
            Ok(true) => continue, // drain without sleeping
            Ok(false) => {}
            Err(e) => tracing::error!(error = %e, "job worker iteration failed"),
        }
        let today = crate::deliverables::today();
        if last_daily.as_deref() != Some(today.as_str()) {
            last_daily = Some(today);
            if let Err(e) = crate::deliverables::enqueue_daily_jobs(&state).await {
                tracing::error!(error = %e, "daily job enqueue failed");
            }
        }
        tokio::select! {
            _ = shutdown.changed() => {
                tracing::info!("job worker shutting down");
                break;
            }
            _ = tokio::time::sleep(std::time::Duration::from_secs(2)) => {}
        }
        if *shutdown.borrow() {
            break;
        }
    }
}
