//! Corpus reads and writes. Thin: every command locks the connection, calls
//! into `db`, and returns. The logic lives below this layer.

use crate::db;
use crate::db::create::NewEntry;
use crate::db::search::SearchHit;
use crate::error::{Error, Result};
use crate::model::Entry;
use crate::state::AppState;
use tauri::State;

#[tauri::command]
pub fn list_entries(state: State<AppState>) -> Result<Vec<Entry>> {
    let conn = state.db();
    db::entries::list(&conn)
}

/// `None` rather than an error: the frontend treats a missing entry as a
/// legitimate answer, not a failure.
#[tauri::command]
pub fn get_entry(state: State<AppState>, id: String) -> Result<Option<Entry>> {
    let conn = state.db();
    db::entries::get(&conn, &id)
}

/// Answers only. A manual or proposed connection is an edge and has no parent,
/// so nothing here is about edges despite what the wire field is called.
#[tauri::command]
pub fn list_children(state: State<AppState>, entry_id: String) -> Result<Vec<Entry>> {
    let conn = state.db();
    db::entries::children_of(&conn, &entry_id)
}

/// Offered from the empty state, never forced.
#[tauri::command]
pub fn load_sample_corpus(state: State<AppState>) -> Result<()> {
    let conn = state.db();
    db::sample::load(&conn)
}

#[tauri::command]
pub fn clear_sample_corpus(state: State<AppState>) -> Result<()> {
    let conn = state.db();
    db::sample::clear(&conn)
}

/// Substring by default; a quoted query matches whole words only.
#[tauri::command]
pub fn search_entries(state: State<AppState>, query: String) -> Result<Vec<SearchHit>> {
    let conn = state.db();
    db::search::search(&conn, &query)
}

/// Places the entry against the existing field and freezes it there.
#[tauri::command]
pub fn create_entry(state: State<AppState>, draft: NewEntry) -> Result<Entry> {
    let conn = state.db();
    db::create::create(&conn, draft)
}

/// Overwrites the frozen position and never re-solves the field (§5.1).
#[tauri::command]
pub fn move_entry(state: State<AppState>, id: String, x: f64, y: f64) -> Result<Entry> {
    let conn = state.db();
    db::entries::move_to(&conn, &id, x, y)?;
    db::entries::get(&conn, &id)?.ok_or_else(|| crate::error::Error::NotFound(id))
}

/// Children are orphaned rather than deleted: an answer is still something you
/// said. Deleting a whole thread is a deliberate second act, not a side effect.
#[tauri::command]
pub fn delete_entry(state: State<AppState>, id: String) -> Result<()> {
    state.delete_entry(&id)
}

/// §6.3 -- user-declared, and the text is the point. The AI never decides you
/// are done thinking, and a bare flag records that you stopped rather than what
/// you concluded.
#[tauri::command]
pub fn resolve_entry(state: State<AppState>, entry_id: String, text: String) -> Result<Entry> {
    let conn = state.db();
    db::entries::resolve(&conn, &entry_id, &text)?;
    db::entries::get(&conn, &entry_id)?.ok_or_else(|| Error::NotFound(entry_id))
}

#[tauri::command]
pub fn reopen_entry(state: State<AppState>, entry_id: String) -> Result<Entry> {
    let conn = state.db();
    db::entries::reopen(&conn, &entry_id)?;
    db::entries::get(&conn, &entry_id)?.ok_or_else(|| Error::NotFound(entry_id))
}

/// Restores an exported corpus. `replace` is also what the status bar's `clear`
/// means, with an empty payload -- which is why clearing needs no verb of its
/// own on the bridge.
#[tauri::command]
pub fn import_corpus(
    state: State<AppState>,
    data: db::import::CorpusImport,
    mode: db::import::ImportMode,
) -> Result<()> {
    let orphaned = {
        let conn = state.db();
        db::import::import(&conn, &data, mode)?
    };
    // The rows are gone either way; a failed unlink costs a file on disk, which
    // is the safe direction and the same one `delete_entry` takes.
    for relative in orphaned {
        state.remove_audio(&relative);
    }
    Ok(())
}

/// The recording for an entry, as bytes.
///
/// Raw rather than JSON: a two-minute note is about 4MB of 16kHz mono, and
/// number-per-byte would be thirty times that. Whole-file rather than ranged,
/// so there is no seeking before it loads -- acceptable while notes are minutes.
#[tauri::command]
pub fn read_audio(state: State<AppState>, entry_id: String) -> Result<tauri::ipc::Response> {
    let relative = {
        let conn = state.db();
        db::entries::audio_path(&conn, &entry_id)?
    }
    .ok_or_else(|| Error::NotFound(format!("{entry_id} has no audio")))?;

    let full = resolve_audio(&state.root, &state.audio_dir(), &relative)?;
    Ok(tauri::ipc::Response::new(std::fs::read(full)?))
}

/// A stored path is a database value, so it is checked rather than trusted: a
/// row claiming `../../` must not read outside the corpus.
fn resolve_audio(
    root: &std::path::Path,
    audio_dir: &std::path::Path,
    relative: &str,
) -> Result<std::path::PathBuf> {
    let (full, audio_dir) = match (root.join(relative).canonicalize(), audio_dir.canonicalize()) {
        (Ok(f), Ok(d)) => (f, d),
        _ => return Err(Error::NotFound(format!("no recording at {relative}"))),
    };
    if !full.starts_with(&audio_dir) {
        return Err(Error::Other(format!("{relative} is outside the corpus")));
    }
    Ok(full)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn corpus_root(tag: &str) -> std::path::PathBuf {
        let root =
            std::env::temp_dir().join(format!("parallax-audio-{tag}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("audio")).unwrap();
        root
    }

    #[test]
    fn a_recording_inside_the_corpus_resolves() {
        let root = corpus_root("inside");
        std::fs::write(root.join("audio/e1.wav"), b"bytes").unwrap();
        let found = resolve_audio(&root, &root.join("audio"), "audio/e1.wav").unwrap();
        assert_eq!(std::fs::read(found).unwrap(), b"bytes");
    }

    #[test]
    fn a_path_that_escapes_the_corpus_is_refused() {
        let root = corpus_root("escape");
        let secret = root.parent().unwrap().join("outside.wav");
        std::fs::write(&secret, b"not yours").unwrap();

        let escaped = resolve_audio(&root, &root.join("audio"), "audio/../../outside.wav");
        assert!(escaped.is_err(), "a stored path must not read outside");
        let _ = std::fs::remove_file(secret);
    }

    #[test]
    fn a_missing_recording_is_not_found() {
        let root = corpus_root("missing");
        assert!(resolve_audio(&root, &root.join("audio"), "audio/gone.wav").is_err());
    }
}
