//! Deliverables domain logic shared across routes, the job worker and other
//! slices (architecture §4 "Deliverables").
//!
//! - the mock external-link checker (`PITCAIRN_LINK_CHECK_MODE=mock`, §2.1)
//! - the `check_link` job executor and the daily link-check enqueue pass the
//!   worker calls once per UTC day (reminders: `jobs::reminders`)
//! - `unavailable_links` — the coordinator dashboard query slice D consumes
//! - the ONE documented measurement CSV format (`site,date,variable,value,unit`)

use serde_json::json;
use sqlx::SqlitePool;

use crate::AppState;
use crate::dto::UnavailableLinkDto;
use crate::error::{AppError, AppResult};
use crate::util::now_rfc3339;
use crate::{jobs, notify};

pub const KIND_CHECK_LINK: &str = "check_link";

/// Deliverable statuses that still need work from the team (open).
pub const OPEN_STATUSES: [&str; 4] = ["proposed", "agreed", "submitted", "changes_requested"];

/// The coordinator warning shown whenever publication settings or files are
/// changed (§4): published files must not leak sensitive data.
pub const PUBLICATION_WARNING: &str =
    "Published files must not contain precise sensitive coordinates or personal data.";

/// Exact header of the ONE documented measurement CSV format (§4
/// `measurements`). Other CSVs are kept as files only.
pub const MEASUREMENT_HEADER: [&str; 5] = ["site", "date", "variable", "value", "unit"];

// ---------------------------------------------------------------------------
// Mock link checker (§2.1)
// ---------------------------------------------------------------------------

/// Mock link availability: `http(s)` URLs whose host ends `.invalid` or whose
/// path contains `/missing` are unavailable, everything else is available.
/// Non-http(s) URLs are treated as unavailable (SSRF guard, §11 — the mock
/// never fetches, so this only gates what the UI may show as "checked").
pub fn link_available(url: &str, _mode: &str) -> bool {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"));
    let Some(rest) = rest else { return false };
    let (host, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let host = host.split(':').next().unwrap_or("").to_ascii_lowercase();
    if host.is_empty() {
        return false;
    }
    !(host.ends_with(".invalid") || path.contains("/missing"))
}

/// Shape check used by request validation: only `http(s)` URLs with a host.
pub fn is_http_url(url: &str) -> bool {
    let Some(rest) = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
    else {
        return false;
    };
    let host = rest.split('/').next().unwrap_or("");
    !host.is_empty() && host.len() <= 253
}

// ---------------------------------------------------------------------------
// Job executors
// ---------------------------------------------------------------------------

/// `check_link` job: run the mock checker for one external link and store the
/// outcome. When a link newly turns unavailable the deliverable recipient is
/// notified (unavailable links are flagged to the coordinator, §4).
pub async fn run_link_check(state: &AppState, external_link_id: &str) -> AppResult<()> {
    let row: Option<(String, String, Option<i64>, String, String, String)> = sqlx::query_as(
        "SELECT l.url, l.submission_id, l.available,
                d.recipient_id, d.title, d.project_id
         FROM external_links l
         JOIN deliverable_submissions s ON s.id = l.submission_id
         JOIN deliverables d ON d.id = s.deliverable_id
         WHERE l.id = ?",
    )
    .bind(external_link_id)
    .fetch_optional(&state.pool)
    .await?;
    let Some((url, _submission_id, was_available, recipient_id, title, project_id)) = row else {
        // Link deleted; nothing to check, treat as done.
        return Ok(());
    };

    let available = link_available(&url, &state.config.link_check_mode);
    let now = now_rfc3339();
    let newly_unavailable = !available && was_available != Some(0);

    let mut tx = crate::db::begin_immediate(&state.pool).await?;
    sqlx::query(
        "UPDATE external_links SET last_checked_at = ?, last_status = ?, available = ? WHERE id = ?",
    )
    .bind(&now)
    .bind(if available { "available" } else { "unavailable" })
    .bind(available as i64)
    .bind(external_link_id)
    .execute(&mut *tx)
    .await?;

    if newly_unavailable {
        notify::notify(
            &mut tx,
            &recipient_id,
            "link.unavailable",
            "Submitted link is unavailable",
            &format!("A link submitted for deliverable \"{title}\" could not be reached: {url}"),
            &format!("/app/projects/{project_id}/results"),
            Some(&project_id),
        )
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// Called by the job worker once per UTC day: enqueues one `check_link` job
/// per submitted external link, deduped per day. (Deliverable reminders are
/// scheduled by `jobs::reminders::enqueue_daily`.)
pub async fn enqueue_daily_jobs(state: &AppState) -> AppResult<()> {
    let today = chrono::Utc::now().date_naive();
    let link_ids: Vec<String> = sqlx::query_scalar("SELECT id FROM external_links")
        .fetch_all(&state.pool)
        .await?;

    let mut tx = crate::db::begin_immediate(&state.pool).await?;
    for id in link_ids {
        jobs::enqueue(
            &mut tx,
            KIND_CHECK_LINK,
            json!({"external_link_id": id}),
            Some(&format!("check_link:{id}:{today}")),
        )
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Coordinator dashboard query (used by slice D)
// ---------------------------------------------------------------------------

#[derive(sqlx::FromRow)]
struct UnavailableLinkRow {
    link_id: String,
    url: String,
    description: String,
    last_checked_at: Option<String>,
    last_status: Option<String>,
    submission_id: String,
    deliverable_id: String,
    deliverable_title: String,
    project_id: String,
    project_reference: Option<String>,
}

/// Every submitted external link whose last check failed, with the context a
/// coordinator needs to chase it up.
pub async fn unavailable_links(pool: &SqlitePool) -> AppResult<Vec<UnavailableLinkDto>> {
    let rows: Vec<UnavailableLinkRow> = sqlx::query_as(
        "SELECT l.id AS link_id, l.url, l.description, l.last_checked_at, l.last_status,
                s.id AS submission_id, d.id AS deliverable_id,
                d.title AS deliverable_title, d.project_id, p.reference AS project_reference
         FROM external_links l
         JOIN deliverable_submissions s ON s.id = l.submission_id
         JOIN deliverables d ON d.id = s.deliverable_id
         JOIN projects p ON p.id = d.project_id
         WHERE l.available = 0
         ORDER BY l.last_checked_at DESC",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| UnavailableLinkDto {
            link_id: r.link_id,
            url: r.url,
            description: r.description,
            last_checked_at: r.last_checked_at,
            last_status: r.last_status,
            submission_id: r.submission_id,
            deliverable_id: r.deliverable_id,
            deliverable_title: r.deliverable_title,
            project_id: r.project_id,
            project_reference: r.project_reference,
        })
        .collect())
}

// ---------------------------------------------------------------------------
// Measurement CSV parsing (§4 `measurements`)
// ---------------------------------------------------------------------------

/// One valid `site,date,variable,value,unit` row.
#[derive(Debug, Clone)]
pub struct ParsedMeasurement {
    pub site_name: String,
    pub observed_on: String,
    pub variable_key: String,
    pub value: f64,
    pub unit: String,
}

/// Is `bytes` a measurement CSV? True only when the header is EXACTLY
/// `site,date,variable,value,unit` — other CSVs stay files only.
pub fn is_measurement_csv(bytes: &[u8]) -> bool {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return false;
    };
    let first = text.lines().next().unwrap_or("");
    first.trim_end_matches('\r') == MEASUREMENT_HEADER.join(",")
}

/// Parse a measurement CSV body. Returns (valid rows, warnings): invalid rows
/// are reported per line (line 2 = first data row) and never fail the parse.
pub fn parse_measurement_csv(bytes: &[u8]) -> (Vec<ParsedMeasurement>, Vec<String>) {
    let mut rows = Vec::new();
    let mut warnings = Vec::new();
    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(true)
        .from_reader(bytes);
    for (i, rec) in rdr.records().enumerate() {
        let line = i + 2; // header is line 1
        match rec {
            Err(e) => warnings.push(format!("row {line}: malformed CSV ({e})")),
            Ok(rec) => {
                let get = |i: usize| rec.get(i).unwrap_or("").trim();
                let (site, date, variable, value, unit) = (get(0), get(1), get(2), get(3), get(4));
                if site.is_empty() || variable.is_empty() {
                    warnings.push(format!("row {line}: site and variable are required"));
                    continue;
                }
                if chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").is_err() {
                    warnings.push(format!("row {line}: date must be YYYY-MM-DD"));
                    continue;
                }
                match value.parse::<f64>() {
                    Err(_) => warnings.push(format!("row {line}: value is not numeric")),
                    Ok(value) => rows.push(ParsedMeasurement {
                        site_name: site.to_string(),
                        observed_on: date.to_string(),
                        variable_key: variable.to_string(),
                        value,
                        unit: unit.to_string(),
                    }),
                }
            }
        }
    }
    (rows, warnings)
}

/// Load the file bytes for a submission file's document version and parse it
/// when it is a measurement CSV. IO happens OUTSIDE any DB transaction.
pub async fn submission_measurements(
    pool: &SqlitePool,
    data_dir: &std::path::Path,
    submission_id: &str,
) -> AppResult<(Vec<ParsedMeasurement>, Vec<String>)> {
    let files: Vec<(String, String)> = sqlx::query_as(
        "SELECT d.title, f.sha256
         FROM submission_files sf
         JOIN document_versions dv ON dv.id = sf.document_version_id
         JOIN documents d ON d.id = dv.document_id
         JOIN files f ON f.id = dv.file_id
         WHERE sf.submission_id = ? AND f.scan_status = 'clean'",
    )
    .bind(submission_id)
    .fetch_all(pool)
    .await?;

    let mut rows = Vec::new();
    let mut warnings = Vec::new();
    for (title, sha256) in files {
        let bytes = crate::files::read_file(data_dir, &sha256).await?;
        if !is_measurement_csv(&bytes) {
            continue;
        }
        let (mut parsed, mut warns) = parse_measurement_csv(&bytes);
        for w in &mut warns {
            *w = format!("{title}: {w}");
        }
        rows.append(&mut parsed);
        warnings.append(&mut warns);
    }
    Ok((rows, warnings))
}

/// Persist parsed measurement rows for an accepted submission.
/// `source_label` = "<project reference> — <deliverable title> (submission #n)".
pub async fn insert_measurements(
    pool: &SqlitePool,
    project_id: &str,
    deliverable_id: &str,
    submission_id: &str,
    source_label: &str,
    rows: &[ParsedMeasurement],
) -> AppResult<()> {
    let now = now_rfc3339();
    let mut tx = crate::db::begin_immediate(pool).await?;
    for r in rows {
        sqlx::query(
            "INSERT INTO measurements
             (id, project_id, deliverable_id, submission_id, site_name, observed_on,
              variable_key, value, unit, source_label, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(crate::util::new_id())
        .bind(project_id)
        .bind(deliverable_id)
        .bind(submission_id)
        .bind(&r.site_name)
        .bind(&r.observed_on)
        .bind(&r.variable_key)
        .bind(r.value)
        .bind(&r.unit)
        .bind(source_label)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// Convenience: current UTC date as `YYYY-MM-DD`.
pub fn today() -> String {
    chrono::Utc::now()
        .date_naive()
        .format("%Y-%m-%d")
        .to_string()
}

/// 404-when-disabled helper for the public catalog.
pub async fn public_catalog_enabled(pool: &SqlitePool) -> AppResult<bool> {
    let v: Option<String> =
        sqlx::query_scalar("SELECT value FROM settings WHERE key = 'public_catalog_enabled'")
            .fetch_optional(pool)
            .await?;
    Ok(v.as_deref() == Some("true"))
}

/// Guard used by catalog handlers — 404 (do not reveal anything) when the
/// catalog is disabled in settings.
pub async fn require_catalog_enabled(pool: &SqlitePool) -> AppResult<()> {
    if public_catalog_enabled(pool).await? {
        Ok(())
    } else {
        Err(AppError::NotFound)
    }
}
