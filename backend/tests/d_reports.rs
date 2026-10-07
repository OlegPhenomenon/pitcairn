//! Slice D: deliverable report and measurement series (units never mixed).

mod common;

use common::d::project_id;
use common::{persona, spawn_app};
use pitcairn::dto::{
    DeliverablesReportResponse, ListResponse, MeasurementVariableDto, MeasurementsReportResponse,
};

#[tokio::test]
async fn measurement_report_separates_units() {
    let app = spawn_app(true).await;
    // Add coral cover recorded as a fraction by another dataset: same
    // variable, different unit → must become a separate series.
    let pid = project_id(&app.pool, "Coral cover transects").await;
    let (deliverable_id, submission_id): (String, String) = sqlx::query_as(
        "SELECT d.id, s.id FROM deliverables d JOIN deliverable_submissions s ON s.deliverable_id = d.id
         WHERE d.project_id = ? AND d.kind = 'dataset'",
    )
    .bind(&pid)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    for (site, day, value) in [
        ("Bounty Bay transect", "2025-03-12", 0.31),
        ("Down Rope transect", "2025-03-17", 0.38),
    ] {
        sqlx::query(
            "INSERT INTO measurements (id, project_id, deliverable_id, submission_id, site_name, observed_on,
                                       variable_key, value, unit, source_label, created_at)
             VALUES (?, ?, ?, ?, ?, ?, 'coral_cover_percent', ?, 'fraction', 'legacy fraction export', ?)",
        )
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(&pid)
        .bind(&deliverable_id)
        .bind(&submission_id)
        .bind(site)
        .bind(day)
        .bind(value)
        .bind("2026-01-01T00:00:00Z")
        .execute(&app.pool)
        .await
        .unwrap();
    }

    let maria = persona(&app, "maria").await;
    let resp = maria.get("/api/v1/reports/measurements/variables").await;
    assert_eq!(resp.status(), 200);
    let vars: ListResponse<MeasurementVariableDto> = maria.json(resp).await;
    let coral: Vec<_> = vars
        .items
        .iter()
        .filter(|v| v.variable_key == "coral_cover_percent")
        .collect();
    assert_eq!(coral.len(), 2, "one row per (variable, unit)");
    let pct = coral.iter().find(|v| v.unit == "%").expect("% row");
    assert_eq!(pct.count, 6, "3 sites × 2 years");
    assert_eq!(pct.projects, 1);
    assert!(
        vars.items
            .iter()
            .any(|v| v.variable_key == "seabird_nest_count" && v.unit == "nests")
    );

    let resp = maria
        .get("/api/v1/reports/measurements?variable_key=coral_cover_percent")
        .await;
    assert_eq!(resp.status(), 200);
    let report: MeasurementsReportResponse = maria.json(resp).await;
    assert_eq!(report.series.len(), 2);
    for s in &report.series {
        assert!(
            s.points.iter().all(|p| p.unit == s.unit),
            "series mixes units"
        );
        assert!(s.points.iter().all(|p| !p.source_label.is_empty()));
    }
    let pct = report.series.iter().find(|s| s.unit == "%").unwrap();
    assert_eq!(pct.points.len(), 6);
    let years: std::collections::BTreeSet<&str> =
        pct.points.iter().map(|p| &p.observed_on[..4]).collect();
    assert_eq!(years.len(), 2, "two comparable years");
    assert!(pct.points.iter().all(|p| p.project_reference.is_some()));

    // variable_key required.
    assert_eq!(
        maria.get("/api/v1/reports/measurements").await.status(),
        422
    );
}

#[tokio::test]
async fn deliverables_report_counts_and_totals() {
    let app = spawn_app(true).await;
    let maria = persona(&app, "maria").await;
    let resp = maria.get("/api/v1/reports/deliverables").await;
    assert_eq!(resp.status(), 200);
    let report: DeliverablesReportResponse = maria.json(resp).await;
    let seabirds = report
        .projects
        .iter()
        .find(|p| p.project_title == "Henderson Island seabird census")
        .expect("seabird row");
    assert_eq!(seabirds.total, 2);
    assert_eq!(seabirds.accepted, 1);
    assert_eq!(seabirds.overdue, 1);
    assert_eq!(seabirds.received, 1);
    assert_eq!(seabirds.agreed, 2);
    let y2024 = report
        .totals_by_year
        .iter()
        .find(|t| t.key == "2024")
        .expect("2024 totals");
    assert!(y2024.overdue >= 1);
    assert!(
        report
            .totals_by_organisation
            .iter()
            .any(|t| t.key == "North Sea Marine Lab (fictional)" && t.accepted == 2)
    );
}

#[tokio::test]
async fn reports_are_staff_only() {
    let app = spawn_app(true).await;
    let anna = persona(&app, "anna").await;
    for path in [
        "/api/v1/reports/deliverables",
        "/api/v1/reports/measurements/variables",
        "/api/v1/reports/measurements?variable_key=coral_cover_percent",
    ] {
        assert_eq!(anna.get(path).await.status(), 403, "{path}");
    }
}

async fn nitrate_report(maria: &common::Client) -> MeasurementsReportResponse {
    let resp = maria
        .get("/api/v1/reports/measurements?variable_key=nitrate")
        .await;
    assert_eq!(resp.status(), 200);
    maria.json(resp).await
}

/// §5 item 14 / §6: a measurement table submitted and accepted through the
/// API (not seeded) reaches the reports page with its unit and source; an
/// accepted correction replaces the old values there.
#[tokio::test]
async fn newly_accepted_measurement_table_reaches_reports() {
    use common::c::*;
    let app = spawn_app(true).await;
    let anna = persona(&app, "anna").await;
    let maria = persona(&app, "maria").await;
    let project = seeded_project(&app).await;
    sqlx::query("UPDATE projects SET reference = 'PIT-2030-042', status = 'approved' WHERE id = ?")
        .bind(&project)
        .execute(&app.pool)
        .await
        .unwrap();
    let anna_id = user_id(&app, "anna@demo.pitcairn.invalid").await;
    let maria_id = user_id(&app, "maria@demo.pitcairn.invalid").await;
    let d = agreed_deliverable(&anna, &maria, &project, "dataset", &anna_id, &maria_id).await;

    // Nothing about nitrate exists before the submission.
    assert!(nitrate_report(&maria).await.series.is_empty());

    let csv1: &[u8] = b"site,date,variable,value,unit\n\
                        Bounty Bay,2030-03-01,nitrate,1.25,umol/L\n\
                        Tedside,2030-03-02,nitrate,0.75,umol/L\n";
    let f1 = upload_clean_file(&anna, &app, csv1, "text/csv").await;
    let v1 = create_document(&anna, &project, "Nutrients", "result", &f1).await;
    let s1 = submit(
        &anna,
        &d.id,
        serde_json::json!({"document_version_ids": [v1]}),
    )
    .await;

    // Submitted but not accepted → not in reports yet.
    assert!(nitrate_report(&maria).await.series.is_empty());
    let resp = maria
        .post(&format!("/api/v1/submissions/{}/accept", s1.id))
        .await;
    assert_eq!(resp.status(), 200);

    let resp = maria.get("/api/v1/reports/measurements/variables").await;
    let vars: ListResponse<MeasurementVariableDto> = maria.json(resp).await;
    let nitrate = vars
        .items
        .iter()
        .find(|v| v.variable_key == "nitrate")
        .expect("variable listed for the report picker");
    assert_eq!(
        (nitrate.unit.as_str(), nitrate.count, nitrate.projects),
        ("umol/L", 2, 1)
    );

    let report = nitrate_report(&maria).await;
    assert_eq!(report.series.len(), 1);
    let series = &report.series[0];
    assert_eq!(series.unit, "umol/L");
    let points: Vec<(&str, &str, f64)> = series
        .points
        .iter()
        .map(|p| (p.site.as_str(), p.observed_on.as_str(), p.value))
        .collect();
    assert_eq!(
        points,
        vec![
            ("Bounty Bay", "2030-03-01", 1.25),
            ("Tedside", "2030-03-02", 0.75)
        ]
    );
    for p in &series.points {
        assert_eq!(p.unit, "umol/L");
        assert_eq!(p.project_id, project);
        assert_eq!(p.project_reference.as_deref(), Some("PIT-2030-042"));
        assert_eq!(
            p.source_label,
            "PIT-2030-042 — Final report (submission #1)"
        );
    }

    // Corrected table: the old values stay in the report until the
    // correction is accepted, then only the new values are shown.
    let csv2: &[u8] = b"site,date,variable,value,unit\n\
                        Bounty Bay,2030-03-01,nitrate,1.5,umol/L\n\
                        Tedside,2030-03-02,nitrate,0.5,umol/L\n";
    let f2 = upload_clean_file(&anna, &app, csv2, "text/csv").await;
    let v2 = create_document(&anna, &project, "Nutrients corrected", "result", &f2).await;
    let s2 = submit(
        &anna,
        &d.id,
        serde_json::json!({"document_version_ids": [v2]}),
    )
    .await;
    let values: Vec<f64> = nitrate_report(&maria).await.series[0]
        .points
        .iter()
        .map(|p| p.value)
        .collect();
    assert_eq!(values, vec![1.25, 0.75]);

    let resp = maria
        .post(&format!("/api/v1/submissions/{}/accept", s2.id))
        .await;
    assert_eq!(resp.status(), 200);
    let report = nitrate_report(&maria).await;
    assert_eq!(report.series.len(), 1);
    let points: Vec<(&str, f64, &str)> = report.series[0]
        .points
        .iter()
        .map(|p| (p.site.as_str(), p.value, p.source_label.as_str()))
        .collect();
    assert_eq!(
        points,
        vec![
            (
                "Bounty Bay",
                1.5,
                "PIT-2030-042 — Final report (submission #2)"
            ),
            (
                "Tedside",
                0.5,
                "PIT-2030-042 — Final report (submission #2)"
            ),
        ]
    );
}
