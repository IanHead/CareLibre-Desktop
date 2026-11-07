//! Schema, pragmas and migrations for the local cache.

use std::path::Path;

use rusqlite::Connection;

const SCHEMA_VERSION: i64 = 1;

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

    conn.execute_batch(
        "BEGIN;

         CREATE TABLE IF NOT EXISTS contacts (
             id          TEXT PRIMARY KEY,
             org_id      TEXT NOT NULL,
             name        TEXT NOT NULL,
             kind        TEXT NOT NULL CHECK (kind IN ('client','worker','dual')),
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

         PRAGMA user_version = 1;
         COMMIT;",
    )?;

    Ok(())
}
