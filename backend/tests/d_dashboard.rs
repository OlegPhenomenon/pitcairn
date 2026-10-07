//! Slice D: role dashboards contain the expected seeded items (§5, §9).

mod common;

use common::{persona, spawn_app};
use pitcairn::dto::{DashboardItemDto, DashboardResponse};

async fn dashboard(app: &common::TestApp, key: &str) -> DashboardResponse {
    let c = persona(app, key).await;
    let resp = c.get("/api/v1/dashboard").await;
    assert_eq!(resp.status(), 200, "dashboard for {key}");
    c.json(resp).await
}

fn section<'a>(d: &'a DashboardResponse, name: &str) -> &'a [DashboardItemDto] {
    d.sections
        .get(name)
        .unwrap_or_else(|| panic!("section {name} missing; have {:?}", d.sections.keys()))
}

fn mentions(items: &[DashboardItemDto], needle: &str) -> bool {
    items
        .iter()
        .any(|i| i.title.contains(needle) || i.subtitle.contains(needle))
}

#[tokio::test]
async fn researcher_sections_show_projects_replies_trips_and_overdue_results() {
    let app = spawn_app(true).await;
    let d = dashboard(&app, "anna").await;
    for name in [
        "my_projects",
        "needs_reply",
        "decisions",
        "trips_and_invoices",
        "results_due",
    ] {
        section(&d, name);
    }
    // No staff sections for a researcher.
    assert!(!d.sections.contains_key("new_applications"));

    let projects = section(&d, "my_projects");
    for title in [
        "Coral health around Pitcairn",
        "Henderson Island seabird census",
        "Reef fish biomass at Bounty Bay",
    ] {
        assert!(mentions(projects, title), "my_projects lacks {title}");
    }
    assert!(mentions(
        section(&d, "needs_reply"),
        "Send the census summary report"
    ));
    assert!(
        section(&d, "decisions")
            .iter()
            .any(|i| i.title.contains("permit"))
    );
    let trips = section(&d, "trips_and_invoices");
    assert!(trips.iter().any(|i| i.kind == "trip"));
    assert!(trips.iter().any(|i| i.kind == "invoice"));

    // Overdue deliverable stays visible after the trip ended, flagged.
    let results = section(&d, "results_due");
    let overdue = results
        .iter()
        .find(|i| i.title == "Census summary report")
        .expect("overdue report listed");
    assert_eq!(overdue.kind, "overdue_deliverable");
    assert_eq!(overdue.due_date.as_deref(), Some("2025-03-31"));
    assert!(overdue.project_reference.is_some());
    assert!(overdue.link.starts_with("/app/projects/"));
}

#[tokio::test]
async fn coordinator_sections_cover_the_whole_pipeline() {
    let app = spawn_app(true).await;
    let d = dashboard(&app, "maria").await;
    assert!(mentions(
        section(&d, "new_applications"),
        "Deep-water sponge assemblages"
    ));
    assert!(mentions(
        section(&d, "waiting_for_applicant"),
        "Land snail survey"
    ));
    assert!(mentions(section(&d, "with_experts"), "Microplastics"));
    assert!(mentions(section(&d, "arriving_soon"), "Reef fish biomass"));
    assert!(mentions(
        section(&d, "results_to_check"),
        "Transect photo archive"
    ));
    let overdue = section(&d, "overdue_results");
    assert!(mentions(overdue, "Census summary report"));
    assert!(
        overdue.iter().any(|i| i.kind == "external_link"),
        "unavailable external links are flagged to the coordinator"
    );
    // Coordinator is not a researcher: no researcher sections.
    assert!(!d.sections.contains_key("my_projects"));
}

#[tokio::test]
async fn expert_base_manager_finance_and_provider_sections() {
    let app = spawn_app(true).await;

    let d = dashboard(&app, "james").await;
    assert!(mentions(section(&d, "assigned_reviews"), "Microplastics"));
    assert_eq!(d.sections.len(), 1, "expert only gets assigned_reviews");

    let d = dashboard(&app, "sam").await;
    assert!(mentions(section(&d, "pending_bookings"), "Dive compressor"));
    assert!(mentions(section(&d, "arrivals"), "Reef fish biomass"));
    // Provider-owned boat requests go to the provider, not the base manager.
    assert!(!mentions(section(&d, "pending_bookings"), "Boat charter"));

    let d = dashboard(&app, "ruth").await;
    assert!(!section(&d, "invoices_to_issue").is_empty());
    assert!(mentions(section(&d, "payments_to_verify"), "manual"));

    let d = dashboard(&app, "david").await;
    let requests = section(&d, "my_requests");
    assert_eq!(requests.len(), 1);
    assert!(requests[0].title.contains("Boat charter"));
    assert!(
        requests[0].project_reference.is_none(),
        "provider gets minimal info"
    );
}

#[tokio::test]
async fn dashboard_requires_login() {
    let app = spawn_app(true).await;
    let anon = common::Client::anonymous(&app);
    assert_eq!(anon.get("/api/v1/dashboard").await.status(), 401);
}

#[tokio::test]
async fn invoice_amounts_keep_cents_on_dashboard() {
    let app = spawn_app(true).await;
    sqlx::query(
        "UPDATE invoice_lines SET amount_cents = 12345
         WHERE invoice_id = (SELECT id FROM invoices WHERE status = 'draft' LIMIT 1)",
    )
    .execute(&app.pool)
    .await
    .unwrap();
    let d = dashboard(&app, "ruth").await;
    assert!(mentions(section(&d, "invoices_to_issue"), "NZD 123.45"));
}
