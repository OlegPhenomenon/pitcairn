# Research notes — Pitcairn Research Permits & Data Hub

Source material gathered 2026-10-07 for grounding the demo's form fields,
workflow and terminology in real official documents. All documents live on the
Government of the Pitcairn Islands site (<https://www.government.pn>,
Marine Science Base page and Laws page) unless noted.

## Files

- **annex2-research-application.md** — verbatim field-by-field extract of the
  official "Annex 2" research application form (17 numbered fields + 4 required
  attachments), the six-month advance-notice rule, the submission mailbox
  (`dmnature@pitcairn.gov.pn`), and the downstream review workflow
  (MSB-SAP → MSB Board, accept/amend/reject within 2 weeks).
- **marine-science-base.md** — what the MSB offers (4 bedrooms, lab, conference
  room, equipment list, community-supplied boats/diving), published NZD fee
  schedule (NZ$90/person/night min.), the Annex 1 user agreement, Annex 3
  review criteria, governance (MSB Board / MSB-SAP / MSB Manager), and the
  legal-framework document list with URLs.
- **permits-and-regulations.md** — MPA Ordinance 2016 and Marine Conservation
  Regulations 2022: zones (MPA, Coastal Conservation Areas, 40 Mile Reef
  transit zone, Specially Protected Areas), all six permit types, who issues
  them (Marine Permit Officer vs Governor + Marine Environment Committee +
  Island Council), the marine scientific research permit's legal tests
  (benefit to Pitcairn, necessity, minimal harm, proportionality), conditions
  incl. mandatory data sharing with the community, fees, appeals, and the
  CITES-based Endangered Species Protection Ordinance for specimen export.
- **geography.md** — WGS84 coordinates for the four islands, Adamstown/Bounty
  Bay, 40 Mile Reef and Mangareva; derived EEZ bounding box; ~18 real named
  nearshore/landmark sites with approximate coordinates for demo survey
  locations; NZD currency, UTC−8 timezone, and the MV Silver Supporter
  Mangareva↔Pitcairn schedule pattern for realistic trip planning.
- **reference-systems.md** — short field/state notes on CKAN (AGPL-3.0),
  Dataverse (Apache-2.0) and Démarches simplifiées (AGPL-3.0), focused on
  dossier lifecycle states, expert reviews, private annotations, dataset
  versioning/embargo/restriction/guestbook — with a mapping table for reuse.

## Gaps / caveats

- **Marine Conservation Regulations 2022** is published only as a scanned
  (image-only) PDF; the content in `permits-and-regulations.md` was recovered
  by local OCR. Regulation numbers and structure verified; exact wording may
  contain minor OCR artifacts.
- **No published application forms for MCR 2022 permits were found.** Reg. 22
  says the Marine Environment Committee prescribes forms and publishes them on
  the public notice board and government website — none are currently on
  government.pn. Same for permit **fee notices** (reg. 31) — only MSB facility
  fees are published.
- **Annex 2 has no signature/declaration block** — signatures live in Annex 1
  (User Agreement, post-approval). The form contains an unfinished parenthetical
  "(….by the published cut off date…)" — no actual cut-off date is given.
- **Annex numbering inconsistency**: the "MSB Research Applications" process
  doc references "Annex 3/Annex 4" for format/criteria, but published annexes
  number them 2/3. Likely an older numbering scheme.
- Six-month rule ownership differs between documents: Annex 2 says prior
  permission from the **MSB Manager**; the process doc says the **MSB Board**.
- **40 Mile Reef / Adams Seamount coordinates** (-25.342, -129.292) come from
  OpenStreetMap (Nominatim), not an official chart; the regulations only say
  "~75 km SE of Pitcairn".
- Most named-site coordinates come from the GeoNames gazetteer and are
  approximate centroids; St Paul's Pool, Ship's Landing Point, The Edge,
  Western Harbour had no gazetteer fix — positions are inferred and marked
  [approx].
- Current officeholders listed in docs may be stale: the MSB page names the
  Manager as "TBC" while the 2023–24 MPA Annual Review names Sue O'Keefe.
- There is **no "Environment Conservation Ordinance"** in Pitcairn law
  (checked the alphabetical ordinance list); environmental instruments are the
  MPA Ordinance, MCR 2022, Endangered Species Protection Ordinance 2004,
  Biosecurity Ordinance 2024 and Local Government Regulations wildlife rules.
- The UK OT biodiversity strategy's "establish a research permit system…"
  bullet is genuinely a Pitcairn priority; two similar-sounding bullets on the
  same page belong to BAT and Ascension (noted in the file to avoid
  misattribution).
