//! The corpus as one file someone can keep: every note as MDX, and the
//! recordings beside them when asked for.
//!
//! A zip because a corpus is hundreds of files and a backup people move around
//! has to be one. MDX rather than JSON because the note file already carries
//! everything JSON did, plus the topics JSON dropped -- an upload that loses
//! topics leaves every note off every shelf, connected to nothing.

use super::{corpus, Note};
use crate::db::import::{CorpusImport, ImportMode};
use crate::error::Result;
use rusqlite::Connection;
use serde::Serialize;
use std::path::Path;

pub const NOTES_DIR: &str = "notes";
pub const AUDIO_DIR: &str = "audio";

/// What an upload holds, read in full before anything is written.
#[derive(Debug, Default)]
pub struct Contents {
    pub notes: Vec<Note>,
    /// A file name inside `audio/` and its bytes. A name, never a path.
    pub audio: Vec<(String, Vec<u8>)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Written {
    pub notes: usize,
    pub audio: usize,
    /// Notes whose recording was not on disk to put in.
    pub missing_audio: usize,
}

/// Writes the whole corpus to `out`.
pub fn write(conn: &Connection, root: &Path, out: &Path, with_audio: bool) -> Result<Written> {
    let _ = (conn, root, out, with_audio);
    todo!()
}

/// Reads an upload: an archive this app wrote, or a JSON export from before.
pub fn read(path: &Path) -> Result<Contents> {
    let _ = path;
    todo!()
}

/// Restores the notes, then puts their recordings where the notes expect them.
/// Returns the audio paths a replace orphaned, for the caller to unlink.
pub fn restore(
    conn: &Connection,
    root: &Path,
    contents: &Contents,
    mode: ImportMode,
) -> Result<Vec<String>> {
    let _ = (conn, root, contents, mode, corpus::restore, CorpusImport::default);
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{self, tags::Kind};
    use crate::model::{Edge, EdgeStatus, Entry, Question, Register, Relation, Role};
    use std::io::Write;
    use std::path::PathBuf;

    fn scratch(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("parallax-archive-{label}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join(AUDIO_DIR)).unwrap();
        dir
    }

    fn entry(id: &str, created_at: &str, audio: bool) -> Entry {
        Entry {
            id: id.into(),
            audio_path: audio.then(|| format!("{AUDIO_DIR}/{id}.wav")),
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
            summary: Some(format!("a summary of {id}")),
            duration_ms: 40_000,
            fingerprint: if audio { vec![0.3] } else { vec![] },
            unfinished: false,
            local_only: false,
            spans: vec![],
            action_items: vec![],
            is_sample: None,
        }
    }

    /// Two notes, one recorded, a topic, an edge and a question: every kind of
    /// thing an upload has lost before.
    fn seeded(root: &Path) -> Connection {
        let conn = db::open_in_memory().unwrap();
        db::entries::insert(&conn, &entry("spoken", "2024-01-01T00:00:00.000Z", true)).unwrap();
        db::entries::insert(&conn, &entry("typed", "2024-02-01T00:00:00.000Z", false)).unwrap();
        std::fs::write(root.join(AUDIO_DIR).join("spoken.wav"), b"RIFF-not-really-a-wav").unwrap();

        let ids = db::tags::upsert(&conn, &["databases".to_string()], Kind::Topic).unwrap();
        db::tags::set_for_entry(&conn, "spoken", &ids).unwrap();
        db::edges::insert(
            &conn,
            &Edge {
                id: "edge-1".into(),
                entry_a: "spoken".into(),
                entry_b: "typed".into(),
                relation: Relation::Extends,
                question: Some("Does the second go further?".into()),
                status: EdgeStatus::Proposed,
                created_at: "2024-02-01T00:00:01.000Z".into(),
            },
        )
        .unwrap();
        let q = Question {
            id: "q-1".into(),
            entry_id: "spoken".into(),
            text: "Where does that stop holding?".into(),
            span: None,
            answered: false,
            dismissed: false,
            provider_name: "qwen3-4b-q4".into(),
            created_at: "2024-01-01T00:00:01.000Z".into(),
        };
        let transcript = db::entries::get(&conn, "spoken").unwrap().unwrap().transcript;
        db::questions::insert(&conn, &q, &transcript).unwrap();
        conn
    }

    fn topics(conn: &Connection, id: &str) -> Vec<String> {
        db::tags::for_entry(conn, id)
            .unwrap()
            .into_iter()
            .filter(|t| t.kind == Kind::Topic)
            .map(|t| t.name)
            .collect()
    }

    /// The upload that lost recordings, topics and nothing else is the one this
    /// replaces, so all three are checked on the far side of a real zip.
    #[test]
    fn a_corpus_and_its_recordings_survive_the_round_trip() {
        let (here, there) = (scratch("here"), scratch("there"));
        let conn = seeded(&here);
        let zip = here.join("corpus.zip");

        let written = write(&conn, &here, &zip, true).unwrap();
        assert_eq!(written, Written { notes: 2, audio: 1, missing_audio: 0 });

        let fresh = db::open_in_memory().unwrap();
        restore(&fresh, &there, &read(&zip).unwrap(), ImportMode::Replace).unwrap();

        assert_eq!(db::entries::list(&fresh).unwrap().len(), 2);
        assert_eq!(db::edges::list(&fresh).unwrap().len(), 1);
        assert_eq!(db::questions::list(&fresh).unwrap().len(), 1);
        assert_eq!(topics(&fresh, "spoken"), vec!["databases".to_string()]);
        assert_eq!(
            std::fs::read(there.join(AUDIO_DIR).join("spoken.wav")).unwrap(),
            b"RIFF-not-really-a-wav"
        );
    }

    #[test]
    fn notes_only_leaves_the_recordings_out() {
        let here = scratch("notes-only");
        let conn = seeded(&here);
        let zip = here.join("notes.zip");

        let written = write(&conn, &here, &zip, false).unwrap();
        assert_eq!(written.audio, 0);
        let contents = read(&zip).unwrap();
        assert_eq!(contents.notes.len(), 2);
        assert!(contents.audio.is_empty());
    }

    /// An archive is a file from anywhere. A name that climbs out of `audio/`
    /// must not be written, the same rule `remove_audio` holds for unlinking.
    #[test]
    fn a_recording_named_outside_the_audio_folder_is_not_written() {
        let (here, there) = (scratch("crafted"), scratch("victim"));
        let conn = seeded(&here);
        let zip = here.join("crafted.zip");
        write(&conn, &here, &zip, false).unwrap();

        // Re-open and append entries a hostile archive could carry.
        let mut appended = zip::ZipWriter::new_append(
            std::fs::OpenOptions::new().read(true).write(true).open(&zip).unwrap(),
        )
        .unwrap();
        for name in ["audio/../../escaped.wav", "audio/nested/deeper.wav"] {
            appended
                .start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            appended.write_all(b"not ours").unwrap();
        }
        appended.finish().unwrap();

        let fresh = db::open_in_memory().unwrap();
        restore(&fresh, &there, &read(&zip).unwrap(), ImportMode::Replace).unwrap();

        assert!(!there.parent().unwrap().join("escaped.wav").exists());
        assert!(!there.join("escaped.wav").exists());
        assert!(!there.join(AUDIO_DIR).join("nested").exists());
    }

    /// Validated in full before the first write, like every other import: a
    /// half-restored corpus is neither the old one nor the new one.
    #[test]
    fn one_unreadable_note_refuses_the_whole_archive() {
        let here = scratch("broken");
        let zip = here.join("broken.zip");
        let conn = seeded(&here);
        write(&conn, &here, &zip, false).unwrap();
        let mut appended = zip::ZipWriter::new_append(
            std::fs::OpenOptions::new().read(true).write(true).open(&zip).unwrap(),
        )
        .unwrap();
        appended
            .start_file("notes/broken.mdx", zip::write::SimpleFileOptions::default())
            .unwrap();
        appended.write_all(b"---\n{ not json\n---\n\nsaid").unwrap();
        appended.finish().unwrap();

        let err = read(&zip).unwrap_err().to_string();
        assert!(err.contains("broken.mdx"), "{err}");
    }

    /// Exports taken before the archive existed are JSON, and people have them.
    #[test]
    fn an_old_json_export_still_uploads() {
        let here = scratch("json");
        let json = here.join("parallax-old.json");
        let file = serde_json::json!({
            "version": 1,
            "exportedAt": "2026-09-12T01:51:52.000Z",
            "entries": [entry("spoken", "2024-01-01T00:00:00.000Z", false), entry("typed", "2024-02-01T00:00:00.000Z", false)],
            "edges": [{
                "id": "edge-1", "entryA": "spoken", "entryB": "typed", "relation": "extends",
                "question": null, "status": "accepted", "createdAt": "2024-02-01T00:00:01.000Z"
            }],
            "questions": []
        });
        std::fs::write(&json, serde_json::to_string_pretty(&file).unwrap()).unwrap();

        let fresh = db::open_in_memory().unwrap();
        restore(&fresh, &here, &read(&json).unwrap(), ImportMode::Merge).unwrap();
        assert_eq!(db::entries::list(&fresh).unwrap().len(), 2);
        assert_eq!(db::edges::list(&fresh).unwrap().len(), 1);
    }

    /// Merge keeps the copy you have, and that includes the recording.
    #[test]
    fn a_merge_does_not_overwrite_a_recording_already_here() {
        let (here, there) = (scratch("merge-src"), scratch("merge-dst"));
        let conn = seeded(&here);
        let zip = here.join("corpus.zip");
        write(&conn, &here, &zip, true).unwrap();

        let existing = seeded(&there);
        std::fs::write(there.join(AUDIO_DIR).join("spoken.wav"), b"the one already here").unwrap();
        restore(&existing, &there, &read(&zip).unwrap(), ImportMode::Merge).unwrap();

        assert_eq!(
            std::fs::read(there.join(AUDIO_DIR).join("spoken.wav")).unwrap(),
            b"the one already here"
        );
    }
}
