//! CareLibre Desktop: a fast, native care management client.
//!
//! The binary is a thin wrapper around the [`CareLibreApp`] shell. Keeping the
//! app in a library makes it available to headless UI tests and tooling.

pub mod api;
pub mod app;
pub mod config;
pub mod pages;
pub mod perf;
pub mod server;
pub mod store;
pub mod theme;

pub use app::CareLibreApp;
