//! Slice D: legacy import — preview flags duplicates, bad coordinates and file
//! problems, commit creates closed legacy projects with their old documents
//! and accepted results (§8, spec §5 item 15).

mod common;

use common::a::get;
use common::d::zip_with;
use common::{persona, spawn_app, spawn_app_with};
use pitcairn::dto::{ImportCommitResponse, LegacyImportPreviewResponse};
use serde_json::Value;

const SAMPLE: &str = include_str!("../fixtures/legacy_projects_sample.csv");

async fn preview(c: &common::Client, csv: &str) -> reqwest::Response {
    c.request(reqwest::Method::POST, "/api/v1/admin/import/legacy/preview")
        .header("content-type", "text/csv")
        .body(csv.to_string())
        .send()
        .await
        .expect("send preview")
}

async fn preview_zip(c: &common::Client, zip: Vec<u8>) -> reqwest::Response {
    c.request(reqwest::Method::POST, "/api/v1/admin/import/legacy/preview")
        .header("content-type", "application/zip")
        .body(zip)
        .send()
        .await
        .expect("send zip preview")
}

const HEADER: &str = "reference,title,organisation,lead_name,lead_email,start_date,end_date,\
summary,keywords,site_name,lat,lng,report_title,report_url,report_file,dataset_title,\
dataset_file,application_file\n";

/// Row with an application in two versions, a report file and a two-file
/// dataset.
const ROW_FULL: &str = "OLD-2009-001,Lobster census at Bounty Bay,Bounty Lobster Trust (fictional),\
Dr Ada Christian,a.christian@legacy.example.invalid,2009-03-02,2009-03-30,Night dive census,\
\"lobsters, census\",Bounty Bay,-25.066,-130.104,Lobster census report 2009,,report.pdf,\
Census counts,data/counts.csv;data/sites.csv,application-v1.txt;application-v2.txt\n";

/// Row whose report file is not (yet) in the ZIP.
const ROW_MISSING: &str = "OLD-2010-004,Tide pool fish inventory,Island Reef Society (fictional),\
Dr Ben Young,b.young@legacy.example.invalid,2010-06-01,2010-06-20,Tide pool survey,fish,,,,\
Tide pool report,,tidepool.pdf,,,\n";

/// Same reference as ROW_FULL: a duplicate inside the file.
const ROW_DUPLICATE: &str = "OLD-2009-001,Lobster census repeat entry,Bounty Lobster Trust (fictional),\
Dr Ada Christian,a.christian@legacy.example.invalid,2009-04-01,2009-04-10,Repeat,,,,,,,,,,\n";

const APP_V1: &[u8] = b"Application form, first submission (1996 scan transcript)";
const APP_V2: &[u8] = b"Application form, corrected resubmission";
const REPORT: &[u8] = b"%PDF-1.4\n% fictional legacy lobster report\n";
const TIDEPOOL: &[u8] = b"%PDF-1.4\n% fictional tide pool report\n";
const COUNTS: &[u8] = b"site,count\nBounty Bay,41\n";
const SITES: &[u8] = b"site,lat,lng\nBounty Bay,-25.066,-130.104\n";

fn row<'a>(
    p: &'a LegacyImportPreviewResponse,
    reference: &str,
    title: &str,
) -> &'a pitcairn::dto::ImportRowPreviewDto {
    p.rows
        .iter()
        .find(|r| r.data["reference"] == reference && r.data["title"] == title)
        .unwrap_or_else(|| panic!("row {reference} {title}"))
}

async fn download(c: &common::Client, version_id: &str) -> Vec<u8> {
    let resp = c
        .get(&format!("/api/v1/document-versions/{version_id}/download"))
        .await;
    assert_eq!(resp.status(), 200, "download {version_id}");
    resp.bytes().await.unwrap().to_vec()
}

#[tokio::test]
async fn zip_import_flags_missing_file_and_duplicate_then_commits_documents_and_results() {
    let app = spawn_app(true).await;
    let admin = persona(&app, "admin").await;

    // First attempt: tidepool.pdf is missing and one row repeats a reference.
    let csv = format!("{HEADER}{ROW_FULL}{ROW_MISSING}{ROW_DUPLICATE}");
    let zip = zip_with(&[
        ("projects.csv", csv.as_bytes()),
        ("files/application-v1.txt", APP_V1),
        ("files/application-v2.txt", APP_V2),
        ("files/report.pdf", REPORT),
        ("files/data/counts.csv", COUNTS),
        ("files/data/sites.csv", SITES),
    ]);
    let resp = preview_zip(&admin, zip).await;
    assert_eq!(resp.status(), 200);
    let p: LegacyImportPreviewResponse = admin.json(resp).await;
    assert_eq!(p.rows.len(), 3);
    let full = row(&p, "OLD-2009-001", "Lobster census at Bounty Bay");
    assert!(full.errors.is_empty(), "{:?}", full.errors);
    assert!(full.duplicate_of.is_none());
    let mut names: Vec<(&str, &str)> = full
        .files
        .iter()
        .map(|f| (f.column.as_str(), f.name.as_str()))
        .collect();
    names.sort();
    assert_eq!(
        names,
        vec![
            ("application_file", "application-v1.txt"),
            ("application_file", "application-v2.txt"),
            ("dataset_file", "data/counts.csv"),
            ("dataset_file", "data/sites.csv"),
            ("report_file", "report.pdf"),
        ]
    );
    let missing = row(&p, "OLD-2010-004", "Tide pool fish inventory");
    assert!(
        missing.errors["report_file"].contains("tidepool.pdf: not found"),
        "{:?}",
        missing.errors
    );
    let dup = row(&p, "OLD-2009-001", "Lobster census repeat entry");
    assert!(
        dup.duplicate_of
            .as_deref()
            .is_some_and(|d| d.contains("OLD-2009-001")),
        "{:?}",
        dup.duplicate_of
    );

    // Fixed ZIP (wrapped in a folder, as "compress folder" makes it): the
    // missing file added, the duplicate row dropped.
    let csv = format!("{HEADER}{ROW_FULL}{ROW_MISSING}");
    let zip = zip_with(&[
        ("legacy-export/projects.csv", csv.as_bytes()),
        ("legacy-export/files/application-v1.txt", APP_V1),
        ("legacy-export/files/application-v2.txt", APP_V2),
        ("legacy-export/files/report.pdf", REPORT),
        ("legacy-export/files/tidepool.pdf", TIDEPOOL),
        ("legacy-export/files/data/counts.csv", COUNTS),
        ("legacy-export/files/data/sites.csv", SITES),
        ("__MACOSX/legacy-export/._projects.csv", b"\x00\x05\x16\x07"),
    ]);
    let resp = preview_zip(&admin, zip).await;
    assert_eq!(resp.status(), 200);
    let p: LegacyImportPreviewResponse = admin.json(resp).await;
    assert!(
        p.rows
            .iter()
            .all(|r| r.errors.is_empty() && r.duplicate_of.is_none()),
        "{:?}",
        p.rows
    );
    let resp = admin
        .post(&format!("/api/v1/admin/import/{}/commit", p.batch.id))
        .await;
    assert_eq!(resp.status(), 200);
    let c: ImportCommitResponse = admin.json(resp).await;
    assert_eq!((c.created, c.skipped), (2, 0), "{:?}", c.errors);

    // Staff see the imported project with its application in two versions.
    let maria = persona(&app, "maria").await;
    let pid: String =
        sqlx::query_scalar("SELECT id FROM projects WHERE reference = 'OLD-2009-001'")
            .fetch_one(&app.pool)
            .await
            .unwrap();
    let (status, docs) = get(&maria, &format!("/projects/{pid}/documents")).await;
    assert_eq!(status, 200, "{docs}");
    let docs = docs["items"].as_array().unwrap();
    let application = docs
        .iter()
        .find(|d| d["title"] == "Application")
        .expect("application document");
    assert_eq!(application["category"], "application");
    let versions = application["versions"].as_array().unwrap();
    assert_eq!(versions.len(), 2);
    let version = |n: i64| -> String {
        versions.iter().find(|v| v["number"] == n).unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string()
    };
    assert_eq!(download(&maria, &version(1)).await, APP_V1);
    assert_eq!(download(&maria, &version(2)).await, APP_V2);

    // Accepted report and dataset deliverables whose files download.
    let (status, deliverables) = get(&maria, &format!("/projects/{pid}/deliverables")).await;
    assert_eq!(status, 200, "{deliverables}");
    let deliverables = deliverables["items"].as_array().unwrap();
    assert_eq!(deliverables.len(), 2);
    let by_kind = |kind: &str| -> &Value {
        deliverables
            .iter()
            .find(|d| d["kind"] == kind)
            .unwrap_or_else(|| panic!("{kind} deliverable"))
    };
    for (kind, title) in [
        ("report", "Lobster census report 2009"),
        ("dataset", "Census counts"),
    ] {
        assert_eq!(by_kind(kind)["status"], "accepted");
        assert_eq!(by_kind(kind)["title"], title);
    }
    let submission_files = |deliverable: &Value| {
        let id = deliverable["id"].as_str().unwrap().to_string();
        let maria = &maria;
        async move {
            let (status, subs) = get(maria, &format!("/deliverables/{id}/submissions")).await;
            assert_eq!(status, 200, "{subs}");
            let sub = &subs["items"][0];
            assert_eq!(sub["status"], "accepted");
            sub["files"].as_array().unwrap().clone()
        }
    };
    let report_files = submission_files(by_kind("report")).await;
    assert_eq!(report_files.len(), 1);
    let report_version = report_files[0]["document_version_id"].as_str().unwrap();
    assert_eq!(download(&maria, report_version).await, REPORT);
    let dataset_files = submission_files(by_kind("dataset")).await;
    let mut dataset_bytes = Vec::new();
    for f in &dataset_files {
        dataset_bytes.push(download(&maria, f["document_version_id"].as_str().unwrap()).await);
    }
    dataset_bytes.sort();
    let mut expected = vec![COUNTS.to_vec(), SITES.to_vec()];
    expected.sort();
    assert_eq!(dataset_bytes, expected);

    // The second row's report file arrived too.
    let tide_pid: String =
        sqlx::query_scalar("SELECT id FROM projects WHERE reference = 'OLD-2010-004'")
            .fetch_one(&app.pool)
            .await
            .unwrap();
    let (_, tide) = get(&maria, &format!("/projects/{tide_pid}/deliverables")).await;
    let tide_files = submission_files(&tide["items"][0]).await;
    assert_eq!(
        download(
            &maria,
            tide_files[0]["document_version_id"].as_str().unwrap()
        )
        .await,
        TIDEPOOL
    );
}

#[tokio::test]
async fn zip_preview_flags_bad_type_oversize_and_csv_file_columns_without_zip() {
    let app = spawn_app_with(true, |config| config.max_upload_bytes = 64 * 1024).await;
    let admin = persona(&app, "admin").await;

    let csv = format!(
        "{HEADER}\
OLD-2001-001,Old survey one,Org A (fictional),A Person,a@legacy.example.invalid,2001-01-01,2001-01-02,,,,,,,,,Raw data,tool.exe,\n\
OLD-2001-002,Old survey two,Org B (fictional),B Person,b@legacy.example.invalid,2001-02-01,2001-02-02,,,,,,Big report,,big.pdf,,,\n\
OLD-2001-003,Old survey three,Org C (fictional),C Person,c@legacy.example.invalid,2001-03-01,2001-03-02,,,,,,,,,,,fake.pdf\n"
    );
    let mut big = b"%PDF-1.4\n".to_vec();
    big.resize(70 * 1024, b'x');
    let zip = zip_with(&[
        ("projects.csv", csv.as_bytes()),
        ("files/tool.exe", b"MZ\x90\x00binary"),
        ("files/big.pdf", &big),
        ("files/fake.pdf", b"just text pretending to be a PDF"),
    ]);
    let resp = preview_zip(&admin, zip).await;
    assert_eq!(resp.status(), 200);
    let p: LegacyImportPreviewResponse = admin.json(resp).await;
    assert!(
        p.rows[0].errors["dataset_file"].contains("file type not allowed"),
        "{:?}",
        p.rows[0].errors
    );
    assert!(
        p.rows[1].errors["report_file"].contains("per-file limit"),
        "{:?}",
        p.rows[1].errors
    );
    assert!(
        p.rows[2].errors["application_file"].contains("rejected by the file scan"),
        "{:?}",
        p.rows[2].errors
    );
    // Commit creates nothing while every row has errors.
    let c: ImportCommitResponse = admin
        .json(
            admin
                .post(&format!("/api/v1/admin/import/{}/commit", p.batch.id))
                .await,
        )
        .await;
    assert_eq!((c.created, c.skipped), (0, 3));

    // A plain CSV cannot carry files.
    let csv = format!(
        "{HEADER}OLD-2002-001,Plain CSV with file,Org D (fictional),D Person,d@legacy.example.invalid,2002-01-01,2002-01-02,,,,,,,,,,,application.pdf\n"
    );
    let p: LegacyImportPreviewResponse = admin.json(preview(&admin, &csv).await).await;
    assert!(
        p.rows[0].errors["application_file"].contains("ZIP"),
        "{:?}",
        p.rows[0].errors
    );

    // A ZIP without a top-level CSV is refused outright.
    let resp = preview_zip(&admin, zip_with(&[("files/a.pdf", b"%PDF-1.4\n")])).await;
    assert_eq!(resp.status(), 422);
}

/// Zero-filled entries above the per-file limit deflate to a few bytes each;
/// preview must flag them as oversize (inflating each only up to the limit)
/// and still accept the upload.
#[tokio::test]
async fn zip_preview_flags_deflated_oversized_entries_without_keeping_them() {
    use std::io::Write;
    let app = spawn_app_with(true, |config| config.max_upload_bytes = 2048).await;
    let admin = persona(&app, "admin").await;

    let csv = format!(
        "{HEADER}OLD-2003-001,Zero survey,Org Z (fictional),Z Person,z@legacy.example.invalid,2003-01-01,2003-01-02,,,,,,,,,Zeros,zero-0.pdf;zero-1.pdf;zero-2.pdf,\n"
    );
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    zip.start_file("projects.csv", opts).unwrap();
    zip.write_all(csv.as_bytes()).unwrap();
    let zeros = vec![0u8; 4096];
    for i in 0..3 {
        zip.start_file(format!("files/zero-{i}.pdf"), opts).unwrap();
        zip.write_all(&zeros).unwrap();
    }
    let zip = zip.finish().unwrap().into_inner();
    assert!(zip.len() < 2048, "upload itself is within the limit");

    let resp = preview_zip(&admin, zip).await;
    assert_eq!(resp.status(), 200);
    let p: LegacyImportPreviewResponse = admin.json(resp).await;
    let error = &p.rows[0].errors["dataset_file"];
    for i in 0..3 {
        assert!(
            error.contains(&format!("zero-{i}.pdf: larger than")),
            "{error}"
        );
    }
    assert!(p.rows[0].files.is_empty());
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
