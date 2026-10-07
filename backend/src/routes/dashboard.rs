//! `GET /dashboard` — role-specific sections (§5). A user with several roles
//! gets all of their sections; researcher sections appear for anyone who is
//! an active member of at least one project (or has no staff-type role).

use std::collections::BTreeMap;

use axum::Json;
use axum::extract::State;
use axum::routing::{Router, get};
use sqlx::FromRow;

use crate::AppState;
use crate::authz::Actor;
use crate::dto::{DashboardItemDto, DashboardResponse};
use crate::error::AppResult;

pub fn router() -> Router<AppState> {
    Router::new().route("/dashboard", get(get_dashboard))
}

fn today() -> String {
    chrono::Utc::now().format("%Y-%m-%d").to_string()
}

fn in_days(days: i64) -> String {
    (chrono::Utc::now() + chrono::Duration::days(days))
        .format("%Y-%m-%d")
        .to_string()
}

fn project_link(project_id: &str) -> String {
    format!("/app/projects/{project_id}")
}

fn item(
    kind: &str,
    title: impl Into<String>,
    subtitle: impl Into<String>,
    project_id: Option<String>,
    project_reference: Option<String>,
    due_date: Option<String>,
    link: String,
) -> DashboardItemDto {
    DashboardItemDto {
        kind: kind.into(),
        title: title.into(),
        subtitle: subtitle.into(),
        project_id,
        project_reference,
        due_date,
        link,
    }
}

#[derive(FromRow)]
struct DashRow {
    project_id: Option<String>,
    project_title: Option<String>,
    reference: Option<String>,
    due: Option<String>,
    extra: Option<String>,
}

async fn get_dashboard(
    State(state): State<AppState>,
    actor: Actor,
) -> AppResult<Json<DashboardResponse>> {
    let mut sections: BTreeMap<String, Vec<DashboardItemDto>> = BTreeMap::new();

    let member_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM project_members WHERE user_id = ? AND removed_at IS NULL",
    )
    .bind(&actor.user_id)
    .fetch_one(&state.pool)
    .await?;
    let is_researcher = member_count > 0 || (!actor.is_staff() && !actor.is_expert());

    if is_researcher {
        researcher_sections(&state, &actor, &mut sections).await?;
    }
    if actor.is_coordinator() {
        coordinator_sections(&state, &mut sections).await?;
    }
    if actor.is_expert() {
        expert_sections(&state, &actor, &mut sections).await?;
    }
    if actor.has_role("base_manager") {
        base_manager_sections(&state, &mut sections).await?;
    }
    if actor.has_role("finance") {
        finance_sections(&state, &mut sections).await?;
    }
    if actor.has_role("provider") {
        provider_sections(&state, &actor, &mut sections).await?;
    }

    Ok(Json(DashboardResponse { sections }))
}

// ---------------------------------------------------------------------------
// Researcher — "my projects" view of the world
// ---------------------------------------------------------------------------

async fn researcher_sections(
    state: &AppState,
    actor: &Actor,
    sections: &mut BTreeMap<String, Vec<DashboardItemDto>>,
) -> AppResult<()> {
    let today = today();

    // my_projects: every project where I'm an active member.
    let rows: Vec<DashRow> = sqlx::query_as(
        "SELECT p.id, p.id AS project_id, p.title AS project_title, p.reference,
                NULL AS due, p.status AS extra
         FROM projects p
         JOIN project_members pm ON pm.project_id = p.id
         WHERE pm.user_id = ? AND pm.removed_at IS NULL
         ORDER BY p.created_at DESC LIMIT 50",
    )
    .bind(&actor.user_id)
    .fetch_all(&state.pool)
    .await?;
    sections.insert(
        "my_projects".into(),
        rows.into_iter()
            .map(|r| {
                let pid = r.project_id.clone().unwrap_or_default();
                item(
                    "project",
                    r.project_title.clone().unwrap_or_default(),
                    format!("status: {}", r.extra.unwrap_or_default()),
                    Some(pid.clone()),
                    r.reference,
                    None,
                    project_link(&pid),
                )
            })
            .collect(),
    );

    // needs_reply: open action items addressed to my teams + submissions of
    // my projects that were sent back for changes.
    let rows: Vec<DashRow> = sqlx::query_as(
        "SELECT ai.id, ai.project_id, p.title AS project_title, p.reference,
                NULL AS due, ai.title AS extra
         FROM action_items ai
         JOIN projects p ON p.id = ai.project_id
         JOIN project_members pm ON pm.project_id = p.id
         WHERE pm.user_id = ? AND pm.removed_at IS NULL
           AND ai.status = 'open' AND ai.addressed_to = 'team'
         ORDER BY ai.created_at DESC LIMIT 50",
    )
    .bind(&actor.user_id)
    .fetch_all(&state.pool)
    .await?;
    let mut needs_reply: Vec<DashboardItemDto> = rows
        .into_iter()
        .map(|r| {
            let pid = r.project_id.clone().unwrap_or_default();
            item(
                "action_item",
                r.extra.unwrap_or_default(),
                r.project_title.clone().unwrap_or_default(),
                Some(pid.clone()),
                r.reference,
                None,
                format!("/app/projects/{pid}/messages"),
            )
        })
        .collect();
    let rows: Vec<DashRow> = sqlx::query_as(
        "SELECT s.id, d.project_id, p.title AS project_title, p.reference,
                d.due_date AS due, d.title AS extra
         FROM deliverable_submissions s
         JOIN deliverables d ON d.id = s.deliverable_id
         JOIN projects p ON p.id = d.project_id
         JOIN project_members pm ON pm.project_id = p.id
         WHERE pm.user_id = ? AND pm.removed_at IS NULL
           AND s.status = 'changes_requested' AND d.status = 'changes_requested'
           AND s.number = (SELECT MAX(s2.number) FROM deliverable_submissions s2
                           WHERE s2.deliverable_id = d.id)
         ORDER BY s.created_at DESC LIMIT 50",
    )
    .bind(&actor.user_id)
    .fetch_all(&state.pool)
    .await?;
    needs_reply.extend(rows.into_iter().map(|r| {
        let pid = r.project_id.clone().unwrap_or_default();
        item(
            "submission",
            format!("Changes requested: {}", r.extra.unwrap_or_default()),
            r.project_title.clone().unwrap_or_default(),
            Some(pid.clone()),
            r.reference,
            r.due,
            format!("/app/projects/{pid}/results"),
        )
    }));
    sections.insert("needs_reply".into(), needs_reply);

    // decisions: issued decisions on my projects (never a bare status — the
    // subtitle carries kind and validity).
    let rows: Vec<DashRow> = sqlx::query_as(
        "SELECT dc.id, dc.project_id, p.title AS project_title, p.reference,
                dc.valid_to AS due,
                (dc.kind || CASE WHEN dc.superseded_by_id IS NOT NULL THEN ' (superseded)' ELSE '' END) AS extra
         FROM decisions dc
         JOIN projects p ON p.id = dc.project_id
         JOIN project_members pm ON pm.project_id = p.id
         WHERE pm.user_id = ? AND pm.removed_at IS NULL AND dc.status = 'issued'
         ORDER BY dc.issued_at DESC LIMIT 50",
    )
    .bind(&actor.user_id)
    .fetch_all(&state.pool)
    .await?;
    sections.insert(
        "decisions".into(),
        rows.into_iter()
            .map(|r| {
                let pid = r.project_id.clone().unwrap_or_default();
                item(
                    "decision",
                    format!(
                        "{} — {}",
                        r.extra.unwrap_or_default(),
                        r.project_title.clone().unwrap_or_default()
                    ),
                    "issued decision".to_string(),
                    Some(pid.clone()),
                    r.reference,
                    r.due,
                    format!("/app/projects/{pid}/decisions"),
                )
            })
            .collect(),
    );

    // trips_and_invoices: upcoming trips and unpaid issued invoices on my
    // projects.
    let rows: Vec<DashRow> = sqlx::query_as(
        "SELECT t.id, t.project_id, p.title AS project_title, p.reference,
                t.arrive_date AS due, (t.status || ' · ' || t.depart_date) AS extra
         FROM trips t
         JOIN projects p ON p.id = t.project_id
         JOIN project_members pm ON pm.project_id = p.id
         WHERE pm.user_id = ? AND pm.removed_at IS NULL
           AND t.status IN ('planned','confirmed') AND t.depart_date >= ?
         ORDER BY t.arrive_date LIMIT 50",
    )
    .bind(&actor.user_id)
    .bind(&today)
    .fetch_all(&state.pool)
    .await?;
    let mut trips_and_invoices: Vec<DashboardItemDto> = rows
        .into_iter()
        .map(|r| {
            let pid = r.project_id.clone().unwrap_or_default();
            item(
                "trip",
                r.project_title.clone().unwrap_or_default(),
                format!("trip {}", r.extra.unwrap_or_default()),
                Some(pid.clone()),
                r.reference,
                r.due,
                format!("/app/projects/{pid}/trips"),
            )
        })
        .collect();
    let rows: Vec<DashRow> = sqlx::query_as(
        "SELECT i.id, i.project_id, p.title AS project_title, p.reference,
                i.due_date AS due,
                (i.number || ' · ' || i.currency || ' ' ||
                    CAST(COALESCE((SELECT SUM(amount_cents) FROM invoice_lines WHERE invoice_id = i.id),0)/100 AS TEXT)) AS extra
         FROM invoices i
         JOIN projects p ON p.id = i.project_id
         JOIN project_members pm ON pm.project_id = p.id
         WHERE pm.user_id = ? AND pm.removed_at IS NULL AND i.status = 'issued'
           AND COALESCE((SELECT SUM(CASE WHEN kind='payment' THEN amount_cents ELSE -amount_cents END)
                         FROM payments WHERE invoice_id = i.id AND status = 'verified'), 0)
               < COALESCE((SELECT SUM(amount_cents) FROM invoice_lines WHERE invoice_id = i.id), 0)
         ORDER BY i.due_date LIMIT 50",
    )
    .bind(&actor.user_id)
    .fetch_all(&state.pool)
    .await?;
    trips_and_invoices.extend(rows.into_iter().map(|r| {
        let pid = r.project_id.clone().unwrap_or_default();
        item(
            "invoice",
            format!("Invoice {}", r.extra.unwrap_or_default()),
            r.project_title.clone().unwrap_or_default(),
            Some(pid.clone()),
            r.reference,
            r.due,
            format!("/app/projects/{pid}/invoices"),
        )
    }));
    sections.insert("trips_and_invoices".into(), trips_and_invoices);

    // results_due: deliverables of my projects that are not accepted, waived
    // or cancelled, with their due dates — overdue flagged, still shown after
    // trips ended.
    let rows: Vec<DashRow> = sqlx::query_as(
        "SELECT d.id, d.project_id, p.title AS project_title, p.reference,
                d.due_date AS due, d.title AS extra
         FROM deliverables d
         JOIN projects p ON p.id = d.project_id
         JOIN project_members pm ON pm.project_id = p.id
         WHERE pm.user_id = ? AND pm.removed_at IS NULL
           AND d.status NOT IN ('accepted','waived','cancelled')
         ORDER BY d.due_date LIMIT 100",
    )
    .bind(&actor.user_id)
    .fetch_all(&state.pool)
    .await?;
    sections.insert(
        "results_due".into(),
        rows.into_iter()
            .map(|r| {
                let pid = r.project_id.clone().unwrap_or_default();
                let overdue = r
                    .due
                    .as_deref()
                    .map(|d| d < today.as_str())
                    .unwrap_or(false);
                let project_title = r.project_title.clone().unwrap_or_default();
                item(
                    if overdue {
                        "overdue_deliverable"
                    } else {
                        "deliverable"
                    },
                    r.extra.unwrap_or_default(),
                    if overdue {
                        format!(
                            "Overdue since {} · {project_title}",
                            r.due.clone().unwrap_or_default()
                        )
                    } else {
                        format!(
                            "Due {} · {project_title}",
                            r.due.clone().unwrap_or_default()
                        )
                    },
                    Some(pid.clone()),
                    r.reference,
                    r.due,
                    format!("/app/projects/{pid}/results"),
                )
            })
            .collect(),
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Coordinator
// ---------------------------------------------------------------------------

async fn coordinator_sections(
    state: &AppState,
    sections: &mut BTreeMap<String, Vec<DashboardItemDto>>,
) -> AppResult<()> {
    let today = today();
    let soon = in_days(30);

    // new_applications / waiting_for_applicant
    for (section, status) in [
        ("new_applications", "submitted"),
        ("waiting_for_applicant", "changes_requested"),
    ] {
        let sql = format!(
            "SELECT p.id, p.id AS project_id, p.title AS project_title, p.reference,
                    NULL AS due, p.organisation AS extra
             FROM projects p WHERE p.status = '{status}'
             ORDER BY p.created_at DESC LIMIT 50"
        );
        let rows: Vec<DashRow> = sqlx::query_as(&sql).fetch_all(&state.pool).await?;
        sections.insert(
            section.into(),
            rows.into_iter()
                .map(|r| {
                    let pid = r.project_id.clone().unwrap_or_default();
                    item(
                        "project",
                        r.project_title.clone().unwrap_or_default(),
                        r.extra.unwrap_or_default(),
                        Some(pid.clone()),
                        r.reference,
                        None,
                        project_link(&pid),
                    )
                })
                .collect(),
        );
    }

    // with_experts: in review and at least one live review assignment.
    let rows: Vec<DashRow> = sqlx::query_as(
        "SELECT p.id, p.id AS project_id, p.title AS project_title, p.reference,
                NULL AS due,
                CAST((SELECT COUNT(*) FROM review_assignments ra
                  WHERE ra.project_id = p.id AND ra.status IN ('invited','accepted')) AS TEXT) AS extra
         FROM projects p
         WHERE p.status = 'in_review'
           AND EXISTS (SELECT 1 FROM review_assignments ra
                       WHERE ra.project_id = p.id AND ra.status IN ('invited','accepted'))
         ORDER BY p.created_at DESC LIMIT 50",
    )
    .fetch_all(&state.pool)
    .await?;
    sections.insert(
        "with_experts".into(),
        rows.into_iter()
            .map(|r| {
                let pid = r.project_id.clone().unwrap_or_default();
                item(
                    "project",
                    r.project_title.clone().unwrap_or_default(),
                    format!("{} review(s) in flight", r.extra.unwrap_or_default()),
                    Some(pid.clone()),
                    r.reference,
                    None,
                    format!("/app/projects/{pid}/review"),
                )
            })
            .collect(),
    );

    // arriving_soon: confirmed/planned trips arriving within 30 days.
    let rows: Vec<DashRow> = sqlx::query_as(
        "SELECT t.id, t.project_id, p.title AS project_title, p.reference,
                t.arrive_date AS due, (t.status || ' · departs ' || t.depart_date) AS extra
         FROM trips t JOIN projects p ON p.id = t.project_id
         WHERE t.status IN ('planned','confirmed')
           AND t.arrive_date >= ? AND t.arrive_date <= ?
         ORDER BY t.arrive_date LIMIT 50",
    )
    .bind(&today)
    .bind(&soon)
    .fetch_all(&state.pool)
    .await?;
    sections.insert(
        "arriving_soon".into(),
        rows.into_iter()
            .map(|r| {
                let pid = r.project_id.clone().unwrap_or_default();
                item(
                    "trip",
                    r.project_title.clone().unwrap_or_default(),
                    format!(
                        "arrives {} · {}",
                        r.due.clone().unwrap_or_default(),
                        r.extra.unwrap_or_default()
                    ),
                    Some(pid.clone()),
                    r.reference,
                    r.due,
                    format!("/app/projects/{pid}/trips"),
                )
            })
            .collect(),
    );

    // results_to_check: submissions received, not yet reviewed.
    let rows: Vec<DashRow> = sqlx::query_as(
        "SELECT s.id, d.project_id, p.title AS project_title, p.reference,
                d.due_date AS due, d.title AS extra
         FROM deliverable_submissions s
         JOIN deliverables d ON d.id = s.deliverable_id
         JOIN projects p ON p.id = d.project_id
         WHERE s.status = 'received'
         ORDER BY s.created_at DESC LIMIT 50",
    )
    .fetch_all(&state.pool)
    .await?;
    sections.insert(
        "results_to_check".into(),
        rows.into_iter()
            .map(|r| {
                let pid = r.project_id.clone().unwrap_or_default();
                item(
                    "submission",
                    format!("Submission received: {}", r.extra.unwrap_or_default()),
                    r.project_title.clone().unwrap_or_default(),
                    Some(pid.clone()),
                    r.reference,
                    r.due,
                    format!("/app/projects/{pid}/results"),
                )
            })
            .collect(),
    );

    // overdue_results: deliverables past due + unavailable external links.
    let rows: Vec<DashRow> = sqlx::query_as(
        "SELECT d.id, d.project_id, p.title AS project_title, p.reference,
                d.due_date AS due, d.title AS extra
         FROM deliverables d JOIN projects p ON p.id = d.project_id
         WHERE d.status NOT IN ('accepted','waived','cancelled') AND d.due_date < ?
         ORDER BY d.due_date LIMIT 100",
    )
    .bind(&today)
    .fetch_all(&state.pool)
    .await?;
    let mut overdue: Vec<DashboardItemDto> = rows
        .into_iter()
        .map(|r| {
            let pid = r.project_id.clone().unwrap_or_default();
            item(
                "deliverable",
                format!("Overdue: {}", r.extra.unwrap_or_default()),
                r.project_title.clone().unwrap_or_default(),
                Some(pid.clone()),
                r.reference,
                r.due,
                format!("/app/projects/{pid}/results"),
            )
        })
        .collect();
    let rows: Vec<DashRow> = sqlx::query_as(
        "SELECT el.id, d.project_id, p.title AS project_title, p.reference,
                d.due_date AS due, el.url AS extra
         FROM external_links el
         JOIN deliverable_submissions s ON s.id = el.submission_id
         JOIN deliverables d ON d.id = s.deliverable_id
         JOIN projects p ON p.id = d.project_id
         WHERE el.available = 0
         ORDER BY el.created_at DESC LIMIT 50",
    )
    .fetch_all(&state.pool)
    .await?;
    overdue.extend(rows.into_iter().map(|r| {
        let pid = r.project_id.clone().unwrap_or_default();
        item(
            "external_link",
            format!("Link unavailable: {}", r.extra.unwrap_or_default()),
            r.project_title.clone().unwrap_or_default(),
            Some(pid.clone()),
            r.reference,
            r.due,
            format!("/app/projects/{pid}/results"),
        )
    }));
    sections.insert("overdue_results".into(), overdue);

    Ok(())
}

// ---------------------------------------------------------------------------
// Expert
// ---------------------------------------------------------------------------

async fn expert_sections(
    state: &AppState,
    actor: &Actor,
    sections: &mut BTreeMap<String, Vec<DashboardItemDto>>,
) -> AppResult<()> {
    let rows: Vec<DashRow> = sqlx::query_as(
        "SELECT ra.id, ra.project_id, p.title AS project_title, p.reference,
                ra.due_date AS due, ra.status AS extra
         FROM review_assignments ra
         JOIN projects p ON p.id = ra.project_id
         WHERE ra.expert_id = ? AND ra.status IN ('invited','accepted')
         ORDER BY ra.due_date LIMIT 50",
    )
    .bind(&actor.user_id)
    .fetch_all(&state.pool)
    .await?;
    sections.insert(
        "assigned_reviews".into(),
        rows.into_iter()
            .map(|r| {
                let pid = r.project_id.clone().unwrap_or_default();
                item(
                    "review",
                    r.project_title.clone().unwrap_or_default(),
                    format!("review {}", r.extra.unwrap_or_default()),
                    Some(pid.clone()),
                    r.reference,
                    r.due,
                    "/app/reviews".to_string(),
                )
            })
            .collect(),
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Base manager
// ---------------------------------------------------------------------------

async fn base_manager_sections(
    state: &AppState,
    sections: &mut BTreeMap<String, Vec<DashboardItemDto>>,
) -> AppResult<()> {
    let today = today();
    let soon = in_days(30);

    // pending_bookings: requested bookings on resources the base manages
    // (provider-owned resources like boats go to the provider instead).
    let rows: Vec<DashRow> = sqlx::query_as(
        "SELECT b.id, t.project_id, p.title AS project_title, p.reference,
                b.start_date AS due,
                (r.name || ' · ' || b.start_date || ' → ' || b.end_date) AS extra
         FROM bookings b
         JOIN resources r ON r.id = b.resource_id
         JOIN trips t ON t.id = b.trip_id
         JOIN projects p ON p.id = t.project_id
         WHERE b.status = 'requested' AND r.provider_user_id IS NULL
         ORDER BY b.start_date LIMIT 50",
    )
    .fetch_all(&state.pool)
    .await?;
    sections.insert(
        "pending_bookings".into(),
        rows.into_iter()
            .map(|r| {
                let pid = r.project_id.clone().unwrap_or_default();
                item(
                    "booking",
                    r.extra.unwrap_or_default(),
                    r.project_title.clone().unwrap_or_default(),
                    Some(pid.clone()),
                    r.reference,
                    r.due,
                    "/app/calendar".to_string(),
                )
            })
            .collect(),
    );

    // arrivals: confirmed trips arriving within 30 days.
    let rows: Vec<DashRow> = sqlx::query_as(
        "SELECT t.id, t.project_id, p.title AS project_title, p.reference,
                t.arrive_date AS due, ('departs ' || t.depart_date) AS extra
         FROM trips t JOIN projects p ON p.id = t.project_id
         WHERE t.status = 'confirmed'
           AND t.arrive_date >= ? AND t.arrive_date <= ?
         ORDER BY t.arrive_date LIMIT 50",
    )
    .bind(&today)
    .bind(&soon)
    .fetch_all(&state.pool)
    .await?;
    sections.insert(
        "arrivals".into(),
        rows.into_iter()
            .map(|r| {
                let pid = r.project_id.clone().unwrap_or_default();
                item(
                    "trip",
                    r.project_title.clone().unwrap_or_default(),
                    format!(
                        "arrives {} · {}",
                        r.due.clone().unwrap_or_default(),
                        r.extra.unwrap_or_default()
                    ),
                    Some(pid.clone()),
                    r.reference,
                    r.due,
                    "/app/calendar".to_string(),
                )
            })
            .collect(),
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Finance
// ---------------------------------------------------------------------------

async fn finance_sections(
    state: &AppState,
    sections: &mut BTreeMap<String, Vec<DashboardItemDto>>,
) -> AppResult<()> {
    let rows: Vec<DashRow> = sqlx::query_as(
        "SELECT i.id, i.project_id, p.title AS project_title, p.reference,
                i.due_date AS due,
                ('NZD ' || CAST(COALESCE((SELECT SUM(amount_cents) FROM invoice_lines WHERE invoice_id = i.id),0)/100 AS TEXT)) AS extra
         FROM invoices i JOIN projects p ON p.id = i.project_id
         WHERE i.status = 'draft'
         ORDER BY i.created_at LIMIT 50",
    )
    .fetch_all(&state.pool)
    .await?;
    sections.insert(
        "invoices_to_issue".into(),
        rows.into_iter()
            .map(|r| {
                let pid = r.project_id.clone().unwrap_or_default();
                item(
                    "invoice",
                    format!("Draft invoice {}", r.extra.unwrap_or_default()),
                    r.project_title.clone().unwrap_or_default(),
                    Some(pid.clone()),
                    r.reference,
                    r.due,
                    "/app/finance".to_string(),
                )
            })
            .collect(),
    );

    let rows: Vec<DashRow> = sqlx::query_as(
        "SELECT py.id, i.project_id, p.title AS project_title, p.reference,
                NULL AS due,
                (i.number || ' · ' || py.currency || ' ' || CAST(py.amount_cents/100 AS TEXT)
                  || ' · ' || py.method) AS extra
         FROM payments py
         JOIN invoices i ON i.id = py.invoice_id
         JOIN projects p ON p.id = i.project_id
         WHERE py.status = 'pending_verification'
         ORDER BY py.received_at LIMIT 50",
    )
    .fetch_all(&state.pool)
    .await?;
    sections.insert(
        "payments_to_verify".into(),
        rows.into_iter()
            .map(|r| {
                let pid = r.project_id.clone().unwrap_or_default();
                item(
                    "payment",
                    format!("Payment to verify: {}", r.extra.unwrap_or_default()),
                    r.project_title.clone().unwrap_or_default(),
                    Some(pid.clone()),
                    r.reference,
                    None,
                    "/app/finance".to_string(),
                )
            })
            .collect(),
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Provider — own resources only, minimal project info (§3)
// ---------------------------------------------------------------------------

async fn provider_sections(
    state: &AppState,
    actor: &Actor,
    sections: &mut BTreeMap<String, Vec<DashboardItemDto>>,
) -> AppResult<()> {
    let rows: Vec<DashRow> = sqlx::query_as(
        "SELECT b.id, t.project_id, p.title AS project_title, NULL AS reference,
                b.start_date AS due,
                (r.name || ' · ' || b.start_date || ' → ' || b.end_date
                  || ' · ' || CAST(b.quantity AS TEXT) || ' ' || r.unit_label
                  || ' · lead ' || COALESCE((SELECT u.name FROM project_members pm
                        JOIN users u ON u.id = pm.user_id
                        WHERE pm.project_id = p.id AND pm.role = 'lead' AND pm.removed_at IS NULL
                        LIMIT 1), '')) AS extra
         FROM bookings b
         JOIN resources r ON r.id = b.resource_id
         JOIN trips t ON t.id = b.trip_id
         JOIN projects p ON p.id = t.project_id
         WHERE b.status = 'requested' AND r.provider_user_id = ?
         ORDER BY b.start_date LIMIT 50",
    )
    .bind(&actor.user_id)
    .fetch_all(&state.pool)
    .await?;
    sections.insert(
        "my_requests".into(),
        rows.into_iter()
            .map(|r| {
                let pid = r.project_id.clone().unwrap_or_default();
                item(
                    "booking",
                    r.extra.unwrap_or_default(),
                    r.project_title.clone().unwrap_or_default(),
                    Some(pid),
                    None,
                    r.due,
                    "/app/provider".to_string(),
                )
            })
            .collect(),
    );
    Ok(())
}
