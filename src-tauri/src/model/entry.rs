//! Entry and its parts. Mirrors `lib/types.ts`, which is the contract.

use serde::{Deserialize, Serialize};

/// Facet 1 — what the entry does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Position,
    Evidence,
    Note,
}

/// Facet 2. Defaults to `Live` under uncertainty: a false `live` costs a
/// missed question, a false `neutral` costs the thing that cannot be undone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Register {
    Live,
    Neutral,
}

/// Facet 3 — provenance. `attributed` is someone else's words, and the app
/// may only push on a span that is the user's own (§7.3).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Span {
    pub start: u32,
    pub end: u32,
    pub attributed: bool,
}

/// Anchored to the span it came from, so ticking never edits the transcript.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionItem {
    pub id: String,
    pub entry_id: String,
    pub span: Span,
    pub text: String,
    pub done: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub id: String,
    /// `None` => typed entry, which is why it also has no fingerprint.
    pub audio_path: Option<String>,
    /// The record. Verbatim, never rewritten, never summarised over.
    pub transcript: String,
    pub created_at: String,

    /// Frozen at insert and never recomputed (§5.1). Position encodes *when*.
    pub x: f64,
    pub y: f64,

    /// An entry id, not an edge id, despite what the wire calls it.
    #[serde(rename = "parentEdge")]
    pub parent_entry_id: Option<String>,
    pub answers_question_id: Option<String>,

    pub role: Role,
    pub register: Register,
    pub type_id: String,

    /// User-declared only. The AI never decides you are done thinking (§6.3).
    pub resolved: bool,
    pub resolution_text: Option<String>,

    pub title: String,
    /// `None` when register is `Live` — flattening those is worse than useless.
    pub summary: Option<String>,

    pub duration_ms: i64,
    /// 7–9 samples downsampled from real amplitude. Empty when typed.
    pub fingerprint: Vec<f32>,

    /// Detected by regex, never by a model (§5.3).
    pub unfinished: bool,
    pub local_only: bool,

    pub spans: Vec<Span>,
    pub action_items: Vec<ActionItem>,

    /// Optional on the wire: absent rather than null.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_sample: Option<bool>,
}
