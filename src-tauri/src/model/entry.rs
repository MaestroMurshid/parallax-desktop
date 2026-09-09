//! Entry and its parts. Mirrors `lib/types.ts` exactly -- the TS file is the
//! contract, and serde has to reproduce its shape byte for byte or the UI
//! silently reads `undefined`.
//!
//! Java notes:
//!   #[derive(Serialize, Deserialize)]  ~  Jackson on a POJO, but generated at
//!                                        compile time rather than by reflection
//!   #[serde(rename_all = "camelCase")] ~  @JsonNaming(SnakeCaseStrategy)
//!   Option<T>                          ~  Optional<T>, except null does not
//!                                        exist in the language at all

use serde::{Deserialize, Serialize};

/// Facet 1 -- what the entry *does*.
///
/// Java: `enum Role { POSITION, EVIDENCE, NOTE }`. The rename_all makes it
/// serialise as "position", not "Position".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Position,
    Evidence,
    Note,
}

/// Facet 2 -- emotionally live? Gates the automatic question only.
///
/// Defaults to `Live` under uncertainty: a false `live` costs a missed
/// question, a false `neutral` costs the thing that cannot be taken back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Register {
    Live,
    Neutral,
}

/// Facet 3 -- provenance, per span. `attributed` = someone else's words.
/// The app may only push on an *own* span.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Span {
    pub start: u32,
    pub end: u32,
    pub attributed: bool,
}

/// An inert type (§1.2): never initiates a question, never nags.
/// Ticking is state on the span, never a mutation of the transcript.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionItem {
    pub id: String,
    pub entry_id: String,
    pub span: Span,
    pub text: String,
    pub done: bool,
}

/// The central record.
///
/// Note `parent_edge`: the name says edge, the value is a *parent entry id*.
/// That trap is inherited from the wire format, so it is renamed at the serde
/// boundary and kept honest everywhere inside Rust.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub id: String,
    /// `None` => typed entry, which is also why it has no fingerprint.
    pub audio_path: Option<String>,
    /// The record. Verbatim, never rewritten, never summarised over.
    pub transcript: String,
    pub created_at: String,

    /// Frozen at insert, never recomputed (§5.1). Position encodes *when*.
    pub x: f64,
    pub y: f64,

    /// Set => this entry answers the entry named here. An entry id, not an edge id.
    #[serde(rename = "parentEdge")]
    pub parent_entry_id: Option<String>,
    pub answers_question_id: Option<String>,

    pub role: Role,
    pub register: Register,
    /// Role id for built-ins, or a user-defined type's id (§3.6).
    pub type_id: String,

    /// User-declared only. The AI never decides you are done thinking (§6.3).
    pub resolved: bool,
    pub resolution_text: Option<String>,

    // --- enrichment: added to the record, never replacing it ---
    /// 3-4 words, from the user's own phrasing where possible (§5.2).
    pub title: String,
    /// Must be `None` when register is `Live` (§1.1).
    pub summary: Option<String>,

    pub duration_ms: i64,
    /// 7-9 samples downsampled from real amplitude. Empty when typed.
    pub fingerprint: Vec<f32>,

    /// §5.3 -- detected by regex, never by a model.
    pub unfinished: bool,
    pub local_only: bool,

    pub spans: Vec<Span>,
    pub action_items: Vec<ActionItem>,

    /// Optional on the wire, so a missing field must not be an error.
    /// `skip_serializing_if` keeps it absent rather than emitting null.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_sample: Option<bool>,
}
