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
    /// Specific and grounded in the note's own words. Measured: anchors share
    /// 0 of 120 pairs, so they are never a retrieval key -- they are what the
    /// note is about, for the reader and for the MDX frontmatter.
    #[serde(default)]
    pub anchors: Vec<String>,
    /// Broad, and deliberately allowed to be ungrounded: the shelf a librarian
    /// would file the note under. This is the only field candidates come from,
    /// at 20 of 120 pairs and 6 of 12 authored edges.
    #[serde(default)]
    pub topics: Vec<String>,
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
as a verb phrase about an unnamed claim.

anchors: two or three. What this note is specifically about, in the speaker's
own words -- the thing it argues about, the system it describes, the mechanism
it turns on. Use words that are actually in the note, and be precise:
hash-table-lookup, write-amplification, sunk-cost. An anchor whose words are
not in the note is discarded.

topics: one or two, and these are the opposite. A topic is the broad field a
librarian would shelve this note under, so that a note about b-trees and a note
about lock contention end up on the same shelf. It does not have to appear in
the note: a note about hash table lookup has the topic databases even if the
word database is never said. Prefer the ordinary, obvious name for the field.
Do not invent a topic narrower than the field, and do not reach for one so
broad it would fit any note at all.";

/// The type list is built from the registry at call time, so a user-defined
/// type becomes a value the model may return -- and constrained decoding makes
/// returning one that does not exist structurally impossible.
fn classify_schema(type_ids: &[String]) -> Value {
    json!({
        "type": "object",
        "properties": {
            // maxLength is the runaway guard, not the shape. Measured: the
            // model loops inside an unbounded string until the token ceiling
            // and the JSON never terminates. The readable cut is `trim_phrase`
            // and `trim_title`, because the grammar cuts mid-word.
            "title": { "type": "string", "maxLength": 80 },
            "role": { "type": "string", "enum": ["position", "evidence", "note"] },
            "register": { "type": "string", "enum": ["live", "neutral"] },
            "typeId": { "type": "string", "enum": type_ids },
            "summary": { "type": "string", "maxLength": 400 },
            "movePhrase": { "type": "string", "maxLength": 200 },
            // Neither is an enum of what the corpus already has. Measured:
            // once an enum exists the model never coins again, fills the array
            // by repeating the one permitted value to maxItems, and picks a
            // listed value even when none fit. The vocabulary froze at one tag
            // across the whole corpus.
            "anchors": { "type": "array", "items": { "type": "string" }, "maxItems": 3 },
            "topics": { "type": "array", "items": { "type": "string" }, "maxItems": 2 },
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
    parsed.move_phrase = trim_phrase(&parsed.move_phrase);

    // Normalised here rather than at the database, so what the rest of the
    // pass compares and what is eventually stored are the same string.
    let tidy = |names: Vec<String>| -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for name in names {
            let key = crate::db::tags::normalise(&name);
            if !key.is_empty() && !out.contains(&key) {
                out.push(key);
            }
        }
        out
    };
    parsed.anchors = tidy(parsed.anchors);
    parsed.topics = tidy(parsed.topics);
    // Anchors only. A topic is the shelf, and the shelf's name is routinely
    // absent from the note -- which is the entire reason it can collide.
    parsed.anchors.retain(|t| grounded(t, transcript));
    parsed.topics.retain(|t| !parsed.anchors.contains(t));

    Ok(parsed)
}

/// True when the tag's words are actually in the note.
///
/// Enforced rather than asked for, because asking failed: measured over the
/// sixteen fixtures, the model coined "philosophy" and "decision-making" on the
/// first note and then reused them on all sixteen -- "philosophy" appears in
/// none of the transcripts and "decision-making" in two. A tag on every note
/// makes the candidate filter select the whole corpus, which is the same as
/// having no filter.
///
/// The rule is the one `title` already follows -- the speaker's own phrasing --
/// and the discipline §3.4 applies to quotes. Matching is on a five-character
/// stem so "index" still finds "indexes"; crude, and wrong in the safe
/// direction, since a dropped tag costs a connection that might have been
/// found and a kept one costs a connection that should not exist (§3.2).
fn grounded(tag: &str, transcript: &str) -> bool {
    let haystack = transcript.to_lowercase();
    let mut words = tag.split('-').filter(|w| w.len() > 2).peekable();
    if words.peek().is_none() {
        return false;
    }
    words.all(|word| {
        let stem: String = word.chars().take(5).collect();
        haystack.contains(&stem)
    })
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

/// A move phrase, cut on a word boundary.
///
/// Measured: `movePhrase` is an unbounded string and the model loops inside
/// it -- "for faster reads of data storage systems and databases that use
/// indexes" repeated until the 700-token ceiling, on three of the first four
/// fixtures, with the JSON unterminated. A `maxLength` in the grammar stops
/// the runaway but cuts mid-word and drags in whatever token happens to fit,
/// CJK included, so the bound is the safety net and the real cut happens
/// here -- the division of labour `trim_quote` already uses.
fn trim_phrase(phrase: &str) -> String {
    const MOST: usize = 14;
    let words: Vec<&str> = phrase.split_whitespace().collect();
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

    /// The model loops inside an unbounded string, so the bound has to leave
    /// a readable phrase behind rather than a severed word.
    #[test]
    fn a_runaway_move_phrase_is_cut_on_a_word_boundary() {
        let runaway = "updating indexes requires tradeoffs in write performance ".to_string()
            + "and storage for faster reads of data storage systems and databases "
            + "that use indexes for faster reads of data storage systems";
        let cut = trim_phrase(&runaway);
        assert!(cut.len() < runaway.len(), "a runaway phrase must be cut");
        assert!(
            runaway.starts_with(&cut),
            "the cut keeps a prefix of what the model said"
        );
        assert!(
            cut.split_whitespace().count() >= 4 && !cut.ends_with(' '),
            "cut on a word boundary, not mid-word: {cut:?}"
        );
    }

    #[test]
    fn a_short_move_phrase_is_left_alone() {
        let said = "trades one cost for another";
        assert_eq!(trim_phrase(said), said);
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

    /// The regression this schema exists to prevent. Measured over the
    /// sixteen fixtures: with the vocabulary offered as an enum the model
    /// stopped coining entirely from note two onward, filled `tags` by
    /// repeating the one permitted value to `maxItems`, and the corpus ended
    /// with a single tag on it. Reuse is `upsert`'s job, not the sampler's.
    #[test]
    fn the_vocabulary_is_never_offered_as_an_enum() {
        let schema = classify_schema(&["position".into()]);
        assert!(
            schema["properties"]["tags"]["items"]["enum"].is_null(),
            "an enum of existing tags deadlocks the vocabulary at one tag"
        );
        assert!(
            schema["properties"]["newTags"].is_null(),
            "one tag field, not a reuse/coin split -- the split had no reader"
        );
    }

    /// Unbounded strings are how the reply stops being parseable: the model
    /// loops inside `movePhrase` until the token ceiling and the JSON never
    /// closes. The bound is the guard; `trim_phrase` makes the cut readable.
    #[test]
    fn the_free_text_fields_are_bounded() {
        let schema = classify_schema(&["position".into()]);
        for field in ["title", "summary", "movePhrase"] {
            assert!(
                schema["properties"][field]["maxLength"].is_number(),
                "{field} is unbounded and the model will loop inside it"
            );
        }
        assert_eq!(schema["properties"]["anchors"]["maxItems"], 3);
        assert_eq!(schema["properties"]["topics"]["maxItems"], 2);
    }

    #[test]
    fn anchors_are_parsed_and_normalised() {
        let p = FakeProvider::replying(
            r#"{"title":"our own reasoning","role":"position","register":"neutral",
                "typeId":"position","summary":"s","movePhrase":"m",
                "anchors":["Free Will","Moral Luck","free-will"]}"#,
        );
        let c = classify(
            &p,
            "Free will and moral luck pull against each other here.",
            &["position".into()],
        )
        .unwrap();

        assert_eq!(
            c.anchors,
            vec!["free-will".to_string(), "moral-luck".to_string()],
            "normalised on the way in, and a repeat of one spelling is one tag"
        );
    }

    /// The model may answer with neither field, and older replies carry
    /// neither. Missing is not an error -- an untagged note simply connects to
    /// nothing until it is tagged.
    #[test]
    fn a_reply_with_no_tags_is_not_an_error() {
        let p = FakeProvider::replying(
            r#"{"title":"t","role":"note","register":"neutral","typeId":"note",
                "summary":"s","movePhrase":"m"}"#,
        );
        let c = classify(&p, "said", &["note".into()]).unwrap();
        assert!(c.anchors.is_empty() && c.topics.is_empty());
    }

    /// Measured, not supposed: over the sixteen fixtures the model coined
    /// "philosophy" and "decision-making" on the first note and put them on
    /// all sixteen. Neither is in the notes.
    #[test]
    fn an_ungrounded_tag_is_dropped() {
        let p = FakeProvider::replying(
            r#"{"title":"t","role":"position","register":"neutral","typeId":"position",
                "summary":"s","movePhrase":"m",
                "anchors":["philosophy","hash-tables"],"topics":["databases"]}"#,
        );
        let c = classify(
            &p,
            "Hash table lookup is O(1) on average, which is the guarantee an index leans on.",
            &["position".into()],
        )
        .unwrap();
        assert_eq!(c.anchors, vec!["hash-tables".to_string()]);
        assert_eq!(
            c.topics,
            vec!["databases".to_string()],
            "a topic is the shelf and is not required to be in the note"
        );
    }

    /// A five-character stem, so a plural still matches its singular. Crude on
    /// purpose: the alternative is a stemmer, and being wrong here costs a
    /// connection rather than a wrong one.
    #[test]
    fn grounding_tolerates_a_plural() {
        let said = "Database indexes trade write performance for faster reads.";
        assert!(grounded("database-indexes", said));
        assert!(grounded("index", said));
        assert!(!grounded("philosophy", said));
        assert!(!grounded("decision-making", said));
    }

    /// Reuse is checked against this note, not the note the tag came from.
    /// Otherwise the first note's vocabulary spreads to every later one, which
    /// is exactly what was measured.
    #[test]
    fn reuse_is_grounded_in_the_note_reusing_it() {
        let p = FakeProvider::replying(
            r#"{"title":"t","role":"note","register":"neutral","typeId":"note",
                "summary":"s","movePhrase":"m","anchors":["free-will"]}"#,
        );
        let c = classify(
            &p,
            "Buy a new charger and send the reimbursement form.",
            &["note".into()],
        )
        .unwrap();
        assert!(
            c.anchors.is_empty(),
            "an errand list is not about free will"
        );
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
