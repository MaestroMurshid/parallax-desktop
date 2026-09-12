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
    pub kind: Kind,
}

/// Two jobs that cannot be done by one field, measured: a key grounded in the
/// note's own words is too specific to ever collide, and one abstract enough
/// to collide lands on the whole corpus. So they are separate fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Specific, grounded, and never a retrieval key -- over sixteen fixtures
    /// anchors shared 0 of 120 pairs. They are what the note is about, for the
    /// reader and for the MDX frontmatter.
    Anchor,
    /// Broad, allowed to be ungrounded, and the only thing that makes two
    /// notes candidates: 20 of 120 pairs and 6 of 12 authored edges.
    Topic,
}

impl Kind {
    fn as_str(self) -> &'static str {
        match self {
            Kind::Anchor => "anchor",
            Kind::Topic => "topic",
        }
    }

    fn from_str(s: &str) -> Kind {
        match s {
            "topic" => Kind::Topic,
            _ => Kind::Anchor,
        }
    }
}

/// A topic that is the same shelf under another name joins the earlier one.
///
/// Measured: the model returned `databases` and `database-systems` on one
/// note. Folding on the **head** word rather than any shared word is what
/// keeps this safe -- an earlier attempt folded `ai-systems` into
/// `distributed-systems` on the generic tail word "systems", which merges two
/// fields that are not the same shelf at all.
pub fn fold_topic(existing: &[String], candidate: &str) -> String {
    let head = |name: &str| -> String {
        name.split('-')
            .find(|w| w.len() > 2)
            .map(|w| w.chars().take(5).collect())
            .unwrap_or_default()
    };
    let want = head(candidate);
    if want.is_empty() {
        return candidate.to_string();
    }
    existing
        .iter()
        .find(|e| head(e) == want)
        .cloned()
        .unwrap_or_else(|| candidate.to_string())
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
pub fn upsert(conn: &Connection, names: &[String], kind: Kind) -> Result<Vec<String>> {
    let mut ids = Vec::with_capacity(names.len());
    for name in names {
        let name = normalise(name);
        if name.is_empty() {
            continue;
        }
        // INSERT OR IGNORE then read back, rather than checking first: two
        // captures enriching at once would both find it missing.
        conn.execute(
            "INSERT OR IGNORE INTO tags (id, name, kind, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![
                format!("tag-{}", uuid::Uuid::new_v4()),
                name,
                kind.as_str(),
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
        "SELECT t.id, t.name, t.kind FROM tags t
         JOIN entry_tags et ON et.tag_id = t.id
         WHERE et.entry_id = ?1
         ORDER BY t.kind, t.name",
    )?;
    let rows = stmt.query_map(params![entry_id], |row| {
        Ok(Tag {
            id: row.get(0)?,
            name: row.get(1)?,
            kind: Kind::from_str(&row.get::<_, String>(2)?),
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Every tag in the corpus. Not sent to the model -- offering the vocabulary
/// as an enum stopped it coining at all -- but the topics are what `fold_topic`
/// is checked against, and both kinds go into the MDX frontmatter.
pub fn all(conn: &Connection) -> Result<Vec<Tag>> {
    let mut stmt = conn.prepare("SELECT id, name, kind FROM tags ORDER BY name")?;
    let rows = stmt.query_map([], |row| {
        Ok(Tag {
            id: row.get(0)?,
            name: row.get(1)?,
            kind: Kind::from_str(&row.get::<_, String>(2)?),
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Candidate notes, most shared topics first.
///
/// Topics only. Anchors are grounded in the note's own words and therefore
/// never collide -- measured at 0 of 120 pairs -- so joining on them returns
/// nothing and costs a scan to do it.
///
/// A topic carried by most of the corpus is ignored rather than prevented: it
/// selects everything, so it narrows nothing, and refusing to rely on it at
/// read time is cheaper than trying to stop the model coining it. The floor
/// matters -- on a young corpus every topic is on "most" of it, and applying
/// the rule there would mean nothing ever connects until the corpus grows.
pub fn sharing(conn: &Connection, entry_id: &str) -> Result<Vec<(String, usize)>> {
    let mut stmt = conn.prepare(
        "WITH corpus AS (SELECT count(*) AS n FROM entries),
              usable AS (
                  SELECT et.tag_id
                  FROM entry_tags et
                  JOIN tags t ON t.id = et.tag_id
                  WHERE t.kind = 'topic'
                  GROUP BY et.tag_id
                  HAVING (SELECT n FROM corpus) < 10
                      OR count(*) * 2 <= (SELECT n FROM corpus)
              )
         SELECT other.entry_id, count(*) AS shared
         FROM entry_tags mine
         JOIN entry_tags other ON other.tag_id = mine.tag_id
         WHERE mine.entry_id = ?1
           AND other.entry_id != ?1
           AND mine.tag_id IN (SELECT tag_id FROM usable)
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

    /// Topics by default: they are the kind the candidate lookup reads, so a
    /// helper that wrote anchors would make every sharing test vacuous.
    fn tag(conn: &Connection, entry_id: &str, names: &[&str]) {
        tag_as(conn, entry_id, names, Kind::Topic);
    }

    fn tag_as(conn: &Connection, entry_id: &str, names: &[&str], kind: Kind) {
        let names: Vec<String> = names.iter().map(|n| normalise(n)).collect();
        let ids = upsert(conn, &names, kind).unwrap();
        set_for_entry(conn, entry_id, &ids).unwrap();
    }

    /// Both kinds at once, which is what a real capture writes.
    fn file(conn: &Connection, entry_id: &str, anchors: &[&str], topics: &[&str]) {
        let a: Vec<String> = anchors.iter().map(|n| normalise(n)).collect();
        let t: Vec<String> = topics.iter().map(|n| normalise(n)).collect();
        let mut ids = upsert(conn, &a, Kind::Anchor).unwrap();
        ids.extend(upsert(conn, &t, Kind::Topic).unwrap());
        set_for_entry(conn, entry_id, &ids).unwrap();
    }

    /// The same shelf under two names, which the model really does return.
    #[test]
    fn a_near_duplicate_topic_folds_onto_the_earlier_one() {
        let have = vec!["databases".to_string()];
        assert_eq!(fold_topic(&have, "database-systems"), "databases");
        assert_eq!(fold_topic(&have, "database"), "databases");
    }

    /// The merge that must NOT happen. An earlier rule folded on any shared
    /// word and put `ai-systems` under `distributed-systems` on "systems",
    /// which is two different fields sharing a generic noun.
    #[test]
    fn a_shared_tail_word_is_not_the_same_shelf() {
        let have = vec!["distributed-systems".to_string()];
        assert_eq!(fold_topic(&have, "ai-systems"), "ai-systems");
    }

    #[test]
    fn an_unrelated_topic_is_left_alone() {
        let have = vec!["databases".to_string(), "epistemology".to_string()];
        assert_eq!(fold_topic(&have, "machine-learning"), "machine-learning");
    }

    /// The measured reason the split exists: anchors are grounded in the note's
    /// own words, so they never collide -- 0 of 120 pairs over the fixtures.
    /// Joining on them returns nothing and pays for a scan to do it.
    #[test]
    fn anchors_never_make_candidates() {
        let conn = open_in_memory().unwrap();
        for id in ["a", "b"] {
            entry(&conn, id, "2024-01-01T00:00:00Z");
            file(&conn, id, &["hash-table-lookup"], &[]);
        }
        assert!(
            sharing(&conn, "a").unwrap().is_empty(),
            "a shared anchor is not a reason to compare two notes"
        );
    }

    #[test]
    fn a_shared_topic_makes_two_notes_candidates() {
        let conn = open_in_memory().unwrap();
        entry(&conn, "a", "2024-01-01T00:00:00Z");
        entry(&conn, "b", "2024-01-02T00:00:00Z");
        file(&conn, "a", &["database-indexes"], &["databases"]);
        file(&conn, "b", &["hash-table-lookup"], &["databases"]);

        assert_eq!(sharing(&conn, "a").unwrap(), vec![("b".to_string(), 1)]);
    }

    /// A topic on most of the corpus selects the corpus, so it narrows
    /// nothing. Declined at read time rather than prevented at write time,
    /// because the model cannot be stopped from coining it.
    #[test]
    fn a_topic_covering_most_of_the_corpus_is_not_a_key() {
        let conn = open_in_memory().unwrap();
        for i in 0..12 {
            let id = format!("e{i}");
            entry(&conn, &id, "2024-01-01T00:00:00Z");
            // On all twelve, plus one rare topic shared by exactly two.
            let topics: Vec<&str> = if i < 2 {
                vec!["everything", "databases"]
            } else {
                vec!["everything"]
            };
            file(&conn, &id, &[], &topics);
        }
        let got = sharing(&conn, "e0").unwrap();
        assert_eq!(
            got,
            vec![("e1".to_string(), 1)],
            "only the rare topic may select, or the filter selects the corpus"
        );
    }

    /// The floor. On a young corpus every topic is on "most" of it, and
    /// applying the rule there would mean nothing ever connects until it grows.
    #[test]
    fn in_a_young_corpus_a_common_topic_still_counts() {
        let conn = open_in_memory().unwrap();
        for id in ["a", "b", "c"] {
            entry(&conn, id, "2024-01-01T00:00:00Z");
            file(&conn, id, &[], &["databases"]);
        }
        assert_eq!(sharing(&conn, "a").unwrap().len(), 2);
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
        let first = upsert(&conn, &["free-will".to_string()], Kind::Topic).unwrap();
        let second = upsert(&conn, &["free-will".to_string()], Kind::Topic).unwrap();

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
