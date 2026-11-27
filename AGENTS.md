# CareLibre Desktop — agent notes

CareLibre Desktop is a native Rust desktop client for the CareLibre care management
platform. It uses egui and wgpu directly: no Electron, no embedded web view and no
JavaScript. Data comes from the CareLibre Server REST API, normally at
`http://127.0.0.1:8484` during local development.

The application is a prototype, but the code is held to production standards:
performance is the feature, the UI thread never blocks on network or SQLite work, and
lists are virtualised.

## Ways of working

- **Never commit code changes directly to `main`.** Create a branch and open a pull
  request. This includes Rust sources, tests, examples and build/dependency config.
- **Metadata-only changes may go straight to `main`.** Docs, README edits,
  screenshots, issue templates and this file are examples.
- Do not commit unrelated changes, generated files or another person's work in
  progress. Check `git status` before staging.
- Do not rewrite or discard uncommitted changes unless explicitly asked.
- CI runs only for releases. Run the local checks before pushing or opening a
  pull request; GitHub Actions is the release gate, not the everyday feedback
  loop.
- Prefer small, focused commits. Commit subjects are short imperative sentences in
  sentence case with no conventional-commit prefix, for example
  `Add relationship constellation and organisation management`.
- Pull requests should explain the user-visible change and list the checks run
  (`cargo fmt`, `cargo clippy`, `cargo test`).

## Commands

```sh
cargo run                     # development build
cargo run --release           # release build
cargo fmt --all               # format
cargo fmt --all -- --check    # verify formatting without changing files
cargo clippy --all-targets -- -D warnings
cargo test                    # UI smoke tests, navigation and store tests
cargo test --test server_live -- --ignored   # needs a running server
```

Live tests target `http://127.0.0.1:8484` by default; set
`CARELIBRE_TEST_SERVER=https://care.example.com` to use another instance.

Headless screenshots are useful for visual review and are checked in under `docs/`:

```sh
cargo run --example screenshot -- docs/contacts.png
cargo run --example screenshot -- docs/rostering.png rostering
cargo run --example screenshot -- docs/contacts.png contacts --live \
  --login=you@example.com:your-password
```

## Layout

| Path | Purpose |
| --- | --- |
| `src/main.rs` | Native entry point and window configuration |
| `src/app.rs` | Application shell: top bar, sidebar, status bar, routing |
| `src/api.rs` | Blocking HTTP client for the CareLibre REST API |
| `src/server.rs` | Connection state and the auth/health worker |
| `src/remote.rs` | Background contact API worker |
| `src/config.rs` | Persisted desktop preferences |
| `src/credentials.rs` | OS keychain / encrypted-vault refresh-token storage |
| `src/chrome.rs` | Optional client-side window title bar and resize handles |
| `src/contact.rs` | Contact, custom-field and update models |
| `src/intake.rs` | Intake draft model and shared types |
| `src/store/` | Local SQLite store for intake drafts and development tooling |
| `src/perf.rs` | F12 performance HUD |
| `src/theme.rs` | Palette, typography and shared widgets |
| `src/rostering.rs` | Rostering API models and local-time conversion |
| `src/pages/` | One module per area, including contacts, editor, rostering, intake and settings |
| `examples/screenshot.rs` | Headless renderer used for snapshots |
| `tests/ui.rs` | Headless rendering and sidebar navigation tests |
| `tests/store.rs` | SQLite schema, filtering, FTS and keyset paging tests |
| `tests/server_live.rs` | Live server connection tests (`--ignored`) |

## Conventions

- Rust 2024 edition, `rustfmt` formatting and Clippy with warnings denied. Release CI
  runs fmt, Clippy, tests and a release build on Linux, Windows and macOS; run the
  same checks locally first.
- Keep network, database and other blocking work off the UI thread. Long-running or
  fallible work goes through background workers and channels, with the UI rendering
  the last snapshot.
- Do not add mock or demo data to production paths. The desktop reads live server data.
- Use the shared palette, typography and widgets in `src/theme.rs` rather than
  hardcoded colours or one-off styling.
- The server owns stock contact columns, custom-field definitions and the relationship
  graph. Treat custom fields as metadata and render them alongside stock fields.
- Preserve the design principles: native rendering, cross-platform behaviour,
  GPU-first with CPU fallback, virtualised lists, and first-class relationships.
- Add or update tests for behaviour changes, and refresh checked-in screenshots when a
  change visibly alters the UI.
- Keep dependencies minimal and compatible with the GPL-3.0-or-later licence.

## Issues

Issue forms live in `.github/ISSUE_TEMPLATE/`: `bug_report.yml` and
`feature_request.yml`. Keep them current when the product or supported platforms
change. The canonical product roadmap is in the
[CareLibre server repository](https://github.com/IanHead/CareLibre/blob/main/docs/roadmap.md).

## Releases

Before tagging a release, add `docs/release-notes/vX.Y.Z.md` using
`docs/release-notes/TEMPLATE.md`. The release workflow refuses to publish without
the `New`, `Changed` and `Fixed` sections.

Write each bullet for a care provider, not a developer: describe what they can
now do, what works differently, or what no longer goes wrong. Avoid commit
hashes, file names, function names, API details and implementation jargon.
