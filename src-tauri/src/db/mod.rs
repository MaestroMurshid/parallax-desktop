pub mod action_items;
pub mod create;
pub mod edges;
pub mod entries;
pub mod import;
pub mod questions;
pub mod sample;
pub mod search;
pub mod settings;
pub mod tags;
pub mod vectors;

use crate::error::Result;
use rusqlite::Connection;
use std::path::Path;

const SCHEMA: &str = include_str!("schema.sql");

/// Bumped whenever `schema.sql` changes shape. `user_version` is a SQLite
/// integer stored in the file header, so the database says which migration it
/// is on without a table of its own.
const SCHEMA_VERSION: i32 = 6;

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
    if current < 3 {
        spans_to_utf16(conn)?;
    }
    if current < 4 {
        // Pure CREATE/ALTER: migrations here are not transactional, and each
        // of these is individually safe to be interrupted by.
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS tags (
                 id         TEXT PRIMARY KEY,
                 name       TEXT NOT NULL UNIQUE,
                 created_at TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS entry_tags (
                 entry_id TEXT NOT NULL REFERENCES entries(id) ON DELETE CASCADE,
                 tag_id   TEXT NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
                 PRIMARY KEY (entry_id, tag_id)
             );
             CREATE INDEX IF NOT EXISTS idx_entry_tags_tag ON entry_tags(tag_id);",
        )?;
        // Generated on every capture since enrichment landed and discarded for
        // want of somewhere to put it.
        let _ = conn.execute_batch("ALTER TABLE entries ADD COLUMN move_phrase TEXT;");
    }
    if current < 5 {
        // Pure CREATE, safe to be interrupted by: migrations here are not
        // transactional. Existing notes have no vector until something
        // backfills them, and `similar` reads that as no candidates.
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS entry_vectors (
                 entry_id   TEXT PRIMARY KEY REFERENCES entries(id) ON DELETE CASCADE,
                 model      TEXT NOT NULL,
                 dims       INTEGER NOT NULL,
                 vec        BLOB NOT NULL,
                 created_at TEXT NOT NULL
             );
             CREATE INDEX IF NOT EXISTS idx_entry_vectors_model ON entry_vectors(model);",
        )?;
    }
    if current < 6 {
        // Existing tags were coined under the one-field design and are
        // grounded in their notes' words, so they are anchors. They will not
        // make candidates, which is what the measurement already said of them.
        let _ =
            conn.execute_batch("ALTER TABLE tags ADD COLUMN kind TEXT NOT NULL DEFAULT 'anchor';");
    }
    if current < SCHEMA_VERSION {
        conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    }
    Ok(())
}

/// Every offset written before version 3 counts UTF-8 bytes; the frontend has
/// always sliced UTF-16. Rewritten in place from the transcript beside it,
/// because the stored quote is what a correction re-finds and it has to keep
/// agreeing with the offsets.
///
/// ASCII is a fixed point, so the common corpus is untouched by design.
fn spans_to_utf16(conn: &Connection) -> Result<()> {
    use crate::text::byte_to_utf16;

    let mut transcripts: std::collections::HashMap<String, String> = Default::default();
    {
        let mut stmt = conn.prepare("SELECT id, transcript FROM entries")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        for row in rows {
            let (id, transcript) = row?;
            transcripts.insert(id, transcript);
        }
    }

    // Every table that stores an offset; missing one leaves a highlight
    // permanently skewed. Addressed by `rowid` because the three primary keys
    // are not the same type and none of these tables is WITHOUT ROWID.
    let tables = [
        ("spans", "start_offset", "end_offset"),
        ("action_items", "span_start", "span_end"),
        ("questions", "span_start", "span_end"),
    ];

    for (table, start_col, end_col) in tables {
        let mut pending: Vec<(i64, u32, u32)> = Vec::new();
        {
            let mut stmt = conn.prepare(&format!(
                "SELECT rowid, entry_id, {start_col}, {end_col} FROM {table}
                 WHERE {start_col} IS NOT NULL AND {end_col} IS NOT NULL"
            ))?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            })?;
            for row in rows {
                let (key_value, entry_id, start, end) = row?;
                let Some(transcript) = transcripts.get(&entry_id) else {
                    continue;
                };
                // Skip the ASCII-only case rather than rewriting it to itself.
                if transcript.is_ascii() {
                    continue;
                }
                pending.push((
                    key_value,
                    byte_to_utf16(transcript, start.max(0) as usize),
                    byte_to_utf16(transcript, end.max(0) as usize),
                ));
            }
        }

        for (key_value, start, end) in pending {
            conn.execute(
                &format!("UPDATE {table} SET {start_col} = ?2, {end_col} = ?3 WHERE rowid = ?1"),
                rusqlite::params![key_value, start, end],
            )?;
        }
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
        assert_eq!(tables, 11);

        // Running it again must not throw: migrate is called on every open.
        migrate(&conn).unwrap();
    }

    /// An install that predates tagging has to open and gain the tables, not
    /// be told to start again. The column is added separately because a
    /// migration here is not transactional -- the tables may land and the
    /// ALTER may not.
    #[test]
    fn version_four_adds_tagging_to_an_existing_corpus() {
        let conn = open_in_memory().unwrap();
        conn.execute(
            "INSERT INTO entries (id, transcript, created_at, x, y, role, register,
             type_id, resolved, title, duration_ms, unfinished, local_only, is_sample)
             VALUES ('e1', 'said', '2024-01-01T00:00:00Z', 0, 0, 'position', 'neutral',
             'position', 0, 't', 40000, 0, 0, 0)",
            [],
        )
        .unwrap();

        conn.execute_batch("DROP TABLE entry_tags; DROP TABLE tags;")
            .unwrap();
        conn.pragma_update(None, "user_version", 3).unwrap();
        migrate(&conn).unwrap();

        let ids = crate::db::tags::upsert(
            &conn,
            &["free-will".to_string()],
            crate::db::tags::Kind::Topic,
        )
        .unwrap();
        crate::db::tags::set_for_entry(&conn, "e1", &ids).unwrap();
        assert_eq!(crate::db::tags::for_entry(&conn, "e1").unwrap().len(), 1);

        let version: i32 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);

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
        assert_eq!(
            read("SELECT span_start, span_end FROM action_items"),
            expected
        );
        assert_eq!(read("SELECT span_start, span_end FROM questions"), expected);
        assert_ne!(expected.0, at as i64, "the fixture has to be non-ASCII");
    }
}
