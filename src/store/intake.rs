//! Persistence for intake drafts and completion.

use rusqlite::{Connection, OptionalExtension, params};

use crate::intake::{CompletedIntake, IntakeDraft, IntakeStatus, IntakeSummary, now_ms};

use super::ContactKind;

pub fn list_intakes(conn: &Connection, org_id: &str) -> Result<Vec<IntakeSummary>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT data, current_step, updated_at
             FROM intakes
             WHERE org_id = ?1 AND status = 'draft'
             ORDER BY updated_at DESC",
        )
        .map_err(|error| error.to_string())?;

    let rows = stmt
        .query_map([org_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)? as usize,
                row.get::<_, i64>(2)?,
            ))
        })
        .map_err(|error| error.to_string())?;

    let mut intakes = Vec::new();
    for row in rows {
        let (data, current_step, updated_at) = row.map_err(|error| error.to_string())?;
        if let Ok(draft) = serde_json::from_str::<IntakeDraft>(&data) {
            intakes.push(IntakeSummary {
                id: draft.id.clone(),
                kind: draft.kind,
                name: draft.display_name(),
                current_step,
                updated_at,
            });
        }
    }

    Ok(intakes)
}

pub fn load_intake(
    conn: &Connection,
    org_id: &str,
    id: &str,
) -> Result<Option<IntakeDraft>, String> {
    let data: Option<String> = conn
        .query_row(
            "SELECT data FROM intakes WHERE id = ?1 AND org_id = ?2",
            params![id, org_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;

    match data {
        Some(data) => serde_json::from_str(&data)
            .map(Some)
            .map_err(|error| format!("could not read intake draft: {error}")),
        None => Ok(None),
    }
}

pub fn save_intake(conn: &Connection, org_id: &str, mut draft: IntakeDraft) -> Result<(), String> {
    let existing_status: Option<String> = conn
        .query_row(
            "SELECT status FROM intakes WHERE id = ?1",
            [&draft.id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;

    if existing_status.as_deref() == Some("completed") {
        return Ok(());
    }

    draft.updated_at = now_ms();
    let data = serde_json::to_string(&draft).map_err(|error| error.to_string())?;

    conn.execute(
        "INSERT INTO intakes
            (id, org_id, kind, status, current_step, data, contact_id, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT(id) DO UPDATE SET
            kind = excluded.kind,
            status = excluded.status,
            current_step = excluded.current_step,
            data = excluded.data,
            contact_id = excluded.contact_id,
            updated_at = excluded.updated_at",
        params![
            draft.id,
            org_id,
            kind_db(draft.kind),
            "draft",
            draft.current_step as i64,
            data,
            draft.contact_id,
            draft.created_at,
            draft.updated_at,
        ],
    )
    .map_err(|error| error.to_string())?;

    Ok(())
}

pub fn complete_intake(
    conn: &mut Connection,
    org_id: &str,
    mut draft: IntakeDraft,
) -> Result<CompletedIntake, String> {
    let now = now_ms();
    let contact_id = draft
        .contact_id
        .clone()
        .unwrap_or_else(|| format!("contact-{}", draft.id));
    let name = draft.display_name();
    let email = optional_field(&draft, "email");
    let phone = optional_field(&draft, "phone");
    let division = optional_field(&draft, "division");

    draft.status = IntakeStatus::Completed;
    draft.contact_id = Some(contact_id.clone());
    draft.updated_at = now;
    let data = serde_json::to_string(&draft).map_err(|error| error.to_string())?;

    let tx = conn.transaction().map_err(|error| error.to_string())?;

    tx.execute(
        "INSERT INTO contacts
            (id, org_id, name, kind, email, phone, division, status, updated_at, version)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'Onboarding', ?8, 1)
         ON CONFLICT(id) DO UPDATE SET
            name = excluded.name,
            kind = excluded.kind,
            email = excluded.email,
            phone = excluded.phone,
            division = excluded.division,
            status = excluded.status,
            updated_at = excluded.updated_at",
        params![
            contact_id,
            org_id,
            name,
            contact_kind_db(draft.kind.contact_kind()),
            email,
            phone,
            division,
            now,
        ],
    )
    .map_err(|error| error.to_string())?;

    tx.execute(
        "INSERT INTO intakes
            (id, org_id, kind, status, current_step, data, contact_id, created_at, updated_at)
         VALUES (?1, ?2, ?3, 'completed', ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(id) DO UPDATE SET
            status = 'completed',
            current_step = excluded.current_step,
            data = excluded.data,
            contact_id = excluded.contact_id,
            updated_at = excluded.updated_at",
        params![
            draft.id,
            org_id,
            kind_db(draft.kind),
            draft.current_step as i64,
            data,
            contact_id,
            draft.created_at,
            now,
        ],
    )
    .map_err(|error| error.to_string())?;

    tx.commit().map_err(|error| error.to_string())?;

    Ok(CompletedIntake {
        contact_id,
        name,
        kind: draft.kind,
    })
}

pub fn delete_intake(conn: &Connection, org_id: &str, id: &str) -> Result<(), String> {
    conn.execute(
        "DELETE FROM intakes WHERE id = ?1 AND org_id = ?2 AND status = 'draft'",
        params![id, org_id],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

fn optional_field(draft: &IntakeDraft, key: &str) -> Option<String> {
    let value = draft.get(key).trim();
    (!value.is_empty()).then(|| value.to_owned())
}

fn kind_db(kind: crate::intake::IntakeKind) -> &'static str {
    match kind {
        crate::intake::IntakeKind::Client => "client",
        crate::intake::IntakeKind::Worker => "worker",
        crate::intake::IntakeKind::Other => "other",
    }
}

fn contact_kind_db(kind: ContactKind) -> &'static str {
    kind.as_db()
}
