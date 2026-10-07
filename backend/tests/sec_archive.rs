//! Security regressions for project-archive import (§8) and public
//! downloads (§5): file metadata must name verified bytes inside the archive,
//! every imported row must belong to the imported project, and the public
//! download only serves clean `result` documents of the deliverable's own
//! project.

mod common;

use std::collections::BTreeMap;

use common::c::{seeded_project, sha256, user_id};
use common::d::zip_with;
use common::{Client, TestApp, persona, spawn_app};
use serde_json::{Value, json};

const NOW: &str = "2026-01-01T00:00:00Z";

struct Ids {
    template_version: String,
    admin: String,
}

async fn ids(app: &TestApp) -> Ids {
    let template_version: String =
        sqlx::query_scalar("SELECT id FROM template_versions ORDER BY created_at LIMIT 1")
            .fetch_one(&app.pool)
            .await
            .unwrap();
    Ids {
        template_version,
        admin: user_id(app, "admin@demo.pitcairn.invalid").await,
    }
}

/// Records of a minimal archive: one new legacy project `pid`.
fn project_records(ids: &Ids, pid: &str) -> BTreeMap<String, Vec<Value>> {
    let mut records = BTreeMap::new();
    records.insert(
        "projects".to_string(),
        vec![json!({
            "id": pid,
            "reference": null,
            "title": format!("Crafted archive {pid}"),
            "template_version_id": ids.template_version,
            "status": "closed",
            "legacy": 1,
            "created_by": ids.admin,
            "created_at": NOW,
        })],
    );
    records
}

/// A result document + version + published deliverable on `pid` whose
/// version points at `file_id`.
fn publish_file(
    records: &mut BTreeMap<String, Vec<Value>>,
    ids: &Ids,
    pid: &str,
    file_id: &str,
    version_id: &str,
) {
    let doc_id = format!("{pid}-doc");
    let deliverable_id = format!("{pid}-del");
    records.entry("documents".into()).or_default().push(json!({
        "id": doc_id, "project_id": pid, "title": "Report", "category": "result",
        "created_by": ids.admin, "created_at": NOW,
    }));
    records
        .entry("document_versions".into())
        .or_default()
        .push(json!({
            "id": version_id, "document_id": doc_id, "number": 1, "file_id": file_id,
            "uploaded_by": ids.admin, "uploaded_at": NOW, "created_at": NOW,
        }));
    records
        .entry("deliverables".into())
        .or_default()
        .push(json!({
            "id": deliverable_id, "project_id": pid, "title": "Final report", "kind": "report",
            "due_date": "2025-01-01", "sender_id": ids.admin, "recipient_id": ids.admin,
            "status": "accepted", "publish_level": "metadata_and_files", "created_by": ids.admin,
            "created_at": NOW,
        }));
    records
        .entry("publication_files".into())
        .or_default()
        .push(json!({
            "id": format!("{pid}-pub"), "deliverable_id": deliverable_id,
            "document_version_id": version_id, "approved_by": ids.admin,
            "approved_at": NOW, "created_at": NOW,
        }));
}

fn file_row(ids: &Ids, id: &str, sha: &str, size: i64) -> Value {
    json!({
        "id": id, "sha256": sha, "size": size, "mime": "text/plain",
        "storage_key": format!("files/{sha}"), "scan_status": "clean",
        "uploaded_by": ids.admin, "created_at": NOW,
    })
}

fn build_zip(records: &BTreeMap<String, Vec<Value>>, files: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut entries: Vec<(String, Vec<u8>)> = vec![(
        "manifest.json".into(),
        br#"{"format":"pitcairn-project-export","schema_version":1}"#.to_vec(),
    )];
    for (table, rows) in records {
        entries.push((
            format!("records/{table}.json"),
            serde_json::to_vec(rows).unwrap(),
        ));
    }
    for (sha, bytes) in files {
        entries.push((format!("files/{sha}"), bytes.clone()));
    }
    let refs: Vec<(&str, &[u8])> = entries
        .iter()
        .map(|(n, b)| (n.as_str(), b.as_slice()))
        .collect();
    zip_with(&refs)
}

/// Preview, then (if the preview passed) commit. Returns the first failing
/// status + body, or the commit's.
async fn import(admin: &Client, bytes: Vec<u8>) -> (u16, Value) {
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
    let status = resp.status().as_u16();
    let body: Value = admin.json(resp).await;
    if status != 200 {
        return (status, body);
    }
    let batch = body["batch"]["id"].as_str().unwrap().to_string();
    let resp = admin
        .post(&format!("/api/v1/admin/import/{batch}/commit"))
        .await;
    let status = resp.status().as_u16();
    (status, admin.json(resp).await)
}

#[tokio::test]
async fn archive_file_rows_must_name_verified_bytes() {
    let app = spawn_app(true).await;
    let admin = persona(&app, "admin").await;
    let ids = ids(&app).await;
    let anon = Client::anonymous(&app);

    // Absolute path as "sha256", no bytes, claimed clean and published.
    let mut records = project_records(&ids, "sec-path");
    records.insert(
        "files".into(),
        vec![file_row(&ids, "sec-path-file", "/etc/passwd", 1)],
    );
    publish_file(
        &mut records,
        &ids,
        "sec-path",
        "sec-path-file",
        "sec-path-v1",
    );
    let (status, body) = import(&admin, build_zip(&records, &[])).await;
    assert_eq!(status, 422, "{body}");
    assert_eq!(body["error"]["code"], "unsafe_archive");
    let resp = anon.get("/api/v1/public/files/sec-path-v1/download").await;
    assert_eq!(resp.status(), 404);
    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM files WHERE sha256 = '/etc/passwd'")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(rows, 0);

    // Short hash: rejected, never a panic.
    let mut records = project_records(&ids, "sec-short");
    records.insert(
        "files".into(),
        vec![file_row(&ids, "sec-short-file", "a", 1)],
    );
    publish_file(
        &mut records,
        &ids,
        "sec-short",
        "sec-short-file",
        "sec-short-v1",
    );
    let (status, body) = import(&admin, build_zip(&records, &[])).await;
    assert_eq!(status, 422, "{body}");

    // Well-formed hash but the bytes are not in the archive.
    let missing = sha256(b"bytes that are not in the archive");
    let mut records = project_records(&ids, "sec-missing");
    records.insert(
        "files".into(),
        vec![file_row(&ids, "sec-missing-file", &missing, 33)],
    );
    publish_file(
        &mut records,
        &ids,
        "sec-missing",
        "sec-missing-file",
        "sec-missing-v1",
    );
    let (status, body) = import(&admin, build_zip(&records, &[])).await;
    assert_eq!(status, 422, "{body}");
    assert_eq!(body["error"]["code"], "missing_file");

    // Bytes present, but the declared size lies.
    let bytes = b"real bytes".to_vec();
    let sha = sha256(&bytes);
    let mut records = project_records(&ids, "sec-size");
    records.insert(
        "files".into(),
        vec![file_row(&ids, "sec-size-file", &sha, 999)],
    );
    publish_file(
        &mut records,
        &ids,
        "sec-size",
        "sec-size-file",
        "sec-size-v1",
    );
    let (status, body) = import(&admin, build_zip(&records, &[(sha, bytes)])).await;
    assert_eq!(status, 422, "{body}");

    // The archive's scan verdict is never trusted: an executable claimed
    // `clean` is scanned on import and ends up `rejected`.
    let exe = b"MZ\x90\x00 not really a text file".to_vec();
    let sha = sha256(&exe);
    let mut records = project_records(&ids, "sec-exe");
    records.insert(
        "files".into(),
        vec![file_row(&ids, "sec-exe-file", &sha, exe.len() as i64)],
    );
    publish_file(&mut records, &ids, "sec-exe", "sec-exe-file", "sec-exe-v1");
    let (status, body) = import(&admin, build_zip(&records, &[(sha.clone(), exe)])).await;
    assert_eq!(status, 200, "{body}");
    let scan: String = sqlx::query_scalar("SELECT scan_status FROM files WHERE sha256 = ?")
        .bind(&sha)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(scan, "rejected");
    let resp = anon.get("/api/v1/public/files/sec-exe-v1/download").await;
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn archive_rows_must_stay_inside_the_imported_project() {
    let app = spawn_app(true).await;
    let admin = persona(&app, "admin").await;
    let ids = ids(&app).await;
    let victim = seeded_project(&app).await;
    let lukas = user_id(&app, "lukas@demo.pitcairn.invalid").await;

    // Membership row pointing at another, existing project.
    let mut records = project_records(&ids, "sec-member");
    records.insert(
        "project_members".into(),
        vec![json!({
            "id": "sec-member-m1", "project_id": victim, "user_id": lukas,
            "role": "lead", "added_at": NOW,
        })],
    );
    let (status, body) = import(&admin, build_zip(&records, &[])).await;
    assert_eq!(status, 422, "{body}");
    assert_eq!(body["error"]["code"], "unsafe_archive");
    let joined: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM project_members WHERE project_id = ? AND user_id = ?",
    )
    .bind(&victim)
    .bind(&lukas)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(joined, 0, "archive must not add members to another project");

    // Publishing another project's personal document version.
    let personal: String = sqlx::query_scalar(
        "SELECT dv.id FROM document_versions dv JOIN documents d ON d.id = dv.document_id
         WHERE d.category = 'personal' LIMIT 1",
    )
    .fetch_one(&app.pool)
    .await
    .unwrap();
    let mut records = project_records(&ids, "sec-pub");
    records.insert(
        "deliverables".into(),
        vec![json!({
            "id": "sec-pub-del", "project_id": "sec-pub", "title": "Leak", "kind": "report",
            "due_date": "2025-01-01", "sender_id": ids.admin, "recipient_id": ids.admin,
            "status": "accepted", "publish_level": "metadata_and_files",
            "created_by": ids.admin, "created_at": NOW,
        })],
    );
    records.insert(
        "publication_files".into(),
        vec![json!({
            "id": "sec-pub-pf", "deliverable_id": "sec-pub-del",
            "document_version_id": personal, "approved_by": ids.admin,
            "approved_at": NOW, "created_at": NOW,
        })],
    );
    let (status, body) = import(&admin, build_zip(&records, &[])).await;
    assert_eq!(status, 422, "{body}");
    let resp = Client::anonymous(&app)
        .get(&format!("/api/v1/public/files/{personal}/download"))
        .await;
    assert_eq!(resp.status(), 404);

    // A version reusing an existing file id that the archive does not carry.
    let foreign_file: String = sqlx::query_scalar("SELECT id FROM files LIMIT 1")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    let mut records = project_records(&ids, "sec-file");
    publish_file(&mut records, &ids, "sec-file", &foreign_file, "sec-file-v1");
    let (status, body) = import(&admin, build_zip(&records, &[])).await;
    assert_eq!(status, 422, "{body}");

    for pid in ["sec-member", "sec-pub", "sec-file"] {
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM projects WHERE id = ?")
            .bind(pid)
            .fetch_one(&app.pool)
            .await
            .unwrap();
        assert_eq!(n, 0, "{pid} must not be imported");
    }
}

/// Insert a document + version on `project_id` pointing at `file_id`, listed
/// in `publication_files` of `deliverable_id`. Returns the version id.
async fn publish_directly(
    app: &TestApp,
    key: &str,
    project_id: &str,
    deliverable_id: &str,
    category: &str,
    file_id: &str,
    by: &str,
) -> String {
    let doc = format!("{key}-doc");
    let version = format!("{key}-v1");
    sqlx::query(
        "INSERT INTO documents (id, project_id, title, category, created_by, created_at)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(&doc)
    .bind(project_id)
    .bind(format!("{key} document"))
    .bind(category)
    .bind(by)
    .bind(NOW)
    .execute(&app.pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO document_versions (id, document_id, number, file_id, uploaded_by, uploaded_at, created_at)
         VALUES (?, ?, 1, ?, ?, ?, ?)",
    )
    .bind(&version)
    .bind(&doc)
    .bind(file_id)
    .bind(by)
    .bind(NOW)
    .bind(NOW)
    .execute(&app.pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO publication_files (id, deliverable_id, document_version_id, approved_by, approved_at, created_at)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(format!("{key}-pf"))
    .bind(deliverable_id)
    .bind(&version)
    .bind(by)
    .bind(NOW)
    .bind(NOW)
    .execute(&app.pool)
    .await
    .unwrap();
    version
}

#[tokio::test]
async fn public_download_serves_only_clean_results_of_the_same_project() {
    let app = spawn_app(true).await;
    let anon = Client::anonymous(&app);
    let today = pitcairn::deliverables::today();
    // A file that is publicly downloadable in the seed.
    let (deliverable, project, version, file_id): (String, String, String, String) =
        sqlx::query_as(
            "SELECT del.id, del.project_id, pf.document_version_id, dv.file_id
             FROM publication_files pf
             JOIN deliverables del ON del.id = pf.deliverable_id
             JOIN document_versions dv ON dv.id = pf.document_version_id
             JOIN projects p ON p.id = del.project_id
             WHERE del.publish_level = 'metadata_and_files'
               AND (del.embargo_until IS NULL OR del.embargo_until <= ?)
               AND p.status != 'withdrawn'
             LIMIT 1",
        )
        .bind(&today)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    let resp = anon
        .get(&format!("/api/v1/public/files/{version}/download"))
        .await;
    assert_eq!(resp.status(), 200, "baseline published file downloads");
    let by = user_id(&app, "maria@demo.pitcairn.invalid").await;

    // A personal document of the same project listed for publication.
    let personal = publish_directly(
        &app,
        "sec-personal",
        &project,
        &deliverable,
        "personal",
        &file_id,
        &by,
    )
    .await;
    // A result document of ANOTHER project listed under this deliverable.
    let other: String = sqlx::query_scalar("SELECT id FROM projects WHERE id != ? LIMIT 1")
        .bind(&project)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    let foreign = publish_directly(
        &app,
        "sec-foreign",
        &other,
        &deliverable,
        "result",
        &file_id,
        &by,
    )
    .await;
    // A result document whose file has not passed the scan.
    let pending_bytes = b"pending scan bytes".to_vec();
    let pending_sha = sha256(&pending_bytes);
    let path = app
        .state
        .config
        .data_dir
        .join("files")
        .join(&pending_sha[0..2])
        .join(&pending_sha);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, &pending_bytes).unwrap();
    sqlx::query(
        "INSERT INTO files (id, sha256, size, mime, storage_key, scan_status, uploaded_by, created_at)
         VALUES ('sec-pending-file', ?, ?, 'text/plain', ?, 'pending', ?, ?)",
    )
    .bind(&pending_sha)
    .bind(pending_bytes.len() as i64)
    .bind(path.to_string_lossy().to_string())
    .bind(&by)
    .bind(NOW)
    .execute(&app.pool)
    .await
    .unwrap();
    let pending = publish_directly(
        &app,
        "sec-pending",
        &project,
        &deliverable,
        "result",
        "sec-pending-file",
        &by,
    )
    .await;

    for (label, v) in [
        ("personal", &personal),
        ("other project", &foreign),
        ("pending scan", &pending),
    ] {
        let resp = anon
            .get(&format!("/api/v1/public/files/{v}/download"))
            .await;
        assert_eq!(resp.status(), 404, "{label} must not be public");
    }
    // Nor are they listed in the public catalog.
    let reference: String = sqlx::query_scalar("SELECT reference FROM projects WHERE id = ?")
        .bind(&project)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    let body = anon
        .get(&format!("/api/v1/public/projects/{reference}"))
        .await
        .text()
        .await
        .unwrap();
    for v in [&personal, &foreign, &pending] {
        assert!(!body.contains(v.as_str()), "{v} listed publicly");
    }
}
