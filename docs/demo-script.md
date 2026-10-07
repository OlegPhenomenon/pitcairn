# Demo script — 10-minute walkthrough for Pitcairn officials

Audience: MSB Board, ECNRD staff, Marine Permit Officer, Governor's office.
Goal: show the whole permit-and-results lifecycle live — not slides.
Everything on screen is fictional; say so once, up front.

**Setup before you start (2 min beforehand):**

- Open <https://pitcairn.shelfcompass.com> (or your local
  `PITCAIRN_DEMO_MODE=true` install).
- Open `/demo/story` — the public **story guide**. Its nine cards switch the
  persona *and* open the right screen, so the walkthrough needs no passwords.
  Keep the persona switcher (header, top right) as the backup.
- Second browser tab on `/catalog` for the ending.
- If you want a clean run, reseed first (`cargo run -- seed-demo --reset`
  locally; `kamal app exec --reuse 'pitcairn seed-demo --reset'` on the live
  demo — it wipes and reseeds).

Times are approximate; the guide's "Switch to _ and open" buttons do the
navigation for you.

## Step 1 — Anna submits the application (≈1.5 min)

**Persona: Dr Anna Hart (researcher).** Story guide → "Submit the
application" → opens **Application**.

- The form is the real Annex 2: applicant details, funders, named
  researchers, aims/objectives/methods, *anticipated outputs and benefit to
  the Pitcairn Island community*, data management and sharing, resources,
  timeline, budget, and research sites on the map.
- Point at the document slots: field safety plan, insurance, CVs, permits —
  the four attachments the paper form requires.
- Say: *"Anna's draft is already complete — today she just sends it. Every
  change autosaves; the site marks sensitive so precise coordinates never
  leave the staff view."*
- Click **Submit application** (tick "fictional test data" in demo mode).
  The project gets its reference `PIT-2026-XXXX` and lands in Maria's queue.

## Step 2 — Maria requests a missing document (≈1 min)

**Persona: Maria Ellis (coordinator).** Story guide → "Request a missing
document" → opens **Messages**.

- Maria's dashboard showed the new application under *New applications*;
  she opened it for screening and found a gap.
- Show: a message anchored to the **safety_plan document slot** with
  *Request information* — it becomes an **action item** addressed to the
  team, not just a chat line.
- Say: *"Requests are anchored to the exact field or document, so the team
  sees precisely what is missing — and the status moves to
  changes_requested."*

## Step 3 — James gives an expert view (≈1 min)

**Persona: Dr James Okafor (expert).** Story guide → "Get an expert view" →
opens **Review**.

- Maria assigned James; he sees *only* this project and only non-personal
  documents — no passports, no insurance, no internal staff notes.
- Click **Accept assignment**, write a line of opinion with recommendation
  *approve with conditions*, **Submit opinion**.
- Say: *"This is the Scientific Advisory Panel step. Opinions are always
  internal — applicants and the public never see them."*

## Step 4 — Helen issues the decision (≈1.5 min)

**Persona: Helen Brooks (decision maker).** Story guide → "Issue the
decision" → opens **Decisions**.

- Open the drafted permit, show its parts: basis, legal reference, validity
  dates, **permitted activities**, **conditions**, restrictions — then
  **Issue decision**.
- Open the printable decision document (`/decisions/{id}/document`).
- **Point out:** *"This is the heart of the system — a decision with
  explicit conditions, not a booking. Bookings come later and separately;
  a permit can be amended, and the superseded version stays on record.
  Only a decision maker can issue — an admin cannot."*
- Optional: open Dr Mele Tupou's *Coral cover transects* project →
  **Decisions**. It holds two independent permits — the monitoring permit
  and "Coral tissue sampling for genetics". Each has its own **Amend /
  Extend / Revoke this permit** buttons; changing one never alters the
  other, and the replaced version stays in the history.

## Step 5 — Anna plans the trip and bookings (≈1.5 min)

**Persona: Anna.** Story guide → "Plan a trip and bookings" → opens
**Trips**.

- **Plan a trip** with the fieldwork dates, then **Request booking** for a
  room/lab — and David's boat, which is a *provider* resource.
- If time allows, switch to **Sam Torres** (base manager) and **David Lane**
  (boat provider) to show each confirming their own bookings; the boat
  owner sees only his booking with minimal project details.
- Say: *"The permit says what Anna may do; the calendar says where she
  sleeps and which boat she takes. The system refuses double-bookings."*

## Step 6 — Maria proposes what must be delivered (≈1 min)

**Persona: Maria.** Story guide → "Agree what must be delivered" → opens
**Results**.

- Maria creates deliverables: the final report, the coral survey dataset —
  each with a due date, a sender on the team and a staff recipient.
- **Point out:** *"Deliverables are agreed **before** anyone flies to
  Pitcairn — the island states up front what it expects back. Nothing here
  is 'send us whatever you feel like'."*
- (Money detour if asked: Ruth's finance dashboard issues an invoice from
  the bookings; Anna pays by test card or bank transfer; Ruth verifies.)

## Step 7 — Anna changes the agreement (≈1 min)

**Persona: Anna.** Story guide → "Change the agreement" → **Results**.

- Anna proposes a new due date **with a reason**; the terms version bumps
  and *both sides must agree again* — Maria re-confirms.
- Say: *"No silent edits — a material change re-opens the agreement, and
  every due-date change keeps its reason."*

## Step 8 — Anna submits, Maria returns one item (≈1 min)

**Persona: Anna.** Story guide → "Submit and return results" → **Results**.

- **Submit results**: files, external links, and a data dictionary for the
  dataset.
- Switch to **Maria**: she checks receipt and clicks **Request changes** on
  one submission with a note; Anna resubmits.
- **Point out:** *"Receipt is not scientific validation — Maria confirms
  the delivery matches what was agreed; she does not peer-review the
  science."*

## Step 9 — Maria accepts and publishes (≈1 min)

**Persona: Maria.** Story guide → "Accept and publish" → **Results**.

- **Accept receipt**, then set the publish level — metadata only, or
  metadata plus selected files, optionally after an embargo date. Only the
  files Maria ticks go public.
- Switch to the catalog tab (`/catalog`, logged out view): the project now
  appears; only the published items are downloadable; the sensitive site
  shows as a generalized grid square, not a precise point.
- Say: *"The public catalog shows only published material — never personal
  documents, expert opinions, invoices or internal discussion."*

## Close (30 s)

- Open the project's **History** tab as Maria: every step above is in the
  audit trail — who did what, when, with reasons for corrections.
- Remind: demo data is fictional; the same build runs as a real
  installation with `PITCAIRN_DEMO_MODE=false` (no switcher, real MFA);
  admin can import legacy CSV records so decades of past projects sit next
  to new ones.
- Leave-behind line: *"From application form to public results — one
  system, one record, and Pitcairn decides what is published."*

## What to point out (cheat sheet for questions)

- **Decision ≠ booking** — the permit with conditions is the legal act;
  rooms and boats are separate operational bookings.
- **Deliverables agreed in advance** — expectations are set before
  fieldwork, not negotiated after.
- **Receipt ≠ scientific validation** — staff check that agreed results
  arrived; experts' opinions stay internal.
- **The catalog shows only published material** — explicit per-file
  publication, embargo-aware, generalized sensitive sites.
- **History is kept** — revisions, superseded decisions and the audit log
  are preserved; nothing is silently overwritten.
