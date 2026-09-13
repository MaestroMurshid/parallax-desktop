//! The whole corpus as files, and files back into a corpus.
//!
//! SQLite stays authoritative. These are transport: a corpus can be written
//! out and read back, and the graph rebuilds from the files alone, but nothing
//! here is consulted while the app is running.
//!
//! The writing half reuses `db::import`, which already checks a payload in
//! full before the first write and does the rest in one transaction. An import
//! that failed halfway would leave a corpus that is neither the old one nor the
//! new one, and the file it came from is often the only other copy.

use super::Note;
use crate::db::{self, import::ImportMode, tags::Kind};
use crate::error::Result;
use rusqlite::Connection;

/// One file per note, named by the id it will be read back as.
pub fn filename(note: &Note) -> String {
    format!("{}.mdx", note.entry.id)
}

/// One note, assembled exactly as `export` assembles each of its own.
///
/// Same assembly deliberately: the viewer is showing what the file would be,
/// so a second way of building it would eventually show something the export
/// does not write.
pub fn note_for(conn: &Connection, entry_id: &str) -> Result<Note> {
    let entry = db::entries::get(conn, entry_id)?
        .ok_or_else(|| crate::error::Error::NotFound(format!("no entry {entry_id}")))?;

    let (mut anchors, mut topics) = (Vec::new(), Vec::new());
    for tag in db::tags::for_entry(conn, entry_id)? {
        match tag.kind {
            Kind::Anchor => anchors.push(tag.name),
            Kind::Topic => topics.push(tag.name),
        }
    }

    Ok(Note {
        questions: db::questions::list_for(conn, entry_id)?,
        // Only the ones leaving this note, matching `export`.
        edges: db::edges::list(conn)?
            .into_iter()
            .filter(|e| e.entry_a == entry_id)
            .collect(),
        anchors,
        topics,
        entry,
    })
}

/// Every note the corpus holds.
pub fn export(conn: &Connection) -> Result<Vec<Note>> {
    let questions = db::questions::list(conn)?;
    let edges = db::edges::list(conn)?;

    db::entries::list(conn)?
        .into_iter()
        .map(|entry| {
            let (mut anchors, mut topics) = (Vec::new(), Vec::new());
            for tag in db::tags::for_entry(conn, &entry.id)? {
                match tag.kind {
                    Kind::Anchor => anchors.push(tag.name),
                    Kind::Topic => topics.push(tag.name),
                }
            }
            Ok(Note {
                questions: questions
                    .iter()
                    .filter(|q| q.entry_id == entry.id)
                    .cloned()
                    .collect(),
                // Carried by the note the edge leaves, so each is written once
                // and the pair does not have to be reconciled on the way back.
                edges: edges
                    .iter()
                    .filter(|e| e.entry_a == entry.id)
                    .cloned()
                    .collect(),
                anchors,
                topics,
                entry,
            })
        })
        .collect()
}

/// Notes back into the corpus.
///
/// Tags are applied after the transaction rather than inside it. A failure
/// there leaves notes that are filed but connected to nothing, which is the
/// same recoverable state enrichment already tolerates -- whereas holding the
/// transaction open across them would risk the entries themselves.
pub fn restore(conn: &Connection, notes: &[Note], mode: ImportMode) -> Result<Vec<String>> {
    let data = db::import::CorpusImport {
        entries: notes.iter().map(|n| n.entry.clone()).collect(),
        edges: notes.iter().flat_map(|n| n.edges.clone()).collect(),
        questions: notes.iter().flat_map(|n| n.questions.clone()).collect(),
    };
    let orphaned = db::import::import(conn, &data, mode)?;

    for note in notes {
        let mut ids = db::tags::upsert(conn, &note.anchors, Kind::Anchor)?;
        ids.extend(db::tags::upsert(conn, &note.topics, Kind::Topic)?);
        if !ids.is_empty() {
            db::tags::set_for_entry(conn, &note.entry.id, &ids)?;
        }
    }
    Ok(orphaned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mdx::{parse, render};
    use crate::model::{Edge, EdgeStatus, Relation};

    fn note(conn: &Connection, transcript: &str) -> String {
        db::create::create(
            conn,
            db::create::NewEntry {
                transcript: transcript.into(),
                duration_ms: 5_000,
                fingerprint: vec![],
                parent_entry_id: None,
                local_only: None,
                typed: true,
            },
        )
        .unwrap()
        .id
    }

    fn seeded() -> (Connection, String, String) {
        let conn = db::open_in_memory().unwrap();
        let a = note(
            &conn,
            "Database indexes trade write performance for faster reads.",
        );
        let b = note(&conn, "Retries can make distributed systems less reliable.");
        for (id, anchor, topic) in [
            (&a, "database-indexes", "databases"),
            (&b, "retries", "distributed-systems"),
        ] {
            let mut ids = db::tags::upsert(&conn, &[anchor.to_string()], Kind::Anchor).unwrap();
            ids.extend(db::tags::upsert(&conn, &[topic.to_string()], Kind::Topic).unwrap());
            db::tags::set_for_entry(&conn, id, &ids).unwrap();
        }
        db::edges::insert(
            &conn,
            &Edge {
                id: "edge-1".into(),
                entry_a: a.clone(),
                entry_b: b.clone(),
                relation: Relation::SameMove,
                question: Some("Does each trade one cost for another?".into()),
                status: EdgeStatus::Proposed,
                created_at: "2026-02-03T10:00:00Z".into(),
            },
        )
        .unwrap();
        (conn, a, b)
    }

    /// The whole point: a corpus written to files and read back is the same
    /// corpus, and the graph rebuilds from the files alone.
    #[test]
    fn a_corpus_survives_being_written_out_and_read_back() {
        let (conn, a, b) = seeded();
        // Through the file format, not just the structs -- rendering is where
        // an offset or a relation would be lost.
        let files: Vec<String> = export(&conn)
            .unwrap()
            .iter()
            .map(|n| render(n).unwrap())
            .collect();
        let read: Vec<Note> = files.iter().map(|f| parse(f).unwrap()).collect();

        let fresh = db::open_in_memory().unwrap();
        restore(&fresh, &read, ImportMode::Replace).unwrap();

        let entries = db::entries::list(&fresh).unwrap();
        assert_eq!(entries.len(), 2);
        let restored_a = db::entries::get(&fresh, &a).unwrap().unwrap();
        assert_eq!(
            restored_a.transcript,
            "Database indexes trade write performance for faster reads."
        );

        let edges = db::edges::list(&fresh).unwrap();
        assert_eq!(edges.len(), 1, "the graph did not rebuild");
        assert_eq!(edges[0].relation, Relation::SameMove);
        assert_eq!(edges[0].entry_b, b);
    }

    /// Tags are the file's job -- there is no panel for them, so losing them
    /// on a round trip loses the only place they can be edited.
    #[test]
    fn anchors_and_topics_come_back_on_the_right_notes() {
        let (conn, a, _) = seeded();
        let notes = export(&conn).unwrap();
        let fresh = db::open_in_memory().unwrap();
        restore(&fresh, &notes, ImportMode::Replace).unwrap();

        let tags = db::tags::for_entry(&fresh, &a).unwrap();
        let anchors: Vec<&str> = tags
            .iter()
            .filter(|t| t.kind == Kind::Anchor)
            .map(|t| t.name.as_str())
            .collect();
        let topics: Vec<&str> = tags
            .iter()
            .filter(|t| t.kind == Kind::Topic)
            .map(|t| t.name.as_str())
            .collect();
        assert_eq!(anchors, vec!["database-indexes"]);
        assert_eq!(topics, vec!["databases"]);
    }

    /// Carried by the note it leaves, so the pair never has to be reconciled.
    #[test]
    fn an_edge_is_written_once_not_on_both_ends() {
        let (conn, a, b) = seeded();
        let notes = export(&conn).unwrap();
        let on_a = notes.iter().find(|n| n.entry.id == a).unwrap();
        let on_b = notes.iter().find(|n| n.entry.id == b).unwrap();
        assert_eq!(on_a.edges.len(), 1);
        assert!(on_b.edges.is_empty(), "the edge was written at both ends");
    }

    #[test]
    fn a_note_is_filed_under_the_id_it_reads_back_as() {
        let (conn, a, _) = seeded();
        let notes = export(&conn).unwrap();
        let one = notes.iter().find(|n| n.entry.id == a).unwrap();
        assert_eq!(filename(one), format!("{a}.mdx"));
    }
}
