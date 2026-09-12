//! Tags, and the index that answers "what else is about this".
//!
//! Internal machinery, never a surface. The user sees proposed connections and
//! accepts or dismisses them; the vocabulary underneath is the app's business,
//! which is why nothing here is tuned for readability -- only for whether two
//! notes about one idea end up sharing a key.

use crate::error::Result;
use rusqlite::{params, Connection};

/// One tag, as stored. `name` is already normalised.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tag {
    pub id: String,
    pub name: String,
}

/// The key two notes have to agree on to ever be compared.
///
/// A model writing "Free Will", "free will" and "free-will" on three notes has
/// written three tags and connected nothing, so the collapse happens here
/// rather than being asked for in the prompt.
pub fn normalise(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut pending_gap = false;
    for ch in name.trim().chars() {
        if ch.is_whitespace() || ch == '-' || ch == '_' {
            // Deferred rather than pushed: runs of separators collapse, and a
            // trailing one never reaches the string at all.
            pending_gap = !out.is_empty();
            continue;
        }
        if pending_gap {
            out.push('-');
            pending_gap = false;
        }
        out.extend(ch.to_lowercase());
    }
    out
}

/// Creates what is missing and returns ids for everything asked for.
///
/// Idempotent because it runs on every capture: the second note about free
/// will must join the existing tag, not make a second one.
pub fn upsert(conn: &Connection, names: &[String]) -> Result<Vec<String>> {
    let mut ids = Vec::with_capacity(names.len());
    for name in names {
        let name = normalise(name);
        if name.is_empty() {
            continue;
        }
        // INSERT OR IGNORE then read back, rather than checking first: two
        // captures enriching at once would both find it missing.
        conn.execute(
            "INSERT OR IGNORE INTO tags (id, name, created_at) VALUES (?1, ?2, ?3)",
            params![
                format!("tag-{}", uuid::Uuid::new_v4()),
                name,
                chrono::Utc::now().to_rfc3339()
            ],
        )?;
        let id: String =
            conn.query_row("SELECT id FROM tags WHERE name = ?1", params![name], |r| {
                r.get(0)
            })?;
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    Ok(ids)
}

/// Replaces an entry's tags wholesale. Re-tagging a corrected transcript
/// should not leave the old vocabulary attached to it.
pub fn set_for_entry(conn: &Connection, entry_id: &str, tag_ids: &[String]) -> Result<()> {
    conn.execute(
        "DELETE FROM entry_tags WHERE entry_id = ?1",
        params![entry_id],
    )?;
    for tag_id in tag_ids {
        conn.execute(
            "INSERT OR IGNORE INTO entry_tags (entry_id, tag_id) VALUES (?1, ?2)",
            params![entry_id, tag_id],
        )?;
    }
    Ok(())
}

pub fn for_entry(conn: &Connection, entry_id: &str) -> Result<Vec<Tag>> {
    let mut stmt = conn.prepare(
        "SELECT t.id, t.name FROM tags t
         JOIN entry_tags et ON et.tag_id = t.id
         WHERE et.entry_id = ?1
         ORDER BY t.name",
    )?;
    let rows = stmt.query_map(params![entry_id], |row| {
        Ok(Tag {
            id: row.get(0)?,
            name: row.get(1)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Every tag in the corpus, for the enum the classifier is constrained to.
pub fn all(conn: &Connection) -> Result<Vec<Tag>> {
    let mut stmt = conn.prepare("SELECT id, name FROM tags ORDER BY name")?;
    let rows = stmt.query_map([], |row| {
        Ok(Tag {
            id: row.get(0)?,
            name: row.get(1)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Candidate notes, most shared tags first.
///
/// The whole point of the join table: this reads only the rows for the handful
/// of tags this entry carries, so it costs the same on a corpus of 500 notes
/// as on one of 20. Scanning was never affordable -- all-pairs on 500 notes is
/// 124,750 model calls.
pub fn sharing(conn: &Connection, entry_id: &str) -> Result<Vec<(String, usize)>> {
    let mut stmt = conn.prepare(
        "SELECT other.entry_id, count(*) AS shared
         FROM entry_tags mine
         JOIN entry_tags other ON other.tag_id = mine.tag_id
         WHERE mine.entry_id = ?1 AND other.entry_id != ?1
         GROUP BY other.entry_id
         ORDER BY shared DESC, other.entry_id ASC",
    )?;
    let rows = stmt.query_map(params![entry_id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as usize))
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;

    fn entry(conn: &Connection, id: &str, created_at: &str) {
        conn.execute(
            "INSERT INTO entries (id, transcript, created_at, x, y, role, register,
             type_id, resolved, title, duration_ms, unfinished, local_only, is_sample)
             VALUES (?1, 'said', ?2, 0, 0, 'position', 'neutral', 'position', 0, 't',
             40000, 0, 0, 0)",
            params![id, created_at],
        )
        .unwrap();
    }

    fn tag(conn: &Connection, entry_id: &str, names: &[&str]) {
        let names: Vec<String> = names.iter().map(|n| normalise(n)).collect();
        let ids = upsert(conn, &names).unwrap();
        set_for_entry(conn, entry_id, &ids).unwrap();
    }

    /// The drift that would stop two notes about one idea ever meeting.
    #[test]
    fn spellings_of_one_idea_collapse_to_one_key() {
        for written in ["Free Will", "free will", "free-will", "  FREE   will  "] {
            assert_eq!(
                normalise(written),
                "free-will",
                "{written} did not collapse"
            );
        }
    }

    #[test]
    fn upsert_is_idempotent() {
        let conn = open_in_memory().unwrap();
        let first = upsert(&conn, &["free-will".to_string()]).unwrap();
        let second = upsert(&conn, &["free-will".to_string()]).unwrap();

        assert_eq!(first, second, "the second note made its own tag");
        assert_eq!(all(&conn).unwrap().len(), 1);
    }

    #[test]
    fn a_note_carries_several_tags() {
        let conn = open_in_memory().unwrap();
        entry(&conn, "e1", "2024-01-01T00:00:00Z");
        tag(&conn, "e1", &["free-will", "determinism", "agency"]);

        let names: Vec<String> = for_entry(&conn, "e1")
            .unwrap()
            .into_iter()
            .map(|t| t.name)
            .collect();
        assert_eq!(names.len(), 3);
        assert!(names.contains(&"determinism".to_string()));
    }

    /// The lookup the proposal path is built on, and the ranking it needs:
    /// two tags in common beats one.
    #[test]
    fn sharing_ranks_by_how_much_is_shared() {
        let conn = open_in_memory().unwrap();
        for (id, at) in [
            ("e1", "2024-01-01T00:00:00Z"),
            ("e2", "2024-02-01T00:00:00Z"),
            ("e3", "2024-03-01T00:00:00Z"),
            ("e4", "2024-04-01T00:00:00Z"),
        ] {
            entry(&conn, id, at);
        }
        tag(&conn, "e1", &["free-will", "determinism"]);
        tag(&conn, "e2", &["free-will", "determinism"]);
        tag(&conn, "e3", &["free-will"]);
        tag(&conn, "e4", &["databases"]);

        let found = sharing(&conn, "e1").unwrap();
        assert_eq!(
            found,
            vec![("e2".to_string(), 2), ("e3".to_string(), 1)],
            "expected e2 first on two shared tags, and e4 absent"
        );
    }

    #[test]
    fn an_entry_is_not_its_own_candidate() {
        let conn = open_in_memory().unwrap();
        entry(&conn, "e1", "2024-01-01T00:00:00Z");
        tag(&conn, "e1", &["free-will"]);

        assert!(sharing(&conn, "e1").unwrap().is_empty());
    }

    /// Re-tagging a corrected transcript must not leave the old vocabulary on
    /// it -- a stale tag is a connection to something the note no longer says.
    #[test]
    fn retagging_replaces_rather_than_accumulates() {
        let conn = open_in_memory().unwrap();
        entry(&conn, "e1", "2024-01-01T00:00:00Z");
        tag(&conn, "e1", &["free-will"]);
        tag(&conn, "e1", &["databases"]);

        let names: Vec<String> = for_entry(&conn, "e1")
            .unwrap()
            .into_iter()
            .map(|t| t.name)
            .collect();
        assert_eq!(names, vec!["databases".to_string()]);
    }

    /// The tag outlives the note. Vocabulary is corpus-wide and a deleted note
    /// must not take the word other notes are filed under with it.
    #[test]
    fn deleting_an_entry_drops_its_pairs_and_keeps_the_tag() {
        let conn = open_in_memory().unwrap();
        entry(&conn, "e1", "2024-01-01T00:00:00Z");
        entry(&conn, "e2", "2024-02-01T00:00:00Z");
        tag(&conn, "e1", &["free-will"]);
        tag(&conn, "e2", &["free-will"]);

        conn.execute("DELETE FROM entries WHERE id = 'e1'", [])
            .unwrap();

        assert_eq!(all(&conn).unwrap().len(), 1, "the tag went with the note");
        let orphans: i64 = conn
            .query_row(
                "SELECT count(*) FROM entry_tags WHERE entry_id = 'e1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(orphans, 0, "the join rows outlived the entry");
    }
}
