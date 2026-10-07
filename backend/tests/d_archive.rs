//! Slice D: project export → import into a fresh install gives identical
//! rows and file bytes; unsafe archives are rejected (§8, §12.5).

mod common;

use std::collections::BTreeMap;

use common::d::{empty_install, project_id, unzip, zip_with};
use common::{Client, TestApp, a, b, c, persona, spawn_app};
use pitcairn::dto::{ArchiveImportPreviewResponse, ImportCommitResponse};
use serde_json::{Value, json};

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

/// `GET` a list endpoint as `client`, asserting 200; returns `items`.
async fn items(client: &Client, path: &str) -> Vec<Value> {
    let (status, body) = a::get(client, path).await;
    assert_eq!(status, 200, "GET {path}: {body}");
    body["items"].as_array().expect("items").clone()
}

async fn download(client: &Client, version_id: &str) -> Vec<u8> {
    let resp = client
        .get(&format!("/api/v1/document-versions/{version_id}/download"))
        .await;
    assert_eq!(resp.status(), 200, "download {version_id}");
    resp.bytes().await.unwrap().to_vec()
}

fn by_id<'a>(rows: &'a [Value], id: &str) -> &'a Value {
    rows.iter()
        .find(|r| r["id"] == id)
        .unwrap_or_else(|| panic!("row {id} missing"))
}

/// What staff see of a project over HTTP, compared between both installs.
struct StaffView {
    project: Value,
    revisions: Vec<Value>,
    decisions: Vec<Value>,
    reviews: Vec<Value>,
    invoices: Vec<Value>,
    deliverables: Vec<Value>,
    timeline: Vec<Value>,
}

async fn staff_view(app: &TestApp, pid: &str) -> StaffView {
    let maria = persona(app, "maria").await;
    let ruth = persona(app, "ruth").await;
    let (status, ws) = a::get(&maria, &format!("/projects/{pid}")).await;
    assert_eq!(status, 200, "workspace: {ws}");
    StaffView {
        project: ws["project"].clone(),
        revisions: items(&maria, &format!("/projects/{pid}/revisions")).await,
        decisions: items(&maria, &format!("/projects/{pid}/decisions")).await,
        reviews: items(&maria, &format!("/projects/{pid}/reviews")).await,
        invoices: items(&ruth, &format!("/projects/{pid}/invoices")).await,
        deliverables: items(&maria, &format!("/projects/{pid}/deliverables")).await,
        timeline: items(&maria, &format!("/projects/{pid}/timeline?limit=200")).await,
    }
}

/// §6 acceptance: a study built from scratch over HTTP is exported and
/// restored on another install with its documents, versions, review,
/// decisions, money, results and history.
#[tokio::test]
async fn new_project_built_over_http_round_trips_into_a_second_install() {
    let app = spawn_app(true).await;
    let maria = persona(&app, "maria").await;
    let helen = persona(&app, "helen").await;
    let james = persona(&app, "james").await;
    let sam = persona(&app, "sam").await;
    let ruth = persona(&app, "ruth").await;

    // 1. A brand-new researcher applies; the coordinator screens it.
    let lead = a::register(&app, "ingrid@fjord.invalid", "Ingrid Fjell").await;
    let pid = a::in_review_project(&lead, &maria, "Kelp forest recovery survey").await;

    // 2. A document with two versions (different bytes).
    let (doc_id, v1) = a::upload_document(&lead, &app, &pid, None, "other", "Dive plan").await;
    let v1_bytes = format!("{pid}:Dive plan").into_bytes();
    let v2_bytes = b"Dive plan, second edition: buddy pairs fixed".to_vec();
    let file2 = a::upload_clean_file(&lead, &app, &v2_bytes).await;
    let (status, doc) = a::post(
        &lead,
        &format!("/documents/{doc_id}/versions"),
        json!({"file_id": file2, "note": "v2"}),
    )
    .await;
    assert_eq!(status, 201, "add version: {doc}");
    let v2 = doc["id"].as_str().unwrap().to_string();
    assert_ne!(v1, v2);

    // 3. An expert review, accepted and submitted.
    let (status, review) = a::post(
        &maria,
        &format!("/projects/{pid}/reviews"),
        json!({"expert_id": a::persona_id(&app, "james").await, "due_date": "2026-10-30"}),
    )
    .await;
    assert_eq!(status, 201, "assign review: {review}");
    let review_id = review["id"].as_str().unwrap().to_string();
    let (status, _) = a::post(&james, &format!("/reviews/{review_id}/accept"), json!({})).await;
    assert_eq!(status, 200);
    let (status, body) = a::post(
        &james,
        &format!("/reviews/{review_id}/submit"),
        json!({"opinion": "Sound methods.", "recommendation": "approve_with_conditions"}),
    )
    .await;
    assert_eq!(status, 200, "submit review: {body}");

    // 4. Permit A, independent permit B, and an extension of A.
    let revision = a::latest_revision_id(&app, &pid).await;
    let permit_a = a::issue_decision(
        &helen,
        &pid,
        json!({"kind": "permit", "title": "Kelp transect survey", "project_revision_id": revision,
               "basis": "Meets the base-use policy", "legal_reference": "MSB Research Policy (demo) s.4",
               "valid_from": "2026-11-01", "valid_to": "2026-11-30",
               "permitted_activities": ["Diving surveys"], "conditions": ["No anchoring on kelp"]}),
    )
    .await;
    let a_id = permit_a["id"].as_str().unwrap().to_string();
    let permit_b = a::issue_decision(
        &helen,
        &pid,
        json!({"kind": "permit", "title": "Coral tissue sampling", "project_revision_id": revision,
               "basis": "Separate activity", "valid_from": "2026-11-05", "valid_to": "2026-11-20",
               "permitted_activities": ["Collect 10 coral fragments"],
               "conditions": ["One fragment per colony"]}),
    )
    .await;
    let b_id = permit_b["id"].as_str().unwrap().to_string();
    let extension = a::issue_decision(
        &helen,
        &pid,
        json!({"kind": "extension", "project_revision_id": revision, "basis": "Season extended",
               "valid_from": "2026-11-01", "valid_to": "2027-01-31", "supersedes_id": a_id}),
    )
    .await;
    let ext_id = extension["id"].as_str().unwrap().to_string();
    assert_eq!(extension["chain_id"], a_id.as_str());

    // 5. A trip with confirmed bookings; one invoiced, partially paid and
    // verified. The other stays uninvoiced for the restored install.
    let room = b::resource_id(&app, b::ROOM).await;
    let lab = b::resource_id(&app, b::LAB).await;
    let trip = b::create_trip(&lead, &pid, "2027-03-01", "2027-03-10").await;
    let room_booking =
        b::request_booking(&lead, &trip.id, &room, "2027-03-01", "2027-03-04", 2).await;
    let lab_booking =
        b::request_booking(&lead, &trip.id, &lab, "2027-03-02", "2027-03-03", 1).await;
    assert_eq!(b::confirm(&sam, &room_booking.id).await.status(), 200);
    assert_eq!(b::confirm(&sam, &lab_booking.id).await.status(), 200);
    let invoice = b::create_invoice(&ruth, &pid, &[&room_booking.id]).await;
    let invoice = b::issue(&ruth, &invoice.id).await;
    let (status, payment) = a::post(
        &ruth,
        &format!("/invoices/{}/payments", invoice.id),
        json!({"amount_cents": 20_000, "method": "manual"}),
    )
    .await;
    assert_eq!(status, 201, "payment: {payment}");
    let payment_id = payment["id"].as_str().unwrap().to_string();
    let (status, _) = a::post(&ruth, &format!("/payments/{payment_id}/verify"), json!({})).await;
    assert_eq!(status, 200);

    // 6. An accepted deliverable with a result file.
    let lead_id = a::user_id(&app, "ingrid@fjord.invalid").await;
    let maria_id = a::persona_id(&app, "maria").await;
    let deliverable =
        c::agreed_deliverable(&lead, &maria, &pid, "dataset", &lead_id, &maria_id).await;
    let result_bytes = b"site,kelp_cover\nA,0.42\n".to_vec();
    let result_file = c::upload_clean_file(&lead, &app, &result_bytes, "text/csv").await;
    let result_version =
        c::create_document(&lead, &pid, "Kelp cover", "result", &result_file).await;
    let submission = c::submit(
        &lead,
        &deliverable.id,
        json!({"document_version_ids": [result_version], "note": "all done"}),
    )
    .await;
    let resp = maria
        .post(&format!("/api/v1/submissions/{}/accept", submission.id))
        .await;
    assert_eq!(resp.status(), 200);

    let source = staff_view(&app, &pid).await;
    assert_eq!(source.project["status"], "approved");

    // Export on A, import into install B over HTTP.
    let resp = maria.get(&format!("/api/v1/projects/{pid}/export")).await;
    assert_eq!(resp.status(), 200);
    let archive = resp.bytes().await.unwrap().to_vec();

    let other = spawn_app(true).await;
    let admin = persona(&other, "admin").await;
    let resp = admin
        .request(
            reqwest::Method::POST,
            "/api/v1/admin/import/project-archive",
        )
        .header("content-type", "application/zip")
        .body(archive)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let preview: ArchiveImportPreviewResponse = admin.json(resp).await;
    assert!(preview.conflicts.is_empty(), "{:?}", preview.conflicts);
    assert_eq!(preview.new_users, vec!["ingrid@fjord.invalid".to_string()]);
    let resp = admin
        .post(&format!("/api/v1/admin/import/{}/commit", preview.batch.id))
        .await;
    assert_eq!(resp.status(), 200);
    let commit: ImportCommitResponse = admin.json(resp).await;
    assert!(
        commit.created > 0 && commit.skipped == 0,
        "nothing imported"
    );

    let restored = staff_view(&other, &pid).await;
    let maria_b = persona(&other, "maria").await;

    // Project and its revisions.
    for key in ["reference", "title", "status"] {
        assert_eq!(restored.project[key], source.project[key], "project.{key}");
    }
    assert!(source.project["reference"].is_string());
    let numbers = |rows: &[Value]| rows.iter().map(|r| r["number"].clone()).collect::<Vec<_>>();
    assert!(!source.revisions.is_empty());
    assert_eq!(numbers(&restored.revisions), numbers(&source.revisions));

    // Both document versions, byte for byte.
    assert_eq!(download(&maria_b, &v1).await, v1_bytes);
    assert_eq!(download(&maria_b, &v2).await, v2_bytes);

    // Decisions: three issued, titles and chains intact.
    assert_eq!(restored.decisions.len(), 3);
    for d in &source.decisions {
        let r = by_id(&restored.decisions, d["id"].as_str().unwrap());
        for key in [
            "kind",
            "title",
            "status",
            "chain_id",
            "supersedes_id",
            "superseded_by_id",
            "valid_from",
            "valid_to",
            "conditions",
            "issued_at",
        ] {
            assert_eq!(r[key], d[key], "decision {}.{key}", d["id"]);
        }
    }
    let a_row = by_id(&restored.decisions, &a_id);
    let b_row = by_id(&restored.decisions, &b_id);
    let ext_row = by_id(&restored.decisions, &ext_id);
    assert_eq!(a_row["title"], "Kelp transect survey");
    assert_eq!(b_row["title"], "Coral tissue sampling");
    assert_eq!(a_row["superseded_by_id"], ext_id.as_str());
    assert_eq!(ext_row["supersedes_id"], a_id.as_str());
    assert_eq!(ext_row["chain_id"], a_id.as_str());
    assert_eq!(b_row["chain_id"], b_id.as_str());
    assert!(b_row["superseded_by_id"].is_null());
    assert!(restored.decisions.iter().all(|d| d["status"] == "issued"));

    // The expert review, attributed to B's matching expert account.
    assert_eq!(restored.reviews.len(), 1);
    let r = by_id(&restored.reviews, &review_id);
    assert_eq!(r["status"], "submitted");
    assert_eq!(r["recommendation"], "approve_with_conditions");
    assert_eq!(r["opinion"], "Sound methods.");
    assert_eq!(
        r["expert_id"],
        a::persona_id(&other, "james").await.as_str()
    );

    // Invoice, lines and the verified partial payment.
    assert_eq!(restored.invoices.len(), 1);
    let (src_inv, inv) = (&source.invoices[0], &restored.invoices[0]);
    for key in [
        "id",
        "number",
        "status",
        "total_cents",
        "net_verified_cents",
        "settlement",
    ] {
        assert_eq!(inv[key], src_inv[key], "invoice.{key}");
    }
    assert_eq!(inv["settlement"], "partially_paid");
    assert_eq!(inv["net_verified_cents"], 20_000);
    assert_eq!(inv["lines"], src_inv["lines"]);
    let payments = inv["payments"].as_array().unwrap();
    assert_eq!(payments.len(), 1);
    assert_eq!(payments[0]["id"], payment_id.as_str());
    assert_eq!(payments[0]["amount_cents"], 20_000);
    assert_eq!(payments[0]["status"], "verified");

    // The accepted deliverable and its result file bytes.
    let d = by_id(&restored.deliverables, &deliverable.id);
    assert_eq!(d["status"], "accepted");
    let accepted = &d["accepted_submission"];
    assert_eq!(accepted["id"], submission.id.as_str());
    assert_eq!(
        accepted["files"][0]["document_version_id"],
        result_version.as_str()
    );
    assert_eq!(download(&maria_b, &result_version).await, result_bytes);

    // The whole audit history moved, plus the import marker.
    let restored_ids: Vec<&Value> = restored.timeline.iter().map(|e| &e["id"]).collect();
    for e in &source.timeline {
        if e["action"] != "project.exported" {
            assert!(restored_ids.contains(&&e["id"]), "lost audit event {e}");
        }
    }
    let actions: Vec<&str> = restored
        .timeline
        .iter()
        .map(|e| e["action"].as_str().unwrap())
        .collect();
    for action in [
        "project.submit",
        "project.screen",
        "document.created",
        "document.version_added",
        "review.submitted",
        "decision.issued",
        "booking.confirmed",
        "invoice.issued",
        "payment.verified",
        "submission.accepted",
        "project.imported",
    ] {
        assert!(
            actions.contains(&action),
            "timeline lacks {action}: {actions:?}"
        );
    }

    // The restored install keeps working: invoicing the remaining confirmed
    // booking allocates a fresh invoice number.
    let ruth_b = persona(&other, "ruth").await;
    let next = b::create_invoice(&ruth_b, &pid, &[&lab_booking.id]).await;
    let next = b::issue(&ruth_b, &next.id).await;
    assert_ne!(next.number, invoice.number);
}

/// An archive exported before permits had chains (migration 0500) still
/// imports as amendable permits: chain and default name are rebuilt.
#[tokio::test]
async fn archive_from_before_permit_chains_rebuilds_them() {
    let app = spawn_app(true).await;
    let pid = project_id(&app.pool, PROJECT).await;
    let export = pitcairn::archive::export_project(&app.pool, app._dir.path(), &pid)
        .await
        .unwrap();
    let mut entries = unzip(&export.bytes);
    let rows: Vec<Value> = serde_json::from_slice(&entries["records/decisions.json"]).unwrap();
    let permit_ids: Vec<String> = rows
        .iter()
        .filter(|r| r["kind"] == "permit" && r["status"] == "issued")
        .map(|r| r["id"].as_str().unwrap().to_string())
        .collect();
    assert!(!permit_ids.is_empty(), "seeded project has a permit");
    let old: Vec<Value> = rows
        .into_iter()
        .map(|mut r| {
            let o = r.as_object_mut().unwrap();
            o.remove("chain_id");
            o.remove("title");
            r
        })
        .collect();
    entries.insert(
        "records/decisions.json".into(),
        serde_json::to_vec(&old).unwrap(),
    );
    let named: Vec<(&str, &[u8])> = entries
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_slice()))
        .collect();
    let bytes = zip_with(&named);

    let fresh_dir = tempfile::TempDir::new().unwrap();
    let fresh = empty_install(fresh_dir.path()).await;
    pitcairn::archive::import_bytes(&fresh, fresh_dir.path(), &bytes, 2_147_483_648)
        .await
        .expect("old archive imports");
    for id in permit_ids {
        let (chain, title): (Option<String>, String) =
            sqlx::query_as("SELECT chain_id, title FROM decisions WHERE id = ?")
                .bind(&id)
                .fetch_one(&fresh)
                .await
                .unwrap();
        assert_eq!(chain.as_deref(), Some(id.as_str()));
        assert_eq!(title, "Research permit");
    }
}
