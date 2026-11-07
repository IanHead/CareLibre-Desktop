//! Full contact reads and writes, including profile and custom-field values.

use std::collections::BTreeMap;

use rusqlite::{Connection, OptionalExtension, params};

use crate::contact::{ContactRecord, ContactUpdate, CustomFieldDef};
use crate::intake::now_ms;

use super::{ContactKind, query};

pub fn load_contact(
    conn: &Connection,
    org_id: &str,
    id: &str,
) -> Result<Option<ContactRecord>, String> {
    let summary = conn
        .query_row(
            "SELECT id, name, kind, email, phone, division, status
             FROM contacts
             WHERE id = ?1 AND org_id = ?2",
            params![id, org_id],
            query::map_contact,
        )
        .optional()
        .map_err(|error| error.to_string())?;

    let Some(summary) = summary else {
        return Ok(None);
    };

    let profile_json: Option<String> = conn
        .query_row(
            "SELECT data FROM contact_profiles WHERE contact_id = ?1",
            [id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;

    let profile = profile_json
        .and_then(|json| serde_json::from_str::<BTreeMap<String, String>>(&json).ok())
        .unwrap_or_default();

    let custom_fields = list_custom_fields(conn, org_id)?;

    let mut stmt = conn
        .prepare("SELECT field_id, value FROM contact_custom_values WHERE contact_id = ?1")
        .map_err(|error| error.to_string())?;
    let values = stmt
        .query_map([id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|error| error.to_string())?;

    let mut custom_values = BTreeMap::new();
    for value in values {
        let (field_id, value) = value.map_err(|error| error.to_string())?;
        custom_values.insert(field_id, value);
    }

    Ok(Some(ContactRecord {
        summary,
        profile,
        custom_fields,
        custom_values,
    }))
}

pub fn list_custom_fields(conn: &Connection, org_id: &str) -> Result<Vec<CustomFieldDef>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, key, label, field_type, options, section, required, sort_order
             FROM custom_fields
             WHERE org_id = ?1 AND entity = 'contact' AND enabled = 1
             ORDER BY sort_order, label, id",
        )
        .map_err(|error| error.to_string())?;

    let rows = stmt
        .query_map([org_id], |row| {
            let options: String = row.get(4)?;
            Ok(CustomFieldDef {
                id: row.get(0)?,
                key: row.get(1)?,
                label: row.get(2)?,
                field_type: row.get(3)?,
                options: serde_json::from_str(&options).unwrap_or_default(),
                section: row.get(5)?,
                required: row.get::<_, i64>(6)? != 0,
                sort_order: row.get(7)?,
            })
        })
        .map_err(|error| error.to_string())?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())
}

pub fn save_contact(
    conn: &mut Connection,
    org_id: &str,
    update: &ContactUpdate,
) -> Result<(), String> {
    let now = now_ms();
    let tx = conn.transaction().map_err(|error| error.to_string())?;

    let changed = tx
        .execute(
            "UPDATE contacts
             SET name = ?1,
                 kind = ?2,
                 email = ?3,
                 phone = ?4,
                 division = ?5,
                 status = ?6,
                 updated_at = ?7,
                 version = version + 1
             WHERE id = ?8 AND org_id = ?9",
            params![
                update.name,
                kind_db(update.kind),
                update.email,
                update.phone,
                update.division,
                update.status,
                now,
                update.id,
                org_id,
            ],
        )
        .map_err(|error| error.to_string())?;

    if changed == 0 {
        return Err("Contact could not be found.".to_owned());
    }

    let profile = serde_json::to_string(&update.profile).map_err(|error| error.to_string())?;
    tx.execute(
        "INSERT INTO contact_profiles (contact_id, data, updated_at)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(contact_id) DO UPDATE SET
            data = excluded.data,
            updated_at = excluded.updated_at",
        params![update.id, profile, now],
    )
    .map_err(|error| error.to_string())?;

    tx.execute(
        "DELETE FROM contact_custom_values WHERE contact_id = ?1",
        [&update.id],
    )
    .map_err(|error| error.to_string())?;

    for (field_id, value) in &update.custom_values {
        if value.trim().is_empty() {
            continue;
        }
        tx.execute(
            "INSERT INTO contact_custom_values (contact_id, field_id, value)
             VALUES (?1, ?2, ?3)",
            params![update.id, field_id, value],
        )
        .map_err(|error| error.to_string())?;
    }

    tx.commit().map_err(|error| error.to_string())
}

fn kind_db(kind: ContactKind) -> &'static str {
    kind.as_db()
}
