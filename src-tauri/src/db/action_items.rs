//! Action items. One flat global list, each row linking back to its entry.

use crate::error::Result;
use crate::model::{ActionItem, Span};
use rusqlite::{params, Connection};

pub fn list(conn: &Connection) -> Result<Vec<ActionItem>> {
    let mut stmt = conn.prepare(
        "SELECT a.id, a.entry_id, a.span_start, a.span_end, a.span_attributed, a.text, a.done
         FROM action_items a
         JOIN entries e ON e.id = a.entry_id
         ORDER BY e.created_at ASC, a.rowid ASC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(ActionItem {
            id: row.get(0)?,
            entry_id: row.get(1)?,
            span: Span {
                start: row.get(2)?,
                end: row.get(3)?,
                attributed: row.get(4)?,
            },
            text: row.get(5)?,
            done: row.get(6)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// State on the span, never a mutation of the transcript.
pub fn set_done(conn: &Connection, id: &str, done: bool) -> Result<()> {
    conn.execute(
        "UPDATE action_items SET done = ?2 WHERE id = ?1",
        params![id, done],
    )?;
    Ok(())
}
