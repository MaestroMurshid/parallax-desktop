//! Corpus reads and writes. Thin: every command locks the connection, calls
//! into `db`, and returns. The logic lives below this layer.

use crate::db;
use crate::db::create::NewEntry;
use crate::error::Result;
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
    let conn = state.db();
    db::entries::delete(&conn, &id)
}
