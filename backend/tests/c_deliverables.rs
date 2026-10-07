//! Slice C: deliverable lifecycle — propose, agree, re-agreement after
//! material changes, waive/cancel, project close and the §12 deny paths.

mod common;

use common::c::*;
use common::{persona, spawn_app};

const ANNA: &str = "anna@demo.pitcairn.invalid";
const MARIA: &str = "maria@demo.pitcairn.invalid";

#[tokio::test]
async fn propose_agree_and_reagree_after_due_date_change() {
    let app = spawn_app(true).await;
    let anna = persona(&app, "anna").await;
    let maria = persona(&app, "maria").await;
    let project = seeded_project(&app).await;
    let anna_id = user_id(&app, ANNA).await;
    let maria_id = user_id(&app, MARIA).await;

    // Anna proposes — her side is acknowledged at creation.
    let d = create_deliverable(&anna, &project, "report", "2030-01-01", &anna_id, &maria_id).await;
    assert_eq!(d.status, "proposed");
    assert_eq!(d.agreement_state, "team_only");
    assert_eq!(d.terms_version, 1);

    // Maria agrees → agreed.
    let resp = maria
        .post(&format!("/api/v1/deliverables/{}/agree", d.id))
        .await;
    assert_eq!(resp.status(), 200);
    let d: pitcairn::dto::DeliverableDto = maria.json(resp).await;
    assert_eq!(d.status, "agreed");
    assert_eq!(d.agreement_state, "agreed");

    // Due-date change without a reason → 422 field error.
    let resp = maria
        .patch_json(
            &format!("/api/v1/deliverables/{}", d.id),
            &serde_json::json!({"due_date": "2030-03-01"}),
        )
        .await;
    assert_eq!(resp.status(), 422);
    let err: serde_json::Value = maria.json(resp).await;
    assert!(err["error"]["fields"]["reason"].is_string());

    // With a reason → terms_version bumps, team ack cleared, back to proposed.
    let resp = maria
        .patch_json(
            &format!("/api/v1/deliverables/{}", d.id),
            &serde_json::json!({"due_date": "2030-03-01", "reason": "trip rescheduled"}),
        )
        .await;
    assert_eq!(resp.status(), 200);
    let d: pitcairn::dto::DeliverableDto = maria.json(resp).await;
    assert_eq!(d.status, "proposed");
    assert_eq!(d.terms_version, 2);
    assert_eq!(d.agreement_state, "staff_only");
    assert_eq!(d.due_date, "2030-03-01");

    // deliverable_due_changes row recorded.
    let changes: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM deliverable_due_changes WHERE deliverable_id = ?")
            .bind(&d.id)
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(changes.0, 1);

    // Anna re-agrees → agreed again.
    let resp = anna
        .post(&format!("/api/v1/deliverables/{}/agree", d.id))
        .await;
    assert_eq!(resp.status(), 200);
    let d: pitcairn::dto::DeliverableDto = anna.json(resp).await;
    assert_eq!(d.status, "agreed");
    assert_eq!(d.agreement_state, "agreed");
}

#[tokio::test]
async fn material_change_by_team_clears_staff_acknowledgement() {
    let app = spawn_app(true).await;
    let anna = persona(&app, "anna").await;
    let maria = persona(&app, "maria").await;
    let project = seeded_project(&app).await;
    let anna_id = user_id(&app, ANNA).await;
    let maria_id = user_id(&app, MARIA).await;

    let d = agreed_deliverable(&maria, &anna, &project, "dataset", &anna_id, &maria_id).await;

    // Anna changes the title → staff ack cleared, status proposed.
    let resp = anna
        .patch_json(
            &format!("/api/v1/deliverables/{}", d.id),
            &serde_json::json!({"title": "Water quality dataset v2"}),
        )
        .await;
    assert_eq!(resp.status(), 200);
    let d: pitcairn::dto::DeliverableDto = anna.json(resp).await;
    assert_eq!(d.status, "proposed");
    assert_eq!(d.terms_version, 2);
    assert_eq!(d.agreement_state, "team_only");
    assert_eq!(d.title, "Water quality dataset v2");

    // Maria re-agrees.
    let resp = maria
        .post(&format!("/api/v1/deliverables/{}/agree", d.id))
        .await;
    assert_eq!(resp.status(), 200);
    let d: pitcairn::dto::DeliverableDto = maria.json(resp).await;
    assert_eq!(d.status, "agreed");
}

#[tokio::test]
async fn deliverable_validation_and_membership_rules() {
    let app = spawn_app(true).await;
    let anna = persona(&app, "anna").await;
    let project = seeded_project(&app).await;
    let anna_id = user_id(&app, ANNA).await;
    let maria_id = user_id(&app, MARIA).await;
    let lukas_id = user_id(&app, "lukas@demo.pitcairn.invalid").await;
    let ruth_id = user_id(&app, "ruth@demo.pitcairn.invalid").await;

    // Sender not a project member + recipient not a coordinator → field errors.
    let resp = anna
        .post_json(
            &format!("/api/v1/projects/{project}/deliverables"),
            &serde_json::json!({
                "title": "x", "kind": "bogus", "due_date": "not-a-date",
                "sender_id": lukas_id, "recipient_id": ruth_id,
            }),
        )
        .await;
    assert_eq!(resp.status(), 422);
    let err: serde_json::Value = anna.json(resp).await;
    assert!(err["error"]["fields"]["kind"].is_string());
    assert!(err["error"]["fields"]["due_date"].is_string());
    assert!(err["error"]["fields"]["sender_id"].is_string());
    assert!(err["error"]["fields"]["recipient_id"].is_string());

    // Happy path with maria as recipient.
    let d = create_deliverable(&anna, &project, "report", "2030-01-01", &anna_id, &maria_id).await;
    assert_eq!(d.sender_name, "Dr Anna Hart");
    assert_eq!(d.recipient_name, "Maria Ellis");
}

#[tokio::test]
async fn viewer_and_outsider_cannot_propose_agree_or_list() {
    let app = spawn_app(true).await;
    let anna = persona(&app, "anna").await;
    let tomasi = persona(&app, "tomasi").await; // viewer on the seeded project
    let lukas = persona(&app, "lukas").await; // not on the project
    let project = seeded_project(&app).await;
    let anna_id = user_id(&app, ANNA).await;
    let maria_id = user_id(&app, MARIA).await;

    let d = create_deliverable(&anna, &project, "report", "2030-01-01", &anna_id, &maria_id).await;

    // Viewer (team, read-only): cannot propose, patch or agree.
    let resp = tomasi
        .post_json(
            &format!("/api/v1/projects/{project}/deliverables"),
            &serde_json::json!({
                "title": "x", "kind": "report", "due_date": "2030-01-01",
                "sender_id": anna_id, "recipient_id": maria_id,
            }),
        )
        .await;
    assert_eq!(resp.status(), 403);
    let resp = tomasi
        .patch_json(
            &format!("/api/v1/deliverables/{}", d.id),
            &serde_json::json!({"title": "hijack"}),
        )
        .await;
    assert_eq!(resp.status(), 403);
    let resp = tomasi
        .post(&format!("/api/v1/deliverables/{}/agree", d.id))
        .await;
    assert_eq!(resp.status(), 403);
    // but may list them (viewer access).
    let resp = tomasi
        .get(&format!("/api/v1/projects/{project}/deliverables"))
        .await;
    assert_eq!(resp.status(), 200);

    // Outsider: cannot even list.
    let resp = lukas
        .get(&format!("/api/v1/projects/{project}/deliverables"))
        .await;
    assert_eq!(resp.status(), 403);
    let resp = lukas
        .post(&format!("/api/v1/deliverables/{}/agree", d.id))
        .await;
    assert_eq!(resp.status(), 403);
}

#[tokio::test]
async fn only_coordinator_can_waive_or_cancel() {
    let app = spawn_app(true).await;
    let anna = persona(&app, "anna").await;
    let maria = persona(&app, "maria").await;
    let ruth = persona(&app, "ruth").await; // finance: staff but no deliverable rights
    let project = seeded_project(&app).await;
    let anna_id = user_id(&app, ANNA).await;
    let maria_id = user_id(&app, MARIA).await;

    let d1 = agreed_deliverable(&anna, &maria, &project, "report", &anna_id, &maria_id).await;
    let d2 = agreed_deliverable(&anna, &maria, &project, "other", &anna_id, &maria_id).await;

    // Team cannot waive; finance cannot waive.
    let resp = anna
        .post_json(
            &format!("/api/v1/deliverables/{}/waive", d1.id),
            &serde_json::json!({"note": "x"}),
        )
        .await;
    assert_eq!(resp.status(), 403);
    let resp = ruth
        .post_json(
            &format!("/api/v1/deliverables/{}/waive", d1.id),
            &serde_json::json!({"note": "x"}),
        )
        .await;
    assert_eq!(resp.status(), 403);

    // Note is required.
    let resp = maria
        .post_json(
            &format!("/api/v1/deliverables/{}/waive", d1.id),
            &serde_json::json!({"note": ""}),
        )
        .await;
    assert_eq!(resp.status(), 422);

    // Maria waives and cancels.
    let resp = maria
        .post_json(
            &format!("/api/v1/deliverables/{}/waive", d1.id),
            &serde_json::json!({"note": "scope cut"}),
        )
        .await;
    assert_eq!(resp.status(), 200);
    let d1: pitcairn::dto::DeliverableDto = maria.json(resp).await;
    assert_eq!(d1.status, "waived");
    assert_eq!(d1.resolution_note.as_deref(), Some("scope cut"));

    let resp = maria
        .post_json(
            &format!("/api/v1/deliverables/{}/cancel", d2.id),
            &serde_json::json!({"note": "permit revoked"}),
        )
        .await;
    assert_eq!(resp.status(), 200);
    let d2: pitcairn::dto::DeliverableDto = maria.json(resp).await;
    assert_eq!(d2.status, "cancelled");

    // Terminal states reject further transitions.
    let resp = maria
        .post(&format!("/api/v1/deliverables/{}/agree", d1.id))
        .await;
    assert_eq!(resp.status(), 409);
}

#[tokio::test]
async fn close_project_requires_resolving_open_deliverables() {
    let app = spawn_app(true).await;
    let anna = persona(&app, "anna").await;
    let maria = persona(&app, "maria").await;
    let project = seeded_project(&app).await;
    set_project_status(&app, &project, "approved").await;
    let anna_id = user_id(&app, ANNA).await;
    let maria_id = user_id(&app, MARIA).await;

    let open = agreed_deliverable(&anna, &maria, &project, "report", &anna_id, &maria_id).await;

    // Close without resolutions → 409 unresolved_deliverables listing it.
    let resp = maria
        .post_json(
            &format!("/api/v1/projects/{project}/close"),
            &serde_json::json!({"deliverable_resolutions": []}),
        )
        .await;
    assert_eq!(resp.status(), 409);
    let err: serde_json::Value = maria.json(resp).await;
    assert_eq!(err["error"]["code"], "unresolved_deliverables");
    assert_eq!(err["error"]["deliverables"][0]["id"], open.id);
    assert_eq!(err["error"]["deliverables"][0]["status"], "agreed");

    // Project still approved, deliverable still agreed.
    let p: (String,) = sqlx::query_as("SELECT status FROM projects WHERE id = ?")
        .bind(&project)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(p.0, "approved");

    // Resolve it (cancel) → project closes.
    let resp = maria
        .post_json(
            &format!("/api/v1/projects/{project}/close"),
            &serde_json::json!({"deliverable_resolutions": [
                {"deliverable_id": open.id, "action": "cancel", "note": "team could not deliver"}
            ]}),
        )
        .await;
    assert_eq!(resp.status(), 200, "close should succeed");
    let ws: pitcairn::dto::ProjectWorkspaceDto = maria.json(resp).await;
    assert_eq!(ws.project.status, "closed");
    assert_eq!(ws.results.deliverables[0].status, "cancelled");
}

#[tokio::test]
async fn close_project_rejects_wrong_status_and_non_coordinator() {
    let app = spawn_app(true).await;
    let anna = persona(&app, "anna").await;
    let maria = persona(&app, "maria").await;
    let ruth = persona(&app, "ruth").await;
    let project = seeded_project(&app).await; // draft

    // Only a coordinator may close.
    let resp = ruth
        .post_json(
            &format!("/api/v1/projects/{project}/close"),
            &serde_json::json!({"deliverable_resolutions": []}),
        )
        .await;
    assert_eq!(resp.status(), 403);

    // Draft cannot close → invalid_transition from the shared state machine.
    let resp = maria
        .post_json(
            &format!("/api/v1/projects/{project}/close"),
            &serde_json::json!({"deliverable_resolutions": []}),
        )
        .await;
    assert_eq!(resp.status(), 409);
    let err: serde_json::Value = maria.json(resp).await;
    assert_eq!(err["error"]["code"], "invalid_transition");

    // Team members cannot close at all.
    let resp = anna
        .post_json(
            &format!("/api/v1/projects/{project}/close"),
            &serde_json::json!({"deliverable_resolutions": []}),
        )
        .await;
    assert_eq!(resp.status(), 403);
}

#[tokio::test]
async fn workspace_results_section_shows_deliverables_and_samples() {
    let app = spawn_app(true).await;
    let anna = persona(&app, "anna").await;
    let maria = persona(&app, "maria").await;
    let project = seeded_project(&app).await;
    let anna_id = user_id(&app, ANNA).await;
    let maria_id = user_id(&app, MARIA).await;

    let d = agreed_deliverable(&anna, &maria, &project, "dataset", &anna_id, &maria_id).await;

    let resp = anna.get(&format!("/api/v1/projects/{project}")).await;
    assert_eq!(resp.status(), 200);
    let ws: pitcairn::dto::ProjectWorkspaceDto = anna.json(resp).await;
    assert_eq!(ws.project.id, project);
    assert_eq!(ws.results.deliverables.len(), 1);
    assert_eq!(ws.results.deliverables[0].id, d.id);
    assert_eq!(ws.results.deliverables[0].agreement_state, "agreed");
    assert_eq!(ws.results.samples_count, 0);
}
