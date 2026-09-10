//! Questions, edges and action items.
//!
//! Enrichment generates and stores; these commands only read. Nothing here
//! calls a model -- when the enrichment pass exists it writes ahead of them,
//! so a question is surfaced rather than produced on demand.

use crate::db;
use crate::error::Result;
use crate::model::{ActionItem, Edge, EdgeStatus, Question, Relation};
use crate::state::AppState;
use tauri::State;

/// The oldest open question, or nothing. `None` is an ordinary answer here:
/// most entries are not eligible for one at all.
#[tauri::command]
pub fn get_question(state: State<AppState>, entry_id: String) -> Result<Option<Question>> {
    let conn = state.db();
    Ok(db::questions::list_for(&conn, &entry_id)?
        .into_iter()
        .find(|q| !q.answered && !q.dismissed))
}

/// One query for the whole corpus. The store previously fetched a question per
/// entry, which is fine against an in-process mock and an N+1 across IPC.
#[tauri::command]
pub fn list_questions(state: State<AppState>) -> Result<Vec<Question>> {
    let conn = state.db();
    db::questions::list(&conn)
}

#[tauri::command]
pub fn dismiss_question(
    state: State<AppState>,
    entry_id: String,
    question_id: String,
) -> Result<()> {
    let conn = state.db();
    db::questions::dismiss(&conn, &entry_id, &question_id)
}

#[tauri::command]
pub fn list_edges(state: State<AppState>) -> Result<Vec<Edge>> {
    let conn = state.db();
    db::edges::list(&conn)
}

#[tauri::command]
pub fn list_proposed_edges(state: State<AppState>, entry_id: String) -> Result<Vec<Edge>> {
    let conn = state.db();
    db::edges::list_proposed_for(&conn, &entry_id)
}

#[tauri::command]
pub fn accept_edge(state: State<AppState>, edge_id: String) -> Result<()> {
    let conn = state.db();
    db::edges::set_status(&conn, &edge_id, EdgeStatus::Accepted)
}

/// Kept, not deleted: a dismissal says the model was wrong about a specific
/// pair, which is the negative example a local prompt bank needs.
#[tauri::command]
pub fn dismiss_edge(state: State<AppState>, edge_id: String) -> Result<()> {
    let conn = state.db();
    db::edges::set_status(&conn, &edge_id, EdgeStatus::Dismissed)
}

/// A person may say `related` when they know two notes belong together and
/// cannot yet say why. A model may not -- that is the escape hatch section 5.4
/// exists to close.
#[tauri::command]
pub fn create_manual_edge(
    state: State<AppState>,
    entry_a: String,
    entry_b: String,
    relation: Relation,
) -> Result<Edge> {
    let conn = state.db();
    let edge = Edge {
        id: format!("edge-manual-{}", uuid::Uuid::new_v4()),
        entry_a,
        entry_b,
        relation,
        question: None,
        status: EdgeStatus::Manual,
        created_at: chrono::Utc::now().to_rfc3339(),
    };
    db::edges::insert(&conn, &edge)?;
    Ok(edge)
}

#[tauri::command]
pub fn list_action_items(state: State<AppState>) -> Result<Vec<ActionItem>> {
    let conn = state.db();
    db::action_items::list(&conn)
}

#[tauri::command]
pub fn set_action_item_done(state: State<AppState>, id: String, done: bool) -> Result<()> {
    let conn = state.db();
    db::action_items::set_done(&conn, &id, done)
}
