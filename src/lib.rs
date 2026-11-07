//! CareLibre Desktop: a fast, native care management client.
//!
//! The binary is a thin wrapper around the [`CareLibreApp`] shell. Keeping the
//! app in a library makes it available to headless UI tests and tooling.

pub mod app;
pub mod pages;
pub mod theme;

pub use app::CareLibreApp;
