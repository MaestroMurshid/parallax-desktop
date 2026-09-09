pub mod action_items;
pub mod create;
pub mod edges;
pub mod entries;
pub mod questions;
pub mod settings;

use crate::error::Result;
use rusqlite::Connection;
use std::path::Path;

const SCHEMA: &str = include_str!("schema.sql");

/// Bumped whenever `schema.sql` changes shape. `user_version` is a SQLite
/// integer stored in the file header, so the database says which migration it
/// is on without a table of its own.
const SCHEMA_VERSION: i32 = 2;

pub fn open(path: &Path) -> Result<Connection> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let conn = Connection::open(path)?;
    // Off by default in SQLite, and every cascade in the schema depends on it.
    conn.pragma_update(None, "foreign_keys", "ON")?;
    // The corpus is written from the capture path while the canvas reads it.
    conn.pragma_update(None, "journal_mode", "WAL")?;
    migrate(&conn)?;
    Ok(conn)
}

/// In-memory database, for tests.
pub fn open_in_memory() -> Result<Connection> {
    let conn = Connection::open_in_memory()?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    migrate(&conn)?;
    Ok(conn)
}

fn migrate(conn: &Connection) -> Result<()> {
    let current: i32 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if current == 0 {
        conn.execute_batch(SCHEMA)?;
        conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        return Ok(());
    }
    // Steps run from wherever the file happens to be. An install that predates
    // a column has to open, not be told to start again.
    if current < 2 {
        conn.execute_batch(
            "ALTER TABLE action_items ADD COLUMN stale INTEGER NOT NULL DEFAULT 0;",
        )?;
    }
    if current < SCHEMA_VERSION {
        conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_creates_the_schema_once() {
        let conn = open_in_memory().unwrap();
        let version: i32 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);

        let tables: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(tables, 8);

        // Running it again must not throw: migrate is called on every open.
        migrate(&conn).unwrap();
    }
}
