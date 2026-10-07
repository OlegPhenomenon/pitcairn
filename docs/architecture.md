# Pitcairn Research Permits & Data Hub — Architecture

Status: binding contract for all implementation work. If code and this document disagree, fix the code or update this document in the same change.

## 1. Product in one paragraph

A foreign researcher (persona **Anna**) applies to work at the Pitcairn Islands Marine Science Base. Pitcairn staff (persona **Maria**, the case coordinator) screen the application, request missing items, bring in a scientific **expert**, and a **decision maker** issues a permit or refusal with explicit conditions. The **base manager** confirms rooms/lab/equipment, an external **provider** (boat owner) confirms their own service, **finance** invoices and records payments. Before fieldwork Anna and Maria agree on concrete **deliverables** (what, due when, who receives). After the trip Anna submits results; Maria checks receipt (not scientific truth), may return items for correction, and decides what is published to the open **catalog**. Years later any staff member finds the project by area/organisation/year/topic and gets the results.

UI language: English. Demo data: fictional people, fictional amounts. All actions are real (persisted, enforced) except external integrations, which are mocked behind interfaces: mail, bank, card payments, link checker, antivirus, AI assistant.

## 2. Stack and layout

```
backend/     Rust 2024, axum 0.8, sqlx 0.8 (sqlite), tokio — single binary `pitcairn`
frontend/    React 19 + TypeScript + Vite, react-router 7, TanStack Query, Tailwind CSS 4,
             react-leaflet (OSM tiles), recharts, hash-wasm (incremental sha256)
e2e/         Playwright end-to-end tests (story scenarios from section 12)
docs/        architecture (this file), research, operations runbook
config/      deploy.yml (Kamal)
Dockerfile   multi-stage: node build → rust build → debian-slim runtime
```

- The binary serves `/api/v1/*` and the built SPA (`frontend/dist`, path from `PITCAIRN_STATIC_DIR`) with index.html fallback. `/up` is the health check.
- TypeScript types for every API DTO are generated from Rust with `ts-rs` into `frontend/src/api/generated/` (`cargo test export_bindings` or a `pitcairn export-types` subcommand). Frontend never hand-writes DTO types.
- Rust crates (preferred versions, align with Cargo.lock): axum, sqlx (`sqlite`, `migrate`, `macros`, `runtime-tokio`), tokio, tower-http (`fs`, `trace`, `compression-gzip`, `set-header`), serde/serde_json, chrono, uuid v4, argon2, totp-rs, sha2, hex, rand, thiserror, tracing, tracing-subscriber, zip, csv, ts-rs, clap, hmac. No outbound HTTP client: all integrations are mocked.
- IDs: TEXT UUID v4 everywhere (portable export/restore across installations). Timestamps: TEXT RFC 3339 UTC. Calendar dates: TEXT `YYYY-MM-DD`. Money: INTEGER cents + `currency` TEXT (default `NZD`).
- SQLite: WAL, `foreign_keys=ON`, `busy_timeout=5000`. Writes that check-then-insert (booking confirmation, payment recording, submission) run inside `BEGIN IMMEDIATE` transactions.
- Migrations: `backend/migrations/NNNN_name.sql`, applied on `serve` start and by `pitcairn migrate`.

### 2.1 Configuration (env)

| Var | Default | Meaning |
|---|---|---|
| `PITCAIRN_BIND` | `127.0.0.1:8080` | listen addr |
| `PITCAIRN_DATA_DIR` | `./data` | sqlite db `pitcairn.sqlite3`, `files/`, `uploads/`, `backups/` |
| `PITCAIRN_STATIC_DIR` | `../frontend/dist` | SPA build |
| `PITCAIRN_BASE_URL` | `http://localhost:8080` | links in mail |
| `PITCAIRN_DEMO_MODE` | `false` | enables persona switcher, demo mailbox, bank simulator, demo banner |
| `PITCAIRN_SESSION_SECRET` | random per start if unset (warn) | cookie signing |
| `PITCAIRN_BANK_WEBHOOK_SECRET` | random if unset (warn) | HMAC for bank notifications |
| `PITCAIRN_MAX_UPLOAD_BYTES` | `2147483648` | per file |
| `PITCAIRN_LINK_CHECK_MODE` | `mock` | only `mock` is implemented: URLs whose host ends in `.invalid` or whose path contains `/missing` are unavailable, everything else available |
| `PITCAIRN_AI_MODE` | `mock` | `mock` or `off` |
| `PITCAIRN_SECURE_COOKIES` | `false` | set `true` behind TLS |

No secret is required to start. No secret lives in the repo.

### 2.2 CLI

`pitcairn serve | migrate | seed-demo [--reset] | create-admin --email --name | backup --out <dir> | restore --from <dir> | export-project <ref> --out <file.zip> | import-project <file.zip> | export-types`

## 3. Roles and access

Global roles (`user_roles`, many per user):

| Role | Meaning |
|---|---|
| `coordinator` | Pitcairn case officer (Maria). Screens, requests info, assigns experts, agrees deliverables, checks results, publishes. |
| `expert` | Reviewer. Sees ONLY projects with an assignment to them; sees application + non-personal documents; writes opinions. |
| `decision_maker` | May issue permits/refusals/amendments. Only an existing `decision_maker` can grant this role (separation of duties). |
| `base_manager` | Resources, calendar, booking confirmation. |
| `finance` | Invoices, payments, refunds, bank verification. |
| `admin` | Users, roles (except `decision_maker`), templates, resources, tariffs, settings, import/export, delivery issues. Gets NO decision rights. |
| `provider` | External service owner (boat). Sees only bookings for resources where `resources.provider_user_id = me`, with minimal project info (title, dates, team size, lead name). |

`staff` = any of coordinator, decision_maker, base_manager, finance, admin. Staff and experts must complete TOTP MFA (`mfa_verified` on the session) before any non-auth endpoint works; not enrolled → forced enrollment.

Project team (`project_members`): `lead` (exactly one active), `editor` (may edit application, upload, submit, answer), `viewer` (read). Researchers see only projects where they are active members. Removal sets `removed_at`; every request re-checks membership, so old file links stop working immediately.

Document categories: `application`, `personal` (passports, insurance, CVs — team editors+ and coordinator only; never experts, never catalog, never exported to public), `decision`, `result`, `other`.

Message visibility: `shared` (team + staff + assigned experts) vs `internal` (staff + assigned experts only; never team, never catalog). Expert opinions are always internal.

All authorization is server-side in `backend/src/authz.rs` (pure functions over an `Actor` + loaded rows). Handlers must call it; tests cover every deny path in section 12.

Operation matrix (T = active team member with given minimum role, C coordinator, E assigned expert, D decision_maker, B base_manager, F finance, A admin, P provider of the resource):

| Operation | Allowed |
|---|---|
| edit draft / upload application docs / submit / answer threads | T editor+ |
| invite, change member roles, make lead, remove member, withdraw | T lead |
| read personal documents | T editor+, C |
| read application + non-personal docs + shared threads | T viewer+, C, D, E, B (B: summary only, no documents) |
| internal threads / staff notes | C, D, E (assigned), B, F (read) |
| action items, assign experts, agree deliverables (staff side), check results, publish | C |
| write opinion | E (own assignment) |
| draft decision | C, D; issue decision | D only |
| request change | T editor+; approve/reject change request | C (+ D if it needs a new decision) |
| trips: create/edit | T editor+, C; bookings confirm/decline | B (rooms/lab/equipment/service), P (own boat) |
| invoices, payments, refunds, verify | F |
| pay with test card | T editor+ |
| templates, resources, tariffs, settings, users, jobs, import, audit | A |
| export a project ZIP | C, A |
| grant `decision_maker` | D only (or CLI `grant-role`) |

An admin cannot reset the password or MFA of, disable, or edit the email of a user holding `decision_maker` (409 `protected_user`); only the CLI can. When the server starts with demo mode off, all sessions with `via_demo_switch=1` are deleted.

## 4. Domain model (tables)

All tables have `id TEXT PK` (uuid) unless stated, `created_at`. `*_by` columns reference `users.id`.

**Identity**
- `users(email UNIQUE COLLATE NOCASE, name, organisation, password_hash, totp_secret NULL, totp_enabled_at NULL, disabled_at NULL)`
- `user_roles(user_id, role, granted_by, granted_at, revoked_at NULL)` PK(user_id, role, granted_at)
- `sessions(id = sha256(token), user_id, created_at, expires_at, mfa_verified INT, via_demo_switch INT)`
- `invitations(project_id, email, role, token_hash, invited_by, expires_at, accepted_at NULL, revoked_at NULL)`

**Templates** (configurable application forms; Annex 2 is the seeded base-use template)
- `templates(key UNIQUE, name, description)` — keys: `base_use` (Annex 2), `fieldwork_permit`.
- `template_versions(template_id, version INT, schema_json, status draft|published|retired, published_at, published_by)` UNIQUE(template_id, version).
  `schema_json = {sections:[{key,title,help,fields:[{key,label,type:text|textarea|date|daterange|number|select|multiselect|people|checkbox|sites,required,help,options?}]}], required_documents:[{key,label,help,category}]}`
- A new project binds to the latest published version. A draft whose version is outdated shows a banner and `POST /projects/{id}/upgrade-template` copies answers by field key. Submit requires the current published version. Submitted revisions are immutable snapshots.

**Projects**
- `projects(reference UNIQUE NULL — assigned at first submit, format PIT-YYYY-NNNN; title, summary, keywords, organisation, template_version_id, answers_json, status, start_date, end_date, legacy INT default 0, closed_reason NULL, created_by)`
- status transitions (enforced in one function, every other transition → 409 `invalid_transition`):

  | from | action (who) | to |
  |---|---|---|
  | draft | submit (team editor+) | submitted |
  | submitted | open for screening (coordinator) | in_review |
  | submitted, in_review | create action item addressed to team (coordinator) | changes_requested |
  | changes_requested | resubmit (team editor+) — creates a new revision and resolves all open team action items that the team answered | in_review |
  | in_review | issue permit decision (decision_maker) | approved |
  | in_review | issue refusal (decision_maker) | refused |
  | approved | close (coordinator, all deliverables resolved) | closed |
  | draft, submitted, in_review, changes_requested | withdraw (team lead) | withdrawn |

  Issuing a decision requires: status `in_review`, no open team action items, and an explicit `project_revision_id` (the revision decided on). A team member answering an action item (message in its thread) does not resolve it; the coordinator resolves it, or resubmission resolves it.
- `projects.version INT` — optimistic concurrency: `PATCH /projects/{id}` must send the `version` it loaded; mismatch → 409 `stale_version`. Submitted revisions are immutable.
- `project_members(project_id, user_id, role lead|editor|viewer, added_by, added_at, removed_at NULL, removed_by NULL)`
- `project_revisions(project_id, number INT, template_version_id, snapshot_json, submitted_by, submitted_at)` UNIQUE(project_id, number) — one row per submit/resubmit. `snapshot_json` is self-contained: title, summary, keywords, organisation, dates, answers, team list, site geometries, and `[{document_id, slot_key, title, category, version_id, sha256}]`. Published template versions are immutable (their `schema_json` never changes), so a revision always renders with its own schema.
- `project_sites(project_id, name, geometry_json GeoJSON Point|Polygon, min_lat, min_lng, max_lat, max_lng, sensitive INT)` — area search = bbox intersection. For viewers without precise-location rights (anyone except active team members, staff, assigned experts) sensitive geometries are replaced by their bbox expanded to a 0.1° grid and flagged `generalized: true` — in API, map, catalog and every download/export path that a non-privileged viewer can reach.

**Documents and files**
- `files(sha256, size, mime, storage_key, scan_status pending|clean|rejected, scan_detail, uploaded_by)` — bytes at `data/files/<sha[0..2]>/<sha>`; dedup by sha256.
- `upload_sessions(user_id, filename, size, sha256, mime, chunk_size, chunks_total, chunks_received_json, status open|finalizing|complete|aborted, file_id NULL, expires_at)` — temp bytes at `data/uploads/<id>.part`.
- `documents(project_id, slot_key NULL, title, category, created_by)`; `document_versions(document_id, number INT, file_id, note, uploaded_by, uploaded_at)` — new upload = new version; old versions stay.

**Conversation**
- `threads(project_id, anchor_type project|field|document|deliverable|decision|change_request, anchor_key, visibility shared|internal)`
- `messages(thread_id, author_id, body, created_at, edited_at NULL)`
- `action_items(project_id, thread_id, addressed_to team|staff, title, status open|resolved|cancelled, created_by, resolved_by, resolved_at)` — "Please add a field safety plan" is an action item anchored to the `safety_plan` document slot.

**Review and decisions**
- `review_assignments(project_id, project_revision_id, expert_id, assigned_by, due_date, status invited|accepted|declined|submitted, decline_reason, opinion, recommendation approve|approve_with_conditions|reject|need_more_info, submitted_at)`
- `decisions(project_id, project_revision_id, kind permit|refusal|amendment|extension|revocation, status draft|issued, basis, legal_reference, valid_from, valid_to, permitted_activities_json [string], conditions_json [string], restrictions_json [string], sites_snapshot_json (copied geometries at issue time), document_version_id NULL (uploaded signed copy), supersedes_id NULL, superseded_by_id NULL, change_request_id NULL, drafted_by, issued_by, issued_at)` — issued decisions are immutable. Only `decision_maker` issues. Issuing a decision that supersedes another sets the older one's `superseded_by_id` in the same transaction; at most one current (not superseded) issued permit-type decision per project; both remain visible. An in-system decision renders a printable HTML document (`GET /decisions/{id}/document`).
- `change_requests(project_id, kind reschedule_trip|extend_permit|expand_scope|other, description, payload_json, status open|approved|rejected|withdrawn, requested_by, resolved_by, resolution_note, resulting_decision_id NULL)` — `GET /change-requests/{id}/impact` lists affected bookings (conflicts at new dates), deliverables whose due dates fall before the new trip end, and decisions whose validity does not cover the new dates. Nothing else changes until a human acts on each item.

**Trips, resources, bookings**
- `resources(kind room|lab|equipment|boat|service, name, description, quantity INT, unit_label, provider_user_id NULL, active INT)`
- `tariffs(resource_id, unit per_night|per_day|per_item|per_hour, price_cents, currency, effective_from, created_by)` — price lookup = latest `effective_from <= date`.
- `trips(project_id, title, arrive_date, depart_date, participants_json [user_id|name], status planned|confirmed|completed|cancelled)` — `PATCH /trips/{id}` may change dates only while the trip has no `confirmed` bookings; otherwise dates change only via an approved `reschedule_trip` change request.
- `bookings(trip_id, resource_id, start_date, end_date, quantity, status requested|confirmed|declined|cancelled|released, requested_by, decided_by, decided_at, decline_reason)` — date intervals are half-open `[start_date, end_date)` (a room booked 3→5 is used the nights of 3 and 4); `quantity >= 1`; all bookings are day-granular (no hourly bookings; `per_hour` tariffs are not used). Confirm runs in `BEGIN IMMEDIATE`: for every day in the interval, the sum of quantities of confirmed bookings covering that day + this quantity must be ≤ resource.quantity (peak simultaneous usage), else 409 `capacity_conflict` naming the first conflicting day. Boat (provider) bookings are confirmed only by that provider. Cancelling/rescheduling a trip sets its bookings to `released`.

**Money**
- `invoices(project_id, number UNIQUE INV-YYYY-NNNN, status draft|issued|cancelled, currency, issued_at, due_date, created_by, cancelled_reason)` + derived (never stored) `settlement`: `unpaid|partially_paid|paid|overpaid` from verified payments minus verified refunds. Cancellation is independent of settlement and terminal; an invoice with net verified receipts > 0 can be cancelled only after refunding them.
- `invoice_lines(invoice_id, booking_id NULL, description, quantity, unit, unit_price_cents, amount_cents)` — prices are snapshotted when the line is created; lines are editable only while the invoice is `draft`; tariff changes never alter existing lines.
- `payments(invoice_id, kind payment|refund, amount_cents > 0, currency (must equal invoice currency), method bank_transfer|test_card|manual, external_ref UNIQUE NULL, request_hash, status pending_verification|verified|rejected, received_at, verified_by, note)` — refunds ≤ net verified receipts. Payments only on `issued` invoices. Overpayment is accepted and shown as `overpaid` (refundable). Bank notification endpoint is idempotent on `external_ref`: same ref + same payload → 200 `{duplicate:true}` and nothing changes; same ref + different payload → 409 `external_ref_conflict`.

**Deliverables (agreed results)**
- `deliverables(project_id, title, description, kind report|dataset|media|samples|other, due_date, sender_id (team member), recipient_id (staff), status proposed|agreed|submitted|changes_requested|accepted|waived|cancelled, terms_version INT, team_agreed_at NULL, team_agreed_by NULL, staff_agreed_at NULL, staff_agreed_by NULL, resolution_note, publish_level none|metadata|metadata_and_files, embargo_until NULL, published_at NULL, published_by NULL, created_by)` — a deliverable becomes `agreed` when both sides acknowledged the current `terms_version`; a material change (title, description, due date, recipient) increments `terms_version` and clears the other side's acknowledgement (due date changes also require a reason). Researcher dashboard: "What I must deliver to Pitcairn"; coordinator: "What we expect from the team".
- `deliverable_due_changes(deliverable_id, old_due, new_due, reason NOT NULL, changed_by, changed_at)`
- `deliverable_submissions(deliverable_id, number INT, submitted_by, note, data_dictionary_json [{column, description, unit, method}], status received|changes_requested|accepted, reviewed_by, review_note, reviewed_at)` UNIQUE(deliverable_id, number) — each item: `submission_files(submission_id, document_version_id)` and/or `external_links(submission_id, url, description, version_label, access_notes, last_checked_at, last_status, available INT)`. Referenced document versions must belong to the same project.
  - A link alone never auto-accepts a deliverable. Unavailable links are flagged to the coordinator.
  - Accepted submissions remain after newer ones; the deliverable shows the accepted one and the latest one.
  - Publication is explicit: the coordinator selects which files of the accepted submission are public (`publication_files(deliverable_id, document_version_id, approved_by, approved_at)`). Nothing becomes public from acceptance alone. `publish_level=metadata`: title, description, kind, project summary and generalized sites are public, files are not. `metadata_and_files`: plus the selected files — but only after `embargo_until` (before it, metadata is shown with "files available from <date>"). The coordinator is shown a warning that published files must not contain precise sensitive coordinates or personal data.
- `samples(project_id, code, site_id NULL, collected_on, material, custodian_org, storage_location, notes, related_deliverable_ids_json)`
- `measurements(project_id, deliverable_id, submission_id, site_name, observed_on, variable_key, value REAL, unit, source_label)` — filled when a dataset submission is accepted and it contains a CSV file in the ONE documented measurement format: header `site,date,variable,value,unit` (date `YYYY-MM-DD`, value numeric). Other CSVs are kept as files only. Charts group by `variable_key` and refuse to mix units (separate series per unit, labelled).

**Platform**
- `notifications(user_id, project_id NULL, kind, title, body, link, read_at NULL)` — always written in-app, independent of mail.
- `jobs(kind, payload_json, dedupe_key UNIQUE NULL, status queued|running|done|failed|dead, attempts, max_attempts, run_after, last_error, locked_until)` — in-process worker; kinds: `send_email`, `scan_file`, `check_link`, `deliverable_reminders` (daily; one reminder per deliverable per day via `dedupe_key`), `cleanup_uploads`. A `running` job whose `locked_until` passed is reclaimed (crash recovery).
- `mail_messages(to_email, subject, body_text, status queued|sent|failed, error, created_at, sent_at)` — the demo mail transport; settings key `mail_enabled=false` makes the transport fail so jobs retry with backoff and appear in Admin → Delivery issues.
- `settings(key PK, value)` — `mail_enabled`, `organisation_name`, `reference_prefix`, `public_catalog_enabled`.
- `audit_events(at, actor_id NULL, actor_label, action, entity_type, entity_id, project_id NULL, visibility shared|internal, summary, before_json, after_json, reason NULL)` — every significant change. Corrections of issued data require `reason`. The project timeline = audit events filtered by visibility.
- `idempotency_keys(user_id, route, key, request_hash, response_status, response_json, created_at)` PK(user_id, route, key) — stored in the same transaction as the effect; same key + same hash → replay stored response; same key + different hash → 422 `idempotency_key_reused`.
- `projects_fts` FTS5 over (title, summary, keywords, organisation, reference).
- `import_batches(kind legacy_csv|project_archive, status previewed|committed|discarded, preview_json, created_by)`

## 5. API (all JSON under `/api/v1`)

Conventions: cookie session `pitcairn_session` (HttpOnly, SameSite=Lax, Secure when configured); every non-GET request must carry header `X-Pitcairn-Csrf: 1` (else 403). Errors: `{ "error": { "code": "snake_case", "message": "Human readable", "fields": {"field": "msg"}? } }` with proper status (400/401/403/404/409/422/503). Lists: `?limit&offset`, response `{items, total}`. `Idempotency-Key` header honoured on submit, payment, bank notification and invoice issue. Request bodies are typed structs validated server-side (lengths, dates, enums, required template fields on submit) with field errors in `error.fields`. Every ID in a body (file, document version, site, booking, user) is checked to belong to the same project / be owned by the actor — a guessed ID never grants access.

- **auth**: `POST /auth/register`, `POST /auth/login` → `{user, mfa_required, mfa_enrollment_required}`, `POST /auth/mfa/verify`, `POST /auth/mfa/enroll` (returns otpauth URI + secret), `POST /auth/mfa/enroll/confirm`, `POST /auth/logout`, `GET /auth/me` (user, roles, mfa state, demo flag), `POST /invitations/{token}/accept`.
- **demo** (404 unless demo mode): `GET /demo/personas`, `POST /demo/switch {persona_key}`, `GET /demo/mailbox`, `POST /demo/bank/notify {invoice_id, amount_cents, external_ref}` (signs and calls the real webhook path), `GET /demo/totp/{user_id}` (current code for seeded staff, so MFA can be shown live).
- **dashboard**: `GET /dashboard` → role-specific sections. Researcher: `my_projects`, `needs_reply`, `decisions`, `trips_and_invoices`, `results_due`. Coordinator: `new_applications`, `waiting_for_applicant`, `with_experts`, `arriving_soon`, `results_to_check`, `overdue_results`. Expert: `assigned_reviews`. Base manager: `pending_bookings`, `arrivals`. Finance: `invoices_to_issue`, `payments_to_verify`. Provider: `my_requests`.
- **notifications**: `GET /notifications`, `POST /notifications/{id}/read`, `POST /notifications/read-all`.
- **templates**: `GET /templates`, `GET /templates/{key}/versions`, `POST /templates/{key}/versions` (admin, new draft from schema), `PUT /template-versions/{id}` (draft only), `POST /template-versions/{id}/publish`.
- **projects**: `POST /projects`, `GET /projects` (mine / staff filters), `GET /projects/{id}` (full workspace payload incl. `primary_message` = most urgent open action item for the viewer), `PATCH /projects/{id}` (draft/changes_requested only, autosave), `POST /projects/{id}/submit` (Idempotency-Key), `POST /projects/{id}/upgrade-template`, `POST /projects/{id}/withdraw`, `POST /projects/{id}/close {deliverable_resolutions:[{deliverable_id, action waive|cancel, note}]}` (409 `unresolved_deliverables` listing them if any planned/submitted/changes_requested remain without resolution), `GET /projects/{id}/revisions`, `GET /projects/{id}/revisions/{n}/diff?against=m`, `GET /projects/{id}/timeline`.
- **team**: `GET /projects/{id}/members`, `POST /projects/{id}/invitations`, `PATCH /projects/{id}/members/{user_id} {role}`, `POST /projects/{id}/members/{user_id}/make-lead`, `DELETE /projects/{id}/members/{user_id}`.
- **sites**: `GET/POST /projects/{id}/sites`, `PATCH/DELETE /sites/{id}`, `GET /sites/search?bbox=minLng,minLat,maxLng,maxLat` (staff), `GET /projects/{id}/sites.geojson`.
- **uploads**: `POST /uploads {filename,size,sha256,mime}` → `{upload_id, chunk_size, chunks_total, chunks_received}`, `PUT /uploads/{id}/chunks/{n}` (raw bytes), `GET /uploads/{id}`, `POST /uploads/{id}/complete` → verifies size+sha256 → `{file_id, scan_status}`, `DELETE /uploads/{id}`.
- **documents**: `GET /projects/{id}/documents`, `POST /projects/{id}/documents {slot_key?, title, category, file_id, note?}`, `POST /documents/{id}/versions {file_id, note}`, `GET /document-versions/{id}/download` (authz re-checked on every request; `Cache-Control: private, no-store`; 409 if scan not clean).
- **conversation**: `GET /projects/{id}/threads`, `POST /projects/{id}/threads {anchor_type, anchor_key, visibility, body, action_item?: {addressed_to, title}}`, `POST /threads/{id}/messages`, `POST /action-items/{id}/resolve`.
- **review**: `POST /projects/{id}/reviews {expert_id, due_date}`, `POST /reviews/{id}/accept`, `POST /reviews/{id}/decline {reason}`, `POST /reviews/{id}/submit {opinion, recommendation}`, `GET /reviews` (expert: mine).
- **decisions**: `GET /projects/{id}/decisions`, `POST /projects/{id}/decisions` (draft; coordinator or decision_maker), `PATCH /decisions/{id}` (draft only), `POST /decisions/{id}/issue` (decision_maker only), `GET /decisions/{id}/document` (printable HTML).
- **change requests**: `POST /projects/{id}/change-requests`, `GET /change-requests/{id}/impact`, `POST /change-requests/{id}/approve {resulting_decision_id?, note}`, `POST /change-requests/{id}/reject`. Approving a `reschedule_trip` moves the trip dates, releases its bookings and re-creates them as `requested` for the new dates; it never touches decisions or deliverable dates — those appear in impact as items needing action.
- **trips/bookings**: `GET/POST /projects/{id}/trips`, `PATCH /trips/{id}`, `POST /trips/{id}/cancel`, `POST /trips/{id}/bookings`, `POST /bookings/{id}/confirm`, `POST /bookings/{id}/decline`, `POST /bookings/{id}/cancel`, `GET /calendar?from&to&resource_id` (base manager/coordinator), `GET /provider/bookings`.
- **resources/tariffs** (admin writes; authenticated users read so project editors can choose a resource and see its tariff): `GET/POST /resources`, `PATCH /resources/{id}`, `GET/POST /resources/{id}/tariffs`.
- **invoices/payments**: `GET /projects/{id}/invoices`, `POST /projects/{id}/invoices {booking_ids}` (draft lines priced from tariffs), `POST /invoices/{id}/issue`, `POST /invoices/{id}/cancel {reason}`, `POST /invoices/{id}/payments {amount_cents, method, external_ref?}`, `POST /payments/{id}/verify`, `POST /payments/{id}/reject`, `POST /invoices/{id}/refunds {amount_cents, note}`, `POST /invoices/{id}/pay-test-card` (team; demo test mode, never moves money), `POST /integrations/bank/notifications` (no session; HMAC-SHA256 hex of body in `X-Bank-Signature`; no CSRF header).
- **deliverables**: `GET/POST /projects/{id}/deliverables`, `PATCH /deliverables/{id}` (due date change requires `reason`; material change bumps `terms_version`), `POST /deliverables/{id}/agree` (team editor+ or coordinator acknowledges current terms_version), `POST /deliverables/{id}/submissions {note, data_dictionary, document_version_ids, links}`, `POST /submissions/{id}/request-changes {note}`, `POST /submissions/{id}/accept`, `POST /deliverables/{id}/waive {note}`, `PATCH /deliverables/{id}/publication {publish_level, embargo_until}` (coordinator), `PUT /deliverables/{id}/publication-files {document_version_ids}` (coordinator; only files of the accepted submission), `POST /external-links/{id}/check`.
- **samples**: `GET/POST /projects/{id}/samples`, `PATCH/DELETE /samples/{id}`.
- **search/reports** (staff): `GET /search/projects?q&organisation&year&bbox&status&has_overdue`, `GET /reports/deliverables`, `GET /reports/measurements/variables`, `GET /reports/measurements?variable_key` → series `{project_reference, project_title, site, observed_on, value, unit, source_label}`.
- **public catalog** (no auth): `GET /public/projects?q&year&bbox`, `GET /public/projects/{reference}`, `GET /public/files/{document_version_id}/download` — only files listed in `publication_files` of deliverables with `publish_level=metadata_and_files`, embargo passed, project not withdrawn. Never: personal docs, internal threads, expert opinions, invoices, application answers, audit events, precise sensitive coordinates.
- **admin**: `GET/POST /admin/users`, `PATCH /admin/users/{id}`, `POST /admin/users/{id}/roles {role}`, `DELETE /admin/users/{id}/roles/{role}`, `GET/PUT /admin/settings`, `GET /admin/jobs?status=failed|dead`, `POST /admin/jobs/{id}/retry`, `GET /admin/audit`, `POST /admin/import/legacy/preview` (CSV upload) → rows with `errors` and `duplicate_of`, `POST /admin/import/{batch_id}/commit`, `GET /projects/{id}/export` (ZIP), `POST /admin/import/project-archive` (ZIP from another install; preview then commit).
- **assist** (optional AI, `PITCAIRN_AI_MODE`): `POST /assist/extract-fields {template_version_id, text}` → suggested answers; `POST /assist/summary {project_id}`. Returns 503 `ai_unavailable` when off; the UI must work fully without it. Suggestions are never applied without the user clicking accept; no decision, access check or calculation calls it.

## 6. Files and uploads

- Chunks are 0-based; every chunk is exactly `chunk_size` (5 MiB) bytes except the last, which is exactly `size - (chunks_total-1)*chunk_size`; wrong length → 422 `bad_chunk_length`. Re-sending a received chunk overwrites it (idempotent). Only the upload's owner may touch it (else 404). Chunk routes get `DefaultBodyLimit::max(chunk_size + 1 MiB)`; all other routes keep axum's default 2 MiB JSON limit.
- Client hashes with hash-wasm incrementally, resumes by `GET /uploads/{id}` and re-sending missing chunks (upload id kept in localStorage keyed by file name+size+lastModified).
- `complete` sets status `finalizing` under a per-upload lock (chunk writes after that → 409), streams the part file through sha256 OUTSIDE any DB transaction; mismatch → 422 `checksum_mismatch` and status back to `open`. On success the bytes are moved atomically (write temp + rename) into `files/` before the `files` row is committed. Only the uploader (or staff) may attach a `file_id` to a document.
- Downloads always `Content-Disposition: attachment`, `X-Content-Type-Options: nosniff`.
- `scan_file` job (mock antivirus): rejects EICAR test string, executables (MZ/ELF magic), declared-vs-sniffed mime mismatch for common types; sets `scan_status`. Downloads of non-clean files → 409.
- No public URLs for private bytes. Public catalog downloads go through `/public/files/...` with publication checks on every request.

## 7. Background jobs

Single tokio worker loop, polls `jobs` every 2s, exponential backoff (30s × 2^attempts, max 6 attempts → `dead`). Failures never roll back the business action that enqueued them. Business change + audit event + notification rows + job rows are written in ONE transaction. Graceful shutdown on SIGTERM: stop taking jobs, finish the current one, close the pool.

## 8. Export, import, backup

- Project export ZIP: `manifest.json` (`format: "pitcairn-project-export"`, `schema_version`, exported_at, source install id), `records/<table>.json` for every project-scoped table (incl. audit events, revisions, decisions, deliverables, submissions, threads, messages, members as user stubs {id, email, name, organisation}), `files/<sha256>`. Import into another install: preview shows conflicts (same project id or reference, unknown users created as disabled stubs), then commit; idempotent on project id.
- Legacy import: CSV columns `reference,title,organisation,lead_name,lead_email,start_date,end_date,summary,keywords,site_name,lat,lng,report_title,report_url`; preview flags missing required values, bad dates, coordinates outside the Pitcairn EEZ bbox, and duplicates (same reference or same normalized title+organisation+year). Commit creates `legacy=1` closed projects.
- Project export ZIP also contains the dependency closure: referenced template versions, resources and tariffs, users as stubs. Import never imports roles, password hashes or TOTP secrets (stubs are created disabled, matched by email if the user exists). Import rejects path traversal entries, entries > `PITCAIRN_MAX_UPLOAD_BYTES`, total uncompressed > 10 GiB, unknown `schema_version`, and verifies every file sha256; preview is re-validated at commit.
- Import request bodies (`/admin/import/legacy/preview`, `/admin/import/project-archive`): the raw file bytes (`text/csv` / `application/zip`, run through the mock scanner first) or JSON `{file_id}` of a `clean` file uploaded via `/uploads` by the same admin. The previewed archive is staged under `uploads/import-<sha256>.zip` until commit; committing a batch twice → 409 `batch_not_previewed`. The CLI `import-project` validates and commits in one step.
- Backup: `pitcairn backup --out DIR` → `VACUUM INTO DIR/pitcairn.sqlite3` + copy `files/` (immutable, content-addressed, so copying after the db snapshot is consistent) + `backup.json` (row counts, sha256 of db and of every file). `pitcairn restore --from DIR` verifies all hashes and restores into an empty data dir. Tested round-trip.
- `pitcairn grant-role --email --role` (CLI, server operator only) bootstraps the first `decision_maker`/`admin` of a real installation.

## 9. Demo mode

- Banner: "Demo — fictional people and data. Do not upload real applications or personal documents." Upload and submit forms in demo mode require ticking "This is fictional test data".
- Persona switcher in header (only when `PITCAIRN_DEMO_MODE=true`); switching creates a normal session flagged `via_demo_switch` with MFA satisfied. Production installs have no switcher.
- Demo pages: Mailbox (all `mail_messages`), Bank simulator (finance), Story guide (the 9-step Anna/Maria walk-through with links).
- Seed (`seed-demo`): personas below, Annex 2 template v1, resources+tariffs, and historic projects in every state so dashboards, map, catalog and charts are populated. No real person, no real university.

| key | name | role | org |
|---|---|---|---|
| anna | Dr Anna Hart | researcher, lead | Te Moana University (fictional), Wellington NZ |
| liam, priya, tomasi | Liam Chen, Priya Nair, Tomasi Vea | team | same |
| lukas | Dr Lukas Weber | researcher (second team) | North Sea Marine Lab (fictional) |
| maria | Maria Ellis | coordinator | Marine Science Base office, Natural Resources Division (demo) |
| james | Dr James Okafor | expert | external reviewer, MSB Scientific Advisory Panel (demo) |
| helen | Helen Brooks | decision_maker | Permits — acting for the Governor / MSB Board (demo) |
| sam | Sam Torres | base_manager | Marine Science Base (demo) |
| ruth | Ruth Palmer | finance | Pitcairn Islands Government (demo) |
| david | David Lane | provider | "Bounty Bay Boat Hire" (fictional) |
| admin | Site Admin | admin | — |

Anna's "Coral health around Pitcairn" starts as a **draft** ready to submit, so the full story can be walked live.

## 10. Frontend structure

Routes: `/` landing, `/catalog`, `/catalog/:reference`, `/login`, `/register`, `/mfa`, `/invite/:token`, `/app` dashboard (role-aware), `/app/projects/:id/{overview|application|team|messages|review|decisions|trips|invoices|results|samples|sites|history|changes}`, `/app/search`, `/app/reports`, `/app/calendar`, `/app/finance`, `/app/provider`, `/app/reviews`, `/app/admin/{users|templates|resources|settings|jobs|import|audit}`, `/app/demo/{mailbox|bank|story}`.

Requirements: mobile-first responsive layout; full keyboard operation (visible focus, skip link, proper labels, dialogs trap focus, Esc closes); clear inline errors from `error.fields`; autosave of drafts (debounced PATCH, "Saved 10:42" indicator, works after reconnect); project workspace headline shows `primary_message` ("Maria asks you to add a description of observation sites") with the anchored item and a reply button; never show a bare "Approved" — show the decision with conditions.

## 11. Security checklist

Server-side authz on every route; private files only via authenticated routes; MFA for staff/experts; argon2id passwords; session tokens hashed at rest; CSRF header; rate-limit login (in-memory per IP+email); upload scan before any processing (CSV parsing only after `clean`); SSRF guard for link checks; no secrets in git (`.env.example` only); admin cannot issue decisions; audit for every significant change.

## 12. Acceptance scenarios (automated: backend integration tests + Playwright)

1. **Full story**: register a brand-new team (not seeded) → submit → coordinator requests a document anchored to a slot → team uploads → resubmit (both revisions kept) → assign expert → opinion → decision issued with conditions → trip + bookings confirmed → invoice → deliverables agreed → submit results → one returned → corrected → accepted → published → visible in catalog without login; a private file of that project returns 404/403 publicly.
2. **Changes**: after a submission, publish template v2 → the old revision still renders with v1 fields; reschedule a trip → bookings released/re-requested, impact lists permit validity and deliverable dates, but no decision is altered silently; amend one permit → old decision kept and marked superseded.
3. **People & access**: second team cannot read Anna's project/files (403); remove a colleague → their previous file link returns 403; expert opinion and internal threads never appear in public API responses.
4. **Failures & money**: interrupted chunked upload resumes; two concurrent confirmations of a single-unit resource → exactly one succeeds; duplicate bank notification counted once; with mail disabled actions succeed, in-app notifications exist, jobs show as failed with a readable error.
5. **Closure & transfer**: closing with an unresolved deliverable → 409 listing it; resolve explicitly → closes; export project → import into a fresh install → identical records & files; backup → restore → identical; no personal keys needed.
6. **Edge cases**: partial payment → refund → cancel; refund larger than receipts → 422; embargoed file not downloadable publicly before its date; attaching another project's document version or another user's `file_id` → 403/404; a `running` job with an expired lease is picked up again after restart; two concurrent autosaves with the same `version` → one 409 `stale_version`.
