//! Slice A test helpers: application lifecycle shortcuts.

use serde_json::{Value, json};

use super::{Client, TestApp};

pub async fn body(resp: reqwest::Response) -> (u16, Value) {
    let status = resp.status().as_u16();
    let text = resp.text().await.expect("read body");
    let value = if text.is_empty() {
        Value::Null
    } else {
        serde_json::from_str(&text).unwrap_or(Value::String(text))
    };
    (status, value)
}

pub async fn get(client: &Client, path: &str) -> (u16, Value) {
    body(client.get(&format!("/api/v1{path}")).await).await
}

pub async fn post(client: &Client, path: &str, payload: Value) -> (u16, Value) {
    body(client.post_json(&format!("/api/v1{path}"), &payload).await).await
}

pub async fn patch(client: &Client, path: &str, payload: Value) -> (u16, Value) {
    body(client.patch_json(&format!("/api/v1{path}"), &payload).await).await
}

pub async fn put(client: &Client, path: &str, payload: Value) -> (u16, Value) {
    body(client.put_json(&format!("/api/v1{path}"), &payload).await).await
}

pub async fn delete(client: &Client, path: &str) -> (u16, Value) {
    body(client.delete(&format!("/api/v1{path}")).await).await
}

/// `POST /projects/{id}/submit`, optionally with an Idempotency-Key.
pub async fn submit(client: &Client, project_id: &str, key: Option<&str>) -> (u16, Value) {
    let mut req = client.request(
        reqwest::Method::POST,
        &format!("/api/v1/projects/{project_id}/submit"),
    );
    if let Some(k) = key {
        req = req.header("Idempotency-Key", k);
    }
    body(req.send().await.expect("send submit")).await
}

pub async fn user_id(app: &TestApp, email: &str) -> String {
    sqlx::query_scalar("SELECT id FROM users WHERE email = ?")
        .bind(email)
        .fetch_one(&app.pool)
        .await
        .expect("user exists")
}

pub fn persona_email(key: &str) -> &'static str {
    pitcairn::seed::PERSONAS
        .iter()
        .find(|p| p.key == key)
        .expect("persona")
        .email
}

pub async fn persona_id(app: &TestApp, key: &str) -> String {
    user_id(app, persona_email(key)).await
}

/// Register a brand-new researcher (logged in via the register cookie).
pub async fn register(app: &TestApp, email: &str, name: &str) -> Client {
    let client = Client::anonymous(app);
    let (status, _) = post(
        &client,
        "/auth/register",
        json!({"email": email, "name": name, "organisation": "Fjord Institute (fictional)",
               "password": "correct-horse-battery"}),
    )
    .await;
    assert_eq!(status, 200, "register");
    client
}

pub async fn upload_clean_file(client: &Client, app: &TestApp, bytes: &[u8]) -> String {
    use sha2::Digest;
    let sha = hex::encode(sha2::Sha256::digest(bytes));
    let (status, state) = post(
        client,
        "/uploads",
        json!({"filename": "doc.txt", "size": bytes.len(), "sha256": sha, "mime": "text/plain"}),
    )
    .await;
    assert_eq!(status, 200, "create upload: {state}");
    let upload_id = state["upload_id"].as_str().unwrap().to_string();
    let chunk = client
        .request(
            reqwest::Method::PUT,
            &format!("/api/v1/uploads/{upload_id}/chunks/0"),
        )
        .body(bytes.to_vec())
        .send()
        .await
        .expect("send chunk");
    assert_eq!(chunk.status(), 200);
    let (status, done) = body(
        client
            .post(&format!("/api/v1/uploads/{upload_id}/complete"))
            .await,
    )
    .await;
    assert_eq!(status, 200, "complete: {done}");
    while pitcairn::jobs::run_once(&app.state).await.expect("run job") {}
    done["file_id"].as_str().unwrap().to_string()
}

/// Upload a document into a project slot; returns `(document_id, version_id)`.
pub async fn upload_document(
    client: &Client,
    app: &TestApp,
    project_id: &str,
    slot: Option<&str>,
    category: &str,
    title: &str,
) -> (String, String) {
    let file_id = upload_clean_file(client, app, format!("{project_id}:{title}").as_bytes()).await;
    let (status, doc) = post(
        client,
        &format!("/projects/{project_id}/documents"),
        json!({"slot_key": slot, "title": title, "category": category, "file_id": file_id}),
    )
    .await;
    assert_eq!(status, 201, "create document: {doc}");
    (
        doc["id"].as_str().unwrap().to_string(),
        doc["latest_version"]["id"].as_str().unwrap().to_string(),
    )
}

pub async fn create_project(client: &Client, template_key: &str, title: &str) -> String {
    let (status, p) = post(
        client,
        "/projects",
        json!({"template_key": template_key, "title": title}),
    )
    .await;
    assert_eq!(status, 201, "create project: {p}");
    p["id"].as_str().unwrap().to_string()
}

pub async fn project_version(client: &Client, project_id: &str) -> i64 {
    let (status, ws) = get(client, &format!("/projects/{project_id}")).await;
    assert_eq!(status, 200, "workspace: {ws}");
    ws["project"]["version"].as_i64().unwrap()
}

pub async fn add_site(client: &Client, project_id: &str, name: &str, sensitive: bool) -> String {
    let (status, site) = post(
        client,
        &format!("/projects/{project_id}/sites"),
        json!({"name": name, "sensitive": sensitive,
               "geometry": {"type": "Point", "coordinates": [-130.1043, -25.0661]}}),
    )
    .await;
    assert_eq!(status, 201, "create site: {site}");
    site["id"].as_str().unwrap().to_string()
}

/// A complete `fieldwork_permit` draft (no required documents).
pub async fn fieldwork_draft(client: &Client, title: &str) -> String {
    let project_id = create_project(client, "fieldwork_permit", title).await;
    let site_id = add_site(client, &project_id, "Bounty Bay reef", true).await;
    let version = project_version(client, &project_id).await;
    let (status, saved) = patch(
        client,
        &format!("/projects/{project_id}"),
        json!({
            "version": version,
            "summary": "Fictional fieldwork",
            "start_date": "2026-11-01", "end_date": "2026-11-30",
            "answers": {
                "purpose": "Survey coral cover",
                "dates": {"start": "2026-11-01", "end": "2026-11-30"},
                "team_size": 3,
                "activities": ["diving"],
                "sites": [site_id],
            }
        }),
    )
    .await;
    assert_eq!(status, 200, "autosave: {saved}");
    project_id
}

/// Draft → submitted → in_review; returns the project id.
pub async fn in_review_project(lead: &Client, coordinator: &Client, title: &str) -> String {
    let project_id = fieldwork_draft(lead, title).await;
    let (status, res) = submit(lead, &project_id, None).await;
    assert_eq!(status, 200, "submit: {res}");
    let (status, res) = post(
        coordinator,
        &format!("/projects/{project_id}/screen"),
        json!({}),
    )
    .await;
    assert_eq!(status, 200, "screen: {res}");
    project_id
}

pub async fn latest_revision_id(app: &TestApp, project_id: &str) -> String {
    sqlx::query_scalar(
        "SELECT id FROM project_revisions WHERE project_id = ? ORDER BY number DESC LIMIT 1",
    )
    .bind(project_id)
    .fetch_one(&app.pool)
    .await
    .expect("revision")
}

/// Draft and issue a decision as `decision_maker`; returns the decision.
pub async fn issue_decision(decision_maker: &Client, project_id: &str, payload: Value) -> Value {
    let (status, draft) = post(
        decision_maker,
        &format!("/projects/{project_id}/decisions"),
        payload,
    )
    .await;
    assert_eq!(status, 201, "draft decision: {draft}");
    let id = draft["id"].as_str().unwrap();
    let (status, issued) = post(decision_maker, &format!("/decisions/{id}/issue"), json!({})).await;
    assert_eq!(status, 200, "issue decision: {issued}");
    issued
}

pub async fn approved_project(
    app: &TestApp,
    lead: &Client,
    coordinator: &Client,
    decision_maker: &Client,
    title: &str,
) -> (String, Value) {
    let project_id = in_review_project(lead, coordinator, title).await;
    let revision = latest_revision_id(app, &project_id).await;
    let permit = issue_decision(
        decision_maker,
        &project_id,
        json!({
            "kind": "permit", "project_revision_id": revision,
            "basis": "Application meets the base-use policy",
            "legal_reference": "MSB Research Policy (demo) s.4",
            "valid_from": "2026-11-01", "valid_to": "2026-11-30",
            "permitted_activities": ["Diving surveys"],
            "conditions": ["No anchoring on live coral"],
        }),
    )
    .await;
    (project_id, permit)
}
