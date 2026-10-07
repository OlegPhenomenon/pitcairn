//! Decisions (§4 Review and decisions): drafts by coordinator or decision
//! maker, issuing by decision maker ONLY (admins get no decision rights),
//! supersession in one transaction, site snapshot at issue time, and a
//! printable HTML document.

use std::collections::HashMap;

use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{Value, json};
use sqlx::FromRow;

use crate::AppState;
use crate::audit::{self, AuditEvent};
use crate::authz::{self, Actor};
use crate::db;
use crate::dto::ListResponse;
use crate::dto::a::{CreateDecisionRequest, DecisionDto, PatchDecisionRequest};
use crate::error::{AppError, AppResult};
use crate::notify;
use crate::projects::{self, Action};
use crate::routes::conversation::open_team_action_items;
use crate::routes::projects::{decision_drafts_visible, require_project_access};
use crate::routes::sites;
use crate::util::{new_id, now_rfc3339};
use crate::validation::FieldErrors;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/projects/{id}/decisions",
            get(list_decisions).post(create_decision),
        )
        .route("/decisions/{id}", get(get_decision).patch(patch_decision))
        .route("/decisions/{id}/issue", post(issue_decision))
        .route("/decisions/{id}/document", get(decision_document))
}

pub const KINDS: &[&str] = &["permit", "refusal", "amendment", "extension", "revocation"];
/// Kinds that form the project's chain of permit-type decisions (at most one
/// current, i.e. issued and not superseded).
pub const PERMIT_CHAIN: &[&str] = &["permit", "amendment", "extension", "revocation"];

#[derive(FromRow)]
struct DecisionRow {
    id: String,
    project_id: String,
    project_revision_id: String,
    revision_number: i64,
    kind: String,
    status: String,
    basis: String,
    legal_reference: String,
    valid_from: Option<String>,
    valid_to: Option<String>,
    permitted_activities_json: String,
    conditions_json: String,
    restrictions_json: String,
    sites_snapshot_json: String,
    document_version_id: Option<String>,
    supersedes_id: Option<String>,
    superseded_by_id: Option<String>,
    change_request_id: Option<String>,
    drafted_by: String,
    drafted_by_name: String,
    issued_by: Option<String>,
    issued_by_name: Option<String>,
    issued_at: Option<String>,
    created_at: String,
}

const DECISION_SELECT: &str =
    "SELECT d.id, d.project_id, d.project_revision_id, r.number AS revision_number, d.kind,
            d.status, d.basis, d.legal_reference, d.valid_from, d.valid_to,
            d.permitted_activities_json, d.conditions_json, d.restrictions_json,
            d.sites_snapshot_json, d.document_version_id, d.supersedes_id, d.superseded_by_id,
            d.change_request_id, d.drafted_by, du.name AS drafted_by_name, d.issued_by,
            iu.name AS issued_by_name, d.issued_at, d.created_at
     FROM decisions d
     JOIN project_revisions r ON r.id = d.project_revision_id
     JOIN users du ON du.id = d.drafted_by
     LEFT JOIN users iu ON iu.id = d.issued_by";

fn string_list(json_text: &str) -> AppResult<Vec<String>> {
    serde_json::from_str(json_text).map_err(AppError::internal)
}

fn decision_dto(r: DecisionRow, precise: bool) -> AppResult<DecisionDto> {
    let mut sites_value: Value =
        serde_json::from_str(&r.sites_snapshot_json).map_err(AppError::internal)?;
    sites::generalize_site_list(&mut sites_value, precise);
    Ok(DecisionDto {
        id: r.id,
        project_id: r.project_id,
        project_revision_id: r.project_revision_id,
        revision_number: r.revision_number,
        kind: r.kind,
        status: r.status,
        basis: r.basis,
        legal_reference: r.legal_reference,
        valid_from: r.valid_from,
        valid_to: r.valid_to,
        permitted_activities: string_list(&r.permitted_activities_json)?,
        conditions: string_list(&r.conditions_json)?,
        restrictions: string_list(&r.restrictions_json)?,
        sites: match sites_value {
            Value::Array(a) => a,
            _ => Vec::new(),
        },
        document_version_id: r.document_version_id,
        supersedes_id: r.supersedes_id,
        superseded_by_id: r.superseded_by_id,
        change_request_id: r.change_request_id,
        drafted_by: r.drafted_by,
        drafted_by_name: r.drafted_by_name,
        issued_by: r.issued_by,
        issued_by_name: r.issued_by_name,
        issued_at: r.issued_at,
        created_at: r.created_at,
    })
}

async fn load_row<'e, E>(exec: E, id: &str) -> AppResult<DecisionRow>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    let row: Option<DecisionRow> = sqlx::query_as(&format!("{DECISION_SELECT} WHERE d.id = ?"))
        .bind(id)
        .fetch_optional(exec)
        .await?;
    row.ok_or(AppError::NotFound)
}

/// Read access to one decision: any project access; drafts only for
/// coordinators and decision makers (others get 404).
async fn readable_decision(
    state: &AppState,
    actor: &Actor,
    id: &str,
) -> AppResult<(DecisionRow, bool)> {
    let row = load_row(&state.pool, id).await?;
    let access = require_project_access(state, actor, &row.project_id).await?;
    if row.status != "issued" && !decision_drafts_visible(actor) {
        return Err(AppError::NotFound);
    }
    Ok((row, authz::can_see_precise_location(access)))
}

fn require_drafter(actor: &Actor) -> AppResult<()> {
    authz::require_role(actor, &["coordinator", "decision_maker"])
}

async fn list_decisions(
    State(state): State<AppState>,
    actor: Actor,
    Path(project_id): Path<String>,
) -> AppResult<Json<ListResponse<DecisionDto>>> {
    let access = require_project_access(&state, &actor, &project_id).await?;
    let precise = authz::can_see_precise_location(access);
    let rows: Vec<DecisionRow> = sqlx::query_as(&format!(
        "{DECISION_SELECT} WHERE d.project_id = ? AND (d.status = 'issued' OR ? = 1)
         ORDER BY COALESCE(d.issued_at, d.created_at), d.created_at"
    ))
    .bind(&project_id)
    .bind(decision_drafts_visible(&actor))
    .fetch_all(&state.pool)
    .await?;
    let items = rows
        .into_iter()
        .map(|r| decision_dto(r, precise))
        .collect::<AppResult<Vec<_>>>()?;
    let total = items.len() as i64;
    Ok(Json(ListResponse { items, total }))
}

async fn get_decision(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<String>,
) -> AppResult<Json<DecisionDto>> {
    let (row, precise) = readable_decision(&state, &actor, &id).await?;
    Ok(Json(decision_dto(row, precise)?))
}

// ---------------------------------------------------------------------------
// Drafting
// ---------------------------------------------------------------------------

/// Fully resolved draft content (after applying a create or patch body).
struct Draft {
    kind: String,
    project_revision_id: String,
    basis: String,
    legal_reference: String,
    valid_from: Option<String>,
    valid_to: Option<String>,
    permitted_activities: Vec<String>,
    conditions: Vec<String>,
    restrictions: Vec<String>,
    supersedes_id: Option<String>,
    change_request_id: Option<String>,
    document_version_id: Option<String>,
}

fn check_list(errors: &mut FieldErrors, field: &str, items: &[String]) {
    errors.check(field, items.len() <= 100, "at most 100 entries");
    errors.check(
        field,
        items
            .iter()
            .all(|s| !s.trim().is_empty() && s.len() <= 2000),
        "entries must be non-empty and at most 2000 characters",
    );
}

/// Validate a draft; every referenced id must belong to `project_id`.
async fn validate_draft(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    project_id: &str,
    self_id: Option<&str>,
    d: &Draft,
) -> AppResult<()> {
    let mut errors = FieldErrors::new();
    errors.check(
        "kind",
        KINDS.contains(&d.kind.as_str()),
        &format!("must be one of {}", KINDS.join(", ")),
    );
    errors.max_len("basis", &d.basis, 20_000);
    errors.max_len("legal_reference", &d.legal_reference, 1000);
    for (field, v) in [("valid_from", &d.valid_from), ("valid_to", &d.valid_to)] {
        if let Some(v) = v {
            errors.valid_date(field, v);
        }
    }
    if let (Some(a), Some(b)) = (&d.valid_from, &d.valid_to) {
        errors.check("valid_to", a <= b, "must not be before valid_from");
    }
    check_list(&mut errors, "permitted_activities", &d.permitted_activities);
    check_list(&mut errors, "conditions", &d.conditions);
    check_list(&mut errors, "restrictions", &d.restrictions);

    let revision_ok: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM project_revisions WHERE id = ? AND project_id = ?",
    )
    .bind(&d.project_revision_id)
    .bind(project_id)
    .fetch_one(&mut **tx)
    .await?;
    errors.check(
        "project_revision_id",
        revision_ok > 0,
        "must be a submitted revision of this project",
    );
    if let Some(s) = &d.supersedes_id {
        let ok: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM decisions WHERE id = ? AND project_id = ? AND status = 'issued'",
        )
        .bind(s)
        .bind(project_id)
        .fetch_one(&mut **tx)
        .await?;
        errors.check(
            "supersedes_id",
            ok > 0 && Some(s.as_str()) != self_id,
            "must be an issued decision of this project",
        );
    }
    if let Some(c) = &d.change_request_id {
        let ok: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM change_requests WHERE id = ? AND project_id = ?",
        )
        .bind(c)
        .bind(project_id)
        .fetch_one(&mut **tx)
        .await?;
        errors.check(
            "change_request_id",
            ok > 0,
            "must be a change request of this project",
        );
    }
    errors.finish()?;
    if let Some(dv) = &d.document_version_id {
        authz::ensure_document_version_in_project(&mut **tx, dv, project_id).await?;
        let category: String = sqlx::query_scalar(
            "SELECT d.category FROM document_versions dv JOIN documents d ON d.id = dv.document_id
             WHERE dv.id = ?",
        )
        .bind(dv)
        .fetch_one(&mut **tx)
        .await?;
        if category != "decision" {
            let mut fields = HashMap::new();
            fields.insert(
                "document_version_id".to_string(),
                "the signed copy must be a document of category decision".to_string(),
            );
            return Err(AppError::Validation { fields });
        }
    }
    Ok(())
}

fn decision_event(
    actor: &Actor,
    action: &str,
    id: &str,
    project_id: &str,
    visibility: &str,
    summary: String,
) -> AuditEvent {
    AuditEvent {
        actor_id: Some(actor.user_id.clone()),
        actor_label: actor.name.clone(),
        action: action.into(),
        entity_type: "decision".into(),
        entity_id: id.into(),
        project_id: Some(project_id.into()),
        visibility: visibility.into(),
        summary,
        before: None,
        after: None,
        reason: None,
    }
}

fn clean_list(v: Option<Vec<String>>) -> Vec<String> {
    v.unwrap_or_default()
        .into_iter()
        .map(|s| s.trim().to_string())
        .collect()
}

fn clean_opt(v: Option<String>) -> Option<String> {
    v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

async fn create_decision(
    State(state): State<AppState>,
    actor: Actor,
    Path(project_id): Path<String>,
    Json(req): Json<CreateDecisionRequest>,
) -> AppResult<impl IntoResponse> {
    require_project_access(&state, &actor, &project_id).await?;
    require_drafter(&actor)?;
    let draft = Draft {
        kind: req.kind.trim().to_string(),
        project_revision_id: req.project_revision_id.trim().to_string(),
        basis: req.basis.unwrap_or_default().trim().to_string(),
        legal_reference: req.legal_reference.unwrap_or_default().trim().to_string(),
        valid_from: clean_opt(req.valid_from),
        valid_to: clean_opt(req.valid_to),
        permitted_activities: clean_list(req.permitted_activities),
        conditions: clean_list(req.conditions),
        restrictions: clean_list(req.restrictions),
        supersedes_id: clean_opt(req.supersedes_id),
        change_request_id: clean_opt(req.change_request_id),
        document_version_id: clean_opt(req.document_version_id),
    };

    let mut tx = db::begin_immediate(&state.pool).await?;
    validate_draft(&mut tx, &project_id, None, &draft).await?;
    let id = new_id();
    sqlx::query(
        "INSERT INTO decisions
         (id, project_id, project_revision_id, kind, status, basis, legal_reference, valid_from, valid_to,
          permitted_activities_json, conditions_json, restrictions_json, sites_snapshot_json,
          document_version_id, supersedes_id, change_request_id, drafted_by, created_at)
         VALUES (?, ?, ?, ?, 'draft', ?, ?, ?, ?, ?, ?, ?, '[]', ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&project_id)
    .bind(&draft.project_revision_id)
    .bind(&draft.kind)
    .bind(&draft.basis)
    .bind(&draft.legal_reference)
    .bind(&draft.valid_from)
    .bind(&draft.valid_to)
    .bind(json!(draft.permitted_activities).to_string())
    .bind(json!(draft.conditions).to_string())
    .bind(json!(draft.restrictions).to_string())
    .bind(&draft.document_version_id)
    .bind(&draft.supersedes_id)
    .bind(&draft.change_request_id)
    .bind(&actor.user_id)
    .bind(now_rfc3339())
    .execute(&mut *tx)
    .await?;
    audit::record(
        &mut tx,
        decision_event(
            &actor,
            "decision.drafted",
            &id,
            &project_id,
            "internal",
            format!("{} drafted a {} decision", actor.name, draft.kind),
        ),
    )
    .await?;
    tx.commit().await?;
    let row = load_row(&state.pool, &id).await?;
    Ok((StatusCode::CREATED, Json(decision_dto(row, true)?)))
}

async fn patch_decision(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<String>,
    Json(req): Json<PatchDecisionRequest>,
) -> AppResult<Json<DecisionDto>> {
    let current = load_row(&state.pool, &id).await?;
    require_project_access(&state, &actor, &current.project_id).await?;
    require_drafter(&actor)?;

    let mut tx = db::begin_immediate(&state.pool).await?;
    let current = load_row(&mut *tx, &id).await?;
    let project_id = current.project_id.clone();
    if current.status == "issued" {
        // Issued decisions are immutable. The one exception: attaching the
        // uploaded signed copy once, when none is attached yet.
        let only_signed_copy = req.kind.is_none()
            && req.project_revision_id.is_none()
            && req.basis.is_none()
            && req.legal_reference.is_none()
            && req.valid_from.is_none()
            && req.valid_to.is_none()
            && req.permitted_activities.is_none()
            && req.conditions.is_none()
            && req.restrictions.is_none()
            && req.supersedes_id.is_none()
            && req.change_request_id.is_none();
        let dv = match (&req.document_version_id, &current.document_version_id) {
            (Some(Some(dv)), None) if only_signed_copy => dv.clone(),
            _ => {
                return Err(AppError::conflict(
                    "decision_issued",
                    "issued decisions cannot be changed",
                ));
            }
        };
        let draft = Draft {
            kind: current.kind.clone(),
            project_revision_id: current.project_revision_id.clone(),
            basis: current.basis.clone(),
            legal_reference: current.legal_reference.clone(),
            valid_from: current.valid_from.clone(),
            valid_to: current.valid_to.clone(),
            permitted_activities: Vec::new(),
            conditions: Vec::new(),
            restrictions: Vec::new(),
            supersedes_id: None,
            change_request_id: None,
            document_version_id: Some(dv.clone()),
        };
        validate_draft(&mut tx, &project_id, Some(&id), &draft).await?;
        sqlx::query("UPDATE decisions SET document_version_id = ? WHERE id = ?")
            .bind(&dv)
            .bind(&id)
            .execute(&mut *tx)
            .await?;
        audit::record(
            &mut tx,
            decision_event(
                &actor,
                "decision.signed_copy_attached",
                &id,
                &project_id,
                "shared",
                format!("{} attached the signed copy of the decision", actor.name),
            ),
        )
        .await?;
    } else {
        let draft = Draft {
            kind: req
                .kind
                .map(|k| k.trim().to_string())
                .unwrap_or(current.kind),
            project_revision_id: req
                .project_revision_id
                .map(|r| r.trim().to_string())
                .unwrap_or(current.project_revision_id),
            basis: req
                .basis
                .map(|b| b.trim().to_string())
                .unwrap_or(current.basis),
            legal_reference: req
                .legal_reference
                .map(|b| b.trim().to_string())
                .unwrap_or(current.legal_reference),
            valid_from: req.valid_from.map(clean_opt).unwrap_or(current.valid_from),
            valid_to: req.valid_to.map(clean_opt).unwrap_or(current.valid_to),
            permitted_activities: match req.permitted_activities {
                Some(v) => clean_list(Some(v)),
                None => string_list(&current.permitted_activities_json)?,
            },
            conditions: match req.conditions {
                Some(v) => clean_list(Some(v)),
                None => string_list(&current.conditions_json)?,
            },
            restrictions: match req.restrictions {
                Some(v) => clean_list(Some(v)),
                None => string_list(&current.restrictions_json)?,
            },
            supersedes_id: req
                .supersedes_id
                .map(clean_opt)
                .unwrap_or(current.supersedes_id),
            change_request_id: req
                .change_request_id
                .map(clean_opt)
                .unwrap_or(current.change_request_id),
            document_version_id: req
                .document_version_id
                .map(clean_opt)
                .unwrap_or(current.document_version_id),
        };
        validate_draft(&mut tx, &project_id, Some(&id), &draft).await?;
        sqlx::query(
            "UPDATE decisions SET kind = ?, project_revision_id = ?, basis = ?, legal_reference = ?,
                    valid_from = ?, valid_to = ?, permitted_activities_json = ?, conditions_json = ?,
                    restrictions_json = ?, supersedes_id = ?, change_request_id = ?,
                    document_version_id = ?
             WHERE id = ? AND status = 'draft'",
        )
        .bind(&draft.kind)
        .bind(&draft.project_revision_id)
        .bind(&draft.basis)
        .bind(&draft.legal_reference)
        .bind(&draft.valid_from)
        .bind(&draft.valid_to)
        .bind(json!(draft.permitted_activities).to_string())
        .bind(json!(draft.conditions).to_string())
        .bind(json!(draft.restrictions).to_string())
        .bind(&draft.supersedes_id)
        .bind(&draft.change_request_id)
        .bind(&draft.document_version_id)
        .bind(&id)
        .execute(&mut *tx)
        .await?;
        audit::record(
            &mut tx,
            decision_event(
                &actor,
                "decision.draft_updated",
                &id,
                &project_id,
                "internal",
                format!("{} edited the {} draft", actor.name, draft.kind),
            ),
        )
        .await?;
    }
    tx.commit().await?;
    let row = load_row(&state.pool, &id).await?;
    Ok(Json(decision_dto(row, true)?))
}

// ---------------------------------------------------------------------------
// Issuing
// ---------------------------------------------------------------------------

async fn issue_decision(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<String>,
) -> AppResult<Json<DecisionDto>> {
    // Only decision makers issue — an admin gets 403 (§3 separation of duties).
    authz::require_role(&actor, &["decision_maker"])?;
    let row = load_row(&state.pool, &id).await?;
    require_project_access(&state, &actor, &row.project_id).await?;

    let mut tx = db::begin_immediate(&state.pool).await?;
    let d = load_row(&mut *tx, &id).await?;
    let project_id = d.project_id.clone();
    if d.status != "draft" {
        return Err(AppError::conflict(
            "decision_issued",
            "this decision has already been issued",
        ));
    }
    let open = open_team_action_items(&mut *tx, &project_id).await?;
    if !open.is_empty() {
        return Err(AppError::conflict(
            "open_action_items",
            format!(
                "{} request(s) to the applicant are still open; resolve them before issuing",
                open.len()
            ),
        ));
    }

    let mut errors = FieldErrors::new();
    errors.require("basis", &d.basis, "the basis of the decision is required");
    if d.kind != "refusal" && d.kind != "revocation" {
        errors.check(
            "valid_from",
            d.valid_from.is_some(),
            "validity start is required",
        );
        errors.check("valid_to", d.valid_to.is_some(), "validity end is required");
    }
    errors.finish()?;

    let current: Option<String> = sqlx::query_scalar(&format!(
        "SELECT id FROM decisions WHERE project_id = ? AND status = 'issued'
           AND superseded_by_id IS NULL AND kind IN ({})",
        PERMIT_CHAIN
            .iter()
            .map(|k| format!("'{k}'"))
            .collect::<Vec<_>>()
            .join(",")
    ))
    .bind(&project_id)
    .fetch_optional(&mut *tx)
    .await?;
    let status: String = sqlx::query_scalar("SELECT status FROM projects WHERE id = ?")
        .bind(&project_id)
        .fetch_one(&mut *tx)
        .await?;

    let transition = match d.kind.as_str() {
        "permit" => Some(Action::Approve),
        "refusal" => Some(Action::Refuse),
        _ => None,
    };
    match d.kind.as_str() {
        "refusal" if d.supersedes_id.is_some() => {
            return Err(AppError::conflict(
                "invalid_supersession",
                "a refusal cannot supersede another decision",
            ));
        }
        "permit" | "refusal" => {
            if current.is_some() && d.supersedes_id != current {
                return Err(AppError::conflict(
                    "supersedes_required",
                    "a permit is already in force; the new decision must supersede it",
                ));
            }
        }
        _ => {
            // amendment / extension / revocation of the permit in force.
            if status != "approved" {
                return Err(AppError::conflict(
                    "invalid_transition",
                    format!(
                        "an {} can only be issued for an approved project (project is {})",
                        d.kind,
                        status.replace('_', " ")
                    ),
                ));
            }
            if current.is_none() || d.supersedes_id != current {
                return Err(AppError::conflict(
                    "supersedes_required",
                    "the decision must supersede the permit currently in force",
                ));
            }
        }
    }

    let sites_snapshot = sites::project_sites(&mut *tx, &project_id)
        .await?
        .iter()
        .map(sites::site_snapshot_value)
        .collect::<AppResult<Vec<_>>>()?;
    let now = now_rfc3339();
    sqlx::query(
        "UPDATE decisions SET status = 'issued', issued_by = ?, issued_at = ?, sites_snapshot_json = ?
         WHERE id = ? AND status = 'draft'",
    )
    .bind(&actor.user_id)
    .bind(&now)
    .bind(Value::Array(sites_snapshot).to_string())
    .bind(&id)
    .execute(&mut *tx)
    .await?;
    if let Some(old) = &d.supersedes_id {
        let res = sqlx::query(
            "UPDATE decisions SET superseded_by_id = ?
             WHERE id = ? AND project_id = ? AND status = 'issued' AND superseded_by_id IS NULL",
        )
        .bind(&id)
        .bind(old)
        .bind(&project_id)
        .execute(&mut *tx)
        .await?;
        if res.rows_affected() != 1 {
            return Err(AppError::conflict(
                "decision_superseded",
                "the superseded decision was already superseded",
            ));
        }
    }
    if let Some(action) = transition {
        projects::transition(&mut tx, &project_id, action, &actor, None).await?;
    }
    let mut event = decision_event(
        &actor,
        "decision.issued",
        &id,
        &project_id,
        "shared",
        format!("{} issued a {} decision", actor.name, d.kind),
    );
    event.after = Some(json!({
        "kind": d.kind, "valid_from": d.valid_from, "valid_to": d.valid_to,
        "supersedes_id": d.supersedes_id, "conditions": string_list(&d.conditions_json)?,
    }));
    audit::record(&mut tx, event).await?;

    let title: String = sqlx::query_scalar("SELECT title FROM projects WHERE id = ?")
        .bind(&project_id)
        .fetch_one(&mut *tx)
        .await?;
    let headline = match d.kind.as_str() {
        "permit" => format!("Permit issued: {title}"),
        "refusal" => format!("Application refused: {title}"),
        k => format!("Permit {k} issued: {title}"),
    };
    let conditions = string_list(&d.conditions_json)?;
    let body = if conditions.is_empty() {
        format!("{} issued the decision.", actor.name)
    } else {
        format!(
            "{} issued the decision with {} condition(s). Please read them before your fieldwork.",
            actor.name,
            conditions.len()
        )
    };
    let link = format!("/app/projects/{project_id}/decisions");
    let mut recipients = notify::team_member_ids(&mut *tx, &project_id).await?;
    recipients.extend(notify::coordinator_ids(&mut *tx).await?);
    recipients.retain(|u| u != &actor.user_id);
    notify::notify_users(
        &mut tx,
        recipients,
        "decision_issued",
        &headline,
        &body,
        &link,
        Some(&project_id),
    )
    .await?;
    tx.commit().await?;

    let row = load_row(&state.pool, &id).await?;
    Ok(Json(decision_dto(row, true)?))
}

// ---------------------------------------------------------------------------
// Printable document
// ---------------------------------------------------------------------------

pub fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

fn html_list(items: &[String], empty: &str) -> String {
    if items.is_empty() {
        return format!("<p class=\"muted\">{}</p>", html_escape(empty));
    }
    let lis: String = items
        .iter()
        .map(|i| format!("<li>{}</li>", html_escape(i)))
        .collect();
    format!("<ol>{lis}</ol>")
}

fn kind_title(kind: &str) -> &'static str {
    match kind {
        "permit" => "Research Permit",
        "refusal" => "Refusal of Application",
        "amendment" => "Amendment to Research Permit",
        "extension" => "Extension of Research Permit",
        "revocation" => "Revocation of Research Permit",
        _ => "Decision",
    }
}

fn site_line(site: &Value) -> String {
    let name = site.get("name").and_then(Value::as_str).unwrap_or("Site");
    let geometry = site.get("geometry").cloned().unwrap_or(Value::Null);
    let coords = match geometry.get("type").and_then(Value::as_str) {
        Some("Point") => geometry
            .get("coordinates")
            .and_then(Value::as_array)
            .map(|c| {
                format!(
                    "point {:.5}, {:.5} (lat, lng)",
                    c.get(1).and_then(Value::as_f64).unwrap_or_default(),
                    c.first().and_then(Value::as_f64).unwrap_or_default()
                )
            })
            .unwrap_or_default(),
        Some("Polygon") => {
            let n = |k: &str| site.get(k).and_then(Value::as_f64).unwrap_or_default();
            format!(
                "area {:.4}…{:.4} lat, {:.4}…{:.4} lng",
                n("min_lat"),
                n("max_lat"),
                n("min_lng"),
                n("max_lng")
            )
        }
        _ => String::new(),
    };
    let generalized = if site.get("generalized").and_then(Value::as_bool) == Some(true) {
        " — location generalized"
    } else {
        ""
    };
    format!(
        "<li><strong>{}</strong> {}{}</li>",
        html_escape(name),
        html_escape(&coords),
        generalized
    )
}

async fn decision_document(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<String>,
) -> AppResult<impl IntoResponse> {
    let (row, precise) = readable_decision(&state, &actor, &id).await?;
    let (reference, title, organisation): (Option<String>, String, String) =
        sqlx::query_as("SELECT reference, title, organisation FROM projects WHERE id = ?")
            .bind(&row.project_id)
            .fetch_one(&state.pool)
            .await?;
    let org_name: String = sqlx::query_scalar(
        "SELECT COALESCE((SELECT value FROM settings WHERE key = 'organisation_name'),
                         'Pitcairn Islands Marine Science Base')",
    )
    .fetch_one(&state.pool)
    .await?;
    let lead: Option<String> = sqlx::query_scalar(
        "SELECT u.name FROM project_members pm JOIN users u ON u.id = pm.user_id
         WHERE pm.project_id = ? AND pm.role = 'lead' AND pm.removed_at IS NULL",
    )
    .bind(&row.project_id)
    .fetch_optional(&state.pool)
    .await?;
    let superseded_note = match &row.superseded_by_id {
        Some(s) => format!(
            "<p class=\"banner\">This decision has been superseded by decision {}.</p>",
            html_escape(s)
        ),
        None => String::new(),
    };
    let draft_note = if row.status == "draft" {
        "<p class=\"banner\">DRAFT — not issued, not valid.</p>"
    } else {
        ""
    };
    let dto = decision_dto(row, precise)?;
    let validity = match (&dto.valid_from, &dto.valid_to) {
        (Some(a), Some(b)) => format!("{a} to {b}"),
        (Some(a), None) => format!("from {a}"),
        (None, Some(b)) => format!("until {b}"),
        (None, None) => "—".into(),
    };
    let sites_html = if dto.sites.is_empty() {
        "<p class=\"muted\">No sites recorded.</p>".to_string()
    } else {
        format!(
            "<ul>{}</ul>",
            dto.sites.iter().map(site_line).collect::<String>()
        )
    };
    let supersedes = dto
        .supersedes_id
        .as_deref()
        .map(|s| format!("<dt>Supersedes</dt><dd>Decision {}</dd>", html_escape(s)))
        .unwrap_or_default();
    let reference = reference.unwrap_or_else(|| "—".into());
    let html = format!(
        r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8">
<title>{kind_title} — {reference}</title>
<style>
  body {{ font-family: Georgia, "Times New Roman", serif; color: #111; max-width: 46rem; margin: 2rem auto; padding: 0 1.5rem; line-height: 1.45; }}
  header {{ border-bottom: 2px solid #111; margin-bottom: 1.25rem; padding-bottom: .5rem; }}
  .org {{ font-size: .95rem; letter-spacing: .04em; text-transform: uppercase; }}
  h1 {{ font-size: 1.6rem; margin: .4rem 0 .2rem; }}
  h2 {{ font-size: 1.1rem; margin-top: 1.4rem; border-bottom: 1px solid #999; }}
  dl {{ display: grid; grid-template-columns: 11rem 1fr; gap: .25rem 1rem; }}
  dt {{ font-weight: bold; }} dd {{ margin: 0; }}
  .muted {{ color: #555; font-style: italic; }}
  .banner {{ border: 2px solid #900; color: #900; padding: .4rem .6rem; font-weight: bold; }}
  .sign {{ margin-top: 2.5rem; display: grid; grid-template-columns: 1fr 1fr; gap: 2rem; }}
  .line {{ border-top: 1px solid #111; padding-top: .25rem; font-size: .9rem; }}
  @media print {{ body {{ margin: 0; max-width: none; }} .noprint {{ display: none; }} }}
</style></head>
<body>
<header><div class="org">{org}</div><h1>{kind_title}</h1><div>Reference <strong>{reference}</strong></div></header>
{draft_note}{superseded_note}
<dl>
  <dt>Project</dt><dd>{title}</dd>
  <dt>Organisation</dt><dd>{organisation}</dd>
  <dt>Lead researcher</dt><dd>{lead}</dd>
  <dt>Decision</dt><dd>{kind}</dd>
  <dt>Validity</dt><dd>{validity}</dd>
  <dt>Application revision</dt><dd>#{revision}</dd>
  {supersedes}
</dl>
<h2>Basis</h2><p>{basis}</p>
<h2>Legal reference</h2><p>{legal}</p>
<h2>Permitted activities</h2>{activities}
<h2>Conditions</h2>{conditions}
<h2>Restrictions</h2>{restrictions}
<h2>Sites</h2>{sites}
<div class="sign">
  <div class="line">Issued by: {issued_by}</div>
  <div class="line">Issued at: {issued_at}</div>
</div>
</body></html>
"#,
        kind_title = kind_title(&dto.kind),
        reference = html_escape(&reference),
        org = html_escape(&org_name),
        title = html_escape(&title),
        organisation = html_escape(&organisation),
        lead = html_escape(lead.as_deref().unwrap_or("—")),
        kind = html_escape(&dto.kind),
        validity = html_escape(&validity),
        revision = dto.revision_number,
        basis = html_escape(&dto.basis),
        legal = html_escape(if dto.legal_reference.is_empty() {
            "—"
        } else {
            &dto.legal_reference
        }),
        activities = html_list(&dto.permitted_activities, "None specified."),
        conditions = html_list(&dto.conditions, "No conditions."),
        restrictions = html_list(&dto.restrictions, "No restrictions."),
        sites = sites_html,
        issued_by = html_escape(dto.issued_by_name.as_deref().unwrap_or("— (draft)")),
        issued_at = html_escape(dto.issued_at.as_deref().unwrap_or("—")),
    );
    Ok((
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (header::CACHE_CONTROL, "private, no-store"),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
            (
                header::CONTENT_SECURITY_POLICY,
                "default-src 'none'; style-src 'unsafe-inline'",
            ),
        ],
        html,
    ))
}
