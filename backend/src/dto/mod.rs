//! API request/response DTOs. Every DTO derives `ts_rs::TS` and is exported
//! to `frontend/src/api/generated/` — the frontend never hand-writes types.
//! Regenerate with `pitcairn export-types` or `cargo test export_bindings`.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

pub const EXPORT_DIR: &str = "frontend/src/api/generated/";

macro_rules! export_all {
    ($($t:ty),* $(,)?) => {
        /// Export every DTO's TypeScript bindings (CLI `export-types`).
        pub fn export_all() -> Result<(), ts_rs::ExportError> {
            $( <$t as TS>::export()?; )*
            Ok(())
        }
    };
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct UserDto {
    pub id: String,
    pub email: String,
    pub name: String,
    pub organisation: String,
    pub roles: Vec<String>,
    pub totp_enrolled: bool,
    pub disabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct MeResponse {
    pub user: UserDto,
    pub mfa_verified: bool,
    pub demo_mode: bool,
}

/// Standard list envelope: `{items, total}` with `?limit&offset`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct ListResponse<T> {
    pub items: Vec<T>,
    pub total: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct ListQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct RegisterRequest {
    pub email: String,
    pub name: String,
    pub organisation: String,
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct LoginResponse {
    pub user: UserDto,
    pub mfa_required: bool,
    pub mfa_enrollment_required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct MfaEnrollResponse {
    pub secret: String,
    pub otpauth_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct MfaCodeRequest {
    pub code: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct AcceptInvitationResponse {
    pub project_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct CreateUploadRequest {
    pub filename: String,
    pub size: u64,
    pub sha256: String,
    pub mime: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct UploadStateDto {
    pub upload_id: String,
    pub chunk_size: u64,
    pub chunks_total: u64,
    pub chunks_received: Vec<u64>,
    pub status: String,
    pub file_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct CompleteUploadResponse {
    pub file_id: String,
    pub scan_status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct CreateProjectRequest {
    pub template_key: String,
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct ProjectDto {
    pub id: String,
    pub reference: Option<String>,
    pub title: String,
    pub summary: String,
    pub keywords: String,
    pub organisation: String,
    pub status: String,
    pub template_version_id: String,
    #[ts(type = "Record<string, unknown>")]
    pub answers: Value,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
    pub version: i64,
    pub created_by: String,
    pub created_at: String,
    pub my_access: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct ProjectListItemDto {
    pub id: String,
    pub reference: Option<String>,
    pub title: String,
    pub status: String,
    pub organisation: String,
    pub created_at: String,
    pub my_role: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct CreateDocumentRequest {
    pub slot_key: Option<String>,
    pub title: String,
    pub category: String,
    pub file_id: String,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct NewVersionRequest {
    pub file_id: String,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct DocumentVersionDto {
    pub id: String,
    pub document_id: String,
    pub number: i64,
    pub file_id: String,
    pub note: String,
    pub uploaded_by: String,
    pub uploaded_at: String,
    pub scan_status: String,
    pub size: i64,
    pub mime: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct DocumentDto {
    pub id: String,
    pub project_id: String,
    pub slot_key: Option<String>,
    pub title: String,
    pub category: String,
    pub created_by: String,
    pub created_at: String,
    pub latest_version: Option<DocumentVersionDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct NotificationDto {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub body: String,
    pub link: String,
    pub project_id: Option<String>,
    pub read_at: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct ReadAllResponse {
    pub marked: i64,
}

impl ListQuery {
    pub fn limit(&self) -> i64 {
        self.limit.unwrap_or(50).clamp(1, 200)
    }

    pub fn offset(&self) -> i64 {
        self.offset.unwrap_or(0).max(0)
    }
}

export_all!(
    UserDto,
    MeResponse,
    ListResponse<String>,
    ListQuery,
    RegisterRequest,
    LoginRequest,
    LoginResponse,
    MfaEnrollResponse,
    MfaCodeRequest,
    AcceptInvitationResponse,
    CreateUploadRequest,
    UploadStateDto,
    CompleteUploadResponse,
    CreateProjectRequest,
    ProjectDto,
    ProjectListItemDto,
    CreateDocumentRequest,
    NewVersionRequest,
    DocumentDto,
    DocumentVersionDto,
    NotificationDto,
    ReadAllResponse,
);
