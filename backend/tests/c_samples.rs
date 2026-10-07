//! Slice C: samples registry — CRUD, site/deliverable scoping, workspace count.

mod common;

use common::c::*;
use common::{persona, spawn_app};

const ANNA: &str = "anna@demo.pitcairn.invalid";
const MARIA: &str = "maria@demo.pitcairn.invalid";

/// Insert a project_site row directly (the demo seed has none) — returns
/// its `project_sites.id`, which `samples.site_id` references.
async fn insert_site(app: &common::TestApp, project: &str, id: &str) -> String {
    sqlx::query(
        "INSERT INTO project_sites (id, project_id, name, geometry_json,
                min_lat, min_lng, max_lat, max_lng, sensitive, created_at)
         VALUES (?, ?, ?, '{\"type\":\"Point\",\"coordinates\":[-130.1,-25.05]}',
                -25.06, -130.11, -25.04, -130.09, 0, '2024-01-01T00:00:00Z')",
    )
    .bind(id)
    .bind(project)
    .bind(id)
    .execute(&app.pool)
    .await
    .unwrap();
    id.to_string()
}

#[tokio::test]
async fn samples_crud_and_workspace_count() {
    let app = spawn_app(true).await;
    let anna = persona(&app, "anna").await;
    let maria = persona(&app, "maria").await;
    let project = seeded_project(&app).await;
    let site = insert_site(&app, &project, "site-a").await;
    let anna_id = user_id(&app, ANNA).await;
    let maria_id = user_id(&app, MARIA).await;
    let d = create_deliverable(
        &anna,
        &project,
        "samples",
        "2030-01-01",
        &anna_id,
        &maria_id,
    )
    .await;

    // Create: team member, linked to a project site + deliverable.
    let resp = anna
        .post_json(
            &format!("/api/v1/projects/{project}/samples"),
            &serde_json::json!({
                "code": "WB-01",
                "material": "seawater",
                "collected_on": "2024-05-10",
                "site_id": site,
                "custodian_org": "Te Moana University",
                "storage_location": "MSB freezer 2",
                "notes": "surface sample",
                "related_deliverable_ids": [d.id],
            }),
        )
        .await;
    assert_eq!(resp.status(), 201);
    let s: pitcairn::dto::SampleDto = anna.json(resp).await;
    assert_eq!(s.code, "WB-01");
    assert_eq!(s.material, "seawater");
    assert_eq!(s.site_id.as_deref(), Some("site-a"));
    assert_eq!(s.collected_on.as_deref(), Some("2024-05-10"));
    assert_eq!(s.related_deliverable_ids, vec![d.id.clone()]);

    // Coordinator can create too.
    let resp = maria
        .post_json(
            &format!("/api/v1/projects/{project}/samples"),
            &serde_json::json!({"code": "SD-01", "material": "sediment"}),
        )
        .await;
    assert_eq!(resp.status(), 201);
    let s2: pitcairn::dto::SampleDto = maria.json(resp).await;

    // List returns items+total.
    let resp = anna
        .get(&format!("/api/v1/projects/{project}/samples"))
        .await;
    assert_eq!(resp.status(), 200);
    let list: pitcairn::dto::ListResponse<pitcairn::dto::SampleDto> = anna.json(resp).await;
    assert_eq!(list.total, 2);
    assert_eq!(list.items.len(), 2);

    // Patch: notes + clear related deliverables; unset fields stay.
    let resp = maria
        .patch_json(
            &format!("/api/v1/samples/{}", s.id),
            &serde_json::json!({"notes": "split into two vials", "related_deliverable_ids": []}),
        )
        .await;
    assert_eq!(resp.status(), 200);
    let s: pitcairn::dto::SampleDto = maria.json(resp).await;
    assert_eq!(s.notes, "split into two vials");
    assert!(s.related_deliverable_ids.is_empty());
    assert_eq!(s.material, "seawater");

    // Delete → 204.
    let resp = anna.delete(&format!("/api/v1/samples/{}", s2.id)).await;
    assert_eq!(resp.status(), 204);

    // Workspace sample count reflects it.
    let resp = anna.get(&format!("/api/v1/projects/{project}")).await;
    let ws: pitcairn::dto::ProjectWorkspaceDto = anna.json(resp).await;
    assert_eq!(ws.results.samples_count, 1);
}

#[tokio::test]
async fn samples_scoping_and_permissions() {
    let app = spawn_app(true).await;
    let anna = persona(&app, "anna").await;
    let tomasi = persona(&app, "tomasi").await; // viewer
    let lukas = persona(&app, "lukas").await;
    let project = seeded_project(&app).await;
    let lukas_id = user_id(&app, "lukas@demo.pitcairn.invalid").await;
    let maria_id = user_id(&app, MARIA).await;

    // A site + deliverable belonging to lukas's project must be rejected.
    let other_project = create_project(&lukas, "Lukas project").await;
    insert_site(&app, &other_project, "site-foreign").await;
    let foreign_d = create_deliverable(
        &lukas,
        &other_project,
        "samples",
        "2030-01-01",
        &lukas_id,
        &maria_id,
    )
    .await;

    let resp = anna
        .post_json(
            &format!("/api/v1/projects/{project}/samples"),
            &serde_json::json!({
                "code": "X1",
                "site_id": "site-foreign",
                "related_deliverable_ids": [foreign_d.id],
            }),
        )
        .await;
    assert_eq!(resp.status(), 422);
    let err: serde_json::Value = anna.json(resp).await;
    assert!(err["error"]["fields"]["site_id"].is_string());
    assert!(err["error"]["fields"]["related_deliverable_ids"].is_string());

    // Missing/invalid fields → 422.
    let resp = anna
        .post_json(
            &format!("/api/v1/projects/{project}/samples"),
            &serde_json::json!({"code": "", "collected_on": "someday"}),
        )
        .await;
    assert_eq!(resp.status(), 422);
    let err: serde_json::Value = anna.json(resp).await;
    assert!(err["error"]["fields"]["code"].is_string());
    assert!(err["error"]["fields"]["collected_on"].is_string());

    // Viewer cannot create or patch.
    let resp = tomasi
        .post_json(
            &format!("/api/v1/projects/{project}/samples"),
            &serde_json::json!({"code": "V1"}),
        )
        .await;
    assert_eq!(resp.status(), 403);

    // Outsider cannot list.
    let resp = lukas
        .get(&format!("/api/v1/projects/{project}/samples"))
        .await;
    assert_eq!(resp.status(), 403);
}
