//! External organisation records: providers, brokers, agencies and services.

use serde::{Deserialize, Serialize};

/// One external organisation in the tenant.
#[derive(Clone, Debug, Deserialize)]
pub struct ApiOrganisation {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub abn: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub phone: Option<String>,
    #[serde(default)]
    pub website: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default = "active")]
    pub is_active: bool,
    #[serde(default)]
    pub relationship_count: usize,
    #[serde(default)]
    pub updated_at: String,
}

fn active() -> bool {
    true
}

#[derive(Clone, Debug, Deserialize)]
pub struct ApiOrganisationList {
    pub items: Vec<ApiOrganisation>,
    pub total: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct ApiOrganisationWrite {
    pub name: String,
    pub kind: Option<String>,
    pub abn: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub website: Option<String>,
    pub notes: Option<String>,
    pub is_active: bool,
}

/// The list filter sent to the server. `All` includes archived records.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrganisationStatusFilter {
    Active,
    Inactive,
    All,
}

impl OrganisationStatusFilter {
    pub const ALL: [Self; 3] = [Self::Active, Self::Inactive, Self::All];

    pub fn label(self) -> &'static str {
        match self {
            Self::Active => "Active",
            Self::Inactive => "Archived",
            Self::All => "All",
        }
    }

    pub fn param(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Inactive => "inactive",
            Self::All => "all",
        }
    }
}

pub fn optional(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}
