pub mod action_items;
pub mod create;
pub mod edges;
pub mod entries;
pub mod questions;
pub mod sample;
pub mod search;
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

    /// Every span written before version 3 is a byte offset. An existing
    /// corpus has to arrive in UTF-16 without being reloaded, and the only
    /// thing that can convert it is the transcript sitting next to it.
    #[test]
    fn version_three_rewrites_byte_offsets_as_utf16() {
        let conn = open_in_memory().unwrap();
        let transcript = "«Наблюдаемость важнее логов», — сказал он.";
        let quote = "важнее логов";
        let at = transcript.find(quote).unwrap();

        conn.execute(
            "INSERT INTO entries (id, transcript, created_at, x, y, role, register,
             type_id, resolved, title, duration_ms, unfinished, local_only, is_sample)
             VALUES ('e1', ?1, '2024-01-01T00:00:00Z', 0, 0, 'position', 'neutral',
             'position', 0, 't', 40000, 0, 0, 0)",
            rusqlite::params![transcript],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO spans (entry_id, start_offset, end_offset, attributed, quoted_text)
             VALUES ('e1', ?1, ?2, 1, ?3)",
            rusqlite::params![at as i64, (at + quote.len()) as i64, quote],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO action_items (id, entry_id, span_start, span_end,
             span_attributed, span_quoted, text, done)
             VALUES ('a1', 'e1', ?1, ?2, 0, ?3, 'check it', 0)",
            rusqlite::params![at as i64, (at + quote.len()) as i64, quote],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO questions (id, entry_id, text, span_start, span_end,
             span_attributed, span_quoted, answered, dismissed, provider_name, created_at)
             VALUES ('q1', 'e1', 'where does it stop?', ?1, ?2, 0, ?3, 0, 0, 'p',
             '2024-01-02T00:00:00Z')",
            rusqlite::params![at as i64, (at + quote.len()) as i64, quote],
        )
        .unwrap();

        conn.pragma_update(None, "user_version", 2).unwrap();
        migrate(&conn).unwrap();

        let expected = (
            crate::text::byte_to_utf16(transcript, at) as i64,
            crate::text::byte_to_utf16(transcript, at + quote.len()) as i64,
        );
        let read = |sql: &str| -> (i64, i64) {
            conn.query_row(sql, [], |row| Ok((row.get(0)?, row.get(1)?)))
                .unwrap()
        };
        assert_eq!(read("SELECT start_offset, end_offset FROM spans"), expected);
        assert_eq!(read("SELECT span_start, span_end FROM action_items"), expected);
        assert_eq!(read("SELECT span_start, span_end FROM questions"), expected);
        assert_ne!(expected.0, at as i64, "the fixture has to be non-ASCII");
    }
}
