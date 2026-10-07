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

/// §5 item 12: the sample ↔ analysis-result link is readable by everyone who
/// can read the project — including a viewer-role team member.
#[tokio::test]
async fn sample_result_link_visible_to_viewer() {
    let app = spawn_app(true).await;
    let anna = persona(&app, "anna").await;
    let tomasi = persona(&app, "tomasi").await; // viewer
    let project = seeded_project(&app).await;
    let anna_id = user_id(&app, ANNA).await;
    let maria_id = user_id(&app, MARIA).await;
    let d = create_deliverable(
        &anna,
        &project,
        "dataset",
        "2030-01-01",
        &anna_id,
        &maria_id,
    )
    .await;

    let resp = anna
        .post_json(
            &format!("/api/v1/projects/{project}/samples"),
            &serde_json::json!({
                "code": "WB-07",
                "material": "seawater",
                "custodian_org": "Te Moana University",
                "related_deliverable_ids": [d.id],
            }),
        )
        .await;
    assert_eq!(resp.status(), 201);
    let created: pitcairn::dto::SampleDto = anna.json(resp).await;
    assert_eq!(created.related_deliverables.len(), 1);
    assert_eq!(created.related_deliverables[0].title, "Final report");

    // The viewer sees the sample with the result it relates to…
    let resp = tomasi
        .get(&format!("/api/v1/projects/{project}/samples"))
        .await;
    assert_eq!(resp.status(), 200);
    let list: pitcairn::dto::ListResponse<pitcairn::dto::SampleDto> = tomasi.json(resp).await;
    let sample = list
        .items
        .iter()
        .find(|s| s.code == "WB-07")
        .expect("viewer sees the sample");
    assert_eq!(sample.custodian_org, "Te Moana University");
    let related = &sample.related_deliverables;
    assert_eq!(related.len(), 1);
    assert_eq!(related[0].id, d.id);
    assert_eq!(related[0].title, "Final report");
    assert_eq!(related[0].kind, "dataset");

    // …and can open that result from the deliverables list.
    let resp = tomasi
        .get(&format!("/api/v1/projects/{project}/deliverables"))
        .await;
    assert_eq!(resp.status(), 200);
    let deliverables: pitcairn::dto::ListResponse<pitcairn::dto::DeliverableDto> =
        tomasi.json(resp).await;
    assert!(deliverables.items.iter().any(|x| x.id == d.id));
}

/// "Linked samples" on a deliverable is filtered on the server, so a link on
/// a sample beyond the first list page (default 50) is never lost.
#[tokio::test]
async fn samples_filter_by_deliverable_beyond_first_page() {
    let app = spawn_app(true).await;
    let anna = persona(&app, "anna").await;
    let tomasi = persona(&app, "tomasi").await; // viewer
    let project = seeded_project(&app).await;
    let anna_id = user_id(&app, ANNA).await;
    let maria_id = user_id(&app, MARIA).await;
    let d = create_deliverable(
        &anna,
        &project,
        "dataset",
        "2030-01-01",
        &anna_id,
        &maria_id,
    )
    .await;

    // 50 unlinked samples, then the 51st (last by code) linked to `d`.
    for i in 0..50 {
        sqlx::query(
            "INSERT INTO samples (id, project_id, code, created_at)
             VALUES (?, ?, ?, '2024-01-01T00:00:00Z')",
        )
        .bind(format!("bulk-{i:03}"))
        .bind(&project)
        .bind(format!("S-{i:03}"))
        .execute(&app.pool)
        .await
        .unwrap();
    }
    let resp = anna
        .post_json(
            &format!("/api/v1/projects/{project}/samples"),
            &serde_json::json!({
                "code": "S-050",
                "material": "sediment core",
                "custodian_org": "Te Moana University",
                "related_deliverable_ids": [d.id],
            }),
        )
        .await;
    assert_eq!(resp.status(), 201);

    // The unfiltered first page does not reach it…
    let resp = tomasi
        .get(&format!("/api/v1/projects/{project}/samples"))
        .await;
    let page: pitcairn::dto::ListResponse<pitcairn::dto::SampleDto> = tomasi.json(resp).await;
    assert_eq!(page.total, 51);
    assert!(page.items.iter().all(|s| s.code != "S-050"));

    // …the deliverable filter returns exactly the linked sample.
    let resp = tomasi
        .get(&format!(
            "/api/v1/projects/{project}/samples?deliverable_id={}",
            d.id
        ))
        .await;
    assert_eq!(resp.status(), 200);
    let linked: pitcairn::dto::ListResponse<pitcairn::dto::SampleDto> = tomasi.json(resp).await;
    assert_eq!(linked.total, 1);
    let codes: Vec<&str> = linked.items.iter().map(|s| s.code.as_str()).collect();
    assert_eq!(codes, vec!["S-050"]);
    assert_eq!(linked.items[0].material, "sediment core");
    assert_eq!(linked.items[0].custodian_org, "Te Moana University");

    // A deliverable nothing links to → empty.
    let resp = tomasi
        .get(&format!(
            "/api/v1/projects/{project}/samples?deliverable_id=unlinked"
        ))
        .await;
    let none: pitcairn::dto::ListResponse<pitcairn::dto::SampleDto> = tomasi.json(resp).await;
    assert_eq!(none.total, 0);
    assert!(none.items.is_empty());
}
