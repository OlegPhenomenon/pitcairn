mod common;

use common::a::*;
use common::{persona, spawn_app};
use serde_json::json;

/// §12 scenario 1 (application part): a brand-new team submits, the
/// coordinator asks for a document anchored to a slot, the team uploads and
/// resubmits (both revisions kept), an expert gives an opinion and the
/// decision maker issues a permit with conditions.
#[tokio::test]
async fn full_application_path_to_issued_permit_with_conditions() {
    let app = spawn_app(true).await;
    let erik = register(&app, "erik@fjord.invalid", "Dr Erik Lund").await;
    let maria = persona(&app, "maria").await;
    let james = persona(&app, "james").await;
    let helen = persona(&app, "helen").await;

    let pid = create_project(&erik, "base_use", "Kelp forest census").await;
    let site_id = add_site(&erik, &pid, "Western kelp bed", false).await;

    // Incomplete submit → 422 with field errors for answers and documents.
    let (status, err) = submit(&erik, &pid, None).await;
    assert_eq!(status, 422);
    assert!(err["error"]["fields"]["answers.aims"].is_string(), "{err}");
    assert!(err["error"]["fields"]["documents.safety_plan"].is_string());

    let version = project_version(&erik, &pid).await;
    let (status, saved) = patch(
        &erik,
        &format!("/projects/{pid}"),
        json!({
            "version": version,
            "summary": "Census of kelp forests (fictional)",
            "answers": {
                "applicant_name": "Dr Erik Lund", "position": "Senior researcher",
                "institution": "Fjord Institute (fictional)", "address": "1 Fjord Road",
                "funders": "Fictional Science Fund",
                "researchers": [{"name": "Erik Lund", "role": "lead"}],
                "research_title": "Kelp forest census", "aims": "Map kelp",
                "objectives": "1. Map\n2. Count", "methods": "Transects",
                "outputs_benefit": "Baseline for the community", "data_management": "Open data",
                "timeline": "Week 1: dives", "budget": "NZD 10k (fictional)",
                "dates": {"start": "2026-11-01", "end": "2026-11-20"},
                "sites": [site_id], "safety_summary": "Buddy diving only",
            }
        }),
    )
    .await;
    assert_eq!(status, 200, "{saved}");
    let (safety_doc, _) = upload_document(
        &erik,
        &app,
        &pid,
        Some("safety_plan"),
        "application",
        "Safety plan v1",
    )
    .await;
    upload_document(
        &erik,
        &app,
        &pid,
        Some("insurance"),
        "personal",
        "Insurance",
    )
    .await;
    upload_document(&erik, &app, &pid, Some("cvs"), "personal", "CVs").await;
    upload_document(&erik, &app, &pid, Some("permits"), "application", "Permits").await;

    let (status, first) = submit(&erik, &pid, Some("submit-1")).await;
    assert_eq!(status, 200, "{first}");
    assert_eq!(first["status"], "submitted");
    assert_eq!(first["revision_number"], 1);
    let reference = first["reference"].as_str().unwrap().to_string();
    assert!(reference.starts_with("PIT-") && reference.len() == "PIT-2026-0001".len());

    let (status, _) = post(&maria, &format!("/projects/{pid}/screen"), json!({})).await;
    assert_eq!(status, 200);

    // Coordinator requests a revised safety plan, anchored to the slot.
    let (status, thread) = post(
        &maria,
        &format!("/projects/{pid}/threads"),
        json!({
            "anchor_type": "document", "anchor_key": "safety_plan", "visibility": "shared",
            "body": "The safety plan lacks an evacuation section.",
            "action_item": {"addressed_to": "team", "title": "Please add a field safety plan with evacuation"},
        }),
    )
    .await;
    assert_eq!(status, 201, "{thread}");
    let thread_id = thread["id"].as_str().unwrap().to_string();
    let (_, ws) = get(&erik, &format!("/projects/{pid}")).await;
    assert_eq!(ws["project"]["status"], "changes_requested");
    let primary = ws["primary_message"]["text"].as_str().unwrap();
    assert!(
        primary.starts_with("Maria asks you to please add"),
        "{primary}"
    );

    // A decision cannot be issued while a team action item is open.
    let rev1 = latest_revision_id(&app, &pid).await;
    let (status, blocked) = post(
        &helen,
        &format!("/projects/{pid}/decisions"),
        json!({"kind": "permit", "project_revision_id": rev1, "basis": "x",
               "valid_from": "2026-11-01", "valid_to": "2026-11-30"}),
    )
    .await;
    assert_eq!(status, 201);
    let blocked_id = blocked["id"].as_str().unwrap().to_string();
    let (status, err) = post(&helen, &format!("/decisions/{blocked_id}/issue"), json!({})).await;
    assert_eq!(status, 409);
    assert_eq!(err["error"]["code"], "open_action_items");

    // Team answers in the thread and uploads a new version of the document.
    let (status, _) = post(
        &erik,
        &format!("/threads/{thread_id}/messages"),
        json!({"body": "Uploaded v2 with an evacuation section."}),
    )
    .await;
    assert_eq!(status, 201);
    let file_id = upload_clean_file(&erik, &app, b"safety plan v2 with evacuation").await;
    let (status, _) = post(
        &erik,
        &format!("/documents/{safety_doc}/versions"),
        json!({"file_id": file_id, "note": "v2"}),
    )
    .await;
    assert!(
        status == 200 || status == 201,
        "new version status {status}"
    );

    let (status, second) = submit(&erik, &pid, Some("submit-2")).await;
    assert_eq!(status, 200, "{second}");
    assert_eq!(second["status"], "in_review");
    assert_eq!(second["revision_number"], 2);
    assert_eq!(second["reference"], reference.as_str(), "reference kept");
    let open: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM action_items WHERE project_id = ? AND status = 'open'",
    )
    .bind(&pid)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(
        open, 0,
        "answered team action item resolved by resubmission"
    );

    // Both revisions kept; the diff shows the new document version.
    let (_, revisions) = get(&erik, &format!("/projects/{pid}/revisions")).await;
    assert_eq!(revisions["total"], 2);
    let (status, diff) = get(
        &erik,
        &format!("/projects/{pid}/revisions/2/diff?against=1"),
    )
    .await;
    assert_eq!(status, 200);
    let changes = diff["changes"].as_array().unwrap();
    assert!(
        changes
            .iter()
            .any(|c| c["path"] == format!("documents.{safety_doc}") && c["kind"] == "changed"),
        "{diff}"
    );

    // Expert review bound to revision 2.
    let james_id = persona_id(&app, "james").await;
    let (status, review) = post(
        &maria,
        &format!("/projects/{pid}/reviews"),
        json!({"expert_id": james_id, "due_date": "2026-10-30"}),
    )
    .await;
    assert_eq!(status, 201, "{review}");
    assert_eq!(review["revision_number"], 2);
    let review_id = review["id"].as_str().unwrap();
    let (status, _) = post(&james, &format!("/reviews/{review_id}/accept"), json!({})).await;
    assert_eq!(status, 200);
    let (status, review) = post(
        &james,
        &format!("/reviews/{review_id}/submit"),
        json!({"opinion": "Sound methods; restrict anchoring.", "recommendation": "approve_with_conditions"}),
    )
    .await;
    assert_eq!(status, 200, "{review}");
    // Opinions are internal: the team cannot list reviews.
    let (status, _) = get(&erik, &format!("/projects/{pid}/reviews")).await;
    assert_eq!(status, 403);
    let (_, timeline) = get(&erik, &format!("/projects/{pid}/timeline")).await;
    assert!(!timeline.to_string().contains("restrict anchoring"));

    // Decision on revision 2 with conditions (the earlier draft is discarded
    // by simply issuing the new one; drafts never take effect).
    let rev2 = latest_revision_id(&app, &pid).await;
    let (status, _) = patch(
        &helen,
        &format!("/decisions/{blocked_id}"),
        json!({
            "project_revision_id": rev2,
            "basis": "Meets the base-use policy; expert recommends approval with conditions",
            "legal_reference": "MSB Research Policy (demo) s.4",
            "permitted_activities": ["Scuba transects", "Photo quadrats"],
            "conditions": ["No anchoring on kelp beds", "Report within 90 days"],
            "restrictions": ["No sampling of protected species"],
        }),
    )
    .await;
    assert_eq!(status, 200);
    let (status, issued) = post(&helen, &format!("/decisions/{blocked_id}/issue"), json!({})).await;
    assert_eq!(status, 200, "{issued}");
    assert_eq!(issued["status"], "issued");
    assert_eq!(issued["sites"].as_array().unwrap().len(), 1);

    let (_, ws) = get(&erik, &format!("/projects/{pid}")).await;
    assert_eq!(ws["project"]["status"], "approved");
    let (_, decisions) = get(&erik, &format!("/projects/{pid}/decisions")).await;
    assert_eq!(decisions["total"], 1);
    assert_eq!(
        decisions["items"][0]["conditions"],
        json!(["No anchoring on kelp beds", "Report within 90 days"])
    );

    // Printable document.
    let resp = erik
        .get(&format!("/api/v1/decisions/{blocked_id}/document"))
        .await;
    assert_eq!(resp.status(), 200);
    assert!(
        resp.headers()["content-type"]
            .to_str()
            .unwrap()
            .starts_with("text/html")
    );
    let html = resp.text().await.unwrap();
    for needle in [
        "Research Permit",
        reference.as_str(),
        "No anchoring on kelp beds",
        "MSB Research Policy (demo) s.4",
        "Western kelp bed",
        "Helen Brooks",
        "2026-11-01 to 2026-11-30",
    ] {
        assert!(html.contains(needle), "document missing {needle}");
    }

    // The team was notified in-app.
    let erik_id = user_id(&app, "erik@fjord.invalid").await;
    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM notifications WHERE user_id = ? AND kind = 'decision_issued'",
    )
    .bind(&erik_id)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(n, 1);
}
