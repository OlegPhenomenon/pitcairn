mod common;

use common::a::*;
use common::{persona, spawn_app};
use serde_json::{Value, json};

fn v2_schema() -> Value {
    json!({
        "sections": [
            {"key": "purpose", "title": "Purpose", "fields": [
                {"key": "purpose", "label": "Purpose of fieldwork", "type": "textarea", "required": true},
                {"key": "drone_pilot", "label": "Licensed drone pilot", "type": "text", "required": true}
            ]},
            {"key": "logistics", "title": "Logistics", "fields": [
                {"key": "dates", "label": "Fieldwork dates", "type": "daterange", "required": true},
                {"key": "team_size", "label": "Team size", "type": "number", "required": true},
                {"key": "sites", "label": "Fieldwork sites", "type": "sites", "required": true}
            ]}
        ],
        "required_documents": []
    })
}

#[tokio::test]
async fn template_v2_published_after_submission_old_revision_renders_with_v1() {
    let app = spawn_app(true).await;
    let anna = persona(&app, "anna").await;
    let admin = persona(&app, "admin").await;

    let submitted = fieldwork_draft(&anna, "Submitted under v1").await;
    let (status, _) = submit(&anna, &submitted, None).await;
    assert_eq!(status, 200);
    let draft = fieldwork_draft(&anna, "Still a draft").await;

    // Admin-only, schema validated against the fixed field-type palette.
    let (status, _) = post(
        &anna,
        "/templates/fieldwork_permit/versions",
        json!({"schema": v2_schema()}),
    )
    .await;
    assert_eq!(status, 403);
    let mut bad = v2_schema();
    bad["sections"][0]["fields"][1]["type"] = json!("signature");
    let (status, err) = post(
        &admin,
        "/templates/fieldwork_permit/versions",
        json!({"schema": bad}),
    )
    .await;
    assert_eq!(status, 422);
    assert!(
        err["error"]["fields"]["sections[0].fields[1].type"].is_string(),
        "{err}"
    );

    let (status, v2) = post(
        &admin,
        "/templates/fieldwork_permit/versions",
        json!({"schema": v2_schema()}),
    )
    .await;
    assert_eq!(status, 201, "{v2}");
    assert_eq!(
        (v2["version"].as_i64(), v2["status"].as_str()),
        (Some(2), Some("draft"))
    );
    let v2_id = v2["id"].as_str().unwrap();
    let (status, _) = put(
        &admin,
        &format!("/template-versions/{v2_id}"),
        json!({"schema": v2_schema()}),
    )
    .await;
    assert_eq!(status, 200, "drafts are editable");
    let (status, published) = post(
        &admin,
        &format!("/template-versions/{v2_id}/publish"),
        json!({}),
    )
    .await;
    assert_eq!(status, 200, "{published}");
    assert_eq!(published["status"], "published");
    let (status, err) = put(
        &admin,
        &format!("/template-versions/{v2_id}"),
        json!({"schema": v2_schema()}),
    )
    .await;
    assert_eq!(
        (status, err["error"]["code"].as_str()),
        (409, Some("version_immutable"))
    );

    let (_, versions) = get(&anna, "/templates/fieldwork_permit/versions").await;
    let statuses: Vec<(i64, String)> = versions["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| {
            (
                v["version"].as_i64().unwrap(),
                v["status"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    assert_eq!(
        statuses,
        vec![(2, "published".into()), (1, "retired".into())]
    );

    // The submitted revision still renders with its own v1 schema.
    let (status, rev) = get(&anna, &format!("/projects/{submitted}/revisions/1")).await;
    assert_eq!(status, 200, "{rev}");
    assert_eq!(rev["template_version"], 1);
    let schema = rev["template_schema"].to_string();
    assert!(schema.contains("activities") && !schema.contains("drone_pilot"));
    assert_eq!(rev["snapshot"]["answers"]["purpose"], "Survey coral cover");

    // The draft is outdated: submit refused until upgraded; upgrade copies by key.
    let (_, ws) = get(&anna, &format!("/projects/{draft}")).await;
    assert_eq!(ws["application"]["template_outdated"], true);
    let (status, err) = submit(&anna, &draft, None).await;
    assert_eq!(
        (status, err["error"]["code"].as_str()),
        (409, Some("template_outdated"))
    );
    let (status, up) = post(
        &anna,
        &format!("/projects/{draft}/upgrade-template"),
        json!({}),
    )
    .await;
    assert_eq!(status, 200, "{up}");
    assert_eq!(up["template_version_number"], 2);
    assert_eq!(up["dropped_keys"], json!(["activities"]));
    let (status, err) = submit(&anna, &draft, None).await;
    assert_eq!(status, 422);
    assert!(
        err["error"]["fields"]["answers.drone_pilot"].is_string(),
        "{err}"
    );

    // New projects bind to v2.
    let fresh = create_project(&anna, "fieldwork_permit", "Fresh").await;
    let (_, ws) = get(&anna, &format!("/projects/{fresh}")).await;
    assert_eq!(ws["application"]["template_version_number"], 2);
}
