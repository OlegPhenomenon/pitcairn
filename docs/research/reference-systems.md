# Reference systems — data models worth borrowing

Three open-source systems map well onto the demo's three main concepts:
**applications/dossiers** (Démarches simplifiées), **published datasets**
(CKAN, Dataverse). Notes kept short; follow the links for full schemas.

## CKAN

- Repo: <https://github.com/ckan/ckan> — **AGPL-3.0**
  (<https://github.com/ckan/ckan/blob/master/LICENSE.txt>).
- A dataset is a **package**. Core fields (from `package_create` docs,
  <https://docs.ckan.org/en/latest/api/index.html>): `name` (url slug),
  `title`, `author`/`author_email`, `maintainer`/`maintainer_email`,
  `license_id`, `notes`, `url`, `version`, `state` (`active`/`deleted`/`draft`
  — only `active` is publicly listed), `type`, `owner_org`, `private` flag,
  `tags[]`, `groups[]`, `resources[]`, `extras` (arbitrary key:value pairs;
  first-class custom fields via the `IDatasetForm` plugin interface —
  <https://docs.ckan.org/en/latest/extensions/adding-custom-fields.html>).
- **Resource** fields (<https://github.com/ckan/ckan/blob/master/ckan/model/resource.py>):
  `url`, `name`, `format`, `description`, `mimetype`, `mimetype_inner`,
  `resource_type`, `size`, `hash`, `position`, `created`, `last_modified`,
  `metadata_modified`, `url_type`, `state`.
- **Licences** come from the opendefinition.org list (`license_list` action);
  the form links to <http://opendefinition.org/licenses/>.
- **Search** is Solr-backed; standard facets: organization, groups, tags,
  `res_format`, `license_id`. Datasets sit inside **Organizations** (owning
  body) and can be private to the org.
- Takeaway for the demo: result deposits = datasets with `license_id`,
  `resources[]` per file, `extras` for permit linkage, tags for themes.

## Dataverse

- Repo: <https://github.com/IQSS/dataverse> — **Apache-2.0**
  (<https://github.com/IQSS/dataverse/blob/develop/LICENSE.md>).
- Hierarchy: **Dataverse collection → dataset → data files**. Datasets get a
  persistent identifier (DOI/Handle) once, not per version.
- Metadata is organised into **metadata blocks** (citation, geospatial, social
  science…). Citation block fields
  (<https://github.com/IQSS/dataverse/blob/develop/scripts/api/data/metadatablocks/citation.tsv>):
  `title`, `subtitle`, `alternativeTitle`, `otherId`, `author`
  (name/affiliation/identifierScheme/identifier), `datasetContact`,
  `dsDescription`, `subject`, `keyword` (+vocabulary+URI), `topicClassification`,
  `publication` (related), `notesText`, `language`, `producer`, `productionDate`,
  `contributor`, `grantNumber`, `depositor`, `dateOfDeposit`, `kindOfData`,
  `geographicCoverage`, …
- **Versions**: edits to a published dataset create a **draft version**;
  publishing is major (2.0) or minor (1.1); per-version "Version Note"; files
  carry their own version history; versions can be **deaccessioned**.
- **Submit for Review**: depositors can flag a dataset "In Review" so a curator
  approves before publication; anonymous **Preview URLs** (optionally
  anonymised for double-blind review) share unpublished datasets.
- **Restricted files** + "Terms of Access" fields (Data Access Place,
  Availability Status, Contact for Access) and optional **Request Access**
  workflow; file-level permissions grant access per user/group.
- **Embargoes** (file-level; an instance config): metadata/PID public at
  publish, files inaccessible until embargo end date; cannot be changed after
  publish (admin fixable); rolling embargoes via successive versions.
  **Retention periods** do the opposite (block access *after* a date).
- **Guestbook**: collection-level form shown at download — collects downloader
  name, email, affiliation, position + custom questions; must be answered
  before restricted-file download proceeds.
- Tabular ingest derives **variable-level metadata** (name, label, categories,
  summary stats, UNF fingerprint) — editable via API, exported as a DDI
  Codebook (a "data dictionary").
- Guide: <https://guides.dataverse.org/en/latest/user/dataset-management.html>

## Démarches simplifiées (demarche.numerique.gouv.fr)

- Repo: <https://github.com/demarche-numerique/demarche.numerique.gouv.fr> —
  **AGPL-3.0**. Docs: <https://doc.demarches-simplifiees.fr/>
- A **démarche** is a published procedure (form + instructeur routing +
  retention policy); a **dossier** is one submitted case. Roles: **usager**
  (applicant), **instructeur** (case officer, grouped into *groupes
  instructeurs*), **expert invité** (external reviewer), administrateur.
- **Lifecycle** (<https://doc.demarches-simplifiees.fr/api-graphql/les-mutations>):
  `brouillon` → `en construction` (UI renamed «déposé»; still editable by the
  usager) → `en instruction` (locked for the usager; instructeur can send it
  back via "demander une correction" / *repasser en construction*) → terminal
  `accepté` / `refusé` / `classé sans suite`; plus `archivé`.
  Expiry: brouillons deleted after 3 months idle; en construction/en
  instruction never expire; terminé expire per the démarche's retention.
- **Messagerie**: per-dossier thread usager↔instructeur (auto-disabled once
  archived); automatic messages on state change.
- **Avis d'experts**
  (<https://doc.demarches-simplifiees.fr/tutoriels/tutoriel-expert-invite>):
  instructeur emails an invitation; the expert sees the demande recap +
  attachments (zip download), answers an optional yes/no question plus free
  text + attachment; avis may be **confidential** (instructeur only) or shared
  among experts; no avis accepted after the dossier is decided; optional final
  decision notification to experts.
- **Annotations privées**: form fields on the dossier visible only to
  instructeurs (<https://doc.demarches-simplifiees.fr/api-graphql/cas-dusages-exemple-dimplementation/modifier-les-annotations-dun-dossier>).
- Also: labels on dossiers, attestation PDFs on accept/refuse when configured,
  GraphQL mutations `dossierPasserEnInstruction`, `dossierAccepter`,
  `dossierRefuser`, `dossierClasserSansSuite`, `dossierRepasserEnConstruction`,
  `dossierArchiver`.
- Takeaway for the demo: application states `draft → submitted → under review →
  approved / rejected / amendments requested`, a per-application message
  thread, expert reviews (MSB-SAP members), and staff-only notes.

## Field-mapping sketch for the demo

| Demo concept | Borrow from |
|---|---|
| Application lifecycle | DS dossier states (+ "amendments requested" from MSB Board's three-way decision) |
| Staff/SAP review | DS avis d'experts + annotations privées |
| Applicant↔staff thread | DS messagerie |
| Permit document | MCR reg. 28 fields (activities, named persons, duration, conditions) |
| Result/data deposit | CKAN package + resources or Dataverse dataset (versioning, embargo, guestbook, restricted files) |
