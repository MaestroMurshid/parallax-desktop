//! What the model adds to an entry, and what it is allowed to add it to.
//!
//! Enrichment generates and stores; `get_question` only reads. Nothing here is
//! called on demand from the UI.

pub mod candidates;
pub mod connect;
pub mod gate;
pub mod invoke;
pub mod propose;
pub mod run;

use crate::error::{Error, Result};
use crate::llm::{Ask, LlmProvider};
use crate::model::{Entry, Register, Role};
use serde::Deserialize;
use serde_json::{json, Value};

pub(crate) fn repair_and_parse_json<T: serde::de::DeserializeOwned>(raw: &str) -> Result<T> {
    if let Ok(parsed) = serde_json::from_str(raw) {
        return Ok(parsed);
    }

    // Attempt simple repairs by appending closing braces in case of truncation
    let fixes = ["}", "]}", "]}}", "\"}", "\"}]}", "\"}]}}"];

    for fix in fixes {
        let repaired = format!("{}{}", raw.trim_end(), fix);
        if let Ok(parsed) = serde_json::from_str(&repaired) {
            return Ok(parsed);
        }
    }

    // If it still fails, return the original parse error
    serde_json::from_str(raw)
        .map_err(|e| Error::Other(format!("JSON parsing failed even after repair: {e}")))
}

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
You are an expert librarian filing a spoken note. Answer about the note, never about the speaker.
Follow these constraints strictly. Answer the fields in the exact order listed.

### Fields to Extract

- **title**: 3-5 words taken from the speaker's own phrasing. Not a description. Lowercase unless names.
- **role**: What the note mostly does. Choose from:
  - `evidence`: Reports something observed, measured or learned.
  - `note`: Records something to do or keep (errands, lists, intents).
  - `position`: The speaker's own reasoning, asserted with grounds (argues, weighs, doubts).
- **register**: 
  - `live`: When something personal is at stake (life, relationships, work).
  - `neutral`: Otherwise (including uncertain or technical weighing).
- **typeId**: Use the value equal to `role` unless another allowed value plainly fits better.
- **summary**: One line, third person, summarizing the core claim of the note. Omit conversational context or personal details (e.g., who the speaker was talking to) unless absolutely crucial to the claim. Required if register is neutral. If live, return an empty string.
- **movePhrase**: What the note does as a move, with its subject removed (verb phrase). e.g., 'trades one cost for another'.
- **anchors**: 2-3 short, precise phrases. What this note is specifically about, in the speaker's exact words (e.g., 'hash-table-lookup').
- **topics**: 1-2 broad fields for shelving (e.g., 'databases', 'distributed-systems'). Does not have to be in the text.";

/// The one sentence the live-register setting has to change, and the reason it
/// is swapped rather than post-processed: the model is told to return an empty
/// summary for a live note, so with the facet off there was nothing for the
/// code to un-suppress. It had never been asked for a summary at all.
const LIVE_SUMMARY_RULE: &str = "Required if register is neutral. If live, return an empty string.";

/// `CLASSIFY_SYSTEM` with that rule lifted when the facet is switched off. The
/// rest is left byte-identical: this is the setting reaching the model, not an
/// edit to how anything is classified.
fn classify_system(live_register: bool) -> std::borrow::Cow<'static, str> {
    if live_register {
        return std::borrow::Cow::Borrowed(CLASSIFY_SYSTEM);
    }
    std::borrow::Cow::Owned(CLASSIFY_SYSTEM.replace(LIVE_SUMMARY_RULE, "Always required."))
}

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
    live_register: bool,
) -> Result<Classification> {
    let system = classify_system(live_register);
    let transcript = within(transcript, CLASSIFY_TRANSCRIPT_BYTES);
    let ask = Ask::new(&system, transcript).constrained(classify_schema(type_ids));
    let reply = provider.ask(ask)?;

    let mut parsed: Classification = repair_and_parse_json(&reply)?;

    // §1.1 enforced here rather than trusted: a summary of a live entry
    // flattens the exact thing that made it worth keeping. Skipped when the
    // facet is off -- what the model answered is still stored, so re-enabling
    // the setting restores the rule, but nothing acts on it meanwhile.
    if live_register && parsed.register == Register::Live {
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
    parsed.anchors = tidy(
        parsed
            .anchors
            .into_iter()
            .map(|a| trim_anchor(&a))
            .collect(),
    );
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
/// and the discipline §3.4 applies to quotes. Matching is on a Porter stem, so
/// "index" still finds "indexes" and "argued" finds "argue"; wrong in the safe
/// direction either way, since a dropped tag costs a connection that might have
/// been found and a kept one costs a connection that should not exist (§3.2).
fn grounded(tag: &str, transcript: &str) -> bool {
    use rust_stemmers::{Algorithm, Stemmer};
    let en_stemmer = Stemmer::create(Algorithm::English);

    let haystack = transcript.to_lowercase();
    let mut words = tag.split('-').filter(|w| w.len() > 2).peekable();
    if words.peek().is_none() {
        return false;
    }
    words.all(|word| {
        let stem = en_stemmer.stem(word);
        haystack.contains(&stem.to_string())
    })
}

/// The front of a transcript that fits in the reasoning model's context.
///
/// Measured in the packaged app, a note of about 4,000 words overflowed the
/// 4,096-token context and llama-server refused it outright -- and a note that
/// is never classified is retried on every open. The front is what the model
/// reads instead: a prefix of the transcript, so any quote taken from it is
/// still found at the same offsets in the whole.
///
/// Bounded in bytes rather than words because a script without spaces has no
/// words to count. Three bytes a token is the conservative end: English speech
/// measured 4.6, and a CJK character is three bytes for about one token.
pub(crate) fn within(text: &str, max_bytes: usize) -> &str {
    if text.len() <= max_bytes {
        return text;
    }
    let mut end = max_bytes;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    // Between words where there are words, so the last one is not severed.
    match text[..end].rfind(char::is_whitespace) {
        Some(space) if space > 0 => &text[..space],
        _ => &text[..end],
    }
}

/// What each prompt leaves for the transcript, at three bytes a token: the
/// 4,096-token context less the system prompt (353 tokens for classification,
/// 184 for the question, measured) and room for the reply.
pub(super) const CLASSIFY_TRANSCRIPT_BYTES: usize = 9_000;
pub(super) const QUESTION_TRANSCRIPT_BYTES: usize = 7_500;
pub(super) const QUESTION_PASSAGE_BYTES: usize = 1_500;

/// A span, enforced rather than asked for: asked for under twenty words it
/// returned twenty-four and thirty-six. Cutting on a word boundary keeps a
/// verbatim substring verbatim, so the anchor still resolves.
///
/// Where a clause ends inside the limit, the cut goes there instead of at the
/// twentieth word. Measured in the packaged app, a count alone stopped a quote
/// at "and it always disagrees at the" -- verbatim, anchored, and plainly
/// broken to anyone reading it. The trailing comma goes with it, and what is
/// left is still a prefix of what the model said.
pub(super) fn trim_quote(quote: &str) -> String {
    const MOST: usize = 20;
    // Backing off further than this trades a clipped quote for a stub.
    const LEAST: usize = 10;
    let words: Vec<&str> = quote.split_whitespace().collect();
    if words.len() <= MOST {
        return words.join(" ");
    }
    let cut = (LEAST..=MOST)
        .rev()
        .find(|&n| words[n - 1].ends_with([',', ';', ':', '.', '!', '?']))
        .unwrap_or(MOST);
    words[..cut]
        .join(" ")
        .trim_end_matches([',', ';', ':'])
        .to_string()
}

/// Three or four words, enforced rather than asked for. A 4B model cannot count
/// and returned six reliably, and a long title widens the box placement was
/// already solved against.
fn trim_title(title: &str) -> String {
    const MOST: usize = 4;
    // Words that only exist to lead into the next one. A count lands on these
    // as readily as on anything else, and a title ending in one reads as cut
    // off rather than short -- "naïve caching hides the", measured in the
    // packaged app.
    const LEADS_ON: &[&str] = &[
        "a", "an", "the", "of", "to", "in", "on", "at", "for", "with", "by", "from", "into",
        "about", "before", "after", "over", "under", "and", "or", "but", "nor", "as", "than",
        "that", "this", "is", "are", "was", "were", "be", "its", "my", "our", "your", "their",
    ];
    let words: Vec<&str> = title.split_whitespace().take(MOST).collect();
    let mut end = words.len();
    while end > 1 && LEADS_ON.contains(&words[end - 1].to_lowercase().as_str()) {
        end -= 1;
    }
    words[..end].join(" ")
}

/// An anchor is a name, not a sentence.
///
/// Measured: asked for short noun phrases the model returned
/// `trade-write-performance-and-storage-for-faster-reads` and
/// `information-isn't-organized-well`. Enforced rather than asked for again,
/// the way `trim_title` already is -- the prompt had its turn.
fn trim_anchor(anchor: &str) -> String {
    const MOST: usize = 3;
    let words: Vec<&str> = anchor.split('-').filter(|w| !w.is_empty()).collect();
    if words.len() <= MOST {
        return words.join("-");
    }
    words[..MOST].join("-")
}

/// A move phrase, cut on a word boundary.
///
/// Measured: `movePhrase` is an unbounded string and the model loops inside
/// it -- "for faster reads of data storage systems and databases that use
/// indexes" repeated on three of the first four fixtures, with the JSON
/// unterminated. That run stopped against a 700-token `max_tokens`; the cap has
/// since been removed, so the same loop now runs to the 4096-token context
/// instead, and `repair_and_parse_json` may close the truncated string rather
/// than failing. A `maxLength` in the grammar stops the runaway but cuts
/// mid-word and drags in whatever token happens to fit, CJK included, so the
/// bound is the safety net and the real cut happens here -- the division of
/// labour `trim_quote` already uses.
fn trim_phrase(phrase: &str) -> String {
    const MOST: usize = 20;
    let words: Vec<&str> = phrase.split_whitespace().collect();
    if words.len() <= MOST {
        return words.join(" ");
    }
    words[..MOST].join(" ")
}

const QUESTION_SYSTEM: &str = "\
You ask one insightful question about a note someone recorded.
Follow these constraints strictly. Answer the fields in the exact order listed.

### Fields to Extract

- **quote**: A verbatim passage from the note that the question will be about. The shortest passage that carries the claim (under 20 words). Do not modify it.
- **text**: One question, ending in a question mark.

### Question Guidelines

- Push on the reasoning, not the conclusion. \"This holds if X -- is X true?\"
- Aim at the load-bearing part, the underlying assumption, not just the topic.
- Ask about the claim, never about the note or the speaker (no \"does the note\" or \"according to the note\").
- Open with what, which, where, how, or why. Avoid yes/no questions.
- It must be answerable out loud, in a sentence or two.";

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
        Some(passage) => format!(
            "\nThe passage to ask about:\n\n{}\n",
            within(passage, QUESTION_PASSAGE_BYTES)
        ),
        None => String::new(),
    };
    let user = format!(
        "The note:\n\n{}\n{selected}\nWhat to ask: {}",
        within(&entry.transcript, QUESTION_TRANSCRIPT_BYTES),
        probe_hint
    );
    let ask = Ask::new(QUESTION_SYSTEM, &user).constrained(question_schema());
    let reply = provider.ask(ask)?;

    let mut asked: Asked = repair_and_parse_json(&reply)?;

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
        // Was "renew the domain before": the same dangling word the packaged
        // app showed, pinned here as correct until it was seen on a real row.
        assert_eq!(
            trim_title("renew the domain before the twentieth"),
            "renew the domain"
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

    /// The clauses the model actually returned, cut to a name.
    #[test]
    fn a_clause_is_cut_to_three_words() {
        assert_eq!(
            trim_anchor("trade-write-performance-and-storage-for-faster-reads"),
            "trade-write-performance"
        );
        assert_eq!(
            trim_anchor("information-isn't-organized-well"),
            "information-isn't-organized"
        );
    }

    #[test]
    fn an_anchor_already_a_name_is_left_alone() {
        for name in [
            "hash-table-lookup",
            "sunk-cost",
            "crdts",
            "write-amplification",
        ] {
            assert_eq!(trim_anchor(name), name);
        }
    }

    #[test]
    fn a_short_move_phrase_is_left_alone() {
        let said = "trades one cost for another";
        assert_eq!(trim_phrase(said), said);
    }

    /// Found in the packaged app: a note of about 4,000 words overflowed the
    /// 4,096-token context and every pass on it failed. Because it then stayed
    /// unclassified, opening it retried the same doomed pass every time.
    #[test]
    fn a_long_transcript_is_cut_to_its_front() {
        let said = "measuring before changing is the habit that saves time ".repeat(900);
        let kept = within(&said, 9_000);
        assert!(kept.len() <= 9_000, "{} bytes", kept.len());
        assert!(
            said.starts_with(kept),
            "a prefix, so quotes still anchor in the whole"
        );
        assert!(
            said[kept.len()..].starts_with(' '),
            "cut between words, not inside one"
        );
    }

    #[test]
    fn a_transcript_that_fits_is_untouched() {
        let said = "profiling beats guessing every single time";
        assert_eq!(within(said, 9_000), said);
    }

    /// No spaces to back off to, and a byte limit that lands inside a
    /// three-byte character. Slicing there would panic.
    #[test]
    fn text_without_spaces_is_cut_on_a_character_boundary() {
        let said = "缓存隐藏了真正的成本".repeat(2_000);
        let kept = within(&said, 9_001);
        assert!(kept.len() <= 9_001);
        assert!(!kept.is_empty());
        assert!(said.starts_with(kept));
    }

    /// Found in the packaged app: the model quoted a twenty-two word sentence
    /// and a count of twenty cut it after "at the", so the question sat under
    /// a quote that stopped mid-thought. A clause that ends inside the limit is
    /// a better place to stop than a word that happens to be twentieth.
    #[test]
    fn a_long_quote_stops_at_a_clause_rather_than_mid_phrase() {
        let said = "Every cache is a second source of truth that can disagree with the first,                     and it always disagrees at the worst moment.";
        let said: String = said.split_whitespace().collect::<Vec<_>>().join(" ");
        let cut = trim_quote(&said);
        assert_eq!(
            cut,
            "Every cache is a second source of truth that can disagree with the first"
        );
        assert!(said.contains(&cut), "a cut quote must still be in the note");
    }

    /// Backing off to a comma three words in trades a clipped quote for a stub.
    #[test]
    fn a_clause_break_too_early_does_not_shrink_the_quote_to_a_stub() {
        let said = "Yes, one two three four five six seven eight nine ten eleven twelve                     thirteen fourteen fifteen sixteen seventeen eighteen nineteen twenty";
        let said: String = said.split_whitespace().collect::<Vec<_>>().join(" ");
        let cut = trim_quote(&said);
        assert_eq!(cut.split_whitespace().count(), 20, "cut to a stub: {cut:?}");
    }

    #[test]
    fn a_short_quote_is_left_alone() {
        assert_eq!(
            trim_quote("the reads anybody waits on"),
            "the reads anybody waits on"
        );
    }

    /// Found in the packaged app: "naïve caching hides the real cost" was cut to
    /// "naïve caching hides the", which reads as a broken title on every row and
    /// label that shows it. The four-word cap stays; it just does not end on a
    /// word that only exists to lead into the next one.
    #[test]
    fn a_title_does_not_end_on_a_function_word() {
        assert_eq!(
            trim_title("naïve caching hides the real cost"),
            "naïve caching hides"
        );
        assert_eq!(
            trim_title("trading reads for writes"),
            "trading reads for writes"
        );
        assert_eq!(trim_title("the cost of a"), "the cost");
    }

    /// Never whittled to nothing, however the words fall.
    #[test]
    fn a_title_of_function_words_keeps_its_first() {
        assert_eq!(trim_title("the of and to"), "the");
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
            true,
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
        let c = classify(&p, "said", &["note".into()], true).unwrap();
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
            true,
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
            true,
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
            true,
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
        let c = classify(&p, "...", &["position".into()], true).unwrap();

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
        assert!(classify(&p, "...", &["note".into()], true)
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
        let c = classify(&p, "...", &types, true).unwrap();

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

    /// The half of the setting that was inert: the gate stopped consulting the
    /// register but the model was still being told to leave a live note without
    /// a summary, so the note the setting exists for still got none.
    #[test]
    fn turning_the_register_off_stops_asking_for_an_empty_summary() {
        let on = classify_system(true);
        let off = classify_system(false);

        assert!(
            on.contains(LIVE_SUMMARY_RULE),
            "the rule went missing with the facet on"
        );
        assert!(
            !off.contains(LIVE_SUMMARY_RULE),
            "the model is still told to omit it"
        );
        assert!(
            off.contains("Always required."),
            "nothing asks for the summary now"
        );

        // Only that sentence moves. The rest of the prompt is not the setting's
        // to rewrite, and a drifting prompt would change classification itself.
        assert_eq!(
            on.replace(LIVE_SUMMARY_RULE, ""),
            off.replace("Always required.", ""),
            "the setting changed more of the prompt than the summary rule"
        );
    }

    #[test]
    fn unreadable_output_is_an_error_not_a_panic() {
        let p = FakeProvider::replying("not json at all");
        assert!(classify(&p, "...", &["position".into()], true).is_err());
        assert!(ask_about(&p, &entry("x"), "hint").is_err());
    }
}
