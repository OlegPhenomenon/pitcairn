//! Slice D: legacy CSV import — preview flags duplicates and bad coordinates,
//! commit creates closed legacy projects (§8).

mod common;

use common::{persona, spawn_app};
use pitcairn::dto::{ImportCommitResponse, LegacyImportPreviewResponse};

const SAMPLE: &str = include_str!("../fixtures/legacy_projects_sample.csv");

async fn preview(c: &common::Client, csv: &str) -> reqwest::Response {
    c.request(reqwest::Method::POST, "/api/v1/admin/import/legacy/preview")
        .header("content-type", "text/csv")
        .body(csv.to_string())
        .send()
        .await
        .expect("send preview")
}

#[tokio::test]
async fn preview_flags_duplicate_and_bad_coordinate_then_commit_creates_closed_legacy_projects() {
    let app = spawn_app(true).await;
    let admin = persona(&app, "admin").await;

    let resp = preview(&admin, SAMPLE).await;
    assert_eq!(resp.status(), 200);
    let p: LegacyImportPreviewResponse = admin.json(resp).await;
    assert_eq!(p.batch.status, "previewed");
    assert_eq!(p.rows.len(), 6);
    for row in &p.rows[..4] {
        assert!(
            row.errors.is_empty(),
            "row {} errors {:?}",
            row.index,
            row.errors
        );
        assert!(row.duplicate_of.is_none(), "row {}", row.index);
    }
    // Same normalized title + organisation + year as row 3.
    assert!(p.rows[4].duplicate_of.is_some(), "duplicate not flagged");
    // Coordinate outside the Pitcairn EEZ bbox.
    assert!(
        p.rows[5].errors.contains_key("lat"),
        "{:?}",
        p.rows[5].errors
    );
    assert!(p.rows[5].errors["lat"].contains("EEZ"));

    let resp = admin
        .post(&format!("/api/v1/admin/import/{}/commit", p.batch.id))
        .await;
    assert_eq!(resp.status(), 200);
    let c: ImportCommitResponse = admin.json(resp).await;
    assert_eq!((c.created, c.skipped), (4, 2), "{:?}", c.errors);

    let rows: Vec<(String, String, i64, Option<String>)> = sqlx::query_as(
        "SELECT reference, status, legacy, closed_reason FROM projects
         WHERE reference LIKE 'MSB-201_-0%' AND reference < 'MSB-2016' ORDER BY reference",
    )
    .fetch_all(&app.pool)
    .await
    .unwrap();
    assert_eq!(
        rows.iter().map(|r| r.0.as_str()).collect::<Vec<_>>(),
        vec![
            "MSB-2011-003",
            "MSB-2012-007",
            "MSB-2013-002",
            "MSB-2014-011"
        ]
    );
    assert!(rows.iter().all(|r| r.1 == "closed" && r.2 == 1));

    // Lead stub users are disabled; sites created; report → published
    // metadata-only deliverable with the URL as an external link.
    let (disabled,): (Option<String>,) = sqlx::query_as(
        "SELECT disabled_at FROM users WHERE email = 'r.marlow@legacy.example.invalid'",
    )
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert!(disabled.is_some());
    let (sites, publish, url): (i64, String, String) = sqlx::query_as(
        "SELECT (SELECT COUNT(*) FROM project_sites s WHERE s.project_id = p.id),
                d.publish_level, el.url
         FROM projects p JOIN deliverables d ON d.project_id = p.id
         JOIN deliverable_submissions sub ON sub.deliverable_id = d.id
         JOIN external_links el ON el.submission_id = sub.id
         WHERE p.reference = 'MSB-2011-003'",
    )
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(sites, 1);
    assert_eq!(publish, "metadata");
    assert_eq!(url, "https://archive.example.org/msb/limpets-2011.pdf");
    let no_report: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM deliverables d JOIN projects p ON p.id = d.project_id
         WHERE p.reference = 'MSB-2012-007'",
    )
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(no_report, 0);

    // Committing twice is refused; a fresh preview now flags every row as a
    // duplicate of the imported projects.
    let again = admin
        .post(&format!("/api/v1/admin/import/{}/commit", p.batch.id))
        .await;
    assert_eq!(again.status(), 409);
    let p2: LegacyImportPreviewResponse = admin.json(preview(&admin, SAMPLE).await).await;
    assert!(p2.rows[..4].iter().all(|r| r.duplicate_of.is_some()));
}

#[tokio::test]
async fn legacy_import_is_admin_only_and_validates_input() {
    let app = spawn_app(true).await;
    let maria = persona(&app, "maria").await;
    assert_eq!(preview(&maria, SAMPLE).await.status(), 403);

    let admin = persona(&app, "admin").await;
    // Missing required columns.
    assert_eq!(
        preview(&admin, "title,organisation\nX,Y\n").await.status(),
        422
    );
    // Executables are rejected by the scanner before any parsing.
    let resp = admin
        .request(reqwest::Method::POST, "/api/v1/admin/import/legacy/preview")
        .header("content-type", "text/csv")
        .body(b"MZ\x90\x00binary".to_vec())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 422);
    // Unknown batch.
    assert_eq!(
        admin
            .post("/api/v1/admin/import/nope/commit")
            .await
            .status(),
        404
    );
    // Missing CSRF header is refused.
    let resp = admin
        .request_no_csrf(reqwest::Method::POST, "/api/v1/admin/import/legacy/preview")
        .body(SAMPLE)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 403);
}
