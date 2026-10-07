//! Slice C: publication settings, public catalog and public downloads.

mod common;

use common::c::*;
use common::{persona, spawn_app};

const ANNA: &str = "anna@demo.pitcairn.invalid";
const MARIA: &str = "maria@demo.pitcairn.invalid";

/// Make the seeded project catalog-realistic: reference, approved status,
/// a start date and one sensitive + one ordinary site.
async fn publishable_project(app: &common::TestApp, project: &str) -> String {
    sqlx::query(
        "UPDATE projects SET reference = 'PIT-2024-001', status = 'approved',
                start_date = '2024-03-01' WHERE id = ?",
    )
    .bind(project)
    .execute(&app.pool)
    .await
    .unwrap();
    // project_sites: one sensitive (generalized publicly), one ordinary.
    sqlx::query(
        "INSERT INTO project_sites (id, project_id, name, geometry_json,
                min_lat, min_lng, max_lat, max_lng, sensitive, created_at)
         VALUES ('ps-sensitive', ?, 'Hidden reef',
                '{\"type\":\"Point\",\"coordinates\":[-130.1234,-25.0678]}',
                -25.07, -130.13, -25.06, -130.12, 1, '2024-01-01T00:00:00Z'),
                ('ps-open', ?, 'Bounty Bay',
                '{\"type\":\"Point\",\"coordinates\":[-130.09,-25.05]}',
                -25.06, -130.10, -25.04, -130.08, 0, '2024-01-01T00:00:00Z')",
    )
    .bind(project)
    .bind(project)
    .execute(&app.pool)
    .await
    .unwrap();
    "PIT-2024-001".to_string()
}

/// Set up an accepted submission with two result files; returns
/// (anna, maria, project, deliverable_id, version_ids).
async fn accepted_setup(
    app: &common::TestApp,
) -> (common::Client, common::Client, String, String, Vec<String>) {
    let anna = persona(app, "anna").await;
    let maria = persona(app, "maria").await;
    let project = seeded_project(app).await;
    let anna_id = user_id(app, ANNA).await;
    let maria_id = user_id(app, MARIA).await;
    let d = agreed_deliverable(&anna, &maria, &project, "dataset", &anna_id, &maria_id).await;

    let f1 = upload_clean_file(&anna, app, b"public data", "text/csv").await;
    let v1 = create_document(&anna, &project, "Results", "result", &f1).await;
    let f2 = upload_clean_file(&anna, app, b"%PDF-1.4 extra annex", "application/pdf").await;
    let v2 = create_document(&anna, &project, "Annex", "result", &f2).await;

    let s = submit(
        &anna,
        &d.id,
        serde_json::json!({"document_version_ids": [v1, v2], "note": "all done"}),
    )
    .await;
    let resp = maria
        .post(&format!("/api/v1/submissions/{}/accept", s.id))
        .await;
    assert_eq!(resp.status(), 200);
    (anna, maria, project, d.id, vec![v1, v2])
}

async fn catalog_projects(
    client: &common::Client,
    path: &str,
) -> Vec<pitcairn::dto::PublicProjectDto> {
    let resp = client.get(path).await;
    assert_eq!(resp.status(), 200, "GET {path}");
    let list: pitcairn::dto::ListResponse<pitcairn::dto::PublicProjectDto> =
        client.json(resp).await;
    list.items
}

#[tokio::test]
async fn metadata_only_publishes_without_files() {
    let app = spawn_app(true).await;
    let (_anna, maria, project, did, versions) = accepted_setup(&app).await;
    let reference = publishable_project(&app, &project).await;

    // Publish metadata only.
    let resp = maria
        .patch_json(
            &format!("/api/v1/deliverables/{did}/publication"),
            &serde_json::json!({"publish_level": "metadata", "embargo_until": null}),
        )
        .await;
    assert_eq!(resp.status(), 200);
    let res: pitcairn::dto::PublicationUpdateResponse = maria.json(resp).await;
    assert_eq!(res.deliverable.publish_level, "metadata");
    assert!(res.warning.contains("sensitive coordinates"));

    // Catalog lists the project by reference, organisation and summary.
    let anon = common::Client::anonymous(&app);
    let projects = catalog_projects(&anon, "/api/v1/public/projects").await;
    let entry = projects
        .iter()
        .find(|p| p.reference.as_deref() == Some("PIT-2024-001"))
        .expect("listed");
    assert_eq!(entry.title, "Coral health around Pitcairn");
    assert_eq!(
        entry.organisation,
        "Te Moana University (fictional), Wellington NZ"
    );
    assert_eq!(entry.year, Some(2024));
    assert!(entry.keywords.contains("coral"));
    let pd = entry
        .deliverables
        .iter()
        .find(|d| d.id == did)
        .expect("del");
    assert_eq!(pd.publish_level, "metadata");
    assert!(pd.files.is_empty(), "metadata-only exposes no files");

    // Sites: sensitive site is generalized (0.1° grid), ordinary site keeps
    // precise geometry.
    let sensitive = entry
        .sites
        .iter()
        .find(|s| s.name == "Hidden reef")
        .unwrap();
    assert!(sensitive.sensitive && sensitive.generalized);
    let open = entry.sites.iter().find(|s| s.name == "Bounty Bay").unwrap();
    assert!(!open.sensitive && !open.generalized);
    assert_eq!(open.geometry["type"], "Point");

    // Detail endpoint by reference returns the same payload.
    let resp = anon
        .get(&format!("/api/v1/public/projects/{reference}"))
        .await;
    assert_eq!(resp.status(), 200);
    let detail: pitcairn::dto::PublicProjectDto = anon.json(resp).await;
    assert_eq!(detail.deliverables.len(), 1);

    // File download → 404 (metadata only; existence never revealed).
    let resp = anon
        .get(&format!("/api/v1/public/files/{}/download", versions[0]))
        .await;
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn metadata_and_files_with_embargo_controls_downloads() {
    let app = spawn_app(true).await;
    let (_anna, maria, project, did, versions) = accepted_setup(&app).await;
    let _ = publishable_project(&app, &project).await;

    // Select only the first file for publication.
    let resp = maria
        .put_json(
            &format!("/api/v1/deliverables/{did}/publication-files"),
            &serde_json::json!({"document_version_ids": [versions[0]]}),
        )
        .await;
    assert_eq!(resp.status(), 200);
    let res: pitcairn::dto::PublicationFilesResponse = maria.json(resp).await;
    assert_eq!(res.document_version_ids, vec![versions[0].clone()]);
    assert!(res.warning.contains("sensitive coordinates"));

    // Embargo in the future → file 404, metadata shows files_available_from.
    let resp = maria
        .patch_json(
            &format!("/api/v1/deliverables/{did}/publication"),
            &serde_json::json!({"publish_level": "metadata_and_files", "embargo_until": "2999-01-01"}),
        )
        .await;
    assert_eq!(resp.status(), 200);
    let res: pitcairn::dto::PublicationUpdateResponse = maria.json(resp).await;
    assert_eq!(res.deliverable.publish_level, "metadata_and_files");
    assert_eq!(res.deliverable.embargo_until.as_deref(), Some("2999-01-01"));

    let anon = common::Client::anonymous(&app);
    let resp = anon
        .get(&format!("/api/v1/public/files/{}/download", versions[0]))
        .await;
    assert_eq!(resp.status(), 404);

    let resp = anon.get("/api/v1/public/projects/PIT-2024-001").await;
    let detail: pitcairn::dto::PublicProjectDto = anon.json(resp).await;
    let pd = detail
        .deliverables
        .iter()
        .find(|d| d.id == did)
        .expect("deliverable listed");
    assert_eq!(pd.files_available_from.as_deref(), Some("2999-01-01"));
    assert!(pd.files.is_empty(), "files hidden while embargoed");

    // Embargo passed (yesterday) → 200 for the selected file only.
    let resp = maria
        .patch_json(
            &format!("/api/v1/deliverables/{did}/publication"),
            &serde_json::json!({"publish_level": "metadata_and_files", "embargo_until": "2000-01-01"}),
        )
        .await;
    assert_eq!(resp.status(), 200);

    let resp = anon
        .get(&format!("/api/v1/public/files/{}/download", versions[0]))
        .await;
    assert_eq!(resp.status(), 200);
    let body = resp.bytes().await.unwrap();
    assert_eq!(body.as_ref(), b"public data");

    // The unselected second version → 404 (not published).
    let resp = anon
        .get(&format!("/api/v1/public/files/{}/download", versions[1]))
        .await;
    assert_eq!(resp.status(), 404);

    // And the file now shows in the catalog entry.
    let resp = anon.get("/api/v1/public/projects/PIT-2024-001").await;
    let detail: pitcairn::dto::PublicProjectDto = anon.json(resp).await;
    assert_eq!(detail.deliverables[0].files.len(), 1);
    assert_eq!(
        detail.deliverables[0].files[0].document_version_id,
        versions[0]
    );
}

#[tokio::test]
async fn publication_rules_enforced() {
    let app = spawn_app(true).await;
    let (anna, maria, project, did, versions) = accepted_setup(&app).await;
    let ruth = persona(&app, "ruth").await;

    // Only coordinators may publish.
    let resp = ruth
        .patch_json(
            &format!("/api/v1/deliverables/{did}/publication"),
            &serde_json::json!({"publish_level": "metadata", "embargo_until": null}),
        )
        .await;
    assert_eq!(resp.status(), 403);
    let resp = anna
        .patch_json(
            &format!("/api/v1/deliverables/{did}/publication"),
            &serde_json::json!({"publish_level": "metadata", "embargo_until": null}),
        )
        .await;
    assert_eq!(resp.status(), 403);

    // Invalid level / invalid embargo date → 422.
    let resp = maria
        .patch_json(
            &format!("/api/v1/deliverables/{did}/publication"),
            &serde_json::json!({"publish_level": "everything", "embargo_until": "someday"}),
        )
        .await;
    assert_eq!(resp.status(), 422);
    let err: serde_json::Value = maria.json(resp).await;
    assert!(err["error"]["fields"]["publish_level"].is_string());
    assert!(err["error"]["fields"]["embargo_until"].is_string());

    // Selecting files not in the accepted submission → 422.
    let f = upload_clean_file(&anna, &app, b"unrelated", "text/csv").await;
    let stray_v = create_document(&anna, &project, "Unrelated", "result", &f).await;
    let resp = maria
        .put_json(
            &format!("/api/v1/deliverables/{did}/publication-files"),
            &serde_json::json!({"document_version_ids": [versions[0], stray_v]}),
        )
        .await;
    assert_eq!(resp.status(), 422);

    // Publishing a not-yet-accepted deliverable → 409.
    let maria_id = user_id(&app, MARIA).await;
    let anna_id = user_id(&app, ANNA).await;
    let open = agreed_deliverable(&anna, &maria, &project, "report", &anna_id, &maria_id).await;
    let resp = maria
        .patch_json(
            &format!("/api/v1/deliverables/{}/publication", open.id),
            &serde_json::json!({"publish_level": "metadata", "embargo_until": null}),
        )
        .await;
    assert_eq!(resp.status(), 409);
    let err: serde_json::Value = maria.json(resp).await;
    assert_eq!(err["error"]["code"], "invalid_transition");
}

#[tokio::test]
async fn unpublished_deliverable_keeps_project_out_of_catalog() {
    let app = spawn_app(true).await;
    let (_a, _m, project, _did, _v) = accepted_setup(&app).await;
    let _ = publishable_project(&app, &project).await;
    let anon = common::Client::anonymous(&app);

    // Accepted but not published → not in the catalog at all.
    let projects = catalog_projects(&anon, "/api/v1/public/projects").await;
    assert!(projects.is_empty());

    let resp = anon.get("/api/v1/public/projects/PIT-2024-001").await;
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn catalog_search_and_filters() {
    let app = spawn_app(true).await;
    let (_a, maria, project, did, _v) = accepted_setup(&app).await;
    let _ = publishable_project(&app, &project).await;
    maria
        .patch_json(
            &format!("/api/v1/deliverables/{did}/publication"),
            &serde_json::json!({"publish_level": "metadata", "embargo_until": null}),
        )
        .await;

    let anon = common::Client::anonymous(&app);
    // Full-text search.
    assert_eq!(
        catalog_projects(&anon, "/api/v1/public/projects?q=coral")
            .await
            .len(),
        1
    );
    assert!(
        catalog_projects(&anon, "/api/v1/public/projects?q=volcano")
            .await
            .is_empty()
    );
    // Year filter (start_date = 2024-03-01).
    assert_eq!(
        catalog_projects(&anon, "/api/v1/public/projects?year=2024")
            .await
            .len(),
        1
    );
    assert!(
        catalog_projects(&anon, "/api/v1/public/projects?year=1999")
            .await
            .is_empty()
    );
    // Bbox overlapping a site vs not.
    assert_eq!(
        catalog_projects(&anon, "/api/v1/public/projects?bbox=-131,-26,-129,-24")
            .await
            .len(),
        1
    );
    assert!(
        catalog_projects(&anon, "/api/v1/public/projects?bbox=0,0,10,10")
            .await
            .is_empty()
    );
    // Malformed bbox → 400.
    let resp = anon.get("/api/v1/public/projects?bbox=oops").await;
    assert_eq!(resp.status(), 400);
}

#[tokio::test]
async fn catalog_never_leaks_internal_content() {
    let app = spawn_app(true).await;
    let (_a, maria, project, did, _v) = accepted_setup(&app).await;
    let _ = publishable_project(&app, &project).await;

    // Seed internal text that must never appear publicly: an internal-only
    // thread (staff opinion) + a shared thread with internal phrasing.
    let maria_id = user_id(&app, MARIA).await;
    sqlx::query(
        "INSERT INTO threads (id, project_id, anchor_type, anchor_key, visibility, created_at)
         VALUES ('t-internal', ?, 'deliverable', ?, 'internal', '2024-01-01T00:00:00Z')",
    )
    .bind(&project)
    .bind(&did)
    .execute(&app.pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO messages (id, thread_id, author_id, body, created_at)
         VALUES ('m-opinion', 't-internal', ?, 'CONFIDENTIAL STAFF OPINION TEXT', '2024-01-01T00:00:00Z')",
    )
    .bind(&maria_id)
    .execute(&app.pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO threads (id, project_id, anchor_type, anchor_key, visibility, created_at)
         VALUES ('t-shared', ?, 'project', '', 'shared', '2024-01-01T00:00:00Z')",
    )
    .bind(&project)
    .execute(&app.pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO messages (id, thread_id, author_id, body, created_at)
         VALUES ('m-shared', 't-shared', ?, 'INTERNAL SECRET THREAD TEXT', '2024-01-01T00:00:00Z')",
    )
    .bind(&maria_id)
    .execute(&app.pool)
    .await
    .unwrap();

    maria
        .patch_json(
            &format!("/api/v1/deliverables/{did}/publication"),
            &serde_json::json!({"publish_level": "metadata", "embargo_until": null}),
        )
        .await;

    let anon = common::Client::anonymous(&app);
    for path in [
        "/api/v1/public/projects".to_string(),
        "/api/v1/public/projects/PIT-2024-001".to_string(),
    ] {
        let resp = anon.get(&path).await;
        assert_eq!(resp.status(), 200);
        let body = resp.text().await.unwrap();
        assert!(
            !body.contains("INTERNAL SECRET"),
            "{path} leaks thread text"
        );
        assert!(!body.contains("CONFIDENTIAL"), "{path} leaks opinion text");
        assert!(!body.contains("passport"), "{path} leaks personal docs");
        assert!(!body.contains("audit"), "{path} leaks audit");
    }
}

#[tokio::test]
async fn catalog_respects_public_catalog_enabled_setting() {
    let app = spawn_app(true).await;
    let (_a, maria, project, did, versions) = accepted_setup(&app).await;
    let _ = publishable_project(&app, &project).await;
    maria
        .patch_json(
            &format!("/api/v1/deliverables/{did}/publication"),
            &serde_json::json!({"publish_level": "metadata", "embargo_until": null}),
        )
        .await;

    sqlx::query("UPDATE settings SET value = 'false' WHERE key = 'public_catalog_enabled'")
        .execute(&app.pool)
        .await
        .unwrap();

    let anon = common::Client::anonymous(&app);
    for path in [
        "/api/v1/public/projects".to_string(),
        "/api/v1/public/projects/PIT-2024-001".to_string(),
        format!("/api/v1/public/files/{}/download", versions[0]),
    ] {
        let resp = anon.get(&path).await;
        assert_eq!(resp.status(), 404, "{path} should 404 while disabled");
    }
}
