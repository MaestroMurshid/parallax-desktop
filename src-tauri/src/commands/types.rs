//! Type definition CRUD (§3.6). Thin, like the rest of `commands/` — the
//! validation and the delete-time fallback live in `db::types`.

use crate::db;
use crate::error::Result;
use crate::model::{NewType, TypeDef, TypePatch};
use crate::state::AppState;
use tauri::State;

#[tauri::command]
pub fn list_types(state: State<AppState>) -> Result<Vec<TypeDef>> {
    let conn = state.db();
    db::types::list(&conn)
}

#[tauri::command]
pub fn create_type(state: State<AppState>, draft: NewType) -> Result<TypeDef> {
    let conn = state.db();
    db::types::create(&conn, draft)
}

#[tauri::command]
pub fn update_type(state: State<AppState>, id: String, patch: TypePatch) -> Result<TypeDef> {
    let conn = state.db();
    db::types::update(&conn, &id, patch)
}

/// Notes carrying this type fall back to their own role's built-in id, in the
/// same transaction as the delete (`db::types::delete`) — so the state
/// guard needs a mutable connection, not the shared one every other command
/// here borrows.
#[tauri::command]
pub fn delete_type(state: State<AppState>, id: String) -> Result<()> {
    let mut conn = state.db();
    db::types::delete(&mut conn, &id)
}
