//! Slice D: mock AI assist; `PITCAIRN_AI_MODE=off` → 503 `ai_unavailable`,
//! and nothing else depends on it.

mod common;

use common::d::{project_id, spawn_app_with_ai};
use common::{persona, spawn_app};
use pitcairn::dto::{AssistExtractResponse, AssistSummaryResponse};
use serde_json::{Value, json};

async fn base_use_version(pool: &sqlx::SqlitePool) -> String {
    sqlx::query_scalar(
        "SELECT tv.id FROM template_versions tv JOIN templates t ON t.id = tv.template_id
         WHERE t.key = 'base_use' AND tv.status = 'published'",
    )
    .fetch_one(pool)
    .await
    .unwrap()
}

#[tokio::test]
async fn assist_returns_503_when_off_and_the_app_works_without_it() {
    let app = spawn_app_with_ai("off").await;
    let tv = base_use_version(&app.pool).await;
    let anna = persona(&app, "anna").await;
    let pid = project_id(&app.pool, "Coral health around Pitcairn").await;

    let resp = anna
        .post_json(
            "/api/v1/assist/extract-fields",
            &json!({"template_version_id": tv, "text": "Coral survey\n\nObjectives:\nCount coral."}),
        )
        .await;
    assert_eq!(resp.status(), 503);
    let body: Value = anna.json(resp).await;
    assert_eq!(body["error"]["code"], "ai_unavailable");
    let resp = anna
        .post_json("/api/v1/assist/summary", &json!({"project_id": pid}))
        .await;
    assert_eq!(resp.status(), 503);

    // Everything else keeps working with the assistant off.
    assert_eq!(anna.get("/api/v1/dashboard").await.status(), 200);
    assert_eq!(
        anna.get(&format!("/api/v1/projects/{pid}")).await.status(),
        200
    );
    let maria = persona(&app, "maria").await;
    assert_eq!(maria.get("/api/v1/dashboard").await.status(), 200);
    assert_eq!(
        maria.get("/api/v1/search/projects?q=coral").await.status(),
        200
    );
    assert_eq!(
        maria
            .get("/api/v1/reports/measurements?variable_key=coral_cover_percent")
            .await
            .status(),
        200
    );
    assert_eq!(
        maria
            .get(&format!("/api/v1/projects/{pid}/export"))
            .await
            .status(),
        200
    );
}

#[tokio::test]
async fn mock_extracts_fields_deterministically() {
    let app = spawn_app(true).await;
    let tv = base_use_version(&app.pool).await;
    let anna = persona(&app, "anna").await;
    let text = "Coral bleaching recovery at Pitcairn\n\
Te Moana University (fictional)\n\n\
Aims:\nUnderstand how reefs recover after bleaching.\n\n\
Objectives:\n1. Re-survey bleached colonies.\n2. Measure recovery rates.\n\n\
Methods:\nPhoto quadrats along fixed transects.\n\n\
We plan to be on the island from 2027-03-04 to 2027-03-25.";
    let body = json!({"template_version_id": tv, "text": text});
    let resp = anna.post_json("/api/v1/assist/extract-fields", &body).await;
    assert_eq!(resp.status(), 200);
    let a: AssistExtractResponse = anna.json(resp).await;
    let get = |key: &str| a.suggestions.iter().find(|s| s.field_key == key);
    assert_eq!(
        get("research_title").expect("title").value,
        json!("Coral bleaching recovery at Pitcairn")
    );
    assert!(
        get("institution")
            .expect("institution")
            .value
            .as_str()
            .unwrap()
            .contains("Te Moana")
    );
    assert!(
        get("objectives")
            .expect("objectives")
            .value
            .as_str()
            .unwrap()
            .contains("Re-survey")
    );
    assert!(
        get("methods")
            .expect("methods")
            .value
            .as_str()
            .unwrap()
            .contains("Photo quadrats")
    );
    assert_eq!(
        get("dates").expect("dates").value,
        json!({"start": "2027-03-04", "end": "2027-03-25"})
    );
    for s in &a.suggestions {
        assert!((0.0..=1.0).contains(&s.confidence));
        assert!(!s.source_excerpt.is_empty());
    }
    // Same input → same output.
    let b: AssistExtractResponse = anna
        .json(anna.post_json("/api/v1/assist/extract-fields", &body).await)
        .await;
    assert_eq!(
        serde_json::to_value(&a.suggestions).unwrap(),
        serde_json::to_value(&b.suggestions).unwrap()
    );

    // Validation and unknown template.
    let resp = anna
        .post_json(
            "/api/v1/assist/extract-fields",
            &json!({"template_version_id": tv, "text": ""}),
        )
        .await;
    assert_eq!(resp.status(), 422);
    let resp = anna
        .post_json(
            "/api/v1/assist/extract-fields",
            &json!({"template_version_id": "nope", "text": "x"}),
        )
        .await;
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn mock_summary_respects_project_access() {
    let app = spawn_app(true).await;
    let pid = project_id(&app.pool, "Coral health around Pitcairn").await;
    let anna = persona(&app, "anna").await;
    let resp = anna
        .post_json("/api/v1/assist/summary", &json!({"project_id": pid}))
        .await;
    assert_eq!(resp.status(), 200);
    let s: AssistSummaryResponse = anna.json(resp).await;
    assert!(s.summary.starts_with("Coral health around Pitcairn."));
    assert!(s.summary.contains("baseline"), "{}", s.summary);

    // Another team cannot use the assistant to read Anna's project.
    let lukas = persona(&app, "lukas").await;
    let resp = lukas
        .post_json("/api/v1/assist/summary", &json!({"project_id": pid}))
        .await;
    assert_eq!(resp.status(), 404);
    let anon = common::Client::anonymous(&app);
    let resp = anon
        .post_json("/api/v1/assist/summary", &json!({"project_id": pid}))
        .await;
    assert_eq!(resp.status(), 401);
}
