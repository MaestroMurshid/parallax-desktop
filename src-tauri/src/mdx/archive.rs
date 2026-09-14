//! The corpus as one file someone can keep: every note as MDX, and the
//! recordings beside them when asked for.
//!
//! A zip because a corpus is hundreds of files and a backup people move around
//! has to be one. MDX rather than JSON because the note file already carries
//! everything JSON did, plus the topics JSON dropped -- an upload that loses
//! topics leaves every note off every shelf, connected to nothing.

use super::{corpus, Note};
use crate::db;
use crate::db::import::{CorpusImport, ImportMode};
use crate::error::{Error, Result};
use crate::model::TypeDef;
use rusqlite::Connection;
use serde::Serialize;
use std::io::{Read, Write};
use std::path::{Component, Path};

pub const NOTES_DIR: &str = "notes";
pub const AUDIO_DIR: &str = "audio";
/// Beside `notes/`, not inside it -- a type definition is not a note, and
/// `read` tells the two apart by extension, not by which folder they sat in.
pub const TYPES_FILE: &str = "types.json";

/// What an upload holds, read in full before anything is written.
#[derive(Debug, Default)]
pub struct Contents {
    pub notes: Vec<Note>,
    /// A file name inside `audio/` and its bytes. A name, never a path.
    pub audio: Vec<(String, Vec<u8>)>,
    /// Custom type definitions (§3.6). Empty for an archive written before
    /// these existed, or a JSON export, which never carried them either.
    pub types: Vec<TypeDef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Written {
    pub notes: usize,
    pub audio: usize,
    /// Notes whose recording was not on disk to put in.
    pub missing_audio: usize,
}

fn zipped(e: zip::result::ZipError) -> Error {
    Error::Other(format!("the archive could not be read or written: {e}"))
}

/// A recording's file name, if it is one: a single plain component, so
/// nothing a stranger's archive names can land outside `audio/`.
fn recording_name(name: &str) -> Option<&str> {
    let mut parts = Path::new(name).components();
    match (parts.next(), parts.next()) {
        (Some(Component::Normal(one)), None) => one.to_str(),
        _ => None,
    }
}

/// Writes the whole corpus to `out`.
///
/// Through a `.part` file renamed at the end, so a failure halfway leaves
/// nothing at the chosen path that looks like a finished backup.
pub fn write(conn: &Connection, root: &Path, out: &Path, with_audio: bool) -> Result<Written> {
    write_notes(
        &corpus::export(conn)?,
        &db::types::exportable(conn)?,
        root,
        out,
        with_audio,
    )
}

/// The writing half alone, so a caller can read the corpus under its lock and
/// then write -- recordings can be hundreds of megabytes, and nothing should
/// wait on a database lock while they copy.
pub fn write_notes(
    notes: &[Note],
    types: &[TypeDef],
    root: &Path,
    out: &Path,
    with_audio: bool,
) -> Result<Written> {
    let partial = out.with_extension("part");
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&partial)?);
    let text = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    // Audio is already dense; deflating it costs time and saves nothing.
    let stored =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);

    let mut written = Written {
        notes: 0,
        audio: 0,
        missing_audio: 0,
    };
    let result = (|| -> Result<()> {
        if !types.is_empty() {
            zip.start_file(TYPES_FILE, text).map_err(zipped)?;
            zip.write_all(serde_json::to_string_pretty(types)?.as_bytes())?;
        }
        for note in notes {
            zip.start_file(format!("{NOTES_DIR}/{}", corpus::filename(note)), text)
                .map_err(zipped)?;
            zip.write_all(super::render(note)?.as_bytes())?;
            written.notes += 1;

            let Some(relative) = note.entry.audio_path.as_deref().filter(|_| with_audio) else {
                continue;
            };
            let name = relative
                .strip_prefix(&format!("{AUDIO_DIR}/"))
                .and_then(recording_name);
            match name.map(|n| (n, std::fs::read(root.join(AUDIO_DIR).join(n)))) {
                Some((n, Ok(bytes))) => {
                    zip.start_file(format!("{AUDIO_DIR}/{n}"), stored)
                        .map_err(zipped)?;
                    zip.write_all(&bytes)?;
                    written.audio += 1;
                }
                // A note whose file is gone still exports; it says so rather
                // than failing a backup over one recording.
                _ => written.missing_audio += 1,
            }
        }
        zip.finish().map_err(zipped)?;
        Ok(())
    })();

    if let Err(e) = result {
        let _ = std::fs::remove_file(&partial);
        return Err(e);
    }
    std::fs::rename(&partial, out)?;
    Ok(written)
}

/// Reads an upload: an archive this app wrote, or a JSON export from before.
///
/// Every note is parsed before this returns, and one that does not parse
/// refuses the file by name -- nothing is written on the strength of a file
/// that is only partly readable.
pub fn read(path: &Path) -> Result<Contents> {
    let is_json = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("json"));
    if is_json {
        return from_json(&std::fs::read_to_string(path)?);
    }

    let mut archive = zip::ZipArchive::new(std::fs::File::open(path)?).map_err(zipped)?;
    let mut contents = Contents::default();
    for i in 0..archive.len() {
        let mut file = archive.by_index(i).map_err(zipped)?;
        if file.is_dir() {
            continue;
        }
        // `enclosed_name` already refuses absolute paths and `..`.
        let Some(inside) = file.enclosed_name() else {
            continue;
        };
        let inside = inside.to_string_lossy().replace('\\', "/");

        if inside.ends_with(".mdx") {
            let mut text = String::new();
            file.read_to_string(&mut text)?;
            let note = super::parse(&text)
                .map_err(|e| Error::Other(format!("{inside} could not be read: {e}")))?;
            contents.notes.push(note);
        } else if inside == TYPES_FILE {
            let mut text = String::new();
            file.read_to_string(&mut text)?;
            // Unreadable types.json refuses the archive by name, the same
            // rule an unreadable note already follows -- a partly-read type
            // definition is not something to guess the rest of.
            contents.types = serde_json::from_str(&text)
                .map_err(|e| Error::Other(format!("{TYPES_FILE} could not be read: {e}")))?;
        } else if let Some(name) = inside
            .strip_prefix(&format!("{AUDIO_DIR}/"))
            .and_then(recording_name)
        {
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes)?;
            contents.audio.push((name.to_string(), bytes));
        }
    }
    if contents.notes.is_empty() {
        return Err(Error::Other("there are no notes in this file".into()));
    }
    Ok(contents)
}

/// A JSON export, regrouped into the notes the archive path restores, so both
/// uploads go through one restore. Edges ride with the note they leave and
/// questions with the note they ask about, exactly as `corpus::export` groups
/// them.
fn from_json(text: &str) -> Result<Contents> {
    let data: CorpusImport = serde_json::from_str(text)?;
    if data.entries.is_empty() {
        return Err(Error::Other("there are no notes in this file".into()));
    }
    let notes = data
        .entries
        .iter()
        .map(|entry| Note {
            edges: data
                .edges
                .iter()
                .filter(|e| e.entry_a == entry.id)
                .cloned()
                .collect(),
            questions: data
                .questions
                .iter()
                .filter(|q| q.entry_id == entry.id)
                .cloned()
                .collect(),
            anchors: Vec::new(),
            topics: Vec::new(),
            entry: entry.clone(),
        })
        .collect();
    Ok(Contents {
        notes,
        audio: Vec::new(),
        types: Vec::new(),
    })
}

/// Restores the notes, then puts their recordings where the notes expect them.
/// Returns the audio paths a replace orphaned, for the caller to unlink.
///
/// Recordings go down after the notes commit: a restore that failed would
/// otherwise leave files behind that no note points at.
pub fn restore(
    conn: &Connection,
    root: &Path,
    contents: &Contents,
    mode: ImportMode,
) -> Result<Vec<String>> {
    // Definitions before the notes that use them -- not load-bearing (there
    // is deliberately no FK from an entry's type_id, §3.6), but it means a
    // restored note's type already exists at the moment the note lands rather
    // than a beat later.
    db::types::restore_types(conn, &contents.types, mode)?;
    let orphaned = corpus::restore(conn, &contents.notes, mode)?;

    let audio_dir = root.join(AUDIO_DIR);
    if !contents.audio.is_empty() {
        std::fs::create_dir_all(&audio_dir)?;
    }
    for (name, bytes) in &contents.audio {
        // Checked again here rather than trusted from `read`: `Contents` is a
        // public type, and the rule is about what reaches the disk.
        let Some(name) = recording_name(name) else {
            continue;
        };
        let target = audio_dir.join(name);
        // Merge keeps the copy you have, recording included.
        if mode == ImportMode::Merge && target.exists() {
            continue;
        }
        std::fs::write(target, bytes)?;
    }
    Ok(orphaned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{self, tags::Kind};
    use crate::model::{Edge, EdgeStatus, Entry, Question, Register, Relation, Role};
    use std::io::Write;
    use std::path::PathBuf;

    fn scratch(label: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("parallax-archive-{label}-{}", uuid::Uuid::new_v4()));
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
        std::fs::write(
            root.join(AUDIO_DIR).join("spoken.wav"),
            b"RIFF-not-really-a-wav",
        )
        .unwrap();

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
        let transcript = db::entries::get(&conn, "spoken")
            .unwrap()
            .unwrap()
            .transcript;
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
        assert_eq!(
            written,
            Written {
                notes: 2,
                audio: 1,
                missing_audio: 0
            }
        );

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
            std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&zip)
                .unwrap(),
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
            std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&zip)
                .unwrap(),
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

    /// §3.6 end to end: a custom type travels in the zip beside `notes/`, and
    /// a note carrying it is still filed under that type on the far side --
    /// not just present as a string with nothing behind it.
    #[test]
    fn a_custom_type_travels_with_the_archive_and_survives_upload() {
        let (here, there) = (scratch("types-here"), scratch("types-there"));
        let conn = db::open_in_memory().unwrap();
        db::types::create(
            &conn,
            crate::model::NewType {
                id: "wondering".into(),
                label: "wondering".into(),
                match_text: "musing without a claim yet".into(),
                prompt: None,
                tier: crate::model::ProbeTier::Heavy,
                role: None,
                mark: None,
            },
        )
        .unwrap();
        let mut e = entry("wondered", "2024-01-01T00:00:00Z", false);
        e.type_id = "wondering".into();
        db::entries::insert(&conn, &e).unwrap();

        let zip = here.join("types.zip");
        write(&conn, &here, &zip, false).unwrap();

        let fresh = db::open_in_memory().unwrap();
        restore(&fresh, &there, &read(&zip).unwrap(), ImportMode::Replace).unwrap();

        let restored_type = db::types::get(&fresh, "wondering").unwrap();
        assert!(restored_type.is_some(), "the custom type did not travel");
        assert_eq!(restored_type.unwrap().tier, crate::model::ProbeTier::Heavy);
        assert_eq!(
            db::entries::get(&fresh, "wondered")
                .unwrap()
                .unwrap()
                .type_id,
            "wondering"
        );
    }

    /// An archive from before types.json existed has nothing named that in
    /// it -- `Contents::types` defaults to empty, and the notes still upload.
    #[test]
    fn an_archive_without_a_types_file_still_uploads() {
        let (here, there) = (scratch("no-types-here"), scratch("no-types-there"));
        let conn = seeded(&here);
        let zip = here.join("old.zip");
        write(&conn, &here, &zip, false).unwrap();

        let contents = read(&zip).unwrap();
        assert!(
            contents.types.is_empty(),
            "this fixture defined no custom type"
        );

        let fresh = db::open_in_memory().unwrap();
        restore(&fresh, &there, &contents, ImportMode::Replace).unwrap();
        assert_eq!(db::entries::list(&fresh).unwrap().len(), 2);
    }

    /// Merge keeps the copy you have, and that includes the recording.
    #[test]
    fn a_merge_does_not_overwrite_a_recording_already_here() {
        let (here, there) = (scratch("merge-src"), scratch("merge-dst"));
        let conn = seeded(&here);
        let zip = here.join("corpus.zip");
        write(&conn, &here, &zip, true).unwrap();

        let existing = seeded(&there);
        std::fs::write(
            there.join(AUDIO_DIR).join("spoken.wav"),
            b"the one already here",
        )
        .unwrap();
        restore(&existing, &there, &read(&zip).unwrap(), ImportMode::Merge).unwrap();

        assert_eq!(
            std::fs::read(there.join(AUDIO_DIR).join("spoken.wav")).unwrap(),
            b"the one already here"
        );
    }
}
