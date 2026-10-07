//! Security regression: the anonymous catalog's `bbox` filter must match
//! sensitive sites by their generalized 0.1° bounds — the same geometry the
//! response shows — so narrowing searches cannot recover the precise point.

mod common;

use common::c::user_id;
use common::{Client, spawn_app};
use serde_json::Value;

#[tokio::test]
async fn bbox_filter_uses_generalized_bounds_for_sensitive_sites() {
    let app = spawn_app(true).await;
    let tv: String =
        sqlx::query_scalar("SELECT id FROM template_versions ORDER BY created_at LIMIT 1")
            .fetch_one(&app.pool)
            .await
            .unwrap();
    let admin = user_id(&app, "admin@demo.pitcairn.invalid").await;
    // A visible (legacy) project whose only site is a sensitive point far
    // from every seeded site: generalized cell lat [-22.2,-22.1], lng [-126.6,-126.5].
    sqlx::query(
        "INSERT INTO projects (id, reference, title, template_version_id, status, legacy, created_by, created_at)
         VALUES ('sec-bbox', 'SEC-2020-0001', 'Sensitive bbox probe', ?, 'closed', 1, ?, '2020-01-01T00:00:00Z')",
    )
    .bind(&tv)
    .bind(&admin)
    .execute(&app.pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO project_sites (id, project_id, name, geometry_json, min_lat, min_lng, max_lat, max_lng, sensitive, created_at)
         VALUES ('sec-bbox-site', 'sec-bbox', 'Nest', ?, -22.1234, -126.5432, -22.1234, -126.5432, 1, '2020-01-01T00:00:00Z')",
    )
    .bind(r#"{"type":"Point","coordinates":[-126.5432,-22.1234]}"#)
    .execute(&app.pool)
    .await
    .unwrap();

    let anon = Client::anonymous(&app);
    let found = |bbox: &'static str| {
        let anon = &anon;
        async move {
            let resp = anon
                .get(&format!("/api/v1/public/projects?bbox={bbox}"))
                .await;
            assert_eq!(resp.status(), 200);
            let body: Value = anon.json(resp).await;
            body["items"]
                .as_array()
                .unwrap()
                .iter()
                .any(|p| p["reference"] == "SEC-2020-0001")
        }
    };

    // Every probe inside the generalized cell answers the same — whether or
    // not it covers the precise point.
    for bbox in [
        "-126.5433,-22.1235,-126.5431,-22.1233", // around the precise point
        "-126.599,-22.199,-126.598,-22.198",     // SW corner, far from the point
        "-126.501,-22.101,-126.5005,-22.1005",   // NE corner
        "-126.59,-22.13,-126.58,-22.12",         // west of the point
    ] {
        assert!(found(bbox).await, "probe {bbox} inside the cell must match");
    }
    // Outside the generalized cell: no match.
    assert!(!found("-126.49,-22.15,-126.48,-22.14").await);
    assert!(!found("-126.56,-22.09,-126.55,-22.08").await);

    // The response itself only shows the generalized cell.
    let resp = anon.get("/api/v1/public/projects/SEC-2020-0001").await;
    let body: Value = anon.json(resp).await;
    let site = &body["sites"][0];
    assert_eq!(site["generalized"], true);
    assert!(!site["geometry"].to_string().contains("126.5432"));
}
