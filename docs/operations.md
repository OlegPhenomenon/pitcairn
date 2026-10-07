# Operations runbook

How to install, bootstrap, back up, upgrade and troubleshoot a real
(non-demo) installation of the Pitcairn Research Permits & Data Hub. The
whole application is a single `pitcairn` binary plus a data directory; there
is no external database, queue or cache to operate.

- Data directory (`PITCAIRN_DATA_DIR`, `/var/lib/pitcairn` in the image):
  `pitcairn.sqlite3` (SQLite, WAL mode), `files/` (uploaded bytes,
  content-addressed), `uploads/` (in-progress chunked uploads),
  `backups/` (conventional place for backup output).
- Health check: `GET /up` → `{"status":"ok"}` (unauthenticated; this is what
  kamal-proxy probes per `config/deploy.yml`).
- Migrations run automatically on `serve` start and via `pitcairn migrate`.
- Logs: `tracing` to stderr, `RUST_LOG` env filter (default
  `pitcairn=info,tower_http=info`).

## Install

### Docker (recommended)

```sh
docker build -t pitcairn .          # or pull your published image
docker run -d --name pitcairn \
  -p 8080:8080 \
  -v pitcairn_data:/var/lib/pitcairn \
  -e PITCAIRN_BASE_URL=https://hub.example.gov \
  -e PITCAIRN_SESSION_SECRET=<openssl rand -hex 32> \
  -e PITCAIRN_BANK_WEBHOOK_SECRET=<openssl rand -hex 32> \
  pitcairn                          # CMD is "serve"
```

The image runs as an unprivileged user, exposes 8080 and declares
`/var/lib/pitcairn` a volume. Any CLI command runs inside the container:

```sh
docker exec pitcairn pitcairn seed-demo        # optional demo data
docker exec pitcairn pitcairn backup --out /var/lib/pitcairn/backups/manual
```

### Binary

```sh
cd frontend && npm ci && npm run build
cd ../backend && cargo build --release --locked
PITCAIRN_STATIC_DIR=/path/to/frontend/dist \
PITCAIRN_DATA_DIR=/var/lib/pitcairn \
PITCAIRN_BASE_URL=https://hub.example.gov \
  target/release/pitcairn serve
```

Put it behind your TLS terminator (nginx, Caddy, kamal-proxy) and set
`PITCAIRN_SECURE_COOKIES=true`. A minimal systemd unit only needs
`ExecStart`, `Environment=`, `WorkingDirectory=` and `Restart=on-failure`;
the process stops cleanly on SIGTERM (finishes the current job, closes the
pool).

## First admin and first decision maker

The admin can grant every role *except* `decision_maker` (separation of
duties: only an existing decision maker — in the UI under Settings → Users —
or the server operator via the CLI, can create another). Bootstrap over the
CLI:

```sh
pitcairn create-admin --email admin@example.gov --name "Site Admin"
# prints a generated password — hand it to the admin, they must change it
# and complete MFA enrollment on first login

pitcairn grant-role --email decision@example.gov --role decision_maker
# the user must already exist (they register via /register first)
```

Valid roles: `coordinator`, `expert`, `decision_maker`, `base_manager`,
`finance`, `admin`, `provider`. Granting a user their first staff/expert
role resets MFA on all their sessions, forcing enrollment. Admins cannot
reset the password/MFA or disable a user who holds `decision_maker` — that
is a deliberate `protected_user` 409; use the CLI.

## Configuring the installation in the UI

Everything below lives under `/app/admin/*` (the header's *Admin* menu, or
*Settings* for non-admin staff) and needs no restart. Each screen is open to
the roles that own the data:

| Screen | Roles |
|---|---|
| Users — list/search | admin, decision_maker, coordinator (base_manager may list users to pick a provider) |
| Users — create, edit, disable | admin |
| Grant/revoke `decision_maker` | decision_maker only (admin gets 403 `cannot_grant_decision_maker`) |
| Grant/revoke `expert`, `provider` | admin, coordinator |
| Grant/revoke `coordinator`, `base_manager`, `finance`, `admin` | admin |
| Templates | admin, coordinator |
| Resources | admin, base_manager |
| Tariffs (prices) | admin, base_manager, finance |
| Settings, Jobs, Audit log, Import | admin |

Nobody may grant a role to themselves (403 `cannot_grant_self`).

- **Users** (`admin/users`): create users, grant/revoke roles, disable
  accounts. Each user only sees the role controls they may use.
- **Templates** (`admin/templates`): application form schemas. Draft a new
  version, publish it; new projects bind the latest published version and
  old drafts show an upgrade banner. Never edit a published version —
  submitted revisions render against the schema they were submitted with.
- **Resources & tariffs** (`admin/resources`): rooms/lab/equipment/boats/
  services, quantities, and time-based tariff rows (`per_night`/`per_day`/
  `per_item`; `per_hour` is unused — bookings are day-granular). Set
  `provider_user_id` on a resource to route its bookings to an external
  provider account. Existing invoice lines keep their snapshotted prices.
- **Settings** (`admin/settings`): `organisation_name`, `reference_prefix`
  (e.g. `PIT` → `PIT-2026-0001`), `public_catalog_enabled`, and
  `mail_enabled` (off = simulate a mail outage; jobs queue and fail into
  Admin → Jobs).
- **Import** (`admin/import`): legacy records (CSV, or ZIP with old
  documents and result files) and project-archive ZIP, each with a preview
  step before commit.
- **Jobs** (`admin/jobs`): failed/dead background jobs with retry.
- **Audit log** (`admin/audit`): every significant change.

## Backups

`pitcairn backup --out DIR` snapshots the database (`VACUUM INTO` — safe on
a live server), copies `files/` (content-addressed and immutable, so copying
after the snapshot stays consistent) and writes `backup.json` with row
counts plus the sha256 of the database and every file.

```sh
# cron, nightly at 03:15 — keep the last 14 backups next to the data dir
15 3 * * *  /usr/local/bin/pitcairn backup --out /var/lib/pitcairn/backups/$(date +\%F) \
            && find /var/lib/pitcairn/backups -mindepth 1 -maxdepth 1 -mtime +14 -exec rm -rf {} +
```

Then copy the backup directory **off-site** (object storage, a second host,
`rsync`). The backup is only as good as its distance from the server.
On Docker, either `docker exec pitcairn pitcairn backup --out
/var/lib/pitcairn/backups/<date>` (lands inside the volume) or run a
`--volumes-from pitcairn` sidecar.

## Restore

`pitcairn restore --from DIR` verifies every hash in `backup.json` and
restores into an **empty** data dir; `--force` overwrites a non-empty one.
Verify afterwards:

```sh
# stop the app first
pitcairn restore --from /var/lib/pitcairn/backups/2026-10-07
# → "restored … (hashes verified, N files)"
pitcairn serve & curl -sf http://127.0.0.1:8080/up
# then log in and spot-check a project and a file download
```

Test restores periodically — a backup that has never been restored is a
hope, not a backup.

## Upgrade

1. **Back up first** (`backup --out`). Migrations are forward-only.
2. Pull/build the new image or binary.
3. Restart `serve`; pending migrations in `backend/migrations/` apply
   automatically at start-up before the listener opens.
4. Check `/up`, then the log line `pitcairn listening` for a clean start.

Downgrade = restore the pre-upgrade backup into an empty data dir and run
the old binary.

## Moving projects between installations

- One project: `pitcairn export-project PIT-2026-0001 --out project.zip`
  (also `GET /api/v1/projects/{id}/export` for coordinator/admin).
  `pitcairn import-project project.zip` on the other install imports it;
  the same archive imported twice is a no-op. Unknown users become disabled
  stub accounts matched by email — no roles, passwords or TOTP secrets are
  ever exported. Files are re-scanned on import. The web UI equivalent is
  Admin → Import → "Project archive ZIP" (preview shows conflicts, then
  commit).
- Legacy records: `backend/fixtures/legacy_projects_sample.csv` is a working
  example of the legacy CSV format (Admin → Import also offers a
  downloadable template):

  `reference,title,organisation,lead_name,lead_email,start_date,end_date,summary,keywords,site_name,lat,lng,report_title,report_url,report_file,dataset_title,dataset_file,application_file`

  Only the first seven columns are required. To import old documents and
  results, upload a **ZIP** with the CSV at its top level (one wrapping
  folder is fine) and the files under `files/`; file columns name paths
  below `files/`:
  - `application_file` — `;`-separated, oldest first: the versions of the
    project's "Application" document.
  - `report_file` — one file attached to the accepted report deliverable
    `report_title` (alongside `report_url` when both are given).
  - `dataset_file` — `;`-separated: the files of an accepted dataset
    deliverable `dataset_title` (default "Dataset").

  Allowed file types: PDF, TXT/MD, CSV/TSV, PNG/JPEG/GIF/TIFF, DOCX, XLSX,
  ZIP, NetCDF. Each file must be within the upload size limit
  (`PITCAIRN_MAX_UPLOAD_BYTES`) and pass the same scanner as uploads.

  Preview flags missing required fields, bad dates, coordinates outside the
  Pitcairn EEZ bounding box, duplicates (same reference, or same normalized
  title+organisation+year), and per-file problems (missing from the ZIP,
  type not allowed, oversize, rejected by the scan, or file columns in a
  plain CSV). The admin UI blocks commit while any row has an error or a
  possible duplicate. Commit creates closed `legacy=1` projects with their
  documents (all versions) and accepted, metadata-published deliverables.
  Upload via Admin → Import → "Legacy records (CSV or ZIP)".

## External link checks

Submitted result links (data left in a university repository) are checked
on submission, once a day, and when a coordinator presses *Check now* on the
deliverable. Outcomes: `available`, `missing` (HTTP 404/410), `unreachable`
(DNS failure, timeout, refused connection, TLS error, 5xx, refused by the
SSRF guard) and `login_required` (401/403/407 or a redirect to a login
page — shown as such, never as data loss). Only `missing`/`unreachable`
reach the coordinator dashboard and trigger a notification.

| Variable | Default | Meaning |
|---|---|---|
| `PITCAIRN_LINK_CHECK_MODE` | `live` | `live`: real HTTP check (`HEAD`, ranged `GET` fallback, 10 s timeouts, ≤ 5 redirects) for every URL except reserved demo hosts (`*.invalid`, `*.example`, `*.test`, `example.org/.com/.net`), which keep the deterministic mock. `mock`: never touch the network (offline installs, tests). |
| `PITCAIRN_LINK_CHECK_ALLOW_PRIVATE` | `false` | Test only. `true` lets the live check reach loopback/private/link-local addresses; leave unset in production (logged as a warning at start-up). |

The server needs outbound HTTPS (port 443, sometimes 80) to the
repositories; HTTP proxies from the environment are deliberately ignored
because the checker vets and pins every address it connects to. Links that
resolve to private or local addresses are reported `unreachable` with
reason "Refused: address is in a private or local network".

## Demo mode vs production

| | Demo (`PITCAIRN_DEMO_MODE=true`) | Production |
|---|---|---|
| Persona switcher + `/demo` picker | yes | absent; demo routes 404 |
| Demo mailbox, bank simulator, story guide | yes | absent |
| Demo banner + "fictional data" ticks on uploads/submits | yes | absent |
| `via_demo_switch` sessions | kept | deleted at start-up |
| Staff/expert MFA | satisfied by switching | real TOTP enrollment required |

Never run a real installation with `PITCAIRN_DEMO_MODE=true` — anyone who
can reach it can act as any persona.

## Kamal deployment (the live demo)

The repository ships `config/deploy.yml` for the public demo at
<https://pitcairn.shelfcompass.com>. Facts about that deployment:

- The app sits behind **kamal-proxy** with automatic Let's Encrypt TLS on
  `pitcairn.shelfcompass.com` → app port 8080; healthcheck `/up`.
- Image `ghcr.io/olegphenomenon/pitcairn`, built **locally on arm64**
  (`builder.local: true`) because the small server cannot do a Rust release
  build; the runtime image is ≈124 MB (15 MB stripped binary + 1.2 MB static
  frontend + debian-slim).
- Secrets come from a git-ignored `.env.deploy`: `PITCAIRN_SESSION_SECRET`
  and `PITCAIRN_BANK_WEBHOOK_SECRET` (generate with `openssl rand -hex 32`)
  plus `KAMAL_REGISTRY_PASSWORD` = a GitHub token with `write:packages`.
  `.kamal/secrets` only forwards these names (see
  `.kamal/secrets.example`).

Deploy (Kamal 2.x, `gem install kamal`):

```sh
# DNS: A record pitcairn.shelfcompass.com → 188.34.152.24 (already done)
set -a; source .env.deploy; set +a
kamal setup        # first time: installs Docker, proxy, app
kamal deploy       # subsequent releases

# demo data on the server (idempotent; --reset wipes and reseeds)
kamal app exec --reuse 'pitcairn seed-demo'
```

Your own installation: copy `config/deploy.yml`, change `servers`, `proxy.host`,
`registry` and the `env.clear` block (`PITCAIRN_DEMO_MODE: "false"` for
production), and provide the same three secret names.

## Troubleshooting

- **Mail not arriving / want to test an outage**: Admin → Settings toggles
  `mail_enabled`. With mail off, actions still succeed and in-app
  notifications exist, but `send_email` jobs fail and appear under
  Admin → Jobs ("Delivery jobs") with the error; retry them after fixing
  the transport. Mail status is also visible per message in
  `mail_messages`.
- **Stuck or failed background jobs**: Admin → Jobs lists `failed` and
  `dead` jobs (jobs retry with exponential backoff, 30 s × 2^attempts, max
  6 attempts → `dead`); `POST …/retry` requeues. A `running` job whose lease
  expired is reclaimed automatically after a crash/restart.
- **Disk space**: growth concentrates in `files/` (sha256-deduplicated) and
  `pitcairn.sqlite3`. Watch the volume; `backups/` inside the data dir
  counts too — ship backups off-site rather than accumulating them.
  Orphaned `uploads/*.part` files are reclaimed by the `cleanup_uploads`
  job.
- **Sessions all invalidated after a restart**: `PITCAIRN_SESSION_SECRET`
  was unset and a new random one was generated. Set it.
- **Bank notifications not verifying**: check the `X-Bank-Signature` HMAC
  matches `PITCAIRN_BANK_WEBHOOK_SECRET`; a repeated notification with the
  same `external_ref` is deduplicated (`{duplicate:true}`), a conflicting
  one returns `external_ref_conflict`.
- **Uploads fail near the size limit**: `PITCAIRN_MAX_UPLOAD_BYTES` (default
  2 GiB) per file; the proxy's own body-size limits must also allow the
  5 MiB chunks.
- **Can't log in / CSRF errors**: non-GET requests need the
  `X-Pitcairn-Csrf: 1` header (the SPA sends it); behind TLS set
  `PITCAIRN_SECURE_COOKIES=true` so the session cookie is accepted.
- **Public file unexpectedly 404s**: intended — only files explicitly
  attached to `publication_files` on a `metadata_and_files` deliverable past
  its embargo are downloadable; personal documents, application answers and
  internal material are never public.
- **Every external link shows "Unreachable"**: the server has no outbound
  HTTP(S) access (firewall, or egress only via a proxy — the checker ignores
  proxies). Check the reason on the deliverable ("Connection timed out",
  "Host name could not be resolved"…); set `PITCAIRN_LINK_CHECK_MODE=mock`
  on an air-gapped installation. "Login required" is not an error: the
  repository needs a sign-in, which may be the agreed closed access.
