//! Public catalog — no auth (architecture §5 "public catalog").
//!
//! Only projects with at least one deliverable published at `metadata` or
//! `metadata_and_files`, plus `legacy` projects, are listed — never withdrawn
//! ones. Files download only through `publication_files` of
//! `metadata_and_files` deliverables once the embargo has passed; every
//! denial is a plain 404 so existence is never revealed. Nothing here may
//! include personal documents, internal threads, expert opinions, invoices,
//! application answers or audit events.

use axum::extract::{Path, Query, State};
use axum::http::{HeaderValue, Response, StatusCode, header};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{FromRow, SqlitePool};

use crate::AppState;
use crate::deliverables::require_catalog_enabled;
use crate::dto::{
    ListResponse, PublicDeliverableDto, PublicFileDto, PublicProjectDto, PublicSiteDto,
};
use crate::error::{AppError, AppResult};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/public/projects", get(list_projects))
        .route("/public/projects/{reference}", get(get_project))
        .route("/public/files/{id}/download", get(download_file))
}

#[derive(Deserialize)]
pub struct CatalogQuery {
    pub q: Option<String>,
    pub year: Option<i64>,
    pub bbox: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(FromRow)]
#[allow(dead_code)]
struct CatalogProjectRow {
    id: String,
    reference: Option<String>,
    title: String,
    organisation: String,
    start_date: Option<String>,
    summary: String,
    keywords: String,
}

#[derive(FromRow)]
struct PublicDeliverableRow {
    id: String,
    title: String,
    description: String,
    kind: String,
    publish_level: String,
    published_at: Option<String>,
    embargo_until: Option<String>,
}

/// Project visibility rule for the catalog (§5): published deliverable OR
/// legacy, never withdrawn.
const VISIBLE: &str = "p.status != 'withdrawn'
     AND (p.legacy = 1 OR EXISTS (
        SELECT 1 FROM deliverables d
        WHERE d.project_id = p.id AND d.publish_level IN ('metadata','metadata_and_files')))";

fn parse_bbox(bbox: &str) -> Option<(f64, f64, f64, f64)> {
    let parts: Vec<&str> = bbox.split(',').collect();
    if parts.len() != 4 {
        return None;
    }
    let v: Option<Vec<f64>> = parts.iter().map(|p| p.trim().parse::<f64>().ok()).collect();
    let v = v?;
    Some((v[0], v[1], v[2], v[3]))
}

/// Snap a bbox outward to the 0.1° grid (§4): the bounds the public sees
/// for a sensitive site. `[min_lat, min_lng, max_lat, max_lng]`.
fn generalized_bounds(min_lat: f64, min_lng: f64, max_lat: f64, max_lng: f64) -> [f64; 4] {
    let f = |x: f64| (x * 10.0).floor() / 10.0;
    let c = |x: f64| (x * 10.0).ceil() / 10.0;
    [f(min_lat), f(min_lng), c(max_lat), c(max_lng)]
}

/// A site generalized to its 0.1°-grid bbox (§4) — the geometry the public
/// and any viewer without precise-location rights sees.
fn generalized_geometry(min_lat: f64, min_lng: f64, max_lat: f64, max_lng: f64) -> Value {
    let [min_lat, min_lng, max_lat, max_lng] =
        generalized_bounds(min_lat, min_lng, max_lat, max_lng);
    json!({
        "type": "Polygon",
        "coordinates": [[
            [min_lng, min_lat],
            [max_lng, min_lat],
            [max_lng, max_lat],
            [min_lng, max_lat],
            [min_lng, min_lat],
        ]],
    })
}

/// Published files of a deliverable: listed in `publication_files`, a
/// `result` document of the deliverable's own project, scan `clean`. The
/// public listing and the public download use the same rule.
const PUBLIC_FILE_JOINS: &str = "FROM publication_files pf
     JOIN deliverables del ON del.id = pf.deliverable_id
     JOIN document_versions dv ON dv.id = pf.document_version_id
     JOIN documents d ON d.id = dv.document_id AND d.project_id = del.project_id
     JOIN files f ON f.id = dv.file_id
     WHERE d.category = 'result' AND f.scan_status = 'clean'";

/// Ids of visible projects with a site intersecting the query bbox, where
/// sensitive sites match by their generalized bounds only — exactly the
/// geometry the response shows, so repeated narrowing reveals nothing more.
async fn projects_in_bbox(
    pool: &SqlitePool,
    (min_lng, min_lat, max_lng, max_lat): (f64, f64, f64, f64),
) -> AppResult<Vec<String>> {
    let sites: Vec<(String, f64, f64, f64, f64, i64)> = sqlx::query_as(&format!(
        "SELECT s.project_id, s.min_lat, s.min_lng, s.max_lat, s.max_lng, s.sensitive
         FROM project_sites s JOIN projects p ON p.id = s.project_id WHERE {VISIBLE}"
    ))
    .fetch_all(pool)
    .await?;
    let mut ids: Vec<String> = sites
        .into_iter()
        .filter_map(
            |(project_id, s_min_lat, s_min_lng, s_max_lat, s_max_lng, sensitive)| {
                let [s_min_lat, s_min_lng, s_max_lat, s_max_lng] = if sensitive != 0 {
                    generalized_bounds(s_min_lat, s_min_lng, s_max_lat, s_max_lng)
                } else {
                    [s_min_lat, s_min_lng, s_max_lat, s_max_lng]
                };
                (s_min_lng <= max_lng
                    && s_max_lng >= min_lng
                    && s_min_lat <= max_lat
                    && s_max_lat >= min_lat)
                    .then_some(project_id)
            },
        )
        .collect();
    ids.sort();
    ids.dedup();
    Ok(ids)
}

async fn load_public_project(
    pool: &SqlitePool,
    row: CatalogProjectRow,
) -> AppResult<PublicProjectDto> {
    // Sites: precise geometry for non-sensitive sites only; sensitive sites
    // are replaced by their 0.1°-grid generalized bbox (§4, §5).
    let sites: Vec<PublicSiteDto> = sqlx::query_as::<_, (String, String, f64, f64, f64, f64, i64)>(
        "SELECT name, geometry_json, min_lat, min_lng, max_lat, max_lng, sensitive
         FROM project_sites WHERE project_id = ? ORDER BY name",
    )
    .bind(&row.id)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(
        |(name, geometry_json, min_lat, min_lng, max_lat, max_lng, sensitive)| {
            let (geometry, generalized) = if sensitive != 0 {
                (
                    generalized_geometry(min_lat, min_lng, max_lat, max_lng),
                    true,
                )
            } else {
                (
                    serde_json::from_str(&geometry_json).unwrap_or(Value::Null),
                    false,
                )
            };
            PublicSiteDto {
                name,
                geometry,
                generalized,
                sensitive: sensitive != 0,
            }
        },
    )
    .collect();

    let today = crate::deliverables::today();
    let drows: Vec<PublicDeliverableRow> = sqlx::query_as(
        "SELECT id, title, description, kind, publish_level, published_at, embargo_until
             FROM deliverables
             WHERE project_id = ? AND publish_level IN ('metadata','metadata_and_files')
             ORDER BY published_at",
    )
    .bind(&row.id)
    .fetch_all(pool)
    .await?;
    let mut deliverables = Vec::new();
    for d in drows {
        // Before the embargo date metadata shows "files available from
        // <date>" and the file list stays empty (§4).
        let embargo_open = d.publish_level == "metadata_and_files"
            && d.embargo_until
                .as_deref()
                .map(|e| e <= today.as_str())
                .unwrap_or(true);
        let files_available_from = if d.publish_level == "metadata_and_files" && !embargo_open {
            d.embargo_until.clone()
        } else {
            None
        };
        let files = if embargo_open {
            sqlx::query_as::<_, (String, String, i64, String)>(&format!(
                "SELECT pf.document_version_id, d.title, f.size, f.mime
                 {PUBLIC_FILE_JOINS} AND pf.deliverable_id = ?
                 ORDER BY d.title"
            ))
            .bind(&d.id)
            .fetch_all(pool)
            .await?
            .into_iter()
            .map(|(document_version_id, title, size, mime)| PublicFileDto {
                document_version_id,
                title,
                size,
                mime,
            })
            .collect()
        } else {
            Vec::new()
        };
        deliverables.push(PublicDeliverableDto {
            id: d.id,
            title: d.title,
            description: d.description,
            kind: d.kind,
            publish_level: d.publish_level,
            published_at: d.published_at,
            files_available_from,
            files,
        });
    }

    let year = row
        .start_date
        .as_deref()
        .and_then(|d| d.get(0..4))
        .and_then(|y| y.parse::<i64>().ok());

    Ok(PublicProjectDto {
        reference: row.reference,
        title: row.title,
        organisation: row.organisation,
        year,
        summary: row.summary,
        keywords: row.keywords,
        sites,
        deliverables,
    })
}

async fn list_projects(
    State(state): State<AppState>,
    Query(query): Query<CatalogQuery>,
) -> AppResult<impl IntoResponse> {
    require_catalog_enabled(&state.pool).await?;
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    let offset = query.offset.unwrap_or(0).max(0);
    let bbox = match &query.bbox {
        Some(b) => {
            let Some(v) = parse_bbox(b) else {
                return Err(AppError::BadRequest(
                    "bbox must be minLng,minLat,maxLng,maxLat".into(),
                ));
            };
            Some(v)
        }
        None => None,
    };

    let mut sql = format!(
        "SELECT p.id, p.reference, p.title, p.organisation, p.start_date, p.summary, p.keywords
         FROM projects p WHERE {VISIBLE}"
    );
    let mut count_sql = format!("SELECT COUNT(*) FROM projects p WHERE {VISIBLE}");
    let mut binds: Vec<String> = Vec::new();

    if let Some(q) = query.q.as_deref().map(str::trim).filter(|q| !q.is_empty()) {
        // Phrase-match the input on the FTS index; quotes are stripped so
        // user input can never break FTS syntax.
        let phrase = format!("\"{}\"", q.replace('"', " "));
        sql.push_str(" AND p.rowid IN (SELECT rowid FROM projects_fts WHERE projects_fts MATCH ?)");
        count_sql.push_str(
            " AND p.rowid IN (SELECT rowid FROM projects_fts WHERE projects_fts MATCH ?)",
        );
        binds.push(phrase);
    }
    if let Some(year) = query.year {
        sql.push_str(" AND substr(p.start_date, 1, 4) = ?");
        count_sql.push_str(" AND substr(p.start_date, 1, 4) = ?");
        binds.push(year.to_string());
    }
    if let Some(bbox) = bbox {
        let ids = projects_in_bbox(&state.pool, bbox).await?;
        let clause = " AND p.id IN (SELECT value FROM json_each(?))";
        sql.push_str(clause);
        count_sql.push_str(clause);
        binds.push(Value::from(ids).to_string());
    }

    let mut count_q = sqlx::query_scalar::<_, i64>(&count_sql);
    for b in &binds {
        count_q = count_q.bind(b);
    }
    let total = count_q.fetch_one(&state.pool).await?;

    sql.push_str(" ORDER BY p.reference DESC LIMIT ? OFFSET ?");
    let mut rows_q = sqlx::query_as::<_, CatalogProjectRow>(&sql);
    for b in &binds {
        rows_q = rows_q.bind(b);
    }
    let rows: Vec<CatalogProjectRow> = rows_q
        .bind(limit)
        .bind(offset)
        .fetch_all(&state.pool)
        .await?;

    let mut items = Vec::new();
    for row in rows {
        items.push(load_public_project(&state.pool, row).await?);
    }
    Ok(Json(ListResponse { items, total }))
}

async fn get_project(
    State(state): State<AppState>,
    Path(reference): Path<String>,
) -> AppResult<impl IntoResponse> {
    require_catalog_enabled(&state.pool).await?;
    let row: Option<CatalogProjectRow> = sqlx::query_as(&format!(
        "SELECT p.id, p.reference, p.title, p.organisation, p.start_date, p.summary, p.keywords
         FROM projects p WHERE {VISIBLE} AND p.reference = ?"
    ))
    .bind(&reference)
    .fetch_optional(&state.pool)
    .await?;
    let row = row.ok_or(AppError::NotFound)?;
    Ok(Json(load_public_project(&state.pool, row).await?))
}

async fn download_file(
    State(state): State<AppState>,
    Path(document_version_id): Path<String>,
) -> AppResult<impl IntoResponse> {
    require_catalog_enabled(&state.pool).await?;
    let today = crate::deliverables::today();
    // Only `result` documents of the deliverable's own project, scan-clean,
    // listed in publication_files of metadata_and_files deliverables whose
    // embargo passed, on non-withdrawn projects. Anything else is a plain
    // 404 — existence is never revealed (§5).
    let row: Option<(String, String, i64, String)> = sqlx::query_as(&format!(
        "SELECT d.title, f.sha256, f.size, f.mime
         {PUBLIC_FILE_JOINS}
           AND pf.document_version_id = ?
           AND del.publish_level = 'metadata_and_files'
           AND (del.embargo_until IS NULL OR del.embargo_until <= ?)
           AND EXISTS (SELECT 1 FROM projects p
                       WHERE p.id = del.project_id AND p.status != 'withdrawn')"
    ))
    .bind(&document_version_id)
    .bind(&today)
    .fetch_optional(&state.pool)
    .await?;
    let Some((title, sha256, _size, mime)) = row else {
        return Err(AppError::NotFound);
    };

    let (body, len) = crate::files::stream_file(&state.config.data_dir, &sha256).await?;
    let filename: String = title
        .chars()
        .filter(|c| c.is_ascii())
        .map(|c| {
            if matches!(c, '"' | '\\' | '/') {
                '_'
            } else {
                c
            }
        })
        .collect();
    let filename = if filename.is_empty() {
        "download".to_string()
    } else {
        filename
    };

    let mut resp = Response::new(body);
    resp.headers_mut()
        .insert(header::CONTENT_LENGTH, HeaderValue::from(len));
    *resp.status_mut() = StatusCode::OK;
    resp.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(&mime).map_err(AppError::internal)?,
    );
    resp.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("attachment; filename=\"{filename}\""))
            .map_err(AppError::internal)?,
    );
    resp.headers_mut().insert(
        header::HeaderName::from_static("x-content-type-options"),
        HeaderValue::from_static("nosniff"),
    );
    resp.headers_mut().insert(
        header::HeaderName::from_static("cache-control"),
        HeaderValue::from_static("public, max-age=3600"),
    );
    Ok(resp)
}
