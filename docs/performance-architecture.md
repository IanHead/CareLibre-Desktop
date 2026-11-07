# Performance and local-first architecture

Top-priority requirement: **FAST**. Every interaction should feel local, even when the
organisation has years of shifts and tens of thousands of contacts. This document is the
working plan for getting there.

> **Implementation status.** Phase A foundations are in place: bundled SQLite cache,
> background query worker, virtualised 50,000-row contact list, deterministic 1M-shift
> fixture, measured roster queries, and an F12 performance HUD. Measured on the seeded
> fixture: first contact page 1.1 ms, roster-week count 0.5 ms, 2,000 roster rows 3.5 ms.

## 1. Targets

Budgets are p95 unless stated otherwise. They are enforced in CI once the measurement
harness exists.

| Interaction | Budget |
| --- | --- |
| Warm start to interactive shell | < 250 ms |
| Switch main views (Home → Clients → Rostering) | < 50 ms |
| Rostering week paint, 2,000 visible shifts | < 8 ms/frame (120 Hz headroom) |
| Scroll a 50,000-row contact list | 60 fps sustained, no allocations spike |
| Local search as you type | < 30 ms |
| Open a client/worker/shift record | < 16 ms |
| Save a local edit (UI acknowledgement) | < 1 frame |
| Commit a local write to SQLite | < 10 ms |
| Apply a 500-change sync delta | < 300 ms |
| Full pull-to-caught-up after a normal day | < 1 s |
| Memory, 1M cached shifts + 50k contacts | < 300 MB resident |

Hard rules:

1. The UI thread never does network, filesystem or SQLite I/O.
2. A view never blocks on the network. It renders the last known snapshot immediately.
3. Spinners are forbidden after first-run bootstrap. Use stale-while-revalidate.
4. Every list/table is virtualised from its first commit.
5. Writes are optimistic and durable locally before they touch the network.
6. Performance is measured with production-sized data, not ten demo rows.

## 2. Architecture

```
┌──────────────────────────────────────────────────────────────┐
│ egui UI thread                                               │
│  • input, layout, painting only                              │
│  • reads Arc<ViewSnapshot> via arc-swap (lock-free)          │
│  • sends Command over crossbeam channels                     │
└───────────────┬───────────────────────────────┬──────────────┘
                │                               │
      query commands                     write commands
                │                               │
┌───────────────▼──────────────┐   ┌────────────▼──────────────┐
│ Query worker(s)              │   │ Write worker              │
│  • SQLite readers (WAL)      │   │  • single SQLite writer   │
│  • build immutable snapshots │   │  • transaction + outbox   │
│  • prefetch adjacent windows │   │  • patch snapshot         │
└───────────────┬──────────────┘   └────────────┬──────────────┘
                │                               │
┌───────────────▼───────────────────────────────▼──────────────┐
│ SQLite local cache (WAL, bundled, per organisation)          │
│  domain tables · indexes · FTS5 · sync cursor · outbox       │
└───────────────────────────────┬──────────────────────────────┘
                                │
┌───────────────────────────────▼──────────────────────────────┐
│ Sync worker                                                  │
│  • push outbox (idempotent, batched)                         │
│  • pull /sync/changes?since=<seq> in pages                   │
│  • apply in one transaction, advance cursor                  │
│  • adaptive polling + immediate sync after writes            │
└───────────────────────────────┬──────────────────────────────┘
                                │ HTTP/2, gzip/zstd
┌───────────────────────────────▼──────────────────────────────┐
│ CareLibre Server (axum + PostgreSQL)                         │
│  • entity writes + change_log in one transaction             │
│  • bootstrap snapshot + delta endpoints                      │
│  • batch mutation endpoint with idempotency keys             │
└──────────────────────────────────────────────────────────────┘
```

The cache is disposable: delete it and the client rebuilds from the server. It is never
the system of record; the server remains authoritative for audit/compliance.

## 3. Local store

Use **SQLite via `rusqlite` with the bundled feature** (no system SQLite dependency on
Windows/macOS/Linux). It gives us relational queries, joins, transactions, indexes and
FTS5 in one embedded file.

- One database per organisation: `cache-<org_id>.sqlite`, under the platform data dir.
- `journal_mode=WAL`, `synchronous=NORMAL`, `busy_timeout=5000`, `foreign_keys=ON`,
  `temp_store=MEMORY`, generous `mmap_size`/`cache_size`.
- One writer thread; reader connections per query worker. WAL allows concurrent readers.
- Migrations via `user_version`/`refinery`; incompatible schema → rebuild cache.
- Keyset pagination everywhere (`WHERE (name, id) > (?, ?)`), never deep `OFFSET`.
- Client-generated UUIDv7 IDs for new records, so offline creates never need temporary-ID
  reconciliation.
- Timestamps stored as UTC epoch milliseconds; display in the organisation timezone.
- FTS5 virtual tables for contact/name/note search, rebuilt per row on sync.
- A durable `outbox` table plus `sync_cursor` and `sync_conflicts` tables.
- Wipe the org database on logout/revocation, not just the UI state.

Do **not** denormalise display fields up front. SQLite joins over a week of shifts are
fast. Add materialised roster rows only if benchmarks show a real need.

### Query strategy

- **Contacts:** summary rows (`id, name, roles, status`) for lists; full record loaded on
  open. FTS5 for type-ahead. Virtualised table reads pages of ~100.
- **Rostering:** query exactly the visible range plus prefetch of the adjacent weeks:
  `WHERE org_id=? AND starts_at >= ? AND starts_at < ?`, index `(org_id, starts_at)`,
  secondary indexes on `(worker_id, starts_at)` and `(client_id, starts_at)`.
- **Hot window:** keep an in-memory working set for the roster range the user moves
  through (default −8 to +16 weeks), plus all contacts/rates/config. Older ranges are
  fetched lazily from SQLite.
- **Snapshots:** query workers build immutable `ViewSnapshot` values (sorted vectors,
  interval index, lookup maps) and swap them with `arc-swap`. The UI holds no locks.
- **Prefetch adjacent windows** when a view opens, and coalesce/cancel stale requests with
  a generation token when the user jumps elsewhere.

For genuinely cold data (a week never seen before), render the previous snapshot or empty
grid cells for the few milliseconds it takes to query SQLite on the worker; never block and
never show a spinner.

## 4. Sync engine

Server-authoritative, optimistic local writes. Do not introduce CRDTs for care data: the
audit/compliance model wants a server truth, and roster conflicts need explicit handling.

### Bootstrap

`GET /api/v1/sync/bootstrap?scope=...` returns a consistent snapshot at a sequence number:

- all contacts, services, rates, divisions, cost-centre configuration and current user
- rostering data for a bounded hot window (for example −12 to +18 months)
- tombstones/watermark so deltas can be applied immediately afterwards

Stream pages (compressed) and apply them in chunks so the UI becomes usable progressively.
Older rostering data is fetched on demand with a date-scoped endpoint.

### Deltas

`GET /api/v1/sync/changes?since=<seq>&limit=1000` returns ordered changes:

```json
{
  "changes": [
    {
      "seq": 91342,
      "entity": "shift",
      "id": "019...",
      "op": "upsert",
      "version": 7,
      "payload": { "...": "..." }
    },
    { "seq": 91343, "entity": "contact", "id": "019...", "op": "delete", "version": 3 }
  ],
  "next_seq": 91343,
  "has_more": false
}
```

- Server increments a per-organisation sequence; clients store the cursor.
- Payloads are the same DTOs used by normal API reads; tombstones carry only IDs.
- Pull loop applies a page in one SQLite transaction, advances the cursor, then continues
  until caught up.
- Poll adaptively: immediately after a local write, on window focus, every ~10 s while
  active, backing off to ~60 s when idle. A server-sent `sync.available` nudge over SSE or
  WebSocket can replace polling later without changing the data contract.

### Writes

`POST /api/v1/sync/mutations` accepts a batch of mutations, each with an idempotency key,
`base_version` and client UUID. The server validates, applies, writes change-log rows in the
same transaction, and returns canonical records/versions. Whole-batch retries are safe.

Local write path:

1. Validate locally (including roster double-booking).
2. Commit entity change + outbox row to SQLite in one transaction.
3. Patch the in-memory snapshot; UI acknowledges in under a frame.
4. Sync worker pushes the outbox in the background.
5. On success, store canonical versions; on `409`, store the conflict and notify.

The outbox survives crashes and network loss. Users can keep rostering offline; the status
bar shows "3 changes pending" rather than blocking anything.

### Conflicts

- Server version wins as the base; return the current record on `409`.
- Safe field-level merges are possible later, but start with explicit conflict records.
- Rostering conflicts (double booking, cancelled client, unavailable worker) are surfaced
  as an inline list with "keep mine / use server / open both" choices.
- Never silently discard a queued local edit.

## 5. Server work required

The server is at Phase 0 (auth only), so the sync contract should land alongside the
care-domain schema, not afterwards.

1. Add `updated_at`, `version` and `id` (UUIDv7) conventions to all mutable entities.
2. Add `change_log(seq bigserial, org_id, entity, entity_id, op, version, payload jsonb,
   changed_at)` with index `(org_id, seq)`. Application code writes it in the same
   transaction as the entity change; do not rely on triggers alone.
3. Add `/sync/bootstrap`, `/sync/changes` and `/sync/mutations` endpoints with cursor
   pagination, compression and role filtering.
4. Add a date-scoped roster endpoint for data outside the hot window.
5. Batch mutation validation; return per-item results, not a single opaque failure.
6. Indexes for roster queries: `(org_id, starts_at)`, `(worker_id, starts_at)`,
   `(client_id, starts_at)`, plus assignment join indexes.
7. Metrics for change-log lag, payload sizes and sync endpoint latency.
8. Compaction policy: snapshot + change-log retention per organisation; prune old deltas.

OpenAPI should describe the sync DTOs so Android/iOS can reuse the same protocol.

## 6. egui performance

- **Virtualise everything.** Use `egui_extras::TableBuilder::body.rows(...)` or custom
  painting. Never build widgets for off-screen rows.
- **Custom paint the roster grid.** Paint the background grid, shift blocks, selection and
  drag preview as shapes. Allocate one interaction response per shift block, not per cell.
  Hit-test only the visible rects.
- **Cache geometry and text.** Recompute row/column rects only on resize/scroll-range
  change. Cache prepared text galleys for repeated labels; use `Arc<str>` for names shared
  across rows.
- **Avoid per-frame allocation.** Reuse `Vec`/`String` buffers in view models; avoid
  `format!` inside row painting; avoid rebuilding filter/sort structures each frame.
- **Repaint on demand.** No continuous animation. Use `request_repaint_after` only for the
  "synced 4s ago" indicator, not every frame.
- **Release profile is the default for perf work.** Keep `lto = "thin"`; consider `panic =
  "abort"`, `opt-level = 3` and `codegen-units = 1` for shipping builds.
- **Multi-window:** share one store/sync service; give each viewport its own view model and
  snapshot subscription. Do not duplicate caches or workers per window.
- **Measure:** integrate `puffin` and a debug perf HUD (frame time, visible rows, snapshot
  build time, cache hit rate, queue depth). Add a CI benchmark for the data layer and a
  large synthetic fixture.

## 7. Rostering and session setup

Rostering is the stress case. Design for it first:

- Week/day grid backed by a hot-window snapshot; workers × days; interval index per row.
- Drag/drop, multi-select, copy/paste shifts, and duplicate day/week; all operations are
  local-first and undoable.
- Shift templates, recurring patterns ("every Mon/Wed for 8 weeks"), bulk worker assignment
  and conflict checks applied in one local transaction.
- Keyboard-first editing: arrow navigation, Enter to edit, Esc to cancel, Tab between
  fields, `Ctrl+C/V`, `Ctrl+Z/Y`, `Delete`.
- "Session setup" wizard with at most three steps, prefilled from the last session/client
  plan, batch participant selection, "create & add another", and save-as-template.
- Defaults and recent choices everywhere; no empty form should require a click to reveal
  the common case.
- Undo/redo makes every aggressive shortcut safe, which is what actually removes clicks.

## 8. Minimising clicks

Build a **command registry** where every action has: a function, a keyboard shortcut, a menu
entry and a command-palette entry. Then:

- `Ctrl+K` command palette for navigation and actions.
- Type-ahead search on every picker (local FTS, no network).
- Inline editing with autosave; no Save dialogs.
- Multi-select + bulk actions ("assign 12 shifts to Sam", "cancel selected").
- Context menus and drag/drop as accelerators, not the only path.
- Remember last-used filters/defaults per user and per organisation.
- Side panels instead of blocking modal dialogs so context stays visible.
- Keep optimistic updates instant; confirmation is a subtle toast, not a modal.

## 9. Threading and API shape

- `crossbeam-channel` command/event buses; no locks shared with the UI.
- `arc-swap<Arc<ViewSnapshot>>` per view; swap whole snapshots, never mutate in place.
- Query worker: reads SQLite, builds snapshots, prefetches adjacent windows, coalesces
  duplicate requests.
- Write worker: owns the only SQLite write connection, applies transactions and outbox
  rows, publishes patches/snapshot invalidations.
- Sync worker: network only; posts events (`status`, `changes_applied`, `conflict`).
- Cancellation: each view request carries a generation; stale results are dropped before
  snapshot build.
- Backpressure: cap queue depth per view; latest request wins.

Keep the UI crate physically unable to touch SQLite or HTTP (module boundaries), so a
blocking call cannot creep into the render path.

## 10. Security and cache lifetime

- Cache contains PII. Make encryption-at-rest an edition option (SQLCipher via rusqlite)
  and store the database key + refresh token in the OS keychain.
- Clear the org cache on logout and when a membership is revoked.
- Scope every query by `org_id` and enforce server-side role filtering in sync payloads.
- Cache schema version is checked at startup; mismatch triggers a rebuild, not a corrupt UI.

## 11. Rollout plan

| Phase | Deliverable | Exit criterion |
| --- | --- | --- |
| A. Foundations | Perf HUD/puffin, synthetic data generator, SQLite store, query worker, virtualised contact table | 50k contacts scroll at 60 fps; warm start < 250 ms |
| B. Read sync | Server change log + bootstrap/delta endpoints, client pull/apply, hot-window roster cache | 1M shifts seeded; week switch < 100 ms; deltas < 300 ms |
| C. Rostering engine | Virtualised week grid, interval index, copy/paste, undo, templates | 2,000 visible shifts paint < 8 ms; no dropped frames while scrolling |
| D. Write path | Outbox, optimistic edits, batch push, conflict records and UI | Edit acknowledged < 1 frame; offline edits survive restart |
| E. Flow polish | Command palette, shortcuts, session wizard, saved views | Common shift/session creation in ≤ 3 interactions |
| F. Hardening | Keychain/encryption, SSE nudge, mobile core reuse, CI perf gates | Budgets enforced on every PR |

Server domain APIs (contacts, rostering) are Phase 2–3 of the server roadmap. The sync
schema and endpoints should be designed in the same slice as those tables.

## 12. Decisions to confirm

1. **SQLite as the local cache** (bundled, WAL, per organisation). Recommended.
2. **Server-authoritative sync with optimistic local writes**, not CRDTs. Recommended.
3. **Bounded hot window** for the first bootstrap (−12/+18 months, configurable), with
   older rostering data fetched on demand. Recommended.
4. **Offline writes enabled**, with a visible queue and conflict resolution. Recommended
   for rostering in the field; can start read-only if the first release needs to be smaller.
5. **Cache encryption at rest** in a later phase, with keychain-held keys. Recommended
   before any production pilot.
