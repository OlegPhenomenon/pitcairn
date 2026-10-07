//! Legacy import (§8, spec §5 item 15). `POST /admin/import/legacy/preview`
//! parses and validates rows into an `import_batches` row;
//! `POST /admin/import/{id}/commit` re-validates and creates `legacy=1`
//! closed projects. The demo seed reuses `commit_rows` so legacy projects
//! enter through the same code path.
//!
//! The upload is either a plain CSV or a ZIP holding one CSV at its top level
//! plus a `files/` folder with the old documents and result files the CSV
//! names. CSV columns: `reference,title,organisation,lead_name,lead_email,
//! start_date,end_date,summary,keywords,site_name,lat,lng,report_title,
//! report_url,report_file,dataset_title,dataset_file,application_file`.
//!
//! File columns (ZIP only; names are paths below `files/`):
//! - `application_file` — `;`-separated, oldest first: the successive
//!   versions of the project's application document.
//! - `report_file` — one file: the report, attached to the accepted report
//!   deliverable (together with `report_url` when both are given).
//! - `dataset_file` — `;`-separated: the files of an accepted dataset
//!   deliverable titled `dataset_title`.
//!
//! Preview checks every referenced file (present, allowed type, within the
//! upload size limit, passes the same scanner as uploads) and stores the clean
//! ones exactly like a completed upload (content-addressed bytes + `files`
//! row), so commit only links them.

use std::collections::{BTreeMap, HashSet};
use std::io::{Cursor, Read};
use std::path::Path;

use serde_json::{Value, json};
use sqlx::SqlitePool;

use crate::audit::{self, AuditEvent};
use crate::db;
use crate::error::{AppError, AppResult};
use crate::util::{new_id, now_rfc3339};

pub const KIND: &str = "legacy_csv";

pub const COLUMNS: [&str; 18] = [
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
    "report_file",
    "dataset_title",
    "dataset_file",
    "application_file",
];

/// Columns naming files inside the ZIP's `files/` folder.
pub const FILE_COLUMNS: [&str; 3] = ["application_file", "report_file", "dataset_file"];

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

/// A referenced file that passed preview: stored and registered in `files`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RowFile {
    pub column: String,
    /// Path below `files/` as written in the CSV (normalized).
    pub name: String,
    pub file_id: String,
    pub size: i64,
    pub mime: String,
}

/// One preview row: the original data plus validation outcome.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PreviewRow {
    pub index: i64,
    pub data: BTreeMap<String, String>,
    pub errors: BTreeMap<String, String>,
    pub duplicate_of: Option<String>,
    #[serde(default)]
    pub files: Vec<RowFile>,
}

/// An entry of the ZIP's `files/` folder.
pub enum BundleFile {
    Bytes(Vec<u8>),
    /// Larger than the per-file upload limit; the bytes were not kept.
    TooLarge,
}

/// A parsed import ZIP: the CSV plus `files/` entries keyed by their path
/// below `files/`.
pub struct Bundle {
    pub csv: Vec<u8>,
    pub files: BTreeMap<String, BundleFile>,
}

pub fn is_zip(bytes: &[u8]) -> bool {
    bytes.starts_with(b"PK\x03\x04")
}

/// Read an import ZIP: exactly one `*.csv` at the top level and any number of
/// files below `files/`. A single wrapping folder (as made by "compress
/// folder") is tolerated, macOS metadata entries are ignored. Unsafe paths are
/// refused outright. Every decompressed byte — kept or not — counts against
/// `budget` (the route passes `archive::MAX_UNCOMPRESSED`), and reading stops
/// the moment an entry passes `max_file_bytes` or the remaining budget, so a
/// ZIP bomb never inflates beyond either bound. Single files above
/// `max_file_bytes` are kept as `TooLarge` so the preview can flag the rows
/// that need them.
pub fn parse_bundle(bytes: &[u8], max_file_bytes: u64, budget: u64) -> AppResult<Bundle> {
    let bad = |m: String| AppError::unprocessable("bad_zip", m);
    let mut zip =
        zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| bad(format!("invalid ZIP: {e}")))?;

    let mut entries: Vec<(String, BundleFile)> = Vec::new();
    let mut total: u64 = 0;
    for i in 0..zip.len() {
        let mut file = zip
            .by_index(i)
            .map_err(|e| bad(format!("ZIP entry {i}: {e}")))?;
        // enclosed_name() rejects `../` and absolute paths.
        let Some(path) = file.enclosed_name() else {
            return Err(AppError::unprocessable(
                "unsafe_archive",
                "ZIP entry has an unsafe path",
            ));
        };
        let name = path.to_string_lossy().replace('\\', "/");
        if file.is_dir()
            || name.starts_with("__MACOSX/")
            || name.rsplit('/').next() == Some(".DS_Store")
        {
            continue;
        }
        // Declared sizes can lie (zip bombs): bound the bytes actually
        // inflated by the per-file limit AND what is left of the budget; one
        // byte past the smaller bound is enough to know it was crossed.
        let limit = max_file_bytes.min(budget - total);
        let mut buf = Vec::with_capacity(file.size().min(limit).min(64 << 20) as usize);
        (&mut file)
            .take(limit + 1)
            .read_to_end(&mut buf)
            .map_err(|e| bad(format!("read {name}: {e}")))?;
        total += buf.len() as u64;
        if total > budget {
            return Err(AppError::unprocessable(
                "archive_too_large",
                format!("ZIP expands to more than {budget} bytes uncompressed"),
            ));
        }
        let content = if buf.len() as u64 > max_file_bytes {
            BundleFile::TooLarge
        } else {
            BundleFile::Bytes(buf)
        };
        entries.push((name, content));
    }

    // One wrapping folder around everything (but not `files/` itself).
    let wrapper = entries
        .first()
        .and_then(|(n, _)| n.split_once('/'))
        .map(|(dir, _)| format!("{dir}/"))
        .filter(|dir| dir != "files/" && entries.iter().all(|(n, _)| n.starts_with(dir.as_str())));

    let mut csv: Option<BundleFile> = None;
    let mut files = BTreeMap::new();
    for (name, content) in entries {
        let name = match &wrapper {
            Some(w) => name[w.len()..].to_string(),
            None => name,
        };
        if let Some(rel) = name.strip_prefix("files/") {
            files.insert(rel.to_string(), content);
        } else if !name.contains('/') && name.to_lowercase().ends_with(".csv") {
            if csv.is_some() {
                return Err(bad(
                    "the ZIP must contain exactly one CSV file at its top level".into(),
                ));
            }
            csv = Some(content);
        }
    }
    let csv = match csv {
        Some(BundleFile::Bytes(b)) => b,
        Some(BundleFile::TooLarge) => {
            return Err(bad("the CSV file exceeds the upload size limit".into()));
        }
        None => {
            return Err(bad(
                "the ZIP must contain one CSV file at its top level".into()
            ));
        }
    };
    if let Some(reason) = crate::jobs::scan_bytes(&csv, "text/csv") {
        return Err(AppError::unprocessable("file_rejected", reason));
    }
    Ok(Bundle { csv, files })
}

/// Allowed file types by extension, with the mime the scanner checks them
/// against.
fn mime_for(name: &str) -> Option<&'static str> {
    let ext = name.rsplit_once('.')?.1.to_lowercase();
    Some(match ext.as_str() {
        "pdf" => "application/pdf",
        "txt" => "text/plain",
        "md" => "text/markdown",
        "csv" => "text/csv",
        "tsv" => "text/tab-separated-values",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "tif" | "tiff" => "image/tiff",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "zip" => "application/zip",
        "nc" => "application/x-netcdf",
        _ => return None,
    })
}

/// `(column, name)` for every file a row references, names normalized to
/// paths below `files/`. Multi-file columns split on `;`.
fn file_refs(data: &BTreeMap<String, String>) -> Vec<(&'static str, String)> {
    let mut out = Vec::new();
    for col in FILE_COLUMNS {
        let value = data.get(col).map(|s| s.as_str()).unwrap_or("");
        for part in value.split(';') {
            let name = part.trim().trim_start_matches("./");
            let name = name.strip_prefix("files/").unwrap_or(name);
            if !name.is_empty() {
                out.push((col, name.to_string()));
            }
        }
    }
    out
}

fn add_error(errors: &mut BTreeMap<String, String>, column: &str, msg: String) {
    errors
        .entry(column.to_string())
        .and_modify(|e| {
            e.push_str("; ");
            e.push_str(&msg);
        })
        .or_insert(msg);
}

/// Where preview takes referenced files from (ZIP uploads only).
pub struct FileSource<'a> {
    pub files: &'a BTreeMap<String, BundleFile>,
    pub data_dir: &'a Path,
    pub max_file_bytes: u64,
    pub uploaded_by: &'a str,
}

/// Check one referenced file and, when it is acceptable, store it the way a
/// completed upload is stored (content-addressed bytes, `files` row deduped by
/// sha256) with the scanner verdict. `Ok(Err(msg))` is a row error.
async fn check_and_store(
    pool: &SqlitePool,
    src: &FileSource<'_>,
    column: &str,
    name: &str,
) -> AppResult<Result<RowFile, String>> {
    let Some(content) = src.files.get(name) else {
        return Ok(Err(format!("{name}: not found in the ZIP's files/ folder")));
    };
    let BundleFile::Bytes(bytes) = content else {
        return Ok(Err(format!(
            "{name}: larger than the {} MB per-file limit",
            src.max_file_bytes.div_ceil(1024 * 1024)
        )));
    };
    let Some(mime) = mime_for(name) else {
        return Ok(Err(format!(
            "{name}: file type not allowed (use PDF, text, CSV/TSV, images, DOCX, XLSX, ZIP or NetCDF)"
        )));
    };
    if bytes.is_empty() {
        return Ok(Err(format!("{name}: file is empty")));
    }
    if let Some(reason) = crate::jobs::scan_bytes(bytes, mime) {
        return Ok(Err(format!("{name}: rejected by the file scan ({reason})")));
    }

    let sha256 = crate::util::sha256_hex(bytes);
    let part = crate::files::part_path(src.data_dir, &format!("legacy-{}", new_id()));
    tokio::fs::write(&part, bytes).await?;
    crate::files::store_file(src.data_dir, &part, &sha256).await?;
    let storage_key = crate::files::file_path(src.data_dir, &sha256)?
        .to_string_lossy()
        .to_string();
    sqlx::query(
        "INSERT INTO files (id, sha256, size, mime, storage_key, scan_status, scan_detail, uploaded_by, created_at)
         VALUES (?, ?, ?, ?, ?, 'clean', NULL, ?, ?)
         ON CONFLICT(sha256) DO NOTHING",
    )
    .bind(new_id())
    .bind(&sha256)
    .bind(bytes.len() as i64)
    .bind(mime)
    .bind(&storage_key)
    .bind(src.uploaded_by)
    .bind(now_rfc3339())
    .execute(pool)
    .await?;
    let (file_id, scan_status, size, stored_mime): (String, String, i64, String) =
        sqlx::query_as("SELECT id, scan_status, size, mime FROM files WHERE sha256 = ?")
            .bind(&sha256)
            .fetch_one(pool)
            .await?;
    if scan_status != "clean" {
        return Ok(Err(format!(
            "{name}: identical bytes were already rejected by the file scan"
        )));
    }
    Ok(Ok(RowFile {
        column: column.to_string(),
        name: name.to_string(),
        file_id,
        size,
        mime: stored_mime,
    }))
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

    let has_report_source = !get("report_url").is_empty() || !get("report_file").trim().is_empty();
    if has_report_source && get("report_title").trim().is_empty() {
        errors.insert(
            "report_title".into(),
            "required when report_url or report_file is given".into(),
        );
    }
    if !get("report_title").trim().is_empty() && !has_report_source {
        errors.insert(
            "report_url".into(),
            "give report_url or report_file together with report_title".into(),
        );
    }
    if get("report_file").contains(';') {
        errors.insert("report_file".into(), "only one report file per row".into());
    }
    if !get("dataset_title").trim().is_empty() && get("dataset_file").trim().is_empty() {
        errors.insert(
            "dataset_file".into(),
            "required when dataset_title is given".into(),
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

/// Validate all rows of a plain CSV and mark duplicates. This is what the
/// preview stores. File columns need a ZIP, so any file reference is an error.
pub async fn preview_rows(
    pool: &SqlitePool,
    rows: Vec<BTreeMap<String, String>>,
) -> AppResult<Vec<PreviewRow>> {
    preview(pool, rows, None).await
}

/// Validate the rows of a ZIP upload: like `preview_rows`, plus every
/// referenced file is checked and, when acceptable, stored.
pub async fn preview_bundle(
    pool: &SqlitePool,
    rows: Vec<BTreeMap<String, String>>,
    src: &FileSource<'_>,
) -> AppResult<Vec<PreviewRow>> {
    preview(pool, rows, Some(src)).await
}

async fn preview(
    pool: &SqlitePool,
    rows: Vec<BTreeMap<String, String>>,
    src: Option<&FileSource<'_>>,
) -> AppResult<Vec<PreviewRow>> {
    let mut out = Vec::new();
    let mut seen_refs = HashSet::new();
    let mut seen_titles = HashSet::new();
    for (i, data) in rows.into_iter().enumerate() {
        let mut errors = validate_row(&data);
        let duplicate_of = find_duplicate(pool, &data, &mut seen_refs, &mut seen_titles).await?;
        let mut files = Vec::new();
        let mut seen_names = HashSet::new();
        for (column, name) in file_refs(&data) {
            let Some(src) = src else {
                add_error(
                    &mut errors,
                    column,
                    "files can only be imported from a ZIP with a files/ folder".into(),
                );
                continue;
            };
            if !seen_names.insert((column, name.clone())) {
                add_error(&mut errors, column, format!("{name}: listed twice"));
                continue;
            }
            match check_and_store(pool, src, column, &name).await? {
                Ok(f) => files.push(f),
                Err(msg) => add_error(&mut errors, column, msg),
            }
        }
        out.push(PreviewRow {
            index: i as i64,
            data,
            errors,
            duplicate_of,
            files,
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

/// A document with one version per file (oldest first). Returns the version
/// ids in the same order.
async fn insert_document(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    project_id: &str,
    title: &str,
    category: &str,
    files: &[&RowFile],
    uploaded_by: &str,
    now: &str,
) -> AppResult<Vec<String>> {
    let document_id = new_id();
    sqlx::query(
        "INSERT INTO documents (id, project_id, slot_key, title, category, created_by, created_at)
         VALUES (?, ?, NULL, ?, ?, ?, ?)",
    )
    .bind(&document_id)
    .bind(project_id)
    .bind(title)
    .bind(category)
    .bind(uploaded_by)
    .bind(now)
    .execute(&mut **tx)
    .await?;
    let mut version_ids = Vec::new();
    for (i, f) in files.iter().enumerate() {
        let version_id = new_id();
        sqlx::query(
            "INSERT INTO document_versions
             (id, document_id, number, file_id, note, uploaded_by, uploaded_at, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&version_id)
        .bind(&document_id)
        .bind(i as i64 + 1)
        .bind(&f.file_id)
        .bind(format!("Imported from legacy file {}", f.name))
        .bind(uploaded_by)
        .bind(now)
        .bind(now)
        .execute(&mut **tx)
        .await?;
        version_ids.push(version_id);
    }
    Ok(version_ids)
}

/// Base name of a `files/` path, used as a document title.
fn base_name(name: &str) -> &str {
    name.rsplit('/').next().unwrap_or(name)
}

struct LegacyDeliverable<'a> {
    project_id: &'a str,
    kind: &'a str,
    title: &'a str,
    description: &'a str,
    due_date: &'a str,
    lead_id: &'a str,
    /// Result files (document versions) of the accepted submission.
    version_ids: &'a [String],
    /// `(url, description)` of an external link on the submission.
    link: Option<(&'a str, &'a str)>,
}

/// A published metadata-only deliverable with one accepted submission carrying
/// the result files and/or external link (§8).
async fn insert_accepted_deliverable(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    d: LegacyDeliverable<'_>,
    actor: &crate::authz::Actor,
    now: &str,
) -> AppResult<()> {
    let deliverable_id = new_id();
    sqlx::query(
        "INSERT INTO deliverables
         (id, project_id, title, description, kind, due_date, sender_id, recipient_id,
          status, terms_version, team_agreed_at, team_agreed_by, staff_agreed_at,
          staff_agreed_by, publish_level, published_at, published_by, created_by, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, 'accepted', 1, ?, ?, ?, ?,
                 'metadata', ?, ?, ?, ?)",
    )
    .bind(&deliverable_id)
    .bind(d.project_id)
    .bind(d.title)
    .bind(d.description)
    .bind(d.kind)
    .bind(d.due_date)
    .bind(d.lead_id)
    .bind(&actor.user_id)
    .bind(now)
    .bind(d.lead_id)
    .bind(now)
    .bind(&actor.user_id)
    .bind(now)
    .bind(&actor.user_id)
    .bind(&actor.user_id)
    .bind(now)
    .execute(&mut **tx)
    .await?;

    let submission_id = new_id();
    sqlx::query(
        "INSERT INTO deliverable_submissions
         (id, deliverable_id, number, submitted_by, note, status, reviewed_by, reviewed_at, created_at)
         VALUES (?, ?, 1, ?, 'Imported from legacy records', 'accepted', ?, ?, ?)",
    )
    .bind(&submission_id)
    .bind(&deliverable_id)
    .bind(d.lead_id)
    .bind(&actor.user_id)
    .bind(now)
    .bind(now)
    .execute(&mut **tx)
    .await?;
    for version_id in d.version_ids {
        sqlx::query(
            "INSERT INTO submission_files (id, submission_id, document_version_id, created_at)
             VALUES (?, ?, ?, ?)",
        )
        .bind(new_id())
        .bind(&submission_id)
        .bind(version_id)
        .bind(now)
        .execute(&mut **tx)
        .await?;
    }
    if let Some((url, description)) = d.link {
        sqlx::query(
            "INSERT INTO external_links (id, submission_id, url, description, created_at)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(new_id())
        .bind(&submission_id)
        .bind(url)
        .bind(description)
        .bind(now)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

/// Create one legacy project from a validated row. Caller must have checked
/// the row has no errors, is not a duplicate and that `files` covers every
/// file reference with a clean stored file. Returns the project id.
async fn insert_legacy_project(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    data: &BTreeMap<String, String>,
    files: &[RowFile],
    template_version_id: &str,
    actor: &crate::authz::Actor,
) -> AppResult<String> {
    let get = |k: &str| data.get(k).map(|s| s.trim()).unwrap_or("");
    let now = now_rfc3339();
    let files_of = |column: &str| -> Vec<&RowFile> {
        // `file_refs` order: oldest version / listed order first.
        file_refs(data)
            .into_iter()
            .filter(|(c, _)| *c == column)
            .filter_map(|(_, name)| files.iter().find(|f| f.column == column && f.name == name))
            .collect()
    };

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

    let mut documents = 0usize;
    let application = files_of("application_file");
    if !application.is_empty() {
        insert_document(
            tx,
            &project_id,
            "Application",
            "application",
            &application,
            &lead_id,
            &now,
        )
        .await?;
        documents += 1;
    }

    let mut deliverables = 0usize;
    if !get("report_title").is_empty() {
        let report = files_of("report_file");
        let version_ids = if report.is_empty() {
            Vec::new()
        } else {
            documents += 1;
            insert_document(
                tx,
                &project_id,
                get("report_title"),
                "result",
                &report,
                &lead_id,
                &now,
            )
            .await?
        };
        let link =
            (!get("report_url").is_empty()).then(|| (get("report_url"), get("report_title")));
        insert_accepted_deliverable(
            tx,
            LegacyDeliverable {
                project_id: &project_id,
                kind: "report",
                title: get("report_title"),
                description: get("summary"),
                due_date: get("end_date"),
                lead_id: &lead_id,
                version_ids: &version_ids,
                link,
            },
            actor,
            &now,
        )
        .await?;
        deliverables += 1;
    }

    let dataset = files_of("dataset_file");
    if !dataset.is_empty() {
        let mut version_ids = Vec::new();
        for f in &dataset {
            version_ids.extend(
                insert_document(
                    tx,
                    &project_id,
                    base_name(&f.name),
                    "result",
                    &[*f],
                    &lead_id,
                    &now,
                )
                .await?,
            );
            documents += 1;
        }
        let title = match get("dataset_title") {
            "" => "Dataset",
            t => t,
        };
        insert_accepted_deliverable(
            tx,
            LegacyDeliverable {
                project_id: &project_id,
                kind: "dataset",
                title,
                description: get("summary"),
                due_date: get("end_date"),
                lead_id: &lead_id,
                version_ids: &version_ids,
                link: None,
            },
            actor,
            &now,
        )
        .await?;
        deliverables += 1;
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
            after: Some(json!({
                "reference": get("reference"),
                "legacy": true,
                "documents": documents,
                "deliverables": deliverables,
            })),
            reason: None,
        },
    )
    .await?;

    Ok(project_id)
}

/// Commit-time file re-validation: every file the row references was stored
/// at preview and its `files` row is still clean.
async fn row_files_ready(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    row: &PreviewRow,
) -> AppResult<bool> {
    for (column, name) in file_refs(&row.data) {
        let Some(f) = row
            .files
            .iter()
            .find(|f| f.column == column && f.name == name)
        else {
            return Ok(false);
        };
        let status: Option<String> =
            sqlx::query_scalar("SELECT scan_status FROM files WHERE id = ?")
                .bind(&f.file_id)
                .fetch_optional(&mut **tx)
                .await?;
        if status.as_deref() != Some("clean") {
            return Ok(false);
        }
    }
    Ok(true)
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
        // Re-validation at commit: structure again (plus the file errors the
        // preview recorded), duplicates against the live database (a
        // concurrent import may have landed meanwhile).
        let errors = validate_row(&row.data);
        if !errors.is_empty() || !row.errors.is_empty() {
            skipped += 1;
            messages.push(format!("row {} skipped: field errors", row.index));
            continue;
        }
        if !row_files_ready(&mut tx, row).await? {
            skipped += 1;
            messages.push(format!(
                "row {} skipped: referenced files are missing or not clean; preview again",
                row.index
            ));
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

        insert_legacy_project(&mut tx, &row.data, &row.files, &template_version_id, actor).await?;
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

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    /// Deflated ZIP: a CSV plus `zero_entries` zero-filled files of
    /// `entry_len` bytes each (they compress to almost nothing).
    fn bomb(zero_entries: usize, entry_len: usize) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        zip.start_file("projects.csv", opts).unwrap();
        zip.write_all(b"reference,title\n").unwrap();
        let zeros = vec![0u8; entry_len];
        for i in 0..zero_entries {
            zip.start_file(format!("files/zero-{i}.bin"), opts).unwrap();
            zip.write_all(&zeros).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    #[test]
    fn oversized_entries_count_against_the_decompression_budget() {
        // 1 KiB per-file limit, 2 KiB entries: each is cut at 1 KiB + 1 byte
        // and flagged, yet those inflated bytes still count. Ten of them
        // (~10 KiB inflated) blow a 4 KiB budget.
        let zip = bomb(10, 2048);
        assert!(zip.len() < 4096, "the bomb itself is small");
        let err = parse_bundle(&zip, 1024, 4096)
            .err()
            .expect("budget exceeded");
        match err {
            AppError::Unprocessable { code, .. } => assert_eq!(code, "archive_too_large"),
            other => panic!("unexpected error {other:?}"),
        }

        // Within budget the same entries are reported as too large, never
        // kept.
        let bundle = parse_bundle(&bomb(2, 2048), 1024, 1 << 20).expect("within budget");
        assert_eq!(bundle.files.len(), 2);
        assert!(
            bundle
                .files
                .values()
                .all(|f| matches!(f, BundleFile::TooLarge))
        );
    }
}
