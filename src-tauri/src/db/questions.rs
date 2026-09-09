//! Questions accumulate on an entry and are never replaced. A bad one is
//! dismissed, which keeps it in the record and stops it counting as open.

use crate::error::Result;
use crate::model::{Question, Span};
use rusqlite::{params, Connection, Row};

fn row_to_question(row: &Row) -> rusqlite::Result<Question> {
    let start: Option<u32> = row.get(3)?;
    let end: Option<u32> = row.get(4)?;
    Ok(Question {
        id: row.get(0)?,
        entry_id: row.get(1)?,
        text: row.get(2)?,
        // Offsets go null when a correction loses the anchor. The question
        // stays; it renders without a highlight.
        span: match (start, end) {
            (Some(start), Some(end)) => Some(Span {
                start,
                end,
                attributed: row.get::<_, Option<bool>>(5)?.unwrap_or(false),
            }),
            _ => None,
        },
        answered: row.get(6)?,
        dismissed: row.get(7)?,
        provider_name: row.get(8)?,
        created_at: row.get(9)?,
    })
}

const COLUMNS: &str = "id, entry_id, text, span_start, span_end, span_attributed,
                       answered, dismissed, provider_name, created_at";

/// Every question, so `loadCorpus` costs one query rather than one per entry.
pub fn list(conn: &Connection) -> Result<Vec<Question>> {
    let mut stmt =
        conn.prepare(&format!("SELECT {COLUMNS} FROM questions ORDER BY created_at ASC"))?;
    let rows = stmt.query_map([], row_to_question)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub fn list_for(conn: &Connection, entry_id: &str) -> Result<Vec<Question>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM questions WHERE entry_id = ?1 ORDER BY created_at ASC"
    ))?;
    let rows = stmt.query_map(params![entry_id], row_to_question)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub fn insert(conn: &Connection, question: &Question, transcript: &str) -> Result<()> {
    let quoted = question.span.as_ref().map(|s| {
        let start = s.start as usize;
        let end = (s.end as usize).min(transcript.len());
        if start < end && transcript.is_char_boundary(start) && transcript.is_char_boundary(end) {
            transcript[start..end].to_string()
        } else {
            String::new()
        }
    });

    conn.execute(
        "INSERT OR IGNORE INTO questions
         (id, entry_id, text, span_start, span_end, span_attributed, span_quoted,
          answered, dismissed, provider_name, created_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
        params![
            question.id,
            question.entry_id,
            question.text,
            question.span.as_ref().map(|s| s.start),
            question.span.as_ref().map(|s| s.end),
            question.span.as_ref().map(|s| s.attributed),
            quoted,
            question.answered,
            question.dismissed,
            question.provider_name,
            question.created_at,
        ],
    )?;
    Ok(())
}

/// Struck out, not deleted. Section 3.4 bans regeneration, so dismissal is the
/// only exit a bad question has -- and those dismissals are the negative
/// examples a local prompt bank needs.
pub fn dismiss(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("UPDATE questions SET dismissed = 1 WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn mark_answered(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("UPDATE questions SET answered = 1 WHERE id = ?1", params![id])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;

    fn seed_entry(conn: &Connection) {
        conn.execute(
            "INSERT INTO entries (id, transcript, created_at, x, y, role, register,
             type_id, resolved, title, duration_ms, unfinished, local_only, is_sample)
             VALUES ('e1', 'Indexes trade writes for reads.', '2024-01-01T00:00:00Z',
             0, 0, 'position', 'neutral', 'position', 0, 'title', 40000, 0, 0, 0)",
            [],
        )
        .unwrap();
    }

    fn question(id: &str) -> Question {
        Question {
            id: id.into(),
            entry_id: "e1".into(),
            text: "What does that cost?".into(),
            span: Some(Span { start: 8, end: 13, attributed: false }),
            answered: false,
            dismissed: false,
            provider_name: "llama-server".into(),
            created_at: "2024-01-02T00:00:00Z".into(),
        }
    }

    /// Section 3.4 with teeth: asking again adds, it does not replace.
    #[test]
    fn questions_accumulate() {
        let conn = open_in_memory().unwrap();
        seed_entry(&conn);
        insert(&conn, &question("q1"), "Indexes trade writes for reads.").unwrap();
        insert(&conn, &question("q2"), "Indexes trade writes for reads.").unwrap();
        assert_eq!(list_for(&conn, "e1").unwrap().len(), 2);
    }

    #[test]
    fn dismissing_keeps_the_question() {
        let conn = open_in_memory().unwrap();
        seed_entry(&conn);
        insert(&conn, &question("q1"), "Indexes trade writes for reads.").unwrap();
        dismiss(&conn, "q1").unwrap();

        let all = list_for(&conn, "e1").unwrap();
        assert_eq!(all.len(), 1);
        assert!(all[0].dismissed);
    }
}
