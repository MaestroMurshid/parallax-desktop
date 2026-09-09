//! Edges and the relation vocabulary.

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
    /// The six a model may propose. `Answers` is system-generated and
    /// `Related` is the manual escape hatch, so neither is a classifier's
    /// to emit — if the best it can say is "related", it draws nothing (§5.4).
    pub const MODEL_RELATIONS: [Relation; 6] = [
        Relation::Contradicts,
        Relation::SameMove,
        Relation::ReturnsTo,
        Relation::Questions,
        Relation::Extends,
        Relation::ExampleOf,
    ];

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
