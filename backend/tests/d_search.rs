//! Slice D: project search by q / organisation / year / bbox / status /
//! has_overdue, with expert scoping and deny paths (§5).

mod common;

use common::{persona, spawn_app};
use pitcairn::dto::{ListResponse, SearchProjectItemDto};

async fn search(c: &common::Client, query: &str) -> Vec<SearchProjectItemDto> {
    let resp = c.get(&format!("/api/v1/search/projects?{query}")).await;
    assert_eq!(resp.status(), 200, "search {query}");
    let list: ListResponse<SearchProjectItemDto> = c.json(resp).await;
    assert_eq!(list.total as usize, list.items.len(), "{query}: total");
    list.items
}

fn titles(items: &[SearchProjectItemDto]) -> Vec<&str> {
    items.iter().map(|i| i.title.as_str()).collect()
}

#[tokio::test]
async fn search_by_q_org_year_bbox_and_overdue() {
    let app = spawn_app(true).await;
    let maria = persona(&app, "maria").await;

    // Full-text with prefix matching ("cor" → coral).
    let items = search(&maria, "q=cor").await;
    let t = titles(&items);
    assert!(t.contains(&"Coral cover transects"), "{t:?}");
    assert!(t.contains(&"Coral health around Pitcairn"), "{t:?}");
    assert!(!t.contains(&"Henderson Island seabird census"), "{t:?}");

    // Reference is indexed too.
    let items = search(&maria, "q=PIT-2023").await;
    assert_eq!(titles(&items), vec!["Humpback whale acoustic monitoring"]);

    // Organisation filter.
    let items = search(&maria, "organisation=north%20sea").await;
    let t = titles(&items);
    assert!(t.contains(&"Humpback whale acoustic monitoring"));
    assert!(t.contains(&"Deep-water sponge assemblages at Adams Seamount"));
    assert!(t.iter().all(|t| !t.contains("Coral")), "{t:?}");

    // Year: projects active in 2016 → only the legacy lobster survey.
    let items = search(&maria, "year=2016").await;
    assert_eq!(titles(&items), vec!["Pitcairn rock lobster abundance"]);

    // Year 2024 combined with q.
    let items = search(&maria, "year=2024&q=seabird").await;
    let t = titles(&items);
    assert!(t.contains(&"Henderson Island seabird census"), "{t:?}");
    assert!(
        t.contains(&"Drone mapping of seabird colonies on Oeno Island"),
        "{t:?}"
    );

    // Bbox around Henderson Island (minLng,minLat,maxLng,maxLat).
    let items = search(&maria, "bbox=-128.5,-24.5,-128.2,-24.2").await;
    let t = titles(&items);
    assert!(t.contains(&"Henderson Island seabird census"), "{t:?}");
    assert!(
        t.contains(&"Henderson Island beach plastics audit"),
        "{t:?}"
    );
    assert!(!t.contains(&"Coral cover transects"), "{t:?}");

    // Bbox + status filter combined.
    let items = search(&maria, "bbox=-128.5,-24.5,-128.2,-24.2&status=closed").await;
    assert_eq!(
        titles(&items),
        vec!["Henderson Island beach plastics audit"]
    );

    // Overdue filter and deliverable counts.
    let items = search(&maria, "has_overdue=true").await;
    let seabirds = items
        .iter()
        .find(|i| i.title == "Henderson Island seabird census")
        .expect("seabird census has an overdue deliverable");
    assert_eq!(seabirds.deliverables_overdue, 1);
    assert_eq!(seabirds.deliverables_received, 1);
    assert!(items.iter().all(|i| i.deliverables_overdue > 0));
}

#[tokio::test]
async fn experts_see_only_assigned_projects_and_researchers_are_denied() {
    let app = spawn_app(true).await;
    let james = persona(&app, "james").await;
    let items = search(&james, "").await;
    let assigned: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT p.title FROM projects p JOIN review_assignments ra ON ra.project_id = p.id
         JOIN users u ON u.id = ra.expert_id
         WHERE u.email = 'james@demo.pitcairn.invalid' AND ra.status != 'declined'",
    )
    .fetch_all(&app.pool)
    .await
    .unwrap();
    assert!(!items.is_empty());
    for item in &items {
        assert!(
            assigned.contains(&item.title),
            "{} not assigned",
            item.title
        );
    }
    assert!(titles(&items).contains(&"Microplastics in Pitcairn coastal waters"));

    let anna = persona(&app, "anna").await;
    assert_eq!(
        anna.get("/api/v1/search/projects?q=coral").await.status(),
        403
    );
    let david = persona(&app, "david").await;
    assert_eq!(david.get("/api/v1/search/projects").await.status(), 403);
}

#[tokio::test]
async fn invalid_filters_are_field_errors() {
    let app = spawn_app(true).await;
    let maria = persona(&app, "maria").await;
    for (query, field) in [
        ("bbox=1,2,3", "bbox"),
        ("year=20x4", "year"),
        ("status=bogus", "status"),
    ] {
        let resp = maria.get(&format!("/api/v1/search/projects?{query}")).await;
        assert_eq!(resp.status(), 422, "{query}");
        let body: serde_json::Value = maria.json(resp).await;
        assert!(
            body["error"]["fields"][field].is_string(),
            "{query}: {body}"
        );
    }
    // Punctuation in q never breaks the FTS query.
    let resp = maria.get("/api/v1/search/projects?q=%22coral%20(AND").await;
    assert_eq!(resp.status(), 200);
}
