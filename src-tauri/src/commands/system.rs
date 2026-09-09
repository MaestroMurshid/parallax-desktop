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
#[tauri::command]
pub fn set_settings(state: State<AppState>, patch: serde_json::Value) -> Result<Settings> {
    let conn = state.db();
    db::settings::merge(&conn, patch)
}

/// Where the corpus is. A tool holding your private thinking should be able to
/// tell you where it keeps it rather than making you go looking.
#[tauri::command]
pub fn corpus_location(state: State<AppState>) -> String {
    state.root.display().to_string()
}
