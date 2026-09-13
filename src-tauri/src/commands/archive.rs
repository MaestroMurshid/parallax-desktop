//! Export and upload through the system's own file dialogs.
//!
//! The dialogs are opened from Rust rather than the page. A browser download
//! from WebView2 saved silently to Downloads with no say in where, and the
//! second one in a session raised a "download multiple files" prompt whose
//! Block would have stopped every export after it.
//!
//! All async: a blocking dialog on the main thread deadlocks it, and writing
//! recordings into an archive is seconds of disk work the window must not
//! wait behind.

use crate::db::import::ImportMode;
use crate::error::{Error, Result};
use crate::mdx::{self, archive};
use crate::state::AppState;
use serde::Serialize;
use std::path::PathBuf;
use tauri::{Manager, State};
use tauri_plugin_dialog::DialogExt;

fn dialog(app: &tauri::AppHandle) -> tauri_plugin_dialog::FileDialogBuilder<tauri::Wry> {
    let builder = app.dialog().file();
    // Parented, so the dialog sits over the app instead of behind it.
    match app.get_webview_window("main") {
        Some(window) => builder.set_parent(&window),
        None => builder,
    }
}

fn chosen(path: Option<tauri_plugin_dialog::FilePath>) -> Option<PathBuf> {
    path.and_then(|p| p.as_path().map(|p| p.to_path_buf()))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Exported {
    pub path: String,
    #[serde(flatten)]
    pub written: archive::Written,
}

/// The corpus as one zip of MDX notes, with the recordings when asked for.
/// `None` when the dialog was cancelled.
#[tauri::command]
pub async fn export_archive(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    with_audio: bool,
) -> Result<Option<Exported>> {
    let day = chrono::Local::now().format("%Y-%m-%d");
    let suggested = if with_audio {
        format!("parallax-{day}.zip")
    } else {
        format!("parallax-notes-{day}.zip")
    };
    let Some(out) = chosen(
        dialog(&app)
            .set_title("Export notes")
            .set_file_name(suggested)
            .add_filter("Parallax archive", &["zip"])
            .blocking_save_file(),
    ) else {
        return Ok(None);
    };

    // Read under the lock, written without it.
    let notes = {
        let conn = state.background_db();
        mdx::corpus::export(&conn)?
    };
    let written = archive::write_notes(&notes, &state.root, &out, with_audio)?;
    Ok(Some(Exported {
        path: out.display().to_string(),
        written,
    }))
}

/// Transcripts as Markdown, rendered by the page and saved where the person
/// chooses. `None` when the dialog was cancelled.
#[tauri::command]
pub async fn export_transcripts(app: tauri::AppHandle, contents: String) -> Result<Option<String>> {
    let day = chrono::Local::now().format("%Y-%m-%d");
    let Some(out) = chosen(
        dialog(&app)
            .set_title("Export transcripts")
            .set_file_name(format!("transcripts-{day}.md"))
            .add_filter("Markdown", &["md"])
            .blocking_save_file(),
    ) else {
        return Ok(None);
    };
    std::fs::write(&out, contents)?;
    Ok(Some(out.display().to_string()))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadPreview {
    pub file_name: String,
    pub notes: usize,
    pub edges: usize,
    pub questions: usize,
    pub recordings: usize,
}

/// Picks and reads an upload, and holds it until a mode is chosen. Reading
/// first is what lets the choice show what the file holds -- and a file that
/// does not read is refused before anyone is asked to replace their corpus
/// with it.
#[tauri::command]
pub async fn pick_upload(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<UploadPreview>> {
    let Some(path) = chosen(
        dialog(&app)
            .set_title("Upload notes")
            .add_filter("Parallax archive or export", &["zip", "json"])
            .blocking_pick_file(),
    ) else {
        return Ok(None);
    };

    let contents = archive::read(&path)?;
    let preview = UploadPreview {
        file_name: path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        notes: contents.notes.len(),
        edges: contents.notes.iter().map(|n| n.edges.len()).sum(),
        questions: contents.notes.iter().map(|n| n.questions.len()).sum(),
        recordings: contents.audio.len(),
    };
    *state.pending_upload.lock().unwrap_or_else(|p| p.into_inner()) = Some(contents);
    Ok(Some(preview))
}

/// Applies the upload `pick_upload` read.
#[tauri::command]
pub async fn apply_upload(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    mode: ImportMode,
) -> Result<()> {
    let contents = state
        .pending_upload
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .take()
        .ok_or_else(|| Error::Other("there is no upload waiting".into()))?;

    let orphaned = {
        let conn = state.background_db();
        archive::restore(&conn, &state.root, &contents, mode)?
    };
    // The rows are gone either way; a failed unlink costs a file on disk, which
    // is the safe direction and the same one `delete_entry` takes.
    for relative in orphaned {
        state.remove_audio(&relative);
    }

    // The archive carries no vectors, and a replace took the old ones with it.
    // Without these `ask` finds nothing, so every restored note is embedded now
    // rather than five per recording. After returning, so the upload itself is
    // not held up; the embedder is small and on the CPU.
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        match state.with_embedder(|e| crate::embed::catch_up(state.background_conn(), e)) {
            Ok(Some(n)) => println!("embedded {n} uploaded notes"),
            Ok(None) => {}
            Err(e) => eprintln!("embedding the upload failed: {e}"),
        }
    });
    Ok(())
}
