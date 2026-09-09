//! The one voice in the app that is not the user's.

use super::entry::Span;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Question {
    pub id: String,
    pub entry_id: String,
    pub text: String,
    /// `None` only where the anchor could not be resolved, never as a default:
    /// every analytical claim quotes a span (§3.4).
    pub span: Option<Span>,
    pub answered: bool,
    /// Struck out, kept. Dismissals are training signal, so they persist.
    pub dismissed: bool,
    /// Shown verbatim — the user always knows who answered.
    pub provider_name: String,
    pub created_at: String,
}
