//! Slice D: project export → import into a fresh install gives identical
//! rows and file bytes; unsafe archives are rejected (§8, §12.5).

mod common;

use std::collections::BTreeMap;

use common::d::{empty_install, project_id, unzip, zip_with};
use common::{persona, spawn_app};
use pitcairn::dto::{ArchiveImportPreviewResponse, ImportCommitResponse};
use serde_json::Value;

const PROJECT: &str = "Humpback whale acoustic monitoring";

/// Normalized per-table JSON of an export ZIP: rows sorted by id, the
/// export/import audit markers dropped.
fn normalized(zip: &BTreeMap<String, Vec<u8>>) -> BTreeMap<String, Value> {
    let mut out = BTreeMap::new();
    for (name, bytes) in zip {
        let Some(table) = name
            .strip_prefix("records/")
            .and_then(|t| t.strip_suffix(".json"))
        else {
            continue;
        };
        let mut rows: Vec<Value> = serde_json::from_slice(bytes).unwrap();
        rows.retain(|r| {
            !matches!(
                r["action"].as_str(),
                Some("project.exported" | "project.imported")
            )
        });
        rows.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
        out.insert(table.to_string(), Value::Array(rows));
    }
    out
}

#[tokio::test]
async fn export_then_import_into_fresh_install_is_identical() {
    let app = spawn_app(true).await;
    let pid = project_id(&app.pool, PROJECT).await;
    let maria = persona(&app, "maria").await;
    let resp = maria.get(&format!("/api/v1/projects/{pid}/export")).await;
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.headers()["content-type"], "application/zip");
    assert!(
        resp.headers()["content-disposition"]
            .to_str()
            .unwrap()
            .contains("PIT-2023-0001.zip")
    );
    let bytes = resp.bytes().await.unwrap().to_vec();
    let source = unzip(&bytes);
    let manifest: Value = serde_json::from_slice(&source["manifest.json"]).unwrap();
    assert_eq!(manifest["format"], "pitcairn-project-export");
    assert_eq!(manifest["schema_version"], 1);
    for table in [
        "projects",
        "project_revisions",
        "decisions",
        "deliverables",
        "deliverable_submissions",
        "audit_events",
        "project_members",
        "users",
        "template_versions",
        "files",
    ] {
        assert!(
            source.contains_key(&format!("records/{table}.json")),
            "missing {table}"
        );
    }
    // Users are stubs only: never password hashes, TOTP secrets or roles.
    let users: Vec<Value> = serde_json::from_slice(&source["records/users.json"]).unwrap();
    assert!(!users.is_empty());
    for u in &users {
        let keys: Vec<&String> = u.as_object().unwrap().keys().collect();
        assert_eq!(keys, vec!["email", "id", "name", "organisation"]);
    }
    let file_entries: Vec<&String> = source.keys().filter(|k| k.starts_with("files/")).collect();
    assert!(
        !file_entries.is_empty(),
        "export contains the published report bytes"
    );

    // Import into a brand-new install (CLI code path).
    let fresh_dir = tempfile::TempDir::new().unwrap();
    let fresh = empty_install(fresh_dir.path()).await;
    let outcome = pitcairn::archive::import_bytes(&fresh, fresh_dir.path(), &bytes, 2_147_483_648)
        .await
        .expect("import into fresh install");
    assert!(!outcome.already_existed);
    assert_eq!(outcome.new_users.len(), users.len());

    // Re-export from the fresh install and compare every table + file bytes.
    let again = pitcairn::archive::export_project(&fresh, fresh_dir.path(), &pid)
        .await
        .unwrap();
    let target = unzip(&again.bytes);
    assert_eq!(normalized(&source), normalized(&target));
    for name in &file_entries {
        assert_eq!(source[*name], target[*name], "{name} bytes differ");
    }
    // Stubs are disabled and carry no credentials or roles.
    let (disabled, roles): (i64, i64) = sqlx::query_as(
        "SELECT (SELECT COUNT(*) FROM users WHERE disabled_at IS NULL),
                (SELECT COUNT(*) FROM user_roles)",
    )
    .fetch_one(&fresh)
    .await
    .unwrap();
    assert_eq!((disabled, roles), (0, 0));
    // FTS index follows the imported project.
    let hits: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM projects_fts WHERE projects_fts MATCH '\"humpback\"*'",
    )
    .fetch_one(&fresh)
    .await
    .unwrap();
    assert_eq!(hits, 1);

    // Idempotent on project id: a second import changes nothing.
    let count_before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM audit_events")
        .fetch_one(&fresh)
        .await
        .unwrap();
    let second = pitcairn::archive::import_bytes(&fresh, fresh_dir.path(), &bytes, 2_147_483_648)
        .await
        .unwrap();
    assert!(second.already_existed);
    let count_after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM audit_events")
        .fetch_one(&fresh)
        .await
        .unwrap();
    assert_eq!(count_before, count_after);
}

#[tokio::test]
async fn http_archive_preview_and_commit_are_idempotent_on_project_id() {
    let app = spawn_app(true).await;
    let pid = project_id(&app.pool, PROJECT).await;
    let admin = persona(&app, "admin").await;
    let bytes = admin
        .get(&format!("/api/v1/projects/{pid}/export"))
        .await
        .bytes()
        .await
        .unwrap()
        .to_vec();
    let resp = admin
        .request(
            reqwest::Method::POST,
            "/api/v1/admin/import/project-archive",
        )
        .header("content-type", "application/zip")
        .body(bytes)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let preview: ArchiveImportPreviewResponse = admin.json(resp).await;
    assert_eq!(preview.project_reference.as_deref(), Some("PIT-2023-0001"));
    assert!(
        preview
            .conflicts
            .iter()
            .any(|c| c.contains("already exists"))
    );
    assert!(preview.new_users.is_empty(), "all users matched by email");
    let resp = admin
        .post(&format!("/api/v1/admin/import/{}/commit", preview.batch.id))
        .await;
    assert_eq!(resp.status(), 200);
    let commit: ImportCommitResponse = admin.json(resp).await;
    assert_eq!((commit.created, commit.skipped), (0, 1));
    let projects: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM projects WHERE title = ?")
        .bind(PROJECT)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(projects, 1);
}

#[tokio::test]
async fn unsafe_archives_are_rejected() {
    let app = spawn_app(true).await;
    let admin = persona(&app, "admin").await;
    let manifest: &[u8] = br#"{"format":"pitcairn-project-export","schema_version":1}"#;
    let post = |bytes: Vec<u8>| {
        admin
            .request(
                reqwest::Method::POST,
                "/api/v1/admin/import/project-archive",
            )
            .header("content-type", "application/zip")
            .body(bytes)
            .send()
    };

    // Path traversal entry.
    let evil = zip_with(&[("manifest.json", manifest), ("../evil", b"pwned")]);
    let resp = post(evil.clone()).await.unwrap();
    assert_eq!(resp.status(), 422);
    let body: Value = admin.json(resp).await;
    assert_eq!(body["error"]["code"], "unsafe_archive");
    assert!(matches!(
        pitcairn::archive::parse_archive(&evil, 1 << 30),
        Err(pitcairn::error::AppError::Unprocessable { .. })
    ));
    assert!(
        !app.state
            .config
            .data_dir
            .parent()
            .unwrap()
            .join("evil")
            .exists()
    );

    // Absolute path entry.
    let abs = zip_with(&[("manifest.json", manifest), ("/etc/evil", b"x")]);
    assert_eq!(post(abs).await.unwrap().status(), 422);

    // Unknown schema_version.
    let future = zip_with(&[(
        "manifest.json",
        br#"{"format":"pitcairn-project-export","schema_version":99}"#,
    )]);
    let resp = post(future).await.unwrap();
    let body: Value = admin.json(resp).await;
    assert_eq!(body["error"]["code"], "unknown_schema_version");

    // File whose bytes do not match its sha256 name.
    let fake_sha = "0".repeat(64);
    let name = format!("files/{fake_sha}");
    let tampered = zip_with(&[("manifest.json", manifest), (&name, b"not matching")]);
    let resp = post(tampered).await.unwrap();
    let body: Value = admin.json(resp).await;
    assert_eq!(body["error"]["code"], "checksum_mismatch");

    // Entry larger than the per-file limit.
    let big = zip_with(&[
        ("manifest.json", manifest),
        ("records/projects.json", &[b' '; 4096]),
    ]);
    assert!(matches!(
        pitcairn::archive::parse_archive(&big, 1024),
        Err(pitcairn::error::AppError::Unprocessable { .. })
    ));

    // Unknown record table never reaches SQL.
    let sneaky = zip_with(&[
        ("manifest.json", manifest),
        ("records/sessions.json", b"[]"),
    ]);
    let resp = post(sneaky).await.unwrap();
    assert_eq!(resp.status(), 422);
}

#[tokio::test]
async fn export_is_coordinator_or_admin_only() {
    let app = spawn_app(true).await;
    let pid = project_id(&app.pool, PROJECT).await;
    for key in ["lukas", "james", "ruth"] {
        let c = persona(&app, key).await;
        assert_eq!(
            c.get(&format!("/api/v1/projects/{pid}/export"))
                .await
                .status(),
            403,
            "{key}"
        );
    }
    let maria = persona(&app, "maria").await;
    assert_eq!(
        maria.get("/api/v1/projects/nope/export").await.status(),
        404
    );
    // Archive import itself is admin-only.
    let resp = maria
        .request(
            reqwest::Method::POST,
            "/api/v1/admin/import/project-archive",
        )
        .body(vec![1u8, 2, 3])
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 403);
}
