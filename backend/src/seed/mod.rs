//! Demo seed (architecture §9). Idempotent: safe to run repeatedly; existing
//! rows (matched by email / template key+version / settings key) are kept.
//! Later slices extend this module with more domain data (resources, tariffs,
//! historic projects).

use serde_json::json;
use sqlx::SqlitePool;

use crate::error::AppResult;

pub const DEMO_PASSWORD: &str = "demo-pass-2026";

pub struct Persona {
    pub key: &'static str,
    pub name: &'static str,
    pub email: &'static str,
    pub organisation: &'static str,
    pub role: Option<&'static str>,
    /// Fixed base32 TOTP secret for staff/expert so `/demo/totp/{user_id}`
    /// can show a live code. None for plain researchers/providers.
    pub totp_secret: Option<&'static str>,
}

pub const PERSONAS: &[Persona] = &[
    Persona {
        key: "anna",
        name: "Dr Anna Hart",
        email: "anna@demo.pitcairn.invalid",
        organisation: "Te Moana University (fictional), Wellington NZ",
        role: None,
        totp_secret: None,
    },
    Persona {
        key: "liam",
        name: "Liam Chen",
        email: "liam@demo.pitcairn.invalid",
        organisation: "Te Moana University (fictional), Wellington NZ",
        role: None,
        totp_secret: None,
    },
    Persona {
        key: "priya",
        name: "Priya Nair",
        email: "priya@demo.pitcairn.invalid",
        organisation: "Te Moana University (fictional), Wellington NZ",
        role: None,
        totp_secret: None,
    },
    Persona {
        key: "tomasi",
        name: "Tomasi Vea",
        email: "tomasi@demo.pitcairn.invalid",
        organisation: "Te Moana University (fictional), Wellington NZ",
        role: None,
        totp_secret: None,
    },
    Persona {
        key: "lukas",
        name: "Dr Lukas Weber",
        email: "lukas@demo.pitcairn.invalid",
        organisation: "North Sea Marine Lab (fictional)",
        role: None,
        totp_secret: None,
    },
    Persona {
        key: "maria",
        name: "Maria Ellis",
        email: "maria@demo.pitcairn.invalid",
        organisation: "Marine Science Base office, Natural Resources Division (demo)",
        role: Some("coordinator"),
        totp_secret: Some("PITCAIRNMARIA222222222222222222"),
    },
    Persona {
        key: "james",
        name: "Dr James Okafor",
        email: "james@demo.pitcairn.invalid",
        organisation: "MSB Scientific Advisory Panel (demo)",
        role: Some("expert"),
        totp_secret: Some("PITCAIRNJAMES222222222222222222"),
    },
    Persona {
        key: "helen",
        name: "Helen Brooks",
        email: "helen@demo.pitcairn.invalid",
        organisation: "Permits — acting for the Governor / MSB Board (demo)",
        role: Some("decision_maker"),
        totp_secret: Some("PITCAIRNHELEN22222222222222222"),
    },
    Persona {
        key: "sam",
        name: "Sam Torres",
        email: "sam@demo.pitcairn.invalid",
        organisation: "Marine Science Base (demo)",
        role: Some("base_manager"),
        totp_secret: Some("PITCAIRNSAM22222222222222222222"),
    },
    Persona {
        key: "ruth",
        name: "Ruth Palmer",
        email: "ruth@demo.pitcairn.invalid",
        organisation: "Pitcairn Islands Government (demo)",
        role: Some("finance"),
        totp_secret: Some("PITCAIRNRUTH2222222222222222222"),
    },
    Persona {
        key: "david",
        name: "David Lane",
        email: "david@demo.pitcairn.invalid",
        organisation: "Bounty Bay Boat Hire (fictional)",
        role: Some("provider"),
        totp_secret: None,
    },
    Persona {
        key: "admin",
        name: "Site Admin",
        email: "admin@demo.pitcairn.invalid",
        organisation: "—",
        role: Some("admin"),
        totp_secret: Some("PITCAIRNADMIN222222222222222222"),
    },
];

/// Annex 2 (base_use) template schema, built from
/// `docs/research/annex2-research-application.md` (items 1–17 + required docs).
pub fn base_use_schema() -> serde_json::Value {
    json!({
        "sections": [
            {"key": "applicant", "title": "Applicant & institution", "help": "The lead researcher takes responsibility for the project (Annex 2 items 1–5).", "fields": [
                {"key": "applicant_name", "label": "Name of applicant (the lead researcher who will take responsibility)", "type": "text", "required": true},
                {"key": "applicant_title", "label": "Title", "type": "text", "required": false, "help": "e.g. Dr / Prof."},
                {"key": "position", "label": "Position", "type": "text", "required": true},
                {"key": "institution", "label": "Institution", "type": "text", "required": true},
                {"key": "address", "label": "Address", "type": "textarea", "required": true}
            ]},
            {"key": "funding_team", "title": "Funding & team", "help": "Annex 2 items 6 and 15.", "fields": [
                {"key": "funders", "label": "Funder(s)", "type": "textarea", "required": true},
                {"key": "researchers", "label": "Named researchers who will be participating", "type": "people", "required": true, "help": "Declare the gender, age and any special needs of each participant."}
            ]},
            {"key": "project", "title": "Project", "help": "Annex 2 items 7–10.", "fields": [
                {"key": "research_title", "label": "Title of proposed research", "type": "text", "required": true},
                {"key": "aims", "label": "Aims (overarching statement, max 50 words)", "type": "textarea", "required": true},
                {"key": "objectives", "label": "Objectives (numbered list, max 200 words)", "type": "textarea", "required": true},
                {"key": "methods", "label": "Proposed approaches/methods (max 500 words)", "type": "textarea", "required": true, "help": "Clearly related to the specified objectives."}
            ]},
            {"key": "outputs_data", "title": "Outputs & data", "help": "Annex 2 items 11–12.", "fields": [
                {"key": "outputs_benefit", "label": "Anticipated outputs and benefit to the Pitcairn Island community (max 500 words)", "type": "textarea", "required": true},
                {"key": "data_management", "label": "Data management and sharing (max 300 words)", "type": "textarea", "required": true}
            ]},
            {"key": "resources", "title": "Resources & logistics", "help": "Annex 2 items 13–14.", "fields": [
                {"key": "resources_brought", "label": "Associated resources you will transport to the island", "type": "textarea", "required": false},
                {"key": "resources_on_island", "label": "Resources which you hope to source on the island", "type": "textarea", "required": false, "help": "Boats, diving support, quad hire etc."}
            ]},
            {"key": "plan", "title": "Timeline & budget", "help": "Annex 2 items 16–17.", "fields": [
                {"key": "timeline", "label": "Timeline (minimum resolution of a week)", "type": "textarea", "required": true},
                {"key": "budget", "label": "Budget (total project budget plus Pitcairn-specific budget)", "type": "textarea", "required": true},
                {"key": "dates", "label": "Proposed dates on Pitcairn", "type": "daterange", "required": true}
            ]},
            {"key": "sites", "title": "Location & sites", "help": "Where the research will take place.", "fields": [
                {"key": "sites", "label": "Research sites", "type": "sites", "required": true, "help": "Mark sensitive locations as sensitive; they are generalized for public viewers."}
            ]},
            {"key": "safety_risk", "title": "Safety & risk", "fields": [
                {"key": "safety_summary", "label": "Summary of key risks and mitigations", "type": "textarea", "required": true}
            ]}
        ],
        "required_documents": [
            {"key": "safety_plan", "label": "Field safety plan", "help": "Full health and safety plan including risk assessments covering all aspects of the proposed research.", "category": "application"},
            {"key": "insurance", "label": "Insurance certificate", "help": "Evidence of personal (medical, incl. evacuation) and third-party insurance for the duration of the visit.", "category": "personal"},
            {"key": "cvs", "label": "CVs of team", "help": "CVs of the lead researcher and members of the associated research party.", "category": "personal"},
            {"key": "permits", "label": "Permits held/needed", "help": "Evidence that necessary permits and permissions (incl. institutional ethical approvals) are or will be in place.", "category": "application"}
        ]
    })
}

/// Simple published fieldwork_permit template v1.
pub fn fieldwork_permit_schema() -> serde_json::Value {
    json!({
        "sections": [
            {"key": "purpose", "title": "Purpose", "fields": [
                {"key": "purpose", "label": "Purpose of fieldwork", "type": "textarea", "required": true}
            ]},
            {"key": "logistics", "title": "Logistics", "fields": [
                {"key": "dates", "label": "Fieldwork dates", "type": "daterange", "required": true},
                {"key": "team_size", "label": "Team size", "type": "number", "required": true},
                {"key": "activities", "label": "Permitted activities", "type": "multiselect", "required": true,
                 "options": ["diving", "sampling", "drone", "shore surveys", "moorings"]},
                {"key": "sites", "label": "Fieldwork sites", "type": "sites", "required": true}
            ]}
        ],
        "required_documents": []
    })
}

/// `pitcairn create-admin`: create a user with the admin role and return the
/// generated password. Errors if the email is already registered.
pub async fn create_admin(pool: &SqlitePool, email: &str, name: &str) -> AppResult<String> {
    let exists: Option<(String,)> = sqlx::query_as("SELECT id FROM users WHERE email = ?")
        .bind(email)
        .fetch_optional(pool)
        .await?;
    if exists.is_some() {
        return Err(crate::error::AppError::conflict(
            "user_exists",
            format!("a user with email {email} already exists"),
        ));
    }
    let password = crate::util::random_token();
    let now = crate::util::now_rfc3339();
    let user_id = crate::util::new_id();
    sqlx::query(
        "INSERT INTO users (id, email, name, organisation, password_hash, created_at)
         VALUES (?, ?, ?, '', ?, ?)",
    )
    .bind(&user_id)
    .bind(email)
    .bind(name)
    .bind(crate::password::hash_password(&password)?)
    .bind(&now)
    .execute(pool)
    .await?;
    sqlx::query("INSERT INTO user_roles (user_id, role, granted_by, granted_at) VALUES (?, 'admin', NULL, ?)")
        .bind(&user_id)
        .bind(&now)
        .execute(pool)
        .await?;
    Ok(password)
}

/// `pitcairn grant-role`: grant a role to an existing user (CLI bootstrap,
/// the only path to the first decision_maker/admin).
pub async fn grant_role(pool: &SqlitePool, email: &str, role: &str) -> AppResult<()> {
    const ROLES: [&str; 7] = [
        "coordinator",
        "expert",
        "decision_maker",
        "base_manager",
        "finance",
        "admin",
        "provider",
    ];
    if !ROLES.contains(&role) {
        return Err(crate::error::AppError::BadRequest(format!(
            "unknown role {role}; expected one of {}",
            ROLES.join(", ")
        )));
    }
    let user_id: String = sqlx::query_scalar("SELECT id FROM users WHERE email = ?")
        .bind(email)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| crate::error::AppError::BadRequest(format!("no user with email {email}")))?;
    let has: Option<(String,)> = sqlx::query_as(
        "SELECT user_id FROM user_roles WHERE user_id = ? AND role = ? AND revoked_at IS NULL",
    )
    .bind(&user_id)
    .bind(role)
    .fetch_optional(pool)
    .await?;
    if has.is_none() {
        sqlx::query(
            "INSERT INTO user_roles (user_id, role, granted_by, granted_at) VALUES (?, ?, NULL, ?)",
        )
        .bind(&user_id)
        .bind(role)
        .bind(crate::util::now_rfc3339())
        .execute(pool)
        .await?;
    }
    Ok(())
}

pub async fn seed_demo(pool: &SqlitePool) -> AppResult<()> {
    let now = crate::util::now_rfc3339();
    let password_hash = crate::password::hash_password(DEMO_PASSWORD)?;

    // --- personas, roles ---
    let mut admin_id = None;
    for p in PERSONAS {
        let existing: Option<(String,)> = sqlx::query_as("SELECT id FROM users WHERE email = ?")
            .bind(p.email)
            .fetch_optional(pool)
            .await?;
        let user_id = match existing {
            Some((id,)) => id,
            None => {
                let id = crate::util::new_id();
                let totp_enabled_at = p.totp_secret.map(|_| now.clone());
                sqlx::query(
                    "INSERT INTO users (id, email, name, organisation, password_hash, totp_secret, totp_enabled_at, created_at)
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                )
                .bind(&id)
                .bind(p.email)
                .bind(p.name)
                .bind(p.organisation)
                .bind(&password_hash)
                .bind(p.totp_secret)
                .bind(&totp_enabled_at)
                .bind(&now)
                .execute(pool)
                .await?;
                id
            }
        };
        if p.key == "admin" {
            admin_id = Some(user_id.clone());
        }
        if let Some(role) = p.role {
            let has: Option<(String,)> = sqlx::query_as(
                "SELECT user_id FROM user_roles WHERE user_id = ? AND role = ? AND revoked_at IS NULL",
            )
            .bind(&user_id)
            .bind(role)
            .fetch_optional(pool)
            .await?;
            if has.is_none() {
                sqlx::query(
                    "INSERT INTO user_roles (user_id, role, granted_by, granted_at) VALUES (?, ?, NULL, ?)",
                )
                .bind(&user_id)
                .bind(role)
                .bind(&now)
                .execute(pool)
                .await?;
            }
        }
    }

    // --- settings defaults ---
    for (key, value) in [
        ("mail_enabled", "true"),
        (
            "organisation_name",
            "Pitcairn Islands Marine Science Base (demo)",
        ),
        ("reference_prefix", "PIT"),
        ("public_catalog_enabled", "true"),
    ] {
        sqlx::query("INSERT OR IGNORE INTO settings (key, value) VALUES (?, ?)")
            .bind(key)
            .bind(value)
            .execute(pool)
            .await?;
    }

    // --- templates (base_use Annex 2 v1, fieldwork_permit v1), published ---
    for (key, name, description, schema) in [
        (
            "base_use",
            "Research application — use of the Marine Science Base (Annex 2)",
            "Application to undertake research involving the use of the Pitcairn Marine Science Base. Submit at least six months in advance.",
            base_use_schema(),
        ),
        (
            "fieldwork_permit",
            "Fieldwork permit",
            "Permit for fieldwork activities in the Pitcairn Islands Marine Protected Area.",
            fieldwork_permit_schema(),
        ),
    ] {
        let template_id: String = match sqlx::query_scalar::<_, String>(
            "SELECT id FROM templates WHERE key = ?",
        )
        .bind(key)
        .fetch_optional(pool)
        .await?
        {
            Some(id) => id,
            None => {
                let id = crate::util::new_id();
                sqlx::query(
                    "INSERT INTO templates (id, key, name, description, created_at) VALUES (?, ?, ?, ?, ?)",
                )
                .bind(&id)
                .bind(key)
                .bind(name)
                .bind(description)
                .bind(&now)
                .execute(pool)
                .await?;
                id
            }
        };
        let has_v1: Option<(String,)> = sqlx::query_as(
            "SELECT id FROM template_versions WHERE template_id = ? AND version = 1",
        )
        .bind(&template_id)
        .fetch_optional(pool)
        .await?;
        if has_v1.is_none() {
            sqlx::query(
                "INSERT INTO template_versions (id, template_id, version, schema_json, status, published_at, published_by, created_at)
                 VALUES (?, ?, 1, ?, 'published', ?, ?, ?)",
            )
            .bind(crate::util::new_id())
            .bind(&template_id)
            .bind(schema.to_string())
            .bind(&now)
            .bind(&admin_id)
            .bind(&now)
            .execute(pool)
            .await?;
        }
    }

    // --- Anna's draft project, ready to submit (§9) ---
    let anna_id: String = sqlx::query_scalar("SELECT id FROM users WHERE email = ?")
        .bind(PERSONAS[0].email)
        .fetch_one(pool)
        .await?;
    let has_project: Option<(String,)> = sqlx::query_as(
        "SELECT id FROM projects WHERE created_by = ? AND title = 'Coral health around Pitcairn'",
    )
    .bind(&anna_id)
    .fetch_optional(pool)
    .await?;
    if has_project.is_none() {
        let tv_id: String = sqlx::query_scalar(
            "SELECT tv.id FROM template_versions tv JOIN templates t ON t.id = tv.template_id
             WHERE t.key = 'base_use' AND tv.status = 'published' ORDER BY tv.version DESC LIMIT 1",
        )
        .fetch_one(pool)
        .await?;
        let project_id = crate::util::new_id();
        sqlx::query(
            "INSERT INTO projects (id, title, summary, keywords, organisation, template_version_id, answers_json, status, created_by, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, 'draft', ?, ?)",
        )
        .bind(&project_id)
        .bind("Coral health around Pitcairn")
        .bind("Baseline survey of coral reef health at four sites around Pitcairn Island (fictional demo project).")
        .bind("coral, reef health, baseline survey")
        .bind("Te Moana University (fictional), Wellington NZ")
        .bind(&tv_id)
        .bind("{}")
        .bind(&anna_id)
        .bind(&now)
        .execute(pool)
        .await?;
        for (email, role) in [
            (PERSONAS[0].email, "lead"),   // anna
            (PERSONAS[1].email, "editor"), // liam
            (PERSONAS[2].email, "editor"), // priya
            (PERSONAS[3].email, "viewer"), // tomasi
        ] {
            let uid: String = sqlx::query_scalar("SELECT id FROM users WHERE email = ?")
                .bind(email)
                .fetch_one(pool)
                .await?;
            sqlx::query(
                "INSERT INTO project_members (id, project_id, user_id, role, added_by, added_at)
                 VALUES (?, ?, ?, ?, ?, ?)",
            )
            .bind(crate::util::new_id())
            .bind(&project_id)
            .bind(&uid)
            .bind(role)
            .bind(&anna_id)
            .bind(&now)
            .execute(pool)
            .await?;
        }
    }

    Ok(())
}
