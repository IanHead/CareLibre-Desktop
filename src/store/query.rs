//! Read queries against the local cache.

use std::time::Instant;

use rusqlite::{Connection, Row, params, params_from_iter, types::Value};

use super::{ContactCursor, ContactFilter, ContactKind, ContactRow};

#[derive(Clone, Copy, Debug, Default)]
pub struct ContactCounts {
    pub all: usize,
    pub clients: usize,
    pub workers: usize,
    pub dual: usize,
    pub other: usize,
}

impl ContactCounts {
    pub fn for_filter(&self, filter: ContactFilter) -> usize {
        match filter {
            ContactFilter::All => self.all,
            ContactFilter::Clients => self.clients + self.dual,
            ContactFilter::Workers => self.workers + self.dual,
            ContactFilter::Other => self.other,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ContactPage {
    pub rows: Vec<ContactRow>,
    pub total: usize,
    pub next_cursor: Option<ContactCursor>,
    pub counts: ContactCounts,
    pub query_ms: f32,
}

#[derive(Clone, Debug)]
pub struct ShiftRow {
    pub id: String,
    pub worker_id: String,
    pub client_id: String,
    pub starts_at: i64,
    pub ends_at: i64,
    pub status: String,
}

/// Fetch one page of contacts, ordered by name. Use `after` for keyset paging.
pub fn query_contacts(
    conn: &Connection,
    org_id: &str,
    filter: ContactFilter,
    search: &str,
    after: Option<&ContactCursor>,
    limit: usize,
) -> rusqlite::Result<ContactPage> {
    let counts = contact_counts(conn, org_id)?;
    query_contacts_with(conn, org_id, filter, search, after, limit, counts, None)
}

/// Query a page using precomputed totals/counts. The worker caches both so
/// scrolling does not re-count the table on every page.
#[allow(clippy::too_many_arguments)]
pub(crate) fn query_contacts_with(
    conn: &Connection,
    org_id: &str,
    filter: ContactFilter,
    search: &str,
    after: Option<&ContactCursor>,
    limit: usize,
    counts: ContactCounts,
    known_total: Option<usize>,
) -> rusqlite::Result<ContactPage> {
    let started = Instant::now();

    let search = search.trim();
    let fts = (!search.is_empty()).then(|| fts_match_query(search));
    let total = match known_total {
        Some(total) => total,
        None => count_contacts(conn, org_id, filter, fts.as_deref())?,
    };

    let kinds = filter.kinds_sql();
    let has_cursor = i64::from(after.is_some());
    let after_name = after.map(|cursor| cursor.name.as_str()).unwrap_or("");
    let after_id = after.map(|cursor| cursor.id.as_str()).unwrap_or("");
    let limit = i64::try_from(limit).unwrap_or(i64::MAX);

    let mut values: Vec<Value> = Vec::new();

    let sql = if let Some(fts) = &fts {
        values.push(Value::Text(fts.clone()));
        values.push(Value::Text(org_id.to_owned()));
        values.push(Value::Integer(has_cursor));
        values.push(Value::Text(after_name.to_owned()));
        values.push(Value::Text(after_id.to_owned()));
        values.push(Value::Integer(limit));

        format!(
            "SELECT c.id, c.name, c.kind, c.email, c.phone, c.division, c.status
             FROM contacts_fts f
             JOIN contacts c ON c.rowid = f.rowid
             WHERE contacts_fts MATCH ?1
               AND c.org_id = ?2
               AND c.kind IN ({kinds})
               AND (?3 = 0 OR (c.name COLLATE NOCASE > ?4 COLLATE NOCASE
                    OR (c.name = ?4 COLLATE NOCASE AND c.id > ?5)))
             ORDER BY c.name COLLATE NOCASE, c.id
             LIMIT ?6"
        )
    } else {
        values.push(Value::Text(org_id.to_owned()));
        values.push(Value::Integer(has_cursor));
        values.push(Value::Text(after_name.to_owned()));
        values.push(Value::Text(after_id.to_owned()));
        values.push(Value::Integer(limit));

        format!(
            "SELECT id, name, kind, email, phone, division, status
             FROM contacts
             WHERE org_id = ?1
               AND kind IN ({kinds})
               AND (?2 = 0 OR (name COLLATE NOCASE > ?3 COLLATE NOCASE
                    OR (name = ?3 COLLATE NOCASE AND id > ?4)))
             ORDER BY name COLLATE NOCASE, id
             LIMIT ?5"
        )
    };

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(params_from_iter(values.iter()), map_contact)?
        .collect::<Result<Vec<_>, _>>()?;

    let next_cursor = if rows.len() == usize::try_from(limit).unwrap_or(usize::MAX) {
        rows.last().map(|row| ContactCursor {
            name: row.name.clone(),
            id: row.id.clone(),
        })
    } else {
        None
    };

    Ok(ContactPage {
        rows,
        total,
        next_cursor,
        counts,
        query_ms: started.elapsed().as_secs_f32() * 1000.0,
    })
}

pub fn contact_counts(conn: &Connection, org_id: &str) -> rusqlite::Result<ContactCounts> {
    conn.query_row(
        "SELECT COUNT(*),
                COALESCE(SUM(CASE WHEN kind = 'client' THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN kind = 'worker' THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN kind = 'dual' THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN kind = 'other' THEN 1 ELSE 0 END), 0)
         FROM contacts
         WHERE org_id = ?1",
        [org_id],
        |row| {
            Ok(ContactCounts {
                all: row.get::<_, i64>(0)? as usize,
                clients: row.get::<_, i64>(1)? as usize,
                workers: row.get::<_, i64>(2)? as usize,
                dual: row.get::<_, i64>(3)? as usize,
                other: row.get::<_, i64>(4)? as usize,
            })
        },
    )
}

fn count_contacts(
    conn: &Connection,
    org_id: &str,
    filter: ContactFilter,
    fts: Option<&str>,
) -> rusqlite::Result<usize> {
    let kinds = filter.kinds_sql();
    let count = if let Some(fts) = fts {
        conn.query_row(
            &format!(
                "SELECT COUNT(*)
                 FROM contacts_fts f
                 JOIN contacts c ON c.rowid = f.rowid
                 WHERE contacts_fts MATCH ?1 AND c.org_id = ?2 AND c.kind IN ({kinds})"
            ),
            params![fts, org_id],
            |row| row.get::<_, i64>(0),
        )?
    } else {
        conn.query_row(
            &format!("SELECT COUNT(*) FROM contacts WHERE org_id = ?1 AND kind IN ({kinds})"),
            [org_id],
            |row| row.get::<_, i64>(0),
        )?
    };

    Ok(count as usize)
}

fn fts_match_query(input: &str) -> String {
    input
        .split_whitespace()
        .map(|term| format!("\"{}\"*", term.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn map_contact(row: &Row<'_>) -> rusqlite::Result<ContactRow> {
    Ok(ContactRow {
        id: row.get(0)?,
        name: row.get(1)?,
        kind: ContactKind::from_db(&row.get::<_, String>(2)?),
        email: row.get(3)?,
        phone: row.get(4)?,
        division: row.get(5)?,
        status: row.get(6)?,
    })
}

pub fn count_shifts_in_range(
    conn: &Connection,
    org_id: &str,
    from_ms: i64,
    to_ms: i64,
) -> rusqlite::Result<usize> {
    let count = conn.query_row(
        "SELECT COUNT(*) FROM shifts
         WHERE org_id = ?1 AND starts_at >= ?2 AND starts_at < ?3",
        params![org_id, from_ms, to_ms],
        |row| row.get::<_, i64>(0),
    )?;
    Ok(count as usize)
}

/// Shifts overlapping a time range, for the roster and benchmarks.
pub fn shifts_in_range(
    conn: &Connection,
    org_id: &str,
    from_ms: i64,
    to_ms: i64,
    limit: usize,
) -> rusqlite::Result<Vec<ShiftRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, worker_id, client_id, starts_at, ends_at, status
         FROM shifts
         WHERE org_id = ?1 AND starts_at >= ?2 AND starts_at < ?3
         ORDER BY starts_at, id
         LIMIT ?4",
    )?;
    let rows = stmt.query_map(params![org_id, from_ms, to_ms, limit as i64], |row| {
        Ok(ShiftRow {
            id: row.get(0)?,
            worker_id: row.get(1)?,
            client_id: row.get(2)?,
            starts_at: row.get(3)?,
            ends_at: row.get(4)?,
            status: row.get(5)?,
        })
    })?;

    rows.collect()
}
