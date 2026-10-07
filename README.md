# Pitcairn Research Permits & Data Hub

A working demo of a permit-and-results hub for the Pitcairn Islands Marine
Science Base: a foreign researcher applies to work on Pitcairn, and Pitcairn
staff take the case from first application to delivered results — screening,
expert review, a formal decision with conditions, base and boat bookings,
invoicing, agreed deliverables, receipt of results, and finally a public
catalog of what was published. This is a **demo**: all people, organisations
and amounts are fictional, and external integrations (mail, bank, card
payments, antivirus, AI assistant) are mocked behind interfaces; the
external-link checker makes real HTTP checks (with a deterministic mock for
the demo's reserved example hosts). Everything else is real — persisted,
enforced and audited.

**Live demo:** <https://pitcairn.shelfcompass.com>

## Why Pitcairn

The workflow, form fields and terminology are grounded in real official
documents (research notes with verbatim extracts live in
[`docs/research/`](docs/research/README.md)):

- The [Pitcairn Islands Marine Science Base](https://www.government.pn/marine-science-base)
  opened in February 2024 to host the international science community working
  in the islands' 842,000 km² marine protected area — with a laboratory, four
  bedrooms, a published NZD fee schedule, and boats/diving supplied by the
  community.
- The official
  [Annex 2 research application](https://www.government.pn/s/Annex-2-Research-Application.pdf)
  is what applicants fill in today and email to the MSB Manager: 17 fields
  plus four required attachments (H&S plan, insurance, CVs, permits), a
  six-month advance-notice rule, and a review path through the Scientific
  Advisory Panel to the MSB Board. The demo's base-use template mirrors it.
- The
  [UK Overseas Territories biodiversity strategy](https://www.gov.uk/government/publications/uk-overseas-territories-biodiversity-strategy/uk-overseas-territories-biodiversity-strategy)
  lists as a Pitcairn national priority: *"establish a research permit system
  to improve data management including collection, analysis, reporting and
  sharing."* That is exactly what this hub demonstrates.

## What it does

**Researcher (team lead / editor / viewer)**

- Apply through configurable form templates (the Annex 2 base-use template is
  seeded); drafts autosave and can be upgraded when a template changes.
- Chunked, resumable file uploads with checksums and a mock antivirus scan;
  required document slots per template.
- Reply to requests for information anchored to a field or document; resubmit
  creates a new immutable revision (old revisions and diffs are kept).
- Invite team members and manage roles; plan trips and request bookings.
- Pay invoices (a labelled TEST-MODE card that never moves money, or bank
  transfer); agree the deliverables Pitcairn expects; submit results as
  files, external links and a data dictionary.

**Coordinator (Pitcairn case officer)**

- Role dashboard: new applications, waiting-for-applicant, with-experts,
  arrivals, results to check, overdue results (including data links that
  went missing or unreachable, with the reason and last check time).
- Open applications for screening; create action items addressed to the team
  anchored to exactly what is missing; assign experts.
- Agree deliverables before fieldwork; check submitted results for *receipt*
  (not scientific truth), return items for correction or accept them.
- Choose what is published: metadata only or metadata plus selected files,
  with optional embargo; close projects; export a project ZIP.
- Settings → **Templates**: edit application fields, required documents and
  hints, publish a new version (no developer needed). Settings → **Users**:
  find people and grant/revoke the operational roles `expert` and `provider`.

**Expert**

- Sees only projects with an active assignment; reads the application and
  non-personal documents; submits an opinion with a recommendation.

**Decision maker**

- Drafts and issues permits, refusals and amendments with explicit
  conditions, permitted activities, restrictions and validity dates;
  printable decision document; superseding a decision keeps both on record.
- Settings → **Users**: lists and searches users and grants or revokes the
  `decision_maker` role — only a decision maker can (or the CLI, for the
  first one); nothing else.

**Base manager**

- Resource calendar; confirms or declines bookings for rooms, lab, equipment
  and services with per-day capacity conflict detection.
- Settings → **Resources & tariffs**: maintains the resource catalogue and
  its prices.

**Boat provider (external)**

- Sees only booking requests for their own resources (minimal project info:
  title, dates, team size, lead name); confirms or declines them.

**Finance**

- Creates invoices from bookings (tariff prices snapshotted per line), issues
  and cancels them, records payments and refunds, and verifies bank-transfer
  notifications (HMAC-signed, idempotent on the external reference).
- Settings → **Resources & tariffs**: adds new prices from an effective date
  (append-only; issued invoices keep their prices).

**Admin**

- Accounts (create, edit, disable) and every role except `decision_maker`
  (403 — the technical admin never gets permit-granting rights), application
  templates, resources and tariffs, settings (organisation name, reference
  prefix, mail, public catalog), delivery jobs with retry, audit log, and
  imports (legacy CSV, project archives). Gets no decision rights.

Nobody can grant a role to themselves.

**Public catalog visitor (no login)**

- Search published projects by text, year and map area; view metadata and the
  files the coordinator chose to publish, once any embargo has passed.
  Sensitive sites appear generalized to a 0.1° grid.

## Demo personas

`seed-demo` creates fictional users covering every role (password
`demo-pass-2026`, not needed with the switcher):

| Persona | Name | Role |
|---|---|---|
| `anna` | Dr Anna Hart | Researcher, project lead — Te Moana University (fictional), Wellington NZ |
| `liam` / `priya` / `tomasi` | Liam Chen, Priya Nair, Tomasi Vea | Anna's team (editors / viewer) |
| `lukas` | Dr Lukas Weber | Researcher, second team — North Sea Marine Lab (fictional) |
| `maria` | Maria Ellis | Coordinator — Marine Science Base office, Natural Resources Division (demo) |
| `james` | Dr James Okafor | Expert — MSB Scientific Advisory Panel (demo) |
| `helen` | Helen Brooks | Decision maker — acting for the Governor / MSB Board (demo) |
| `sam` | Sam Torres | Base manager — Marine Science Base (demo) |
| `ruth` | Ruth Palmer | Finance — Pitcairn Islands Government (demo) |
| `david` | David Lane | Boat provider — Bounty Bay Boat Hire (fictional) |
| `admin` | Site Admin | Administrator |

With `PITCAIRN_DEMO_MODE=true` the header shows a **persona switcher**: pick a
persona and a real session is created for that user (flagged
`via_demo_switch`, MFA satisfied) — no password needed. `/demo` offers the
same picker to anonymous visitors, and `/demo/story` is a nine-step guided
walkthrough. Demo mode also adds the demo mailbox, the bank simulator and a
"Demo — fictional people and data" banner. **Production installs have no
switcher**: the demo API routes return 404 and any `via_demo_switch` sessions
are deleted at start-up.

## Quick start (local)

Prerequisites: **Rust 1.98** (`backend/rust-toolchain.toml` pins it) and
**Node 22**.

```sh
# 1. Seed the demo data and start the API + demo UI server (one terminal)
cd backend
cargo run -- seed-demo
PITCAIRN_DEMO_MODE=true cargo run -- serve        # http://127.0.0.1:8080

# 2. Frontend dev server (second terminal)
cd frontend
npm ci
npm run dev                                     # http://localhost:5173
```

Open <http://localhost:5173>. Vite proxies `/api` and `/up` to the backend on
`127.0.0.1:8080`. Use the persona switcher or `/demo` to explore.

**Production-like single binary** — the backend serves the built SPA:

```sh
cd frontend && npm run build                    # writes frontend/dist
cd ../backend
PITCAIRN_STATIC_DIR=../frontend/dist cargo run --release -- serve
# → http://127.0.0.1:8080 (API, SPA and /up on one port)
```

## Docker

```sh
docker build -t pitcairn .
docker run -d --name pitcairn -p 8080:8080 \
  -v pitcairn_data:/var/lib/pitcairn \
  -e PITCAIRN_DEMO_MODE=true \
  pitcairn
docker exec pitcairn pitcairn seed-demo         # optional demo data
```

The multi-stage `Dockerfile` (node build → rust build → debian-slim runtime)
produces an image of roughly 124 MB containing the stripped `pitcairn` binary
and the static frontend. State lives in the `/var/lib/pitcairn` volume
(SQLite database, uploaded files, backups).

## Configuration

All configuration is environment variables; unset secrets are generated
randomly per start (with a log warning). See `.env.example` for a commented
copy.

| Variable | Default | Meaning |
|---|---|---|
| `PITCAIRN_BIND` | `127.0.0.1:8080` | Listen address (`0.0.0.0:8080` in the image) |
| `PITCAIRN_DATA_DIR` | `./data` | SQLite db, `files/`, `uploads/`, `backups/` |
| `PITCAIRN_STATIC_DIR` | `../frontend/dist` | Built SPA directory |
| `PITCAIRN_BASE_URL` | `http://localhost:8080` | Public URL used in mail links |
| `PITCAIRN_DEMO_MODE` | `false` | Persona switcher, demo mailbox, bank simulator, demo banner |
| `PITCAIRN_SESSION_SECRET` | random per start | Cookie signing key — set it so sessions survive restarts |
| `PITCAIRN_BANK_WEBHOOK_SECRET` | random per start | HMAC secret for bank notifications |
| `PITCAIRN_MAX_UPLOAD_BYTES` | `2147483648` (2 GiB) | Max size per uploaded file |
| `PITCAIRN_LINK_CHECK_MODE` | `live` | `live`: real HTTP check of submitted data links (reserved demo hosts stay mocked); `mock`: deterministic mock for every URL — see below |
| `PITCAIRN_LINK_CHECK_ALLOW_PRIVATE` | `false` | Test only: let live link checks reach loopback/private addresses |
| `PITCAIRN_AI_MODE` | `mock` | `mock` or `off` |
| `PITCAIRN_SECURE_COOKIES` | `false` | Set `true` when serving over TLS |

The CLI (`pitcairn --help`): `serve`, `migrate`, `seed-demo [--reset]`,
`create-admin --email --name`, `grant-role --email --role`,
`backup --out DIR`, `restore --from DIR [--force]`,
`export-project REF --out FILE.zip`, `import-project FILE.zip`,
`export-types`.

### External link checks

When results stay in a university repository the team submits the address,
a description and a version label. Each link is checked when it is
submitted, once a day afterwards, and on demand (coordinator → *Check now*
on the deliverable). Every check stores the outcome, the HTTP status code,
a short reason and the check time:

| Outcome | When | Coordinator alert |
|---|---|---|
| Available | 2xx (redirects followed) | — |
| Not found (`missing`) | HTTP 404 / 410 — the file was deleted or moved | dashboard + notification |
| Unreachable | DNS failure, timeout, refused connection, TLS error, 5xx, too many redirects | dashboard + notification |
| Login required | HTTP 401 / 403 / 407 or a redirect to a login page | none — closed access may be agreed |

The coordinator is notified once when a link turns missing/unreachable (and
again only after it recovered and failed anew). In `live` mode the checker
sends `HEAD` (falling back to a one-byte ranged `GET` on 405/501) with 10 s
timeouts and at most 5 redirects. SSRF guard: only `http(s)`; every hop's
host is resolved and refused if it points at a loopback, private,
link-local, unique-local, multicast or unspecified address; the connection
is pinned to the vetted address; proxies are not used. Reserved demo hosts
(`*.invalid`, `*.example`, `*.test`, `example.org/.com/.net`) keep the
deterministic mock in both modes: `.invalid` hosts are unreachable, paths
containing `/missing` are not found, paths containing `/restricted` need a
login, everything else is available.

## Tech stack & layout

```
backend/    Rust 2024, axum 0.8, sqlx 0.8 (SQLite), tokio — single binary `pitcairn`
frontend/   React 19 + TypeScript + Vite, React Router 7, TanStack Query,
            Tailwind CSS 4, react-leaflet, recharts, hash-wasm
e2e/        Playwright acceptance tests
docs/       architecture.md (binding contract), research/, operations.md
config/     deploy.yml — Kamal deployment
Dockerfile  multi-stage: node build → rust build → debian-slim runtime
```

- One binary serves `/api/v1/*`, the built SPA (with `index.html` fallback)
  and the `/up` health check.
- SQLite in WAL mode; migrations in `backend/migrations/` run automatically
  on `serve` start (or via `pitcairn migrate`).
- TypeScript DTOs are generated from Rust with `ts-rs` into
  `frontend/src/api/generated/` (`cargo run -- export-types`); the frontend
  never hand-writes DTO types.
- Background jobs (mail, file scan, link checks, reminders) run in an
  in-process worker with retry/backoff.

**Architecture and product contract:** [`docs/architecture.md`](docs/architecture.md) —
domain model, roles matrix, API surface, security checklist, acceptance
scenarios. Operations runbook: [`docs/operations.md`](docs/operations.md).
Presenter walkthrough: [`docs/demo-script.md`](docs/demo-script.md).

## Tests

```sh
cd backend && cargo test        # integration tests over the HTTP API
cd frontend && npm test         # vitest
cd e2e && npm ci && npx playwright install chromium && npm test
```

The e2e suite seeds a fresh temporary data directory and drives the real UI
in headless Chromium (see [`e2e/README.md`](e2e/README.md)); `npm test` in
`frontend/` runs vitest and `npm run lint` / `npm run typecheck` are the
style gates.

## Security notes

- All authorization is server-side (`backend/src/authz.rs`); document
  downloads re-check access on every request and there are no public URLs
  for private bytes.
- Staff and experts must complete TOTP MFA before any non-auth endpoint
  works; passwords are argon2id; session tokens are stored hashed.
- The public catalog only ever exposes what a coordinator explicitly
  published — never personal documents, internal threads, expert opinions,
  invoices or precise sensitive coordinates.
- No secrets in the repo (`.env.example` contains placeholders; real env
  files and `.kamal/secrets` are git-ignored).
- External integrations are mocked behind interfaces: mail transport, bank
  notifications, card payments, antivirus scan, AI assistant. The only
  outbound HTTP is the external-link check, which refuses private and local
  network addresses (SSRF guard).
  The AI assistant only suggests — it never decides, grants access or
  calculates.

## Licence & using this without us

MIT — see [`LICENSE`](LICENSE).

This demo was built for the Government of the Pitcairn Islands, but nothing
in it is tied to the authors: clone it, build the binary or the image, run
`create-admin` for the first user and `grant-role` for the first decision
maker, configure your own templates, resources and tariffs, and run it as a
real installation (`PITCAIRN_DEMO_MODE=false`). The operations runbook in
[`docs/operations.md`](docs/operations.md) covers install, first admin,
backups, restore, upgrades and deployment.
