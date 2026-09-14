//! The judge: given two notes, what the edge between them says, or nothing.
//!
//! Nothing is the common answer and has to stay cheap to give. The candidate
//! filter hands over eight notes that share a shelf; most of those eight are
//! merely on the same subject, which is not a connection worth drawing (§5.4).
//!
//! Both quotes must be verbatim, for the reason §3.4 gives about questions: a
//! claim that cannot be checked against the transcript is not allowed to anchor
//! anything. `run::anchor` already refuses a quote that is not present, and one
//! that lands on words the speaker did not say.

use super::run::anchor;
use super::trim_quote;
use crate::error::Result;
use crate::llm::{Ask, LlmProvider};
use crate::model::{Entry, Relation, Span};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Debug, Clone)]
pub struct Proposal {
    pub relation: Relation,
    /// The proposal card renders this. An edge without one is a line with no
    /// reason on it, which is the thing §5.4 refuses.
    pub question: String,
    pub span_a: Span,
    pub span_b: Span,
}

/// `none` is a value rather than an empty reply, because a model that must
/// answer something answers; one that may decline by omission does not.
const NONE: &str = "none";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Reply {
    relation: String,
    #[serde(default)]
    quote_a: String,
    #[serde(default)]
    quote_b: String,
    #[serde(default)]
    question: String,
}

const CONNECT_SYSTEM: &str = "\
You are shown two notes someone recorded at different times. Say how the second relates to the first.
Follow these constraints strictly. Answer the fields in the exact order listed.

### Fields to Extract

- **relation**: What the second note does to the first. Choose exactly one:
  - `contradicts`: Asserts something that cannot hold alongside the first.
  - `extends`: Carries the first further on the same line of thought.
  - `same move`: Different subjects, same shape of argument.
  - `returns to`: Comes back to something the first left open.
  - `questions`: Doubts the first rather than answering it.
  - `example of`: A concrete instance of what the first says generally.
  - `none`: Default. Use if they are merely on similar subjects.
- **quoteA** & **quoteB**: Copy, word for word, the short phrase (at most 15 words) from each note that carries the relation.
- **question**: One sentence asking what the two together put to the speaker, about the claims, not the notes.";

fn connect_schema() -> Value {
    let relations: Vec<String> = Relation::MODEL_RELATIONS
        .iter()
        .filter_map(|r| serde_json::to_value(r).ok())
        .filter_map(|v| v.as_str().map(str::to_string))
        .collect();
    // Well under a sentence of speech. At 300 the model pasted whole notes and
    // the cap cut them mid-word; the prompt asks for 15 words, this catches drift.
    let quote = json!({ "type": "string", "maxLength": 160 });

    json!({
        // Two shapes rather than optional fields. Declining stays one token, and
        // naming a relation commits to everything the edge needs: with the
        // quotes and question optional, the model closed the object after one
        // quote and the relation it had found was thrown away.
        "anyOf": [
            {
                "type": "object",
                "properties": { "relation": { "const": NONE } },
                "required": ["relation"],
                "additionalProperties": false,
            },
            {
                "type": "object",
                "properties": {
                    "relation": { "type": "string", "enum": relations },
                    "quoteA": quote,
                    "quoteB": quote,
                    "question": { "type": "string", "maxLength": 300 },
                },
                "required": ["relation", "quoteA", "quoteB", "question"],
                "additionalProperties": false,
            },
        ],
    })
}

/// Anchors a quote that the schema's cap may have cut in half.
///
/// Measured over the twelve authored pairs, nothing anchored at all. Not
/// because the model paraphrased -- it quoted the **whole note** rather than
/// the passage, and `maxLength` then severed the final word: "doesn't feel
/// generally inteligl", and once a fragment of CJK. Cutting to whole words
/// from the front rescues the long ones, and dropping a severed tail rescues
/// the short ones, where there was nothing past twenty words to cut.
///
/// Retried once and no further. The point is a quote that is otherwise
/// entirely present, not a search for any substring that happens to match.
fn anchor_quote(entry: &Entry, raw: &str) -> Option<Span> {
    let trimmed = trim_quote(raw);
    if let Some(span) = anchor(entry, &trimmed) {
        return Some(span);
    }
    let mut words: Vec<&str> = trimmed.split_whitespace().collect();
    words.pop()?;
    if words.is_empty() {
        return None;
    }
    anchor(entry, &words.join(" "))
}

/// Two notes share one context, so each gets a little under half of what a
/// classification gives its one, less the connection prompt's own 213 tokens.
const JUDGE_TRANSCRIPT_BYTES: usize = 4_200;

/// What the edge from `a` to `b` says, or `None`.
pub fn judge(provider: &dyn LlmProvider, a: &Entry, b: &Entry) -> Result<Option<Proposal>> {
    // Dated, because `returns to` is a claim about time: the second note
    // coming back to the first is not the same as the two merely agreeing.
    let user = format!(
        "First note, {}:
{}

Second note, {}:
{}",
        a.created_at,
        super::within(&a.transcript, JUDGE_TRANSCRIPT_BYTES),
        b.created_at,
        super::within(&b.transcript, JUDGE_TRANSCRIPT_BYTES)
    );
    let ask = Ask::new(CONNECT_SYSTEM, &user).constrained(connect_schema());

    let raw = provider.ask(ask)?;
    let reply: Reply = super::repair_and_parse_json(&raw)?;

    if reply.relation.trim() == NONE {
        return Ok(None);
    }
    // The grammar allows only these seven, but the guard is not the grammar's
    // to keep: a scripted or remote provider is under no such constraint.
    let Ok(relation) = serde_json::from_value::<Relation>(Value::String(reply.relation)) else {
        return Ok(None);
    };
    // §5.4 -- `answers` is system-generated and `related` is the manual escape
    // hatch. If the best it can say is "related", it draws nothing.
    if !relation.is_model_emittable() {
        return Ok(None);
    }

    let question = reply.question.trim();
    if question.is_empty() {
        return Ok(None);
    }

    // Both, or neither. A connection anchored at one end is a claim about a
    // pair with only half of it checkable.
    let (Some(span_a), Some(span_b)) = (
        anchor_quote(a, &reply.quote_a),
        anchor_quote(b, &reply.quote_b),
    ) else {
        return Ok(None);
    };

    Ok(Some(Proposal {
        relation,
        question: question.to_string(),
        span_a,
        span_b,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::fake::FakeProvider;
    use crate::model::{Register, Role};

    /// Found in the packaged app: the corpus never got a single connection.
    /// The model named them -- `extends`, `questions` -- but sorted keys put
    /// `question` ahead of the quotes, the grammar follows that order, and once
    /// the model wrote a quote as the prompt asks, the question could no longer
    /// be written. Every proposal then failed the empty-question check. Measured
    /// on the real model: 0 of 4 authored pairs landed sorted, 4 of 4 in order.
    #[test]
    fn the_schema_lists_fields_in_the_order_the_prompt_asks_for_them() {
        let schema = connect_schema();
        let keys: Vec<&str> = related_branch(&schema)["properties"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(keys, ["relation", "quoteA", "quoteB", "question"]);
    }

    /// The branch of the schema a model takes when it names a relation.
    fn related_branch(schema: &Value) -> &Value {
        schema["anyOf"]
            .as_array()
            .expect("the schema offers declining and relating as separate shapes")
            .iter()
            .find(|b| b["properties"]["relation"]["enum"].is_array())
            .expect("a branch that names a relation")
    }

    /// Found replaying a real pair: the model named `returns to` four times out
    /// of four, then closed the object after one quote in three of them because
    /// the rest was optional, and each landed nothing for want of a question.
    #[test]
    fn naming_a_relation_requires_both_quotes_and_the_question() {
        let schema = connect_schema();
        let related = related_branch(&schema);
        let required: Vec<&str> = related["required"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .collect();
        assert_eq!(required, ["relation", "quoteA", "quoteB", "question"]);
        let offered: Vec<&str> = related["properties"]["relation"]["enum"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .collect();
        assert!(
            !offered.contains(&NONE),
            "none belongs to the declining branch"
        );
    }

    #[test]
    fn declining_still_costs_only_the_relation() {
        let schema = connect_schema();
        let declining = schema["anyOf"]
            .as_array()
            .unwrap()
            .iter()
            .find(|b| b["properties"]["relation"]["const"] == NONE)
            .expect("a branch that declines");
        assert_eq!(declining["required"], json!(["relation"]));
    }

    /// Asked for a "short passage", the model pasted whole notes into the
    /// quote until the cap cut them mid-word.
    #[test]
    fn quotes_are_asked_for_and_capped_short() {
        assert!(CONNECT_SYSTEM.contains("at most 15 words"));
        let schema = connect_schema();
        let cap = related_branch(&schema)["properties"]["quoteA"]["maxLength"]
            .as_u64()
            .unwrap();
        assert!(cap <= 160, "a quote cap of {cap} lets a whole note through");
    }

    const SAID_A: &str = "Database indexes trade write performance for faster reads.";
    const SAID_B: &str = "Retries can make distributed systems less reliable under load.";

    fn entry(id: &str, transcript: &str) -> Entry {
        Entry {
            id: id.into(),
            audio_path: None,
            transcript: transcript.into(),
            created_at: "2026-01-01T00:00:00Z".into(),
            x: 0.0,
            y: 0.0,
            parent_entry_id: None,
            answers_question_id: None,
            role: Role::Position,
            register: Register::Neutral,
            type_id: "position".into(),
            resolved: false,
            resolution_text: None,
            title: "t".into(),
            summary: None,
            duration_ms: 5_000,
            fingerprint: Vec::new(),
            unfinished: false,
            local_only: false,
            spans: Vec::new(),
            action_items: Vec::new(),
            is_sample: None,
        }
    }

    fn reply(relation: &str, qa: &str, qb: &str, question: &str) -> String {
        json!({
            "relation": relation, "quoteA": qa, "quoteB": qb, "question": question
        })
        .to_string()
    }

    /// The answer the filter makes common: eight notes share a shelf and most
    /// of them are merely on the same subject.
    #[test]
    fn none_is_not_a_proposal() {
        let p = FakeProvider::replying(&reply("none", "", "", ""));
        let got = judge(&p, &entry("a", SAID_A), &entry("b", SAID_B)).unwrap();
        assert!(got.is_none());
    }

    /// §5.4: `answers` is system-generated and `related` is the manual escape
    /// hatch. If the best a model can say is "related", it draws nothing.
    #[test]
    fn a_relation_the_model_may_not_emit_is_refused() {
        for forbidden in ["related", "answers"] {
            let p = FakeProvider::replying(&reply(
                forbidden,
                "Database indexes",
                "Retries can make",
                "Which cost is worth paying?",
            ));
            let got = judge(&p, &entry("a", SAID_A), &entry("b", SAID_B)).unwrap();
            assert!(got.is_none(), "{forbidden} must not become an edge");
        }
    }

    /// The same discipline §3.4 puts on a question: a claim that cannot be
    /// checked against the transcript does not get to anchor anything.
    #[test]
    fn a_quote_that_is_not_in_the_note_is_refused() {
        let p = FakeProvider::replying(&reply(
            "same move",
            "Database indexes",
            "something nobody said",
            "Do both trade one cost for another?",
        ));
        let got = judge(&p, &entry("a", SAID_A), &entry("b", SAID_B)).unwrap();
        assert!(got.is_none(), "an unanchored quote must not land");
    }

    /// An edge without a question is a line with no reason on it.
    #[test]
    fn a_proposal_without_a_question_is_refused() {
        let p = FakeProvider::replying(&reply(
            "same move",
            "Database indexes",
            "Retries can make",
            "   ",
        ));
        let got = judge(&p, &entry("a", SAID_A), &entry("b", SAID_B)).unwrap();
        assert!(got.is_none());
    }

    /// The measured failure, and it is not truncation alone. Near the cap the
    /// model corrupts the token it is cut on -- the note says "generally
    /// intelligent" and back came "generally inteligl", once a CJK fragment --
    /// so the quote diverges from the note rather than merely stopping short.
    /// Dropping the severed word is what rescues it.
    #[test]
    fn a_quote_corrupted_at_the_cap_still_anchors() {
        let p = FakeProvider::replying(&reply(
            "same move",
            "Database indexes trade write performance for faster rezds",
            "Retries can make distributed systems less reliable under loax",
            "Does each trade one cost for another?",
        ));
        let got = judge(&p, &entry("a", SAID_A), &entry("b", SAID_B))
            .unwrap()
            .expect("the corrupted tail should be dropped, not the whole quote");
        let a = SAID_A.to_string();
        assert_eq!(
            &a[got.span_a.start as usize..got.span_a.end as usize],
            "Database indexes trade write performance for faster"
        );
    }

    /// Retried once and no further: a quote that is mostly invented must not
    /// be whittled down until some fragment of it happens to match.
    #[test]
    fn a_mostly_invented_quote_is_not_whittled_until_it_fits() {
        let p = FakeProvider::replying(&reply(
            "same move",
            "Database indexes are stored as B-trees on spinning disks",
            "Retries can make distributed systems less reliable",
            "Does each trade one cost for another?",
        ));
        let got = judge(&p, &entry("a", SAID_A), &entry("b", SAID_B)).unwrap();
        assert!(got.is_none(), "only the final word may be dropped");
    }

    #[test]
    fn a_proposal_anchors_both_notes() {
        let p = FakeProvider::replying(&reply(
            "same move",
            "trade write performance",
            "less reliable under load",
            "Does each make the thing it improves worse somewhere else?",
        ));
        let got = judge(&p, &entry("a", SAID_A), &entry("b", SAID_B))
            .unwrap()
            .expect("a proposal");

        assert_eq!(got.relation, Relation::SameMove);
        assert!(got.question.starts_with("Does each"));
        // The spans have to point at the words that were quoted.
        let a = SAID_A.to_string();
        assert_eq!(
            &a[got.span_a.start as usize..got.span_a.end as usize],
            "trade write performance"
        );
        assert!(!got.span_a.attributed, "a proposal does not attribute");
    }

    /// Declining must be reachable, and the six must be offered under the
    /// names the database stores.
    #[test]
    fn the_schema_offers_the_six_and_a_way_out() {
        let schema = connect_schema();
        let mut values: Vec<String> = related_branch(&schema)["properties"]["relation"]["enum"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        let way_out = schema["anyOf"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|b| b["properties"]["relation"]["const"].as_str())
            .map(str::to_string);
        values.extend(way_out);

        assert!(values.contains(&NONE.to_string()), "{values:?}");
        assert_eq!(values.len(), Relation::MODEL_RELATIONS.len() + 1);
        for r in Relation::MODEL_RELATIONS {
            let name = serde_json::to_value(r).unwrap();
            assert!(
                values.contains(&name.as_str().unwrap().to_string()),
                "{r:?}"
            );
        }
        assert!(
            !values.iter().any(|v| v == "related" || v == "answers"),
            "neither is a classifier's to emit (§5.4)"
        );
    }
}
