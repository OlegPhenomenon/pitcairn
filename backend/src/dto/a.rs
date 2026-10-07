//! Slice A DTOs — application lifecycle: templates, project workspace,
//! team, sites, conversation, reviews, decisions, change requests.
//! Kept in a separate module so parallel slices don't collide on the
//! shared `export_all!` list in `mod.rs`.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

/// `Option<Option<T>>` for PATCH bodies: absent → `None` (keep),
/// `null` → `Some(None)` (clear), value → `Some(Some(v))` (set).
/// Use with `#[serde(default, deserialize_with = "double_option")]`.
fn double_option<'de, D, T>(de: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(de).map(Some)
}

macro_rules! export_all {
    ($($t:ty),* $(,)?) => {
        /// Export this slice's TypeScript bindings (called from dto::export_all).
        pub fn export() -> Result<(), ts_rs::ExportError> {
            $( <$t as TS>::export()?; )*
            Ok(())
        }
    };
}

// ---------------------------------------------------------------------------
// Templates
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct TemplateDto {
    pub id: String,
    pub key: String,
    pub name: String,
    pub description: String,
    /// Latest published version number, if any (new projects bind to it).
    pub latest_published_version: Option<i64>,
    /// Latest draft version number, if any.
    pub draft_version: Option<i64>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct TemplateVersionDto {
    pub id: String,
    pub template_id: String,
    pub template_key: String,
    pub version: i64,
    pub status: String,
    #[ts(type = "Record<string, unknown>")]
    pub schema: Value,
    pub published_at: Option<String>,
    pub published_by: Option<String>,
    pub created_at: String,
}

/// Body for `POST /templates/{key}/versions` and `PUT /template-versions/{id}`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct TemplateSchemaRequest {
    #[ts(type = "Record<string, unknown>")]
    pub schema: Value,
}

// ---------------------------------------------------------------------------
// Projects — workspace, lifecycle
// ---------------------------------------------------------------------------

/// `PATCH /projects/{id}` — autosave. `version` is the optimistic-concurrency
/// token the client loaded; mismatch → 409 `stale_version`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct PatchProjectRequest {
    pub version: i64,
    pub title: Option<String>,
    pub summary: Option<String>,
    pub keywords: Option<String>,
    pub organisation: Option<String>,
    /// Absent = keep; null = clear; string = set.
    #[serde(default, deserialize_with = "double_option")]
    #[ts(optional = nullable)]
    pub start_date: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    #[ts(optional = nullable)]
    pub end_date: Option<Option<String>>,
    /// Full replacement of the answers object when present.
    #[ts(optional, type = "Record<string, unknown>")]
    pub answers: Option<Value>,
}

/// `PATCH /projects/{id}` autosave response — the new optimistic `version`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct PatchProjectResponse {
    pub version: i64,
    pub saved_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct SubmitResponse {
    pub project_id: String,
    pub reference: String,
    pub revision_number: i64,
    pub status: String,
    pub version: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct WithdrawRequest {
    pub reason: Option<String>,
}

/// `POST /projects/{id}/upgrade-template` response: answers were copied by
/// field key; keys absent from the new schema are listed in `dropped_keys`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct UpgradeTemplateResponse {
    pub template_version_id: String,
    pub template_version_number: i64,
    pub version: i64,
    pub dropped_keys: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct TeamMemberDto {
    pub user_id: String,
    pub email: String,
    pub name: String,
    pub organisation: String,
    pub role: String,
    pub added_at: String,
    pub removed_at: Option<String>,
    pub removed_by: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct SiteDto {
    pub id: String,
    pub project_id: String,
    pub name: String,
    /// GeoJSON Point|Polygon — generalized bbox polygon when `generalized`.
    #[ts(type = "Record<string, unknown>")]
    pub geometry: Value,
    pub sensitive: bool,
    /// True when the geometry was generalized (0.1° grid) for this viewer.
    pub generalized: bool,
    pub min_lat: f64,
    pub min_lng: f64,
    pub max_lat: f64,
    pub max_lng: f64,
    pub created_at: String,
}

/// The most urgent open item for the viewer (§5 `primary_message`).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct PrimaryMessageDto {
    pub text: String,
    /// Set when the item is a conversation action item.
    pub action_item_id: Option<String>,
    pub thread_id: Option<String>,
    /// Set when the item is a pending review invitation (expert viewers).
    pub review_id: Option<String>,
    pub created_at: String,
}

/// Per-tab counters for the project workspace navigation.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct WorkspaceCountsDto {
    pub documents: i64,
    pub team: i64,
    pub sites: i64,
    pub threads: i64,
    pub open_action_items: i64,
    pub reviews: i64,
    pub decisions: i64,
    pub change_requests: i64,
    pub trips: i64,
    pub invoices: i64,
    pub deliverables: i64,
    pub samples: i64,
    pub revisions: i64,
}

/// Slice A's section of the `GET /projects/{id}` workspace: the bound
/// template schema, team, sites and per-tab counts
/// (built by `routes::projects::workspace_section`).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct ApplicationSectionDto {
    /// Schema of the template version this project is bound to.
    #[ts(type = "Record<string, unknown>")]
    pub template_schema: Value,
    pub template_version_number: i64,
    pub template_status: String,
    /// True when a newer published template version exists (draft banner).
    pub template_outdated: bool,
    pub latest_template_version_id: Option<String>,
    pub team: Vec<TeamMemberDto>,
    pub sites: Vec<SiteDto>,
    pub counts: WorkspaceCountsDto,
}

/// `GET /projects/{id}` — the workspace payload: the project itself, the
/// viewer's most urgent item, and one top-level field per slice section.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct ProjectWorkspaceDto {
    pub project: crate::dto::ProjectDto,
    pub primary_message: Option<PrimaryMessageDto>,
    pub application: ApplicationSectionDto,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct RevisionDto {
    pub id: String,
    pub number: i64,
    pub template_version_id: String,
    pub template_version: i64,
    /// Self-contained immutable snapshot; personal documents are redacted for
    /// viewers without personal-document rights.
    #[ts(type = "Record<string, unknown>")]
    pub snapshot: Value,
    /// Schema of the revision's own template version (revisions always render
    /// with the schema they were submitted under).
    #[ts(type = "Record<string, unknown>")]
    pub template_schema: Value,
    pub submitted_by: String,
    pub submitted_by_name: String,
    pub submitted_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct DiffEntryDto {
    /// e.g. "title", "answers.aims", "sites.<id>", "documents.<id>",
    /// "team.<user_id>".
    pub path: String,
    /// "added" | "removed" | "changed"
    pub kind: String,
    #[ts(type = "unknown | null")]
    pub before: Option<Value>,
    #[ts(type = "unknown | null")]
    pub after: Option<Value>,
    /// True when the entry is redacted for this viewer (personal documents).
    pub redacted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct RevisionDiffDto {
    pub revision: i64,
    pub against: i64,
    pub changes: Vec<DiffEntryDto>,
}

// ---------------------------------------------------------------------------
// Team / invitations
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct InvitationDto {
    pub id: String,
    pub email: String,
    pub role: String,
    pub invited_by: String,
    pub invited_by_name: String,
    /// Accept link (contains the token). Only returned once, in the
    /// `POST /projects/{id}/invitations` response (tokens are stored hashed).
    pub accept_url: Option<String>,
    pub expires_at: String,
    pub accepted_at: Option<String>,
    pub revoked_at: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct MembersResponse {
    pub members: Vec<TeamMemberDto>,
    /// Pending invitations — only populated for team lead and staff.
    pub invitations: Vec<InvitationDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct InviteRequest {
    pub email: String,
    /// "editor" | "viewer" (lead is assigned via make-lead)
    pub role: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct MemberRoleRequest {
    /// "editor" | "viewer" — lead transfers via make-lead only.
    pub role: String,
}

// ---------------------------------------------------------------------------
// Sites
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct SiteRequest {
    pub name: String,
    /// GeoJSON: {"type":"Point","coordinates":[lng,lat]} or Polygon.
    #[ts(type = "Record<string, unknown>")]
    pub geometry: Value,
    pub sensitive: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct PatchSiteRequest {
    pub name: Option<String>,
    #[ts(optional, type = "Record<string, unknown>")]
    pub geometry: Option<Value>,
    pub sensitive: Option<bool>,
}

/// `GET /sites/search` item — site plus the project it belongs to.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct SiteSearchItemDto {
    pub site: SiteDto,
    pub project_reference: Option<String>,
    pub project_title: String,
    pub project_status: String,
}

// ---------------------------------------------------------------------------
// Conversation
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct MessageDto {
    pub id: String,
    pub author_id: String,
    pub author_name: String,
    pub body: String,
    pub created_at: String,
    pub edited_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct ActionItemDto {
    pub id: String,
    pub thread_id: String,
    pub addressed_to: String,
    pub title: String,
    pub status: String,
    pub created_by: String,
    pub created_by_name: String,
    pub resolved_by: Option<String>,
    pub resolved_by_name: Option<String>,
    pub resolved_at: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct ThreadDto {
    pub id: String,
    pub project_id: String,
    /// project | field | document | deliverable | decision | change_request
    pub anchor_type: String,
    pub anchor_key: String,
    /// shared | internal (internal never visible to the team)
    pub visibility: String,
    pub created_at: String,
    pub messages: Vec<MessageDto>,
    pub action_items: Vec<ActionItemDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct ActionItemInput {
    /// "team" | "staff"
    pub addressed_to: String,
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct CreateThreadRequest {
    pub anchor_type: String,
    pub anchor_key: Option<String>,
    pub visibility: String,
    /// First message body.
    pub body: String,
    pub action_item: Option<ActionItemInput>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct PostMessageRequest {
    pub body: String,
}

// ---------------------------------------------------------------------------
// Reviews
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct CreateReviewRequest {
    pub expert_id: String,
    pub due_date: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct ReviewAssignmentDto {
    pub id: String,
    pub project_id: String,
    pub project_title: String,
    pub project_reference: Option<String>,
    /// The revision this review is bound to.
    pub project_revision_id: String,
    pub revision_number: i64,
    pub expert_id: String,
    pub expert_name: String,
    pub assigned_by: String,
    pub assigned_by_name: String,
    pub due_date: Option<String>,
    /// invited | accepted | declined | submitted
    pub status: String,
    pub decline_reason: Option<String>,
    /// Internal opinion — never exposed to the team.
    pub opinion: Option<String>,
    /// approve | approve_with_conditions | reject | need_more_info
    pub recommendation: Option<String>,
    pub submitted_at: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct DeclineRequest {
    /// Reason (e.g. conflict of interest) — required.
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct SubmitOpinionRequest {
    pub opinion: String,
    /// approve | approve_with_conditions | reject | need_more_info
    pub recommendation: String,
}

// ---------------------------------------------------------------------------
// Decisions
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct DecisionDto {
    pub id: String,
    pub project_id: String,
    pub project_revision_id: String,
    pub revision_number: i64,
    /// permit | refusal | amendment | extension | revocation
    pub kind: String,
    /// draft | issued (issued is immutable)
    pub status: String,
    pub basis: String,
    pub legal_reference: String,
    pub valid_from: Option<String>,
    pub valid_to: Option<String>,
    pub permitted_activities: Vec<String>,
    pub conditions: Vec<String>,
    pub restrictions: Vec<String>,
    /// Site geometries copied at issue time.
    #[ts(type = "unknown[]")]
    pub sites: Vec<Value>,
    /// Uploaded signed copy (document version of category `decision`).
    pub document_version_id: Option<String>,
    pub supersedes_id: Option<String>,
    pub superseded_by_id: Option<String>,
    pub change_request_id: Option<String>,
    pub drafted_by: String,
    pub drafted_by_name: String,
    pub issued_by: Option<String>,
    pub issued_by_name: Option<String>,
    pub issued_at: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct CreateDecisionRequest {
    /// permit | refusal | amendment | extension | revocation
    pub kind: String,
    /// The revision being decided on — required.
    pub project_revision_id: String,
    pub basis: Option<String>,
    pub legal_reference: Option<String>,
    pub valid_from: Option<String>,
    pub valid_to: Option<String>,
    pub permitted_activities: Option<Vec<String>>,
    pub conditions: Option<Vec<String>>,
    pub restrictions: Option<Vec<String>>,
    /// Issued decision this one supersedes (required for amendment/extension/
    /// revocation, and for a permit when a current one exists).
    pub supersedes_id: Option<String>,
    pub change_request_id: Option<String>,
    /// Signed copy: document version of category `decision` in this project.
    pub document_version_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct PatchDecisionRequest {
    pub kind: Option<String>,
    pub project_revision_id: Option<String>,
    pub basis: Option<String>,
    pub legal_reference: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    #[ts(optional = nullable)]
    pub valid_from: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    #[ts(optional = nullable)]
    pub valid_to: Option<Option<String>>,
    pub permitted_activities: Option<Vec<String>>,
    pub conditions: Option<Vec<String>>,
    pub restrictions: Option<Vec<String>>,
    #[serde(default, deserialize_with = "double_option")]
    #[ts(optional = nullable)]
    pub supersedes_id: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    #[ts(optional = nullable)]
    pub change_request_id: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    #[ts(optional = nullable)]
    pub document_version_id: Option<Option<String>>,
}

// ---------------------------------------------------------------------------
// Change requests
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct CreateChangeRequestRequest {
    /// reschedule_trip | extend_permit | expand_scope | other
    pub kind: String,
    pub description: String,
    /// For reschedule_trip: {trip_id, new_arrive_date, new_depart_date}.
    /// For extend_permit: {new_valid_to}.
    #[ts(type = "Record<string, unknown>")]
    pub payload: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct ChangeRequestDto {
    pub id: String,
    pub project_id: String,
    pub kind: String,
    pub description: String,
    #[ts(type = "Record<string, unknown>")]
    pub payload: Value,
    /// open | approved | rejected | withdrawn
    pub status: String,
    pub requested_by: String,
    pub requested_by_name: String,
    pub resolved_by: Option<String>,
    pub resolved_by_name: Option<String>,
    pub resolution_note: Option<String>,
    pub resulting_decision_id: Option<String>,
    pub created_at: String,
}

/// One affected booking in `GET /change-requests/{id}/impact`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct ImpactBookingDto {
    pub booking_id: String,
    pub trip_id: String,
    pub resource_id: String,
    pub resource_name: String,
    /// "trip" = belongs to the rescheduled trip (released/re-requested);
    /// "conflict" = other booking overlapping the requested new dates.
    pub kind: String,
    pub status: String,
    pub start_date: String,
    pub end_date: String,
    pub quantity: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct ImpactDeliverableDto {
    pub id: String,
    pub title: String,
    pub due_date: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct ImpactDecisionDto {
    pub id: String,
    pub kind: String,
    pub status: String,
    pub valid_from: Option<String>,
    pub valid_to: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct ChangeRequestImpactDto {
    pub bookings: Vec<ImpactBookingDto>,
    /// Deliverables whose due date falls before the new trip end.
    pub deliverables: Vec<ImpactDeliverableDto>,
    /// Issued decisions whose validity does not cover the new dates.
    pub decisions: Vec<ImpactDecisionDto>,
    /// True when acting on this change requires a new decision (expand_scope,
    /// extend_permit).
    pub requires_new_decision: bool,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct ResolveChangeRequestRequest {
    /// Decision implementing the change (extend_permit/expand_scope).
    pub resulting_decision_id: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/api/generated/")]
pub struct RejectChangeRequestRequest {
    pub note: Option<String>,
}

// ---------------------------------------------------------------------------
// Export
// ---------------------------------------------------------------------------

export_all!(
    TemplateDto,
    TemplateVersionDto,
    TemplateSchemaRequest,
    PatchProjectRequest,
    PatchProjectResponse,
    SubmitResponse,
    WithdrawRequest,
    UpgradeTemplateResponse,
    TeamMemberDto,
    SiteDto,
    PrimaryMessageDto,
    WorkspaceCountsDto,
    ApplicationSectionDto,
    ProjectWorkspaceDto,
    RevisionDto,
    DiffEntryDto,
    RevisionDiffDto,
    InvitationDto,
    MembersResponse,
    InviteRequest,
    MemberRoleRequest,
    SiteRequest,
    PatchSiteRequest,
    SiteSearchItemDto,
    MessageDto,
    ActionItemDto,
    ThreadDto,
    ActionItemInput,
    CreateThreadRequest,
    PostMessageRequest,
    CreateReviewRequest,
    ReviewAssignmentDto,
    DeclineRequest,
    SubmitOpinionRequest,
    DecisionDto,
    CreateDecisionRequest,
    PatchDecisionRequest,
    CreateChangeRequestRequest,
    ChangeRequestDto,
    ImpactBookingDto,
    ImpactDeliverableDto,
    ImpactDecisionDto,
    ChangeRequestImpactDto,
    ResolveChangeRequestRequest,
    RejectChangeRequestRequest,
);
