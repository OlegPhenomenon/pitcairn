//! API request/response DTOs. Every DTO derives `ts_rs::TS` and is exported
//! to `frontend/src/api/generated/` — the frontend never hand-writes types.
//! Regenerate with `pitcairn export-types` or `cargo test export_bindings`.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

pub mod b;
pub use b::*;

pub const EXPORT_DIR: &str = "frontend/src/api/generated/";

macro_rules! export_all {
    ($($t:ty),* $(,)?) => {
        /// Export every DTO's TypeScript bindings (CLI `export-types`).
        pub fn export_all() -> Result<(), ts_rs::ExportError> {
            $( <$t as TS>::export()?; )*
            b::export()?;
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

// ---------------------------------------------------------------------------
// Slice C: deliverables, submissions, publication, catalog, samples
// ---------------------------------------------------------------------------

/// One row of a submission's data dictionary (§4 `data_dictionary_json`).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct DataDictionaryEntryDto {
    pub column: String,
    pub description: String,
    pub unit: String,
    pub method: String,
}

/// A link handed in with a submission (request body shape).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct SubmissionLinkInput {
    pub url: String,
    pub description: String,
    pub version_label: String,
    pub access_notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct ExternalLinkDto {
    pub id: String,
    pub submission_id: String,
    pub url: String,
    pub description: String,
    pub version_label: String,
    pub access_notes: String,
    pub last_checked_at: Option<String>,
    /// "available" | "unavailable" | null (never checked).
    pub last_status: Option<String>,
    pub available: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct SubmissionFileDto {
    pub document_version_id: String,
    pub document_id: String,
    pub title: String,
    pub number: i64,
    pub mime: String,
    pub size: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct SubmissionDto {
    pub id: String,
    pub deliverable_id: String,
    pub number: i64,
    pub status: String,
    pub note: String,
    pub data_dictionary: Vec<DataDictionaryEntryDto>,
    pub submitted_by: String,
    pub submitted_at: String,
    pub reviewed_by: Option<String>,
    pub review_note: Option<String>,
    pub reviewed_at: Option<String>,
    pub files: Vec<SubmissionFileDto>,
    pub links: Vec<ExternalLinkDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct DeliverableDto {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub description: String,
    pub kind: String,
    pub due_date: String,
    pub sender_id: String,
    pub sender_name: String,
    pub recipient_id: String,
    pub recipient_name: String,
    pub status: String,
    pub terms_version: i64,
    pub team_agreed_at: Option<String>,
    pub team_agreed_by: Option<String>,
    pub staff_agreed_at: Option<String>,
    pub staff_agreed_by: Option<String>,
    /// Derived: "pending" | "team_only" | "staff_only" | "agreed".
    pub agreement_state: String,
    pub resolution_note: Option<String>,
    pub publish_level: String,
    pub embargo_until: Option<String>,
    pub published_at: Option<String>,
    pub published_by: Option<String>,
    pub created_by: String,
    pub created_at: String,
    /// Latest submission (any status) and the latest accepted one (§4:
    /// "the deliverable shows the accepted one and the latest one").
    pub latest_submission: Option<SubmissionDto>,
    pub accepted_submission: Option<SubmissionDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct CreateDeliverableRequest {
    pub title: String,
    pub description: Option<String>,
    pub kind: String,
    pub due_date: String,
    /// Team member who must deliver (must be an active project member).
    pub sender_id: String,
    /// Staff member who receives (must hold the coordinator role).
    pub recipient_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct UpdateDeliverableRequest {
    pub title: Option<String>,
    pub description: Option<String>,
    pub kind: Option<String>,
    pub due_date: Option<String>,
    pub sender_id: Option<String>,
    pub recipient_id: Option<String>,
    /// Required when `due_date` changes (recorded in deliverable_due_changes).
    pub reason: Option<String>,
}

/// Body for waive / cancel / request-changes style endpoints.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct NoteRequest {
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct CreateSubmissionRequest {
    pub note: Option<String>,
    pub data_dictionary: Option<Vec<DataDictionaryEntryDto>>,
    pub document_version_ids: Option<Vec<String>>,
    pub links: Option<Vec<SubmissionLinkInput>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct AcceptSubmissionResponse {
    pub submission: SubmissionDto,
    /// Acceptance marks receipt, not scientific validity.
    pub message: String,
    /// Rows skipped while parsing measurement CSVs (never fatal).
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct UpdatePublicationRequest {
    /// "none" | "metadata" | "metadata_and_files".
    pub publish_level: String,
    /// YYYY-MM-DD or null; files become public only after this date.
    pub embargo_until: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct PublicationUpdateResponse {
    pub deliverable: DeliverableDto,
    /// Reminder shown to the coordinator (§4).
    pub warning: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct SetPublicationFilesRequest {
    pub document_version_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct PublicationFilesResponse {
    pub document_version_ids: Vec<String>,
    /// Reminder shown to the coordinator (§4).
    pub warning: String,
}

/// One deliverable needing an explicit resolution at project close.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct UnresolvedDeliverableDto {
    pub id: String,
    pub title: String,
    pub status: String,
    pub due_date: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct DeliverableResolution {
    pub deliverable_id: String,
    /// "waive" | "cancel".
    pub action: String,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct CloseProjectRequest {
    pub deliverable_resolutions: Vec<DeliverableResolution>,
}

/// Coordinator dashboard row: a submitted external link that failed its
/// availability check (§4 "Unavailable links are flagged to the coordinator").
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct UnavailableLinkDto {
    pub link_id: String,
    pub url: String,
    pub description: String,
    pub last_checked_at: Option<String>,
    pub last_status: Option<String>,
    pub submission_id: String,
    pub deliverable_id: String,
    pub deliverable_title: String,
    pub project_id: String,
    pub project_reference: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct SampleDto {
    pub id: String,
    pub project_id: String,
    pub code: String,
    pub site_id: Option<String>,
    pub collected_on: Option<String>,
    pub material: String,
    pub custodian_org: String,
    pub storage_location: String,
    pub notes: String,
    pub related_deliverable_ids: Vec<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct CreateSampleRequest {
    pub code: String,
    pub site_id: Option<String>,
    pub collected_on: Option<String>,
    pub material: Option<String>,
    pub custodian_org: Option<String>,
    pub storage_location: Option<String>,
    pub notes: Option<String>,
    pub related_deliverable_ids: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct UpdateSampleRequest {
    pub code: Option<String>,
    /// Absent = unchanged; explicit null clears the site link.
    pub site_id: Option<Option<String>>,
    pub collected_on: Option<Option<String>>,
    pub material: Option<String>,
    pub custodian_org: Option<String>,
    pub storage_location: Option<String>,
    pub notes: Option<String>,
    pub related_deliverable_ids: Option<Vec<String>>,
}

// --- Public catalog (no auth) ---

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct PublicSiteDto {
    pub name: String,
    #[ts(type = "unknown")]
    pub geometry: Value,
    /// True when the geometry was replaced by its 0.1°-grid bbox (§4).
    pub generalized: bool,
    pub sensitive: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct PublicFileDto {
    pub document_version_id: String,
    pub title: String,
    pub size: i64,
    pub mime: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct PublicDeliverableDto {
    pub id: String,
    pub title: String,
    pub description: String,
    pub kind: String,
    pub publish_level: String,
    pub published_at: Option<String>,
    /// Set while the embargo has not passed; `files` stays empty until then.
    pub files_available_from: Option<String>,
    pub files: Vec<PublicFileDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct PublicProjectDto {
    pub reference: Option<String>,
    pub title: String,
    pub organisation: String,
    pub year: Option<i64>,
    pub summary: String,
    pub keywords: String,
    pub sites: Vec<PublicSiteDto>,
    pub deliverables: Vec<PublicDeliverableDto>,
}

// --- Project workspace assembly (§5 GET /projects/{id}) ---

/// The most urgent open action item for the viewer, rendered as the
/// workspace headline ("Maria asks you to add a description of sites").
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct PrimaryMessageDto {
    pub action_item_id: String,
    pub thread_id: String,
    pub title: String,
    pub by_name: String,
    pub created_at: String,
}

/// Slice C's workspace section: results & samples (§9 task).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct ResultsSectionDto {
    pub deliverables: Vec<DeliverableDto>,
    pub samples_count: i64,
}

/// `GET /projects/{id}` payload. Every slice adds ONE top-level field plus
/// one assembly line in `routes::projects::load_workspace`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct ProjectWorkspaceDto {
    pub project: ProjectDto,
    pub primary_message: Option<PrimaryMessageDto>,
    pub results: ResultsSectionDto,
    /// Slice B: trips incl. bookings. Empty for viewers without project access.
    pub trips: Vec<TripDto>,
    /// Slice B: finance sees all invoices; everyone else sees issued + cancelled only.
    pub invoices: Vec<InvoiceDto>,
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
    DataDictionaryEntryDto,
    SubmissionLinkInput,
    ExternalLinkDto,
    SubmissionFileDto,
    SubmissionDto,
    DeliverableDto,
    CreateDeliverableRequest,
    UpdateDeliverableRequest,
    NoteRequest,
    CreateSubmissionRequest,
    AcceptSubmissionResponse,
    UpdatePublicationRequest,
    PublicationUpdateResponse,
    SetPublicationFilesRequest,
    PublicationFilesResponse,
    UnresolvedDeliverableDto,
    DeliverableResolution,
    CloseProjectRequest,
    UnavailableLinkDto,
    SampleDto,
    CreateSampleRequest,
    UpdateSampleRequest,
    PublicSiteDto,
    PublicFileDto,
    PublicDeliverableDto,
    PublicProjectDto,
    PrimaryMessageDto,
    ResultsSectionDto,
    ProjectWorkspaceDto,
);
