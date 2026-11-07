//! Full contact records, including profile data and custom-field metadata.

use std::collections::BTreeMap;

use crate::store::{ContactKind, ContactRow};

#[derive(Clone, Debug)]
pub struct ContactRecord {
    pub summary: ContactRow,
    pub profile: BTreeMap<String, String>,
    pub custom_fields: Vec<CustomFieldDef>,
    /// Values keyed by custom field id.
    pub custom_values: BTreeMap<String, String>,
}

#[derive(Clone, Debug)]
pub struct CustomFieldDef {
    pub id: String,
    pub key: String,
    pub label: String,
    pub field_type: String,
    pub options: Vec<String>,
    pub section: Option<String>,
    pub required: bool,
    pub sort_order: i64,
}

#[derive(Clone, Debug)]
pub struct ContactUpdate {
    pub id: String,
    pub name: String,
    pub kind: ContactKind,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub division: Option<String>,
    pub status: String,
    pub profile: BTreeMap<String, String>,
    /// Values keyed by custom field id.
    pub custom_values: BTreeMap<String, String>,
}

impl ContactRecord {
    pub fn value(&self, key: &str) -> &str {
        self.profile.get(key).map(String::as_str).unwrap_or("")
    }

    pub fn custom_value(&self, field_id: &str) -> &str {
        self.custom_values
            .get(field_id)
            .map(String::as_str)
            .unwrap_or("")
    }
}
