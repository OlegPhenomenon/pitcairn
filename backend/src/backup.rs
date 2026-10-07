//! Backup / restore (§8).
//!
//! `backup --out DIR`: `VACUUM INTO DIR/pitcairn.sqlite3` (a consistent
//! snapshot even while the server runs), then copy `files/` (immutable and
//! content-addressed, so copying after the snapshot is consistent), then
//! write `DIR/backup.json` with row counts and sha256 of the db and of every
//! file.
//!
//! `restore --from DIR`: verifies every hash first, refuses a non-empty data
//! dir unless `force`, copies db + files, re-points `files.storage_key` at the
//! new data dir and checks the row counts.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::error::{AppError, AppResult};

pub const FORMAT: &str = "pitcairn-backup";
pub const DB_FILE: &str = "pitcairn.sqlite3";
pub const MANIFEST: &str = "backup.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BackupManifest {
    pub format: String,
    pub created_at: String,
    pub db_sha256: String,
    /// table -> row count (of the snapshot).
    pub row_counts: BTreeMap<String, i64>,
    /// relative path under `files/` (`aa/<sha256>`) -> sha256 of its bytes.
    pub files: BTreeMap<String, String>,
}

/// Row counts for every ordinary table (FTS shadow tables excluded — they
/// are derived from `projects`).
pub async fn row_counts(pool: &SqlitePool) -> AppResult<BTreeMap<String, i64>> {
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master
         WHERE type = 'table' AND name NOT LIKE 'sqlite_%' AND name NOT LIKE 'projects_fts%'
         ORDER BY name",
    )
    .fetch_all(pool)
    .await?;
    let mut out = BTreeMap::new();
    for table in tables {
        // Names come from sqlite_master, quoted defensively.
        let n: i64 = sqlx::query_scalar(&format!(
            "SELECT COUNT(*) FROM \"{}\"",
            table.replace('"', "\"\"")
        ))
        .fetch_one(pool)
        .await?;
        out.insert(table, n);
    }
    Ok(out)
}

/// Every regular file under `files/` as (relative path, absolute path).
fn list_files(files_dir: &Path) -> AppResult<Vec<(String, PathBuf)>> {
    let mut out = Vec::new();
    if !files_dir.exists() {
        return Ok(out);
    }
    for shard in std::fs::read_dir(files_dir)? {
        let shard = shard?;
        if !shard.file_type()?.is_dir() {
            continue;
        }
        for entry in std::fs::read_dir(shard.path())? {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if name.ends_with(".tmp") {
                continue; // interrupted write, never referenced
            }
            let rel = format!("{}/{}", shard.file_name().to_string_lossy(), name);
            out.push((rel, entry.path()));
        }
    }
    out.sort();
    Ok(out)
}

fn dir_is_empty(dir: &Path) -> AppResult<bool> {
    if !dir.exists() {
        return Ok(true);
    }
    Ok(std::fs::read_dir(dir)?.next().is_none())
}

/// Create a backup of `pool`'s database and `data_dir/files` into `out`.
pub async fn backup(pool: &SqlitePool, data_dir: &Path, out: &Path) -> AppResult<BackupManifest> {
    std::fs::create_dir_all(out)?;
    if out.join(DB_FILE).exists() || out.join(MANIFEST).exists() {
        return Err(AppError::conflict(
            "backup_exists",
            format!("{} already contains a backup", out.display()),
        ));
    }
    let db_out = out.join(DB_FILE);
    let db_out_str = db_out.to_string_lossy().to_string();
    sqlx::query(&format!("VACUUM INTO '{}'", db_out_str.replace('\'', "''")))
        .execute(pool)
        .await?;

    // Count rows of the snapshot itself (not the live db).
    let snapshot = crate::db::connect(&db_out).await?;
    let counts = row_counts(&snapshot).await?;
    snapshot.close().await;
    // The snapshot connection may have left WAL side files; fold them back.
    for suffix in ["-wal", "-shm"] {
        let side = PathBuf::from(format!("{db_out_str}{suffix}"));
        if side.exists() {
            std::fs::remove_file(side)?;
        }
    }
    let db_sha256 = crate::files::hash_file(&db_out).await?;

    let mut files = BTreeMap::new();
    for (rel, src) in list_files(&data_dir.join("files"))? {
        let dst = out.join("files").join(&rel);
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent)?;
        }
        tokio::fs::copy(&src, &dst).await?;
        files.insert(rel, crate::files::hash_file(&dst).await?);
    }

    let manifest = BackupManifest {
        format: FORMAT.into(),
        created_at: crate::util::now_rfc3339(),
        db_sha256,
        row_counts: counts,
        files,
    };
    std::fs::write(
        out.join(MANIFEST),
        serde_json::to_vec_pretty(&manifest).map_err(AppError::internal)?,
    )?;
    Ok(manifest)
}

/// Verify a backup directory: manifest format, db hash, every file hash (and
/// that each file is stored under its own sha256 name).
pub async fn verify(from: &Path) -> AppResult<BackupManifest> {
    let raw = std::fs::read(from.join(MANIFEST)).map_err(|e| {
        AppError::unprocessable(
            "bad_backup",
            format!("cannot read {}: {e}", from.join(MANIFEST).display()),
        )
    })?;
    let manifest: BackupManifest = serde_json::from_slice(&raw)
        .map_err(|e| AppError::unprocessable("bad_backup", format!("backup.json: {e}")))?;
    if manifest.format != FORMAT {
        return Err(AppError::unprocessable(
            "bad_backup",
            "not a pitcairn backup",
        ));
    }
    let db_sha = crate::files::hash_file(&from.join(DB_FILE)).await?;
    if db_sha != manifest.db_sha256 {
        return Err(AppError::unprocessable(
            "checksum_mismatch",
            "database snapshot fails sha256 verification",
        ));
    }
    for (rel, sha) in &manifest.files {
        let (shard, name) = rel
            .split_once('/')
            .ok_or_else(|| AppError::unprocessable("bad_backup", format!("bad path {rel}")))?;
        let well_formed = name.len() == 64
            && name.chars().all(|c| c.is_ascii_hexdigit())
            && shard == &name[0..2]
            && name == sha;
        if !well_formed {
            return Err(AppError::unprocessable(
                "bad_backup",
                format!("unexpected file entry {rel}"),
            ));
        }
        let actual = crate::files::hash_file(&from.join("files").join(rel)).await?;
        if &actual != sha {
            return Err(AppError::unprocessable(
                "checksum_mismatch",
                format!("file {rel} fails sha256 verification"),
            ));
        }
    }
    Ok(manifest)
}

/// Restore a verified backup into `data_dir`. Refuses a data dir that already
/// holds a database or stored files unless `force`.
pub async fn restore(from: &Path, data_dir: &Path, force: bool) -> AppResult<BackupManifest> {
    let manifest = verify(from).await?;

    let db_path = data_dir.join(DB_FILE);
    let files_dir = data_dir.join("files");
    let occupied = db_path.exists() || !dir_is_empty(&files_dir)?;
    if occupied && !force {
        return Err(AppError::conflict(
            "data_dir_not_empty",
            format!(
                "{} is not empty; restore refuses to overwrite it (use --force)",
                data_dir.display()
            ),
        ));
    }
    if occupied {
        for suffix in ["", "-wal", "-shm"] {
            let p = PathBuf::from(format!("{}{suffix}", db_path.display()));
            if p.exists() {
                std::fs::remove_file(p)?;
            }
        }
        if files_dir.exists() {
            std::fs::remove_dir_all(&files_dir)?;
        }
    }
    std::fs::create_dir_all(&files_dir)?;
    std::fs::create_dir_all(data_dir.join("uploads"))?;
    std::fs::create_dir_all(data_dir.join("backups"))?;

    let tmp_db = data_dir.join(format!("{DB_FILE}.restore-tmp"));
    tokio::fs::copy(from.join(DB_FILE), &tmp_db).await?;
    std::fs::rename(&tmp_db, &db_path)?;
    for rel in manifest.files.keys() {
        let dst = files_dir.join(rel);
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent)?;
        }
        tokio::fs::copy(from.join("files").join(rel), &dst).await?;
    }

    let pool = crate::db::connect(&db_path).await?;
    let counts = row_counts(&pool).await?;
    let mismatch: Vec<String> = manifest
        .row_counts
        .iter()
        .filter(|(t, n)| counts.get(*t) != Some(*n))
        .map(|(t, _)| t.clone())
        .collect();
    if !mismatch.is_empty() {
        pool.close().await;
        return Err(AppError::internal(format!(
            "restored row counts differ for: {}",
            mismatch.join(", ")
        )));
    }
    // storage_key is an absolute path on the source install; re-point it.
    let shas: Vec<String> = sqlx::query_scalar("SELECT sha256 FROM files")
        .fetch_all(&pool)
        .await?;
    let mut tx = pool.begin().await?;
    for sha in shas {
        sqlx::query("UPDATE files SET storage_key = ? WHERE sha256 = ?")
            .bind(
                crate::files::file_path(data_dir, &sha)?
                    .to_string_lossy()
                    .to_string(),
            )
            .bind(&sha)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    // Newer binaries may carry migrations the backup predates.
    crate::db::migrate(&pool).await?;
    pool.close().await;
    Ok(manifest)
}
