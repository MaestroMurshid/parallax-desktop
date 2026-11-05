//! Settings, and where the corpus lives.

use crate::db;
use crate::error::{Error, Result};
use crate::llm::{keys, remote::Endpoint, Ask};
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
    if crate::model::settings::reasoning_model_path_changed(&before, &after)
        || crate::model::settings::reasoning_provider_changed(&before, &after)
    {
        state.stop_reasoning();
    }
    Ok(after)
}

/// Whether a key is saved for the API at `base_url`. A boolean only: a key
/// goes into the keychain from here and never comes back out to the window.
#[tauri::command]
pub fn api_key_saved(base_url: String) -> Result<bool> {
    Ok(keys::has(&Endpoint::parse(&base_url)?))
}

#[tauri::command]
pub fn set_api_key(base_url: String, key: String) -> Result<()> {
    keys::set(&Endpoint::parse(&base_url)?, &key)
}

#[tauri::command]
pub fn clear_api_key(base_url: String) -> Result<()> {
    keys::clear(&Endpoint::parse(&base_url)?)
}

/// One short call against whatever reasoning is chosen, so a wrong key or
/// model name is found in Settings rather than as notes that never file.
/// Async so the window does not freeze for the seconds a call can take.
#[tauri::command]
pub async fn test_reasoning(state: State<'_, AppState>) -> Result<String> {
    let reply = state.with_reasoning(|provider| {
        let answer = provider.ask(Ask::new(
            "You are checking a connection. Reply with the single word: ok",
            "ping",
        ))?;
        Ok((provider.name(), answer))
    })?;
    match reply {
        Some((name, _)) => Ok(format!("{name} answered")),
        None => Err(Error::Other("no reasoning model is set up yet".into())),
    }
}

/// Where the corpus is. A tool holding your private thinking should be able to
/// tell you where it keeps it rather than making you go looking.
#[tauri::command]
pub fn corpus_location(state: State<AppState>) -> String {
    state.root.display().to_string()
}
