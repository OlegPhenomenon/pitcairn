//! Historic demo data (§9): fictional projects in every state so dashboards,
//! search, map, catalog and charts have content, plus Anna's submit-ready
//! draft. Idempotent: each project is matched by title and skipped when it
//! already exists; every project is written in ONE transaction (file bytes
//! are stored first, outside the transaction).
//!
//! Projects: (a) 2023 humpback whale acoustics — closed, report published
//! with files; (b) 2024 Henderson seabird census — approved, dataset with
//! measurements accepted, one overdue deliverable; (c) 2025 reef fish biomass
//! — approved, upcoming confirmed trip, invoice partially paid; (d) 2025 coral
//! cover transects — measurements across 3 sites × 2 years, files embargoed;
//! (e) submitted (Lukas's team); (f) refused with reasons; (g) with the expert;
//! (h) changes requested; 4 legacy projects via the legacy import code path.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

use chrono::{Datelike, Duration, NaiveDate, Utc};
use serde_json::{Value, json};
use sqlx::SqlitePool;

use crate::authz::Actor;
use crate::error::{AppError, AppResult};
use crate::util::new_id;

type Tx<'a> = sqlx::Transaction<'a, sqlx::Sqlite>;

pub const WHALES: &str = "Humpback whale acoustic monitoring";
pub const SEABIRDS: &str = "Henderson Island seabird census";
pub const REEF_FISH: &str = "Reef fish biomass at Bounty Bay";
pub const CORAL_COVER: &str = "Coral cover transects";
pub const SPONGES: &str = "Deep-water sponge assemblages at Adams Seamount";
pub const DRONES: &str = "Drone mapping of seabird colonies on Oeno Island";
pub const MICROPLASTICS: &str = "Microplastics in Pitcairn coastal waters";
pub const SNAILS: &str = "Land snail survey on Pitcairn Island";
pub const ANNA_DRAFT: &str = "Coral health around Pitcairn";

/// Extra fictional researchers (not personas; they can log in with the demo
/// password but have no staff role).
const EXTRA_USERS: &[(&str, &str, &str, &str)] = &[
    (
        "mele",
        "Dr Mele Tupou",
        "mele@demo.pitcairn.invalid",
        "Coral Triangle Reef Institute (fictional)",
    ),
    (
        "erik",
        "Dr Erik Lindqvist",
        "erik@demo.pitcairn.invalid",
        "Baltic Ocean Institute (fictional)",
    ),
    (
        "sofia",
        "Dr Sofia Marquez",
        "sofia@demo.pitcairn.invalid",
        "Instituto Oceánico del Pacífico Sur (fictional)",
    ),
];

/// Legacy rows (2016–2020) committed through `legacy::commit_rows`.
const LEGACY_CSV: &str = "\
reference,title,organisation,lead_name,lead_email,start_date,end_date,summary,keywords,site_name,lat,lng,report_title,report_url
MSB-2016-004,Pitcairn rock lobster abundance,Southern Reef Fisheries Trust (fictional),Dr Grace Holloway,g.holloway@legacy.example.invalid,2016-04-04,2016-05-13,\"Pot survey of rock lobster abundance along the north coast (fictional legacy record).\",\"rock lobster, fisheries, pot survey\",Youngs Rock,-25.0575,-130.1128,Rock lobster abundance report 2016,https://archive.example.org/msb/lobster-2016.pdf
MSB-2017-009,Henderson Island beach plastics survey,Clean Ocean Futures (fictional),Dr Malik Rahman,m.rahman@legacy.example.invalid,2017-06-05,2017-06-30,\"Transect counts of beached plastic debris on Henderson East Beach (fictional legacy record).\",\"plastics, debris, beaches\",Henderson East Beach,-24.3683,-128.2986,Henderson beach plastics report,https://archive.example.org/msb/plastics-2017
MSB-2019-002,Oeno Island coral bleaching snapshot,Atoll Reef Watch (fictional),Dr Isla Brennan,i.brennan@legacy.example.invalid,2019-03-11,2019-03-29,\"Rapid bleaching survey after the 2019 marine heatwave (fictional legacy record).\",\"coral, bleaching, heatwave\",Oeno lagoon,-23.9239,-130.7342,,
MSB-2020-006,Pitcairn freshwater spring chemistry,Island Hydrology Group (fictional),Dr Paulo Sefo,p.sefo@legacy.example.invalid,2020-01-13,2020-02-07,\"Major-ion chemistry of the island's springs and catchments (fictional legacy record).\",\"hydrology, springs, water chemistry\",Middle Hill,-25.0740,-130.1018,Spring chemistry data summary,https://archive.example.org/msb/springs-2020
";

fn ds(d: NaiveDate) -> String {
    d.format("%Y-%m-%d").to_string()
}

fn ts(d: NaiveDate, hour: u32) -> String {
    format!("{}T{hour:02}:00:00Z", ds(d))
}

fn date(s: &str) -> NaiveDate {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").expect("valid seed date")
}

struct Ctx {
    pool: SqlitePool,
    data_dir: PathBuf,
    today: NaiveDate,
    template_version_id: String,
    users: HashMap<String, (String, String)>, // key -> (id, name)
}

impl Ctx {
    fn id(&self, key: &str) -> &str {
        &self.users[key].0
    }
    fn name(&self, key: &str) -> &str {
        &self.users[key].1
    }
}

pub async fn seed(pool: &SqlitePool, password_hash: &str) -> AppResult<()> {
    // The data dir is the directory of the main database file; bytes go to
    // its content-addressed `files/` store like every upload.
    let db_file: String =
        sqlx::query_scalar("SELECT file FROM pragma_database_list WHERE name = 'main'")
            .fetch_one(pool)
            .await?;
    let data_dir = PathBuf::from(db_file)
        .parent()
        .map(|p| p.to_path_buf())
        .ok_or_else(|| AppError::internal("cannot determine data dir"))?;

    let mut users = HashMap::new();
    for p in super::PERSONAS {
        let id: String = sqlx::query_scalar("SELECT id FROM users WHERE email = ?")
            .bind(p.email)
            .fetch_one(pool)
            .await?;
        users.insert(p.key.to_string(), (id, p.name.to_string()));
    }
    for (key, name, email, org) in EXTRA_USERS {
        let id = match sqlx::query_scalar::<_, String>("SELECT id FROM users WHERE email = ?")
            .bind(email)
            .fetch_optional(pool)
            .await?
        {
            Some(id) => id,
            None => {
                let id = new_id();
                sqlx::query(
                    "INSERT INTO users (id, email, name, organisation, password_hash, created_at)
                     VALUES (?, ?, ?, ?, ?, ?)",
                )
                .bind(&id)
                .bind(email)
                .bind(name)
                .bind(org)
                .bind(password_hash)
                .bind(crate::util::now_rfc3339())
                .execute(pool)
                .await?;
                id
            }
        };
        users.insert(key.to_string(), (id, name.to_string()));
    }

    let template_version_id: String = sqlx::query_scalar(
        "SELECT tv.id FROM template_versions tv JOIN templates t ON t.id = tv.template_id
         WHERE t.key = 'base_use' AND tv.status = 'published' ORDER BY tv.version DESC LIMIT 1",
    )
    .fetch_one(pool)
    .await?;

    let ctx = Ctx {
        pool: pool.clone(),
        data_dir,
        today: Utc::now().date_naive(),
        template_version_id,
        users,
    };
    let resources = Resources::ensure(&ctx).await?;

    whales(&ctx).await?;
    drones(&ctx).await?;
    seabirds(&ctx).await?;
    reef_fish(&ctx, &resources).await?;
    coral_cover(&ctx).await?;
    sponges(&ctx).await?;
    microplastics(&ctx).await?;
    snails(&ctx).await?;
    legacy_projects(&ctx).await?;
    anna_draft(&ctx).await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Generic helpers
// ---------------------------------------------------------------------------

async fn exists(ctx: &Ctx, title: &str) -> AppResult<bool> {
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM projects WHERE title = ?")
        .bind(title)
        .fetch_one(&ctx.pool)
        .await?;
    Ok(n > 0)
}

/// Store bytes in content-addressed storage and return the `files.id`.
async fn store_file(ctx: &Ctx, bytes: &[u8], mime: &str, uploaded_by: &str) -> AppResult<String> {
    let sha256 = crate::util::sha256_hex(bytes);
    let path = crate::files::file_path(&ctx.data_dir, &sha256);
    if !path.exists() {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let tmp = path.with_extension("tmp");
        tokio::fs::write(&tmp, bytes).await?;
        tokio::fs::rename(&tmp, &path).await?;
    }
    sqlx::query(
        "INSERT INTO files (id, sha256, size, mime, storage_key, scan_status, scan_detail, uploaded_by, created_at)
         VALUES (?, ?, ?, ?, ?, 'clean', NULL, ?, ?)
         ON CONFLICT (sha256) DO NOTHING",
    )
    .bind(new_id())
    .bind(&sha256)
    .bind(bytes.len() as i64)
    .bind(mime)
    .bind(path.to_string_lossy().to_string())
    .bind(uploaded_by)
    .bind(crate::util::now_rfc3339())
    .execute(&ctx.pool)
    .await?;
    Ok(sqlx::query_scalar("SELECT id FROM files WHERE sha256 = ?")
        .bind(&sha256)
        .fetch_one(&ctx.pool)
        .await?)
}

/// A small text document that starts like a PDF (sniffed as application/pdf).
fn pdf_like(title: &str, body: &str) -> Vec<u8> {
    format!(
        "%PDF-1.4\n% Demo document — fictional people and data.\n% Title: {title}\n\n{body}\n%%EOF\n"
    )
    .into_bytes()
}

struct NewProject<'a> {
    title: &'a str,
    summary: &'a str,
    keywords: &'a str,
    organisation: &'a str,
    status: &'a str,
    start: NaiveDate,
    end: NaiveDate,
    created_by: &'a str,
    created_at: String,
    reference_year: Option<i32>,
    closed_reason: Option<&'a str>,
}

async fn insert_project(tx: &mut Tx<'_>, ctx: &Ctx, p: NewProject<'_>) -> AppResult<String> {
    let reference = match p.reference_year {
        Some(year) => Some(crate::refs::next(tx, "PIT", i64::from(year)).await?),
        None => None,
    };
    let id = new_id();
    sqlx::query(
        "INSERT INTO projects
         (id, reference, title, summary, keywords, organisation, template_version_id, answers_json,
          status, start_date, end_date, legacy, closed_reason, version, created_by, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, '{}', ?, ?, ?, 0, ?, 1, ?, ?)",
    )
    .bind(&id)
    .bind(&reference)
    .bind(p.title)
    .bind(p.summary)
    .bind(p.keywords)
    .bind(p.organisation)
    .bind(&ctx.template_version_id)
    .bind(p.status)
    .bind(ds(p.start))
    .bind(ds(p.end))
    .bind(p.closed_reason)
    .bind(p.created_by)
    .bind(&p.created_at)
    .execute(&mut **tx)
    .await?;
    audit(
        tx,
        &p.created_at,
        Some(p.created_by),
        "project.created",
        "project",
        &id,
        &id,
        "shared",
        &format!("Project '{}' created", p.title),
    )
    .await?;
    Ok(id)
}

async fn member(
    tx: &mut Tx<'_>,
    project_id: &str,
    user_id: &str,
    role: &str,
    added_by: &str,
    at: &str,
) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO project_members (id, project_id, user_id, role, added_by, added_at)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(new_id())
    .bind(project_id)
    .bind(user_id)
    .bind(role)
    .bind(added_by)
    .bind(at)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

fn point(lat: f64, lng: f64) -> Value {
    json!({"type": "Point", "coordinates": [lng, lat]})
}

/// Closed ring polygon from (lat, lng) corners.
fn polygon(corners: &[(f64, f64)]) -> Value {
    let mut ring: Vec<Value> = corners.iter().map(|(lat, lng)| json!([lng, lat])).collect();
    ring.push(ring[0].clone());
    json!({"type": "Polygon", "coordinates": [ring]})
}

fn bbox(geometry: &Value) -> (f64, f64, f64, f64) {
    let coords: Vec<(f64, f64)> = match geometry["type"].as_str() {
        Some("Point") => vec![(
            geometry["coordinates"][1].as_f64().unwrap_or(0.0),
            geometry["coordinates"][0].as_f64().unwrap_or(0.0),
        )],
        _ => geometry["coordinates"][0]
            .as_array()
            .map(|ring| {
                ring.iter()
                    .map(|c| (c[1].as_f64().unwrap_or(0.0), c[0].as_f64().unwrap_or(0.0)))
                    .collect()
            })
            .unwrap_or_default(),
    };
    let min_lat = coords.iter().map(|c| c.0).fold(f64::MAX, f64::min);
    let max_lat = coords.iter().map(|c| c.0).fold(f64::MIN, f64::max);
    let min_lng = coords.iter().map(|c| c.1).fold(f64::MAX, f64::min);
    let max_lng = coords.iter().map(|c| c.1).fold(f64::MIN, f64::max);
    (min_lat, min_lng, max_lat, max_lng)
}

async fn site(
    tx: &mut Tx<'_>,
    project_id: &str,
    name: &str,
    geometry: Value,
    sensitive: bool,
    at: &str,
) -> AppResult<String> {
    let (min_lat, min_lng, max_lat, max_lng) = bbox(&geometry);
    let id = new_id();
    sqlx::query(
        "INSERT INTO project_sites
         (id, project_id, name, geometry_json, min_lat, min_lng, max_lat, max_lng, sensitive, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(project_id)
    .bind(name)
    .bind(geometry.to_string())
    .bind(min_lat)
    .bind(min_lng)
    .bind(max_lat)
    .bind(max_lng)
    .bind(i64::from(sensitive))
    .bind(at)
    .execute(&mut **tx)
    .await?;
    Ok(id)
}

#[allow(clippy::too_many_arguments)]
async fn audit(
    tx: &mut Tx<'_>,
    at: &str,
    actor_id: Option<&str>,
    action: &str,
    entity_type: &str,
    entity_id: &str,
    project_id: &str,
    visibility: &str,
    summary: &str,
) -> AppResult<()> {
    let label: String = match actor_id {
        Some(id) => {
            sqlx::query_scalar("SELECT name FROM users WHERE id = ?")
                .bind(id)
                .fetch_one(&mut **tx)
                .await?
        }
        None => "Demo seed".into(),
    };
    sqlx::query(
        "INSERT INTO audit_events
         (id, at, actor_id, actor_label, action, entity_type, entity_id, project_id, visibility,
          summary, before_json, after_json, reason, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, NULL, NULL, NULL, ?)",
    )
    .bind(new_id())
    .bind(at)
    .bind(actor_id)
    .bind(&label)
    .bind(action)
    .bind(entity_type)
    .bind(entity_id)
    .bind(project_id)
    .bind(visibility)
    .bind(summary)
    .bind(at)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Annex 2 answers with every required field filled.
struct Answers<'a> {
    lead: &'a str,
    position: &'a str,
    organisation: &'a str,
    title: &'a str,
    aims: &'a str,
    objectives: &'a str,
    methods: &'a str,
    start: NaiveDate,
    end: NaiveDate,
    team: Value,
}

async fn set_answers(tx: &mut Tx<'_>, project_id: &str, a: Answers<'_>) -> AppResult<()> {
    let site_ids: Vec<String> = sqlx::query_scalar(
        "SELECT id FROM project_sites WHERE project_id = ? ORDER BY created_at, name",
    )
    .bind(project_id)
    .fetch_all(&mut **tx)
    .await?;
    let answers = json!({
        "applicant_name": a.lead,
        "applicant_title": "Dr",
        "position": a.position,
        "institution": a.organisation,
        "address": format!("{}\n1 Harbour Road (fictional address)", a.organisation),
        "funders": "Fictional Marine Research Fund (demo grant), institutional matching funds.",
        "researchers": a.team,
        "research_title": a.title,
        "aims": a.aims,
        "objectives": a.objectives,
        "methods": a.methods,
        "outputs_benefit": "A plain-language summary for the Pitcairn community, a public talk at the Public Hall during the visit, and an open dataset with metadata published through the Data Hub.",
        "data_management": "Raw data are stored at the home institution with daily off-site copies; processed data and a data dictionary are deposited with the Marine Science Base within 90 days of the trip. Sensitive locations are generalized before publication.",
        "resources_brought": "Survey equipment, dive gear and spare parts (listed in the safety plan).",
        "resources_on_island": "Base accommodation, wet lab bench, local boat hire with an islander skipper.",
        "timeline": "Week 1: arrival, safety briefing, site reconnaissance. Weeks 2–3: fieldwork. Final week: data checks, community talk, departure.",
        "budget": "Total project budget NZD 84,000 (fictional); Pitcairn-specific budget NZD 21,500 for accommodation, lab use and boat hire.",
        "dates": {"start": ds(a.start), "end": ds(a.end)},
        "sites": site_ids,
        "safety_summary": "Diving follows the institutional dive plan (buddy system, max 18 m, surface cover by boat). Medical evacuation is via the supply vessel; all team members carry evacuation insurance.",
    });
    sqlx::query("UPDATE projects SET answers_json = ? WHERE id = ?")
        .bind(answers.to_string())
        .bind(project_id)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

fn person(name: &str, role: &str, gender: &str, age: i64) -> Value {
    json!({"name": name, "role": role, "gender": gender, "age": age, "special_needs": "none"})
}

/// Insert a revision whose snapshot is built from the project's current rows
/// (self-contained, like a real submit).
async fn revision(
    tx: &mut Tx<'_>,
    project_id: &str,
    number: i64,
    submitted_by: &str,
    at: &str,
) -> AppResult<String> {
    #[allow(clippy::type_complexity)]
    let (title, summary, keywords, organisation, start, end, answers, tv): (
        String,
        String,
        String,
        String,
        Option<String>,
        Option<String>,
        String,
        String,
    ) = sqlx::query_as(
        "SELECT title, summary, keywords, organisation, start_date, end_date, answers_json,
                template_version_id FROM projects WHERE id = ?",
    )
    .bind(project_id)
    .fetch_one(&mut **tx)
    .await?;
    let team: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT u.id, u.name, pm.role FROM project_members pm JOIN users u ON u.id = pm.user_id
         WHERE pm.project_id = ? AND pm.removed_at IS NULL ORDER BY pm.added_at, u.name",
    )
    .bind(project_id)
    .fetch_all(&mut **tx)
    .await?;
    let sites: Vec<(String, String, String, i64)> = sqlx::query_as(
        "SELECT id, name, geometry_json, sensitive FROM project_sites WHERE project_id = ? ORDER BY name",
    )
    .bind(project_id)
    .fetch_all(&mut **tx)
    .await?;
    let docs: Vec<(String, Option<String>, String, String, String, String)> = sqlx::query_as(
        "SELECT d.id, d.slot_key, d.title, d.category, dv.id, f.sha256
         FROM documents d JOIN document_versions dv ON dv.document_id = d.id
         JOIN files f ON f.id = dv.file_id
         WHERE d.project_id = ? AND d.category != 'result'
           AND dv.number = (SELECT MAX(number) FROM document_versions WHERE document_id = d.id)
         ORDER BY d.title",
    )
    .bind(project_id)
    .fetch_all(&mut **tx)
    .await?;
    let snapshot = json!({
        "title": title,
        "summary": summary,
        "keywords": keywords,
        "organisation": organisation,
        "start_date": start,
        "end_date": end,
        "answers": serde_json::from_str::<Value>(&answers).unwrap_or(json!({})),
        "team": team.iter().map(|(id, name, role)| json!({"user_id": id, "name": name, "role": role})).collect::<Vec<_>>(),
        "sites": sites.iter().map(|(id, name, g, s)| json!({
            "id": id, "name": name,
            "geometry": serde_json::from_str::<Value>(g).unwrap_or(Value::Null),
            "sensitive": *s != 0,
        })).collect::<Vec<_>>(),
        "documents": docs.iter().map(|(id, slot, title, cat, vid, sha)| json!({
            "document_id": id, "slot_key": slot, "title": title, "category": cat,
            "version_id": vid, "sha256": sha,
        })).collect::<Vec<_>>(),
    });
    let id = new_id();
    sqlx::query(
        "INSERT INTO project_revisions
         (id, project_id, number, template_version_id, snapshot_json, submitted_by, submitted_at, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(project_id)
    .bind(number)
    .bind(&tv)
    .bind(snapshot.to_string())
    .bind(submitted_by)
    .bind(at)
    .bind(at)
    .execute(&mut **tx)
    .await?;
    audit(
        tx,
        at,
        Some(submitted_by),
        if number == 1 {
            "project.submit"
        } else {
            "project.resubmit"
        },
        "project",
        project_id,
        project_id,
        "shared",
        &format!("Revision {number} submitted"),
    )
    .await?;
    Ok(id)
}

#[allow(clippy::too_many_arguments)]
async fn document(
    tx: &mut Tx<'_>,
    project_id: &str,
    slot_key: Option<&str>,
    title: &str,
    category: &str,
    file_id: &str,
    by: &str,
    at: &str,
) -> AppResult<String> {
    let doc_id = new_id();
    sqlx::query(
        "INSERT INTO documents (id, project_id, slot_key, title, category, created_by, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&doc_id)
    .bind(project_id)
    .bind(slot_key)
    .bind(title)
    .bind(category)
    .bind(by)
    .bind(at)
    .execute(&mut **tx)
    .await?;
    let version_id = new_id();
    sqlx::query(
        "INSERT INTO document_versions (id, document_id, number, file_id, note, uploaded_by, uploaded_at, created_at)
         VALUES (?, ?, 1, ?, '', ?, ?, ?)",
    )
    .bind(&version_id)
    .bind(&doc_id)
    .bind(file_id)
    .bind(by)
    .bind(at)
    .bind(at)
    .execute(&mut **tx)
    .await?;
    Ok(version_id)
}

#[allow(clippy::too_many_arguments)]
async fn review(
    tx: &mut Tx<'_>,
    project_id: &str,
    revision_id: &str,
    expert_id: &str,
    assigned_by: &str,
    due: NaiveDate,
    status: &str,
    opinion: Option<(&str, &str)>,
    at: &str,
) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO review_assignments
         (id, project_id, project_revision_id, expert_id, assigned_by, due_date, status,
          opinion, recommendation, submitted_at, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(new_id())
    .bind(project_id)
    .bind(revision_id)
    .bind(expert_id)
    .bind(assigned_by)
    .bind(ds(due))
    .bind(status)
    .bind(opinion.map(|o| o.0))
    .bind(opinion.map(|o| o.1))
    .bind(opinion.map(|_| ts(due - Duration::days(2), 15)))
    .bind(at)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

struct NewDecision<'a> {
    kind: &'a str,
    basis: &'a str,
    valid: Option<(NaiveDate, NaiveDate)>,
    activities: Value,
    conditions: Value,
    restrictions: Value,
    drafted_by: &'a str,
    issued_by: &'a str,
    at: String,
}

async fn decision(
    tx: &mut Tx<'_>,
    project_id: &str,
    revision_id: &str,
    d: NewDecision<'_>,
) -> AppResult<String> {
    let sites: Vec<(String, String, String, i64)> = sqlx::query_as(
        "SELECT id, name, geometry_json, sensitive FROM project_sites WHERE project_id = ? ORDER BY name",
    )
    .bind(project_id)
    .fetch_all(&mut **tx)
    .await?;
    let snapshot: Vec<Value> = sites
        .iter()
        .map(|(id, name, g, s)| {
            json!({"id": id, "name": name,
                   "geometry": serde_json::from_str::<Value>(g).unwrap_or(Value::Null),
                   "sensitive": *s != 0})
        })
        .collect();
    let id = new_id();
    sqlx::query(
        "INSERT INTO decisions
         (id, project_id, project_revision_id, kind, status, basis, legal_reference, valid_from,
          valid_to, permitted_activities_json, conditions_json, restrictions_json,
          sites_snapshot_json, drafted_by, issued_by, issued_at, created_at)
         VALUES (?, ?, ?, ?, 'issued', ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(project_id)
    .bind(revision_id)
    .bind(d.kind)
    .bind(d.basis)
    .bind("Pitcairn Islands Marine Protected Area Ordinance 2016 (demo reference)")
    .bind(d.valid.map(|v| ds(v.0)))
    .bind(d.valid.map(|v| ds(v.1)))
    .bind(d.activities.to_string())
    .bind(d.conditions.to_string())
    .bind(d.restrictions.to_string())
    .bind(Value::Array(snapshot).to_string())
    .bind(d.drafted_by)
    .bind(d.issued_by)
    .bind(&d.at)
    .bind(&d.at)
    .execute(&mut **tx)
    .await?;
    audit(
        tx,
        &d.at,
        Some(d.issued_by),
        "decision.issued",
        "decision",
        &id,
        project_id,
        "shared",
        &format!("{} decision issued", d.kind),
    )
    .await?;
    Ok(id)
}

struct NewDeliverable<'a> {
    title: &'a str,
    description: &'a str,
    kind: &'a str,
    due: NaiveDate,
    sender: &'a str,
    recipient: &'a str,
    status: &'a str,
    publish_level: &'a str,
    embargo_until: Option<NaiveDate>,
    at: String,
}

async fn deliverable(
    tx: &mut Tx<'_>,
    project_id: &str,
    d: NewDeliverable<'_>,
) -> AppResult<String> {
    let agreed = d.status != "proposed";
    let published = d.publish_level != "none";
    let id = new_id();
    sqlx::query(
        "INSERT INTO deliverables
         (id, project_id, title, description, kind, due_date, sender_id, recipient_id, status,
          terms_version, team_agreed_at, team_agreed_by, staff_agreed_at, staff_agreed_by,
          resolution_note, publish_level, embargo_until, published_at, published_by,
          created_by, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?, ?, ?, ?, NULL, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(project_id)
    .bind(d.title)
    .bind(d.description)
    .bind(d.kind)
    .bind(ds(d.due))
    .bind(d.sender)
    .bind(d.recipient)
    .bind(d.status)
    .bind(agreed.then(|| d.at.clone()))
    .bind(agreed.then_some(d.sender))
    .bind(agreed.then(|| d.at.clone()))
    .bind(agreed.then_some(d.recipient))
    .bind(d.publish_level)
    .bind(d.embargo_until.map(ds))
    .bind(published.then(|| d.at.clone()))
    .bind(published.then_some(d.recipient))
    .bind(d.recipient)
    .bind(&d.at)
    .execute(&mut **tx)
    .await?;
    Ok(id)
}

#[allow(clippy::too_many_arguments)]
async fn submission(
    tx: &mut Tx<'_>,
    deliverable_id: &str,
    number: i64,
    by: &str,
    note: &str,
    status: &str,
    reviewer: Option<&str>,
    dictionary: Value,
    at: &str,
) -> AppResult<String> {
    let id = new_id();
    sqlx::query(
        "INSERT INTO deliverable_submissions
         (id, deliverable_id, number, submitted_by, note, data_dictionary_json, status,
          reviewed_by, review_note, reviewed_at, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(deliverable_id)
    .bind(number)
    .bind(by)
    .bind(note)
    .bind(dictionary.to_string())
    .bind(status)
    .bind(reviewer)
    .bind(reviewer.map(|_| "Received complete; thank you."))
    .bind(reviewer.map(|_| at.to_string()))
    .bind(at)
    .execute(&mut **tx)
    .await?;
    Ok(id)
}

async fn submission_file(
    tx: &mut Tx<'_>,
    submission_id: &str,
    version_id: &str,
    at: &str,
) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO submission_files (id, submission_id, document_version_id, created_at) VALUES (?, ?, ?, ?)",
    )
    .bind(new_id())
    .bind(submission_id)
    .bind(version_id)
    .bind(at)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn publication_file(
    tx: &mut Tx<'_>,
    deliverable_id: &str,
    version_id: &str,
    by: &str,
    at: &str,
) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO publication_files (id, deliverable_id, document_version_id, approved_by, approved_at, created_at)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(new_id())
    .bind(deliverable_id)
    .bind(version_id)
    .bind(by)
    .bind(at)
    .bind(at)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Insert measurements from a CSV in the ONE documented format
/// (`site,date,variable,value,unit`).
async fn measurements(
    tx: &mut Tx<'_>,
    project_id: &str,
    deliverable_id: &str,
    submission_id: &str,
    csv_text: &str,
    source_label: &str,
    at: &str,
) -> AppResult<()> {
    for line in csv_text.lines().skip(1).filter(|l| !l.trim().is_empty()) {
        let cols: Vec<&str> = line.split(',').map(str::trim).collect();
        let value: f64 = cols[3].parse().map_err(AppError::internal)?;
        sqlx::query(
            "INSERT INTO measurements
             (id, project_id, deliverable_id, submission_id, site_name, observed_on, variable_key,
              value, unit, source_label, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(new_id())
        .bind(project_id)
        .bind(deliverable_id)
        .bind(submission_id)
        .bind(cols[0])
        .bind(cols[1])
        .bind(cols[2])
        .bind(value)
        .bind(cols[4])
        .bind(source_label)
        .bind(at)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn trip(
    tx: &mut Tx<'_>,
    project_id: &str,
    title: &str,
    arrive: NaiveDate,
    depart: NaiveDate,
    participants: &[&str],
    status: &str,
    at: &str,
) -> AppResult<String> {
    let id = new_id();
    sqlx::query(
        "INSERT INTO trips (id, project_id, title, arrive_date, depart_date, participants_json, status, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(project_id)
    .bind(title)
    .bind(ds(arrive))
    .bind(ds(depart))
    .bind(json!(participants).to_string())
    .bind(status)
    .bind(at)
    .execute(&mut **tx)
    .await?;
    Ok(id)
}

#[allow(clippy::too_many_arguments)]
async fn booking(
    tx: &mut Tx<'_>,
    trip_id: &str,
    resource_id: &str,
    start: NaiveDate,
    end: NaiveDate,
    status: &str,
    requested_by: &str,
    decided_by: Option<&str>,
    at: &str,
) -> AppResult<String> {
    let id = new_id();
    sqlx::query(
        "INSERT INTO bookings
         (id, trip_id, resource_id, start_date, end_date, quantity, status, requested_by,
          decided_by, decided_at, created_at)
         VALUES (?, ?, ?, ?, ?, 1, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(trip_id)
    .bind(resource_id)
    .bind(ds(start))
    .bind(ds(end))
    .bind(status)
    .bind(requested_by)
    .bind(decided_by)
    .bind(decided_by.map(|_| at.to_string()))
    .bind(at)
    .execute(&mut **tx)
    .await?;
    Ok(id)
}

async fn thread_with_message(
    tx: &mut Tx<'_>,
    project_id: &str,
    anchor: (&str, &str),
    visibility: &str,
    author: &str,
    body: &str,
    at: &str,
) -> AppResult<String> {
    let thread_id = new_id();
    sqlx::query(
        "INSERT INTO threads (id, project_id, anchor_type, anchor_key, visibility, created_at)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(&thread_id)
    .bind(project_id)
    .bind(anchor.0)
    .bind(anchor.1)
    .bind(visibility)
    .bind(at)
    .execute(&mut **tx)
    .await?;
    sqlx::query(
        "INSERT INTO messages (id, thread_id, author_id, body, created_at) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(new_id())
    .bind(&thread_id)
    .bind(author)
    .bind(body)
    .bind(at)
    .execute(&mut **tx)
    .await?;
    Ok(thread_id)
}

async fn action_item(
    tx: &mut Tx<'_>,
    project_id: &str,
    thread_id: &str,
    title: &str,
    created_by: &str,
    at: &str,
) -> AppResult<()> {
    let id = new_id();
    sqlx::query(
        "INSERT INTO action_items (id, project_id, thread_id, addressed_to, title, status, created_by, created_at)
         VALUES (?, ?, ?, 'team', ?, 'open', ?, ?)",
    )
    .bind(&id)
    .bind(project_id)
    .bind(thread_id)
    .bind(title)
    .bind(created_by)
    .bind(at)
    .execute(&mut **tx)
    .await?;
    audit(
        tx,
        at,
        Some(created_by),
        "action_item.created",
        "action_item",
        &id,
        project_id,
        "shared",
        &format!("Action item for the team: {title}"),
    )
    .await
}

// ---------------------------------------------------------------------------
// Resources: the MSB resources and tariffs seeded by `seed::resources`
// (called before this module), looked up by name — never duplicated.
// ---------------------------------------------------------------------------

struct Resources {
    room: String,
    lab: String,
    equipment: String,
    boat: String,
}

impl Resources {
    async fn ensure(ctx: &Ctx) -> AppResult<Resources> {
        Ok(Resources {
            room: resource(ctx, "MSB twin bedroom").await?,
            lab: resource(ctx, "MSB wet laboratory").await?,
            equipment: resource(ctx, "Dive compressor").await?,
            boat: resource(ctx, "Boat charter — Bounty Bay Boat Hire (fictional)").await?,
        })
    }
}

async fn resource(ctx: &Ctx, name: &str) -> AppResult<String> {
    sqlx::query_scalar("SELECT id FROM resources WHERE name = ?")
        .bind(name)
        .fetch_optional(&ctx.pool)
        .await?
        .ok_or_else(|| {
            AppError::internal(format!(
                "seeded resource '{name}' missing (seed::resources must run first)"
            ))
        })
}

/// Snapshot price for an invoice line: latest tariff effective on `on`.
async fn tariff(
    tx: &mut Tx<'_>,
    resource_id: &str,
    on: NaiveDate,
) -> AppResult<(String, i64, String)> {
    let row: (String, i64, String) = sqlx::query_as(
        "SELECT t.unit, t.price_cents, r.name FROM tariffs t JOIN resources r ON r.id = t.resource_id
         WHERE t.resource_id = ? AND t.effective_from <= ?
         ORDER BY t.effective_from DESC LIMIT 1",
    )
    .bind(resource_id)
    .bind(ds(on))
    .fetch_one(&mut **tx)
    .await?;
    Ok(row)
}

// ---------------------------------------------------------------------------
// (a) 2023 — closed, report published with files
// ---------------------------------------------------------------------------

async fn whales(ctx: &Ctx) -> AppResult<()> {
    if exists(ctx, WHALES).await? {
        return Ok(());
    }
    let lukas = ctx.id("lukas");
    let maria = ctx.id("maria");
    let helen = ctx.id("helen");
    let james = ctx.id("james");
    let org = "North Sea Marine Lab (fictional)";

    let report_bytes = pdf_like(
        "Humpback whale acoustic monitoring around Pitcairn — final report",
        "Summary: Two bottom-moored hydrophones recorded humpback whale song from July to \
         August 2023. Song was detected on 31 of 38 recording days, peaking in early August.\n\
         Methods: SoundTrap recorders at 25 m depth off Bounty Bay and Tedside; 10-minute \
         duty cycle; manual and automated song detection.\n\
         Data: The recording index is deposited with the Marine Science Base.",
    );
    let report_file = store_file(ctx, &report_bytes, "application/pdf", lukas).await?;
    let index_csv = "recorder,site,start,end,files\nST-01,Bounty Bay hydrophone,2023-07-08,2023-08-09,4512\nST-02,Tedside hydrophone,2023-07-09,2023-08-09,4390\n";
    let index_file = store_file(ctx, index_csv.as_bytes(), "text/csv", lukas).await?;
    let safety = store_file(
        ctx,
        &pdf_like(
            "Field safety plan — hydrophone moorings",
            "Boat operations, mooring deployment and recovery risk assessment (fictional).",
        ),
        "application/pdf",
        lukas,
    )
    .await?;

    let mut tx = ctx.pool.begin().await?;
    let created = ts(date("2023-01-16"), 9);
    let pid = insert_project(
        &mut tx,
        ctx,
        NewProject {
            title: WHALES,
            summary: "Passive acoustic monitoring of humpback whale song off Pitcairn Island during the austral winter migration (fictional demo project).",
            keywords: "humpback whale, passive acoustics, hydrophone, migration",
            organisation: org,
            status: "closed",
            start: date("2023-07-03"),
            end: date("2023-08-14"),
            created_by: lukas,
            created_at: created.clone(),
            reference_year: Some(2023),
            closed_reason: Some("All deliverables accepted"),
        },
    )
    .await?;
    member(&mut tx, &pid, lukas, "lead", lukas, &created).await?;
    site(
        &mut tx,
        &pid,
        "Bounty Bay hydrophone",
        point(-25.0640, -130.0880),
        false,
        &created,
    )
    .await?;
    site(
        &mut tx,
        &pid,
        "Tedside hydrophone",
        point(-25.0650, -130.1250),
        false,
        &created,
    )
    .await?;
    set_answers(
        &mut tx,
        &pid,
        Answers {
            lead: ctx.name("lukas"),
            position: "Senior Research Scientist",
            organisation: org,
            title: WHALES,
            aims: "Describe the timing and intensity of humpback whale song off Pitcairn during the winter migration.",
            objectives: "1. Deploy two hydrophones for six weeks.\n2. Detect and count song bouts per day.\n3. Compare the two sites.",
            methods: "Bottom-moored SoundTrap recorders at 25 m; duty-cycled recording; manual validation of automated detections.",
            start: date("2023-07-03"),
            end: date("2023-08-14"),
            team: json!([person(ctx.name("lukas"), "lead researcher", "male", 46)]),
        },
    )
    .await?;
    document(
        &mut tx,
        &pid,
        Some("safety_plan"),
        "Field safety plan",
        "application",
        &safety,
        lukas,
        &created,
    )
    .await?;
    let submitted = ts(date("2023-01-20"), 10);
    let rev = revision(&mut tx, &pid, 1, lukas, &submitted).await?;
    review(
        &mut tx,
        &pid,
        &rev,
        james,
        maria,
        date("2023-03-10"),
        "submitted",
        Some((
            "Well-designed, low-impact study. Moorings must avoid coral heads; recovery plan is adequate.",
            "approve_with_conditions",
        )),
        &ts(date("2023-02-10"), 9),
    )
    .await?;
    decision(
        &mut tx,
        &pid,
        &rev,
        NewDecision {
            kind: "permit",
            basis: "The study is non-invasive and supports the MPA management plan.",
            valid: Some((date("2023-07-01"), date("2023-08-31"))),
            activities: json!([
                "Deployment of two bottom-moored hydrophones",
                "Boat-based mooring service visits"
            ]),
            conditions: json!([
                "Moorings placed on sand, not on live coral",
                "All equipment recovered before departure",
                "Recording index deposited within 90 days"
            ]),
            restrictions: json!(["No playback experiments"]),
            drafted_by: maria,
            issued_by: helen,
            at: ts(date("2023-03-20"), 14),
        },
    )
    .await?;
    trip(
        &mut tx,
        &pid,
        "Hydrophone deployment and recovery",
        date("2023-07-06"),
        date("2023-08-10"),
        &[lukas],
        "completed",
        &created,
    )
    .await?;

    let accepted_at = ts(date("2023-10-02"), 11);
    let report = deliverable(
        &mut tx,
        &pid,
        NewDeliverable {
            title: "Final acoustic monitoring report",
            description: "Report on song detections per day and site, with methods and recommendations.",
            kind: "report",
            due: date("2023-10-31"),
            sender: lukas,
            recipient: maria,
            status: "accepted",
            publish_level: "metadata_and_files",
            embargo_until: None,
            at: accepted_at.clone(),
        },
    )
    .await?;
    let report_version = document(
        &mut tx,
        &pid,
        None,
        "Final acoustic monitoring report (PDF)",
        "result",
        &report_file,
        lukas,
        &accepted_at,
    )
    .await?;
    let sub = submission(
        &mut tx,
        &report,
        1,
        lukas,
        "Final report attached.",
        "accepted",
        Some(maria),
        json!([]),
        &accepted_at,
    )
    .await?;
    submission_file(&mut tx, &sub, &report_version, &accepted_at).await?;
    publication_file(&mut tx, &report, &report_version, maria, &accepted_at).await?;

    let dataset = deliverable(
        &mut tx,
        &pid,
        NewDeliverable {
            title: "Hydrophone recording index",
            description: "Index of all recordings (recorder, site, date range, file count).",
            kind: "dataset",
            due: date("2023-11-15"),
            sender: lukas,
            recipient: maria,
            status: "accepted",
            publish_level: "metadata",
            embargo_until: None,
            at: accepted_at.clone(),
        },
    )
    .await?;
    let index_version = document(
        &mut tx,
        &pid,
        None,
        "Recording index (CSV)",
        "result",
        &index_file,
        lukas,
        &accepted_at,
    )
    .await?;
    let sub = submission(
        &mut tx,
        &dataset,
        1,
        lukas,
        "Recording index; raw audio held at the home institution.",
        "accepted",
        Some(maria),
        json!([{"column": "files", "description": "Number of 10-minute audio files", "unit": "count", "method": "file listing"}]),
        &accepted_at,
    )
    .await?;
    submission_file(&mut tx, &sub, &index_version, &accepted_at).await?;
    audit(
        &mut tx,
        &ts(date("2023-11-20"), 9),
        Some(maria),
        "project.close",
        "project",
        &pid,
        &pid,
        "shared",
        "Status changed from approved to closed",
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// (f) 2024 — refused with reasons
// ---------------------------------------------------------------------------

async fn drones(ctx: &Ctx) -> AppResult<()> {
    if exists(ctx, DRONES).await? {
        return Ok(());
    }
    let erik = ctx.id("erik");
    let maria = ctx.id("maria");
    let helen = ctx.id("helen");
    let james = ctx.id("james");
    let org = "Baltic Ocean Institute (fictional)";
    let mut tx = ctx.pool.begin().await?;
    let created = ts(date("2024-02-05"), 9);
    let pid = insert_project(
        &mut tx,
        ctx,
        NewProject {
            title: DRONES,
            summary: "Low-altitude drone photogrammetry of seabird nesting colonies on Oeno Island (fictional demo project).",
            keywords: "drone, seabirds, photogrammetry, Oeno",
            organisation: org,
            status: "refused",
            start: date("2024-10-01"),
            end: date("2024-10-20"),
            created_by: erik,
            created_at: created.clone(),
            reference_year: Some(2024),
            closed_reason: None,
        },
    )
    .await?;
    member(&mut tx, &pid, erik, "lead", erik, &created).await?;
    site(
        &mut tx,
        &pid,
        "Oeno nesting colonies",
        polygon(&[
            (-23.918, -130.742),
            (-23.918, -130.728),
            (-23.930, -130.728),
            (-23.930, -130.742),
        ]),
        true,
        &created,
    )
    .await?;
    set_answers(
        &mut tx,
        &pid,
        Answers {
            lead: ctx.name("erik"),
            position: "Research Fellow",
            organisation: org,
            title: DRONES,
            aims: "Map seabird nest density on Oeno with drone imagery.",
            objectives: "1. Fly grid surveys at 30 m altitude.\n2. Count nests from orthomosaics.",
            methods: "Multirotor drone, daily flights during peak nesting.",
            start: date("2024-10-01"),
            end: date("2024-10-20"),
            team: json!([person(ctx.name("erik"), "lead researcher", "male", 39)]),
        },
    )
    .await?;
    let submitted = ts(date("2024-02-12"), 10);
    let rev = revision(&mut tx, &pid, 1, erik, &submitted).await?;
    review(
        &mut tx,
        &pid,
        &rev,
        james,
        maria,
        date("2024-03-20"),
        "submitted",
        Some((
            "Flights at 30 m during peak nesting are likely to cause flushing and egg loss. No disturbance mitigation is proposed.",
            "reject",
        )),
        &ts(date("2024-02-20"), 9),
    )
    .await?;
    decision(
        &mut tx,
        &pid,
        &rev,
        NewDecision {
            kind: "refusal",
            basis: "Refused. Drone overflights at 30 m during the peak nesting season pose an unacceptable risk of disturbance to protected seabird colonies, and the application contains no disturbance mitigation plan (minimum altitude, timing outside peak nesting, abort criteria). A new application addressing these points is welcome.",
            valid: None,
            activities: json!([]),
            conditions: json!([]),
            restrictions: json!([]),
            drafted_by: maria,
            issued_by: helen,
            at: ts(date("2024-04-02"), 14),
        },
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// (b) 2024 — approved, dataset accepted with measurements, one overdue
// ---------------------------------------------------------------------------

async fn seabirds(ctx: &Ctx) -> AppResult<()> {
    if exists(ctx, SEABIRDS).await? {
        return Ok(());
    }
    let anna = ctx.id("anna");
    let priya = ctx.id("priya");
    let maria = ctx.id("maria");
    let helen = ctx.id("helen");
    let org = "Te Moana University (fictional), Wellington NZ";

    let csv_text = "site,date,variable,value,unit\n\
North Beach colony,2024-09-12,seabird_nest_count,412,nests\n\
East Beach colony,2024-09-14,seabird_nest_count,268,nests\n\
Plateau transect,2024-09-18,seabird_nest_count,157,nests\n\
North Beach colony,2024-10-03,seabird_nest_count,447,nests\n\
East Beach colony,2024-10-05,seabird_nest_count,281,nests\n\
Plateau transect,2024-10-07,seabird_nest_count,149,nests\n";
    let csv_file = store_file(ctx, csv_text.as_bytes(), "text/csv", priya).await?;

    let mut tx = ctx.pool.begin().await?;
    let created = ts(date("2024-01-22"), 9);
    let pid = insert_project(
        &mut tx,
        ctx,
        NewProject {
            title: SEABIRDS,
            summary: "Census of breeding seabirds (petrels, boobies, noddies) on Henderson Island, a World Heritage site (fictional demo project).",
            keywords: "seabirds, census, Henderson Island, petrels, nests",
            organisation: org,
            status: "approved",
            start: date("2024-09-02"),
            end: date("2024-10-15"),
            created_by: anna,
            created_at: created.clone(),
            reference_year: Some(2024),
            closed_reason: None,
        },
    )
    .await?;
    member(&mut tx, &pid, anna, "lead", anna, &created).await?;
    member(&mut tx, &pid, priya, "editor", anna, &created).await?;
    site(
        &mut tx,
        &pid,
        "North Beach colony",
        point(-24.3505, -128.3196),
        false,
        &created,
    )
    .await?;
    site(
        &mut tx,
        &pid,
        "East Beach colony",
        point(-24.3683, -128.2986),
        false,
        &created,
    )
    .await?;
    site(
        &mut tx,
        &pid,
        "Plateau transect",
        polygon(&[
            (-24.360, -128.335),
            (-24.360, -128.320),
            (-24.372, -128.320),
            (-24.372, -128.335),
        ]),
        true,
        &created,
    )
    .await?;
    set_answers(
        &mut tx,
        &pid,
        Answers {
            lead: ctx.name("anna"),
            position: "Senior Lecturer in Marine Ecology",
            organisation: org,
            title: SEABIRDS,
            aims: "Estimate breeding population sizes of seabirds on Henderson Island.",
            objectives: "1. Count active nests at three sites twice.\n2. Compare with earlier counts.\n3. Deposit a standard measurement dataset.",
            methods: "Ground counts of active nests along fixed transects; minimal-disturbance protocol.",
            start: date("2024-09-02"),
            end: date("2024-10-15"),
            team: json!([person(ctx.name("anna"), "lead researcher", "female", 44), person(ctx.name("priya"), "field ecologist", "female", 31)]),
        },
    )
    .await?;
    let rev = revision(&mut tx, &pid, 1, anna, &ts(date("2024-02-01"), 10)).await?;
    decision(
        &mut tx,
        &pid,
        &rev,
        NewDecision {
            kind: "permit",
            basis: "Census supports the Henderson Island World Heritage management plan.",
            valid: Some((date("2024-09-01"), date("2024-10-31"))),
            activities: json!(["Ground counts of seabird nests", "Camping at North Beach"]),
            conditions: json!([
                "Stay on marked transects",
                "No handling of birds or eggs",
                "Nest-count dataset in the standard measurement format"
            ]),
            restrictions: json!(["No visits to the plateau colony after dusk"]),
            drafted_by: maria,
            issued_by: helen,
            at: ts(date("2024-04-15"), 14),
        },
    )
    .await?;
    trip(
        &mut tx,
        &pid,
        "Henderson census expedition",
        date("2024-09-05"),
        date("2024-10-10"),
        &[anna, priya],
        "completed",
        &created,
    )
    .await?;

    let accepted_at = ts(date("2024-12-02"), 11);
    let dataset = deliverable(
        &mut tx,
        &pid,
        NewDeliverable {
            title: "Seabird nest count dataset",
            description: "Active nest counts per site and visit in the standard measurement CSV format.",
            kind: "dataset",
            due: date("2024-12-15"),
            sender: priya,
            recipient: maria,
            status: "accepted",
            publish_level: "metadata",
            embargo_until: None,
            at: accepted_at.clone(),
        },
    )
    .await?;
    let csv_version = document(
        &mut tx,
        &pid,
        None,
        "Seabird nest counts 2024 (CSV)",
        "result",
        &csv_file,
        priya,
        &accepted_at,
    )
    .await?;
    let sub = submission(
        &mut tx,
        &dataset,
        1,
        priya,
        "Nest counts for both visits; photos available on request.",
        "accepted",
        Some(maria),
        json!([{"column": "value", "description": "Active nests counted", "unit": "nests", "method": "ground count along transect"}]),
        &accepted_at,
    )
    .await?;
    submission_file(&mut tx, &sub, &csv_version, &accepted_at).await?;
    sqlx::query(
        "INSERT INTO external_links (id, submission_id, url, description, version_label, access_notes,
                                     last_checked_at, last_status, available, created_at)
         VALUES (?, ?, ?, ?, 'v1', 'Photo archive mirror', ?, 'unavailable (mock link check)', 0, ?)",
    )
    .bind(new_id())
    .bind(&sub)
    .bind("https://data.example.invalid/missing/henderson-seabird-photos")
    .bind("Nest photo archive (external mirror)")
    .bind(ts(date("2025-01-05"), 3))
    .bind(&accepted_at)
    .execute(&mut *tx)
    .await?;
    measurements(
        &mut tx,
        &pid,
        &dataset,
        &sub,
        csv_text,
        "Seabird nest count dataset — submission 1",
        &accepted_at,
    )
    .await?;

    let report = deliverable(
        &mut tx,
        &pid,
        NewDeliverable {
            title: "Census summary report",
            description: "Short report comparing 2024 counts with earlier censuses.",
            kind: "report",
            due: date("2025-03-31"),
            sender: anna,
            recipient: maria,
            status: "agreed",
            publish_level: "none",
            embargo_until: None,
            at: ts(date("2024-08-20"), 10),
        },
    )
    .await?;
    let thread = thread_with_message(
        &mut tx,
        &pid,
        ("deliverable", &report),
        "shared",
        maria,
        "The census summary report is overdue. Could you send it, or propose a new due date with a reason?",
        &ts(date("2025-05-06"), 9),
    )
    .await?;
    action_item(
        &mut tx,
        &pid,
        &thread,
        "Send the census summary report or propose a new date",
        maria,
        &ts(date("2025-05-06"), 9),
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// (c) 2025 — approved, upcoming confirmed trip, invoice partially paid
// ---------------------------------------------------------------------------

async fn reef_fish(ctx: &Ctx, res: &Resources) -> AppResult<()> {
    if exists(ctx, REEF_FISH).await? {
        return Ok(());
    }
    let anna = ctx.id("anna");
    let liam = ctx.id("liam");
    let tomasi = ctx.id("tomasi");
    let maria = ctx.id("maria");
    let helen = ctx.id("helen");
    let sam = ctx.id("sam");
    let ruth = ctx.id("ruth");
    let david = ctx.id("david");
    let org = "Te Moana University (fictional), Wellington NZ";
    let today = ctx.today;
    let arrive = today + Duration::days(20);
    let depart = arrive + Duration::days(14);

    let mut tx = ctx.pool.begin().await?;
    let created = ts(date("2025-02-10"), 9);
    let pid = insert_project(
        &mut tx,
        ctx,
        NewProject {
            title: REEF_FISH,
            summary: "Underwater visual census of reef fish biomass on the Bounty Bay reef flat and slope (fictional demo project).",
            keywords: "reef fish, biomass, visual census, Bounty Bay",
            organisation: org,
            status: "approved",
            start: arrive,
            end: depart,
            created_by: liam,
            created_at: created.clone(),
            reference_year: Some(2025),
            closed_reason: None,
        },
    )
    .await?;
    member(&mut tx, &pid, liam, "lead", liam, &created).await?;
    member(&mut tx, &pid, anna, "editor", liam, &created).await?;
    member(&mut tx, &pid, tomasi, "viewer", liam, &created).await?;
    site(
        &mut tx,
        &pid,
        "Bounty Bay reef slope",
        polygon(&[
            (-25.0655, -130.0935),
            (-25.0655, -130.0905),
            (-25.0680, -130.0905),
            (-25.0680, -130.0935),
        ]),
        false,
        &created,
    )
    .await?;
    site(
        &mut tx,
        &pid,
        "Adams Rock",
        point(-25.0690, -130.0906),
        false,
        &created,
    )
    .await?;
    set_answers(
        &mut tx,
        &pid,
        Answers {
            lead: ctx.name("liam"),
            position: "Postdoctoral Researcher",
            organisation: org,
            title: REEF_FISH,
            aims: "Quantify reef fish biomass in Bounty Bay as a baseline for MPA monitoring.",
            objectives: "1. Survey 12 belt transects.\n2. Estimate biomass by trophic group.\n3. Publish a standard dataset.",
            methods: "Diver belt transects (25 m × 5 m) with length estimates converted to biomass.",
            start: arrive,
            end: depart,
            team: json!([person(ctx.name("liam"), "lead diver", "male", 34), person(ctx.name("anna"), "scientist", "female", 44), person(ctx.name("tomasi"), "dive support", "male", 28)]),
        },
    )
    .await?;
    let rev = revision(&mut tx, &pid, 1, liam, &ts(date("2025-02-20"), 10)).await?;
    decision(
        &mut tx,
        &pid,
        &rev,
        NewDecision {
            kind: "permit",
            basis: "Non-extractive baseline survey consistent with the MPA monitoring plan.",
            valid: Some((arrive - Duration::days(7), depart + Duration::days(30))),
            activities: json!(["SCUBA belt transects", "Boat access to Bounty Bay reef"]),
            conditions: json!([
                "Dive plan followed at all times",
                "Dataset deposited within 90 days of departure",
                "Community talk during the visit"
            ]),
            restrictions: json!(["No collection of specimens"]),
            drafted_by: maria,
            issued_by: helen,
            at: ts(date("2025-04-10"), 14),
        },
    )
    .await?;
    let trip_id = trip(
        &mut tx,
        &pid,
        "Reef fish survey",
        arrive,
        depart,
        &[liam, anna, tomasi],
        "confirmed",
        &created,
    )
    .await?;
    let decided = ts(today - Duration::days(10), 10);
    let room = booking(
        &mut tx,
        &trip_id,
        &res.room,
        arrive,
        depart,
        "confirmed",
        liam,
        Some(sam),
        &decided,
    )
    .await?;
    let lab = booking(
        &mut tx,
        &trip_id,
        &res.lab,
        arrive + Duration::days(1),
        depart - Duration::days(1),
        "confirmed",
        liam,
        Some(sam),
        &decided,
    )
    .await?;
    let boat = booking(
        &mut tx,
        &trip_id,
        &res.boat,
        arrive + Duration::days(2),
        arrive + Duration::days(5),
        "confirmed",
        liam,
        Some(david),
        &decided,
    )
    .await?;
    booking(
        &mut tx,
        &trip_id,
        &res.equipment,
        arrive + Duration::days(2),
        arrive + Duration::days(9),
        "requested",
        liam,
        None,
        &decided,
    )
    .await?;
    booking(
        &mut tx,
        &trip_id,
        &res.boat,
        arrive + Duration::days(9),
        arrive + Duration::days(11),
        "requested",
        liam,
        None,
        &decided,
    )
    .await?;

    // Invoice: issued, partially paid (one verified payment), one more
    // payment waiting for verification; plus a draft for the compressor.
    let year = today.year();
    let number = crate::refs::next(&mut tx, "INV", i64::from(year)).await?;
    let invoice_id = new_id();
    let issued_at = ts(today - Duration::days(9), 9);
    sqlx::query(
        "INSERT INTO invoices (id, project_id, number, status, currency, issued_at, due_date, created_by, created_at)
         VALUES (?, ?, ?, 'issued', 'NZD', ?, ?, ?, ?)",
    )
    .bind(&invoice_id)
    .bind(&pid)
    .bind(&number)
    .bind(&issued_at)
    .bind(ds(today + Duration::days(21)))
    .bind(ruth)
    .bind(&issued_at)
    .execute(&mut *tx)
    .await?;
    let mut total = 0i64;
    for (booking_id, resource_id, start, end) in [
        (&room, &res.room, arrive, depart),
        (
            &lab,
            &res.lab,
            arrive + Duration::days(1),
            depart - Duration::days(1),
        ),
        (
            &boat,
            &res.boat,
            arrive + Duration::days(2),
            arrive + Duration::days(5),
        ),
    ] {
        let (unit, price, name) = tariff(&mut tx, resource_id, start).await?;
        let quantity = (end - start).num_days();
        let amount = price * quantity;
        total += amount;
        sqlx::query(
            "INSERT INTO invoice_lines (id, invoice_id, booking_id, description, quantity, unit, unit_price_cents, amount_cents, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(new_id())
        .bind(&invoice_id)
        .bind(booking_id)
        .bind(format!("{name} {} → {}", ds(start), ds(end)))
        .bind(quantity as f64)
        .bind(&unit)
        .bind(price)
        .bind(amount)
        .bind(&issued_at)
        .execute(&mut *tx)
        .await?;
    }
    let partial = (total / 2 / 100) * 100;
    for (amount, status, method, ext_ref, verified_by, received) in [
        (
            partial,
            "verified",
            "bank_transfer",
            Some(format!("DEMO-{number}-1")),
            Some(ruth),
            today - Duration::days(5),
        ),
        (
            25_000,
            "pending_verification",
            "manual",
            None,
            None,
            today - Duration::days(1),
        ),
    ] {
        sqlx::query(
            "INSERT INTO payments (id, invoice_id, kind, amount_cents, currency, method, external_ref, status,
                                   received_at, verified_by, note, created_at)
             VALUES (?, ?, 'payment', ?, 'NZD', ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(new_id())
        .bind(&invoice_id)
        .bind(amount)
        .bind(method)
        .bind(&ext_ref)
        .bind(status)
        .bind(ts(received, 12))
        .bind(verified_by)
        .bind(if method == "manual" { "Cash payment at the base office (demo)" } else { "" })
        .bind(ts(received, 12))
        .execute(&mut *tx)
        .await?;
    }
    audit(
        &mut tx,
        &issued_at,
        Some(ruth),
        "invoice.issued",
        "invoice",
        &invoice_id,
        &pid,
        "shared",
        &format!("Invoice {number} issued"),
    )
    .await?;
    let draft_id = new_id();
    sqlx::query(
        "INSERT INTO invoices (id, project_id, number, status, currency, created_by, created_at)
         VALUES (?, ?, NULL, 'draft', 'NZD', ?, ?)",
    )
    .bind(&draft_id)
    .bind(&pid)
    .bind(ruth)
    .bind(&issued_at)
    .execute(&mut *tx)
    .await?;
    let (unit, price, name) = tariff(&mut tx, &res.equipment, arrive).await?;
    sqlx::query(
        "INSERT INTO invoice_lines (id, invoice_id, booking_id, description, quantity, unit, unit_price_cents, amount_cents, created_at)
         VALUES (?, ?, NULL, ?, 7, ?, ?, ?, ?)",
    )
    .bind(new_id())
    .bind(&draft_id)
    .bind(format!("{name} (7 days, pending booking confirmation)"))
    .bind(&unit)
    .bind(price)
    .bind(price * 7)
    .bind(&issued_at)
    .execute(&mut *tx)
    .await?;

    let agreed_at = ts(today - Duration::days(30), 10);
    deliverable(
        &mut tx,
        &pid,
        NewDeliverable {
            title: "Reef fish biomass dataset",
            description: "Transect-level biomass estimates in the standard measurement CSV format.",
            kind: "dataset",
            due: depart + Duration::days(90),
            sender: liam,
            recipient: maria,
            status: "agreed",
            publish_level: "none",
            embargo_until: None,
            at: agreed_at.clone(),
        },
    )
    .await?;
    deliverable(
        &mut tx,
        &pid,
        NewDeliverable {
            title: "Field report and community summary",
            description: "Short field report plus a plain-language summary for the community.",
            kind: "report",
            due: depart + Duration::days(60),
            sender: anna,
            recipient: maria,
            status: "agreed",
            publish_level: "none",
            embargo_until: None,
            at: agreed_at,
        },
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// (d) 2025 — comparable measurements, metadata published, files embargoed
// ---------------------------------------------------------------------------

async fn coral_cover(ctx: &Ctx) -> AppResult<()> {
    if exists(ctx, CORAL_COVER).await? {
        return Ok(());
    }
    let mele = ctx.id("mele");
    let maria = ctx.id("maria");
    let helen = ctx.id("helen");
    let org = "Coral Triangle Reef Institute (fictional)";
    let csv_text = "site,date,variable,value,unit\n\
Bounty Bay transect,2025-03-12,coral_cover_percent,31.5,%\n\
St Paul's Pool transect,2025-03-14,coral_cover_percent,22.0,%\n\
Down Rope transect,2025-03-17,coral_cover_percent,38.2,%\n\
Bounty Bay transect,2026-03-10,coral_cover_percent,29.8,%\n\
St Paul's Pool transect,2026-03-12,coral_cover_percent,23.4,%\n\
Down Rope transect,2026-03-16,coral_cover_percent,36.9,%\n";
    let csv_file = store_file(ctx, csv_text.as_bytes(), "text/csv", mele).await?;
    let photo_index = store_file(
        ctx,
        b"photo,site,date,quadrat\nBB-001.jpg,Bounty Bay transect,2026-03-10,1\nBB-002.jpg,Bounty Bay transect,2026-03-10,2\n",
        "text/csv",
        mele,
    )
    .await?;

    let mut tx = ctx.pool.begin().await?;
    let created = ts(date("2024-11-04"), 9);
    let pid = insert_project(
        &mut tx,
        ctx,
        NewProject {
            title: CORAL_COVER,
            summary: "Repeat photo-quadrat transects of live coral cover at three sites around Pitcairn (fictional demo project).",
            keywords: "coral cover, transects, photo quadrats, monitoring",
            organisation: org,
            status: "approved",
            start: date("2025-03-01"),
            end: date("2026-04-30"),
            created_by: mele,
            created_at: created.clone(),
            reference_year: Some(2025),
            closed_reason: None,
        },
    )
    .await?;
    member(&mut tx, &pid, mele, "lead", mele, &created).await?;
    site(
        &mut tx,
        &pid,
        "Bounty Bay transect",
        polygon(&[
            (-25.0660, -130.0930),
            (-25.0660, -130.0915),
            (-25.0670, -130.0915),
            (-25.0670, -130.0930),
        ]),
        false,
        &created,
    )
    .await?;
    site(
        &mut tx,
        &pid,
        "St Paul's Pool transect",
        point(-25.0760, -130.0880),
        false,
        &created,
    )
    .await?;
    site(
        &mut tx,
        &pid,
        "Down Rope transect",
        point(-25.0765, -130.0935),
        false,
        &created,
    )
    .await?;
    set_answers(
        &mut tx,
        &pid,
        Answers {
            lead: ctx.name("mele"),
            position: "Principal Investigator",
            organisation: org,
            title: CORAL_COVER,
            aims: "Track year-to-year change in live coral cover at three fixed sites.",
            objectives: "1. Photograph fixed quadrats in March each year.\n2. Score live coral cover.\n3. Deposit comparable datasets.",
            methods: "Fixed 20 m transects with 1 m² photo quadrats every 2 m; point-count scoring.",
            start: date("2025-03-01"),
            end: date("2026-04-30"),
            team: json!([person(ctx.name("mele"), "lead researcher", "female", 41)]),
        },
    )
    .await?;
    let rev = revision(&mut tx, &pid, 1, mele, &ts(date("2025-01-15"), 10)).await?;
    decision(
        &mut tx,
        &pid,
        &rev,
        NewDecision {
            kind: "permit",
            basis: "Long-term monitoring directly supports the MPA management plan.",
            valid: Some((date("2025-03-01"), date("2026-04-30"))),
            activities: json!([
                "SCUBA photo-quadrat surveys",
                "Fixed transect markers (stainless pins)"
            ]),
            conditions: json!([
                "Annual dataset in the standard measurement format",
                "Markers removed at project end"
            ]),
            restrictions: json!([]),
            drafted_by: maria,
            issued_by: helen,
            at: ts(date("2025-02-12"), 14),
        },
    )
    .await?;
    trip(
        &mut tx,
        &pid,
        "March 2025 transects",
        date("2025-03-10"),
        date("2025-03-20"),
        &[mele],
        "completed",
        &created,
    )
    .await?;
    trip(
        &mut tx,
        &pid,
        "March 2026 transects",
        date("2026-03-08"),
        date("2026-03-18"),
        &[mele],
        "completed",
        &created,
    )
    .await?;

    let accepted_at = ts(date("2026-05-04"), 11);
    let embargo = NaiveDate::from_ymd_opt(ctx.today.year() + 1, 1, 31).expect("valid date");
    let dataset = deliverable(
        &mut tx,
        &pid,
        NewDeliverable {
            title: "Coral cover dataset 2025–2026",
            description: "Live coral cover per transect and year in the standard measurement CSV format.",
            kind: "dataset",
            due: date("2026-05-31"),
            sender: mele,
            recipient: maria,
            status: "accepted",
            publish_level: "metadata_and_files",
            embargo_until: Some(embargo),
            at: accepted_at.clone(),
        },
    )
    .await?;
    let csv_version = document(
        &mut tx,
        &pid,
        None,
        "Coral cover 2025–2026 (CSV)",
        "result",
        &csv_file,
        mele,
        &accepted_at,
    )
    .await?;
    let sub = submission(
        &mut tx,
        &dataset,
        1,
        mele,
        "Both survey years in one file.",
        "accepted",
        Some(maria),
        json!([{"column": "value", "description": "Live coral cover", "unit": "%", "method": "point count on photo quadrats"}]),
        &accepted_at,
    )
    .await?;
    submission_file(&mut tx, &sub, &csv_version, &accepted_at).await?;
    publication_file(&mut tx, &dataset, &csv_version, maria, &accepted_at).await?;
    measurements(
        &mut tx,
        &pid,
        &dataset,
        &sub,
        csv_text,
        "Coral cover dataset 2025–2026 — submission 1",
        &accepted_at,
    )
    .await?;

    let photos = deliverable(
        &mut tx,
        &pid,
        NewDeliverable {
            title: "Transect photo archive",
            description: "Index of all quadrat photos with site and date.",
            kind: "media",
            due: date("2026-06-30"),
            sender: mele,
            recipient: maria,
            status: "submitted",
            publish_level: "none",
            embargo_until: None,
            at: ts(date("2026-06-20"), 10),
        },
    )
    .await?;
    let photo_version = document(
        &mut tx,
        &pid,
        None,
        "Photo archive index (CSV)",
        "result",
        &photo_index,
        mele,
        &ts(date("2026-06-20"), 10),
    )
    .await?;
    let sub = submission(
        &mut tx,
        &photos,
        1,
        mele,
        "Index of the photo archive; full-resolution images on request.",
        "received",
        None,
        json!([]),
        &ts(date("2026-06-20"), 10),
    )
    .await?;
    submission_file(&mut tx, &sub, &photo_version, &ts(date("2026-06-20"), 10)).await?;
    tx.commit().await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// (e) submitted, waiting for screening — Lukas's team
// ---------------------------------------------------------------------------

async fn sponges(ctx: &Ctx) -> AppResult<()> {
    if exists(ctx, SPONGES).await? {
        return Ok(());
    }
    let lukas = ctx.id("lukas");
    let org = "North Sea Marine Lab (fictional)";
    let submitted_on = ctx.today - Duration::days(4);
    let start = ctx.today + Duration::days(200);
    let end = start + Duration::days(18);
    let safety = store_file(
        ctx,
        &pdf_like(
            "Field safety plan — ROV operations",
            "ROV launch and recovery from a chartered vessel; weather limits (fictional).",
        ),
        "application/pdf",
        lukas,
    )
    .await?;
    let mut tx = ctx.pool.begin().await?;
    let created = ts(submitted_on - Duration::days(20), 9);
    let pid = insert_project(
        &mut tx,
        ctx,
        NewProject {
            title: SPONGES,
            summary: "ROV video transects of deep-water sponge assemblages on the flanks of Adams Seamount (fictional demo project).",
            keywords: "sponges, deep sea, ROV, seamount",
            organisation: org,
            status: "submitted",
            start,
            end,
            created_by: lukas,
            created_at: created.clone(),
            reference_year: Some(submitted_on.year()),
            closed_reason: None,
        },
    )
    .await?;
    member(&mut tx, &pid, lukas, "lead", lukas, &created).await?;
    site(
        &mut tx,
        &pid,
        "Adams Seamount flank",
        point(-25.342, -129.292),
        false,
        &created,
    )
    .await?;
    set_answers(
        &mut tx,
        &pid,
        Answers {
            lead: ctx.name("lukas"),
            position: "Senior Research Scientist",
            organisation: org,
            title: SPONGES,
            aims: "Describe sponge assemblages between 150 and 600 m on Adams Seamount.",
            objectives: "1. Fly six ROV transects.\n2. Annotate taxa and densities.\n3. Share video metadata.",
            methods: "Work-class ROV video transects with paired lasers for scale.",
            start,
            end,
            team: json!([person(ctx.name("lukas"), "lead researcher", "male", 46)]),
        },
    )
    .await?;
    document(
        &mut tx,
        &pid,
        Some("safety_plan"),
        "Field safety plan",
        "application",
        &safety,
        lukas,
        &created,
    )
    .await?;
    revision(&mut tx, &pid, 1, lukas, &ts(submitted_on, 10)).await?;
    tx.commit().await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// (g) in review with the expert
// ---------------------------------------------------------------------------

async fn microplastics(ctx: &Ctx) -> AppResult<()> {
    if exists(ctx, MICROPLASTICS).await? {
        return Ok(());
    }
    let sofia = ctx.id("sofia");
    let maria = ctx.id("maria");
    let james = ctx.id("james");
    let org = "Instituto Oceánico del Pacífico Sur (fictional)";
    let submitted_on = ctx.today - Duration::days(25);
    let start = ctx.today + Duration::days(150);
    let end = start + Duration::days(12);
    let mut tx = ctx.pool.begin().await?;
    let created = ts(submitted_on - Duration::days(10), 9);
    let pid = insert_project(
        &mut tx,
        ctx,
        NewProject {
            title: MICROPLASTICS,
            summary: "Surface manta-net sampling of microplastics in coastal waters around Pitcairn (fictional demo project).",
            keywords: "microplastics, manta net, pollution, coastal waters",
            organisation: org,
            status: "in_review",
            start,
            end,
            created_by: sofia,
            created_at: created.clone(),
            reference_year: Some(submitted_on.year()),
            closed_reason: None,
        },
    )
    .await?;
    member(&mut tx, &pid, sofia, "lead", sofia, &created).await?;
    site(
        &mut tx,
        &pid,
        "North coast sampling line",
        polygon(&[
            (-25.050, -130.120),
            (-25.050, -130.090),
            (-25.058, -130.090),
            (-25.058, -130.120),
        ]),
        false,
        &created,
    )
    .await?;
    set_answers(
        &mut tx,
        &pid,
        Answers {
            lead: ctx.name("sofia"),
            position: "Associate Researcher",
            organisation: org,
            title: MICROPLASTICS,
            aims: "Measure surface microplastic concentrations around Pitcairn.",
            objectives: "1. Tow a manta net on four lines.\n2. Count and classify particles.",
            methods: "Manta-net tows (333 µm), FTIR identification of particles.",
            start,
            end,
            team: json!([person(ctx.name("sofia"), "lead researcher", "female", 36)]),
        },
    )
    .await?;
    let rev = revision(&mut tx, &pid, 1, sofia, &ts(submitted_on, 10)).await?;
    audit(
        &mut tx,
        &ts(submitted_on + Duration::days(2), 9),
        Some(maria),
        "project.screen",
        "project",
        &pid,
        &pid,
        "shared",
        "Status changed from submitted to in review",
    )
    .await?;
    review(
        &mut tx,
        &pid,
        &rev,
        james,
        maria,
        ctx.today + Duration::days(10),
        "accepted",
        None,
        &ts(submitted_on + Duration::days(3), 9),
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// (h) changes requested — waiting for the applicant
// ---------------------------------------------------------------------------

async fn snails(ctx: &Ctx) -> AppResult<()> {
    if exists(ctx, SNAILS).await? {
        return Ok(());
    }
    let erik = ctx.id("erik");
    let maria = ctx.id("maria");
    let org = "Baltic Ocean Institute (fictional)";
    let submitted_on = ctx.today - Duration::days(12);
    let start = ctx.today + Duration::days(170);
    let end = start + Duration::days(10);
    let mut tx = ctx.pool.begin().await?;
    let created = ts(submitted_on - Duration::days(7), 9);
    let pid = insert_project(
        &mut tx,
        ctx,
        NewProject {
            title: SNAILS,
            summary: "Night surveys of endemic land snails in the island's forest remnants (fictional demo project).",
            keywords: "land snails, endemics, forest, night survey",
            organisation: org,
            status: "changes_requested",
            start,
            end,
            created_by: erik,
            created_at: created.clone(),
            reference_year: Some(submitted_on.year()),
            closed_reason: None,
        },
    )
    .await?;
    member(&mut tx, &pid, erik, "lead", erik, &created).await?;
    site(
        &mut tx,
        &pid,
        "Pawala Valley Ridge forest",
        point(-25.0684, -130.1131),
        true,
        &created,
    )
    .await?;
    set_answers(
        &mut tx,
        &pid,
        Answers {
            lead: ctx.name("erik"),
            position: "Research Fellow",
            organisation: org,
            title: SNAILS,
            aims: "Map remaining populations of endemic land snails.",
            objectives: "1. Survey 20 night plots.\n2. Record species and counts.",
            methods: "Timed night searches in fixed plots; no collection.",
            start,
            end,
            team: json!([person(ctx.name("erik"), "lead researcher", "male", 39)]),
        },
    )
    .await?;
    revision(&mut tx, &pid, 1, erik, &ts(submitted_on, 10)).await?;
    let asked = ts(submitted_on + Duration::days(3), 9);
    let thread = thread_with_message(
        &mut tx,
        &pid,
        ("document", "safety_plan"),
        "shared",
        maria,
        "Please add a field safety plan covering night work on steep terrain.",
        &asked,
    )
    .await?;
    action_item(
        &mut tx,
        &pid,
        &thread,
        "Please add a field safety plan",
        maria,
        &asked,
    )
    .await?;
    audit(
        &mut tx,
        &asked,
        Some(maria),
        "project.request_changes",
        "project",
        &pid,
        &pid,
        "shared",
        "Status changed from submitted to changes requested",
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Legacy projects — through the real legacy import code path
// ---------------------------------------------------------------------------

async fn legacy_projects(ctx: &Ctx) -> AppResult<()> {
    let already: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM projects WHERE reference = 'MSB-2016-004'")
            .fetch_one(&ctx.pool)
            .await?;
    if already > 0 {
        return Ok(());
    }
    let admin = Actor {
        user_id: ctx.id("admin").to_string(),
        email: "admin@demo.pitcairn.invalid".into(),
        name: ctx.name("admin").to_string(),
        roles: vec!["admin".into()],
        mfa_verified: true,
        demo: true,
    };
    let rows = crate::legacy::parse_csv(LEGACY_CSV.as_bytes())?;
    let rows = crate::legacy::preview_rows(&ctx.pool, rows).await?;
    let preview = crate::legacy::preview_json(&rows);
    let (batch_id, _) =
        crate::legacy::create_batch(&ctx.pool, crate::legacy::KIND, &preview, &admin).await?;
    crate::legacy::commit_rows(&ctx.pool, &batch_id, &rows, &admin).await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Anna's draft: every required field, sites and required documents, so the
// story can start with "Submit".
// ---------------------------------------------------------------------------

async fn anna_draft(ctx: &Ctx) -> AppResult<()> {
    let anna = ctx.id("anna");
    let row: Option<(String, String)> = sqlx::query_as(
        "SELECT id, answers_json FROM projects WHERE created_by = ? AND title = ? AND status = 'draft'",
    )
    .bind(anna)
    .bind(ANNA_DRAFT)
    .fetch_optional(&ctx.pool)
    .await?;
    let Some((pid, answers)) = row else {
        return Ok(());
    };
    if answers.trim() != "{}" {
        return Ok(()); // already filled (or edited by a user) — keep it
    }
    let org = "Te Moana University (fictional), Wellington NZ";
    let start = ctx.today + Duration::days(190);
    let end = start + Duration::days(21);
    let docs: BTreeMap<&str, (&str, &str, Vec<u8>)> = BTreeMap::from([
        (
            "safety_plan",
            (
                "Field safety plan",
                "application",
                pdf_like(
                    "Field safety plan — coral health survey",
                    "Dive safety, boat operations, medical evacuation and emergency contacts (fictional).",
                ),
            ),
        ),
        (
            "insurance",
            (
                "Insurance certificates",
                "personal",
                pdf_like(
                    "Insurance certificates",
                    "Medical incl. evacuation and third-party cover for all four team members (fictional).",
                ),
            ),
        ),
        (
            "cvs",
            (
                "CVs of the team",
                "personal",
                pdf_like(
                    "Team CVs",
                    "Short CVs of Anna Hart, Liam Chen, Priya Nair and Tomasi Vea (fictional).",
                ),
            ),
        ),
        (
            "permits",
            (
                "Permits and ethics approval",
                "application",
                pdf_like(
                    "Permits held",
                    "Institutional dive approval and research ethics waiver (fictional).",
                ),
            ),
        ),
    ]);
    let mut files = Vec::new();
    for (slot, (title, category, bytes)) in &docs {
        let file_id = store_file(ctx, bytes, "application/pdf", anna).await?;
        files.push((*slot, *title, *category, file_id));
    }

    let mut tx = ctx.pool.begin().await?;
    let now = crate::util::now_rfc3339();
    let has_sites: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM project_sites WHERE project_id = ?")
            .bind(&pid)
            .fetch_one(&mut *tx)
            .await?;
    if has_sites == 0 {
        site(
            &mut tx,
            &pid,
            "Bounty Bay reef",
            polygon(&[
                (-25.0650, -130.0950),
                (-25.0650, -130.0915),
                (-25.0685, -130.0915),
                (-25.0685, -130.0950),
            ]),
            false,
            &now,
        )
        .await?;
        site(
            &mut tx,
            &pid,
            "Spawning aggregation site (sensitive)",
            point(-25.0740, -130.0870),
            true,
            &now,
        )
        .await?;
    }
    for (slot, title, category, file_id) in files {
        let has: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM documents WHERE project_id = ? AND slot_key = ?",
        )
        .bind(&pid)
        .bind(slot)
        .fetch_one(&mut *tx)
        .await?;
        if has == 0 {
            document(
                &mut tx,
                &pid,
                Some(slot),
                title,
                category,
                &file_id,
                anna,
                &now,
            )
            .await?;
        }
    }
    set_answers(
        &mut tx,
        &pid,
        Answers {
            lead: ctx.name("anna"),
            position: "Senior Lecturer in Marine Ecology",
            organisation: org,
            title: ANNA_DRAFT,
            aims: "Establish a baseline of coral reef health around Pitcairn Island to support long-term MPA monitoring.",
            objectives: "1. Survey coral cover and bleaching at four sites.\n2. Record coral disease prevalence.\n3. Deposit a standard measurement dataset and a community summary.",
            methods: "SCUBA photo-quadrat transects (20 m) at 5 and 12 m depth; bleaching and disease scored per colony; water temperature loggers at each site.",
            start,
            end,
            team: json!([
                person(ctx.name("anna"), "lead researcher", "female", 44),
                person(ctx.name("liam"), "lead diver", "male", 34),
                person(ctx.name("priya"), "field ecologist", "female", 31),
                person(ctx.name("tomasi"), "dive support", "male", 28),
            ]),
        },
    )
    .await?;
    sqlx::query("UPDATE projects SET start_date = ?, end_date = ? WHERE id = ?")
        .bind(ds(start))
        .bind(ds(end))
        .bind(&pid)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}
