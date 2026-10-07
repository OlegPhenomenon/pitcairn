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

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct SettingsDto {
    pub mail_enabled: bool,
    pub organisation_name: String,
    pub reference_prefix: String,
    pub public_catalog_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct AdminCreateUserRequest {
    pub email: String,
    pub name: String,
    pub organisation: String,
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct AdminPatchUserRequest {
    pub name: Option<String>,
    pub organisation: Option<String>,
    pub email: Option<String>,
    pub disabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct GrantRoleRequest {
    pub role: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct JobDto {
    pub id: String,
    pub kind: String,
    #[ts(type = "Record<string, unknown>")]
    pub payload: Value,
    pub dedupe_key: Option<String>,
    pub status: String,
    pub attempts: i64,
    pub max_attempts: i64,
    pub run_after: String,
    pub last_error: Option<String>,
    pub locked_until: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct AuditEventDto {
    pub id: String,
    pub at: String,
    pub actor_id: Option<String>,
    pub actor_label: String,
    pub action: String,
    pub entity_type: String,
    pub entity_id: String,
    pub project_id: Option<String>,
    pub visibility: String,
    pub summary: String,
    #[ts(type = "unknown | null")]
    pub before: Option<Value>,
    #[ts(type = "unknown | null")]
    pub after: Option<Value>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct PersonaDto {
    pub key: String,
    pub user_id: String,
    pub name: String,
    pub email: String,
    pub organisation: String,
    pub role: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct PersonasResponse {
    pub personas: Vec<PersonaDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct DemoSwitchRequest {
    pub persona_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct MailMessageDto {
    pub id: i64,
    pub to_email: String,
    pub subject: String,
    pub body_text: String,
    pub status: String,
    pub error: Option<String>,
    pub created_at: String,
    pub sent_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct DemoTotpResponse {
    pub user_id: String,
    pub code: String,
}

impl ListQuery {
    pub fn limit(&self) -> i64 {
        self.limit.unwrap_or(50).clamp(1, 200)
    }

    pub fn offset(&self) -> i64 {
        self.offset.unwrap_or(0).max(0)
    }
}

// ---------------------------------------------------------------------------
// Slice D: dashboard, search, reports, import, assist
// ---------------------------------------------------------------------------

/// One item in a dashboard section (§5).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct DashboardItemDto {
    pub kind: String,
    pub title: String,
    pub subtitle: String,
    pub project_id: Option<String>,
    pub project_reference: Option<String>,
    pub due_date: Option<String>,
    pub link: String,
}

/// `GET /dashboard` — sections keyed by their §5 names; a user with several
/// roles gets every one of their sections.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct DashboardResponse {
    #[ts(type = "Record<string, Array<DashboardItemDto>>")]
    pub sections: std::collections::BTreeMap<String, Vec<DashboardItemDto>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct SearchProjectItemDto {
    pub id: String,
    pub reference: Option<String>,
    pub title: String,
    pub organisation: String,
    pub status: String,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
    pub deliverables_received: i64,
    pub deliverables_overdue: i64,
}

/// Per-project row of `GET /reports/deliverables`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct DeliverableReportRowDto {
    pub project_id: String,
    pub project_reference: Option<String>,
    pub project_title: String,
    pub organisation: String,
    pub agreed: i64,
    pub received: i64,
    pub accepted: i64,
    pub overdue: i64,
    pub waived: i64,
    pub total: i64,
}

/// Aggregated totals (by year or by organisation).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct DeliverableReportTotalsDto {
    pub key: String,
    pub agreed: i64,
    pub received: i64,
    pub accepted: i64,
    pub overdue: i64,
    pub waived: i64,
    pub total: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct DeliverablesReportResponse {
    pub projects: Vec<DeliverableReportRowDto>,
    pub totals_by_year: Vec<DeliverableReportTotalsDto>,
    pub totals_by_organisation: Vec<DeliverableReportTotalsDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct MeasurementVariableDto {
    pub variable_key: String,
    pub unit: String,
    pub count: i64,
    pub projects: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct MeasurementPointDto {
    pub project_id: String,
    pub project_reference: Option<String>,
    pub project_title: String,
    pub site: String,
    pub observed_on: String,
    pub value: f64,
    pub unit: String,
    pub source_label: String,
}

/// Series of points sharing one unit — charts never mix units (§4).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct MeasurementSeriesDto {
    pub unit: String,
    pub points: Vec<MeasurementPointDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct MeasurementsReportResponse {
    pub variable_key: String,
    pub series: Vec<MeasurementSeriesDto>,
}

/// One parsed CSV row of a legacy import preview, with per-field errors and
/// the duplicate match (if any).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct ImportRowPreviewDto {
    pub index: i64,
    #[ts(type = "Record<string, string>")]
    pub data: std::collections::BTreeMap<String, String>,
    #[ts(type = "Record<string, string>")]
    pub errors: std::collections::BTreeMap<String, String>,
    pub duplicate_of: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct ImportBatchDto {
    pub id: String,
    pub kind: String,
    pub status: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct LegacyImportPreviewResponse {
    pub batch: ImportBatchDto,
    pub rows: Vec<ImportRowPreviewDto>,
}

/// Preview of a project-archive import (§8).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct ArchiveImportPreviewResponse {
    pub batch: ImportBatchDto,
    pub project_reference: Option<String>,
    pub project_title: String,
    /// table name -> row count
    #[ts(type = "Record<string, number>")]
    pub records: std::collections::BTreeMap<String, i64>,
    pub files: i64,
    pub conflicts: Vec<String>,
    /// Emails of users matched to existing accounts.
    pub matched_users: Vec<String>,
    /// Emails of users that will be created as disabled stubs.
    pub new_users: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct ImportCommitResponse {
    pub batch_id: String,
    pub status: String,
    pub created: i64,
    pub skipped: i64,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct AssistExtractRequest {
    pub template_version_id: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct AssistSuggestionDto {
    pub field_key: String,
    #[ts(type = "unknown")]
    pub value: Value,
    pub confidence: f64,
    pub source_excerpt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct AssistExtractResponse {
    pub suggestions: Vec<AssistSuggestionDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct AssistSummaryRequest {
    pub project_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct AssistSummaryResponse {
    pub project_id: String,
    pub summary: String,
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
    SettingsDto,
    AdminCreateUserRequest,
    AdminPatchUserRequest,
    GrantRoleRequest,
    JobDto,
    AuditEventDto,
    PersonaDto,
    PersonasResponse,
    DemoSwitchRequest,
    MailMessageDto,
    DemoTotpResponse,
    DashboardItemDto,
    DashboardResponse,
    SearchProjectItemDto,
    DeliverableReportRowDto,
    DeliverableReportTotalsDto,
    DeliverablesReportResponse,
    MeasurementVariableDto,
    MeasurementPointDto,
    MeasurementSeriesDto,
    MeasurementsReportResponse,
    ImportRowPreviewDto,
    ImportBatchDto,
    LegacyImportPreviewResponse,
    ArchiveImportPreviewResponse,
    ImportCommitResponse,
    AssistExtractRequest,
    AssistSuggestionDto,
    AssistExtractResponse,
    AssistSummaryRequest,
    AssistSummaryResponse,
);
