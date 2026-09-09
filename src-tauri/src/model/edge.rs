//! Edges and the relation vocabulary.
//!
//! Two sets, and the distinction has teeth: `Relation` is everything that can
//! exist; `MODEL_RELATIONS` is the subset a classifier may propose. `Answers`
//! is created by the system when an entry answers another; `Related` is the
//! human escape hatch for "I know these belong together and cannot yet say
//! why". A model that can only say "related" must draw nothing (§5.4).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Relation {
    #[serde(rename = "contradicts")]
    Contradicts,
    #[serde(rename = "same move")]
    SameMove,
    #[serde(rename = "returns to")]
    ReturnsTo,
    #[serde(rename = "questions")]
    Questions,
    #[serde(rename = "extends")]
    Extends,
    #[serde(rename = "example of")]
    ExampleOf,
    #[serde(rename = "answers")]
    Answers,
    #[serde(rename = "related")]
    Related,
}

impl Relation {
    /// The six a model may emit. `Answers` is system-generated and `Related`
    /// is manual-only, so neither belongs to a classifier's vocabulary.
    pub const MODEL_RELATIONS: [Relation; 6] = [
        Relation::Contradicts,
        Relation::SameMove,
        Relation::ReturnsTo,
        Relation::Questions,
        Relation::Extends,
        Relation::ExampleOf,
    ];

    /// Guard for the edge-proposal path: reject anything outside the six.
    pub fn is_model_emittable(&self) -> bool {
        Self::MODEL_RELATIONS.contains(self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EdgeStatus {
    Proposed,
    Accepted,
    Dismissed,
    Manual,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Edge {
    pub id: String,
    pub entry_a: String,
    pub entry_b: String,
    pub relation: Relation,
    /// A question the connection carries, shown on the proposal card.
    pub question: Option<String>,
    pub status: EdgeStatus,
    pub created_at: String,
}
