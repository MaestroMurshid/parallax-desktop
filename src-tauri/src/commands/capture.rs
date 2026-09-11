//! Recording: start, stop, discard, undo.
//!
//! §4 -- the hotkey starts recording immediately and the panel appears second,
//! so `start_recording` must not wait on anything visual.

use crate::audio::{fingerprint, recorder, wav};
use crate::db;
use crate::error::{Error, Result};
use crate::model::Entry;
use crate::state::AppState;
use tauri::{Emitter, Manager, State};

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

/// The transcript so far, for the panel to show while you are still talking.
///
/// Polled for the same reason the level is: the panel asks while it is on
/// screen, and nothing has to be torn down when it is not. Each call transcribes
/// everything recorded so far, because whisper needs the whole utterance to be
/// coherent -- so a pass costs about a thirty-fifth of the duration and the
/// refresh naturally slows as the note grows. Empty rather than an error when
/// there is no model, too little audio, or no recording.
#[tauri::command]
pub async fn partial_transcript(state: State<'_, AppState>) -> Result<String> {
    const MIN_SAMPLES: usize = 16_000;

    let samples = {
        let slot = state.recording.lock().unwrap_or_else(|p| p.into_inner());
        match slot.as_ref() {
            Some(recording) => recording.samples(),
            None => return Ok(String::new()),
        }
    };
    if samples.len() < MIN_SAMPLES {
        return Ok(String::new());
    }

    let settings = {
        let conn = state.db();
        db::settings::get(&conn)?
    };
    let Some(model) = state.transcription_model(settings.transcription_model) else {
        return Ok(String::new());
    };

    // Always on the CPU: the reasoning model has first claim on VRAM, and this
    // runs repeatedly while a recording is in flight.
    Ok(
        crate::stt::transcribe(&model, &samples, crate::model::ComputeBackend::Cpu)?
            .text
            .trim()
            .to_string(),
    )
}

/// Stops, writes the audio, transcribes, and lands an entry.
///
/// Async so the transcription does not run on the thread pumping the window.
/// A long recording takes seconds even at 35x realtime, and doing that inline
/// freezes the UI and the hotkey with it.
///
/// Enrichment does not happen here. What this owes the caller is a row that
/// exists and a place on the canvas; classification and the question arrive
/// after, so a slow or absent model cannot cost someone their recording.
#[tauri::command]
pub async fn stop_recording(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
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
    let entry = finish(&state, pcm, duration_ms, parent_edge, question_id)?;
    enrich_later(&app, entry.id.clone());
    Ok(entry)
}

/// Enrichment runs after the entry is safe on disk, never before. It is allowed
/// to be slow, absent or wrong, and none of that may cost a recording (§9.4).
fn enrich_later(app: &tauri::AppHandle, entry_id: String) {
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let done = state
            .with_reasoning(|provider| crate::enrich::run::run(&state.db(), provider, &entry_id));
        match done {
            // No binary or no model yet: the question arrives when one lands.
            Ok(None) => {}
            Ok(Some(enriched)) => {
                let _ = app.emit("entry://enriched", &entry_id);
                if enriched.question_id.is_none() {
                    println!("classified {entry_id}, no question");
                }
            }
            Err(e) => eprintln!("enrichment failed for {entry_id}: {e}"),
        }
    });
}

/// Everything after the microphone stops, separated so the order in which a
/// recording becomes durable is testable without a microphone.
pub fn finish(
    state: &AppState,
    pcm: Vec<f32>,
    duration_ms: i64,
    parent_edge: Option<String>,
    question_id: Option<String>,
) -> Result<Entry> {
    // The recording cannot be recreated, so it is staged before anything that
    // can fail -- reading settings included.
    stage(state, &pcm, duration_ms);

    let settings = {
        let conn = state.db();
        db::settings::get(&conn)?
    };

    // On disk before transcription, not after: a model that fails to load must
    // cost the transcript and never the recording.
    let id = uuid::Uuid::new_v4().to_string();
    let full = state.root.join(format!("audio/{id}.wav"));
    let bytes = wav::write(&pcm, &full)?;

    let transcript = match state.transcription_model(settings.transcription_model) {
        Some(model) => crate::stt::transcribe(&model, &pcm, settings.transcription_backend)?.text,
        // No model yet is not a lost recording: the audio is the record and
        // the transcript is derived from it, so it can be filled in later.
        None => String::new(),
    };

    let entry = land(
        state,
        id,
        &full,
        bytes,
        pcm,
        transcript,
        duration_ms,
        parent_edge,
    )?;

    let conn = state.db();
    if let Some(question_id) = question_id {
        db::questions::mark_answered(&conn, &question_id)?;
    }

    // Only now is there another copy.
    state
        .discarded
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .take();
    Ok(entry)
}

fn stage(state: &AppState, pcm: &[f32], duration_ms: i64) {
    *state.discarded.lock().unwrap_or_else(|p| p.into_inner()) = Some(crate::state::Discarded {
        pcm: pcm.to_vec(),
        duration_ms,
        at: std::time::Instant::now(),
    });
}

/// The row for audio that is already on disk. The file is written first so a
/// full disk cannot leave an entry on the canvas pointing at nothing.
#[allow(clippy::too_many_arguments)]
fn land(
    state: &AppState,
    id: String,
    full: &std::path::Path,
    bytes: u64,
    pcm: Vec<f32>,
    transcript: String,
    duration_ms: i64,
    parent_edge: Option<String>,
) -> Result<Entry> {
    let conn = state.db();
    let entry = db::create::create_with_id(
        &conn,
        id,
        db::create::NewEntry {
            transcript,
            duration_ms,
            fingerprint: fingerprint::downsample(&pcm),
            parent_entry_id: parent_edge,
            local_only: None,
            typed: false,
        },
    );

    match entry {
        Ok(entry) => {
            conn.execute(
                "UPDATE audio SET byte_size = ?2 WHERE entry_id = ?1",
                rusqlite::params![entry.id, bytes as i64],
            )?;
            Ok(entry)
        }
        Err(e) => {
            // No row, so the file is an orphan. Leaving it would accumulate
            // silently in the audio directory.
            let _ = std::fs::remove_file(full);
            Err(e)
        }
    }
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

/// Async for the same reason as stop_recording: this transcribes too.
#[tauri::command]
pub async fn undo_discard(state: State<'_, AppState>) -> Result<Option<Entry>> {
    // Read, do not take. The staged samples are the only copy, and consuming
    // them before the replacement is written would let a failure downstream
    // destroy exactly the recording the undo window exists to protect.
    let staged = {
        let slot = state.discarded.lock().unwrap_or_else(|p| p.into_inner());
        match slot.as_ref() {
            Some(d) if d.at.elapsed().as_millis() as u64 <= UNDO_WINDOW_MS => {
                Some((d.pcm.clone(), d.duration_ms))
            }
            _ => None,
        }
    };
    let Some((pcm, duration_ms)) = staged else {
        return Ok(None);
    };

    // Same pipeline as a fresh stop, so undo cannot drift from it. Parent and
    // question are still lost here -- the staging slot does not carry them.
    Ok(Some(finish(&state, pcm, duration_ms, None, None)?))
}
