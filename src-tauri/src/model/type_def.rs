//! User-defined types (§3.6), on the wire. Mirrors `TypeDefinition` in
//! `lib/scene/classification.ts`, which is the contract.

use super::entry::Role;
use serde::{Deserialize, Serialize};

/// How far a type may go on its own. `Retrieval` describes mode G, which is
/// edge-driven rather than a property of a type, so the editor never offers it
/// and a custom type may not claim it (`db::types` rejects the attempt).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProbeTier {
    Silent,
    Safe,
    Heavy,
    Retrieval,
}

impl ProbeTier {
    pub fn as_str(self) -> &'static str {
        match self {
            ProbeTier::Silent => "silent",
            ProbeTier::Safe => "safe",
            ProbeTier::Heavy => "heavy",
            ProbeTier::Retrieval => "retrieval",
        }
    }

    // Returns `Option`, not `Result`, because the wire format has no error to
    // carry; matching `std::str::FromStr`'s name (not its signature) would ask
    // its one caller to invent one.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<ProbeTier> {
        match s {
            "silent" => Some(ProbeTier::Silent),
            "safe" => Some(ProbeTier::Safe),
            "heavy" => Some(ProbeTier::Heavy),
            "retrieval" => Some(ProbeTier::Retrieval),
            _ => None,
        }
    }
}

/// Built-in marks are drawn paths keyed by role; user marks are one grapheme.
/// `None` on a built-in — it draws its role's glyph and claims no mark of its
/// own.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Mark {
    Glyph { id: Role },
    Char { char: String },
}

/// A user-defined (or built-in) note type, as stored and as sent over the wire.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TypeDef {
    pub id: String,
    pub label: String,
    pub built_in: bool,
    /// The sentence the classifier matches against; `match` is a Rust keyword,
    /// so the wire name is set explicitly rather than derived.
    #[serde(rename = "match")]
    pub match_text: String,
    pub prompt: Option<String>,
    pub tier: ProbeTier,
    /// Letterform binding — `None` draws the default letterform, no mark.
    pub role: Option<Role>,
    pub mark: Option<Mark>,
    /// Always true once stored: §3.6 rule 1 wanted an extra opt-in, but the
    /// tier narrowing in the gate is what actually has teeth (mirrors
    /// `resolveTypes` in classification.ts).
    pub auto_approved: bool,
}

/// What creating a type accepts. `id` is computed client-side from the label
/// (the same slug the editor already shows before submitting), not derived
/// here — a create that raced a rename would otherwise mint a different id
/// than what the confirmation showed.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewType {
    pub id: String,
    pub label: String,
    #[serde(rename = "match")]
    pub match_text: String,
    pub prompt: Option<String>,
    pub tier: ProbeTier,
    pub role: Option<Role>,
    pub mark: Option<Mark>,
}

/// What updating a type accepts. `id` and `builtIn` are not patchable — the id
/// is the join key everything else keys off, and a built-in's identity is not
/// the user's to take over.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TypePatch {
    pub label: String,
    #[serde(rename = "match")]
    pub match_text: String,
    pub prompt: Option<String>,
    pub tier: ProbeTier,
    pub role: Option<Role>,
    pub mark: Option<Mark>,
}
