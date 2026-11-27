# Intake implementation plan

**Status:** Phases 1–2 delivered; desktop board next

**Scope:** CareLibre Server + CareLibre Desktop

**Related:** `CareLibre/docs/adr/0013-intake-cases.md`, `CareLibre/docs/intake.md`, `CareLibre/docs/permissions.md`

**Desktop entry points:** `src/pages/intake/`, `src/intake.rs`, `src/store/intake.rs`, `src/remote.rs`, `src/app.rs`

---

## 1. Purpose

This plan defines a universal intake capability for any care provider organisation. It is
not tied to a particular provider, region, funding scheme or service model.

Default flows, stages, fields, documents and checklists are starting templates. An
organisation can rename, reorder, replace or disable them. Jurisdiction-specific and
program-specific behaviour is expressed through configuration and integrations rather than
hardcoded assumptions.

Examples in this document (aged care, disability, insurance, private fee-for-service,
brokerage, emergency, community, residential) are illustrative. Any organisation can define
its own intake types and variants.

### 1.1 Universal design principles

| Principle | Application |
|---|---|
| Configuration over hardcoding | Flows, stages, fields, actions, checklists, documents and assignments are admin-configurable. |
| Organisation-scoped | Every record, catalogue and configuration row belongs to one tenant. |
| Division-scoped views | Boards can filter by division and child subdivisions. |
| Jurisdiction-neutral | Regional identifiers, currencies, time zones, languages and funding programs are configuration or integration concerns. |
| Program-neutral | Funding programs are configurable; no single scheme is mandatory. |
| Role-neutral | Placement, coordination, intake, rostering, finance and administration roles are capabilities, not fixed job titles. |
| Extensible by adapter | External portals, CRMs, document stores, e-sign providers and communication channels connect through adapters. |
| Safe defaults, editable templates | Seeded flows demonstrate a complete pipeline without preventing divergence. |
| Audit by default | Transitions, assignments, policy refusals and configuration changes are recorded. |
| Desktop and server parity | The server remains authoritative; clients render the same configuration and policy. |

---

## 2. Requirements

### 2.1 Functional requirements

| ID | Requirement |
|---|---|
| R1 | Display intake work as a kanban board whose columns are configurable intake stages. |
| R2 | Support multiple intake flows for clients, workers, other contacts and organisations. |
| R3 | Support configurable variants such as standard, aged care, disability standard, disability emergency, fee-for-service, brokerage or organisation-specific variants. |
| R4 | Support optional parent-flow grouping and flow cloning. |
| R5 | Create a contact at enquiry and reuse that contact across repeated intake attempts. |
| R6 | Match existing contacts by email, phone or name + date of birth before creating a new one. |
| R7 | Prevent duplicate open cases for the same contact and flow while allowing a new case after onboarding or closure. |
| R8 | Capture referral and lead data: source, external reference, referrer contact or organisation, and received time. |
| R9 | Track waitlisted cases with service, division, priority, requested start and offer status. |
| R10 | Allow each stage to have a named assignee, a permission-set queue, or both. |
| R11 | Alert assignees and watchers in-app when a case enters their stage. |
| R12 | Configure which fields are visible and required at each stage. |
| R13 | Configure which actions are allowed at each stage. |
| R14 | Configure checklists per stage, including required items that gate transition. |
| R15 | Filter the board by division or subdivision. |
| R16 | Show contact lifecycle status independently of intake stages. |
| R17 | Support configurable lifecycle labels over a stable set of lifecycle states. |
| R18 | Allow cancellation, archiving and reopening of cases with a reason. |
| R19 | Show a case timeline of transitions, assignments, checklist changes and lifecycle changes. |
| R20 | Configure flows, stages, fields, actions, checklists and assignees in an admin UI. |
| R21 | Enforce stage action policy on the server for profile, relationship, funding and roster actions. |
| R22 | Scope all records to the signed-in organisation. |
| R23 | Restrict intake configuration and case changes by capability. |
| R24 | Expose the intake API through the generated OpenAPI document. |
| R25 | Use optimistic concurrency for case edits and transitions. |
| R26 | Support idempotent case creation for client retries and legacy draft migration. |
| R27 | Support staff-assisted intake where a coordinator completes forms on behalf of a person. |
| R28 | Support configurable document requirements by service, funding program or jurisdiction. |
| R29 | Support rapid or emergency intake with incomplete identity data and placeholder values. |
| R30 | Support post-intake catch-up work after rostering has started. |
| R31 | Support quote-required and budget-only funding workflows. |
| R32 | Support capture of evidence of available or remaining funding. |
| R33 | Support follow-up reminders for open enquiries. |
| R34 | Support intake-type assignment groups in addition to stage assignment. |
| R35 | Support external lead references, referral identifiers and portal sync adapters. |
| R36 | Support a roster-readiness check separate from document completeness. |
| R37 | Support external referrer or self-service enquiry channels. |
| R38 | Support locale, currency, time-zone and language configuration. |
| R39 | Support reporting on intake volume, source, conversion, stage duration and waitlist position. |
| R40 | Audit every intake transition, assignment, configuration change and policy refusal. |

### 2.2 Non-functional requirements

| ID | Requirement |
|---|---|
| N1 | The UI thread never blocks on network or SQLite work. |
| N2 | Lists and board columns are virtualised or paged. |
| N3 | The desktop remains native Rust/egui; no embedded web view. |
| N4 | All intake writes are transactional and audited. |
| N5 | Schema changes ship as migrations with integration tests. |
| N6 | The OpenAPI contract and markdown contract stay in sync. |
| N7 | The board remains usable on narrow and wide desktop windows. |
| N8 | Sensitive actions require capability checks and are never protected only by hidden UI. |
| N9 | Configuration changes take effect without a server release. |
| N10 | Tests cover every guarded action key, transition rule and permission path. |
| N11 | External integrations are optional and isolated behind adapters. |
| N12 | No default flow, field, document or status assumes a particular jurisdiction or funding scheme. |

---

## 3. Solutions

### 3.1 Case-centred pipeline

**Solution:** An `intake_case` is the board card and pipeline unit.

- A case records flow, current stage, source, division, assignee, priority, referral data and version.
- A contact is created at enquiry and linked to the case.
- The care or employment relationship is linked when the case reaches the role-link stage.
- A contact may have multiple cases over time.
- At most one case may be open per contact and flow. Onboarded and closed cases do not count as open.

### 3.2 Stages and lifecycle

**Solution:** Stages and contact lifecycle are separate axes.

- Board columns are `intake_stages`.
- Contact lifecycle is `contacts.lifecycle_status`.
- Each stage may optionally nudge the lifecycle on entry.
- Lifecycle nudges follow legal transitions and do not downgrade active, suspended, offboarding or offboarded contacts.
- Lifecycle changes are recorded in `contact_status_history`.

Stage kinds are fixed so server behaviour is stable:

`enquiry`, `eligibility`, `risk_assessment`, `service_planning`, `custom`, `onboarded`, `cancelled`.

Lifecycle values are stable for code semantics; labels and colours are configurable.

### 3.3 Flow configuration

**Solution:** Each flow owns its ordered stages and policy.

- Flows are configured per subject kind and variant.
- New flows may clone an existing flow or start from a default template.
- Parent-flow grouping is optional.
- Stage labels, colours, order, fields, actions, checklists and assignees are editable.
- Stage kinds remain immutable once cases have entered a stage.

Default templates:

| Subject kind | Variant | Key |
|---|---|---|
| client | standard | `client_standard` |
| client | aged_care | `client_aged_care` |
| client | disability_standard | `client_disability_standard` |
| client | disability_emergency | `client_disability_emergency` |
| client | fee_for_service | `client_fee_for_service` |
| worker | standard | `worker_standard` |
| other_contact | standard | `other_contact_standard` |
| organisation | standard | `organisation_standard` |

Each default flow has six stages: Enquiry, Eligibility / fit check, Risk assessment, Service planning, Onboarded and Cancelled.

Organisations may add, remove, rename or reorder stages and create additional flows for
brokerage, residential, respite, insurance, private or other service models.

### 3.4 Fields and validation

**Solution:** A canonical field catalogue drives configuration and validation.

- Built-in fields live in `crates/server/src/intake/fields.rs`.
- Custom contact fields are addressed as `custom:<key>`.
- Each stage declares `visible_fields` and `required_fields`.
- Required fields must be a subset of visible fields.
- Required fields are validated before a transition.
- Field values are stored in `intake_cases.data`; custom values use `custom_field_values` with entity type `intake_case`.
- Onboarding copies built-in values to contact columns/details and custom values to the contact.
- Organisations can add fields for local identifiers, funding references, language, accessibility or service-specific data.

### 3.5 Checklists

**Solution:** Stage checklist templates materialise onto cases.

- Template items live on the stage.
- Entering a stage creates case checklist items with label, description and required flag snapshots.
- Checklist toggles record user, time and note.
- A stage may require all required items to be complete before transition.
- Checklist completion is advisory until a transition checks the gate.

### 3.6 Assignees and alerts

**Solution:** Assignment is resolved per stage and stored on the case.

- A stage can target a named user, a permission-set pool, or both.
- Pools support `first_available` and `round_robin`.
- The first resolvable rule sets the primary assignee.
- Later rules can notify watchers without changing the primary assignee.
- Manual reassignment requires `intake.assign`.
- Assignment creates an in-app notification unless the recipient is the actor.
- Notifications are deduplicated by case, stage and event.

### 3.7 Stage action policy

**Solution:** A closed action vocabulary is enforced server-side.

Action keys:

`roster.add`, `roster.remove`, `funding.add`, `funding.edit`, `funding.remove`, `profile.edit`, `relationships.update`.

- Each stage stores an allow/deny value for every action.
- Contact, relationship, funding and rostering endpoints call a shared policy helper.
- A blocked action returns a structured conflict with the stage and reason.
- Stage policy can deny an action but can never grant a capability the user lacks.

### 3.8 Board and case detail

**Solution:** The board is single-flow in v1.

- Columns come from the selected flow.
- Cards show name, lifecycle, division, assignee, checklist progress, waitlist state and due date.
- Filters cover division/subdivision, assignee, lifecycle, search and closed cases.
- Columns use keyset paging.
- Case detail is a docked column and supports pop-out.
- Drag a card to transition. Validation blocks are returned as missing fields or incomplete checklist items.
- A keyboard “Move to…” action provides the same transition path.

### 3.9 Contact and onboarding flow

**Solution:** Enquiry creates or reuses a contact and case together.

- Existing contacts are matched by normalised email, phone or name + date of birth.
- A duplicate match returns candidates; a forced duplicate requires a reason.
- The contact starts at lifecycle `enquiry` when new.
- Existing active, suspended or offboarding contacts keep their lifecycle.
- Reaching `onboarded` links the client or worker role, copies case data to the contact and sets `onboarded_at`.
- Reaching `cancelled` closes the case and applies `intake_cancelled` when legal.

### 3.10 Contact lifecycle

**Solution:** Lifecycle is an explicit state machine.

Allowed transitions:

| From | To |
|---|---|
| enquiry | onboarding, active, intake_cancelled |
| onboarding | active, intake_cancelled |
| active | suspended, offboarding, offboarded |
| suspended | active, offboarding, offboarded |
| offboarding | offboarded, active |
| intake_cancelled | enquiry |
| offboarded | enquiry |

Reasons are required for suspended, offboarding, offboarded and intake_cancelled.

### 3.11 Referrals, leads and waitlists

**Solution:** Referral and waitlist data are case attributes, not board columns.

- Source, external reference, referrer contact or organisation, and received time are stored on the case.
- Waitlist entries hold service, division, priority, requested start, status and offer timestamps.
- Waitlist state appears as a card pill and filter.
- Cases remain in their stage while waitlisted.

### 3.12 Admin configuration

**Solution:** Configuration lives in the server admin UI.

- `/admin/intake` manages flows, stages, fields, actions, checklists and assignments.
- Flow creation can clone an existing flow.
- Stage editing covers visible/required fields, actions, checklist items and assignment rules.
- A read-only board is available for visual review.
- All configuration writes are audited.

### 3.13 Data model

**Solution:** Intake uses these tables:

- `intake_flows`, `intake_stages`, `intake_stage_actions`, `intake_stage_assignees`, `intake_stage_checklist_items`.
- `intake_cases`, `intake_case_checklist_items`, `intake_case_events`, `intake_waitlist_entries`, `intake_assignment_cursors`.
- `notifications`, `contact_status_history`.
- `custom_field_values` with entity type `intake_case`.

### 3.14 API

**Solution:** Intake endpoints are grouped under `/api/v1/intake` and contact lifecycle under `/api/v1/contacts/{id}`.

Configuration:

- `GET /intake/field-catalog`
- `GET/POST /intake/flows`
- `PATCH/DELETE /intake/flows/{id}`
- `POST /intake/flows/{id}/stages`
- `PATCH/DELETE /intake/stages/{id}`
- `POST /intake/flows/{id}/stages/reorder`
- `PUT /intake/stages/{id}/actions`
- `PUT /intake/stages/{id}/checklist`
- `PUT /intake/stages/{id}/assignees`
- `GET /intake/assignees`

Cases and board:

- `GET /intake/board`
- `GET /intake/cases`
- `POST /intake/cases`
- `GET/PATCH /intake/cases/{id}`
- `POST /intake/cases/{id}/transition`
- `POST /intake/cases/{id}/checklist/{item_id}`
- `POST /intake/cases/{id}/cancel`
- `POST /intake/cases/{id}/archive`
- `GET /intake/cases/{id}/timeline`
- `GET /intake/contact-matches`

Lifecycle and notifications:

- `PATCH /contacts/{id}/status`
- `GET /contacts/{id}/status-history`
- `GET /notifications`
- `POST /notifications/{id}/read`
- `POST /notifications/read-all`

The full request/response contract is in `CareLibre/docs/intake.md`.

### 3.15 Permissions

**Solution:** Intake adds four capabilities.

| Capability | Grants |
|---|---|
| `intake.read` | Board, cases, flows, timeline and waitlists |
| `intake.write` | Create, edit, transition, cancel and archive cases |
| `intake.assign` | Reassign cases and queues |
| `intake.configure` | Manage flows, stages, fields, actions, checklists and assignees |

Personas:

- Intake: read, write, assign.
- Senior Management and House Manager: read.
- Administrator: all.

Existing contact, relationship, funding and roster capabilities still apply.

### 3.16 Division, jurisdiction and locale

**Solution:** Organisational structure is configuration.

- A case and a flow may carry a division.
- `include_descendants=true` includes child divisions.
- `include_descendants=false` matches exactly.
- Division filtering narrows the board; it is not object-level authorisation.
- Jurisdiction, currency, time zone and language are organisation or division settings.
- Regional identifiers and funding references are ordinary custom fields unless an integration supplies them.

### 3.17 Migration and compatibility

**Solution:** Intake ships as additive migrations.

- Existing contacts default to lifecycle `active`.
- Default flows are seeded for existing and new tenant roots.
- Existing local intake drafts migrate into a matched or created contact plus a case.
- Completed intakes are not recreated.
- `client_request_id` and `legacy_draft_id` make creation idempotent.
- API changes stay additive under `/api/v1`.

### 3.18 Testing

**Solution:** Tests cover schema, API and UI.

Server:

- Migration and seed tests.
- Config validation and permission tests.
- Case creation, duplicate matching, idempotency and open-case rules.
- Transition required fields and checklist gates.
- Onboarding role linking and lifecycle nudges.
- Board filters, paging and division descendants.
- Action policy matrix.
- Assignment and notification dedupe.
- OpenAPI path coverage.

Desktop:

- Board rendering and filters.
- Case column and stage fields.
- Drag/drop success and blocked transitions.
- Notification bell and action disabling.
- Legacy draft migration.
- Headless screenshot snapshots.

---

## 4. Extended requirements for universal use

These requirements extend the core pipeline for organisations with more complex intake
models. Each item is expressed as a requirement and a proposed solution.

| ID | Requirement | Proposed solution | Status |
|---|---|---|---|
| X1 | Intake may originate from an external lead or referral identifier. | Add `external_referral_code` and `crm_lead_id` to cases; show on cards and filters. | Planned |
| X2 | Organisations may need to sync with external portals or CRMs. | Define an integration adapter interface for leads, referrals, funding and documents. | Later |
| X3 | Document requirements vary by service, funding program or jurisdiction. | Add document requirement sets per flow/domain, with required/received/verified states. | Planned |
| X4 | Staff must generate, print, store and retrieve intake documents. | Add document generation, print-friendly output, upload and storage metadata. | Planned |
| X5 | Signatures may be paper, in-person or online. | Support printed and in-person workflows now; add e-sign providers through adapters. | Planned |
| X6 | Coordinators may complete forms on behalf of a person. | Desktop forms support staff-assisted entry; no client account is required. | Delivered for core fields |
| X7 | Rapid or emergency intake may lack identity data. | Add a rapid-intake mode with relaxable required fields, unknown/placeholder values and a completeness indicator. | Planned |
| X8 | Supports may need to start before documents and funding are complete. | Add a roster-readiness checklist and allow a temporary care plan. | Planned |
| X9 | Post-intake work may continue after rostering starts. | Add case tasks with owners, due dates and reminders, separate from stage checklists. | Planned |
| X10 | Brokerage or short-term work may need a compressed flow. | Provide a configurable short-term flow template with shorter stages and minimal gates. | Planned |
| X11 | Private or fee-for-service intake may need only contact and billing details. | Provide a short fee-for-service flow template. | Planned |
| X12 | Some services require a quote before delivery. | Add `quote_required` to cases and link to quotes or agreements. | Planned |
| X13 | Some funding models are budget-only. | Add `funding_mode` with `quote`, `budget` and `contract` values. | Planned |
| X14 | Coordinators may need evidence of available or remaining funding. | Add funding-evidence attachments and status, with configurable advisory copy. | Planned |
| X15 | Open enquiries may need scheduled follow-up. | Add follow-up tasks, due dates, stale filters and assignee reminders. | Planned |
| X16 | Work may be assigned by intake type as well as stage. | Add assignment groups or permission-set queues per intake type. | Partly delivered |
| X17 | A referrer may request a quote without full identity details. | Add a lightweight referral/quote path that creates a case with placeholder identity. | Planned |
| X18 | Documents may live in an external store. | Add external storage references and deep links; keep upload optional. | Planned |
| X19 | Service or accommodation enquiries may duplicate intake fields. | Add configurable enquiry variants or templates that reuse existing fields. | Planned |
| X20 | Some work may be billed before delivery. | Add a finance exception path outside the standard intake and billing cycle. | Later |
| X21 | Long intake calls may be transcribed. | Add consented transcription and form prefill as an optional AI-assisted feature. | Later |
| X22 | Organisations may operate across jurisdictions. | Add locale, currency, time-zone, language and regional identifier settings. | Planned |
| X23 | External referrers may need a portal or API. | Add a scoped referral endpoint or portal adapter. | Later |
| X24 | Intake reporting must support universal operations. | Add volume, source, conversion, stage duration and waitlist reports. | Planned |

---

## 5. Delivery phases

| Phase | Scope | Status |
|---|---|---|
| 0 | ADR, API contract, universal default templates, design decisions | Done |
| 1 | Intake schema, capabilities, field catalogue, default flow seed, config API and admin configuration | Done |
| 2 | Contact matching, case creation, board/list API, transitions, checklists, assignment, notifications, onboarding, lifecycle | Done |
| 3 | Desktop kanban board, case detail column, filters, drag/drop transitions, blocker dialogs | Next |
| 4 | Desktop notification bell/toast, my-cases/unassigned views, assignment UX | Planned |
| 5 | Desktop action-policy disabling, field visibility, checklist UX, configuration links | Planned |
| 6 | Referrals, waitlists, lifecycle polish and reporting | Planned |
| 7 | Extended requirements X1–X24, prioritised by organisation feedback | Backlog |

---

## 6. Current delivery

Delivered on `CareLibre` branch `feature/intake-cases`:

- Intake schema and capabilities.
- Eight seeded flows with six stages each.
- Configuration API and admin UI.
- Read-only admin board.
- Case creation and contact matching.
- Board and case list APIs.
- Transitions with required-field and checklist gates.
- Assignment resolution and in-app notifications.
- Onboarding role linking and contact lifecycle.
- Referral and waitlist data structures.
- Schema, API and HTTP smoke tests.

Next:

1. Build the desktop board and case detail.
2. Add the notification bell and assignment filters.
3. Apply action policy in the contact editor, funding and rostering pages.
4. Add referral and waitlist UI.
5. Prioritise the extended requirements in Section 4 for the provider community.
