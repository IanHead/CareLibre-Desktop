# CareLibre Desktop

A fast, native desktop client for [CareLibre](https://github.com/IanHead/CareLibre), the
open-source care management platform for disability and aged care providers.

> **Status: prototype.** The native application shell, home dashboard and server
> authentication are in place. Contacts, clients and workers are read live from the
> CareLibre server through a background worker and displayed in virtualised, paged lists.
> Every contact opens in its own native, free-floating editor window; edits and custom-field
> values are saved back to the server. Guided intake wizards persist drafts locally and
> create onboarding contacts on the server. Rostering, services, documents and
> communications are next.

![CareLibre home screen](docs/home.png)

![CareLibre virtualised contacts](docs/contacts.png)

![CareLibre server settings](docs/settings.png)

![CareLibre intake picker](docs/intake.png)

![CareLibre client intake wizard](docs/intake-client.png)

![CareLibre contact editor with custom fields](docs/contact-editor.png)

## Design principles

- **Performance is the feature.** The desktop client is designed to be as fast as
  possible: every list is virtualised, API calls run on background workers, the UI thread
  never blocks on the network or SQLite, and no screen waits on a spinner. Contacts are
  fetched directly from the server today; the local-first cache and delta-sync plan for
  large rosters is in [`docs/performance-architecture.md`](docs/performance-architecture.md).
- **Native, not a web page in a trench coat.** Rust, [egui](https://github.com/emilk/egui)
  and wgpu. No Electron, no embedded web view, no JavaScript.
- **Cross-platform.** Windows, macOS and Linux from one codebase.
- **GPU first, CPU fallback.** wgpu selects Vulkan, Metal, DX12 or OpenGL at runtime and
  can fall back to software adapters (for example lavapipe or llvmpipe) on machines
  without a usable GPU.
- **Multitasking by default.** Contact records open in independent native OS windows that
  can be moved, resized and kept open alongside the home screen.
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
Signing in is required for all contact data and intake completion: the desktop no longer
uses mock contacts. Access tokens are refreshed automatically; the refresh token is kept in
memory only for now. Persisting it in the OS keychain is on the roadmap.

For local development, the server repository ships a demo seed (Blue Dreams Care) with
1,700+ contacts and custom fields:

```sh
cd ../CareLibre
make seed-demo
# owner@bluedreams.example / BlueDreamsCare!2026
```

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

Wizards save drafts automatically to the local SQLite cache, can be closed and resumed from
the picker, and create the contact on the server when completed. Core fields are sent as
contact columns; the rest of the intake data is stored in the contact's server-side
`details` JSON. Completed records appear immediately in the server-backed Contacts list.

## Contact records and custom fields

Click any row in Contacts, Clients or Workers to open a native editor window. Each window is
an independent OS window that can be moved and resized away from the main application.

The editor loads the contact, its extended `details` profile and every active custom-field
definition from the server. Nothing is hardcoded in the UI:

- **Stock fields** are the server's contact columns: display name, given/family/preferred
  name, pronouns, date of birth, email, phone, notes, role and status.
- **Custom fields are metadata.** Definitions live in the server's
  `custom_field_definitions` table and values in `custom_field_values`. The editor renders
  them with the same labels and inputs as stock fields, in the same form — they are not
  grouped into a separate custom-fields section. A field created in the server admin panel
  appears automatically on the next contact open.
- Profile JSON from intake is rendered as editable fields alongside the stock record.
- Saving sends a `PATCH /api/v1/contacts/{id}` request and refreshes the server-backed list.

## Server data and performance

Contacts are read from `GET /api/v1/contacts` and detail/custom-field data from
`GET /api/v1/contacts/{id}`. All calls run on a background worker; the UI renders the last
snapshot and pages in more rows as you scroll. There is no mock contact data in the desktop
client.

Press **F12** in the app to toggle the performance HUD (frame time, rows loaded, API
latency). The full architecture and roadmap for local-first caching and delta sync is in
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
cargo run --example screenshot -- docs/contacts.png contacts --live \
  --login=you@example.com:your-password
cargo run --example screenshot -- docs/contact-editor.png contacts --live --contact \
  --login=you@example.com:your-password
```

### Layout

| Path | Purpose |
| --- | --- |
| `src/main.rs` | Native entry point and window configuration |
| `src/app.rs` | Application shell: top bar, sidebar, status bar, routing |
| `src/api.rs` | Blocking HTTP client for the CareLibre REST API |
| `src/server.rs` | Connection state and the auth/health worker |
| `src/remote.rs` | Background contact API worker |
| `src/config.rs` | Persisted desktop preferences |
| `src/contact.rs` | Full contact, custom-field and update models |
| `src/intake.rs` | Intake draft model and shared types |
| `src/store/` | Local SQLite store for intake drafts and development tooling |
| `src/perf.rs` | F12 performance HUD |
| `src/theme.rs` | Palette, typography and shared card/pill widgets |
| `src/pages/` | One module per area; `home.rs` dashboard, `contacts.rs` virtualised list, `contact_editor.rs` record editor, `intake/` wizards, `settings.rs` connection/account UI |
| `assets/fonts/` | Bundled Inter font (SIL OFL 1.1) and licence |
| `examples/screenshot.rs` | Headless renderer used for snapshots |
| `tests/ui.rs` | Headless rendering and sidebar navigation tests |
| `tests/store.rs` | SQLite schema, filtering, FTS and keyset paging tests |
| `tests/server_live.rs` | Live server connection tests (`--ignored`) |

## Roadmap

The desktop client tracks the wider CareLibre requirements:

- [x] Native egui/wgpu application shell
- [x] Home dashboard and navigation
- [x] Live server connection, account authentication and organisation onboarding
- [x] PostgreSQL-backed CareLibre server (via REST API)
- [x] Server-backed contacts, background API worker and virtualised lists
- [x] Guided intake wizards that create server contacts
- [x] Native free-floating contact editor windows
- [x] Contact editing with server-stored custom-field metadata
- [x] Performance HUD
- [ ] Delta sync, optimistic local writes and offline cache
- [ ] Server-side rostering, services and other care-domain modules
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
