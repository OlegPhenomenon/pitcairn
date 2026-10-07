//! Project export/import archives (§8). A ZIP with `manifest.json`,
//! `records/<table>.json` for every project-scoped table plus the dependency
//! closure (template versions, resources+tariffs, users as stubs), and
//! `files/<sha256>` payloads. Import is idempotent on project id, never
//! imports roles/passwords/TOTP secrets, and re-validates everything at
//! commit.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};
use sqlx::SqlitePool;
use sqlx::sqlite::SqliteRow;

use crate::audit::{self, AuditEvent};
use crate::db;
use crate::error::{AppError, AppResult};
use crate::util::{new_id, now_rfc3339};

pub const FORMAT: &str = "pitcairn-project-export";
pub const SCHEMA_VERSION: i64 = 1;
pub const MAX_UNCOMPRESSED: u64 = 10 * 1024 * 1024 * 1024; // 10 GiB
pub const KIND: &str = "project_archive";

/// Project-scoped tables, each selected by `?` = project_id (§8 "records/<table>.json").
/// Order matters on import for FK dependencies; deferred-column updates are
/// applied afterwards (see `DEFERRED_COLUMNS`).
const PROJECT_TABLES: &[(&str, &str)] = &[
    ("projects", "SELECT * FROM projects WHERE id = ?"),
    (
        "project_sites",
        "SELECT * FROM project_sites WHERE project_id = ?",
    ),
    (
        "project_members",
        "SELECT * FROM project_members WHERE project_id = ?",
    ),
    (
        "project_revisions",
        "SELECT * FROM project_revisions WHERE project_id = ?",
    ),
    (
        "invitations",
        "SELECT * FROM invitations WHERE project_id = ?",
    ),
    ("documents", "SELECT * FROM documents WHERE project_id = ?"),
    (
        "document_versions",
        "SELECT * FROM document_versions WHERE document_id IN
         (SELECT id FROM documents WHERE project_id = ?)",
    ),
    ("threads", "SELECT * FROM threads WHERE project_id = ?"),
    (
        "messages",
        "SELECT * FROM messages WHERE thread_id IN
         (SELECT id FROM threads WHERE project_id = ?)",
    ),
    (
        "action_items",
        "SELECT * FROM action_items WHERE project_id = ?",
    ),
    (
        "review_assignments",
        "SELECT * FROM review_assignments WHERE project_id = ?",
    ),
    (
        "change_requests",
        "SELECT * FROM change_requests WHERE project_id = ?",
    ),
    ("decisions", "SELECT * FROM decisions WHERE project_id = ?"),
    ("trips", "SELECT * FROM trips WHERE project_id = ?"),
    (
        "bookings",
        "SELECT * FROM bookings WHERE trip_id IN
         (SELECT id FROM trips WHERE project_id = ?)",
    ),
    ("invoices", "SELECT * FROM invoices WHERE project_id = ?"),
    (
        "invoice_lines",
        "SELECT * FROM invoice_lines WHERE invoice_id IN
         (SELECT id FROM invoices WHERE project_id = ?)",
    ),
    (
        "payments",
        "SELECT * FROM payments WHERE invoice_id IN
         (SELECT id FROM invoices WHERE project_id = ?)",
    ),
    (
        "deliverables",
        "SELECT * FROM deliverables WHERE project_id = ?",
    ),
    (
        "deliverable_due_changes",
        "SELECT * FROM deliverable_due_changes WHERE deliverable_id IN
         (SELECT id FROM deliverables WHERE project_id = ?)",
    ),
    (
        "deliverable_submissions",
        "SELECT * FROM deliverable_submissions WHERE deliverable_id IN
         (SELECT id FROM deliverables WHERE project_id = ?)",
    ),
    (
        "submission_files",
        "SELECT * FROM submission_files WHERE submission_id IN
         (SELECT id FROM deliverable_submissions WHERE deliverable_id IN
          (SELECT id FROM deliverables WHERE project_id = ?))",
    ),
    (
        "external_links",
        "SELECT * FROM external_links WHERE submission_id IN
         (SELECT id FROM deliverable_submissions WHERE deliverable_id IN
          (SELECT id FROM deliverables WHERE project_id = ?))",
    ),
    (
        "publication_files",
        "SELECT * FROM publication_files WHERE deliverable_id IN
         (SELECT id FROM deliverables WHERE project_id = ?)",
    ),
    ("samples", "SELECT * FROM samples WHERE project_id = ?"),
    (
        "measurements",
        "SELECT * FROM measurements WHERE project_id = ?",
    ),
    (
        "notifications",
        "SELECT * FROM notifications WHERE project_id = ?",
    ),
    (
        "audit_events",
        "SELECT * FROM audit_events WHERE project_id = ?",
    ),
];

/// Columns written on a second pass after all rows exist (circular FKs:
/// change_requests → decisions and decisions → decisions).
const DEFERRED_COLUMNS: &[(&str, &str)] = &[
    ("change_requests", "resulting_decision_id"),
    ("decisions", "supersedes_id"),
    ("decisions", "superseded_by_id"),
];

/// Every table name an archive may carry under `records/`: the project
/// tables plus the dependency closure (users are stubs, handled separately).
const IMPORTABLE_TABLES: &[&str] = &[
    "projects",
    "project_sites",
    "project_members",
    "project_revisions",
    "invitations",
    "documents",
    "document_versions",
    "threads",
    "messages",
    "action_items",
    "review_assignments",
    "change_requests",
    "decisions",
    "trips",
    "bookings",
    "invoices",
    "invoice_lines",
    "payments",
    "deliverables",
    "deliverable_due_changes",
    "deliverable_submissions",
    "submission_files",
    "external_links",
    "publication_files",
    "samples",
    "measurements",
    "notifications",
    "audit_events",
    "templates",
    "template_versions",
    "resources",
    "tariffs",
    "files",
];

/// Project-scoped table names in FK-safe import order.
pub fn project_tables() -> impl Iterator<Item = &'static str> {
    PROJECT_TABLES.iter().map(|(t, _)| *t)
}

fn deferred(table: &str, column: &str) -> bool {
    DEFERRED_COLUMNS
        .iter()
        .any(|(t, c)| *t == table && *c == column)
}

// ---------------------------------------------------------------------------
// Row <-> JSON
// ---------------------------------------------------------------------------

fn cell_to_json(row: &SqliteRow, i: usize) -> AppResult<Value> {
    use sqlx::decode::Decode;
    use sqlx::{Row, ValueRef};
    let raw = row.try_get_raw(i).map_err(AppError::internal)?;
    if raw.is_null() {
        return Ok(Value::Null);
    }
    let ty = raw.type_info().to_string();
    let v = match ty.as_str() {
        "INTEGER" | "INT" | "BIGINT" | "SMALLINT" | "TINYINT" | "INT2" | "INT8" => {
            Value::from(<i64 as Decode<sqlx::Sqlite>>::decode(raw).map_err(AppError::internal)?)
        }
        "REAL" | "DOUBLE" | "DOUBLE PRECISION" | "FLOAT" | "NUMERIC" => {
            Value::from(<f64 as Decode<sqlx::Sqlite>>::decode(raw).map_err(AppError::internal)?)
        }
        "BLOB" => Value::from(hex::encode(
            <Vec<u8> as Decode<sqlx::Sqlite>>::decode(raw).map_err(AppError::internal)?,
        )),
        _ => {
            Value::from(<String as Decode<sqlx::Sqlite>>::decode(raw).map_err(AppError::internal)?)
        }
    };
    Ok(v)
}

async fn rows_as_json(
    pool: &SqlitePool,
    sql: &str,
    project_id: &str,
) -> AppResult<Vec<Map<String, Value>>> {
    use sqlx::{Column, Row};
    let rows: Vec<SqliteRow> = sqlx::query(sql).bind(project_id).fetch_all(pool).await?;
    let mut out = Vec::new();
    for row in &rows {
        let mut obj = Map::new();
        for (i, col) in row.columns().iter().enumerate() {
            obj.insert(col.name().to_string(), cell_to_json(row, i)?);
        }
        out.push(obj);
    }
    Ok(out)
}

fn bind_value<'q>(
    q: sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments<'q>>,
    v: &Value,
) -> sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments<'q>> {
    match v {
        Value::Null => q.bind(Option::<String>::None),
        Value::Bool(b) => q.bind(*b as i64),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                q.bind(i)
            } else if let Some(u) = n.as_u64() {
                q.bind(u as i64)
            } else {
                q.bind(n.as_f64().unwrap_or(0.0))
            }
        }
        Value::String(s) => q.bind(s.clone()),
        other => q.bind(other.to_string()),
    }
}

// ---------------------------------------------------------------------------
// Export
// ---------------------------------------------------------------------------

/// `install_id` identifies the exporting install (§8 manifest); generated once
/// and persisted in settings.
async fn install_id(exec: &SqlitePool) -> AppResult<String> {
    if let Some(v) =
        sqlx::query_scalar::<_, String>("SELECT value FROM settings WHERE key = 'install_id'")
            .fetch_optional(exec)
            .await?
    {
        return Ok(v);
    }
    let id = new_id();
    sqlx::query("INSERT OR IGNORE INTO settings (key, value) VALUES ('install_id', ?)")
        .bind(&id)
        .execute(exec)
        .await?;
    Ok(id)
}

pub struct ExportResult {
    pub bytes: Vec<u8>,
    pub project_reference: Option<String>,
    pub project_title: String,
    pub project_id: String,
}

/// Build the export ZIP for `project_id`.
pub async fn export_project(
    pool: &SqlitePool,
    data_dir: &Path,
    project_id: &str,
) -> AppResult<ExportResult> {
    let project_rows =
        rows_as_json(pool, "SELECT * FROM projects WHERE id = ?", project_id).await?;
    let project = project_rows.first().ok_or(AppError::NotFound)?;
    let reference = project["reference"].as_str().map(|s| s.to_string());
    let title = project["title"].as_str().unwrap_or_default().to_string();

    let mut records: BTreeMap<String, Vec<Map<String, Value>>> = BTreeMap::new();
    for (table, sql) in PROJECT_TABLES {
        records.insert(
            (*table).to_string(),
            rows_as_json(pool, sql, project_id).await?,
        );
    }

    // Dependency closure: template versions bound by the project and its
    // revisions, plus their parent templates.
    let tv_ids: Vec<String> = {
        let mut ids: Vec<String> = Vec::new();
        for t in ["projects", "project_revisions"] {
            for row in &records[t] {
                if let Some(v) = row["template_version_id"].as_str() {
                    ids.push(v.to_string());
                }
            }
        }
        ids.sort();
        ids.dedup();
        ids
    };
    let mut template_ids: Vec<String> = Vec::new();
    if !tv_ids.is_empty() {
        let mut versions = Vec::new();
        for id in &tv_ids {
            let rows =
                rows_as_json(pool, "SELECT * FROM template_versions WHERE id = ?", id).await?;
            for r in rows {
                if let Some(t) = r["template_id"].as_str() {
                    template_ids.push(t.to_string());
                }
                versions.push(r);
            }
        }
        records.insert("template_versions".into(), versions);
        template_ids.sort();
        template_ids.dedup();
        let mut templates = Vec::new();
        for id in &template_ids {
            templates.extend(rows_as_json(pool, "SELECT * FROM templates WHERE id = ?", id).await?);
        }
        records.insert("templates".into(), templates);
    }

    // Resources + tariffs referenced by the project's bookings.
    let resource_ids: Vec<String> = records["bookings"]
        .iter()
        .filter_map(|r| r["resource_id"].as_str().map(String::from))
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    let mut resources = Vec::new();
    for id in &resource_ids {
        resources.extend(rows_as_json(pool, "SELECT * FROM resources WHERE id = ?", id).await?);
    }
    let mut tariffs = Vec::new();
    for id in &resource_ids {
        tariffs
            .extend(rows_as_json(pool, "SELECT * FROM tariffs WHERE resource_id = ?", id).await?);
    }
    records.insert("resources".into(), resources);
    records.insert("tariffs".into(), tariffs);

    // File metadata for every document version of the project.
    let file_ids: Vec<String> = records["document_versions"]
        .iter()
        .filter_map(|r| r["file_id"].as_str().map(String::from))
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    let mut files = Vec::new();
    let mut file_shas: Vec<String> = Vec::new();
    for id in &file_ids {
        for row in rows_as_json(pool, "SELECT * FROM files WHERE id = ?", id).await? {
            // storage_key is an absolute path on the source install; the
            // importer recomputes it — export the sha-derived relative form.
            let mut row = row;
            if let Some(sha) = row["sha256"].as_str().map(str::to_string) {
                row.insert(
                    "storage_key".into(),
                    Value::from(crate::files::storage_rel(&sha)?),
                );
                file_shas.push(sha.to_string());
            }
            files.push(row);
        }
    }
    file_shas.sort();
    file_shas.dedup();
    records.insert("files".into(), files);

    // Users: every string cell that is a known user id becomes a stub
    // ({id,email,name,organisation} — never roles/passwords/TOTP), plus ids
    // embedded in trips.participants_json arrays.
    let mut candidate_ids: HashSet<String> = HashSet::new();
    for rows in records.values() {
        for row in rows {
            for (k, v) in row.iter() {
                if let Some(s) = v.as_str() {
                    candidate_ids.insert(s.to_string());
                }
                if k == "participants_json"
                    && let Some(arr) = v
                        .as_str()
                        .and_then(|s| serde_json::from_str::<Value>(s).ok())
                    && let Some(items) = arr.as_array()
                {
                    for it in items {
                        if let Some(s) = it.as_str() {
                            candidate_ids.insert(s.to_string());
                        }
                    }
                }
            }
        }
    }
    let mut users: Vec<Map<String, Value>> = Vec::new();
    for id in &candidate_ids {
        let stub: Option<(String, String, String, String)> =
            sqlx::query_as("SELECT id, email, name, organisation FROM users WHERE id = ?")
                .bind(id)
                .fetch_optional(pool)
                .await?;
        if let Some((id, email, name, organisation)) = stub {
            users.push(Map::from_iter([
                ("id".into(), Value::from(id)),
                ("email".into(), Value::from(email)),
                ("name".into(), Value::from(name)),
                ("organisation".into(), Value::from(organisation)),
            ]));
        }
    }
    users.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
    records.insert("users".into(), users);

    // ---- write the ZIP ----
    let source = install_id(pool).await?;
    let manifest = json!({
        "format": FORMAT,
        "schema_version": SCHEMA_VERSION,
        "exported_at": now_rfc3339(),
        "source_install_id": source,
        "project_id": project_id,
        "project_reference": reference,
        "project_title": title,
        "tables": records.keys().collect::<Vec<_>>(),
    });

    let cursor = Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(cursor);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    zip.start_file("manifest.json", opts)
        .map_err(AppError::internal)?;
    zip.write_all(
        serde_json::to_string_pretty(&manifest)
            .map_err(AppError::internal)?
            .as_bytes(),
    )
    .map_err(AppError::internal)?;
    for (table, rows) in &records {
        zip.start_file(format!("records/{table}.json"), opts)
            .map_err(AppError::internal)?;
        let body = serde_json::to_string_pretty(&rows).map_err(AppError::internal)?;
        zip.write_all(body.as_bytes()).map_err(AppError::internal)?;
    }
    for sha in &file_shas {
        let bytes = crate::files::read_file(data_dir, sha).await?;
        zip.start_file(format!("files/{sha}"), opts)
            .map_err(AppError::internal)?;
        zip.write_all(&bytes).map_err(AppError::internal)?;
    }
    let cursor = zip.finish().map_err(AppError::internal)?;

    Ok(ExportResult {
        bytes: cursor.into_inner(),
        project_reference: reference,
        project_title: title,
        project_id: project_id.to_string(),
    })
}

/// CLI `export-project <reference>`.
pub async fn export_by_reference(
    pool: &SqlitePool,
    data_dir: &Path,
    reference: &str,
) -> AppResult<ExportResult> {
    let project_id: String = sqlx::query_scalar("SELECT id FROM projects WHERE reference = ?")
        .bind(reference)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AppError::BadRequest(format!("no project with reference {reference}")))?;
    export_project(pool, data_dir, &project_id).await
}

/// CLI `import-project <file.zip>`: parse + validate + commit in one go (the
/// operator runs it deliberately, so there is no separate preview step).
pub async fn import_bytes(
    pool: &SqlitePool,
    data_dir: &Path,
    bytes: &[u8],
    max_upload_bytes: u64,
) -> AppResult<ImportOutcome> {
    let parsed = parse_archive(bytes, max_upload_bytes)?;
    commit_archive(pool, data_dir, &parsed, None, "CLI import-project", None).await
}

// ---------------------------------------------------------------------------
// Import: parse + validate
// ---------------------------------------------------------------------------

pub struct ParsedArchive {
    pub manifest: Map<String, Value>,
    pub records: BTreeMap<String, Vec<Map<String, Value>>>,
    /// sha256 -> bytes, already verified.
    pub files: BTreeMap<String, Vec<u8>>,
}

/// Parse and fully validate an archive ZIP (§8): rejects path traversal,
/// oversized entries/totals, unknown schema_version, and verifies every
/// file's sha256. Used by preview AND re-run at commit.
pub fn parse_archive(bytes: &[u8], max_upload_bytes: u64) -> AppResult<ParsedArchive> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|e| AppError::unprocessable("bad_archive", format!("invalid ZIP: {e}")))?;

    let mut total: u64 = 0;
    let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
    for i in 0..zip.len() {
        let mut file = zip
            .by_index(i)
            .map_err(|e| AppError::unprocessable("bad_archive", format!("ZIP entry {i}: {e}")))?;
        // enclosed_name() rejects `../` and absolute paths.
        let name = match file.enclosed_name() {
            Some(p) => p.to_string_lossy().to_string(),
            None => {
                return Err(AppError::unprocessable(
                    "unsafe_archive",
                    "archive entry has an unsafe path",
                ));
            }
        };
        if file.size() > max_upload_bytes {
            return Err(AppError::unprocessable(
                "entry_too_large",
                format!("archive entry {name} exceeds the per-file limit"),
            ));
        }
        total += file.size();
        if total > MAX_UNCOMPRESSED {
            return Err(AppError::unprocessable(
                "archive_too_large",
                "archive exceeds the 10 GiB uncompressed limit",
            ));
        }
        if file.is_dir() {
            continue;
        }
        // Declared sizes can lie (zip bombs): bound the bytes actually read.
        let mut buf = Vec::with_capacity(file.size().min(64 << 20) as usize);
        (&mut file)
            .take(max_upload_bytes + 1)
            .read_to_end(&mut buf)
            .map_err(|e| AppError::unprocessable("bad_archive", format!("read {name}: {e}")))?;
        if buf.len() as u64 > max_upload_bytes {
            return Err(AppError::unprocessable(
                "entry_too_large",
                format!("archive entry {name} exceeds the per-file limit"),
            ));
        }
        total = total - file.size() + buf.len() as u64;
        if total > MAX_UNCOMPRESSED {
            return Err(AppError::unprocessable(
                "archive_too_large",
                "archive exceeds the 10 GiB uncompressed limit",
            ));
        }
        entries.push((name, buf));
    }

    let mut manifest_bytes: Option<Vec<u8>> = None;
    let mut records: BTreeMap<String, Vec<Map<String, Value>>> = BTreeMap::new();
    let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    for (name, buf) in entries {
        if name == "manifest.json" {
            manifest_bytes = Some(buf);
        } else if let Some(table) = name
            .strip_prefix("records/")
            .and_then(|t| t.strip_suffix(".json"))
        {
            if table != "users" && !IMPORTABLE_TABLES.contains(&table) {
                return Err(AppError::unprocessable(
                    "unsafe_archive",
                    format!("unexpected entry {name}"),
                ));
            }
            let rows: Vec<Map<String, Value>> = serde_json::from_slice(&buf).map_err(|e| {
                AppError::unprocessable("bad_archive", format!("records/{table}.json: {e}"))
            })?;
            records.insert(table.to_string(), rows);
        } else if let Some(sha) = name.strip_prefix("files/") {
            if sha.len() != 64
                || !sha
                    .chars()
                    .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
            {
                return Err(AppError::unprocessable(
                    "unsafe_archive",
                    format!("unexpected file entry {name}"),
                ));
            }
            let actual = crate::util::sha256_hex(&buf);
            if actual != sha {
                return Err(AppError::unprocessable(
                    "checksum_mismatch",
                    format!("file {sha} fails sha256 verification"),
                ));
            }
            files.insert(sha.to_string(), buf);
        } else {
            return Err(AppError::unprocessable(
                "unsafe_archive",
                format!("unexpected archive entry {name}"),
            ));
        }
    }

    let manifest_bytes = manifest_bytes
        .ok_or_else(|| AppError::unprocessable("bad_archive", "manifest.json missing"))?;
    let manifest: Map<String, Value> = serde_json::from_slice(&manifest_bytes)
        .map_err(|e| AppError::unprocessable("bad_archive", format!("manifest.json: {e}")))?;
    if cell(&manifest, "format").as_str() != Some(FORMAT) {
        return Err(AppError::unprocessable(
            "bad_archive",
            "not a pitcairn project export",
        ));
    }
    let schema_version = cell(&manifest, "schema_version");
    if schema_version.as_i64() != Some(SCHEMA_VERSION) {
        return Err(AppError::unprocessable(
            "unknown_schema_version",
            format!("unsupported schema_version {schema_version}"),
        ));
    }
    if records.get("projects").map(|r| r.len()) != Some(1) {
        return Err(AppError::unprocessable(
            "bad_archive",
            "archive must contain exactly one project",
        ));
    }
    validate_records(&records, &files)?;

    Ok(ParsedArchive {
        manifest,
        records,
        files,
    })
}

/// Where a reference column must point: the archive's own project, or a row
/// of another project-scoped table carried in the same archive.
enum Parent {
    Project,
    Table(&'static str),
}

/// Every reference a project-scoped row holds to another project-scoped row.
/// Each must resolve inside the archive — never to a row of a project that
/// already exists on this install (§8).
const PROJECT_REFS: &[(&str, &str, Parent)] = &[
    ("project_sites", "project_id", Parent::Project),
    ("project_members", "project_id", Parent::Project),
    ("project_revisions", "project_id", Parent::Project),
    ("invitations", "project_id", Parent::Project),
    ("documents", "project_id", Parent::Project),
    (
        "document_versions",
        "document_id",
        Parent::Table("documents"),
    ),
    ("document_versions", "file_id", Parent::Table("files")),
    ("threads", "project_id", Parent::Project),
    ("messages", "thread_id", Parent::Table("threads")),
    ("action_items", "project_id", Parent::Project),
    ("action_items", "thread_id", Parent::Table("threads")),
    ("review_assignments", "project_id", Parent::Project),
    (
        "review_assignments",
        "project_revision_id",
        Parent::Table("project_revisions"),
    ),
    ("change_requests", "project_id", Parent::Project),
    (
        "change_requests",
        "resulting_decision_id",
        Parent::Table("decisions"),
    ),
    ("decisions", "project_id", Parent::Project),
    (
        "decisions",
        "project_revision_id",
        Parent::Table("project_revisions"),
    ),
    (
        "decisions",
        "document_version_id",
        Parent::Table("document_versions"),
    ),
    ("decisions", "supersedes_id", Parent::Table("decisions")),
    ("decisions", "superseded_by_id", Parent::Table("decisions")),
    (
        "decisions",
        "change_request_id",
        Parent::Table("change_requests"),
    ),
    ("trips", "project_id", Parent::Project),
    ("bookings", "trip_id", Parent::Table("trips")),
    ("invoices", "project_id", Parent::Project),
    ("invoice_lines", "invoice_id", Parent::Table("invoices")),
    ("invoice_lines", "booking_id", Parent::Table("bookings")),
    ("payments", "invoice_id", Parent::Table("invoices")),
    ("deliverables", "project_id", Parent::Project),
    (
        "deliverable_due_changes",
        "deliverable_id",
        Parent::Table("deliverables"),
    ),
    (
        "deliverable_submissions",
        "deliverable_id",
        Parent::Table("deliverables"),
    ),
    (
        "submission_files",
        "submission_id",
        Parent::Table("deliverable_submissions"),
    ),
    (
        "submission_files",
        "document_version_id",
        Parent::Table("document_versions"),
    ),
    (
        "external_links",
        "submission_id",
        Parent::Table("deliverable_submissions"),
    ),
    (
        "publication_files",
        "deliverable_id",
        Parent::Table("deliverables"),
    ),
    (
        "publication_files",
        "document_version_id",
        Parent::Table("document_versions"),
    ),
    ("samples", "project_id", Parent::Project),
    ("samples", "site_id", Parent::Table("project_sites")),
    ("measurements", "project_id", Parent::Project),
    (
        "measurements",
        "deliverable_id",
        Parent::Table("deliverables"),
    ),
    (
        "measurements",
        "submission_id",
        Parent::Table("deliverable_submissions"),
    ),
    ("notifications", "project_id", Parent::Project),
    ("audit_events", "project_id", Parent::Project),
];

/// A row cell, `Null` when absent (archive rows are untrusted JSON objects).
fn cell<'a>(row: &'a Map<String, Value>, key: &str) -> &'a Value {
    row.get(key).unwrap_or(&Value::Null)
}

fn str_cell<'a>(row: &'a Map<String, Value>, key: &str) -> &'a str {
    cell(row, key).as_str().unwrap_or_default()
}

/// Structural validation of the archive records (§8), before anything is
/// written: every row of the project graph belongs to the imported project
/// or to a row of the same archive, and every file row names verified bytes
/// carried in the archive with a matching size.
fn validate_records(
    records: &BTreeMap<String, Vec<Map<String, Value>>>,
    files: &BTreeMap<String, Vec<u8>>,
) -> AppResult<()> {
    let unsafe_archive = |msg: String| AppError::unprocessable("unsafe_archive", msg);
    let rows_of = |table: &str| records.get(table).map(Vec::as_slice).unwrap_or_default();

    let project_id = str_cell(&records["projects"][0], "id");
    if project_id.is_empty() {
        return Err(unsafe_archive("project row without id".into()));
    }

    // Row ids per project-scoped table (plus files), all required.
    let mut ids: HashMap<&str, HashSet<&str>> = HashMap::new();
    for table in project_tables().chain(["files"]) {
        let set = ids.entry(table).or_default();
        for row in rows_of(table) {
            let id = str_cell(row, "id");
            if id.is_empty() {
                return Err(unsafe_archive(format!("{table} row without id")));
            }
            set.insert(id);
        }
    }

    for (table, column, parent) in PROJECT_REFS {
        for row in rows_of(table) {
            let value = cell(row, column);
            let ok = match parent {
                Parent::Project => value.as_str() == Some(project_id),
                Parent::Table(t) => {
                    value.is_null() || value.as_str().is_some_and(|v| ids[t].contains(v))
                }
            };
            if !ok {
                return Err(unsafe_archive(format!(
                    "{table}.{column} references a row outside the imported project"
                )));
            }
        }
    }

    for row in rows_of("files") {
        let sha = str_cell(row, "sha256");
        if !crate::files::is_sha256_hex(sha) {
            return Err(unsafe_archive(format!(
                "file {} has an invalid sha256",
                str_cell(row, "id")
            )));
        }
        let Some(bytes) = files.get(sha) else {
            return Err(AppError::unprocessable(
                "missing_file",
                format!("file {sha} is listed without its bytes"),
            ));
        };
        if cell(row, "size").as_i64() != Some(bytes.len() as i64) {
            return Err(AppError::unprocessable(
                "checksum_mismatch",
                format!("file {sha} size does not match its bytes"),
            ));
        }
    }

    // User stubs are remapped by value: a stub id equal to a project row id
    // would rewrite validated references.
    for stub in rows_of("users") {
        let id = str_cell(stub, "id");
        if id.is_empty() || str_cell(stub, "email").is_empty() {
            return Err(unsafe_archive("user stub without id or email".into()));
        }
        if id == project_id || ids.values().any(|set| set.contains(id)) {
            return Err(unsafe_archive(format!(
                "user stub {id} collides with a project row id"
            )));
        }
    }
    Ok(())
}

/// What preview (and the DTO) reports about an archive.
pub struct ArchivePreview {
    pub project_id: String,
    pub project_reference: Option<String>,
    pub project_title: String,
    pub record_counts: BTreeMap<String, i64>,
    pub file_count: i64,
    pub conflicts: Vec<String>,
    pub matched_users: Vec<String>,
    pub new_users: Vec<String>,
}

pub async fn preview_archive(
    pool: &SqlitePool,
    parsed: &ParsedArchive,
) -> AppResult<ArchivePreview> {
    let project = &parsed.records["projects"][0];
    let project_id = str_cell(project, "id").to_string();
    let reference = cell(project, "reference").as_str().map(String::from);
    let title = str_cell(project, "title").to_string();

    let mut conflicts = Vec::new();
    let by_id: Option<String> = sqlx::query_scalar("SELECT reference FROM projects WHERE id = ?")
        .bind(&project_id)
        .fetch_optional(pool)
        .await?;
    if by_id.is_some() {
        conflicts.push(format!(
            "project id {} already exists (import is idempotent — it will be skipped)",
            project_id
        ));
    }
    if let Some(r) = &reference {
        let owner: Option<String> =
            sqlx::query_scalar("SELECT id FROM projects WHERE reference = ?")
                .bind(r)
                .fetch_optional(pool)
                .await?;
        if let Some(owner) = owner
            && owner != project_id
        {
            conflicts.push(format!("reference {r} is already used by another project"));
        }
    }

    let mut matched = Vec::new();
    let mut new = Vec::new();
    if let Some(users) = parsed.records.get("users") {
        for stub in users {
            let email = str_cell(stub, "email");
            let exists: Option<String> = sqlx::query_scalar("SELECT id FROM users WHERE email = ?")
                .bind(email)
                .fetch_optional(pool)
                .await?;
            if exists.is_some() {
                matched.push(email.to_string());
            } else {
                new.push(email.to_string());
            }
        }
    }

    Ok(ArchivePreview {
        project_id,
        project_reference: reference,
        project_title: title,
        record_counts: parsed
            .records
            .iter()
            .map(|(t, r)| (t.clone(), r.len() as i64))
            .collect(),
        file_count: parsed.files.len() as i64,
        conflicts,
        matched_users: matched,
        new_users: new,
    })
}

// ---------------------------------------------------------------------------
// Import: commit
// ---------------------------------------------------------------------------

pub struct ImportOutcome {
    pub project_id: String,
    pub already_existed: bool,
    pub created_tables: BTreeMap<String, i64>,
    pub new_users: Vec<String>,
    pub messages: Vec<String>,
}

/// File bytes land in content-addressed storage before the DB transaction.
async fn store_file_bytes(data_dir: &Path, sha256: &str, bytes: &[u8]) -> AppResult<PathBuf> {
    let target = crate::files::file_path(data_dir, sha256)?;
    if let Some(parent) = target.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    if !target.exists() {
        let tmp = target.with_extension("tmp");
        tokio::fs::write(&tmp, bytes).await?;
        tokio::fs::rename(&tmp, &target).await?;
    }
    Ok(target)
}

/// Insert one row into `table`, remapping user/file/template/resource ids and
/// deferring circular-FK columns. Returns deferred cells `(column, value)`.
async fn insert_row(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    table: &str,
    row: &Map<String, Value>,
    maps: &Remaps,
    deferred_out: &mut Vec<(String, String, Value)>,
) -> AppResult<()> {
    // Table and column names come from the archive: only known tables and
    // their real columns may reach the SQL text.
    if !IMPORTABLE_TABLES.contains(&table) {
        return Err(AppError::unprocessable(
            "bad_archive",
            format!("unknown table {table}"),
        ));
    }
    let known: HashSet<String> = sqlx::query_scalar("SELECT name FROM pragma_table_info(?)")
        .bind(table)
        .fetch_all(&mut **tx)
        .await?
        .into_iter()
        .collect();
    if let Some(bad) = row.keys().find(|k| !known.contains(k.as_str())) {
        return Err(AppError::unprocessable(
            "bad_archive",
            format!("unknown column {table}.{bad}"),
        ));
    }
    let mut cols = Vec::new();
    let mut vals = Vec::new();
    for (k, v) in row {
        if deferred(table, k) {
            if !v.is_null() {
                let row_id = row["id"].as_str().unwrap_or_default().to_string();
                deferred_out.push((k.clone(), row_id, v.clone()));
            }
            continue;
        }
        let v = remap_value(table, k, v, maps);
        cols.push(k.clone());
        vals.push(v);
    }
    let sql = format!(
        "INSERT INTO {table} ({}) VALUES ({})",
        cols.join(", "),
        cols.iter().map(|_| "?").collect::<Vec<_>>().join(", ")
    );
    let mut q = sqlx::query(&sql);
    for v in &vals {
        q = bind_value(q, v);
    }
    q.execute(&mut **tx).await?;
    Ok(())
}

struct Remaps {
    users: HashMap<String, String>,
    template_versions: HashMap<String, String>,
    files: HashMap<String, String>,
    resources: HashMap<String, String>,
}

fn remap_value(table: &str, column: &str, v: &Value, maps: &Remaps) -> Value {
    let Some(s) = v.as_str() else {
        return v.clone();
    };
    if let Some(m) = maps.users.get(s) {
        return Value::from(m.clone());
    }
    if column == "template_version_id"
        && let Some(m) = maps.template_versions.get(s)
    {
        return Value::from(m.clone());
    }
    if column == "file_id"
        && let Some(m) = maps.files.get(s)
    {
        return Value::from(m.clone());
    }
    if column == "resource_id"
        && let Some(m) = maps.resources.get(s)
    {
        return Value::from(m.clone());
    }
    // trips.participants_json embeds user ids inside a JSON array.
    if column == "participants_json"
        && let Ok(arr) = serde_json::from_str::<Value>(s)
        && let Some(items) = arr.as_array()
    {
        let remapped: Vec<Value> = items
            .iter()
            .map(|it| {
                it.as_str()
                    .and_then(|s| maps.users.get(s))
                    .map(|m| Value::from(m.clone()))
                    .unwrap_or_else(|| it.clone())
            })
            .collect();
        return Value::from(Value::Array(remapped).to_string());
    }
    let _ = table;
    v.clone()
}

async fn table_exists_row(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    table: &str,
    id: &str,
) -> AppResult<bool> {
    let n: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table} WHERE id = ?"))
        .bind(id)
        .fetch_one(&mut **tx)
        .await?;
    Ok(n > 0)
}

/// Commit a parsed archive: create user stubs, dependency rows and all project
/// rows, then restore deferred FK columns. Idempotent on project id.
pub async fn commit_archive(
    pool: &SqlitePool,
    data_dir: &Path,
    parsed: &ParsedArchive,
    actor_id: Option<&str>,
    actor_label: &str,
    batch_id: Option<&str>,
) -> AppResult<ImportOutcome> {
    let project = &parsed.records["projects"][0];
    let project_id = str_cell(project, "id").to_string();
    let reference = cell(project, "reference").as_str().map(String::from);

    // Write file bytes first (content-addressed; stray bytes are harmless).
    for (sha, bytes) in &parsed.files {
        store_file_bytes(data_dir, sha, bytes).await?;
    }

    let mut outcome = ImportOutcome {
        project_id: project_id.clone(),
        already_existed: false,
        created_tables: BTreeMap::new(),
        new_users: Vec::new(),
        messages: Vec::new(),
    };

    let mut tx = db::begin_immediate(pool).await?;

    if let Some(batch_id) = batch_id {
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
    }
    let exists: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM projects WHERE id = ?")
        .bind(&project_id)
        .fetch_one(&mut *tx)
        .await?;
    if exists > 0 {
        outcome.already_existed = true;
        outcome
            .messages
            .push("project already exists; import skipped (idempotent)".into());
        tx.commit().await?;
        return Ok(outcome);
    }
    if let Some(r) = &reference {
        let owner: Option<String> =
            sqlx::query_scalar("SELECT id FROM projects WHERE reference = ?")
                .bind(r)
                .fetch_optional(&mut *tx)
                .await?;
        if owner.is_some() {
            return Err(AppError::conflict(
                "reference_conflict",
                format!("reference {r} is already used by another project"),
            ));
        }
    }

    let now = now_rfc3339();
    let mut maps = Remaps {
        users: HashMap::new(),
        template_versions: HashMap::new(),
        files: HashMap::new(),
        resources: HashMap::new(),
    };
    let mut deferred_cells: Vec<(String, String, Value)> = Vec::new(); // (column, row_id, value)

    // users stubs — matched by email, created disabled, never roles/secrets.
    if let Some(users) = parsed.records.get("users") {
        for stub in users {
            let old_id = str_cell(stub, "id").to_string();
            let email = str_cell(stub, "email");
            let name = str_cell(stub, "name");
            let org = str_cell(stub, "organisation");
            let existing: Option<(String, String)> =
                sqlx::query_as("SELECT id, email FROM users WHERE id = ? OR email = ?")
                    .bind(&old_id)
                    .bind(email)
                    .fetch_optional(&mut *tx)
                    .await?;
            let new_id_final = match existing {
                Some((id, _)) => id,
                None => {
                    sqlx::query(
                        "INSERT INTO users (id, email, name, organisation, password_hash, disabled_at, created_at)
                         VALUES (?, ?, ?, ?, '!import-stub', ?, ?)",
                    )
                    .bind(&old_id)
                    .bind(email)
                    .bind(name)
                    .bind(org)
                    .bind(&now)
                    .bind(&now)
                    .execute(&mut *tx)
                    .await?;
                    outcome.new_users.push(email.to_string());
                    old_id.clone()
                }
            };
            maps.users.insert(old_id, new_id_final);
        }
    }

    // templates + template_versions (matched by key/version).
    if let Some(templates) = parsed.records.get("templates") {
        for t in templates {
            let old_id = str_cell(t, "id");
            let key = str_cell(t, "key");
            let existing: Option<String> =
                sqlx::query_scalar("SELECT id FROM templates WHERE id = ? OR key = ?")
                    .bind(old_id)
                    .bind(key)
                    .fetch_optional(&mut *tx)
                    .await?;
            if existing.is_none() {
                let mut deferred = Vec::new();
                insert_row(&mut tx, "templates", t, &maps, &mut deferred).await?;
            }
        }
    }
    if let Some(versions) = parsed.records.get("template_versions") {
        for tv in versions {
            let old_id = str_cell(tv, "id").to_string();
            if let Some(m) =
                sqlx::query_scalar::<_, String>("SELECT id FROM template_versions WHERE id = ?")
                    .bind(&old_id)
                    .fetch_optional(&mut *tx)
                    .await?
            {
                maps.template_versions.insert(old_id, m);
                continue;
            }
            // Same template key + version already present under another id?
            let template_key = parsed
                .records
                .get("templates")
                .and_then(|ts| {
                    ts.iter()
                        .find(|t| cell(t, "id").as_str() == cell(tv, "template_id").as_str())
                })
                .and_then(|t| cell(t, "key").as_str().map(String::from));
            let existing_tv: Option<String> = if let Some(key) = &template_key {
                sqlx::query_scalar(
                    "SELECT tv.id FROM template_versions tv JOIN templates t ON t.id = tv.template_id
                     WHERE t.key = ? AND tv.version = ?",
                )
                .bind(key)
                .bind(cell(tv, "version").as_i64().unwrap_or(0))
                .fetch_optional(&mut *tx)
                .await?
            } else {
                None
            };
            if let Some(existing) = existing_tv {
                maps.template_versions.insert(old_id, existing);
            } else {
                // Remap the row's template_id to the (possibly existing) local
                // template matched by key.
                let mut row = tv.clone();
                if let Some(key) = &template_key {
                    let local_tid: Option<String> =
                        sqlx::query_scalar("SELECT id FROM templates WHERE key = ?")
                            .bind(key)
                            .fetch_optional(&mut *tx)
                            .await?;
                    if let Some(tid) = local_tid {
                        row.insert("template_id".into(), Value::from(tid));
                    }
                }
                let mut deferred = Vec::new();
                insert_row(&mut tx, "template_versions", &row, &maps, &mut deferred).await?;
                maps.template_versions.insert(old_id.clone(), old_id);
            }
        }
    }

    // resources + tariffs (same id reused if free).
    if let Some(resources) = parsed.records.get("resources") {
        for r in resources {
            let old_id = str_cell(r, "id").to_string();
            if table_exists_row(&mut tx, "resources", &old_id).await? {
                maps.resources.insert(old_id.clone(), old_id);
                continue;
            }
            let mut deferred = Vec::new();
            insert_row(&mut tx, "resources", r, &maps, &mut deferred).await?;
            maps.resources.insert(old_id.clone(), old_id);
        }
    }
    if let Some(tariffs) = parsed.records.get("tariffs") {
        for t in tariffs {
            if let Some(id) = cell(t, "id").as_str()
                && table_exists_row(&mut tx, "tariffs", id).await?
            {
                continue;
            }
            let mut deferred = Vec::new();
            insert_row(&mut tx, "tariffs", t, &maps, &mut deferred).await?;
        }
    }

    // files metadata — dedup by sha256 (the id may differ across installs).
    // `parse_archive` verified every sha256 shape, size and byte payload.
    if let Some(files) = parsed.records.get("files") {
        for f in files {
            let old_id = str_cell(f, "id").to_string();
            let sha = str_cell(f, "sha256");
            let existing: Option<String> =
                sqlx::query_scalar("SELECT id FROM files WHERE sha256 = ?")
                    .bind(sha)
                    .fetch_optional(&mut *tx)
                    .await?;
            if let Some(id) = existing {
                maps.files.insert(old_id, id);
                continue;
            }
            let bytes = parsed.files.get(sha).ok_or_else(|| {
                AppError::unprocessable("missing_file", format!("file {sha} has no bytes"))
            })?;
            let mut row = f.clone();
            row.insert(
                "storage_key".into(),
                Value::from(
                    crate::files::file_path(data_dir, sha)?
                        .to_string_lossy()
                        .to_string(),
                ),
            );
            // Never trust the archive's scan verdict: scan the verified bytes.
            let detail = crate::jobs::scan_bytes(bytes, str_cell(f, "mime"));
            row.insert(
                "scan_status".into(),
                Value::from(if detail.is_none() {
                    "clean"
                } else {
                    "rejected"
                }),
            );
            row.insert(
                "scan_detail".into(),
                detail.map(Value::from).unwrap_or(Value::Null),
            );
            // If a different file somehow already owns this id, mint a new one.
            if table_exists_row(&mut tx, "files", &old_id).await? {
                let nid = new_id();
                row.insert("id".into(), Value::from(nid.clone()));
                maps.files.insert(old_id, nid);
            } else {
                maps.files.insert(old_id.clone(), old_id);
            }
            let mut deferred = Vec::new();
            insert_row(&mut tx, "files", &row, &maps, &mut deferred).await?;
        }
    }

    // All project-scoped tables, in FK-safe order.
    for table in project_tables() {
        if let Some(rows) = parsed.records.get(table) {
            let mut count = 0i64;
            for row in rows {
                insert_row(&mut tx, table, row, &maps, &mut deferred_cells).await?;
                count += 1;
            }
            outcome.created_tables.insert(table.to_string(), count);
        }
    }

    // Deferred circular-FK columns.
    for (table, column) in DEFERRED_COLUMNS {
        for (col, row_id, value) in &deferred_cells {
            if col == column {
                sqlx::query(&format!("UPDATE {table} SET {col} = ? WHERE id = ?"))
                    .bind(value.as_str().unwrap_or_default())
                    .bind(row_id)
                    .execute(&mut *tx)
                    .await?;
            }
        }
    }

    // Keep reference counters ahead of the imported reference so a future
    // submit in the same year can never collide with the imported number.
    if let Some(r) = &reference
        && let Some((prefix, rest)) = r.split_once('-')
        && let Some((year_s, n_s)) = rest.rsplit_once('-')
        && let (Ok(year), Ok(n)) = (year_s.parse::<i64>(), n_s.parse::<i64>())
    {
        sqlx::query(
            "INSERT INTO reference_counters (prefix, year, next) VALUES (?, ?, ?)
             ON CONFLICT(prefix, year) DO UPDATE SET next = MAX(next, excluded.next)",
        )
        .bind(prefix)
        .bind(year)
        .bind(n + 1)
        .execute(&mut *tx)
        .await?;
    }

    audit::record(
        &mut tx,
        AuditEvent {
            actor_id: actor_id.map(String::from),
            actor_label: actor_label.to_string(),
            action: "project.imported".into(),
            entity_type: "project".into(),
            entity_id: project_id.clone(),
            project_id: Some(project_id.clone()),
            visibility: "internal".into(),
            summary: format!(
                "Project {} imported from archive",
                reference.clone().unwrap_or_else(|| project_id.clone())
            ),
            before: None,
            after: Some(json!({"reference": reference})),
            reason: None,
        },
    )
    .await?;

    tx.commit().await?;
    Ok(outcome)
}
