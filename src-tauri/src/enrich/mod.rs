//! What the model adds to an entry, and what it is allowed to add it to.
//!
//! Enrichment generates and stores; `get_question` only reads. Nothing here is
//! called on demand from the UI.

pub mod gate;
pub mod invoke;
pub mod run;

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

Answer the fields in the order they are listed. An earlier answer cannot be
revised once a later one has been given.

title: three or four words taken from the speaker's own phrasing. Not a
description of the note. Lowercase unless the words are names.

role -- what the note mostly does:
  evidence: reports something observed, measured or learned. A figure, a reading
  or a result is actually in it. Saying a measurement has not been taken is not
  evidence; it is the absence of one.
  note: records something to do or to keep -- errands, lists, intents,
  reminders. Mostly items means note.
  position: the speaker's own reasoning, asserted with grounds. It argues,
  weighs or doubts rather than reporting or listing.
A note that weighs a tradeoff, doubts itself, or admits it has not checked
something is arguing, so it is a position even where measuring is mentioned.

register -- live when something personal is at stake in it: the speaker's own
life, a relationship, work they might leave, something raw or unresolved about
themselves. Neutral otherwise, and that includes a note that is uncertain,
weighing a tradeoff, or admitting it has not checked something. Doubt about an
idea is not personal stake. Answer live only when personal stake is genuinely
unclear.

typeId: which drawer the note is filed in. Use the value equal to role unless
another allowed value plainly fits the note better.

summary: one line, third person, saying what the note says. Required whenever
register is neutral -- an empty summary there is an error. When register is
live, return an empty string and nothing else: a tidy sentence about something
raw is worse than nothing.

movePhrase: what the note does as a move, with its subject removed, so that two
notes about different things can be recognised as doing the same thing. Say it
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
    // 400 truncated a real summary mid-string, and a constrained reply that stops
    // early is unparseable rather than short.
    let mut ask = Ask::new(CLASSIFY_SYSTEM, transcript).constrained(classify_schema(type_ids));
    ask.max_tokens = 700;
    let reply = provider.ask(ask)?;

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
    parsed.title = trim_title(&parsed.title);
    Ok(parsed)
}

/// A span, enforced rather than asked for: asked for under twenty words it
/// returned twenty-four and thirty-six. Cutting on a word boundary keeps a
/// verbatim substring verbatim, so the anchor still resolves.
fn trim_quote(quote: &str) -> String {
    const MOST: usize = 20;
    let words: Vec<&str> = quote.split_whitespace().collect();
    if words.len() <= MOST {
        return words.join(" ");
    }
    words[..MOST].join(" ")
}

/// Three or four words, enforced rather than asked for. A 4B model cannot count
/// and returned six reliably, and a long title widens the box placement was
/// already solved against.
fn trim_title(title: &str) -> String {
    const MOST: usize = 4;
    let words: Vec<&str> = title.split_whitespace().collect();
    if words.len() <= MOST {
        return words.join(" ");
    }
    words[..MOST].join(" ")
}

const QUESTION_SYSTEM: &str = "\
You ask one question about a note someone recorded. You may ask. You may not
conclude. Answer the two fields in the order they are listed.

quote: first choose the passage the question will be about, and copy it out of
the note. The shortest passage that carries the claim -- a phrase, under twenty
words, never the whole note. Transcribe it rather than recall it: read the
characters off the note in order. Substituting a word the note uses elsewhere
makes the question uncheckable, and it is discarded.

text: then write the question about that passage. One question, ending in a
question mark. Never the passage again, never a statement, never a list.

The move you are asked to make decides the shape of the question, and it is not
always a request for evidence. Asked where something stops holding, name the
condition it needs. Asked what would overturn it, ask for the observation that
would. Asked to steelman, put the claim at its strongest and then press the part
that is still weak. Asked to follow the reasons down, ask what the reason given
rests on in turn. Asked to apply it somewhere new, bring a case the note has not
considered and ask what it gives there. Follow the move; do not fall back on
asking for evidence every time.

Push on the reasoning, not the conclusion: \"this holds if X -- is X true?\"
produces thinking, \"you are wrong about X\" produces a rebuttal. One question is
an invitation; five objections is an attack.

Aim at the load-bearing part -- the assumption the rest of it rests on -- and not
at the topic. A question that asks what something means, or asks for a
definition, moves nothing; a question that asks what it would take for the claim
to be wrong, or what it commits the speaker to, moves a great deal.

Ask about the claim, never about the note and never about the speaker. Asking
what the note means, considers, or treats as true makes someone interpret their
own words back, and nothing moves. Do not write \"does the note\", \"according to
the note\", or \"the note considers\". Name the thing itself.

Open with what, which, where, how or why. A question opening with is, does, can,
has or would can be answered with yes, and yes is not an answer.

It has to be answerable out loud, in a sentence or two, out of what the speaker
already knows. Not a literature question and not a research task.";

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
            "quote": { "type": "string" },
            "text": { "type": "string" },
        },
        "required": ["quote", "text"],
        "additionalProperties": false,
    })
}

/// The hint is the probe: which move to make on this entry. §3.6 -- the model
/// is given the stance rules and left to generate, rather than selecting from
/// an enum, so adding a mode is noticing a shape in output worth having.
pub fn ask_about(provider: &dyn LlmProvider, entry: &Entry, probe_hint: &str) -> Result<Asked> {
    ask_about_passage(provider, entry, probe_hint, None)
}

/// The invoked path names the passage the user selected (§3.6). The whole
/// transcript still goes with it: a sentence on its own is not enough to ask a
/// question that lands, and the quote has to be verbatim in the note anyway.
pub fn ask_about_passage(
    provider: &dyn LlmProvider,
    entry: &Entry,
    probe_hint: &str,
    passage: Option<&str>,
) -> Result<Asked> {
    let selected = match passage {
        Some(passage) => format!("\nThe passage to ask about:\n\n{passage}\n"),
        None => String::new(),
    };
    let user = format!(
        "The note:\n\n{}\n{selected}\nWhat to ask: {}",
        entry.transcript, probe_hint
    );
    let reply = provider.ask(Ask::new(QUESTION_SYSTEM, &user).constrained(question_schema()))?;

    let mut asked: Asked = serde_json::from_str(&reply)
        .map_err(|e| Error::Other(format!("the question was not readable: {e}")))?;

    if asked.text.trim().is_empty() {
        return Err(Error::Other("the model returned an empty question".into()));
    }
    asked.quote = trim_quote(&asked.quote);
    Ok(asked)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::fake::FakeProvider;

    #[test]
    fn a_title_within_four_words_is_left_alone() {
        assert_eq!(trim_title("indexing dilemma"), "indexing dilemma");
        assert_eq!(
            trim_title("free will and reflection"),
            "free will and reflection"
        );
    }

    /// Measured: asked for four, it returned six.
    #[test]
    fn a_longer_title_is_cut_to_four_words() {
        assert_eq!(
            trim_title("renew the domain before the twentieth"),
            "renew the domain before"
        );
    }

    /// Cutting on a word boundary has to leave it findable in the transcript.
    #[test]
    fn a_long_quote_is_cut_but_stays_verbatim() {
        let said = "one two three four five six seven eight nine ten eleven twelve                     thirteen fourteen fifteen sixteen seventeen eighteen nineteen                     twenty twenty-one twenty-two";
        let said: String = said.split_whitespace().collect::<Vec<_>>().join(" ");
        let cut = trim_quote(&said);
        assert_eq!(cut.split_whitespace().count(), 20);
        assert!(said.contains(&cut), "a cut quote must still be in the note");
    }

    #[test]
    fn a_short_quote_is_left_alone() {
        assert_eq!(
            trim_quote("the reads anybody waits on"),
            "the reads anybody waits on"
        );
    }

    #[test]
    fn a_title_is_not_left_padded_with_whitespace() {
        assert_eq!(trim_title("  indexing   dilemma  "), "indexing dilemma");
    }

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
