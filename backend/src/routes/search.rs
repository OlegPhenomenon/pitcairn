//! Staff search and reports (§5): `GET /search/projects` (staff see all,
//! experts only projects assigned to them), `GET /reports/deliverables`,
//! `GET /reports/measurements/variables`, `GET /reports/measurements`.

use std::collections::BTreeMap;

use axum::Json;
use axum::extract::{Query, State};
use axum::routing::{Router, get};
use serde::Deserialize;
use sqlx::FromRow;

use crate::AppState;
use crate::authz::{self, Actor};
use crate::dto::{
    DeliverableReportRowDto, DeliverableReportTotalsDto, DeliverablesReportResponse, ListResponse,
    MeasurementPointDto, MeasurementSeriesDto, MeasurementVariableDto, MeasurementsReportResponse,
    SearchProjectItemDto,
};
use crate::error::{AppError, AppResult};
use crate::validation::FieldErrors;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/search/projects", get(search_projects))
        .route("/reports/deliverables", get(report_deliverables))
        .route(
            "/reports/measurements/variables",
            get(report_measurement_variables),
        )
        .route("/reports/measurements", get(report_measurements))
}

fn today() -> String {
    chrono::Utc::now().format("%Y-%m-%d").to_string()
}

// ---------------------------------------------------------------------------
// GET /search/projects
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct SearchProjectsQuery {
    q: Option<String>,
    organisation: Option<String>,
    year: Option<String>,
    /// `minLng,minLat,maxLng,maxLat` — bbox intersection over project sites.
    bbox: Option<String>,
    status: Option<String>,
    has_overdue: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
}

const PROJECT_STATUSES: &[&str] = &[
    "draft",
    "submitted",
    "in_review",
    "changes_requested",
    "approved",
    "refused",
    "closed",
    "withdrawn",
];

/// Build the FTS5 MATCH expression: every whitespace-separated term becomes a
/// quoted prefix term (`"coral"*`), so punctuation can never break the query.
fn fts_query(q: &str) -> Option<String> {
    let terms: Vec<String> = q
        .split_whitespace()
        .filter(|t| !t.is_empty())
        .map(|t| format!("\"{}\"*", t.replace('"', "\"\"")))
        .collect();
    if terms.is_empty() {
        None
    } else {
        Some(terms.join(" "))
    }
}

#[derive(FromRow)]
struct SearchRow {
    id: String,
    reference: Option<String>,
    title: String,
    organisation: String,
    status: String,
    start_date: Option<String>,
    end_date: Option<String>,
    received: i64,
    overdue: i64,
}

async fn search_projects(
    State(state): State<AppState>,
    actor: Actor,
    Query(query): Query<SearchProjectsQuery>,
) -> AppResult<Json<ListResponse<SearchProjectItemDto>>> {
    // staff see everything; experts see only projects assigned to them (§5).
    if !actor.is_staff() && !actor.is_expert() {
        return Err(AppError::forbidden("search requires staff or expert role"));
    }
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    let offset = query.offset.unwrap_or(0).max(0);
    let today = today();

    let mut errors = FieldErrors::new();
    let mut bbox: Option<[f64; 4]> = None;
    if let Some(raw) = &query.bbox {
        let parts: Vec<&str> = raw.split(',').collect();
        let parsed: Option<[f64; 4]> = if parts.len() == 4 {
            let nums: Vec<f64> = parts
                .iter()
                .filter_map(|p| p.trim().parse::<f64>().ok())
                .collect();
            (nums.len() == 4).then(|| [nums[0], nums[1], nums[2], nums[3]])
        } else {
            None
        };
        match parsed {
            Some(b) if b[0] <= b[2] && b[1] <= b[3] => bbox = Some(b),
            _ => errors.check("bbox", false, "must be minLng,minLat,maxLng,maxLat numbers"),
        }
    }
    if let Some(year) = &query.year {
        errors.check(
            "year",
            year.len() == 4 && year.chars().all(|c| c.is_ascii_digit()),
            "must be a 4-digit year",
        );
    }
    if let Some(status) = &query.status {
        errors.check(
            "status",
            PROJECT_STATUSES.contains(&status.as_str()),
            "unknown project status",
        );
    }
    errors.finish()?;

    // Dynamic WHERE assembled from the active filters; every value is bound.
    let mut conds: Vec<String> = Vec::new();
    let mut binds: Vec<String> = Vec::new();

    if !actor.is_staff() {
        conds.push(
            "EXISTS (SELECT 1 FROM review_assignments ra
             WHERE ra.project_id = p.id AND ra.expert_id = ? AND ra.status != 'declined')"
                .to_string(),
        );
        binds.push(actor.user_id.clone());
    }
    if let Some(fts) = fts_query(query.q.as_deref().unwrap_or("")) {
        conds.push("p.rowid IN (SELECT rowid FROM projects_fts WHERE projects_fts MATCH ?)".into());
        binds.push(fts);
    }
    if let Some(org) = &query.organisation {
        conds.push("LOWER(p.organisation) LIKE '%' || LOWER(?) || '%'".into());
        binds.push(org.clone());
    }
    if let Some(year) = &query.year {
        // Active during the calendar year: range overlap, or either date in it.
        conds.push(
            "(substr(p.start_date,1,4) = ? OR substr(p.end_date,1,4) = ?
              OR (p.start_date IS NOT NULL AND p.end_date IS NOT NULL
                  AND p.start_date <= ? AND p.end_date >= ?))"
                .into(),
        );
        binds.push(year.clone());
        binds.push(year.clone());
        binds.push(format!("{year}-12-31"));
        binds.push(format!("{year}-01-01"));
    }
    if let Some(b) = bbox {
        conds.push(
            "EXISTS (SELECT 1 FROM project_sites s WHERE s.project_id = p.id
              AND NOT (s.max_lng < ? OR s.min_lng > ? OR s.max_lat < ? OR s.min_lat > ?))"
                .into(),
        );
        for v in b {
            binds.push(v.to_string());
        }
    }
    if let Some(status) = &query.status {
        conds.push("p.status = ?".into());
        binds.push(status.clone());
    }
    let has_overdue = matches!(query.has_overdue.as_deref(), Some("true") | Some("1"));
    let overdue_sql = "EXISTS (SELECT 1 FROM deliverables d WHERE d.project_id = p.id
        AND d.status NOT IN ('accepted','waived','cancelled') AND d.due_date < ?)";
    if has_overdue {
        conds.push(overdue_sql.into());
        binds.push(today.clone());
    }

    let where_clause = if conds.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conds.join(" AND "))
    };

    let count_sql = format!("SELECT COUNT(*) FROM projects p {where_clause}");
    let mut count_q = sqlx::query_scalar::<_, i64>(&count_sql);
    for b in &binds {
        count_q = count_q.bind(b);
    }
    let total = count_q.fetch_one(&state.pool).await?;

    let select_sql = format!(
        "SELECT p.id, p.reference, p.title, p.organisation, p.status, p.start_date, p.end_date,
                (SELECT COUNT(*) FROM deliverables d WHERE d.project_id = p.id
                  AND EXISTS (SELECT 1 FROM deliverable_submissions s WHERE s.deliverable_id = d.id)) AS received,
                (SELECT COUNT(*) FROM deliverables d WHERE d.project_id = p.id
                  AND d.status NOT IN ('accepted','waived','cancelled') AND d.due_date < ?) AS overdue
         FROM projects p
         {where_clause}
         ORDER BY p.created_at DESC LIMIT ? OFFSET ?"
    );
    // The overdue count's `?` precedes the WHERE binds in the statement.
    let mut select_q = sqlx::query_as::<_, SearchRow>(&select_sql).bind(&today);
    for b in &binds {
        select_q = select_q.bind(b);
    }
    let rows = select_q
        .bind(limit)
        .bind(offset)
        .fetch_all(&state.pool)
        .await?;

    let items = rows
        .into_iter()
        .map(|r| SearchProjectItemDto {
            id: r.id,
            reference: r.reference,
            title: r.title,
            organisation: r.organisation,
            status: r.status,
            start_date: r.start_date,
            end_date: r.end_date,
            deliverables_received: r.received,
            deliverables_overdue: r.overdue,
        })
        .collect();
    Ok(Json(ListResponse { items, total }))
}

// ---------------------------------------------------------------------------
// GET /reports/deliverables
// ---------------------------------------------------------------------------

#[derive(FromRow)]
struct DeliverableReportRow {
    project_id: String,
    reference: Option<String>,
    title: String,
    organisation: String,
    year: String,
    agreed: i64,
    received: i64,
    accepted: i64,
    overdue: i64,
    waived: i64,
    total: i64,
}

async fn report_deliverables(
    State(state): State<AppState>,
    actor: Actor,
) -> AppResult<Json<DeliverablesReportResponse>> {
    authz::require_role(&actor, &authz::STAFF_ROLES)?;
    let today = today();

    // Per-deliverable flags computed in a derived table (SQLite is fussy
    // about correlated subqueries inside aggregate arguments).
    let rows: Vec<DeliverableReportRow> = sqlx::query_as(
        "SELECT p.id AS project_id, p.reference, p.title, p.organisation,
                substr(COALESCE(p.start_date, p.created_at), 1, 4) AS year,
                SUM(CASE WHEN d.status NOT IN ('proposed','cancelled') THEN 1 ELSE 0 END) AS agreed,
                SUM(CASE WHEN d.has_received THEN 1 ELSE 0 END) AS received,
                SUM(CASE WHEN d.status = 'accepted' THEN 1 ELSE 0 END) AS accepted,
                SUM(CASE WHEN d.status NOT IN ('accepted','waived','cancelled')
                        AND d.due_date < ? THEN 1 ELSE 0 END) AS overdue,
                SUM(CASE WHEN d.status = 'waived' THEN 1 ELSE 0 END) AS waived,
                COUNT(d.id) AS total
         FROM projects p
         JOIN (SELECT id, project_id, status, due_date,
                      (EXISTS (SELECT 1 FROM deliverable_submissions s
                               WHERE s.deliverable_id = deliverables.id)
                       OR status IN ('submitted','changes_requested','accepted')) AS has_received
               FROM deliverables WHERE status != 'cancelled') d
           ON d.project_id = p.id
         GROUP BY p.id
         ORDER BY year DESC, p.title",
    )
    .bind(&today)
    .fetch_all(&state.pool)
    .await?;

    fn bucket(
        map: &mut BTreeMap<String, (i64, i64, i64, i64, i64, i64)>,
        key: String,
    ) -> &mut (i64, i64, i64, i64, i64, i64) {
        map.entry(key).or_default()
    }

    let mut by_year: BTreeMap<String, (i64, i64, i64, i64, i64, i64)> = BTreeMap::new();
    let mut by_org: BTreeMap<String, (i64, i64, i64, i64, i64, i64)> = BTreeMap::new();
    let mut projects = Vec::new();
    for r in rows {
        for map_key in [
            (&mut by_year, r.year.clone()),
            (&mut by_org, r.organisation.clone()),
        ] {
            let acc = bucket(map_key.0, map_key.1);
            acc.0 += r.agreed;
            acc.1 += r.received;
            acc.2 += r.accepted;
            acc.3 += r.overdue;
            acc.4 += r.waived;
            acc.5 += r.total;
        }
        projects.push(DeliverableReportRowDto {
            project_id: r.project_id,
            project_reference: r.reference,
            project_title: r.title,
            organisation: r.organisation,
            agreed: r.agreed,
            received: r.received,
            accepted: r.accepted,
            overdue: r.overdue,
            waived: r.waived,
            total: r.total,
        });
    }

    let totals = |map: BTreeMap<String, (i64, i64, i64, i64, i64, i64)>| {
        map.into_iter()
            .map(
                |(key, (agreed, received, accepted, overdue, waived, total))| {
                    DeliverableReportTotalsDto {
                        key,
                        agreed,
                        received,
                        accepted,
                        overdue,
                        waived,
                        total,
                    }
                },
            )
            .collect()
    };
    Ok(Json(DeliverablesReportResponse {
        projects,
        totals_by_year: totals(by_year),
        totals_by_organisation: totals(by_org),
    }))
}

// ---------------------------------------------------------------------------
// GET /reports/measurements/variables and /reports/measurements
// ---------------------------------------------------------------------------

#[derive(FromRow)]
struct VariableRow {
    variable_key: String,
    unit: String,
    count: i64,
    projects: i64,
}

async fn report_measurement_variables(
    State(state): State<AppState>,
    actor: Actor,
) -> AppResult<Json<ListResponse<MeasurementVariableDto>>> {
    authz::require_role(&actor, &authz::STAFF_ROLES)?;
    let rows: Vec<VariableRow> = sqlx::query_as(
        "SELECT variable_key, unit, COUNT(*) AS count, COUNT(DISTINCT project_id) AS projects
         FROM measurements GROUP BY variable_key, unit
         ORDER BY variable_key, unit",
    )
    .fetch_all(&state.pool)
    .await?;
    let total = rows.len() as i64;
    let items = rows
        .into_iter()
        .map(|r| MeasurementVariableDto {
            variable_key: r.variable_key,
            unit: r.unit,
            count: r.count,
            projects: r.projects,
        })
        .collect();
    Ok(Json(ListResponse { items, total }))
}

#[derive(Deserialize)]
struct MeasurementsQuery {
    variable_key: Option<String>,
}

#[derive(FromRow)]
struct MeasurementRow {
    project_id: String,
    reference: Option<String>,
    title: String,
    site_name: String,
    observed_on: String,
    value: f64,
    unit: String,
    source_label: String,
}

async fn report_measurements(
    State(state): State<AppState>,
    actor: Actor,
    Query(query): Query<MeasurementsQuery>,
) -> AppResult<Json<MeasurementsReportResponse>> {
    authz::require_role(&actor, &authz::STAFF_ROLES)?;
    let variable_key = query.variable_key.unwrap_or_default();
    let mut errors = FieldErrors::new();
    errors.require("variable_key", &variable_key, "variable_key is required");
    errors.finish()?;

    let rows: Vec<MeasurementRow> = sqlx::query_as(
        "SELECT m.project_id, p.reference, p.title, m.site_name, m.observed_on,
                m.value, m.unit, m.source_label
         FROM measurements m JOIN projects p ON p.id = m.project_id
         WHERE m.variable_key = ?
         ORDER BY m.unit, m.observed_on, m.site_name",
    )
    .bind(&variable_key)
    .fetch_all(&state.pool)
    .await?;

    // One series per unit — charts never mix units (§4).
    let mut series: Vec<MeasurementSeriesDto> = Vec::new();
    for r in rows {
        let point = MeasurementPointDto {
            project_id: r.project_id,
            project_reference: r.reference,
            project_title: r.title,
            site: r.site_name,
            observed_on: r.observed_on,
            value: r.value,
            unit: r.unit.clone(),
            source_label: r.source_label,
        };
        match series.iter_mut().find(|s| s.unit == r.unit) {
            Some(s) => s.points.push(point),
            None => series.push(MeasurementSeriesDto {
                unit: r.unit,
                points: vec![point],
            }),
        }
    }
    Ok(Json(MeasurementsReportResponse {
        variable_key,
        series,
    }))
}
