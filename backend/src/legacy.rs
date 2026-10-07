//! Legacy CSV import (§8). `POST /admin/import/legacy/preview` parses and
//! validates rows into an `import_batches` row; `POST /admin/import/{id}/commit`
//! re-validates and creates `legacy=1` closed projects. The demo seed reuses
//! `commit_rows` so legacy projects enter through the same code path.
//!
//! CSV columns (§8): `reference,title,organisation,lead_name,lead_email,
//! start_date,end_date,summary,keywords,site_name,lat,lng,report_title,
//! report_url`.

use std::collections::{BTreeMap, HashSet};

use serde_json::{Value, json};
use sqlx::SqlitePool;

use crate::audit::{self, AuditEvent};
use crate::db;
use crate::error::{AppError, AppResult};
use crate::util::{new_id, now_rfc3339};

pub const KIND: &str = "legacy_csv";

pub const COLUMNS: [&str; 14] = [
    "reference",
    "title",
    "organisation",
    "lead_name",
    "lead_email",
    "start_date",
    "end_date",
    "summary",
    "keywords",
    "site_name",
    "lat",
    "lng",
    "report_title",
    "report_url",
];

const REQUIRED: [&str; 7] = [
    "reference",
    "title",
    "organisation",
    "lead_name",
    "lead_email",
    "start_date",
    "end_date",
];

/// Approximate Pitcairn EEZ/MPA bbox (docs/research/geography.md):
/// SW (-28.4, -134.6) → NE (-20.7, -121.2). Coordinates outside it are
/// flagged at preview.
pub const EEZ_MIN_LAT: f64 = -28.4;
pub const EEZ_MAX_LAT: f64 = -20.7;
pub const EEZ_MIN_LNG: f64 = -134.6;
pub const EEZ_MAX_LNG: f64 = -121.2;

/// One preview row: the original data plus validation outcome.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PreviewRow {
    pub index: i64,
    pub data: BTreeMap<String, String>,
    pub errors: BTreeMap<String, String>,
    pub duplicate_of: Option<String>,
}

/// Parse CSV bytes into per-column string maps. The header row is required;
/// unknown extra columns are ignored, missing columns produce one error.
pub fn parse_csv(bytes: &[u8]) -> AppResult<Vec<BTreeMap<String, String>>> {
    let mut reader = csv::ReaderBuilder::new()
        .trim(csv::Trim::All)
        .from_reader(bytes);
    let headers: Vec<String> = reader
        .headers()
        .map_err(|e| AppError::BadRequest(format!("CSV header parse failed: {e}")))?
        .iter()
        .map(|h| h.trim().to_lowercase())
        .collect();
    let missing: Vec<&str> = COLUMNS
        .iter()
        .copied()
        .filter(|c| !headers.iter().any(|h| h == c))
        .collect();
    // Only required columns must be present; optional ones default to "".
    let missing_required: Vec<&str> = missing
        .iter()
        .copied()
        .filter(|c| REQUIRED.contains(c))
        .collect();
    if !missing_required.is_empty() {
        return Err(AppError::unprocessable(
            "bad_csv",
            format!("missing required columns: {}", missing_required.join(", ")),
        ));
    }

    let mut rows = Vec::new();
    for record in reader.records() {
        let record =
            record.map_err(|e| AppError::BadRequest(format!("CSV row parse failed: {e}")))?;
        let mut data = BTreeMap::new();
        for col in COLUMNS {
            let value = headers
                .iter()
                .position(|h| h == col)
                .and_then(|i| record.get(i))
                .unwrap_or("")
                .to_string();
            data.insert(col.to_string(), value);
        }
        rows.push(data);
    }
    Ok(rows)
}

fn valid_date(s: &str) -> bool {
    chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").is_ok()
}

/// Normalize for duplicate detection: lowercase, whitespace-collapsed.
fn norm(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Validate one row structurally (no DB). Returns per-field errors.
fn validate_row(data: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    let mut errors = BTreeMap::new();
    let get = |k: &str| data.get(k).map(|s| s.as_str()).unwrap_or("");

    for col in REQUIRED {
        if get(col).trim().is_empty() {
            errors.insert(col.to_string(), "required".to_string());
        }
    }
    let email = get("lead_email");
    if !email.is_empty() && !email.contains('@') {
        errors.insert("lead_email".into(), "must be a valid email".into());
    }
    for col in ["start_date", "end_date"] {
        let v = get(col);
        if !v.is_empty() && !valid_date(v) {
            errors.insert(col.into(), "must be a date YYYY-MM-DD".into());
        }
    }
    if !errors.contains_key("start_date")
        && !errors.contains_key("end_date")
        && !get("start_date").is_empty()
        && !get("end_date").is_empty()
        && get("end_date") < get("start_date")
    {
        errors.insert("end_date".into(), "must be on or after start_date".into());
    }

    let site_fields = [get("site_name"), get("lat"), get("lng")];
    let any_site = site_fields.iter().any(|v| !v.trim().is_empty());
    if any_site {
        if get("site_name").trim().is_empty() {
            errors.insert("site_name".into(), "required when lat/lng are given".into());
        }
        let mut coord_ok = true;
        for (col, range) in [("lat", -90.0..=90.0), ("lng", -180.0..=180.0)] {
            match get(col).trim().parse::<f64>() {
                Ok(v) if range.contains(&v) => {}
                _ => {
                    errors.insert(col.into(), "must be a valid coordinate".into());
                    coord_ok = false;
                }
            }
        }
        if coord_ok {
            let lat: f64 = get("lat").trim().parse().expect("checked above");
            let lng: f64 = get("lng").trim().parse().expect("checked above");
            if !(EEZ_MIN_LAT..=EEZ_MAX_LAT).contains(&lat)
                || !(EEZ_MIN_LNG..=EEZ_MAX_LNG).contains(&lng)
            {
                errors.insert("lat".into(), "outside the Pitcairn EEZ bounding box".into());
                errors.insert("lng".into(), "outside the Pitcairn EEZ bounding box".into());
            }
        }
    }

    if get("report_title").is_empty() != get("report_url").is_empty() {
        errors.insert(
            "report_url".into(),
            "report_title and report_url must be given together".into(),
        );
    }
    if !get("report_url").is_empty()
        && !(get("report_url").starts_with("http://") || get("report_url").starts_with("https://"))
    {
        errors.insert("report_url".into(), "must be an http(s) URL".into());
    }
    errors
}

/// Duplicate check against the database plus previously-seen rows of the same
/// file: same reference, or same normalized title+organisation+year.
async fn find_duplicate(
    pool: &SqlitePool,
    data: &BTreeMap<String, String>,
    seen_refs: &mut HashSet<String>,
    seen_titles: &mut HashSet<String>,
) -> AppResult<Option<String>> {
    let get = |k: &str| data.get(k).map(|s| s.trim()).unwrap_or("");
    let reference = get("reference");
    let year = get("start_date").get(..4).unwrap_or("");
    let title_key = format!(
        "{}|{}|{}",
        norm(get("title")),
        norm(get("organisation")),
        year
    );

    if !reference.is_empty() {
        if !seen_refs.insert(reference.to_lowercase()) {
            return Ok(Some(format!("row with same reference {reference}")));
        }
        let existing: Option<String> =
            sqlx::query_scalar("SELECT reference FROM projects WHERE reference = ?")
                .bind(reference)
                .fetch_optional(pool)
                .await?;
        if let Some(r) = existing {
            return Ok(Some(format!("project {r}")));
        }
    }
    if !seen_titles.insert(title_key.clone()) {
        return Ok(Some(
            "row with same title, organisation and year".to_string(),
        ));
    }
    if !get("title").is_empty() && !get("organisation").is_empty() && !year.is_empty() {
        let existing: Option<Option<String>> = sqlx::query_scalar(
            "SELECT reference FROM projects
             WHERE lower(title) = ? AND lower(organisation) = ?
               AND substr(start_date, 1, 4) = ?
             LIMIT 1",
        )
        .bind(norm(get("title")))
        .bind(norm(get("organisation")))
        .bind(year)
        .fetch_optional(pool)
        .await?;
        if let Some(r) = existing {
            return Ok(Some(format!(
                "project {}",
                r.as_deref().unwrap_or("(no reference)")
            )));
        }
    }
    Ok(None)
}

/// Validate all rows and mark duplicates. This is what the preview stores.
pub async fn preview_rows(
    pool: &SqlitePool,
    rows: Vec<BTreeMap<String, String>>,
) -> AppResult<Vec<PreviewRow>> {
    let mut out = Vec::new();
    let mut seen_refs = HashSet::new();
    let mut seen_titles = HashSet::new();
    for (i, data) in rows.into_iter().enumerate() {
        let errors = validate_row(&data);
        let duplicate_of = find_duplicate(pool, &data, &mut seen_refs, &mut seen_titles).await?;
        out.push(PreviewRow {
            index: i as i64,
            data,
            errors,
            duplicate_of,
        });
    }
    Ok(out)
}

/// Latest published template version — legacy projects must still bind one
/// (`projects.template_version_id` is NOT NULL).
async fn default_template_version(pool: &SqlitePool) -> AppResult<String> {
    sqlx::query_scalar(
        "SELECT tv.id FROM template_versions tv
         JOIN templates t ON t.id = tv.template_id
         WHERE tv.status = 'published'
         ORDER BY CASE t.key WHEN 'base_use' THEN 0 ELSE 1 END, tv.version DESC
         LIMIT 1",
    )
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| {
        AppError::unprocessable(
            "no_template",
            "no published template version exists to bind legacy projects to",
        )
    })
}

/// Find or create the lead stub user (disabled; never gets a usable password).
async fn lead_stub_user(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    name: &str,
    email: &str,
    organisation: &str,
) -> AppResult<String> {
    if let Some(id) = sqlx::query_scalar::<_, String>("SELECT id FROM users WHERE email = ?")
        .bind(email)
        .fetch_optional(&mut **tx)
        .await?
    {
        return Ok(id);
    }
    let id = new_id();
    let now = now_rfc3339();
    sqlx::query(
        "INSERT INTO users (id, email, name, organisation, password_hash, disabled_at, created_at)
         VALUES (?, ?, ?, ?, '!legacy-stub', ?, ?)",
    )
    .bind(&id)
    .bind(email)
    .bind(name)
    .bind(organisation)
    .bind(&now)
    .bind(&now)
    .execute(&mut **tx)
    .await?;
    Ok(id)
}

/// Create one legacy project from a validated row. Caller must have checked
/// the row has no errors and is not a duplicate. Returns the project id.
async fn insert_legacy_project(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    data: &BTreeMap<String, String>,
    template_version_id: &str,
    actor: &crate::authz::Actor,
) -> AppResult<String> {
    let get = |k: &str| data.get(k).map(|s| s.trim()).unwrap_or("");
    let now = now_rfc3339();

    let lead_id =
        lead_stub_user(tx, get("lead_name"), get("lead_email"), get("organisation")).await?;

    let project_id = new_id();
    sqlx::query(
        "INSERT INTO projects
         (id, reference, title, summary, keywords, organisation, template_version_id,
          answers_json, status, start_date, end_date, legacy, closed_reason, version,
          created_by, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, '{}', 'closed', ?, ?, 1, 'legacy import', 1, ?, ?)",
    )
    .bind(&project_id)
    .bind(get("reference"))
    .bind(get("title"))
    .bind(get("summary"))
    .bind(get("keywords"))
    .bind(get("organisation"))
    .bind(template_version_id)
    .bind(get("start_date"))
    .bind(get("end_date"))
    .bind(&lead_id)
    .bind(&now)
    .execute(&mut **tx)
    .await?;

    sqlx::query(
        "INSERT INTO project_members (id, project_id, user_id, role, added_by, added_at)
         VALUES (?, ?, ?, 'lead', ?, ?)",
    )
    .bind(new_id())
    .bind(&project_id)
    .bind(&lead_id)
    .bind(&actor.user_id)
    .bind(&now)
    .execute(&mut **tx)
    .await?;

    if !get("site_name").is_empty() {
        let lat: f64 = get("lat").parse().map_err(AppError::internal)?;
        let lng: f64 = get("lng").parse().map_err(AppError::internal)?;
        let geometry = json!({"type": "Point", "coordinates": [lng, lat]});
        sqlx::query(
            "INSERT INTO project_sites
             (id, project_id, name, geometry_json, min_lat, min_lng, max_lat, max_lng, sensitive, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, 0, ?)",
        )
        .bind(new_id())
        .bind(&project_id)
        .bind(get("site_name"))
        .bind(geometry.to_string())
        .bind(lat)
        .bind(lng)
        .bind(lat)
        .bind(lng)
        .bind(&now)
        .execute(&mut **tx)
        .await?;
    }

    if !get("report_title").is_empty() {
        // A published metadata-only deliverable with the report URL as an
        // external link on an accepted submission (§8).
        let deliverable_id = new_id();
        sqlx::query(
            "INSERT INTO deliverables
             (id, project_id, title, description, kind, due_date, sender_id, recipient_id,
              status, terms_version, team_agreed_at, team_agreed_by, staff_agreed_at,
              staff_agreed_by, publish_level, published_at, published_by, created_by, created_at)
             VALUES (?, ?, ?, ?, 'report', ?, ?, ?, 'accepted', 1, ?, ?, ?, ?,
                     'metadata', ?, ?, ?, ?)",
        )
        .bind(&deliverable_id)
        .bind(&project_id)
        .bind(get("report_title"))
        .bind(get("summary"))
        .bind(get("end_date"))
        .bind(&lead_id)
        .bind(&actor.user_id)
        .bind(&now)
        .bind(&lead_id)
        .bind(&now)
        .bind(&actor.user_id)
        .bind(&now)
        .bind(&actor.user_id)
        .bind(&actor.user_id)
        .bind(&now)
        .execute(&mut **tx)
        .await?;

        let submission_id = new_id();
        sqlx::query(
            "INSERT INTO deliverable_submissions
             (id, deliverable_id, number, submitted_by, note, status, reviewed_by, reviewed_at, created_at)
             VALUES (?, ?, 1, ?, 'legacy report link', 'accepted', ?, ?, ?)",
        )
        .bind(&submission_id)
        .bind(&deliverable_id)
        .bind(&lead_id)
        .bind(&actor.user_id)
        .bind(&now)
        .bind(&now)
        .execute(&mut **tx)
        .await?;
        sqlx::query(
            "INSERT INTO external_links (id, submission_id, url, description, created_at)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(new_id())
        .bind(&submission_id)
        .bind(get("report_url"))
        .bind(get("report_title"))
        .bind(&now)
        .execute(&mut **tx)
        .await?;
    }

    audit::record(
        tx,
        AuditEvent {
            actor_id: Some(actor.user_id.clone()),
            actor_label: actor.name.clone(),
            action: "project.imported".into(),
            entity_type: "project".into(),
            entity_id: project_id.clone(),
            project_id: Some(project_id.clone()),
            visibility: "internal".into(),
            summary: format!("Legacy project '{}' imported", get("title")),
            before: None,
            after: Some(json!({"reference": get("reference"), "legacy": true})),
            reason: None,
        },
    )
    .await?;

    Ok(project_id)
}

/// Re-validate and commit previously previewed rows: creates `legacy=1`
/// closed projects, skipping rows with errors and duplicates. Returns
/// (created, skipped, messages).
pub async fn commit_rows(
    pool: &SqlitePool,
    batch_id: &str,
    rows: &[PreviewRow],
    actor: &crate::authz::Actor,
) -> AppResult<(i64, i64, Vec<String>)> {
    let template_version_id = default_template_version(pool).await?;
    let mut created = 0i64;
    let mut skipped = 0i64;
    let mut messages = Vec::new();

    let mut tx = db::begin_immediate(pool).await?;
    // Claim the batch inside the commit transaction: a second commit of the
    // same batch (or a concurrent one) gets 409 instead of a partial re-run.
    let claimed = sqlx::query(
        "UPDATE import_batches SET status = 'committed'
         WHERE id = ? AND kind = ? AND status = 'previewed'",
    )
    .bind(batch_id)
    .bind(KIND)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if claimed == 0 {
        return Err(AppError::conflict(
            "batch_not_previewed",
            "this import batch was already committed or discarded",
        ));
    }
    let mut seen_refs = HashSet::new();
    let mut seen_titles = HashSet::new();
    for row in rows {
        // Re-validation at commit: structure again, duplicates against the
        // live database (a concurrent import may have landed meanwhile).
        let errors = validate_row(&row.data);
        if !errors.is_empty() {
            skipped += 1;
            messages.push(format!("row {} skipped: field errors", row.index));
            continue;
        }
        // Duplicates: check inside the commit transaction via savepoint-less
        // reads on the pool would race; do the DB check on the tx instead.
        let reference = row.data.get("reference").map(|s| s.trim()).unwrap_or("");
        if !reference.is_empty() && !seen_refs.insert(reference.to_lowercase()) {
            skipped += 1;
            messages.push(format!(
                "row {} skipped: duplicate reference {reference}",
                row.index
            ));
            continue;
        }
        let existing: Option<String> =
            sqlx::query_scalar("SELECT reference FROM projects WHERE reference = ?")
                .bind(reference)
                .fetch_optional(&mut *tx)
                .await?;
        if existing.is_some() {
            skipped += 1;
            messages.push(format!(
                "row {} skipped: reference {} already exists",
                row.index, reference
            ));
            continue;
        }
        let year = row
            .data
            .get("start_date")
            .and_then(|s| s.get(..4))
            .unwrap_or("");
        let title_key = format!(
            "{}|{}|{}",
            norm(row.data.get("title").map(|s| s.as_str()).unwrap_or("")),
            norm(
                row.data
                    .get("organisation")
                    .map(|s| s.as_str())
                    .unwrap_or("")
            ),
            year
        );
        if !seen_titles.insert(title_key) {
            skipped += 1;
            messages.push(format!(
                "row {} skipped: duplicate title/organisation/year",
                row.index
            ));
            continue;
        }
        let dup: Option<String> = sqlx::query_scalar(
            "SELECT id FROM projects
             WHERE lower(title) = lower(?) AND lower(organisation) = lower(?)
               AND substr(start_date, 1, 4) = ? LIMIT 1",
        )
        .bind(row.data.get("title").map(|s| s.trim()).unwrap_or(""))
        .bind(row.data.get("organisation").map(|s| s.trim()).unwrap_or(""))
        .bind(year)
        .fetch_optional(&mut *tx)
        .await?;
        if dup.is_some() {
            skipped += 1;
            messages.push(format!(
                "row {} skipped: matches existing project by title/organisation/year",
                row.index
            ));
            continue;
        }

        insert_legacy_project(&mut tx, &row.data, &template_version_id, actor).await?;
        created += 1;
    }
    tx.commit().await?;
    Ok((created, skipped, messages))
}

/// Build the `preview_json` payload stored on the import batch.
pub fn preview_json(rows: &[PreviewRow]) -> Value {
    json!({
        "total": rows.len(),
        "rows": rows,
    })
}

/// Read preview rows back out of a stored `preview_json`.
pub fn rows_from_preview(preview: &Value) -> AppResult<Vec<PreviewRow>> {
    serde_json::from_value(preview["rows"].clone()).map_err(AppError::internal)
}

/// Store a preview as a `previewed` import batch (+ internal audit event) and
/// return its id. Shared by the admin preview endpoint and the demo seed.
pub async fn create_batch(
    pool: &SqlitePool,
    kind: &str,
    preview: &Value,
    actor: &crate::authz::Actor,
) -> AppResult<(String, String)> {
    let id = new_id();
    let now = now_rfc3339();
    let mut tx = pool.begin().await?;
    sqlx::query(
        "INSERT INTO import_batches (id, kind, status, preview_json, created_by, created_at)
         VALUES (?, ?, 'previewed', ?, ?, ?)",
    )
    .bind(&id)
    .bind(kind)
    .bind(preview.to_string())
    .bind(&actor.user_id)
    .bind(&now)
    .execute(&mut *tx)
    .await?;
    audit::record(
        &mut tx,
        AuditEvent {
            actor_id: Some(actor.user_id.clone()),
            actor_label: actor.name.clone(),
            action: "import.previewed".into(),
            entity_type: "import_batch".into(),
            entity_id: id.clone(),
            project_id: None,
            visibility: "internal".into(),
            summary: format!("Import preview ({kind}) created"),
            before: None,
            after: None,
            reason: None,
        },
    )
    .await?;
    tx.commit().await?;
    Ok((id, now))
}
