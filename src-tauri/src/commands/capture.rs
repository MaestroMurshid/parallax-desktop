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

/// Takes the calling window, because the take belongs to it until it ends: the
/// hotkey is global and routes the stop back to whoever started it.
#[tauri::command]
pub fn start_recording(window: tauri::Window, state: State<AppState>) -> Result<()> {
    let mut slot = state.recording.lock().unwrap_or_else(|p| p.into_inner());
    if slot.is_some() {
        return Err(Error::Other("already recording".into()));
    }
    *slot = Some(crate::state::InFlight {
        take: recorder::start()?,
        owner: window.label().to_string(),
    });
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
        .map(|r| r.take.level())
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
            Some(in_flight) => in_flight.take.samples(),
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
        .ok_or_else(|| Error::Other("not recording".into()))?
        .take;

    let duration_ms = recording.elapsed_ms();
    let pcm = recording.stop();
    let entry = finish(&state, pcm, duration_ms, parent_edge, question_id)?;
    enrich_later(&app, entry.id.clone());
    Ok(entry)
}

/// One model call per candidate, so this is the cost of a capture. Eight is
/// the number Task 10 settled on: enough that a real connection is usually in
/// the set, few enough that the pass stays behind a single recording.
const PROPOSE_CAP: usize = 8;

/// Enrichment runs after the entry is safe on disk, never before. It is allowed
/// to be slow, absent or wrong, and none of that may cost a recording (§9.4).
///
/// Both ends of the pass are announced, and the settled event fires on every
/// exit including failure. A single event on success only would leave the
/// indicator spinning forever on the paths that are most likely to be taken --
/// no model installed, or a model that threw.
fn enrich_later(app: &tauri::AppHandle, entry_id: String) {
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        // Emitted before with_reasoning, which is where a cold llama-server is
        // started: the first pass after launch spends most of its time there,
        // and that wait is exactly what needs saying.
        let _ = app.emit("entry://enriching", &entry_id);
        let done = state
            .with_reasoning(|provider| crate::enrich::run::run(&state.db(), provider, &entry_id));

        // Separate from enrichment and after it, because it is ranking rather
        // than eligibility: topics already decided who this note can be
        // compared against, and the vector only orders them. An absent or
        // failing embedder therefore costs ordering and no connections at all.
        if let Err(e) = state
            .with_embedder(|embedder| crate::embed::embed_now(&state.db(), embedder, &entry_id))
        {
            eprintln!("embedding failed for {entry_id}: {e}");
        }
        // After embedding, because candidates are ordered by cosine and this
        // note's own vector has to exist for that to mean anything. And before
        // the settled event, or the indicator clears while the judge is still
        // working -- one model call per candidate is the slowest part of the
        // whole pass.
        if let Err(e) = state.with_reasoning(|provider| {
            crate::enrich::propose::propose(&state.db(), provider, &entry_id, PROPOSE_CAP)
        }) {
            eprintln!("proposing failed for {entry_id}: {e}");
        }

        match done {
            // No binary or no model yet: the question arrives when one lands.
            Ok(None) => {}
            Ok(Some(enriched)) => {
                if enriched.question_id.is_none() {
                    println!("classified {entry_id}, no question");
                }
            }
            Err(e) => eprintln!("enrichment failed for {entry_id}: {e}"),
        }
        let _ = app.emit("entry://enriched", &entry_id);
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
        .ok_or_else(|| Error::Other("not recording".into()))?
        .take;

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

#[cfg(test)]
mod tests {
    use super::*;

    /// `finish` takes its PCM as an argument precisely so this needs no
    /// microphone. A few hundred samples stand in for a take.
    fn a_take() -> Vec<f32> {
        (0..800).map(|i| (i as f32 * 0.02).sin() * 0.4).collect()
    }

    fn corpus(tag: &str) -> (AppState, std::path::PathBuf) {
        let root =
            std::env::temp_dir().join(format!("parallax-capture-{tag}-{}", uuid::Uuid::new_v4()));
        let state = AppState::open(root.clone()).expect("a corpus");
        (state, root)
    }

    /// §9.4 -- the audio is the record and the transcript is derived from it, so
    /// a corpus with no speech model installed still keeps the take. This is the
    /// state every install is in before the first download finishes.
    #[test]
    fn a_take_lands_with_no_speech_model_installed() {
        let (state, root) = corpus("no-model");

        let entry = finish(&state, a_take(), 4_200, None, None).expect("the take landed");

        assert_eq!(entry.transcript, "", "no model, so nothing was transcribed");
        assert_eq!(entry.duration_ms, 4_200);
        let wav = root.join(entry.audio_path.as_ref().expect("a recording"));
        assert!(wav.is_file(), "the recording is not on disk");
        assert!(
            std::fs::metadata(&wav).unwrap().len() > 0,
            "the recording is empty"
        );

        drop(state);
        let _ = std::fs::remove_dir_all(root);
    }

    /// The whole reason staging comes first. A recording cannot be re-made, so
    /// anything that fails after the microphone stops has to leave it
    /// recoverable -- the four defects fixed on 10 September were all this
    /// shape. Failure is induced by putting a file where the audio directory
    /// belongs: `wav::write` calls `create_dir_all` first, so simply deleting
    /// the directory is not a failure at all -- it is recreated.
    #[test]
    fn a_failure_after_the_microphone_stops_still_leaves_the_take() {
        let (state, root) = corpus("staged");
        let audio = state.audio_dir();
        std::fs::remove_dir_all(&audio).expect("no audio directory");
        std::fs::write(&audio, b"not a directory").expect("a file in its place");

        let failed = finish(&state, a_take(), 4_200, None, None);
        assert!(failed.is_err(), "writing the WAV should have failed");

        {
            let slot = state.discarded.lock().unwrap_or_else(|p| p.into_inner());
            let staged = slot.as_ref().expect("the take was not staged");
            assert_eq!(staged.pcm.len(), a_take().len(), "the take was truncated");
            assert_eq!(staged.duration_ms, 4_200);
        }

        drop(state);
        let _ = std::fs::remove_dir_all(root);
    }

    /// Staging is a safety net, not a second copy kept forever: once the entry
    /// is durable the slot is released, so an undo cannot resurrect a take that
    /// is already on the canvas.
    #[test]
    fn a_landed_take_is_no_longer_staged() {
        let (state, root) = corpus("released");

        finish(&state, a_take(), 4_200, None, None).expect("the take landed");

        assert!(
            state
                .discarded
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .is_none(),
            "the staging slot outlived the entry"
        );

        drop(state);
        let _ = std::fs::remove_dir_all(root);
    }
}
