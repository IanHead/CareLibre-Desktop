# CareLibre Desktop

A fast, native desktop client for [CareLibre](https://github.com/IanHead/CareLibre), the
open-source care management platform for disability and aged care providers.

> **Status: prototype.** The native application shell, home dashboard, live server
> connection and account authentication are in place. Contacts, clients and workers render
> from a local SQLite cache with virtualised 50,000-row lists, and every contact can be
> opened and edited against the database. Guided intake wizards persist drafts locally and
> create onboarding records. Custom fields are database-backed metadata and render
> dynamically in the editor. Rostering, services, documents and communications are next.

![CareLibre home screen](docs/home.png)

![CareLibre virtualised contacts](docs/contacts.png)

![CareLibre server settings](docs/settings.png)

![CareLibre intake picker](docs/intake.png)

![CareLibre client intake wizard](docs/intake-client.png)

![CareLibre contact editor with custom fields](docs/contact-editor.png)

## Design principles

- **Performance is the feature.** The desktop client is designed to be as fast as
  possible: every list is virtualised, reads come from a local SQLite cache, the UI thread
  never touches SQLite or the network, writes are local-first, and no screen waits on a
  spinner. Performance budgets and the local-first sync plan are documented in
  [`docs/performance-architecture.md`](docs/performance-architecture.md).
- **Native, not a web page in a trench coat.** Rust, [egui](https://github.com/emilk/egui)
  and wgpu. No Electron, no embedded web view, no JavaScript.
- **Cross-platform.** Windows, macOS and Linux from one codebase.
- **GPU first, CPU fallback.** wgpu selects Vulkan, Metal, DX12 or OpenGL at runtime and
  can fall back to software adapters (for example lavapipe or llvmpipe) on machines
  without a usable GPU.
- **Multitasking by default.** The shell is built for a home screen plus free-floating
  windows, so several records can be open at once.
- **Open source, self-hostable.** Community Edition is free forever. Business and
  Enterprise editions add support, consulting and managed deployment.

## Running it

### Prerequisites

- [Rust](https://rustup.rs) 1.95 or newer
- A C linker / build toolchain:
  - **Linux:** `build-essential` (or equivalent)
  - **macOS:** Xcode Command Line Tools
  - **Windows:** MSVC Build Tools

On Linux you also need an X11 or Wayland session and a graphics driver offering Vulkan
or OpenGL. On Debian/Ubuntu the runtime packages are typically:

```sh
sudo apt install libxkbcommon0 libwayland-client0 libx11-6 libvulkan1
```

### Build and run

```sh
git clone https://github.com/IanHead/CareLibre-Desktop.git
cd CareLibre-Desktop
cargo run --release
```

For development builds with fast incremental compiles:

```sh
cargo run
```

## Connecting to the server

The client talks to the CareLibre Server REST API. A local development server listens
on `http://127.0.0.1:8484` by default.

Open **Settings** in the sidebar to:

- change the server URL (saved to the platform config directory)
- check liveness and database readiness (`/healthz` and `/readyz`)
- sign in, or create the first organisation with an owner account
- sign out and inspect the active organisation and edition

The connection is checked every 15 seconds and the status is shown in the status bar.
Access tokens are refreshed automatically; the refresh token is kept in memory only
for now. Persisting it in the OS keychain is on the roadmap, together with offline
support and domain data.

### Live integration tests

With a server running, exercise the real connection path:

```sh
cargo test --test server_live -- --ignored
```

Set `CARELIBRE_TEST_SERVER=https://care.example.com` to target another instance.

## Intake and onboarding

The **Intake** button in the top bar opens a scenario picker for:

- **Client intake** — identity, contact and address, health and support needs, emergency
  and professional contacts, behaviours and risk, funding and consent.
- **Worker intake** — identity and contact details, position and division, employment
  terms, award classification, compliance checks and expiry dates.
- **Other contact intake** — coordinators, family, GPs and other support-network contacts.

Wizards save automatically to the local SQLite cache, can be closed and resumed from the
picker, and create a contact with status **Onboarding** when completed. Completed records
appear immediately in the virtualised Contacts list. The captured intake data is stored as
JSON alongside the contact summary ready for the sync API.

## Contact records and custom fields

Click any row in Contacts, Clients or Workers to open the record editor. The editor loads
the core record, its extended profile and all custom-field values from SQLite; nothing is
hardcoded in the UI.

- Core fields: display name, contact type, status, division, email and phone.
- Profile fields are grouped into sections appropriate to the contact type, with client,
  worker and other-contact variants.
- **Custom fields are metadata.** Definitions live in the `custom_fields` table (label,
  type, options, section, order, required flag) and values in `contact_custom_values`.
  The editor renders them dynamically, then saves everything in one transaction.
- Saving updates the full-text index and refreshes the virtualised lists without a restart.

## Local cache and performance

Rostering and contact screens read from a bundled SQLite cache; the UI thread never
touches SQLite or the network. The contacts list is virtualised and pages in as you
scroll, with local full-text search.

Seed a large deterministic fixture and time the hot queries:

```sh
cargo run --release --example seed_cache -- 50000 1000000
cargo run --release --example bench_cache
```

This creates 50,000 contacts and 1,000,000 shifts in the platform data directory. Press
**F12** in the app to toggle the performance HUD (frame time, loaded rows, last query,
cache location).

The full architecture and roadmap for local-first sync is in
[`docs/performance-architecture.md`](docs/performance-architecture.md).

## Development

```sh
cargo test                 # UI smoke tests and navigation checks
cargo clippy --all-targets # lints
cargo fmt                  # formatting
```

### Headless screenshots

The UI can be rendered without a display, which is handy for design review and CI:

```sh
cargo run --example screenshot -- docs/home.png
cargo run --example screenshot -- docs/rostering.png rostering
cargo run --example screenshot -- docs/settings.png settings --live
```

### Layout

| Path | Purpose |
| --- | --- |
| `src/main.rs` | Native entry point and window configuration |
| `src/app.rs` | Application shell: top bar, sidebar, status bar, routing |
| `src/api.rs` | Blocking HTTP client for the CareLibre REST API |
| `src/server.rs` | Connection state and the background API worker |
| `src/config.rs` | Persisted desktop preferences |
| `src/contact.rs` | Full contact, custom-field and update models |
| `src/intake.rs` | Intake draft model and shared types |
| `src/store/` | SQLite schema, queries, intake persistence, demo seeding and the cache worker |
| `src/perf.rs` | F12 performance HUD |
| `src/theme.rs` | Palette, typography and shared card/pill widgets |
| `src/pages/` | One module per area; `home.rs` dashboard, `contacts.rs` virtualised list, `contact_editor.rs` record editor, `intake/` wizards, `settings.rs` connection/account UI |
| `assets/fonts/` | Bundled Inter font (SIL OFL 1.1) and licence |
| `examples/screenshot.rs` | Headless renderer used for snapshots |
| `examples/seed_cache.rs` | Deterministic cache fixture generator |
| `examples/bench_cache.rs` | Local query benchmark |
| `tests/ui.rs` | Headless rendering and sidebar navigation tests |
| `tests/store.rs` | SQLite schema, filtering, FTS and keyset paging tests |
| `tests/server_live.rs` | Live server connection tests (`--ignored`) |

## Roadmap

The desktop client tracks the wider CareLibre requirements:

- [x] Native egui/wgpu application shell
- [x] Home dashboard and navigation
- [x] Live server connection, account authentication and organisation onboarding
- [x] PostgreSQL-backed CareLibre server (via REST API)
- [x] Local SQLite cache, background query worker and virtualised contacts
- [x] Guided intake wizards with persisted drafts
- [x] Contact editing backed by SQLite, including custom-field metadata
- [x] Performance HUD and 1M-shift synthetic fixture
- [ ] Delta sync and optimistic local writes
- [ ] Care-domain API integration for the screens below
- [ ] Contacts, clients, workers and divisions syncing to the server
- [ ] Services, rates and effective-dated rate versions
- [ ] Rostering with shifts, assignments and sessions
- [ ] Natural-language quoting and document merge templates
- [ ] Configurable forms, fields and cost-centre inheritance
- [ ] Configurable SMS and email communications
- [ ] Offline cache and OS keychain-backed sessions
- [ ] Android and iOS companions

## Licence

CareLibre Desktop is free software licensed under the
[GNU General Public License v3.0 or later](LICENSE).
