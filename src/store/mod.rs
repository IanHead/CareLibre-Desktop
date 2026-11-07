//! Local-first SQLite cache.
//!
//! The cache is disposable: it can always be rebuilt from the server. Reads are
//! served from SQLite by a background worker; the UI thread never touches the
//! database.

mod contact;
mod intake;
mod query;
mod schema;
mod seed;
mod worker;

use std::path::PathBuf;

pub use contact::{list_custom_fields, load_contact, save_contact};
pub use intake::{complete_intake, delete_intake, list_intakes, load_intake, save_intake};
pub use query::{
    ContactCounts, ContactPage, ShiftRow, contact_counts, count_shifts_in_range, query_contacts,
    shifts_in_range,
};
pub use schema::{open, open_in_memory};
pub use seed::{SeedReport, seed_demo};
pub use worker::{StoreEvent, StoreHandle};

/// Until organisation-scoped caches land with the sync engine, demo data and
/// the local cache live under this organisation id.
pub const DEMO_ORG_ID: &str = "demo";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContactFilter {
    All,
    Clients,
    Workers,
}

impl ContactFilter {
    pub fn label(self) -> &'static str {
        match self {
            Self::All => "Contacts",
            Self::Clients => "Clients",
            Self::Workers => "Workers",
        }
    }

    pub(crate) fn kinds_sql(self) -> &'static str {
        match self {
            Self::All => "'client','worker','dual','other'",
            Self::Clients => "'client','dual'",
            Self::Workers => "'worker','dual'",
        }
    }
}

#[derive(Clone, Debug)]
pub struct ContactRow {
    pub id: String,
    pub name: String,
    pub kind: ContactKind,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub division: Option<String>,
    pub status: String,
}

#[derive(Clone, Debug)]
pub struct ContactCursor {
    pub name: String,
    pub id: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContactKind {
    Client,
    Worker,
    Dual,
    Other,
}

impl ContactKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Client => "Client",
            Self::Worker => "Worker",
            Self::Dual => "Dual role",
            Self::Other => "Other",
        }
    }

    pub(crate) fn as_db(self) -> &'static str {
        match self {
            Self::Client => "client",
            Self::Worker => "worker",
            Self::Dual => "dual",
            Self::Other => "other",
        }
    }

    pub(crate) fn from_db(value: &str) -> Self {
        match value {
            "worker" => Self::Worker,
            "dual" => Self::Dual,
            "other" => Self::Other,
            _ => Self::Client,
        }
    }
}

/// Location of the local cache database.
pub fn default_cache_path() -> Option<PathBuf> {
    directories::ProjectDirs::from("au", "CareLibre", "CareLibre Desktop")
        .map(|dirs| dirs.data_dir().join("cache-demo.sqlite"))
}
