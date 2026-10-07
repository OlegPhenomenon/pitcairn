-- 0001_init.sql — COMPLETE base schema for the Pitcairn Research Permits & Data Hub.
--
-- This file is owned by slice B1 (backend foundation) and intentionally creates
-- EVERY table of docs/architecture.md §4, including tables used only by later
-- slices (decisions, bookings, invoices, deliverables, measurements, ...).
-- Later parallel slices may ONLY add new migration files in their own number
-- range (A: 0100–0199, B: 0200–0299, C: 0300–0399, D: 0400–0499) and should
-- not need to alter anything here.

-- ============ Identity ============

CREATE TABLE users (
    id TEXT PRIMARY KEY,
    email TEXT NOT NULL UNIQUE COLLATE NOCASE,
    name TEXT NOT NULL,
    organisation TEXT NOT NULL DEFAULT '',
    password_hash TEXT NOT NULL,
    totp_secret TEXT,
    totp_enabled_at TEXT,
    disabled_at TEXT,
    created_at TEXT NOT NULL
);

CREATE TABLE user_roles (
    user_id TEXT NOT NULL REFERENCES users(id),
    role TEXT NOT NULL CHECK (role IN ('coordinator','expert','decision_maker','base_manager','finance','admin','provider')),
    granted_by TEXT REFERENCES users(id),
    granted_at TEXT NOT NULL,
    revoked_at TEXT,
    PRIMARY KEY (user_id, role, granted_at)
);
CREATE INDEX idx_user_roles_user ON user_roles(user_id, revoked_at);

CREATE TABLE sessions (
    id TEXT PRIMARY KEY, -- sha256(token)
    user_id TEXT NOT NULL REFERENCES users(id),
    created_at TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    mfa_verified INTEGER NOT NULL DEFAULT 0,
    via_demo_switch INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX idx_sessions_user ON sessions(user_id);

CREATE TABLE invitations (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id),
    email TEXT NOT NULL COLLATE NOCASE,
    role TEXT NOT NULL CHECK (role IN ('lead','editor','viewer')),
    token_hash TEXT NOT NULL UNIQUE,
    invited_by TEXT NOT NULL REFERENCES users(id),
    expires_at TEXT NOT NULL,
    accepted_at TEXT,
    revoked_at TEXT,
    created_at TEXT NOT NULL
);
CREATE INDEX idx_invitations_project ON invitations(project_id);

-- ============ Templates ============

CREATE TABLE templates (
    id TEXT PRIMARY KEY,
    key TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL
);

CREATE TABLE template_versions (
    id TEXT PRIMARY KEY,
    template_id TEXT NOT NULL REFERENCES templates(id),
    version INTEGER NOT NULL,
    schema_json TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('draft','published','retired')),
    published_at TEXT,
    published_by TEXT REFERENCES users(id),
    created_at TEXT NOT NULL,
    UNIQUE (template_id, version)
);
CREATE INDEX idx_template_versions_template ON template_versions(template_id, status);

-- ============ Projects ============

CREATE TABLE projects (
    id TEXT PRIMARY KEY,
    reference TEXT UNIQUE, -- PIT-YYYY-NNNN, assigned at first submit
    title TEXT NOT NULL DEFAULT '',
    summary TEXT NOT NULL DEFAULT '',
    keywords TEXT NOT NULL DEFAULT '',
    organisation TEXT NOT NULL DEFAULT '',
    template_version_id TEXT NOT NULL REFERENCES template_versions(id),
    answers_json TEXT NOT NULL DEFAULT '{}',
    status TEXT NOT NULL CHECK (status IN ('draft','submitted','in_review','changes_requested','approved','refused','closed','withdrawn')),
    start_date TEXT,
    end_date TEXT,
    legacy INTEGER NOT NULL DEFAULT 0,
    closed_reason TEXT,
    version INTEGER NOT NULL DEFAULT 1, -- optimistic concurrency
    created_by TEXT NOT NULL REFERENCES users(id),
    created_at TEXT NOT NULL
);
CREATE INDEX idx_projects_status ON projects(status);
CREATE INDEX idx_projects_created_by ON projects(created_by);

CREATE TABLE project_members (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id),
    user_id TEXT NOT NULL REFERENCES users(id),
    role TEXT NOT NULL CHECK (role IN ('lead','editor','viewer')),
    added_by TEXT REFERENCES users(id),
    added_at TEXT NOT NULL,
    removed_at TEXT,
    removed_by TEXT REFERENCES users(id)
);
CREATE INDEX idx_project_members_project ON project_members(project_id, removed_at);
CREATE INDEX idx_project_members_user ON project_members(user_id, removed_at);

CREATE TABLE project_revisions (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id),
    number INTEGER NOT NULL,
    template_version_id TEXT NOT NULL REFERENCES template_versions(id),
    snapshot_json TEXT NOT NULL,
    submitted_by TEXT NOT NULL REFERENCES users(id),
    submitted_at TEXT NOT NULL,
    created_at TEXT NOT NULL,
    UNIQUE (project_id, number)
);

CREATE TABLE project_sites (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id),
    name TEXT NOT NULL,
    geometry_json TEXT NOT NULL, -- GeoJSON Point|Polygon
    min_lat REAL NOT NULL,
    min_lng REAL NOT NULL,
    max_lat REAL NOT NULL,
    max_lng REAL NOT NULL,
    sensitive INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL
);
CREATE INDEX idx_project_sites_project ON project_sites(project_id);
CREATE INDEX idx_project_sites_bbox ON project_sites(min_lat, min_lng, max_lat, max_lng);

-- ============ Documents and files ============

CREATE TABLE files (
    id TEXT PRIMARY KEY,
    sha256 TEXT NOT NULL UNIQUE,
    size INTEGER NOT NULL,
    mime TEXT NOT NULL,
    storage_key TEXT NOT NULL,
    scan_status TEXT NOT NULL CHECK (scan_status IN ('pending','clean','rejected')),
    scan_detail TEXT,
    uploaded_by TEXT NOT NULL REFERENCES users(id),
    created_at TEXT NOT NULL
);
CREATE INDEX idx_files_sha ON files(sha256);

CREATE TABLE upload_sessions (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id),
    filename TEXT NOT NULL,
    size INTEGER NOT NULL,
    sha256 TEXT NOT NULL,
    mime TEXT NOT NULL,
    chunk_size INTEGER NOT NULL,
    chunks_total INTEGER NOT NULL,
    chunks_received_json TEXT NOT NULL DEFAULT '[]',
    status TEXT NOT NULL CHECK (status IN ('open','finalizing','complete','aborted')),
    file_id TEXT REFERENCES files(id),
    expires_at TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE INDEX idx_upload_sessions_user ON upload_sessions(user_id);

CREATE TABLE documents (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id),
    slot_key TEXT,
    title TEXT NOT NULL,
    category TEXT NOT NULL CHECK (category IN ('application','personal','decision','result','other')),
    created_by TEXT NOT NULL REFERENCES users(id),
    created_at TEXT NOT NULL
);
CREATE INDEX idx_documents_project ON documents(project_id);

CREATE TABLE document_versions (
    id TEXT PRIMARY KEY,
    document_id TEXT NOT NULL REFERENCES documents(id),
    number INTEGER NOT NULL,
    file_id TEXT NOT NULL REFERENCES files(id),
    note TEXT NOT NULL DEFAULT '',
    uploaded_by TEXT NOT NULL REFERENCES users(id),
    uploaded_at TEXT NOT NULL,
    created_at TEXT NOT NULL,
    UNIQUE (document_id, number)
);

-- ============ Conversation ============

CREATE TABLE threads (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id),
    anchor_type TEXT NOT NULL CHECK (anchor_type IN ('project','field','document','deliverable','decision','change_request')),
    anchor_key TEXT NOT NULL DEFAULT '',
    visibility TEXT NOT NULL CHECK (visibility IN ('shared','internal')),
    created_at TEXT NOT NULL
);
CREATE INDEX idx_threads_project ON threads(project_id);

CREATE TABLE messages (
    id TEXT PRIMARY KEY,
    thread_id TEXT NOT NULL REFERENCES threads(id),
    author_id TEXT NOT NULL REFERENCES users(id),
    body TEXT NOT NULL,
    created_at TEXT NOT NULL,
    edited_at TEXT
);
CREATE INDEX idx_messages_thread ON messages(thread_id);

CREATE TABLE action_items (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id),
    thread_id TEXT NOT NULL REFERENCES threads(id),
    addressed_to TEXT NOT NULL CHECK (addressed_to IN ('team','staff')),
    title TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('open','resolved','cancelled')),
    created_by TEXT NOT NULL REFERENCES users(id),
    resolved_by TEXT REFERENCES users(id),
    resolved_at TEXT,
    created_at TEXT NOT NULL
);
CREATE INDEX idx_action_items_project ON action_items(project_id, status);

-- ============ Review and decisions ============

CREATE TABLE review_assignments (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id),
    project_revision_id TEXT NOT NULL REFERENCES project_revisions(id),
    expert_id TEXT NOT NULL REFERENCES users(id),
    assigned_by TEXT NOT NULL REFERENCES users(id),
    due_date TEXT,
    status TEXT NOT NULL CHECK (status IN ('invited','accepted','declined','submitted')),
    decline_reason TEXT,
    opinion TEXT,
    recommendation TEXT CHECK (recommendation IN ('approve','approve_with_conditions','reject','need_more_info')),
    submitted_at TEXT,
    created_at TEXT NOT NULL
);
CREATE INDEX idx_review_assignments_project ON review_assignments(project_id);
CREATE INDEX idx_review_assignments_expert ON review_assignments(expert_id, status);

CREATE TABLE decisions (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id),
    project_revision_id TEXT NOT NULL REFERENCES project_revisions(id),
    kind TEXT NOT NULL CHECK (kind IN ('permit','refusal','amendment','extension','revocation')),
    status TEXT NOT NULL CHECK (status IN ('draft','issued')),
    basis TEXT NOT NULL DEFAULT '',
    legal_reference TEXT NOT NULL DEFAULT '',
    valid_from TEXT,
    valid_to TEXT,
    permitted_activities_json TEXT NOT NULL DEFAULT '[]',
    conditions_json TEXT NOT NULL DEFAULT '[]',
    restrictions_json TEXT NOT NULL DEFAULT '[]',
    sites_snapshot_json TEXT NOT NULL DEFAULT '[]',
    document_version_id TEXT REFERENCES document_versions(id),
    supersedes_id TEXT REFERENCES decisions(id),
    superseded_by_id TEXT REFERENCES decisions(id),
    change_request_id TEXT REFERENCES change_requests(id),
    drafted_by TEXT NOT NULL REFERENCES users(id),
    issued_by TEXT REFERENCES users(id),
    issued_at TEXT,
    created_at TEXT NOT NULL
);
CREATE INDEX idx_decisions_project ON decisions(project_id);

CREATE TABLE change_requests (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id),
    kind TEXT NOT NULL CHECK (kind IN ('reschedule_trip','extend_permit','expand_scope','other')),
    description TEXT NOT NULL DEFAULT '',
    payload_json TEXT NOT NULL DEFAULT '{}',
    status TEXT NOT NULL CHECK (status IN ('open','approved','rejected','withdrawn')),
    requested_by TEXT NOT NULL REFERENCES users(id),
    resolved_by TEXT REFERENCES users(id),
    resolution_note TEXT,
    resulting_decision_id TEXT REFERENCES decisions(id),
    created_at TEXT NOT NULL
);
CREATE INDEX idx_change_requests_project ON change_requests(project_id);

-- ============ Trips, resources, bookings ============

CREATE TABLE resources (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL CHECK (kind IN ('room','lab','equipment','boat','service')),
    name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    quantity INTEGER NOT NULL CHECK (quantity >= 1),
    unit_label TEXT NOT NULL DEFAULT '',
    provider_user_id TEXT REFERENCES users(id),
    active INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL
);

CREATE TABLE tariffs (
    id TEXT PRIMARY KEY,
    resource_id TEXT NOT NULL REFERENCES resources(id),
    unit TEXT NOT NULL CHECK (unit IN ('per_night','per_day','per_item','per_hour')),
    price_cents INTEGER NOT NULL CHECK (price_cents >= 0),
    currency TEXT NOT NULL DEFAULT 'NZD',
    effective_from TEXT NOT NULL,
    created_by TEXT NOT NULL REFERENCES users(id),
    created_at TEXT NOT NULL
);
CREATE INDEX idx_tariffs_resource ON tariffs(resource_id, effective_from);

CREATE TABLE trips (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id),
    title TEXT NOT NULL,
    arrive_date TEXT NOT NULL,
    depart_date TEXT NOT NULL,
    participants_json TEXT NOT NULL DEFAULT '[]',
    status TEXT NOT NULL CHECK (status IN ('planned','confirmed','completed','cancelled')),
    created_at TEXT NOT NULL
);
CREATE INDEX idx_trips_project ON trips(project_id);

CREATE TABLE bookings (
    id TEXT PRIMARY KEY,
    trip_id TEXT NOT NULL REFERENCES trips(id),
    resource_id TEXT NOT NULL REFERENCES resources(id),
    start_date TEXT NOT NULL,
    end_date TEXT NOT NULL,
    quantity INTEGER NOT NULL CHECK (quantity >= 1),
    status TEXT NOT NULL CHECK (status IN ('requested','confirmed','declined','cancelled','released')),
    requested_by TEXT NOT NULL REFERENCES users(id),
    decided_by TEXT REFERENCES users(id),
    decided_at TEXT,
    decline_reason TEXT,
    created_at TEXT NOT NULL
);
CREATE INDEX idx_bookings_trip ON bookings(trip_id);
CREATE INDEX idx_bookings_resource ON bookings(resource_id, status, start_date, end_date);

-- ============ Money ============

CREATE TABLE invoices (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id),
    number TEXT UNIQUE, -- INV-YYYY-NNNN
    status TEXT NOT NULL CHECK (status IN ('draft','issued','cancelled')),
    currency TEXT NOT NULL DEFAULT 'NZD',
    issued_at TEXT,
    due_date TEXT,
    created_by TEXT NOT NULL REFERENCES users(id),
    cancelled_reason TEXT,
    created_at TEXT NOT NULL
);
CREATE INDEX idx_invoices_project ON invoices(project_id);

CREATE TABLE invoice_lines (
    id TEXT PRIMARY KEY,
    invoice_id TEXT NOT NULL REFERENCES invoices(id),
    booking_id TEXT REFERENCES bookings(id),
    description TEXT NOT NULL,
    quantity REAL NOT NULL,
    unit TEXT NOT NULL,
    unit_price_cents INTEGER NOT NULL,
    amount_cents INTEGER NOT NULL,
    created_at TEXT NOT NULL
);
CREATE INDEX idx_invoice_lines_invoice ON invoice_lines(invoice_id);

CREATE TABLE payments (
    id TEXT PRIMARY KEY,
    invoice_id TEXT NOT NULL REFERENCES invoices(id),
    kind TEXT NOT NULL CHECK (kind IN ('payment','refund')),
    amount_cents INTEGER NOT NULL CHECK (amount_cents > 0),
    currency TEXT NOT NULL,
    method TEXT NOT NULL CHECK (method IN ('bank_transfer','test_card','manual')),
    external_ref TEXT UNIQUE,
    request_hash TEXT,
    status TEXT NOT NULL CHECK (status IN ('pending_verification','verified','rejected')),
    received_at TEXT NOT NULL,
    verified_by TEXT REFERENCES users(id),
    note TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL
);
CREATE INDEX idx_payments_invoice ON payments(invoice_id, status);

-- ============ Deliverables ============

CREATE TABLE deliverables (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id),
    title TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    kind TEXT NOT NULL CHECK (kind IN ('report','dataset','media','samples','other')),
    due_date TEXT NOT NULL,
    sender_id TEXT NOT NULL REFERENCES users(id),
    recipient_id TEXT NOT NULL REFERENCES users(id),
    status TEXT NOT NULL CHECK (status IN ('proposed','agreed','submitted','changes_requested','accepted','waived','cancelled')),
    terms_version INTEGER NOT NULL DEFAULT 1,
    team_agreed_at TEXT,
    team_agreed_by TEXT REFERENCES users(id),
    staff_agreed_at TEXT,
    staff_agreed_by TEXT REFERENCES users(id),
    resolution_note TEXT,
    publish_level TEXT NOT NULL DEFAULT 'none' CHECK (publish_level IN ('none','metadata','metadata_and_files')),
    embargo_until TEXT,
    published_at TEXT,
    published_by TEXT REFERENCES users(id),
    created_by TEXT NOT NULL REFERENCES users(id),
    created_at TEXT NOT NULL
);
CREATE INDEX idx_deliverables_project ON deliverables(project_id);

CREATE TABLE deliverable_due_changes (
    id TEXT PRIMARY KEY,
    deliverable_id TEXT NOT NULL REFERENCES deliverables(id),
    old_due TEXT NOT NULL,
    new_due TEXT NOT NULL,
    reason TEXT NOT NULL,
    changed_by TEXT NOT NULL REFERENCES users(id),
    changed_at TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE TABLE deliverable_submissions (
    id TEXT PRIMARY KEY,
    deliverable_id TEXT NOT NULL REFERENCES deliverables(id),
    number INTEGER NOT NULL,
    submitted_by TEXT NOT NULL REFERENCES users(id),
    note TEXT NOT NULL DEFAULT '',
    data_dictionary_json TEXT NOT NULL DEFAULT '[]',
    status TEXT NOT NULL CHECK (status IN ('received','changes_requested','accepted')),
    reviewed_by TEXT REFERENCES users(id),
    review_note TEXT,
    reviewed_at TEXT,
    created_at TEXT NOT NULL,
    UNIQUE (deliverable_id, number)
);

CREATE TABLE submission_files (
    id TEXT PRIMARY KEY,
    submission_id TEXT NOT NULL REFERENCES deliverable_submissions(id),
    document_version_id TEXT NOT NULL REFERENCES document_versions(id),
    created_at TEXT NOT NULL,
    UNIQUE (submission_id, document_version_id)
);

CREATE TABLE external_links (
    id TEXT PRIMARY KEY,
    submission_id TEXT NOT NULL REFERENCES deliverable_submissions(id),
    url TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    version_label TEXT NOT NULL DEFAULT '',
    access_notes TEXT NOT NULL DEFAULT '',
    last_checked_at TEXT,
    last_status TEXT,
    available INTEGER,
    created_at TEXT NOT NULL
);

CREATE TABLE publication_files (
    id TEXT PRIMARY KEY,
    deliverable_id TEXT NOT NULL REFERENCES deliverables(id),
    document_version_id TEXT NOT NULL REFERENCES document_versions(id),
    approved_by TEXT NOT NULL REFERENCES users(id),
    approved_at TEXT NOT NULL,
    created_at TEXT NOT NULL,
    UNIQUE (deliverable_id, document_version_id)
);

-- ============ Samples & measurements ============

CREATE TABLE samples (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id),
    code TEXT NOT NULL,
    site_id TEXT REFERENCES project_sites(id),
    collected_on TEXT,
    material TEXT NOT NULL DEFAULT '',
    custodian_org TEXT NOT NULL DEFAULT '',
    storage_location TEXT NOT NULL DEFAULT '',
    notes TEXT NOT NULL DEFAULT '',
    related_deliverable_ids_json TEXT NOT NULL DEFAULT '[]',
    created_at TEXT NOT NULL
);
CREATE INDEX idx_samples_project ON samples(project_id);

CREATE TABLE measurements (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id),
    deliverable_id TEXT NOT NULL REFERENCES deliverables(id),
    submission_id TEXT NOT NULL REFERENCES deliverable_submissions(id),
    site_name TEXT NOT NULL,
    observed_on TEXT NOT NULL,
    variable_key TEXT NOT NULL,
    value REAL NOT NULL,
    unit TEXT NOT NULL,
    source_label TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL
);
CREATE INDEX idx_measurements_project ON measurements(project_id);
CREATE INDEX idx_measurements_variable ON measurements(variable_key);

-- ============ Platform ============

CREATE TABLE notifications (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id),
    project_id TEXT REFERENCES projects(id),
    kind TEXT NOT NULL,
    title TEXT NOT NULL,
    body TEXT NOT NULL DEFAULT '',
    link TEXT NOT NULL DEFAULT '',
    read_at TEXT,
    created_at TEXT NOT NULL
);
CREATE INDEX idx_notifications_user ON notifications(user_id, read_at);

CREATE TABLE jobs (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL,
    payload_json TEXT NOT NULL DEFAULT '{}',
    dedupe_key TEXT UNIQUE,
    status TEXT NOT NULL CHECK (status IN ('queued','running','done','failed','dead')),
    attempts INTEGER NOT NULL DEFAULT 0,
    max_attempts INTEGER NOT NULL DEFAULT 6,
    run_after TEXT NOT NULL,
    last_error TEXT,
    locked_until TEXT,
    created_at TEXT NOT NULL
);
CREATE INDEX idx_jobs_poll ON jobs(status, run_after);

CREATE TABLE mail_messages (
    id TEXT PRIMARY KEY,
    to_email TEXT NOT NULL,
    subject TEXT NOT NULL,
    body_text TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('queued','sent','failed')),
    error TEXT,
    created_at TEXT NOT NULL,
    sent_at TEXT
);

CREATE TABLE settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE audit_events (
    id TEXT PRIMARY KEY,
    at TEXT NOT NULL,
    actor_id TEXT REFERENCES users(id),
    actor_label TEXT NOT NULL DEFAULT '',
    action TEXT NOT NULL,
    entity_type TEXT NOT NULL,
    entity_id TEXT NOT NULL,
    project_id TEXT REFERENCES projects(id),
    visibility TEXT NOT NULL CHECK (visibility IN ('shared','internal')),
    summary TEXT NOT NULL DEFAULT '',
    before_json TEXT,
    after_json TEXT,
    reason TEXT,
    created_at TEXT NOT NULL
);
CREATE INDEX idx_audit_project ON audit_events(project_id, at);
CREATE INDEX idx_audit_entity ON audit_events(entity_type, entity_id);

CREATE TABLE idempotency_keys (
    user_id TEXT NOT NULL REFERENCES users(id),
    route TEXT NOT NULL,
    key TEXT NOT NULL,
    request_hash TEXT NOT NULL,
    response_status INTEGER NOT NULL,
    response_json TEXT NOT NULL,
    created_at TEXT NOT NULL,
    PRIMARY KEY (user_id, route, key)
);

-- Per-prefix/year counters for reference numbers (PIT-YYYY-NNNN, INV-YYYY-NNNN).
CREATE TABLE reference_counters (
    prefix TEXT NOT NULL,
    year INTEGER NOT NULL,
    next INTEGER NOT NULL,
    PRIMARY KEY (prefix, year)
);

CREATE TABLE import_batches (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL CHECK (kind IN ('legacy_csv','project_archive')),
    status TEXT NOT NULL CHECK (status IN ('previewed','committed','discarded')),
    preview_json TEXT NOT NULL DEFAULT '{}',
    created_by TEXT NOT NULL REFERENCES users(id),
    created_at TEXT NOT NULL
);

-- ============ Full-text search ============

CREATE VIRTUAL TABLE projects_fts USING fts5(
    title, summary, keywords, organisation, reference,
    content='projects', content_rowid='rowid'
);

CREATE TRIGGER projects_fts_insert AFTER INSERT ON projects BEGIN
    INSERT INTO projects_fts(rowid, title, summary, keywords, organisation, reference)
    VALUES (new.rowid, new.title, new.summary, new.keywords, new.organisation, coalesce(new.reference, ''));
END;

CREATE TRIGGER projects_fts_delete AFTER DELETE ON projects BEGIN
    INSERT INTO projects_fts(projects_fts, rowid, title, summary, keywords, organisation, reference)
    VALUES ('delete', old.rowid, old.title, old.summary, old.keywords, old.organisation, coalesce(old.reference, ''));
END;

CREATE TRIGGER projects_fts_update AFTER UPDATE ON projects BEGIN
    INSERT INTO projects_fts(projects_fts, rowid, title, summary, keywords, organisation, reference)
    VALUES ('delete', old.rowid, old.title, old.summary, old.keywords, old.organisation, coalesce(old.reference, ''));
    INSERT INTO projects_fts(rowid, title, summary, keywords, organisation, reference)
    VALUES (new.rowid, new.title, new.summary, new.keywords, new.organisation, coalesce(new.reference, ''));
END;
