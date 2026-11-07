//! CareLibre Desktop: a fast, native care management client.
//!
//! The binary is a thin wrapper around the [`CareLibreApp`] shell. Keeping the
//! app in a library makes it available to headless UI tests and tooling.

pub mod api;
pub mod app;
pub mod chrome;
pub mod config;
pub mod contact;
pub mod credentials;
pub mod intake;
pub mod pages;
pub mod perf;
pub mod remote;
pub mod server;
pub mod store;
pub mod system_theme;
pub mod theme;

pub use app::CareLibreApp;
