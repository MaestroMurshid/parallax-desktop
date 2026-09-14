//! Restoring an exported corpus.
//!
//! Checked in full before the first write and written in one transaction. An
//! import that failed halfway would leave a corpus that is neither the old one
//! nor the new one, and the file it came from is often the only other copy.

use crate::error::{Error, Result};
use crate::model::{Edge, Entry, Question};
use rusqlite::{params, Connection};
use serde::Deserialize;
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImportMode {
    /// Keeps what is already there: an id in the corpus wins over the file.
    Merge,
    /// Wipes first. The status bar's `clear` is this with an empty payload,
    /// which is why it needs no verb of its own.
    Replace,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct CorpusImport {
    #[serde(default)]
    pub entries: Vec<Entry>,
    #[serde(default)]
    pub edges: Vec<Edge>,
    #[serde(default)]
    pub questions: Vec<Question>,
}

/// Returns the audio paths the import orphaned. The row goes through SQLite
/// and the file does not, so a replace that dropped the rows and left the WAVs
/// would grow the audio directory every time -- the defect `delete_entry` was
/// already fixed for. Unlinking belongs to the caller, which knows the root.
pub fn import(conn: &Connection, data: &CorpusImport, mode: ImportMode) -> Result<Vec<String>> {
    validate(conn, data, mode)?;

    let tx = conn.unchecked_transaction()?;
    let mut orphaned = Vec::new();
    if mode == ImportMode::Replace {
        orphaned = audio_paths(&tx)?;
        // Everything else cascades: audio, spans, action items, questions and
        // edges all hang off an entry.
        tx.execute("DELETE FROM entries", [])?;
    }

    // Parent before child. `answers_entry_id` is a foreign key checked per
    // statement, so insert order is not the file's to decide -- and this is the
    // order placement solved the field in anyway (§5.1).
    let mut entries: Vec<&Entry> = data.entries.iter().collect();
    entries.sort_by(|a, b| a.created_at.cmp(&b.created_at));

    for entry in entries {
        // Merge keeps the copy you have: it may have been corrected or dragged
        // since the export was taken.
        if exists(&tx, &entry.id)? {
            continue;
        }
        super::entries::insert(&tx, entry)?;
    }

    for edge in &data.edges {
        // An edge naming an entry that is not there is a broken export, not a
        // connection. Dropped rather than failing the import, which is what the
        // sample loader does with the same problem.
        if !exists(&tx, &edge.entry_a)? || !exists(&tx, &edge.entry_b)? {
            continue;
        }
        super::edges::insert(&tx, edge)?;
    }

    for question in &data.questions {
        let Some(entry) = super::entries::get(&tx, &question.entry_id)? else {
            continue;
        };
        // `INSERT OR IGNORE` on the id, so re-importing the same file does not
        // double every question up. The mock keeps one question per entry;
        // here they accumulate, which is what §3.4 actually asks for.
        super::questions::insert(&tx, question, &entry.transcript)?;
    }

    // A path the incoming entries still use is not orphaned, however many rows
    // pointed at it before. Replacing a corpus with its own export deleted
    // every recording it restored without this.
    if !orphaned.is_empty() {
        let kept: HashSet<String> = audio_paths(&tx)?.into_iter().collect();
        orphaned.retain(|path| !kept.contains(path));
    }

    tx.commit()?;
    Ok(orphaned)
}

/// Everything that can be known to be wrong before anything is written.
///
/// Dangling edges and questions are dropped rather than refused -- they cost
/// one connection. An id collision or a missing parent is a broken file, and
/// finding that out halfway through the write is what this exists to avoid.
fn validate(conn: &Connection, data: &CorpusImport, mode: ImportMode) -> Result<()> {
    let mut arriving: HashSet<&str> = HashSet::new();
    for entry in &data.entries {
        if entry.id.trim().is_empty() {
            return Err(Error::Other("an entry in the import has no id".to_string()));
        }
        if !arriving.insert(entry.id.as_str()) {
            return Err(Error::Other(format!(
                "the import names entry {} twice",
                entry.id
            )));
        }
    }

    // What will exist once this lands: a merge keeps the corpus, a replace does
    // not, so the same file can be valid one way and not the other.
    let kept: HashSet<String> = match mode {
        ImportMode::Merge => all_ids(conn)?,
        ImportMode::Replace => HashSet::new(),
    };

    for entry in &data.entries {
        let Some(parent) = &entry.parent_entry_id else {
            continue;
        };
        if !arriving.contains(parent.as_str()) && !kept.contains(parent) {
            return Err(Error::Other(format!(
                "entry {} answers {parent}, which the import does not contain",
                entry.id
            )));
        }
    }

    let mut edge_ids: HashSet<&str> = HashSet::new();
    for edge in &data.edges {
        if !edge_ids.insert(edge.id.as_str()) {
            return Err(Error::Other(format!(
                "the import names edge {} twice",
                edge.id
            )));
        }
    }

    let mut question_ids: HashSet<&str> = HashSet::new();
    for question in &data.questions {
        if !question_ids.insert(question.id.as_str()) {
            return Err(Error::Other(format!(
                "the import names question {} twice",
                question.id
            )));
        }
    }

    Ok(())
}

fn exists(conn: &Connection, id: &str) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM entries WHERE id = ?1)",
        params![id],
        |row| row.get(0),
    )?)
}

fn all_ids(conn: &Connection) -> Result<HashSet<String>> {
    let mut stmt = conn.prepare("SELECT id FROM entries")?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    Ok(rows.collect::<rusqlite::Result<HashSet<_>>>()?)
}

fn audio_paths(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT path FROM audio")?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;
    use crate::model::{EdgeStatus, Register, Relation, Role, Span};

    fn entry(id: &str, created_at: &str, audio: bool) -> Entry {
        Entry {
            id: id.into(),
            audio_path: if audio {
                Some(format!("audio/{id}.wav"))
            } else {
                None
            },
            transcript: format!("Something I said, filed as {id}."),
            created_at: created_at.into(),
            x: 0.0,
            y: 0.0,
            parent_entry_id: None,
            answers_question_id: None,
            role: Role::Position,
            register: Register::Neutral,
            type_id: "position".into(),
            resolved: false,
            resolution_text: None,
            title: id.into(),
            summary: None,
            duration_ms: 40_000,
            fingerprint: if audio { vec![0.3] } else { vec![] },
            unfinished: false,
            local_only: false,
            spans: vec![],
            action_items: vec![],
            is_sample: None,
        }
    }

    fn edge(id: &str, a: &str, b: &str) -> Edge {
        Edge {
            id: id.into(),
            entry_a: a.into(),
            entry_b: b.into(),
            relation: Relation::Extends,
            question: None,
            status: EdgeStatus::Accepted,
            created_at: "2024-02-03T10:21:00.000Z".into(),
        }
    }

    fn question(id: &str, entry_id: &str) -> Question {
        Question {
            id: id.into(),
            entry_id: entry_id.into(),
            text: "Where does that stop holding?".into(),
            span: Some(Span {
                start: 0,
                end: 9,
                attributed: false,
            }),
            answered: true,
            dismissed: false,
            provider_name: "llama-server".into(),
            created_at: "2024-02-04T10:21:00.000Z".into(),
        }
    }

    fn two_entries() -> CorpusImport {
        CorpusImport {
            entries: vec![
                entry("a", "2024-02-03T10:00:00.000Z", false),
                entry("b", "2024-02-03T11:00:00.000Z", false),
            ],
            edges: vec![edge("edge-a-b", "a", "b")],
            questions: vec![question("q1", "a")],
        }
    }

    fn counts(conn: &Connection) -> (usize, usize, usize) {
        (
            super::super::entries::list(conn).unwrap().len(),
            super::super::edges::list(conn).unwrap().len(),
            super::super::questions::list(conn).unwrap().len(),
        )
    }

    /// An export carries all three, so an import that only restores entries
    /// loses every connection and every question that was ever asked.
    #[test]
    fn everything_in_the_file_comes_back() {
        let conn = open_in_memory().unwrap();
        import(&conn, &two_entries(), ImportMode::Merge).unwrap();
        assert_eq!(counts(&conn), (2, 1, 1));

        // An answered question is part of the record, not noise to drop.
        let restored = super::super::questions::list(&conn).unwrap();
        assert!(restored[0].answered);
    }

    #[test]
    fn a_replace_wipes_what_was_there() {
        let conn = open_in_memory().unwrap();
        super::super::entries::insert(&conn, &entry("mine", "2024-01-01T00:00:00.000Z", false))
            .unwrap();

        import(&conn, &two_entries(), ImportMode::Replace).unwrap();

        let ids: Vec<String> = super::super::entries::list(&conn)
            .unwrap()
            .into_iter()
            .map(|e| e.id)
            .collect();
        assert_eq!(ids, vec!["a".to_string(), "b".to_string()]);
    }

    /// The status bar's `clear` is a replace with nothing in it.
    #[test]
    fn an_empty_replace_clears_the_corpus() {
        let conn = open_in_memory().unwrap();
        import(&conn, &two_entries(), ImportMode::Merge).unwrap();

        import(&conn, &CorpusImport::default(), ImportMode::Replace).unwrap();
        assert_eq!(counts(&conn), (0, 0, 0));
    }

    /// Merge keeps the copy you have: the one in the corpus may have been
    /// corrected or dragged since the export was taken.
    #[test]
    fn a_merge_keeps_an_id_already_in_the_corpus() {
        let conn = open_in_memory().unwrap();
        let mut mine = entry("a", "2024-02-03T10:00:00.000Z", false);
        mine.transcript = "What I actually said.".into();
        super::super::entries::insert(&conn, &mine).unwrap();

        import(&conn, &two_entries(), ImportMode::Merge).unwrap();

        let kept = super::super::entries::get(&conn, "a").unwrap().unwrap();
        assert_eq!(kept.transcript, "What I actually said.");
        assert_eq!(counts(&conn).0, 2, "b should still have arrived");
    }

    /// An edge naming an entry the file does not contain is a broken export,
    /// not a connection. Dropped rather than aborting the whole import, which
    /// is what the sample loader does with the same problem.
    #[test]
    fn an_edge_naming_a_missing_entry_is_dropped() {
        let conn = open_in_memory().unwrap();
        let mut data = two_entries();
        data.edges.push(edge("edge-a-gone", "a", "gone"));
        data.questions.push(question("q-gone", "gone"));

        import(&conn, &data, ImportMode::Merge).unwrap();

        let edges = super::super::edges::list(&conn).unwrap();
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].id, "edge-a-b");
        assert_eq!(super::super::questions::list(&conn).unwrap().len(), 1);
    }

    /// The same id twice cannot both be written, and finding that out halfway
    /// through is what validating first exists to avoid.
    #[test]
    fn a_duplicate_entry_id_refuses_the_whole_import() {
        let conn = open_in_memory().unwrap();
        let mut data = two_entries();
        data.entries
            .push(entry("a", "2024-02-03T12:00:00.000Z", false));

        assert!(import(&conn, &data, ImportMode::Merge).is_err());
        assert_eq!(counts(&conn), (0, 0, 0), "a refused import wrote anyway");
    }

    /// An answer whose parent is nowhere would fail on the foreign key partway
    /// through the write, after the entries before it had already landed.
    #[test]
    fn a_child_whose_parent_is_absent_is_refused() {
        let conn = open_in_memory().unwrap();
        let mut data = two_entries();
        let mut orphan = entry("c", "2024-02-03T12:00:00.000Z", false);
        orphan.parent_entry_id = Some("never-exported".into());
        data.entries.push(orphan);

        assert!(import(&conn, &data, ImportMode::Merge).is_err());
        assert_eq!(counts(&conn), (0, 0, 0));
    }

    /// An answer to an entry that is in the same file is ordinary, and has to
    /// survive whatever order the file happens to list them in.
    #[test]
    fn an_answer_arrives_with_the_entry_it_answers() {
        let conn = open_in_memory().unwrap();
        let mut child = entry("c", "2024-02-03T12:00:00.000Z", false);
        child.parent_entry_id = Some("a".into());
        let mut data = two_entries();
        // Listed before its parent on purpose: the foreign key is checked per
        // statement, so insert order is not the file's to decide.
        data.entries.insert(0, child);

        import(&conn, &data, ImportMode::Merge).unwrap();
        assert_eq!(
            super::super::entries::get(&conn, "c")
                .unwrap()
                .unwrap()
                .parent_entry_id
                .as_deref(),
            Some("a")
        );
    }

    /// Importing the same file twice is something a person does when they are
    /// not sure the first one took.
    #[test]
    fn importing_twice_changes_nothing() {
        let conn = open_in_memory().unwrap();
        import(&conn, &two_entries(), ImportMode::Merge).unwrap();
        let before = counts(&conn);

        import(&conn, &two_entries(), ImportMode::Merge).unwrap();
        assert_eq!(counts(&conn), before);
    }

    /// The row goes through SQLite and the file does not. A replace that
    /// dropped the rows and left the WAVs would grow the audio directory every
    /// time -- the defect `delete_entry` was already fixed for.
    #[test]
    fn a_replace_reports_the_audio_it_orphaned() {
        let conn = open_in_memory().unwrap();
        super::super::entries::insert(&conn, &entry("mine", "2024-01-01T00:00:00.000Z", true))
            .unwrap();

        let orphaned = import(&conn, &CorpusImport::default(), ImportMode::Replace).unwrap();
        assert_eq!(orphaned, vec!["audio/mine.wav".to_string()]);
    }

    /// Found in the packaged app: uploading the app's own export with replace
    /// deleted every recording it restored. All the old rows' paths were
    /// reported as orphaned, including the ones the incoming entries point at,
    /// so the caller unlinked files the corpus still used.
    #[test]
    fn a_replace_keeps_the_recordings_its_own_entries_still_use() {
        let conn = open_in_memory().unwrap();
        for (id, at) in [
            ("kept", "2024-01-01T00:00:00.000Z"),
            ("gone", "2024-01-02T00:00:00.000Z"),
        ] {
            super::super::entries::insert(&conn, &entry(id, at, true)).unwrap();
        }
        let file = CorpusImport {
            entries: vec![entry("kept", "2024-01-01T00:00:00.000Z", true)],
            ..Default::default()
        };

        let orphaned = import(&conn, &file, ImportMode::Replace).unwrap();
        assert_eq!(orphaned, vec!["audio/gone.wav".to_string()]);
    }

    /// A merge removes nothing, so it can orphan nothing.
    #[test]
    fn a_merge_orphans_nothing() {
        let conn = open_in_memory().unwrap();
        super::super::entries::insert(&conn, &entry("mine", "2024-01-01T00:00:00.000Z", true))
            .unwrap();

        assert!(import(&conn, &two_entries(), ImportMode::Merge)
            .unwrap()
            .is_empty());
    }
}
