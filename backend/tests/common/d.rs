//! Slice D test helpers: app with a custom AI mode, empty installs, ZIP
//! builders and seed lookups.
#![allow(dead_code)]

use std::io::Write;
use std::path::Path;

use super::{TestApp, spawn_app_with};

/// Like `spawn_app(true)` but with `PITCAIRN_AI_MODE` = `ai_mode`.
pub async fn spawn_app_with_ai(ai_mode: &str) -> TestApp {
    spawn_app_with(true, |config| config.ai_mode = ai_mode.into()).await
}

/// A brand-new install: migrated, nothing seeded.
pub async fn empty_install(data_dir: &Path) -> sqlx::SqlitePool {
    std::fs::create_dir_all(data_dir.join("files")).unwrap();
    let pool = pitcairn::db::connect(&data_dir.join("pitcairn.sqlite3"))
        .await
        .expect("connect fresh db");
    pitcairn::db::migrate(&pool)
        .await
        .expect("migrate fresh db");
    pool
}

pub async fn project_id(pool: &sqlx::SqlitePool, title: &str) -> String {
    sqlx::query_scalar("SELECT id FROM projects WHERE title = ?")
        .bind(title)
        .fetch_one(pool)
        .await
        .unwrap_or_else(|e| panic!("project {title}: {e}"))
}

/// Build a ZIP from raw (name, bytes) entries — names are written verbatim.
pub fn zip_with(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default();
    for (name, bytes) in entries {
        zip.start_file(*name, opts).expect("start zip entry");
        zip.write_all(bytes).expect("write zip entry");
    }
    zip.finish().expect("finish zip").into_inner()
}

/// Read every entry of a ZIP into memory.
pub fn unzip(bytes: &[u8]) -> std::collections::BTreeMap<String, Vec<u8>> {
    use std::io::Read;
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("open zip");
    let mut out = std::collections::BTreeMap::new();
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).unwrap();
        let mut buf = Vec::new();
        f.read_to_end(&mut buf).unwrap();
        out.insert(f.name().to_string(), buf);
    }
    out
}

/// sha256 of every file under `<data_dir>/files`, keyed by relative path.
pub fn file_hashes(data_dir: &Path) -> std::collections::BTreeMap<String, String> {
    use sha2::Digest;
    let mut out = std::collections::BTreeMap::new();
    let root = data_dir.join("files");
    if !root.exists() {
        return out;
    }
    for shard in std::fs::read_dir(&root).unwrap() {
        let shard = shard.unwrap();
        if !shard.file_type().unwrap().is_dir() {
            continue;
        }
        for f in std::fs::read_dir(shard.path()).unwrap() {
            let f = f.unwrap();
            let bytes = std::fs::read(f.path()).unwrap();
            out.insert(
                format!(
                    "{}/{}",
                    shard.file_name().to_string_lossy(),
                    f.file_name().to_string_lossy()
                ),
                hex::encode(sha2::Sha256::digest(&bytes)),
            );
        }
    }
    out
}
