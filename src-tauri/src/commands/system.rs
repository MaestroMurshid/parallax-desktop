//! Settings, and where the corpus lives.

use crate::db;
use crate::error::Result;
use crate::model::Settings;
use crate::state::AppState;
use tauri::State;

#[tauri::command]
pub fn get_settings(state: State<AppState>) -> Result<Settings> {
    let conn = state.db();
    db::settings::get(&conn)
}

/// Takes a partial and merges it, so the frontend can send one field without
/// having to round-trip the whole document first.
///
/// A hotkey rebind has to reach the OS registration here, not just the row --
/// nothing else in the app ever revisits a shortcut once it is registered.
#[tauri::command]
pub fn set_settings(
    app: tauri::AppHandle,
    state: State<AppState>,
    patch: serde_json::Value,
) -> Result<Settings> {
    let conn = state.db();
    let before = db::settings::get(&conn)?;
    let after = db::settings::merge(&conn, patch)?;
    drop(conn);
    if after.hotkey != before.hotkey {
        crate::shortcuts::rebind_hotkey(&app, state.inner(), &after.hotkey);
    }
    // A running server would otherwise keep answering from the old file --
    // `with_reasoning` only spawns one when the slot is empty.
    if crate::model::settings::reasoning_model_path_changed(&before, &after) {
        state.stop_reasoning();
    }
    Ok(after)
}

/// Where the corpus is. A tool holding your private thinking should be able to
/// tell you where it keeps it rather than making you go looking.
#[tauri::command]
pub fn corpus_location(state: State<AppState>) -> String {
    state.root.display().to_string()
}
