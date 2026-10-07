//! `/assist/*` — mock AI endpoints (§5). When `PITCAIRN_AI_MODE=off` they
//! return 503 `ai_unavailable`; nothing else calls them and the UI must work
//! fully without them.

use axum::Json;
use axum::extract::State;
use axum::routing::{Router, post};

use crate::AppState;
use crate::assist;
use crate::authz::{self, Actor, ProjectAccess};
use crate::dto::{
    AssistExtractRequest, AssistExtractResponse, AssistSummaryRequest, AssistSummaryResponse,
};
use crate::error::{AppError, AppResult};
use crate::validation::FieldErrors;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/assist/extract-fields", post(extract_fields))
        .route("/assist/summary", post(summary))
}

fn ai_enabled(state: &AppState) -> AppResult<()> {
    if state.config.ai_mode == "off" {
        return Err(AppError::Unavailable {
            code: "ai_unavailable".into(),
            message: "the AI assistant is disabled (PITCAIRN_AI_MODE=off)".into(),
        });
    }
    Ok(())
}

async fn load_schema(state: &AppState, template_version_id: &str) -> AppResult<serde_json::Value> {
    let schema_json: Option<String> =
        sqlx::query_scalar("SELECT schema_json FROM template_versions WHERE id = ?")
            .bind(template_version_id)
            .fetch_optional(&state.pool)
            .await?;
    let schema_json = schema_json.ok_or(AppError::NotFound)?;
    serde_json::from_str(&schema_json).map_err(AppError::internal)
}

async fn extract_fields(
    State(state): State<AppState>,
    _actor: Actor,
    Json(req): Json<AssistExtractRequest>,
) -> AppResult<Json<AssistExtractResponse>> {
    ai_enabled(&state)?;
    let mut errors = FieldErrors::new();
    errors.require(
        "template_version_id",
        &req.template_version_id,
        "template_version_id is required",
    );
    errors.require("text", &req.text, "text is required");
    errors.max_len("text", &req.text, 200_000);
    errors.finish()?;

    let schema = load_schema(&state, &req.template_version_id).await?;
    Ok(Json(AssistExtractResponse {
        suggestions: assist::extract_fields(&schema, &req.text),
    }))
}

async fn summary(
    State(state): State<AppState>,
    actor: Actor,
    Json(req): Json<AssistSummaryRequest>,
) -> AppResult<Json<AssistSummaryResponse>> {
    ai_enabled(&state)?;
    let mut errors = FieldErrors::new();
    errors.require("project_id", &req.project_id, "project_id is required");
    errors.finish()?;

    let access = authz::project_access(&state.pool, &actor, &req.project_id).await?;
    if access == ProjectAccess::None {
        return Err(AppError::NotFound);
    }

    let row: Option<(String, String, String, String)> = sqlx::query_as(
        "SELECT p.title, p.summary, p.answers_json, tv.schema_json
         FROM projects p JOIN template_versions tv ON tv.id = p.template_version_id
         WHERE p.id = ?",
    )
    .bind(&req.project_id)
    .fetch_optional(&state.pool)
    .await?;
    let (title, summary, answers_json, schema_json) = row.ok_or(AppError::NotFound)?;
    let answers: serde_json::Value =
        serde_json::from_str(&answers_json).unwrap_or(serde_json::Value::Null);
    let schema: serde_json::Value =
        serde_json::from_str(&schema_json).unwrap_or(serde_json::Value::Null);

    Ok(Json(AssistSummaryResponse {
        project_id: req.project_id,
        summary: assist::summarize_answers(&schema, &answers, &title, &summary),
    }))
}
