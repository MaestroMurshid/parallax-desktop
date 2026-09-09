//! What the model adds to an entry, and what it is allowed to add it to.
//!
//! Enrichment generates and stores; `get_question` only reads. Nothing here is
//! called on demand from the UI.

pub mod gate;

use crate::error::{Error, Result};
use crate::llm::{Ask, LlmProvider};
use crate::model::{Entry, Register, Role};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Classification {
    pub title: String,
    pub role: Role,
    pub register: Register,
    pub type_id: String,
    pub summary: Option<String>,
    /// What the entry *does*, independent of its subject. §7.1 -- the thing
    /// that finds two notes making the same move in different vocabulary.
    pub move_phrase: String,
}

const CLASSIFY_SYSTEM: &str = "\
You are filing a spoken note. Answer about the note, never about the speaker.

role -- what the note does:
  position: the speaker's own reasoning, asserted with grounds
  evidence: a fact, a number, something noticed, or something being learned
  note: admin, lists, intents, reminders

register -- live when something personal is at stake in it, unresolved or raw;
neutral otherwise. When it is not clear, answer live.

title: three or four words taken from the speaker's own phrasing. Not a
description of the note. Lowercase unless the words are names.

summary: one line. Leave it empty when the register is live -- a tidy sentence
about something raw is worse than nothing.

movePhrase: what the note does as a move, with its subject removed, so that two
notes about different things can be recognised as doing the same thing. Say it \
as a verb phrase about an unnamed claim.";

/// The type list is built from the registry at call time, so a user-defined
/// type becomes a value the model may return -- and constrained decoding makes
/// returning one that does not exist structurally impossible.
fn classify_schema(type_ids: &[String]) -> Value {
    json!({
        "type": "object",
        "properties": {
            "title": { "type": "string" },
            "role": { "type": "string", "enum": ["position", "evidence", "note"] },
            "register": { "type": "string", "enum": ["live", "neutral"] },
            "typeId": { "type": "string", "enum": type_ids },
            "summary": { "type": "string" },
            "movePhrase": { "type": "string" },
        },
        "required": ["title", "role", "register", "typeId", "summary", "movePhrase"],
        "additionalProperties": false,
    })
}

pub fn classify(
    provider: &dyn LlmProvider,
    transcript: &str,
    type_ids: &[String],
) -> Result<Classification> {
    let reply = provider
        .ask(Ask::new(CLASSIFY_SYSTEM, transcript).constrained(classify_schema(type_ids)))?;

    let mut parsed: Classification = serde_json::from_str(&reply)
        .map_err(|e| Error::Other(format!("classification was not readable: {e}")))?;

    // §1.1 enforced here rather than trusted: a summary of a live entry
    // flattens the exact thing that made it worth keeping.
    if parsed.register == Register::Live {
        parsed.summary = None;
    }
    if parsed.summary.as_deref().is_some_and(str::is_empty) {
        parsed.summary = None;
    }
    Ok(parsed)
}

const QUESTION_SYSTEM: &str = "\
You ask one question about a note someone recorded. You may ask. You may not \
conclude.

Push on the reasoning, not the conclusion: \"this holds if X -- is X true?\" \
produces thinking, \"you are wrong about X\" produces a rebuttal.

One question. Not two, not a list. Five objections is an attack; one question \
is an invitation.

Speak about the note in the third person, never about the person who made it. \
\"The note treats X as settled\", never \"you believe X\".

Quote the span your question is about, exactly as it appears in the note, so \
the question can be checked rather than taken on trust.";

#[derive(Debug, Clone, Deserialize)]
pub struct Asked {
    pub text: String,
    /// Verbatim from the transcript, so the anchor can be located rather than
    /// trusted. Unanchored output is not allowed (§3.4).
    pub quote: String,
}

fn question_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "text": { "type": "string" },
            "quote": { "type": "string" },
        },
        "required": ["text", "quote"],
        "additionalProperties": false,
    })
}

/// The hint is the probe: which move to make on this entry. §3.6 -- the model
/// is given the stance rules and left to generate, rather than selecting from
/// an enum, so adding a mode is noticing a shape in output worth having.
pub fn ask_about(provider: &dyn LlmProvider, entry: &Entry, probe_hint: &str) -> Result<Asked> {
    let user = format!(
        "The note:\n\n{}\n\nWhat to ask: {}",
        entry.transcript, probe_hint
    );
    let reply = provider.ask(Ask::new(QUESTION_SYSTEM, &user).constrained(question_schema()))?;

    let asked: Asked = serde_json::from_str(&reply)
        .map_err(|e| Error::Other(format!("the question was not readable: {e}")))?;

    if asked.text.trim().is_empty() {
        return Err(Error::Other("the model returned an empty question".into()));
    }
    Ok(asked)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::fake::FakeProvider;

    fn entry(transcript: &str) -> Entry {
        Entry {
            id: "e1".into(),
            audio_path: None,
            transcript: transcript.into(),
            created_at: "2024-01-01T00:00:00Z".into(),
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
            duration_ms: 40_000,
            fingerprint: vec![],
            unfinished: false,
            local_only: false,
            spans: vec![],
            action_items: vec![],
            is_sample: None,
        }
    }

    #[test]
    fn a_classification_is_parsed() {
        let p = FakeProvider::replying(
            r#"{"title":"our own reasoning","role":"position","register":"neutral",
                "typeId":"position","summary":"Free will as your own reasoning.",
                "movePhrase":"redefines a test so it no longer requires an alternative"}"#,
        );
        let c = classify(
            &p,
            "I don't think free will requires...",
            &["position".into()],
        )
        .unwrap();

        assert_eq!(c.role, Role::Position);
        assert_eq!(c.register, Register::Neutral);
        assert_eq!(c.title, "our own reasoning");
        assert!(c.summary.is_some());
    }

    /// §1.1 -- a tidy sentence about something raw is worse than nothing, so a
    /// summary is dropped rather than trusted when the register is live.
    #[test]
    fn a_live_entry_never_keeps_a_summary() {
        let p = FakeProvider::replying(
            r#"{"title":"what I lost","role":"position","register":"live",
                "typeId":"position","summary":"Reflects on a relationship that ended.",
                "movePhrase":"states a loss"}"#,
        );
        let c = classify(&p, "...", &["position".into()]).unwrap();

        assert_eq!(c.register, Register::Live);
        assert!(
            c.summary.is_none(),
            "the model offered one and it was dropped"
        );
    }

    #[test]
    fn an_empty_summary_becomes_none_not_an_empty_string() {
        let p = FakeProvider::replying(
            r#"{"title":"a list","role":"note","register":"neutral","typeId":"note",
                "summary":"","movePhrase":"records errands"}"#,
        );
        assert!(classify(&p, "...", &["note".into()])
            .unwrap()
            .summary
            .is_none());
    }

    /// A user-defined type has to be a value the model may return, or it can
    /// never be assigned to anything.
    #[test]
    fn custom_types_are_offered_to_the_model() {
        let p = FakeProvider::replying(
            r#"{"title":"t","role":"position","register":"neutral","typeId":"wondering",
                "summary":"s","movePhrase":"m"}"#,
        );
        let types = vec!["position".to_string(), "wondering".to_string()];
        let c = classify(&p, "...", &types).unwrap();

        assert_eq!(c.type_id, "wondering");
        let schema = p.last_schema().unwrap();
        assert_eq!(schema["properties"]["typeId"]["enum"][1], "wondering");
    }

    /// Constrained decoding is what makes the shape guaranteed rather than
    /// hoped for, so the schema must actually be sent.
    #[test]
    fn the_reply_is_constrained_by_a_schema() {
        let p = FakeProvider::replying(r#"{"text":"why?","quote":"because"}"#);
        ask_about(&p, &entry("because of the thing"), "find the edge").unwrap();
        assert!(p.last_schema().is_some());
    }

    #[test]
    fn the_transcript_and_the_probe_both_reach_the_model() {
        let p = FakeProvider::replying(r#"{"text":"why?","quote":"indexes"}"#);
        ask_about(&p, &entry("indexes cost writes"), "what would break it").unwrap();

        let prompt = p.last_user_prompt();
        assert!(prompt.contains("indexes cost writes"));
        assert!(prompt.contains("what would break it"));
    }

    /// An empty question is a failure, not a question. Better to surface
    /// nothing than to render a blank one.
    #[test]
    fn an_empty_question_is_rejected() {
        let p = FakeProvider::replying(r#"{"text":"   ","quote":"x"}"#);
        assert!(ask_about(&p, &entry("something"), "hint").is_err());
    }

    #[test]
    fn unreadable_output_is_an_error_not_a_panic() {
        let p = FakeProvider::replying("not json at all");
        assert!(classify(&p, "...", &["position".into()]).is_err());
        assert!(ask_about(&p, &entry("x"), "hint").is_err());
    }
}
