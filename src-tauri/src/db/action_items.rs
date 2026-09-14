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

/// Adds the tasks a note was read for, as its own words. Returns how many landed.
pub fn add(conn: &Connection, entry_id: &str, tasks: &[(Span, String)]) -> Result<usize> {
    todo!()
}

/// State on the span, never a mutation of the transcript.
pub fn set_done(conn: &Connection, id: &str, done: bool) -> Result<()> {
    let n = conn.execute(
        "UPDATE action_items SET done = ?2 WHERE id = ?1",
        params![id, done],
    )?;
    if n == 0 {
        return Err(crate::error::Error::NotFound(id.to_string()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAID: &str = "I need to buy a pen and some books.";

    fn a_note(conn: &Connection) -> String {
        crate::db::create::create(
            conn,
            crate::db::create::NewEntry {
                transcript: SAID.into(),
                duration_ms: 9_000,
                fingerprint: vec![],
                parent_entry_id: None,
                local_only: None,
                typed: true,
            },
        )
        .unwrap()
        .id
    }

    fn buy() -> (Span, String) {
        (
            Span {
                start: 10,
                end: 34,
                attributed: false,
            },
            "buy a pen and some books".into(),
        )
    }

    #[test]
    fn a_task_lands_as_the_words_it_quotes() {
        let conn = crate::db::open_in_memory().unwrap();
        let id = a_note(&conn);

        assert_eq!(add(&conn, &id, &[buy()]).unwrap(), 1);

        let items = list(&conn).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].entry_id, id);
        assert_eq!(items[0].text, "buy a pen and some books");
        assert_eq!((items[0].span.start, items[0].span.end), (10, 34));
        assert!(!items[0].done);
    }

    /// Correcting a transcript re-anchors tasks by the quote they carry, so a
    /// task stored without one could never follow its words.
    #[test]
    fn the_quote_is_stored_for_re_anchoring() {
        let conn = crate::db::open_in_memory().unwrap();
        let id = a_note(&conn);
        add(&conn, &id, &[buy()]).unwrap();

        let quoted: String = conn
            .query_row("SELECT span_quoted FROM action_items", [], |r| r.get(0))
            .unwrap();
        assert_eq!(quoted, "buy a pen and some books");
    }

    /// A note is read again after its transcript is corrected. A second pass
    /// must not list the same errand twice, or undo one already ticked.
    #[test]
    fn reading_a_note_again_neither_doubles_a_task_nor_unticks_it() {
        let conn = crate::db::open_in_memory().unwrap();
        let id = a_note(&conn);
        add(&conn, &id, &[buy()]).unwrap();
        let first = list(&conn).unwrap()[0].id.clone();
        set_done(&conn, &first, true).unwrap();

        assert_eq!(add(&conn, &id, &[buy()]).unwrap(), 0);

        let items = list(&conn).unwrap();
        assert_eq!(items.len(), 1);
        assert!(items[0].done);
    }
}
