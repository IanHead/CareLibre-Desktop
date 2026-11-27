# Intake, referrals, waitlists and enquiries — implementation plan

**Status:** decisions agreed; ADR and API contract drafted for review

**Scope:** CareLibre Server + CareLibre Desktop

**Related:** server `docs/roadmap.md`, `docs/permissions.md`, `docs/intake.md`, `docs/adr/0013-intake-cases.md`

**Desktop entry points:** `src/pages/intake/`, `src/intake.rs`, `src/store/intake.rs`, `src/remote.rs`, `src/app.rs`

---

## 1. Summary

The brief describes three related things that are currently conflated in one checklist:

1. A **pipeline** of configurable intake stages, shown as a kanban board.
2. A **contact lifecycle status** (Enquiry, Onboarding, Active, Suspended, Offboarding,
   Cancelled, Offboarded).
3. **Per-stage policy**: who owns the stage, which fields are visible, which checklist
   items must be done, and which actions are allowed while the contact is in that stage.

This plan separates those into a first-class **intake case** and a **lifecycle status** on
the contact, with the contact created at enquiry so repeated attempts reuse the same person
record. It keeps the server as the source of truth so referrals and enquiries entered by
one coordinator are visible to everyone. The desktop renders the board and card detail;
configuration lives in the server admin UI.

The work is split into seven vertical slices. The critical path is:

```
server schema + flows/stages + permissions
        ↓
server cases + transitions + board API
        ↓
desktop board + case detail + filters
        ↓
assignees + notifications
        ↓
checklists + field/action policy + admin configuration
        ↓
referrals + waitlist + lifecycle management
        ↓
migration, polish, reporting
```

---

## 2. What exists today

### Desktop

- `src/pages/intake/` is a three-scenario wizard (client, worker, other contact) with
  hard-coded steps in `fields.rs`.
- Drafts are saved to local SQLite (`intakes` table) and only become server records on
  completion, when `POST /api/v1/contacts` is called. The contact is then also written to
  the local cache.
- The picker lists local drafts. There is no server-side list of enquiries or cases.
- Contact status is not a server column today. `src/contact.rs::row_from_api` derives a
  display status from `details.contact_status`, falling back to `Active`, and from
  `is_active`.
- Columns are already a generic workspace concept: `ColumnKind::Page`, `Contact`,
  `Organisation` in `src/app.rs`. Adding an intake-case column fits that model.
- Background API calls already use a request-id command/event pattern in `src/remote.rs`.

### Server

- Contacts are deliberately untyped. Client/worker identity comes from the
  `contact_care_roles` view over two-sided relationships.
- `contacts.details` is JSONB used for intake profile data. There is no lifecycle column.
- Divisions are hierarchical via `divisions.parent_id`; a subdivision is a child division.
- Permissions are capability-based (`docs/permissions.md`), with an Intake persona already
  present but no intake-specific capabilities.
- A generic `audit_log` exists. There is no notification table or mail transport.
- The latest migration is `0026_organisation_child_name_unique.sql`.
- The server roadmap has "Intake, referrals, waitlists, enquiries" unchecked; only the
  guided wizards are checked.

---

## 3. Core design decisions

### 3.1 An intake case plus a contact, created together at enquiry

An enquiry or referral must appear on a shared board, and the person must be findable before
the intake is complete. The board card is an **intake case**, and the **contact record is
created at the same time as the case**, at enquiry.

- `intake_cases` holds the pipeline state, source, division, assignee and stage data.
- For client, worker and other-contact flows, `intake_cases.contact_id` is required and is
  written in the same transaction as the case.
- Organisation intake links an `external_organisation_id` instead of a contact.
- **One person, many attempts.** A contact can have more than one case over its life
  (re-intake, a different flow, a new service). Reuse the existing contact rather than
  creating a duplicate; enforce at most one open case per contact per flow so a second
  attempt is explicit rather than accidental.
- **Duplicate matching.** Before creating a contact, match on normalised email, phone, or
  name + date of birth. The API can return `409 contact_match` with candidates. The desktop
  offers "link this contact" or "create a new contact anyway"; a forced create is recorded
  with the reason.
- **Role linking stays with onboarding.** The contact record exists at enquiry, but the
  care/employment relationship is added when the case reaches the configured role-link
  stage (default: `onboarded`). This keeps a cancelled enquiry from permanently classifying
  someone as a client or employee. Until then the contact lifecycle is `enquiry`, the case
  carries `subject_kind`, and the contact is found in the directory under All or by
  lifecycle status rather than under Clients/Workers. If the organisation wants
  enquiry-stage contacts in those role lists, a dedicated `prospect` relationship is the
  clean follow-up; do not fake a client/employee relationship.

This keeps the board and the directory consistent: the person exists once, while each
attempt at intake is a separate auditable case. It also avoids overloading
`contacts.details` with pipeline state and makes referral and waitlist data queryable.

### 3.2 Two status axes, not one

The brief lists both "possible contact statuses" and "intake status columns". They are
related but not the same. Treat them as two axes:

| Axis | Where it lives | What it means | Who changes it |
|---|---|---|---|
| **Intake stage** | `intake_cases.stage_id` | Where the case is in a flow | Transition endpoint / board drag |
| **Contact lifecycle status** | `contacts.lifecycle_status` | Whether the person is an enquiry, onboarding, active, suspended, etc. | Explicit contact actions, with optional stage-driven nudges |

**Board columns are intake stages.** The lifecycle status is shown as a coloured pill on
each card and is available as a filter. It is not a board column, and it may change later
independently of intake. The stage mapping below is a convenience only: a user with the
lifecycle capability can always set the contact status directly, and a later service
intake does not move an active client back to `enquiry`.

Suggested stage → lifecycle nudge:

| Stage kind | Default lifecycle on enter |
|---|---|
| `enquiry` | `enquiry` |
| `eligibility` | `onboarding` |
| `risk_assessment` | `onboarding` |
| `service_planning` | `onboarding` |
| `custom` | unchanged |
| `onboarded` | `active` |
| `cancelled` | `intake_cancelled` |

`Suspended`, `Offboarding` and `Offboarded` are post-intake states managed from the
contact record, with their own endpoint and history. They are not intake stages. If a
"lifecycle board" is wanted later, it belongs on the Contacts page, not the intake board.

Lifecycle rules when a case is created:

- New contact: `enquiry`.
- Existing contact: keep the current status. An `active` client starting a second intake
  stays `active`; the new case still appears at the Enquiry stage on the board.
- Existing contact in `intake_cancelled` or `offboarded`: a new case moves them back to
  `enquiry`.
- Stage nudges never overwrite `suspended`, `offboarding` or `offboarded`, and a cancelled
  stage never downgrades an `active` contact. Only an explicit lifecycle action can leave
  those states.
- `onboarded` may move `enquiry`/`onboarding` to `active`; `cancelled` may move
  `enquiry`/`onboarding` to `intake_cancelled`. Everything else is skipped and logged.

### 3.3 Configurable definitions, fixed vocabulary

Administrators can rename, reorder, colour, add and remove stages per flow. The code still
needs stable semantics, so each stage also has a fixed `stage_kind`:

`enquiry`, `eligibility`, `risk_assessment`, `service_planning`, `custom`, `onboarded`,
`cancelled`.

Only `onboarded` and `cancelled` are terminal by default. `custom` lets an organisation add
a column such as "Funding check" without a server release. Contact lifecycle values are
fixed in v1 because code branches on them; labels and colours use server defaults and can
be made configurable later. The set is decoupled from intake, so it can be extended later
(a small migration) without redesigning any flow or board.

### 3.4 Per-flow configuration, cloned not inherited

A **flow** is a complete pipeline for one subject kind and variant:

- client / standard
- client / aged care
- client / disability standard
- client / disability emergency
- client / fee for service
- worker / standard
- other contact / standard
- organisation / standard

Sub-flows are independent stage pipelines grouped under a parent flow for the picker. When
a sub-flow is created it is cloned from a template or another flow. Later edits to the
parent do not silently change the child; the admin UI can offer "apply these stages to
sub-flows" as an explicit action. This is predictable and avoids hidden coupling. A shared
live stage library can replace this later if duplication becomes painful.

### 3.5 Server is the source of truth; local drafts are a buffer

Cases are created on the server at enquiry time. The desktop may keep a local field buffer
for offline/unsaved edits, but the board and ownership are server-backed. This is required
for shared boards, assignee alerts and referrals coming from other users.

---

## 4. Domain model

```
intake_flows 1 ─── * intake_stages 1 ─── * intake_stage_actions
     │                    │            ├── * intake_stage_assignees
     │                    │            └── * intake_stage_checklist_items
     │
     └── * intake_cases 1 ─── * intake_case_checklist_items
                  │      ├── * intake_case_events
                  │      └── * intake_waitlist_entries
                  │
                  └── contacts (1 for person flows) ─── * contact_status_history

intake_stages 1 ─── * notifications (via transition/assignment)
```

### Flow

- Subject kind: `client`, `worker`, `other_contact`, `organisation`.
- Variant: free text with suggested values (`standard`, `aged_care`, `disability`,
  `emergency`, `fee_for_service`); used for grouping and labels.
- Optional division scope so only relevant flows appear for a division.
- `parent_flow_id` for grouping sub-flows.
- `is_default` for the picker's suggested flow.
- `is_active` for archiving without deleting history.

### Stage

- Ordered within a flow.
- `key` (stable machine key), `name`, `color`, `stage_kind`, `is_terminal`.
- `on_enter_contact_status` (nullable; one of the lifecycle values).
- `visible_fields` and `required_fields` as JSON arrays of field keys.
- `sla_hours` for optional due dates.
- `checklist_gate`: leaving the stage requires all required checklist items.
- Actions are stored in `intake_stage_actions`, one row per action key with an `allowed`
  flag.

### Case

- Flow, current stage, contact (required for client/worker/other-contact flows), subject
  kind. Organisation flows link `external_organisation_id` instead.
- The contact is created or matched and linked in the same transaction as the case. A
  contact may accumulate multiple cases over time; at most one open case per contact per
  flow. Contacts with cases cannot be hard-deleted (`ON DELETE RESTRICT`); archive or
  deactivate them instead so intake history remains intact.
- `display_name` denormalised for fast board rendering.
- `data` JSONB for built-in intake fields; custom values in the existing
  `custom_field_values` table with `entity_type = 'intake_case'`.
- `source`, `referrer_contact_id`, `referrer_organisation_id`, `referral_reference`,
  `referral_received_at`.
- `division_id` (leaf division; subdivision is a child division).
- `assignee_user_id`, `priority`, `stage_entered_at`, `stage_due_at`.
- `version` for optimistic concurrency.
- `created_by`, `created_at`, `updated_at`, `onboarded_at`, `closed_at`,
  `cancellation_reason`.
- `onboarded_at` marks successful completion; the case remains visible in the Onboarded
  column until archived. `closed_at` marks cancellation or archival only, so a new case for
  the same contact and flow can start after onboarding.
- `legacy_draft_id` for migration from the current local drafts.

### Checklist

- Template items live on the stage (`intake_stage_checklist_items`).
- On entering a stage, items are materialised into `intake_case_checklist_items` with a
  snapshot of the label and required flag. Template edits do not rewrite history.
- Completion records user, time and optional note.
- Required incomplete items block transition when `checklist_gate` is on.

### Assignees

Both assignment models are supported and configured per stage in the server admin panel:

- **Named user** — a specific person owns the stage, e.g. risk assessment goes to Alex.
- **Queue / pool** — a permission set (or explicit group) owns the stage; a member is
  chosen with `first_available` or `round_robin`. The admin picks the strategy.
- **Both** — a named user can be the primary assignee while a pool is notified as backup
  or watchers. Resolution order follows the stage's assignee rows by `sort_order`: the
  first resolvable rule sets `assignee_user_id`; additional rules with `notify` receive the
  alert.
- The case creator and division-manager strategies are optional later additions.

Assignment resolves to `intake_cases.assignee_user_id` when the case enters the stage, so
the board and alerts do not need to re-evaluate rules on every render. Manual reassignment
is allowed with `intake.assign`.

The admin UI exposes this as a single "Who handles this stage?" panel with named users,
permission-set pools, strategy and notification toggles.

### Notifications

- In-app notifications first: `notifications(user_id, kind, title, body, entity_type,
  entity_id, read_at, created_at, dedupe_key)`.
- Notify when a case enters a stage and the resolved assignee is not the actor.
- Also notify on manual assignment.
- Do not notify the actor about their own action.
- Deduplicate by case + stage + transition event so retries do not spam.
- Email/SMS is phase 5, once the Communications workspace exists. The notification row can
  later feed an outbox.

### Waitlist

- A case can be waitlisted while still progressing through eligibility, risk and planning.
  Waitlist is an attribute, not a stage, so it does not create a dead-end column.
- `intake_waitlist_entries`: case, service (optional), division, priority, requested start,
  status (`waiting`, `offered`, `placed`, `withdrawn`), offered/expires timestamps, notes.
- The board shows a "Waitlisted" pill and supports a waitlist filter. A dedicated waitlist
  view can be added once the board ships.

### Referrals

- A referral creates a case at the `enquiry` stage with source and referrer fields.
- The referrer may be an existing contact or organisation, or free text for an external
  referrer not yet on file.
- Later, referral documents/attachments can hang off the case.

---

## 5. Server data model (migration sketch)

Create `migrations/0027_intake.sql` and `migrations/0028_intake_notifications.sql` (or one
migration if the review prefers a single slice). Follow existing conventions: text + CHECK
instead of enums, org-scoped FKs, `set_updated_at()` triggers, comments on tables.

### Contact lifecycle

```sql
ALTER TABLE contacts
    ADD COLUMN lifecycle_status text NOT NULL DEFAULT 'active'
        CHECK (lifecycle_status IN (
            'enquiry', 'onboarding', 'suspended', 'active',
            'offboarding', 'intake_cancelled', 'offboarded'
        )),
    ADD COLUMN lifecycle_changed_at timestamptz NOT NULL DEFAULT now();

CREATE TABLE contact_status_history (
    id               uuid PRIMARY KEY,
    organisation_id  uuid NOT NULL REFERENCES organisations (id) ON DELETE CASCADE,
    contact_id       uuid NOT NULL,
    from_status      text CHECK (from_status IS NULL OR from_status IN (
                         'enquiry', 'onboarding', 'suspended', 'active',
                         'offboarding', 'intake_cancelled', 'offboarded')),
    to_status        text NOT NULL CHECK (to_status IN (
                         'enquiry', 'onboarding', 'suspended', 'active',
                         'offboarding', 'intake_cancelled', 'offboarded')),
    reason           text,
    actor_user_id    uuid REFERENCES users (id) ON DELETE SET NULL,
    created_at       timestamptz NOT NULL DEFAULT now(),
    FOREIGN KEY (organisation_id, contact_id)
        REFERENCES contacts (organisation_id, id) ON DELETE CASCADE
);
```

All existing contacts default to `active`. Add indexes for duplicate matching:

```sql
CREATE INDEX contacts_org_email_lower_idx
    ON contacts (organisation_id, lower(email))
    WHERE email IS NOT NULL;
CREATE INDEX contacts_org_phone_digits_idx
    ON contacts (organisation_id, regexp_replace(phone, '[^0-9]', '', 'g'))
    WHERE phone IS NOT NULL;
```

### Flows and stages

```sql
CREATE TABLE intake_flows (
    id                uuid PRIMARY KEY,
    organisation_id   uuid NOT NULL REFERENCES organisations (id) ON DELETE CASCADE,
    parent_flow_id    uuid,
    key               text NOT NULL,
    name              text NOT NULL,
    subject_kind      text NOT NULL CHECK (subject_kind IN (
                          'client', 'worker', 'other_contact', 'organisation')),
    variant           text,
    description       text,
    division_id       uuid,
    is_default        boolean NOT NULL DEFAULT false,
    is_active         boolean NOT NULL DEFAULT true,
    sort_order        integer NOT NULL DEFAULT 0,
    created_at        timestamptz NOT NULL DEFAULT now(),
    updated_at        timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT intake_flows_id_org_key UNIQUE (organisation_id, id),
    CONSTRAINT intake_flows_key_key UNIQUE (organisation_id, key),
    CONSTRAINT intake_flows_parent_fk
        FOREIGN KEY (organisation_id, parent_flow_id)
        REFERENCES intake_flows (organisation_id, id),
    CONSTRAINT intake_flows_division_fk
        FOREIGN KEY (organisation_id, division_id)
        REFERENCES divisions (organisation_id, id)
);

CREATE UNIQUE INDEX intake_flows_one_default
    ON intake_flows (organisation_id, subject_kind, COALESCE(variant, ''))
    WHERE is_default AND is_active;

CREATE TABLE intake_stages (
    id                      uuid PRIMARY KEY,
    organisation_id         uuid NOT NULL REFERENCES organisations (id) ON DELETE CASCADE,
    flow_id                 uuid NOT NULL,
    key                     text NOT NULL,
    name                    text NOT NULL,
    stage_kind              text NOT NULL CHECK (stage_kind IN (
                                'enquiry', 'eligibility', 'risk_assessment',
                                'service_planning', 'custom', 'onboarded', 'cancelled')),
    color                   text,
    sort_order              integer NOT NULL DEFAULT 0,
    is_terminal             boolean NOT NULL DEFAULT false,
    on_enter_contact_status text CHECK (on_enter_contact_status IS NULL OR
                                on_enter_contact_status IN (
                                    'enquiry', 'onboarding', 'active',
                                    'intake_cancelled')),
    sla_hours               integer CHECK (sla_hours IS NULL OR sla_hours > 0),
    checklist_gate          boolean NOT NULL DEFAULT false,
    visible_fields          jsonb NOT NULL DEFAULT '[]'::jsonb,
    required_fields         jsonb NOT NULL DEFAULT '[]'::jsonb,
    created_at              timestamptz NOT NULL DEFAULT now(),
    updated_at              timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT intake_stages_id_org_key UNIQUE (organisation_id, id),
    CONSTRAINT intake_stages_flow_key_key UNIQUE (organisation_id, flow_id, key),
    CONSTRAINT intake_stages_terminal_kind
        CHECK (is_terminal = (stage_kind IN ('onboarded', 'cancelled'))),
    CONSTRAINT intake_stages_flow_fk
        FOREIGN KEY (organisation_id, flow_id)
        REFERENCES intake_flows (organisation_id, id) ON DELETE CASCADE
);
```

`visible_fields`/`required_fields` are validated in the API against the field catalogue, not
by the database.

### Stage policy

```sql
CREATE TABLE intake_stage_actions (
    organisation_id  uuid NOT NULL,
    stage_id         uuid NOT NULL,
    action_key       text NOT NULL CHECK (action_key IN (
                         'roster.add', 'roster.remove',
                         'funding.add', 'funding.edit', 'funding.remove',
                         'profile.edit', 'relationships.update')),
    allowed          boolean NOT NULL DEFAULT false,
    PRIMARY KEY (organisation_id, stage_id, action_key),
    FOREIGN KEY (organisation_id, stage_id)
        REFERENCES intake_stages (organisation_id, id) ON DELETE CASCADE
);

CREATE TABLE intake_stage_assignees (
    id                 uuid PRIMARY KEY,
    organisation_id    uuid NOT NULL,
    stage_id           uuid NOT NULL,
    user_id            uuid REFERENCES users (id) ON DELETE CASCADE,
    permission_set_id  uuid,
    strategy           text NOT NULL DEFAULT 'fixed'
                       CHECK (strategy IN ('fixed', 'round_robin', 'first_available')),
    notify             boolean NOT NULL DEFAULT true,
    sort_order         integer NOT NULL DEFAULT 0,
    CONSTRAINT intake_stage_assignees_target
        CHECK (num_nonnulls(user_id, permission_set_id) = 1),
    FOREIGN KEY (organisation_id, stage_id)
        REFERENCES intake_stages (organisation_id, id) ON DELETE CASCADE,
    FOREIGN KEY (organisation_id, permission_set_id)
        REFERENCES permission_sets (organisation_id, id) ON DELETE CASCADE
);
```

### Cases

```sql
CREATE TABLE intake_cases (
    id                        uuid PRIMARY KEY,
    organisation_id           uuid NOT NULL REFERENCES organisations (id) ON DELETE CASCADE,
    flow_id                   uuid NOT NULL,
    stage_id                  uuid NOT NULL,
    -- Required for person flows; organisation flows use external_organisation_id.
    contact_id                uuid,
    external_organisation_id  uuid,
    subject_kind              text NOT NULL CHECK (subject_kind IN (
                                  'client', 'worker', 'other_contact', 'organisation')),
    display_name              text NOT NULL CHECK (char_length(display_name) BETWEEN 1 AND 200),
    data                      jsonb NOT NULL DEFAULT '{}'::jsonb,
    source                    text NOT NULL DEFAULT 'enquiry'
                              CHECK (source IN ('enquiry', 'referral', 'internal', 'waitlist')),
    referrer_contact_id       uuid,
    referrer_organisation_id  uuid,
    referral_reference        text,
    referral_received_at      timestamptz,
    division_id               uuid,
    assignee_user_id          uuid REFERENCES users (id) ON DELETE SET NULL,
    priority                  smallint NOT NULL DEFAULT 0,
    stage_entered_at          timestamptz NOT NULL DEFAULT now(),
    stage_due_at              timestamptz,
    version                   integer NOT NULL DEFAULT 1,
    created_by                uuid REFERENCES users (id) ON DELETE SET NULL,
    created_at                timestamptz NOT NULL DEFAULT now(),
    updated_at                timestamptz NOT NULL DEFAULT now(),
    onboarded_at              timestamptz,
    closed_at                 timestamptz,
    cancellation_reason       text,
    client_request_id         text,
    legacy_draft_id           text,
    CONSTRAINT intake_cases_id_org_key UNIQUE (organisation_id, id),
    CONSTRAINT intake_cases_flow_fk
        FOREIGN KEY (organisation_id, flow_id)
        REFERENCES intake_flows (organisation_id, id),
    CONSTRAINT intake_cases_stage_fk
        FOREIGN KEY (organisation_id, stage_id)
        REFERENCES intake_stages (organisation_id, id),
    CONSTRAINT intake_cases_contact_fk
        FOREIGN KEY (organisation_id, contact_id)
        REFERENCES contacts (organisation_id, id) ON DELETE RESTRICT,
    CONSTRAINT intake_cases_division_fk
        FOREIGN KEY (organisation_id, division_id)
        REFERENCES divisions (organisation_id, id),
    CONSTRAINT intake_cases_external_org_fk
        FOREIGN KEY (organisation_id, external_organisation_id)
        REFERENCES organisations (tenant_id, id) ON DELETE RESTRICT,
    CONSTRAINT intake_cases_referrer_contact_fk
        FOREIGN KEY (organisation_id, referrer_contact_id)
        REFERENCES contacts (organisation_id, id) ON DELETE SET NULL,
    CONSTRAINT intake_cases_referrer_org_fk
        FOREIGN KEY (organisation_id, referrer_organisation_id)
        REFERENCES organisations (tenant_id, id) ON DELETE SET NULL,
    CONSTRAINT intake_cases_referrers
        CHECK (num_nonnulls(referrer_contact_id, referrer_organisation_id) <= 1),
    CONSTRAINT intake_cases_subject_link
        CHECK (
            (subject_kind = 'organisation'
                AND contact_id IS NULL AND external_organisation_id IS NOT NULL)
            OR (subject_kind <> 'organisation'
                AND contact_id IS NOT NULL AND external_organisation_id IS NULL)
        )
);

CREATE INDEX intake_cases_board_idx
    ON intake_cases (organisation_id, flow_id, stage_id, priority DESC, stage_entered_at, id);
CREATE INDEX intake_cases_assignee_idx
    ON intake_cases (organisation_id, assignee_user_id, updated_at DESC)
    WHERE closed_at IS NULL;
CREATE INDEX intake_cases_division_idx
    ON intake_cases (organisation_id, division_id)
    WHERE closed_at IS NULL;
CREATE INDEX intake_cases_org_name_idx
    ON intake_cases (organisation_id, lower(display_name));
CREATE UNIQUE INDEX intake_cases_one_open_per_flow
    ON intake_cases (organisation_id, contact_id, flow_id)
    WHERE contact_id IS NOT NULL AND closed_at IS NULL AND onboarded_at IS NULL;
CREATE UNIQUE INDEX intake_cases_client_request_key
    ON intake_cases (organisation_id, client_request_id)
    WHERE client_request_id IS NOT NULL;
CREATE UNIQUE INDEX intake_cases_legacy_draft_key
    ON intake_cases (organisation_id, legacy_draft_id)
    WHERE legacy_draft_id IS NOT NULL;
```

Register `intake_case` in `custom_field_entities` so custom fields work with the existing
validation trigger.

### Checklist, events, waitlist, notifications

```sql
CREATE TABLE intake_stage_checklist_items (
    id               uuid PRIMARY KEY,
    organisation_id  uuid NOT NULL,
    stage_id         uuid NOT NULL,
    label            text NOT NULL,
    description      text,
    is_required      boolean NOT NULL DEFAULT true,
    sort_order       integer NOT NULL DEFAULT 0,
    FOREIGN KEY (organisation_id, stage_id)
        REFERENCES intake_stages (organisation_id, id) ON DELETE CASCADE
);

CREATE TABLE intake_case_checklist_items (
    id               uuid PRIMARY KEY,
    organisation_id  uuid NOT NULL,
    case_id          uuid NOT NULL,
    stage_id         uuid NOT NULL,
    template_item_id uuid,
    label            text NOT NULL,
    is_required      boolean NOT NULL DEFAULT true,
    completed_at     timestamptz,
    completed_by     uuid REFERENCES users (id) ON DELETE SET NULL,
    note             text,
    sort_order       integer NOT NULL DEFAULT 0,
    FOREIGN KEY (organisation_id, case_id)
        REFERENCES intake_cases (organisation_id, id) ON DELETE CASCADE,
    FOREIGN KEY (organisation_id, stage_id)
        REFERENCES intake_stages (organisation_id, id) ON DELETE RESTRICT
);

CREATE TABLE intake_case_events (
    id               bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    organisation_id  uuid NOT NULL,
    case_id          uuid NOT NULL,
    event_type       text NOT NULL, -- created, updated, transitioned, assigned,
                                    -- checklist_completed, checklist_reopened, waitlisted,
                                    -- cancelled, onboarded, archived, reopened, action_denied
    actor_user_id    uuid REFERENCES users (id) ON DELETE SET NULL,
    from_stage_id    uuid,
    to_stage_id      uuid,
    metadata         jsonb NOT NULL DEFAULT '{}'::jsonb,
    created_at       timestamptz NOT NULL DEFAULT now(),
    FOREIGN KEY (organisation_id, case_id)
        REFERENCES intake_cases (organisation_id, id) ON DELETE CASCADE
);

CREATE TABLE intake_waitlist_entries (
    id               uuid PRIMARY KEY,
    organisation_id  uuid NOT NULL,
    case_id          uuid NOT NULL,
    service_id       uuid,
    division_id      uuid,
    priority         smallint NOT NULL DEFAULT 0,
    requested_start  date,
    status           text NOT NULL DEFAULT 'waiting'
                     CHECK (status IN ('waiting', 'offered', 'placed', 'withdrawn')),
    offered_at       timestamptz,
    offer_expires_at timestamptz,
    notes            text,
    created_at       timestamptz NOT NULL DEFAULT now(),
    updated_at       timestamptz NOT NULL DEFAULT now(),
    FOREIGN KEY (organisation_id, case_id)
        REFERENCES intake_cases (organisation_id, id) ON DELETE CASCADE,
    FOREIGN KEY (organisation_id, service_id)
        REFERENCES services (organisation_id, id) ON DELETE SET NULL,
    FOREIGN KEY (organisation_id, division_id)
        REFERENCES divisions (organisation_id, id) ON DELETE SET NULL
);

CREATE TABLE notifications (
    id               bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    organisation_id  uuid NOT NULL REFERENCES organisations (id) ON DELETE CASCADE,
    user_id          uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    kind             text NOT NULL,
    title            text NOT NULL,
    body             text,
    entity_type      text,
    entity_id        uuid,
    dedupe_key       text,
    read_at          timestamptz,
    created_at       timestamptz NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX notifications_dedupe_key
    ON notifications (organisation_id, user_id, dedupe_key)
    WHERE dedupe_key IS NOT NULL;
CREATE INDEX notifications_unread_idx
    ON notifications (organisation_id, user_id, created_at DESC)
    WHERE read_at IS NULL;
```

### Field catalogue

No new table is needed for built-in fields. Add a server-side canonical catalogue (Rust
constants) and expose it merged with `custom_field_definitions`:

- Built-in keys stay exactly as they are today (`first_name`, `last_name`, `email`,
  `phone`, `division`, etc.) so existing drafts map cleanly.
- Custom fields are addressed as `custom:<key>`.
- `GET /api/v1/intake/field-catalog` returns built-in and custom fields with labels, data
  types, options and sections.
- A desktop test asserts that every key used by `src/pages/intake/fields.rs` exists in the
  server catalogue while the two definitions coexist during migration.

---

## 6. API design

The authoritative wire contract is `CareLibre/docs/intake.md`. This section is the desktop
summary; if the two differ, the server contract wins.

All routes are under `/api/v1/intake` unless noted.

### Configuration

| Method | Route | Purpose |
|---|---|---|
| `GET` | `/intake/flows` | Flows with stages, fields, actions, assignees, checklist |
| `POST` | `/intake/flows` | Create a flow (optionally clone from another) |
| `PATCH` | `/intake/flows/{id}` | Update name, variant, parent, division, active |
| `DELETE` | `/intake/flows/{id}` | Archive a flow (never hard-delete with cases) |
| `POST` | `/intake/flows/{id}/stages` | Add a stage |
| `PATCH` | `/intake/stages/{id}` | Update stage labels, fields, terminal, mapping |
| `DELETE` | `/intake/stages/{id}` | Remove an unused stage |
| `POST` | `/intake/flows/{id}/stages/reorder` | Persist column order |
| `PUT` | `/intake/stages/{id}/actions` | Replace the allowed-action set |
| `PUT` | `/intake/stages/{id}/checklist` | Replace checklist template items |
| `PUT` | `/intake/stages/{id}/assignees` | Replace assignment rules |
| `GET` | `/intake/field-catalog` | Built-in + custom field definitions |

### Board and cases

| Method | Route | Purpose |
|---|---|---|
| `GET` | `/intake/board` | Columns, counts and first-page cards for the selected flow/filters |
| `GET` | `/intake/cases` | Per-column paging with keyset cursors |
| `POST` | `/intake/cases` | Create a contact (or link an existing one) and an enquiry/referral case |
| `GET` | `/intake/contact-matches` | Find existing contacts by email, phone or name + DOB before creating |
| `GET` | `/intake/cases/{id}` | Full card: fields, checklist, timeline, policy, contact |
| `PATCH` | `/intake/cases/{id}` | Update data, assignee, division, priority; version check |
| `POST` | `/intake/cases/{id}/transition` | Move to another stage; returns blockers on failure |
| `POST` | `/intake/cases/{id}/checklist/{item_id}` | Toggle a checklist item |
| `POST` | `/intake/cases/{id}/waitlist` | Create a waitlist entry |
| `POST` | `/intake/cases/{id}/cancel` | Cancel with reason (or use transition) |
| `POST` | `/intake/cases/{id}/archive` | Hide a completed/cancelled card |
| `POST` | `/intake/cases/{id}/reopen` | Reopen a closed/onboarded case (admin) |
| `GET` | `/intake/cases/{id}/timeline` | Paged case events |
| `PATCH` | `/intake/waitlist/{id}` | Update priority/status/offer |
| `DELETE` | `/intake/waitlist/{id}` | Withdraw a waitlist entry |
| `GET` | `/intake/assignees` | Assignable users and permission-set pools |
| `GET` | `/intake/waitlist` | Waitlist view with division/service filters |

### Contact lifecycle

| Method | Route | Purpose |
|---|---|---|
| `PATCH` | `/contacts/{id}/status` | Change lifecycle status with reason |
| `GET` | `/contacts/{id}/status-history` | Lifecycle audit trail |

### Notifications

| Method | Route | Purpose |
|---|---|---|
| `GET` | `/notifications?unread=true` | In-app notification list |
| `POST` | `/notifications/{id}/read` | Mark one read |
| `POST` | `/notifications/read-all` | Mark all read |

### Board query

```
GET /intake/board
    ?flow_id=<uuid>             required; one flow per board
    &division_id=<uuid>
    &include_descendants=true
    &assignee_user_id=<uuid>|me|unassigned
    &lifecycle_status=suspended,active
    &search=<text>
    &include_closed=false
    &limit_per_stage=30
```

Response:

```json
{
  "flow": { "id": "…", "name": "Disability — standard", "subject_kind": "client" },
  "columns": [
    {
      "stage": {
        "id": "…", "key": "risk_assessment", "name": "Risk assessment",
        "kind": "risk_assessment", "color": "amber"
      },
      "count": 12,
      "cards": [CaseSummary, "…"],
      "next_cursor": "opaque-or-null"
    }
  ],
  "generated_at": "2026-11-27T05:00:00Z"
}
```

`CaseSummary` fields are defined in the server contract
(`CareLibre/docs/intake.md`). Per-column paging is important: a mature provider may have
hundreds of enquiries. Return `limit_per_stage` cards plus a cursor per stage; the desktop
loads more with `GET /intake/cases?stage_id=…&cursor=…` when a column is scrolled.

Per-column paging is important: a mature provider may have hundreds of enquiries. Return
`limit_per_stage` plus a cursor per stage; the desktop loads more when a column is scrolled.

### Case creation contract

`POST /intake/cases` creates the contact and the case together, or links an existing
contact.

```json
{
  "client_request_id": "01J9Z8K6T7V0M2Q4R6S8T0W2X4",
  "flow_id": "…",
  "contact_id": null,
  "contact": {
    "given_name": "Mia", "family_name": "Chen",
    "email": "mia@example.com", "phone": "0400 000 000",
    "date_of_birth": "1990-04-12"
  },
  "display_name": "Mia Chen",
  "source": "referral",
  "referrer_contact_id": null,
  "referrer_organisation_id": "…",
  "division_id": "…",
  "data": { "referral_reference": "…" },
  "allow_duplicate_contact": false,
  "duplicate_reason": null
}
```

- `client_request_id` makes retries idempotent; the same id returns the existing case.
- If `contact_id` is present, link it; otherwise create a contact from `contact`.
- Before creating, match normalised email, phone and name + DOB. If candidates exist and
  `allow_duplicate_contact` is false, return the standard error envelope with
  `details.reason = "contact_match"` and candidates.
- `allow_duplicate_contact=true` requires a non-empty `duplicate_reason` and is recorded in
  the case timeline.
- If the contact already has an open case for this flow, return `409` with
  `details.reason = "open_case_exists"` and the open case id so the desktop can open it
  instead of creating a duplicate.
- Success returns `{ "case": { … }, "contact": { … } }` and records a `created` event.
- The whole operation is one transaction: contact create/match, case, first-stage checklist
  materialisation, assignment resolution and creation event. The care/employment role
  relationship is deliberately not created here; it is linked by the role-link stage.

### Transition contract

```json
POST /intake/cases/{id}/transition
{ "to_stage_id": "…", "version": 4, "reason": null, "override": false }
```

Success returns the updated case and event. Failure uses the standard error envelope:

- `409` — stale `version`; `details.reason = "version_conflict"` and the current case.
- `422` — blockers; `details.reason = "intake_transition_blocked"` with
  `missing_fields`, `incomplete_checklist` and `invalid_transition`.
- `403` — missing `intake.write` or `intake.configure` capability.

Stage action policy does not apply to transitions themselves; it applies to contact,
relationship, funding and roster actions. Those refusals use `409` with
`details.reason = "intake_action_blocked"`.

`override` is only accepted for users with `intake.configure` (or a dedicated
`intake.override` capability if review prefers) and is recorded in the case timeline with a
reason.

---

## 7. Permissions

Add capabilities to the catalogue and permission sets:

| Capability | Meaning |
|---|---|
| `intake.read` | View flows, board, cases and waitlists |
| `intake.write` | Create/edit cases and transition them |
| `intake.assign` | Reassign a case or another user's queue |
| `intake.configure` | Manage flows, stages, fields, actions, checklists and assignees |

Suggested persona changes:

- **Intake**: `intake.read`, `intake.write`, `intake.assign`
- **Senior Management**, **House Manager**: `intake.read`
- **Rostering**, **HR**, **Finance**: no intake access by default; they see only the
  action locks on contacts they already work with
- **Administrator**: all, immutable

Every endpoint checks the capability server-side. The desktop uses the same rules to hide
or disable controls, but the server is authoritative.

Division scoping is a **filter, not authorisation** in this phase. The existing permission
model has no object-level scope; documenting that now avoids implying a guarantee that does
not exist. Add `scope_division_id` to permission assignments in a later phase if needed.

---

## 8. Assignments and alerts

### Resolution

When a case enters a stage (or is created in it):

1. Read that stage's assignment rules in `sort_order`.
2. The first resolvable rule sets the primary assignee:
   - `fixed` with a user → that user;
   - permission-set pool with `first_available` or `round_robin` → resolve a member and
     persist the round-robin cursor.
3. Additional rules with `notify = true` are watchers/backup recipients and receive the
   alert without becoming the assignee.
4. If no rule resolves, keep the previous assignee or leave unassigned.
5. Write `assignee_user_id` and `stage_entered_at`/`stage_due_at`.

This supports the admin panel's three choices: named user, queue/pool, or both. Manual
reassignment is always available to `intake.assign` and is recorded as an event.

### Notifications

- On assignment, if the assignee is not the actor, insert one notification with a dedupe
  key such as `case:<id>:stage:<stage_id>:<event_id>`.
- Show a bell with unread count in the top bar. Poll every 30 seconds; refresh immediately
  after a transition. Add SSE/websockets later if polling is insufficient.
- Toasting on new assignment gives immediate feedback while the board is open.
- Email/SMS is deliberately deferred to the Communications workspace. The notification row
  is the record that an outbox can later deliver.

### Assignment UX

- Board columns show the stage's default assignee and each card shows the current assignee.
- A "My cases" filter and an "Unassigned" filter are first-class.
- Reassignment is available from the card detail and via a board card menu.

---

## 9. Desktop UX

### Intake page becomes a board

Replace the current scenario picker with a board:

```
┌ Intake ────────────────────────────────────────────────────────────────┐
│ [Flow: Disability — standard ▾] [Division ▾] [Assignee ▾] [Search…]    │
│ [My cases] [Unassigned] [Waitlisted] [Include closed]      [+ New intake]│
├──────────────┬──────────────┬──────────────┬──────────────┬────────────┤
│ Enquiry  4   │ Eligibility 3│ Risk      2  │ Planning  5  │ Onboarded  │
│ ┌──────────┐ │ ┌──────────┐ │ ┌──────────┐ │ ┌──────────┐ │ ┌────────┐ │
│ │ Mia Chen │ │ │ Sam Lee  │ │ │ Jo Patel │ │ │ Ana Ruiz │ │ │ …      │ │
│ │ Disability│ │ │ Aged care│ │ │ Alex · 2d│ │ │ Waitlist │ │ │        │ │
│ │ 2/5 ✓    │ │ │ 1/3 ✓    │ │ │ ⚠ 1 block│ │ │ 4/6 ✓    │ │ │        │ │
│ └──────────┘ │ └──────────┘ │ └──────────┘ │ └──────────┘ │ └────────┘ │
└──────────────┴──────────────┴──────────────┴──────────────┴────────────┘
```

- The board is single-flow in v1: choose a flow with a selector or tabs. Columns come from
  that flow in configured order. Merging several flows by stage kind is deferred because
  stage ids, fields, actions and checklists are flow-specific.
- Horizontal scrolling per column with vertical virtualised card lists.
- Cards show name, flow/variant, lifecycle pill, division, assignee, checklist progress,
  waitlist pill, due/overdue indicator and a risk/blocked icon.
- Drag a card to another column to transition. On success the move is optimistic; on
  `422` the card returns and a dialog lists missing fields/checklist items. On `409` the
  board refreshes and explains that someone else changed the case.
- A "Move to…" action on the card menu provides a keyboard/accessibility path.
- "New intake" opens a flow picker grouped by subject kind and variant, then a contact
  step: search the directory for an existing person or enter core details. The server
  performs duplicate matching, and the desktop lets the user link a candidate, create a new
  contact, or (with a reason) force a duplicate. Submitting creates the contact and case
  transactionally at the first stage and opens the detail column. If a legacy draft is
  selected, it is migrated into a case and contact instead.

### Case detail

Open as a docked column (`ColumnKind::IntakeCase(String)`) so the board remains visible and
the case can be popped out like a contact record. Structure:

- Header: name, flow, stage, lifecycle pill, assignee, division, priority, waitlist.
- Stage banner: what this stage is for, due date, assignee, blocked/complete indicators.
- Fields: only the current stage's `visible_fields`, with required markers. A collapsed
  "Captured so far" section shows fields from earlier stages read-only.
- Checklist card: required/optional items, completion, notes, progress.
- Actions card: allowed actions for this stage, with disabled reasons. These are shortcuts,
  not the enforcement point.
- Timeline: stage changes, assignments, checklist completions, field changes, actions.
- Contact: always present from enquiry; open the contact record in its own column. The
  care/employment role is linked automatically when the case reaches the configured
  role-link stage (default: Onboarded).
- Footer: Save (debounced autosave), Move stage, Cancel case.

### Contact record and other pages

- `ContactDetailDto` gains an `intake` block:
  ```json
  {
    "case_id": "…",
    "flow": "Disability — standard",
    "stage": "Risk assessment",
    "lifecycle_status": "onboarding",
    "allowed_actions": ["profile.edit", "relationships.update"],
    "denied_actions": {
      "funding.add": "Funding changes are locked during risk assessment."
    }
  }
  ```
- The contact editor reads it and disables profile save, relationship editing, funding
  buttons and roster actions. Disabled controls get a tooltip explaining the stage, not a
  generic "not allowed".
- The rostering and funding pages apply the same policy for the contacts involved in the
  action.
- Server-side guards are mandatory for every action; the UI is a convenience.

### Configuration

- Configuration lives in the **server admin UI**: `/admin/intake` for flows, stages, fields,
  actions, checklists and assignees.
- The desktop consumes the configuration and renders it. It does not ship a second
  configuration surface, so validation and permissions stay in one place.
- Admin changes are audited and versioned; the desktop refreshes flows on next board load.

---

## 10. Desktop architecture changes

| Area | Change |
|---|---|
| `src/intake.rs` | Add case/flow/stage/board models; keep `IntakeDraft` as a local buffer linked to a case |
| `src/pages/intake/mod.rs` | Replace picker/wizard with board + flow picker; keep or retire wizard after migration |
| `src/pages/intake/board.rs` | New: columns, cards, filters, drag/drop |
| `src/pages/intake/case.rs` | New: case detail column, fields, checklist, timeline, actions |
| `src/app.rs` | Add `ColumnKind::IntakeCase`, board/case state, notification bell, action routing |
| `src/remote.rs` | Add commands/events for flows, board, cases, transitions, checklist, notifications, assignees |
| `src/store/schema.rs` | Schema v4: link local drafts to server cases; optional board snapshot cache |
| `src/store/intake.rs` | Upsert/load local buffers by case id; migrate legacy drafts |
| `src/contact.rs` | Parse `intake` policy block; lifecycle status display |
| `src/pages/contact_editor.rs` | Apply action policy to controls |
| `src/pages/rostering.rs` | Apply action policy to rostering actions |
| `src/pages/*` funding | Apply action policy to funding actions |
| `src/pages/settings.rs` | No intake configuration; link to the server admin panel only |

### Local cache migration

- Keep the `intakes` table as a local draft buffer. Add `case_id`, `contact_id`, `flow_id`
  and `remote_version` columns, or create `intake_draft_buffers` keyed by `case_id`.
- On first sign-in after upgrade, for each local draft:
  1. Match an existing contact or create one from the draft's name/email/phone.
  2. Create a server case at the flow's first stage with `legacy_draft_id`.
  3. Store the returned case and contact ids locally.
  4. If offline, show it under "Local drafts" and sync when the server is reachable.
- Existing completed intakes stay completed; their contacts are already on the server.
- Do not delete local drafts until the server case has been acknowledged and the contact
  id is stored.

### Performance

- Board fetch is one request per filter change, with per-column limits and cursors.
- Keep the current request-id pattern so stale responses are discarded.
- Cache the board in memory; optionally persist a snapshot for first paint later.
- Do not block the UI on transitions: optimistic move, then reconcile.
- Refresh notifications on a 30-second timer and after transitions, not on every frame.

---

## 11. Contact lifecycle actions

Introduce a small, explicit state machine rather than free-text `contact_status`.

Allowed transitions (server-enforced):

| From | To |
|---|---|
| `enquiry` | `onboarding`, `intake_cancelled` |
| `onboarding` | `active`, `intake_cancelled` |
| `active` | `suspended`, `offboarding`, `offboarded` |
| `suspended` | `active`, `offboarding`, `offboarded` |
| `offboarding` | `offboarded`, `active` |
| `intake_cancelled` | `enquiry` (new case) |
| `offboarded` | `enquiry` (new case) |

- Reasons are required for `suspended`, `offboarding`, `offboarded` and
  `intake_cancelled`.
- Every change writes `contact_status_history` and `audit_log`.
- `is_active` remains for backwards compatibility but becomes derived or kept in sync with
  the lifecycle (`offboarded` implies inactive). Prefer making the lifecycle authoritative
  and deprecating the free-text `details.contact_status`.
- The contact editor gets a status control with a confirmation dialog and reason field.
- Stage nudges use the same transition rules. If a stage would move a contact along an
  illegal edge (or would downgrade an `active` client starting a new intake), the nudge is
  skipped and logged; it never silently overwrites the lifecycle.
- Statuses may evolve independently of intake later. The board does not own the status, and
  a future lifecycle board belongs on the Contacts page.

---

## 12. Migration and compatibility

1. **Existing contacts** get `lifecycle_status = 'active'`. If
   `details.contact_status` is present, map known values; otherwise active.
2. **Existing local drafts** become a contact (matched or created) plus a server case at
   the first stage of the matching default flow, with `legacy_draft_id` set. Their fields
   move into `intake_cases.data`; core identity fields populate the contact.
3. **Existing completed intakes** are not re-created. Their contacts remain; a completed
   event is not needed.
4. **Existing desktop builds** talking to a new server: old clients continue to use
   `POST /contacts`; the new endpoints are additive. A new client talking to an old server
   should show a clear "server does not support intake boards yet" message rather than
   silently falling back to local-only drafts.
5. **API versioning:** keep everything under `/api/v1` and make new fields additive. If a
   breaking shape is unavoidable, add `/api/v2/intake`; prefer additive.
6. **Feature flag:** `intake.boards` capability or server config flag lets an organisation
   enable the board before every flow is configured. Until then, the existing wizard
   remains available. Remove the fallback after one release.

---

## 13. Delivery phases

### Phase 0 — Decisions and contract (small)

Decisions confirmed:

- Kanban columns are intake stages; contact lifecycle is independent and may change later.
- Server work is in scope.
- Contacts are created immediately at enquiry and reused across attempts.
- Assignment supports named users and queues/pools, configurable per stage in the admin
  panel.
- Configuration lives in the server admin UI.

Phase 0 artefacts drafted:

- server ADR 0013 covering the case model, stage vocabulary, policy enforcement and
  assignment/notification semantics;
- the board/case/transition/contact-match JSON contract and error shape;
- default flows, stages, field visibility, actions and checklists.

Remaining Phase 0 work:

- review and accept ADR 0013;
- resolve the role-linking question for enquiry-stage directory visibility;
- freeze the contract version and begin the migration.

**Exit:** accepted ADR + frozen API contract + seed configuration.

### Phase 1 — Server foundation (medium)

- Migration for lifecycle, flows, stages, actions, assignees, checklist templates,
  cases, events, waitlist, notifications.
- Capabilities and persona seeds.
- Flow/stage config CRUD and field catalogue.
- Server admin pages for flows, stages, fields, actions, checklists and assignees.
- Seed the eight default flows, each with the six default stages. The generic
  `client_disability` parent was removed because it duplicated the standard pipeline;
  the disability standard/emergency variants are top-level flows.
- Integration tests for config and permissions.

**Exit:** an admin can define a flow and its stages; the API returns a complete
configuration; no cases yet.

### Phase 2 — Cases, board and onboarding (large)

- Case create/read/update/transition, version checks, validation, event timeline.
- Board endpoint with filters, counts and per-column paging.
- Contact creation or matching at enquiry, transactional with the case, duplicate
  detection, lifecycle rules for new versus existing contacts, and custom values.
- Role relationship linking at the configured role-link stage (default: Onboarded).
- Contact status endpoint and history.
- Tests for transitions, duplicate contacts and cases, division filtering and permissions.

**Exit:** creating an enquiry creates or links the contact and case together; the case can
travel from Enquiry to Onboarded and link the care/employment role and lifecycle status.

### Phase 3 — Desktop board and case detail (large)

- Board page with columns, cards, filters, search and paging.
- Flow picker, contact search/create/match step, and "New intake".
- Case detail column with stage fields, autosave, timeline and contact link.
- Drag/drop transition with blocker dialog and optimistic rollback.
- Legacy draft migration and local buffer.
- UI tests and screenshots.

**Exit:** a coordinator can run the whole intake visually from the desktop.

### Phase 4 — Assignees and notifications (medium)

- Stage assignment rules and resolution: named user, queue/pool, or both, with
  first-available/round-robin strategies configured in the admin panel.
- Manual reassignment.
- In-app notifications, bell, toast, polling.
- Assignment tests and UI tests.

**Exit:** moving a case into a stage alerts its assignee; unassigned and my-case filters
work.

### Phase 5 — Checklists, field policy and action policy (large)

- Checklist materialisation, toggles and gating.
- Stage field visibility and required-field validation.
- Action policies and server guards on contacts, relationships, funding and rostering.
- Contact detail policy block and desktop disabling.
- Admin UI for all of the above.
- Policy tests for every action and endpoint.

**Exit:** the admin can say "during risk assessment, profile and relationships can be
edited but funding and rostering cannot", and both the UI and API enforce it.

### Phase 6 — Referrals, waitlist and lifecycle polish (medium)

- Referral source/referrer fields and UI.
- Waitlist entries, filters, priority and offers.
- Contact suspend/offboard flows and history UI.
- Reporting counts (enquiries by source, conversion, time in stage).
- Documentation and demo seed.

**Exit:** the full brief is covered, including referrals and waitlists.

### Phase 7 — Hardening (small)

- Accessibility, keyboard moves, empty/error/offline states.
- Concurrency and retry tests.
- Performance pass on a large seeded board.
- Remove the old wizard fallback once migrations are proven.

---

## 14. Requirement traceability

| Brief item | Where it lands |
|---|---|
| Kanban board with columns per intake stage | Phases 1–3 (`intake_stages`, board API, desktop board) |
| Contact statuses (Enquiry … Offboarded), independent of intake | Phases 1–2 (`contacts.lifecycle_status`, history), Phase 6 UI |
| Contact created at enquiry and reused across attempts | Phase 2 (transactional contact + case creation, duplicate matching), Phase 3 contact step |
| Different flows and sub-flows | Phase 1 (`intake_flows`, parent/child, seed variants) |
| Client aged care / disability / emergency / fee for service | Phase 1 seed + Phase 3 flow picker |
| Workers, other contacts, organisations | Phase 1 seed + Phase 3 flow picker |
| Filter by division or subdivision | Phase 2 recursive division filter, Phase 3 filter bar |
| Assignee per stage: named user, queue/pool, or both | Phase 4 (`intake_stage_assignees`, `assignee_user_id`, admin panel rules) |
| Alert assignee on entry | Phase 4 (`notifications`, bell, toast) |
| Admin-configurable stages | Phases 1 and 5 (server admin UI) |
| Fields visible per stage | Phases 1/5 (`visible_fields`, `required_fields`, field catalogue) |
| Actions allowed per stage | Phases 1/5 (`intake_stage_actions`, server guards) |
| Roster add/remove, funding add/edit/remove, profile edit, relationships update | Phase 5 (seven action keys and guards) |
| Checklists per stage | Phases 1/5 (stage templates, case items, gate) |
| Columns: Enquiry, Eligibility, Risk, Planning, Onboarded, Cancelled | Phase 1 seed, Phase 3 board |
| Referrals | Phase 6 (source/referrer fields) |
| Waitlists | Phase 6 (`intake_waitlist_entries`) |
| Enquiries | Phase 2 (case created at enquiry stage) |

---

## 15. Testing strategy

### Server

- Migration/backfill tests: existing contacts become active; legacy custom values survive;
  custom fields can target `intake_case`.
- Config tests: flow/stage CRUD, reorder, invalid field keys, terminal stage rules,
  permission failures, clone/sub-flow.
- Case tests: create with a new contact, link an existing contact, duplicate contact
  candidates, one open case per flow, version conflict, transition required fields,
  checklist gate, terminal stage, cancellation, onboarding idempotency, role link at the
  configured stage, lifecycle rules for new vs active contacts, division filter with
  descendants, waitlist.
- Policy tests: each of the seven actions allowed/denied by stage, on every guarded
  endpoint (contacts, relationships, funding, rostering).
- Notification tests: assignment creates one notification, self-assignment does not,
  retries dedupe, unread counts.
- Permission matrix tests for `intake.read/write/assign/configure`.

### Desktop

- `tests/ui.rs`: board renders, filter changes, flow picker, case column opens, drag
  transition success/rollback, notification bell, action disabling.
- Unit tests for board filter parsing, stage field visibility, policy parsing, legacy draft
  migration.
- `examples/screenshot.rs` flags for `--intake-board`, `--intake-case`,
  `--intake-config` so snapshots remain part of design review.
- Manual QA against the Blue Dreams demo seed with a large number of enquiries.

### Contract

- A small fixture set shared between server tests and desktop parsing tests (JSON board,
  case, transition error). This catches field renames early.

---

## 16. Risks and open questions

### Confirmed decisions

1. **Kanban columns are intake stages.** The seven contact statuses are separate and may
   change later independently of intake.
2. **Server work is in scope** and sequenced first.
3. **Contacts are created immediately at enquiry** and reused across intake attempts.
4. **Both assignment models are supported** — named users and queues/pools — selectable per
   stage in the admin panel.
5. **Configuration lives in the server admin UI.**

### Remaining questions

6. **Contact role timing at enquiry.** This plan creates the contact at enquiry but links
   the client/employee role at the configured role-link stage (default: Onboarded), so an
   enquiry is not counted as an active client or employee. If the organisation wants
   enquiry-stage people in the Clients/Workers lists immediately, add a dedicated
   `prospect` relationship rather than a fake care role.
7. **Sub-flow inheritance.** Clone-on-create is proposed. A live shared stage library is
   more maintainable but harder to reason about when one child needs an exception.
8. **Alert channel.** In-app only in v1; email/SMS waits for Communications. Confirm this
   is acceptable for the first release.
9. **Waitlist model.** Waitlist as a case attribute with entries is proposed rather than a
   column, so cases do not disappear from the pipeline. Confirm.
10. **Multiple open cases.** One open case per contact per flow is enforced; different flows
    may be open concurrently. Confirm this matches re-intake and dual-role scenarios.
11. **Division authorisation.** Division filtering is not row-level security in this phase.
    If House Manager-style scoping is required for intake, add scoped permission
    assignments in the same slice rather than implying it works.

### Delivery risks

- **Action guards are cross-cutting.** Every mutating contact/relationship/funding/roster
  endpoint must call the policy helper. A missed endpoint is a silent hole. Use a shared
  helper plus endpoint tests, and consider a lint/test that enumerates guarded routes.
- **Field catalogue duplication.** The desktop currently owns step definitions. Until the
  server catalogue is authoritative, keep a compatibility test so keys cannot drift.
- **Configuration complexity.** A stage with fields, actions, checklist, assignees and SLA
  is a lot of settings. Ship sensible defaults and a preview; do not make every flow start
  from a blank canvas.
- **Board performance.** Per-column paging and denormalised card fields are needed before
  large providers use it. Do not fetch all cases and filter client-side.
- **Concurrent edits.** Version checks are required from day one; without them two
  coordinators can silently overwrite each other.
- **Legacy drafts.** Migration must be idempotent and must not create duplicate contacts or
  cases on retry.

---

## 17. Current state and next step

The ADR and API contract are written:

- `CareLibre/docs/adr/0013-intake-cases.md`
- `CareLibre/docs/intake.md`

Phases 1 and 2 are implemented on `CareLibre` branch `feature/intake-cases`:
schema, capabilities, seeded flows, configuration API, server admin configuration and a
read-only admin board, case creation/matching, board/list APIs, transitions, checklist
gates, assignment resolution, in-app notifications, onboarding role linking and contact
lifecycle.

Next:

1. Build the interactive desktop board and case detail (Phase 3).
2. Add the desktop notification bell and action-policy disabling (Phases 4–5).
3. Add referrals/waitlist and lifecycle polish (Phase 6).
4. Revisit the provider discovery gaps in Appendix A before finalising the roadmap.

---

## Appendix A — Provider discovery notes (November 2026)

These notes summarise operational reality from intake discovery sessions. They are
sanitised: names, individuals and identifying details are deliberately omitted. They
inform sequencing and future scope; they are not a commitment for the first desktop board.

### How intake actually runs today

- Some intakes arrive as **leads already in the CRM**, started by another coordinator;
  others arrive as direct calls. Direct calls can land with the aged-care team first.
- Aged care work is physically tracked on a whiteboard; the intake board should replace it.
- Aged care needs a **referral code entered into the MAC portal first**. Funding problems are
  often visible immediately. The person then goes onto the MAC waitlist, and the MAC record is
  transcribed into the CRM. An API integration to pull MAC data is a possible stretch goal.
- Different document packs apply by domain. Aged care and community variants differ. The
  current pack includes new-client, risk, in-home support, consent, falls/Barthel and advance
  care plan documents depending on the care domain.
- Aged-care clients usually do not complete online forms. Staff fill the form on the phone or
  in person, then visit to get signatures, take photos, and return. Some clients still need
  printed copies. Consents are difficult on small phone screens.
- Community supports are quoted before delivery; no supports start until the quote is agreed
  and approved. SAH funding is visible up front, so it is budgeted rather than quoted.
- Brokerage referrals are usually short-term PAC/hospital or carer-support referrals. They
  turn around quickly; the risk assessment is often compressed or skipped in practice.
- Emergency intakes bypass the normal coordinator, often happen after hours, and can come from
  child protection or similar agencies. The referrer may not know the funding source.
- Emergency intake needs only enough information to write a safe care plan, populate the
  worker mobile app, and warn workers about immediate risks. Placeholder data is acceptable.
  Documentation and funding are completed after rostering starts.
- Emergency supports can be grafted onto existing rosters. Care plans may be temporary.
  Billing can sometimes happen before delivery for agency-funded work.
- Documents still live on a shared drive because upload/storage and e-signing are seen as
  clunky. Drag-and-drop upload into the right folder would be valuable.
- Cold enquiries currently disappear into the ether; coordinators lack a reliable follow-up
  store and prompts.
- Support coordinators may guess at remaining funding and withhold evidence. Providers need
  evidence of **remaining funds**, not just a plan total, and can ask for it early with a
  gentle advisory that it is optional but speeds up intake.
- Intake handoffs are organised by type: aged care/community, brokerage, fee-for-service and
  respite are handled by different people. Stage assignment alone may not capture this.
- The fields used most are the detailed profile and notes/forms (referral forms and follow-up
  notes). Accommodation EOI forms duplicate much of the new-client form.
- After an emergency is rostered, the team circles back to finish documentation and funding.

### Feature gaps to consider later

| Gap | Why it matters | Suggested home |
|---|---|---|
| CRM lead linkage and referral code | Avoid duplicate entry and support MAC-first workflows | `intake_cases.crm_lead_id`, `external_referral_code`; CRM import |
| MAC portal integration | Aged care starts with a MAC code and waitlist entry | Future integration; stretch goal |
| Document requirements per flow/domain | Document packs differ by aged care/community/care domain | Flow/stage document requirements, document records |
| Document upload, storage and drag-and-drop | Shared drive is still used for agreements and copies | Documents workspace; storage integration |
| Generated forms, e-sign and print | Older clients often need staff-assisted or printed forms | Document merge templates; e-sign provider |
| Call transcription into new-client forms | Staff currently transcribe long conversations | AI-assisted form fill with consent; deferred |
| Emergency intake mode | Required fields block work when a name/address is unavailable | Per-flow optional-required fields, placeholder values, completeness flags |
| Minimum roster-readiness checklist | Workers need care plan, warnings and app access before first shift | Separate readiness policy from document completeness |
| Post-intake catch-up tasks | Funding and documents finish after rostering | Case tasks/reminders, not just stage checklists |
| Cold-enquiry follow-up | Enquiries currently go cold with no prompt | Follow-up tasks, SLA reminders, stale filter |
| Type-based assignment groups | Work is split by aged care/community/brokerage/fee-for-service/respite | Assignment groups or permission-set queues per intake type |
| Quote-required and budget-only modes | Community is quoted; SAH is budgeted; emergency may skip quoting | `quote_required` flag and links to Quote Builder/agreements |
| Funding evidence capture | Coordinators may withhold or guess remaining funds | Attachment/evidence field with advisory copy |
| Brokerage fast lane | Short-term PAC referrals need a compressed flow | Brokerage flow variant with minimal gates |
| Fee-for-service short form | Only basic contact details are needed | Dedicated short flow |
| Accommodation EOI | Word form duplicates new-client data | EOI flow or document template |
| External referrer portal / no-name quotes | Referrers sometimes request a quote without a name | Referral intake endpoint/portal; deferred |
| Shared-drive/deep-link integration | Staff rely on folder drag-and-drop | Storage integration; deferred |
| Pre-delivery billing exceptions | Some agency work pays before delivery | Finance exception workflow; outside intake core |

### What the current design already handles

- Configurable flows and stage columns.
- Per-stage field visibility, required fields and checklists.
- Named-user and queue-based assignment and in-app alerts.
- Division/subdivision filtering.
- Contact created at enquiry and reused across attempts.
- Stage checklists, contact lifecycle and onboarding.
- Flow variants for aged care, disability standard/emergency and fee-for-service.

### Planning consequences

1. Keep emergency intake as a **flow variant** with relaxed required fields and a separate
   roster-readiness checklist, rather than weakening validation globally.
2. Treat documents as a first-class future workspace. Checklist items can track “document
   received” now, but they do not replace document storage or e-signing.
3. Model referral code, CRM lead and funding-evidence status on the case before building
   reporting, because they are the fields coordinators most need to see on the board.
4. Allow assignment by intake type (aged care/community, brokerage, fee-for-service,
   respite) in addition to stage.
5. Do not assume every client can use an online form or phone signature. The desktop should
   support staff-assisted entry and print flows even if mobile/portal forms come later.
