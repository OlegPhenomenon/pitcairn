//! Research sites (architecture §4 `project_sites`, §5).
//!
//! Sensitive-site generalization lives here and is reused by every read path
//! (workspace payload, revision snapshots, decision site snapshots, GeoJSON
//! export, and the public catalog): viewers without precise-location rights
//! see the site's bbox expanded outward to a 0.1° grid, flagged
//! `generalized: true`.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, patch};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::FromRow;

use crate::AppState;
use crate::audit::{self, AuditEvent};
use crate::authz::{self, Actor, ProjectAccess};
use crate::db;
use crate::dto::ListResponse;
use crate::dto::a::{PatchSiteRequest, SiteDto, SiteRequest, SiteSearchItemDto};
use crate::error::{AppError, AppResult};
use crate::util::{new_id, now_rfc3339};
use crate::validation::FieldErrors;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/projects/{id}/sites", get(list_sites).post(create_site))
        .route("/projects/{id}/sites.geojson", get(sites_geojson))
        .route("/sites/search", get(search_sites))
        .route("/sites/{id}", patch(update_site).delete(delete_site))
}

// ---------------------------------------------------------------------------
// Geometry + generalization helpers (pub — reused by workspace, revision and
// decision snapshots, and the public catalog)
// ---------------------------------------------------------------------------

/// Snap outward to the 0.1° grid: mins floor, maxes ceil (§4 generalization).
pub fn snapped_bbox(
    min_lat: f64,
    min_lng: f64,
    max_lat: f64,
    max_lng: f64,
) -> (f64, f64, f64, f64) {
    let floor1 = |v: f64| ((v * 10.0).floor() / 10.0 * 1e6).round() / 1e6;
    let ceil1 = |v: f64| ((v * 10.0).ceil() / 10.0 * 1e6).round() / 1e6;
    let (min_lat, min_lng, mut max_lat, mut max_lng) = (
        floor1(min_lat),
        floor1(min_lng),
        ceil1(max_lat),
        ceil1(max_lng),
    );
    // A point exactly on the grid would collapse to a zero-size cell.
    if max_lat <= min_lat {
        max_lat = ((min_lat + 0.1) * 1e6).round() / 1e6;
    }
    if max_lng <= min_lng {
        max_lng = ((min_lng + 0.1) * 1e6).round() / 1e6;
    }
    (min_lat, min_lng, max_lat, max_lng)
}

/// The generalized replacement geometry: a Polygon covering the site's bbox
/// snapped outward to the 0.1° grid.
pub fn generalized_geometry(min_lat: f64, min_lng: f64, max_lat: f64, max_lng: f64) -> Value {
    let (min_lat, min_lng, max_lat, max_lng) = snapped_bbox(min_lat, min_lng, max_lat, max_lng);
    json!({
        "type": "Polygon",
        "coordinates": [[
            [min_lng, min_lat],
            [max_lng, min_lat],
            [max_lng, max_lat],
            [min_lng, max_lat],
            [min_lng, min_lat]
        ]]
    })
}

/// Compute `(min_lat, min_lng, max_lat, max_lng)` for a GeoJSON Point or
/// Polygon. Returns None for other/invalid shapes.
pub fn geometry_bbox(geometry: &Value) -> Option<(f64, f64, f64, f64)> {
    let gtype = geometry.get("type").and_then(|t| t.as_str())?;
    let mut min_lat = f64::INFINITY;
    let mut min_lng = f64::INFINITY;
    let mut max_lat = f64::NEG_INFINITY;
    let mut max_lng = f64::NEG_INFINITY;
    let mut push = |coord: &Value| {
        let pair = coord.as_array()?;
        if pair.len() < 2 {
            return None;
        }
        let lng = pair[0].as_f64()?;
        let lat = pair[1].as_f64()?;
        min_lng = min_lng.min(lng);
        min_lat = min_lat.min(lat);
        max_lng = max_lng.max(lng);
        max_lat = max_lat.max(lat);
        Some(())
    };
    match gtype {
        "Point" => push(geometry.get("coordinates")?)?,
        "Polygon" => {
            let rings = geometry.get("coordinates")?.as_array()?;
            if rings.is_empty() {
                return None;
            }
            for ring in rings {
                let ring = ring.as_array()?;
                if ring.len() < 4 {
                    return None;
                }
                for coord in ring {
                    push(coord)?;
                }
            }
        }
        _ => return None,
    }
    if min_lat.is_infinite() {
        return None;
    }
    Some((min_lat, min_lng, max_lat, max_lng))
}

/// Field-level geometry validation → 422 `fields.geometry`.
pub fn validate_geometry(geometry: &Value) -> AppResult<(f64, f64, f64, f64)> {
    let mut errors = FieldErrors::new();
    let bbox = geometry_bbox(geometry);
    errors.check(
        "geometry",
        bbox.is_some(),
        "must be a GeoJSON Point or Polygon with [lng, lat] coordinates",
    );
    if let Some((min_lat, min_lng, max_lat, max_lng)) = bbox {
        errors.check(
            "geometry",
            (-90.0..=90.0).contains(&min_lat)
                && (-90.0..=90.0).contains(&max_lat)
                && (-180.0..=180.0).contains(&min_lng)
                && (-180.0..=180.0).contains(&max_lng),
            "coordinates out of range (lat -90..90, lng -180..180)",
        );
    }
    errors.finish()?;
    bbox.ok_or_else(|| AppError::internal("geometry bbox missing after validation"))
}

#[derive(FromRow, Clone)]
pub struct SiteRow {
    pub id: String,
    pub project_id: String,
    pub name: String,
    pub geometry_json: String,
    pub min_lat: f64,
    pub min_lng: f64,
    pub max_lat: f64,
    pub max_lng: f64,
    pub sensitive: i64,
    pub created_at: String,
}

const SITE_SELECT: &str =
    "SELECT id, project_id, name, geometry_json, min_lat, min_lng, max_lat, max_lng,
            sensitive, created_at FROM project_sites";

/// All sites of a project, oldest first.
pub async fn project_sites<'e, E>(exec: E, project_id: &str) -> AppResult<Vec<SiteRow>>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    Ok(sqlx::query_as(&format!(
        "{SITE_SELECT} WHERE project_id = ? ORDER BY created_at, id"
    ))
    .bind(project_id)
    .fetch_all(exec)
    .await?)
}

/// Build the API view of a site: sensitive geometries are replaced by the
/// generalized bbox polygon unless `precise` (`authz::can_see_precise_location`).
/// The bbox numbers are generalized too, so nothing precise leaks.
pub fn site_dto(row: &SiteRow, precise: bool) -> AppResult<SiteDto> {
    let mut value = site_snapshot_value(row)?;
    generalize_site_value(&mut value, precise);
    let num = |k: &str| value.get(k).and_then(Value::as_f64).unwrap_or_default();
    Ok(SiteDto {
        id: row.id.clone(),
        project_id: row.project_id.clone(),
        name: row.name.clone(),
        geometry: value["geometry"].clone(),
        sensitive: row.sensitive != 0,
        generalized: value
            .get("generalized")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        min_lat: num("min_lat"),
        min_lng: num("min_lng"),
        max_lat: num("max_lat"),
        max_lng: num("max_lng"),
        created_at: row.created_at.clone(),
    })
}

/// Self-contained JSON copy of a site (precise) for revision snapshots and
/// decision `sites_snapshot_json`. Pass through [`generalize_site_value`]
/// before showing it to a viewer.
pub fn site_snapshot_value(row: &SiteRow) -> AppResult<Value> {
    let geometry: Value = serde_json::from_str(&row.geometry_json).map_err(AppError::internal)?;
    Ok(json!({
        "id": row.id,
        "name": row.name,
        "geometry": geometry,
        "sensitive": row.sensitive != 0,
        "min_lat": row.min_lat,
        "min_lng": row.min_lng,
        "max_lat": row.max_lat,
        "max_lng": row.max_lng,
        "generalized": false,
    }))
}

/// THE generalization function: replace a sensitive site's geometry (and
/// bbox) by its bbox snapped outward to the 0.1° grid when the viewer lacks
/// precise rights. Used for API site lists, revision snapshots, decision
/// snapshots, GeoJSON export and the public catalog.
pub fn generalize_site_value(site: &mut Value, precise: bool) {
    if precise {
        return;
    }
    let sensitive = site
        .get("sensitive")
        .map(|s| s.as_bool().unwrap_or(s.as_i64() == Some(1)))
        .unwrap_or(false);
    if !sensitive {
        return;
    }
    let stored = [
        site.get("min_lat").and_then(Value::as_f64),
        site.get("min_lng").and_then(Value::as_f64),
        site.get("max_lat").and_then(Value::as_f64),
        site.get("max_lng").and_then(Value::as_f64),
    ];
    let bbox = match stored {
        [Some(a), Some(b), Some(c), Some(d)] => Some((a, b, c, d)),
        _ => site.get("geometry").and_then(geometry_bbox),
    };
    match bbox {
        Some((min_lat, min_lng, max_lat, max_lng)) => {
            let (a, b, c, d) = snapped_bbox(min_lat, min_lng, max_lat, max_lng);
            site["geometry"] = generalized_geometry(min_lat, min_lng, max_lat, max_lng);
            site["min_lat"] = json!(a);
            site["min_lng"] = json!(b);
            site["max_lat"] = json!(c);
            site["max_lng"] = json!(d);
        }
        // Unreadable geometry: never leak it.
        None => site["geometry"] = Value::Null,
    }
    site["generalized"] = json!(true);
}

/// Generalize every entry of a JSON site list in place.
pub fn generalize_site_list(sites: &mut Value, precise: bool) {
    if let Some(list) = sites.as_array_mut() {
        for site in list {
            generalize_site_value(site, precise);
        }
    }
}

/// Is the project visible in the public catalog (§5 public catalog: at least
/// one published deliverable, or a legacy project; never withdrawn)?
/// Viewers without project access may then see its GENERALIZED sites.
pub async fn project_is_public<'e, E>(exec: E, project_id: &str) -> AppResult<bool>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    let public: i64 = sqlx::query_scalar(
        "SELECT EXISTS (
            SELECT 1 FROM projects p
            WHERE p.id = ? AND p.status != 'withdrawn'
              AND (p.legacy = 1 OR EXISTS (
                    SELECT 1 FROM deliverables d
                    WHERE d.project_id = p.id AND d.publish_level != 'none'
                      AND d.published_at IS NOT NULL)))",
    )
    .bind(project_id)
    .fetch_one(exec)
    .await?;
    Ok(public != 0)
}

/// How may `actor` read the sites of a project? `Ok(true)` precise (team,
/// staff, assigned expert), `Ok(false)` generalized only (project is in the
/// public catalog), else 403.
pub async fn site_read_precision(
    state: &AppState,
    actor: &Actor,
    project_id: &str,
) -> AppResult<bool> {
    let exists: Option<String> = sqlx::query_scalar("SELECT id FROM projects WHERE id = ?")
        .bind(project_id)
        .fetch_optional(&state.pool)
        .await?;
    exists.ok_or(AppError::NotFound)?;
    let access = authz::project_access(&state.pool, actor, project_id).await?;
    if authz::can_see_precise_location(access) {
        return Ok(true);
    }
    if project_is_public(&state.pool, project_id).await? {
        return Ok(false);
    }
    Err(AppError::forbidden(
        "you do not have access to this project",
    ))
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// Write access: team editor+ while the project is editable
/// (draft | changes_requested) — sites are part of the application.
async fn require_site_write(state: &AppState, actor: &Actor, project_id: &str) -> AppResult<()> {
    let access = authz::project_access(&state.pool, actor, project_id).await?;
    if !matches!(access, ProjectAccess::TeamEditor | ProjectAccess::TeamLead) {
        return Err(AppError::forbidden(
            "editing sites requires a team editor or lead role",
        ));
    }
    let status: Option<String> = sqlx::query_scalar("SELECT status FROM projects WHERE id = ?")
        .bind(project_id)
        .fetch_optional(&state.pool)
        .await?;
    let status = status.ok_or(AppError::NotFound)?;
    if !matches!(status.as_str(), "draft" | "changes_requested") {
        return Err(AppError::conflict(
            "not_editable",
            "sites can only be edited while the application is a draft or changes were requested",
        ));
    }
    Ok(())
}

async fn list_sites(
    State(state): State<AppState>,
    actor: Actor,
    Path(project_id): Path<String>,
) -> AppResult<Json<ListResponse<SiteDto>>> {
    let precise = site_read_precision(&state, &actor, &project_id).await?;
    let items = project_sites(&state.pool, &project_id)
        .await?
        .iter()
        .map(|r| site_dto(r, precise))
        .collect::<AppResult<Vec<_>>>()?;
    let total = items.len() as i64;
    Ok(Json(ListResponse { items, total }))
}

async fn sites_geojson(
    State(state): State<AppState>,
    actor: Actor,
    Path(project_id): Path<String>,
) -> AppResult<impl IntoResponse> {
    let precise = site_read_precision(&state, &actor, &project_id).await?;
    let mut features = Vec::new();
    for row in project_sites(&state.pool, &project_id).await? {
        let dto = site_dto(&row, precise)?;
        features.push(json!({
            "type": "Feature",
            "id": dto.id,
            "geometry": dto.geometry,
            "properties": {
                "name": dto.name,
                "sensitive": dto.sensitive,
                "generalized": dto.generalized,
                "project_id": dto.project_id,
            }
        }));
    }
    Ok((
        [
            (
                axum::http::header::CONTENT_TYPE,
                "application/geo+json; charset=utf-8",
            ),
            (axum::http::header::CACHE_CONTROL, "private, no-store"),
        ],
        Json(json!({"type": "FeatureCollection", "features": features})),
    ))
}

fn validate_site_name(errors: &mut FieldErrors, name: &str) {
    errors.require("name", name, "name is required");
    errors.max_len("name", name, 300);
}

async fn create_site(
    State(state): State<AppState>,
    actor: Actor,
    Path(project_id): Path<String>,
    Json(req): Json<SiteRequest>,
) -> AppResult<impl IntoResponse> {
    require_site_write(&state, &actor, &project_id).await?;

    let mut errors = FieldErrors::new();
    validate_site_name(&mut errors, &req.name);
    errors.finish()?;
    let bbox = validate_geometry(&req.geometry)?;
    let sensitive = req.sensitive.unwrap_or(false);

    let mut tx = db::begin_immediate(&state.pool).await?;
    let id = new_id();
    sqlx::query(
        "INSERT INTO project_sites
         (id, project_id, name, geometry_json, min_lat, min_lng, max_lat, max_lng, sensitive, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&project_id)
    .bind(req.name.trim())
    .bind(req.geometry.to_string())
    .bind(bbox.0)
    .bind(bbox.1)
    .bind(bbox.2)
    .bind(bbox.3)
    .bind(sensitive as i64)
    .bind(now_rfc3339())
    .execute(&mut *tx)
    .await?;

    audit::record(
        &mut tx,
        AuditEvent {
            actor_id: Some(actor.user_id.clone()),
            actor_label: actor.name.clone(),
            action: "site.created".into(),
            entity_type: "project_site".into(),
            entity_id: id.clone(),
            project_id: Some(project_id.clone()),
            visibility: "shared".into(),
            summary: format!("{} added site {}", actor.name, req.name.trim()),
            before: None,
            after: Some(json!({"name": req.name.trim(), "sensitive": sensitive})),
            reason: None,
        },
    )
    .await?;
    tx.commit().await?;

    let row = load_site(&state, &id).await?;
    Ok((StatusCode::CREATED, Json(site_dto(&row, true)?)))
}

async fn load_site(state: &AppState, site_id: &str) -> AppResult<SiteRow> {
    let row: Option<SiteRow> = sqlx::query_as(&format!("{SITE_SELECT} WHERE id = ?"))
        .bind(site_id)
        .fetch_optional(&state.pool)
        .await?;
    row.ok_or(AppError::NotFound)
}

async fn update_site(
    State(state): State<AppState>,
    actor: Actor,
    Path(site_id): Path<String>,
    Json(req): Json<PatchSiteRequest>,
) -> AppResult<Json<SiteDto>> {
    let row = load_site(&state, &site_id).await?;
    require_site_write(&state, &actor, &row.project_id).await?;

    let mut errors = FieldErrors::new();
    if let Some(name) = &req.name {
        validate_site_name(&mut errors, name);
    }
    errors.finish()?;
    let bbox = match &req.geometry {
        Some(g) => Some(validate_geometry(g)?),
        None => None,
    };

    let mut tx = db::begin_immediate(&state.pool).await?;
    let name = req.name.as_deref().map(str::trim).unwrap_or(&row.name);
    let geometry_json = req
        .geometry
        .as_ref()
        .map(Value::to_string)
        .unwrap_or_else(|| row.geometry_json.clone());
    let (min_lat, min_lng, max_lat, max_lng) =
        bbox.unwrap_or((row.min_lat, row.min_lng, row.max_lat, row.max_lng));
    let sensitive = req.sensitive.unwrap_or(row.sensitive != 0);
    sqlx::query(
        "UPDATE project_sites SET name = ?, geometry_json = ?, min_lat = ?, min_lng = ?,
                max_lat = ?, max_lng = ?, sensitive = ? WHERE id = ?",
    )
    .bind(name)
    .bind(&geometry_json)
    .bind(min_lat)
    .bind(min_lng)
    .bind(max_lat)
    .bind(max_lng)
    .bind(sensitive as i64)
    .bind(&site_id)
    .execute(&mut *tx)
    .await?;

    audit::record(
        &mut tx,
        AuditEvent {
            actor_id: Some(actor.user_id.clone()),
            actor_label: actor.name.clone(),
            action: "site.updated".into(),
            entity_type: "project_site".into(),
            entity_id: site_id.clone(),
            project_id: Some(row.project_id.clone()),
            visibility: "shared".into(),
            summary: format!("{} updated site {}", actor.name, name),
            before: Some(json!({"name": row.name, "sensitive": row.sensitive != 0})),
            after: Some(json!({"name": name, "sensitive": sensitive})),
            reason: None,
        },
    )
    .await?;
    tx.commit().await?;

    let row = load_site(&state, &site_id).await?;
    Ok(Json(site_dto(&row, true)?))
}

async fn delete_site(
    State(state): State<AppState>,
    actor: Actor,
    Path(site_id): Path<String>,
) -> AppResult<StatusCode> {
    let row = load_site(&state, &site_id).await?;
    require_site_write(&state, &actor, &row.project_id).await?;

    let mut tx = db::begin_immediate(&state.pool).await?;
    let in_use: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM samples WHERE site_id = ?")
        .bind(&site_id)
        .fetch_one(&mut *tx)
        .await?;
    if in_use > 0 {
        return Err(AppError::conflict(
            "site_in_use",
            "the site is referenced by samples",
        ));
    }
    sqlx::query("DELETE FROM project_sites WHERE id = ?")
        .bind(&site_id)
        .execute(&mut *tx)
        .await?;

    audit::record(
        &mut tx,
        AuditEvent {
            actor_id: Some(actor.user_id.clone()),
            actor_label: actor.name.clone(),
            action: "site.deleted".into(),
            entity_type: "project_site".into(),
            entity_id: site_id.clone(),
            project_id: Some(row.project_id.clone()),
            visibility: "shared".into(),
            summary: format!("{} removed site {}", actor.name, row.name),
            before: Some(json!({"name": row.name})),
            after: None,
            reason: None,
        },
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct SiteSearchQuery {
    /// "minLng,minLat,maxLng,maxLat"
    pub bbox: Option<String>,
}

/// Parse `minLng,minLat,maxLng,maxLat` → `(min_lng, min_lat, max_lng, max_lat)`.
pub fn parse_bbox(raw: &str) -> AppResult<(f64, f64, f64, f64)> {
    let parts: Vec<f64> = raw
        .split(',')
        .map(|p| p.trim().parse::<f64>())
        .collect::<Result<_, _>>()
        .map_err(|_| bad_bbox())?;
    let [min_lng, min_lat, max_lng, max_lat] = parts[..] else {
        return Err(bad_bbox());
    };
    if !(min_lng <= max_lng && min_lat <= max_lat) {
        return Err(bad_bbox());
    }
    Ok((min_lng, min_lat, max_lng, max_lat))
}

fn bad_bbox() -> AppError {
    let mut fields = std::collections::HashMap::new();
    fields.insert(
        "bbox".to_string(),
        "must be minLng,minLat,maxLng,maxLat with min <= max".to_string(),
    );
    AppError::Validation { fields }
}

/// `GET /sites/search?bbox=` — staff area search (bbox intersection).
async fn search_sites(
    State(state): State<AppState>,
    actor: Actor,
    Query(query): Query<SiteSearchQuery>,
) -> AppResult<Json<ListResponse<SiteSearchItemDto>>> {
    if !actor.is_staff() {
        return Err(AppError::forbidden("site search requires a staff role"));
    }
    let (min_lng, min_lat, max_lng, max_lat) = parse_bbox(query.bbox.as_deref().unwrap_or(""))?;

    #[derive(FromRow)]
    struct SearchRow {
        #[sqlx(flatten)]
        site: SiteRow,
        reference: Option<String>,
        title: String,
        status: String,
    }
    let rows: Vec<SearchRow> = sqlx::query_as(
        "SELECT s.id, s.project_id, s.name, s.geometry_json, s.min_lat, s.min_lng,
                s.max_lat, s.max_lng, s.sensitive, s.created_at,
                p.reference, p.title, p.status
         FROM project_sites s JOIN projects p ON p.id = s.project_id
         WHERE s.min_lng <= ? AND s.max_lng >= ? AND s.min_lat <= ? AND s.max_lat >= ?
         ORDER BY s.created_at DESC",
    )
    .bind(max_lng)
    .bind(min_lng)
    .bind(max_lat)
    .bind(min_lat)
    .fetch_all(&state.pool)
    .await?;

    // Staff always have precise-location rights.
    let items = rows
        .into_iter()
        .map(|row| {
            Ok(SiteSearchItemDto {
                site: site_dto(&row.site, true)?,
                project_reference: row.reference,
                project_title: row.title,
                project_status: row.status,
            })
        })
        .collect::<AppResult<Vec<_>>>()?;
    let total = items.len() as i64;
    Ok(Json(ListResponse { items, total }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generalization_snaps_outward_and_hides_precise_point() {
        let mut site = json!({
            "geometry": {"type": "Point", "coordinates": [-130.1043, -25.0661]},
            "sensitive": true,
            "min_lat": -25.0661, "min_lng": -130.1043, "max_lat": -25.0661, "max_lng": -130.1043,
        });
        generalize_site_value(&mut site, false);
        assert_eq!(site["generalized"], json!(true));
        assert_eq!(site["geometry"]["type"], json!("Polygon"));
        assert_eq!(site["min_lat"], json!(-25.1));
        assert_eq!(site["max_lat"], json!(-25.0));
        assert_eq!(site["min_lng"], json!(-130.2));
        assert_eq!(site["max_lng"], json!(-130.1));
        assert!(!site.to_string().contains("25.0661"));
    }

    #[test]
    fn non_sensitive_or_precise_viewers_unchanged() {
        let original = json!({
            "geometry": {"type": "Point", "coordinates": [-130.1, -25.06]},
            "sensitive": false, "min_lat": -25.06, "min_lng": -130.1, "max_lat": -25.06, "max_lng": -130.1,
        });
        let mut site = original.clone();
        generalize_site_value(&mut site, false);
        assert_eq!(site, original);
        let mut sensitive = json!({"geometry": original["geometry"], "sensitive": true});
        let before = sensitive.clone();
        generalize_site_value(&mut sensitive, true);
        assert_eq!(sensitive, before);
    }

    #[test]
    fn bbox_parse_and_geometry_validation() {
        assert!(parse_bbox("-131,-26,-129,-24").is_ok());
        assert!(parse_bbox("1,2,3").is_err());
        assert!(parse_bbox("3,2,1,4").is_err());
        assert!(geometry_bbox(&json!({"type": "LineString", "coordinates": []})).is_none());
        let poly = json!({"type": "Polygon", "coordinates": [[[0.0, 0.0], [1.0, 0.0], [1.0, 2.0], [0.0, 0.0]]]});
        assert_eq!(geometry_bbox(&poly), Some((0.0, 0.0, 2.0, 1.0)));
    }
}
