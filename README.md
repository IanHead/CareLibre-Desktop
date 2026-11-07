# CareLibre Desktop

A fast, native desktop client for [CareLibre](https://github.com/IanHead/CareLibre), the
open-source care management platform for disability and aged care providers.

> **Status: prototype.** The native application shell, home dashboard, live server
> connection, account authentication and organisation onboarding are in place. Clients
> and workers now render from a local SQLite cache with virtualised 50,000-row lists.
> Rostering, services, documents and communications are navigable placeholders waiting on
> the care-domain APIs.

![CareLibre home screen](docs/home.png)

![CareLibre virtualised contacts](docs/contacts.png)

![CareLibre server settings](docs/settings.png)

## Design principles

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
| `src/store/` | SQLite schema, queries, demo seeding and the cache worker |
| `src/perf.rs` | F12 performance HUD |
| `src/theme.rs` | Palette, typography and shared card/pill widgets |
| `src/pages/` | One module per area; `home.rs` dashboard, `contacts.rs` virtualised list, `settings.rs` connection/account UI |
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
- [x] Performance HUD and 1M-shift synthetic fixture
- [ ] Delta sync and optimistic local writes
- [ ] Care-domain API integration for the screens below
- [ ] Contacts, clients, workers and divisions
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
