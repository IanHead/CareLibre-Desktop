//! Schema, pragmas and migrations for the local cache.

use std::path::Path;

use rusqlite::Connection;

const SCHEMA_VERSION: i64 = 3;

const CONTACTS_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS contacts (
    id          TEXT PRIMARY KEY,
    org_id      TEXT NOT NULL,
    name        TEXT NOT NULL,
    kind        TEXT NOT NULL CHECK (kind IN ('client','worker','dual','other')),
    email       TEXT,
    phone       TEXT,
    division    TEXT,
    status      TEXT NOT NULL,
    updated_at  INTEGER NOT NULL,
    version     INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS contacts_org_name
    ON contacts(org_id, name COLLATE NOCASE, id);
CREATE INDEX IF NOT EXISTS contacts_org_kind
    ON contacts(org_id, kind);

CREATE VIRTUAL TABLE IF NOT EXISTS contacts_fts USING fts5(
    name,
    email,
    phone,
    content='contacts',
    content_rowid='rowid'
);

CREATE TRIGGER IF NOT EXISTS contacts_ai AFTER INSERT ON contacts BEGIN
    INSERT INTO contacts_fts(rowid, name, email, phone)
    VALUES (new.rowid, new.name, new.email, new.phone);
END;

CREATE TRIGGER IF NOT EXISTS contacts_ad AFTER DELETE ON contacts BEGIN
    INSERT INTO contacts_fts(contacts_fts, rowid, name, email, phone)
    VALUES ('delete', old.rowid, old.name, old.email, old.phone);
END;

CREATE TRIGGER IF NOT EXISTS contacts_au AFTER UPDATE ON contacts BEGIN
    INSERT INTO contacts_fts(contacts_fts, rowid, name, email, phone)
    VALUES ('delete', old.rowid, old.name, old.email, old.phone);
    INSERT INTO contacts_fts(rowid, name, email, phone)
    VALUES (new.rowid, new.name, new.email, new.phone);
END;

CREATE TABLE IF NOT EXISTS shifts (
    id          TEXT PRIMARY KEY,
    org_id      TEXT NOT NULL,
    worker_id   TEXT NOT NULL,
    client_id   TEXT NOT NULL,
    starts_at   INTEGER NOT NULL,
    ends_at     INTEGER NOT NULL,
    status      TEXT NOT NULL,
    version     INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS shifts_org_start
    ON shifts(org_id, starts_at);
CREATE INDEX IF NOT EXISTS shifts_worker_start
    ON shifts(worker_id, starts_at);
CREATE INDEX IF NOT EXISTS shifts_client_start
    ON shifts(client_id, starts_at);

CREATE TABLE IF NOT EXISTS intakes (
    id           TEXT PRIMARY KEY,
    org_id       TEXT NOT NULL,
    kind         TEXT NOT NULL,
    status       TEXT NOT NULL CHECK (status IN ('draft','completed')),
    current_step INTEGER NOT NULL,
    data         TEXT NOT NULL,
    contact_id   TEXT,
    created_at   INTEGER NOT NULL,
    updated_at   INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS intakes_org_updated
    ON intakes(org_id, status, updated_at DESC);
"#;

const MIGRATE_V1_TO_V2: &str = r#"
BEGIN;

DROP TRIGGER IF EXISTS contacts_ai;
DROP TRIGGER IF EXISTS contacts_ad;
DROP TRIGGER IF EXISTS contacts_au;
DROP TABLE IF EXISTS contacts_fts;
DROP INDEX IF EXISTS contacts_org_name;
DROP INDEX IF EXISTS contacts_org_kind;

ALTER TABLE contacts RENAME TO contacts_v1;

CREATE TABLE contacts (
    id          TEXT PRIMARY KEY,
    org_id      TEXT NOT NULL,
    name        TEXT NOT NULL,
    kind        TEXT NOT NULL CHECK (kind IN ('client','worker','dual','other')),
    email       TEXT,
    phone       TEXT,
    division    TEXT,
    status      TEXT NOT NULL,
    updated_at  INTEGER NOT NULL,
    version     INTEGER NOT NULL
);

INSERT INTO contacts (id, org_id, name, kind, email, phone, division, status, updated_at, version)
SELECT id, org_id, name, kind, email, phone, division, status, updated_at, version
FROM contacts_v1;

DROP TABLE contacts_v1;

CREATE INDEX contacts_org_name
    ON contacts(org_id, name COLLATE NOCASE, id);
CREATE INDEX contacts_org_kind
    ON contacts(org_id, kind);

CREATE VIRTUAL TABLE contacts_fts USING fts5(
    name,
    email,
    phone,
    content='contacts',
    content_rowid='rowid'
);

CREATE TRIGGER contacts_ai AFTER INSERT ON contacts BEGIN
    INSERT INTO contacts_fts(rowid, name, email, phone)
    VALUES (new.rowid, new.name, new.email, new.phone);
END;

CREATE TRIGGER contacts_ad AFTER DELETE ON contacts BEGIN
    INSERT INTO contacts_fts(contacts_fts, rowid, name, email, phone)
    VALUES ('delete', old.rowid, old.name, old.email, old.phone);
END;

CREATE TRIGGER contacts_au AFTER UPDATE ON contacts BEGIN
    INSERT INTO contacts_fts(contacts_fts, rowid, name, email, phone)
    VALUES ('delete', old.rowid, old.name, old.email, old.phone);
    INSERT INTO contacts_fts(rowid, name, email, phone)
    VALUES (new.rowid, new.name, new.email, new.phone);
END;

INSERT INTO contacts_fts(contacts_fts) VALUES ('rebuild');

CREATE TABLE IF NOT EXISTS intakes (
    id           TEXT PRIMARY KEY,
    org_id       TEXT NOT NULL,
    kind         TEXT NOT NULL,
    status       TEXT NOT NULL CHECK (status IN ('draft','completed')),
    current_step INTEGER NOT NULL,
    data         TEXT NOT NULL,
    contact_id   TEXT,
    created_at   INTEGER NOT NULL,
    updated_at   INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS intakes_org_updated
    ON intakes(org_id, status, updated_at DESC);

PRAGMA user_version = 2;
COMMIT;
"#;

const CUSTOM_FIELDS_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS contact_profiles (
    contact_id TEXT PRIMARY KEY,
    data       TEXT NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS custom_fields (
    id          TEXT PRIMARY KEY,
    org_id      TEXT NOT NULL,
    entity      TEXT NOT NULL DEFAULT 'contact',
    key         TEXT NOT NULL,
    label       TEXT NOT NULL,
    field_type  TEXT NOT NULL,
    options     TEXT NOT NULL DEFAULT '[]',
    section     TEXT,
    required    INTEGER NOT NULL DEFAULT 0,
    enabled     INTEGER NOT NULL DEFAULT 1,
    sort_order  INTEGER NOT NULL DEFAULT 0,
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS custom_fields_org_key
    ON custom_fields(org_id, entity, key);
CREATE INDEX IF NOT EXISTS custom_fields_org_order
    ON custom_fields(org_id, entity, enabled, sort_order, id);

CREATE TABLE IF NOT EXISTS contact_custom_values (
    contact_id TEXT NOT NULL,
    field_id   TEXT NOT NULL,
    value      TEXT NOT NULL,
    PRIMARY KEY (contact_id, field_id),
    FOREIGN KEY (contact_id) REFERENCES contacts(id) ON DELETE CASCADE,
    FOREIGN KEY (field_id) REFERENCES custom_fields(id) ON DELETE CASCADE
);

INSERT OR IGNORE INTO custom_fields
    (id, org_id, entity, key, label, field_type, options, section, required, enabled, sort_order, created_at, updated_at)
VALUES
    ('demo-cf-medicare', 'demo', 'contact', 'medicare_number', 'Medicare number', 'text', '[]', 'Identifiers', 0, 1, 10, 0, 0),
    ('demo-cf-cultural', 'demo', 'contact', 'cultural_support', 'Cultural support needs', 'multiline', '[]', 'Support needs', 0, 1, 20, 0, 0),
    ('demo-cf-comms', 'demo', 'contact', 'preferred_communication', 'Preferred communication', 'select', '["Phone","SMS","Email","In person"]', 'Contact preferences', 0, 1, 30, 0, 0),
    ('demo-cf-shirt', 'demo', 'contact', 'shirt_size', 'Shirt size', 'select', '["XS","S","M","L","XL","2XL","3XL"]', 'Worker details', 0, 1, 40, 0, 0);
"#;

const MIGRATE_V2_TO_V3: &str = r#"
BEGIN;

CREATE TABLE IF NOT EXISTS contact_profiles (
    contact_id TEXT PRIMARY KEY,
    data       TEXT NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS custom_fields (
    id          TEXT PRIMARY KEY,
    org_id      TEXT NOT NULL,
    entity      TEXT NOT NULL DEFAULT 'contact',
    key         TEXT NOT NULL,
    label       TEXT NOT NULL,
    field_type  TEXT NOT NULL,
    options     TEXT NOT NULL DEFAULT '[]',
    section     TEXT,
    required    INTEGER NOT NULL DEFAULT 0,
    enabled     INTEGER NOT NULL DEFAULT 1,
    sort_order  INTEGER NOT NULL DEFAULT 0,
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS custom_fields_org_key
    ON custom_fields(org_id, entity, key);
CREATE INDEX IF NOT EXISTS custom_fields_org_order
    ON custom_fields(org_id, entity, enabled, sort_order, id);

CREATE TABLE IF NOT EXISTS contact_custom_values (
    contact_id TEXT NOT NULL,
    field_id   TEXT NOT NULL,
    value      TEXT NOT NULL,
    PRIMARY KEY (contact_id, field_id),
    FOREIGN KEY (contact_id) REFERENCES contacts(id) ON DELETE CASCADE,
    FOREIGN KEY (field_id) REFERENCES custom_fields(id) ON DELETE CASCADE
);

INSERT OR IGNORE INTO custom_fields
    (id, org_id, entity, key, label, field_type, options, section, required, enabled, sort_order, created_at, updated_at)
VALUES
    ('demo-cf-medicare', 'demo', 'contact', 'medicare_number', 'Medicare number', 'text', '[]', 'Identifiers', 0, 1, 10, 0, 0),
    ('demo-cf-cultural', 'demo', 'contact', 'cultural_support', 'Cultural support needs', 'multiline', '[]', 'Support needs', 0, 1, 20, 0, 0),
    ('demo-cf-comms', 'demo', 'contact', 'preferred_communication', 'Preferred communication', 'select', '["Phone","SMS","Email","In person"]', 'Contact preferences', 0, 1, 30, 0, 0),
    ('demo-cf-shirt', 'demo', 'contact', 'shirt_size', 'Shirt size', 'select', '["XS","S","M","L","XL","2XL","3XL"]', 'Worker details', 0, 1, 40, 0, 0);

INSERT OR IGNORE INTO contact_profiles (contact_id, data, updated_at)
SELECT contact_id, json_extract(data, '$.fields'), updated_at
FROM intakes
WHERE status = 'completed'
  AND contact_id IS NOT NULL
  AND json_extract(data, '$.fields') IS NOT NULL;

PRAGMA user_version = 3;
COMMIT;
"#;

pub fn open(path: &Path) -> rusqlite::Result<Connection> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| {
            rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CANTOPEN),
                Some(format!("could not create {}: {err}", parent.display())),
            )
        })?;
    }

    let conn = Connection::open(path)?;
    configure(&conn)?;
    migrate(&conn)?;
    Ok(conn)
}

pub fn open_in_memory() -> rusqlite::Result<Connection> {
    let conn = Connection::open_in_memory()?;
    configure(&conn)?;
    migrate(&conn)?;
    Ok(conn)
}

fn configure(conn: &Connection) -> rusqlite::Result<()> {
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA foreign_keys = ON;
         PRAGMA temp_store = MEMORY;
         PRAGMA cache_size = -65536;
         PRAGMA mmap_size = 268435456;",
    )?;
    Ok(())
}

fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if version >= SCHEMA_VERSION {
        return Ok(());
    }

    if version == 0 {
        conn.execute_batch(&format!(
            "BEGIN; {CONTACTS_SCHEMA} {CUSTOM_FIELDS_SCHEMA} PRAGMA user_version = {SCHEMA_VERSION}; COMMIT;"
        ))?;
        return Ok(());
    }

    if version < 2 {
        conn.execute_batch(MIGRATE_V1_TO_V2)?;
    }
    if version < 3 {
        conn.execute_batch(MIGRATE_V2_TO_V3)?;
    }

    Ok(())
}
