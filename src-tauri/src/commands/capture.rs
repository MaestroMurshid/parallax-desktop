//! Recording: start, stop, discard, undo.
//!
//! §4 -- the hotkey starts recording immediately and the panel appears second,
//! so `start_recording` must not wait on anything visual.

use crate::audio::{fingerprint, recorder, wav};
use crate::db;
use crate::error::{Error, Result};
use crate::model::Entry;
use crate::state::AppState;
use tauri::State;

/// §4 -- a discard is undoable for a minute, because escape meaning "throw it
/// away" while recording and "leave it" once stopped is a muscle-memory trap.
pub const UNDO_WINDOW_MS: u64 = 60_000;

#[tauri::command]
pub fn start_recording(state: State<AppState>) -> Result<()> {
    let mut slot = state.recording.lock().unwrap_or_else(|p| p.into_inner());
    if slot.is_some() {
        return Err(Error::Other("already recording".into()));
    }
    *slot = Some(recorder::start()?);
    Ok(())
}

/// The live level for the equalizer bars. Polled rather than pushed: the
/// panel asks while it is on screen, and nothing has to be torn down when it
/// is not.
#[tauri::command]
pub fn recording_level(state: State<AppState>) -> f32 {
    state
        .recording
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .as_ref()
        .map(|r| r.level())
        .unwrap_or(0.0)
}

/// Stops, writes the audio, transcribes, and lands an entry.
///
/// Enrichment does not happen here. What this owes the caller is a row that
/// exists and a place on the canvas; classification and the question arrive
/// after, so a slow or absent model cannot cost someone their recording.
#[tauri::command]
pub fn stop_recording(
    state: State<AppState>,
    parent_edge: Option<String>,
    question_id: Option<String>,
) -> Result<Entry> {
    let recording = state
        .recording
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .take()
        .ok_or_else(|| Error::Other("not recording".into()))?;

    let duration_ms = recording.elapsed_ms();
    let pcm = recording.stop();

    let settings = {
        let conn = state.db();
        db::settings::get(&conn)?
    };

    let transcript = match state.transcription_model() {
        Some(model) => crate::stt::transcribe(&model, &pcm, settings.transcription_backend)?.text,
        // No model yet is not a lost recording: the audio is on disk and the
        // transcript is a derivation of it, so it can be filled in later.
        None => String::new(),
    };

    let conn = state.db();
    let entry = db::create::create(
        &conn,
        db::create::NewEntry {
            transcript,
            duration_ms,
            fingerprint: fingerprint::downsample(&pcm),
            parent_entry_id: parent_edge,
            local_only: None,
            typed: false,
        },
    )?;

    // The path is derived from the id, so it cannot be known until the entry
    // exists. Written after, and the row already points at it.
    if let Some(relative) = &entry.audio_path {
        let full = state.root.join(relative);
        let bytes = wav::write(&pcm, &full)?;
        conn.execute(
            "UPDATE audio SET byte_size = ?2 WHERE entry_id = ?1",
            rusqlite::params![entry.id, bytes as i64],
        )?;
    }

    if let Some(question_id) = question_id {
        db::questions::mark_answered(&conn, &question_id)?;
    }

    Ok(entry)
}

/// §4 -- discard belongs in the recording state, not after it. You know it is
/// junk before you stop.
#[tauri::command]
pub fn discard_recording(state: State<AppState>) -> Result<()> {
    let recording = state
        .recording
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .take()
        .ok_or_else(|| Error::Other("not recording".into()))?;

    let duration_ms = recording.elapsed_ms();
    let pcm = recording.stop();

    // Held, not dropped. Nothing is written to the corpus, but the samples
    // stay recoverable for the length of the window.
    *state.discarded.lock().unwrap_or_else(|p| p.into_inner()) = Some(crate::state::Discarded {
        pcm,
        duration_ms,
        at: std::time::Instant::now(),
    });
    Ok(())
}

#[tauri::command]
pub fn undo_discard(state: State<AppState>) -> Result<Option<Entry>> {
    let discarded = state
        .discarded
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .take();

    let Some(discarded) = discarded else {
        return Ok(None);
    };
    if discarded.at.elapsed().as_millis() as u64 > UNDO_WINDOW_MS {
        return Ok(None);
    }

    let settings = {
        let conn = state.db();
        db::settings::get(&conn)?
    };
    let transcript = match state.transcription_model() {
        Some(model) => {
            crate::stt::transcribe(&model, &discarded.pcm, settings.transcription_backend)?.text
        }
        None => String::new(),
    };

    let conn = state.db();
    let entry = db::create::create(
        &conn,
        db::create::NewEntry {
            transcript,
            duration_ms: discarded.duration_ms,
            fingerprint: fingerprint::downsample(&discarded.pcm),
            parent_entry_id: None,
            local_only: None,
            typed: false,
        },
    )?;

    if let Some(relative) = &entry.audio_path {
        wav::write(&discarded.pcm, &state.root.join(relative))?;
    }
    Ok(Some(entry))
}
