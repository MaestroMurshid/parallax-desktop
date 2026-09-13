//! What runs after a capture lands: classify, then at most one question.
//!
//! Order matters. Creation sets register to live, which suppresses the automatic
//! question, so the entry is re-read after classification and gated on what the
//! model decided rather than on the placeholder.

use super::gate;
use crate::db;
use crate::error::{Error, Result};
use crate::llm::LlmProvider;
use crate::model::{Entry, Span};
use rusqlite::Connection;

#[derive(Debug, Default)]
pub struct Enriched {
    pub classified: bool,
    pub question_id: Option<String>,
}

/// Locates a model's quote in the transcript.
///
/// `None` when the quote is not verbatim -- a claim that cannot be checked is
/// not allowed to anchor one (§3.4) -- or when it lands on words the speaker did
/// not say, which may be quoted but not pushed on (§7.3).
pub fn anchor(entry: &Entry, quote: &str) -> Option<Span> {
    let quote = quote.trim();
    if quote.is_empty() {
        return None;
    }
    let at = entry.transcript.find(quote)?;
    let span = Span {
        start: crate::text::byte_to_utf16(&entry.transcript, at),
        end: crate::text::byte_to_utf16(&entry.transcript, at + quote.len()),
        attributed: false,
    };

    let borrowed = entry
        .spans
        .iter()
        .any(|s| s.attributed && s.start < span.end && span.start < s.end);
    (!borrowed).then_some(span)
}

/// Anchors as themselves, topics folded onto the shelf that already exists.
///
/// Kept out of `run` because it must not be able to fail the pass: an untagged
/// note is connected to nothing, which is recoverable, while a failed pass
/// would lose the filing too.
fn file_under(conn: &Connection, entry_id: &str, c: &super::Classification) -> Result<()> {
    let shelves: Vec<String> = db::tags::all(conn)?
        .into_iter()
        .filter(|t| t.kind == db::tags::Kind::Topic)
        .map(|t| t.name)
        .collect();
    // Folded against this note's own accepted topics as well as the corpus's,
    // or two names for one shelf on a single note both land: measured, the
    // model returned `databases` and `database-performance` together.
    let mut shelves = shelves;
    let mut topics: Vec<String> = Vec::new();
    for wanted in &c.topics {
        let onto = db::tags::fold_topic(&shelves, wanted);
        if !topics.contains(&onto) {
            shelves.push(onto.clone());
            topics.push(onto);
        }
    }

    let mut ids = db::tags::upsert(conn, &c.anchors, db::tags::Kind::Anchor)?;
    ids.extend(db::tags::upsert(conn, &topics, db::tags::Kind::Topic)?);
    db::tags::set_for_entry(conn, entry_id, &ids)
}

/// Classify the entry, then ask one question if the gates allow it.
///
/// The entry is already saved before this runs, so every failure here costs
/// enrichment and nothing else.
pub fn run(conn: &Connection, provider: &dyn LlmProvider, entry_id: &str) -> Result<Enriched> {
    let entry = db::entries::get(conn, entry_id)?
        .ok_or_else(|| Error::NotFound(format!("no entry {entry_id}")))?;

    // Nothing to read, so nothing to decide. Asked anyway, the model files a
    // blank note with a title and a summary it invented -- measured in the
    // packaged app -- and a spoken note recorded before the transcription model
    // lands is blank in exactly this way. Left unclassified, it keeps the
    // derived title and waits for words.
    if entry.transcript.trim().is_empty() {
        return Ok(Enriched {
            classified: false,
            question_id: None,
        });
    }

    // Read once, so the classification and the gate below cannot disagree
    // about it halfway through a pass.
    let live_register = db::settings::get(conn)?.live_register;

    let type_ids = db::entries::type_ids(conn)?;
    // The vocabulary is deliberately not sent. Offered as an enum it stopped
    // the model coining at all and froze the corpus at one tag; reuse happens
    // below, where `upsert` folds a repeated name into the existing row.
    let classification = super::classify(provider, &entry.transcript, &type_ids, live_register)?;
    db::entries::set_classification(
        conn,
        entry_id,
        &classification.title,
        classification.role,
        classification.register,
        &classification.type_id,
        classification.summary.as_deref(),
        Some(classification.move_phrase.as_str()),
    )?;

    // Tagging is not allowed to cost the classification that already landed:
    // an untagged note is connected to nothing, which is recoverable, while a
    // failed pass would lose the filing too.
    if let Err(e) = file_under(conn, entry_id, &classification) {
        eprintln!("tagging failed for {entry_id}: {e}");
    }

    // Re-read: the gates below read role and register, which only just changed.
    let entry = db::entries::get(conn, entry_id)?
        .ok_or_else(|| Error::NotFound(format!("no entry {entry_id}")))?;

    let Some(probe) = gate::automatic_probes(&entry, live_register)
        .first()
        .copied()
    else {
        return Ok(Enriched {
            classified: true,
            question_id: None,
        });
    };

    // Shared with the invoked path: both owe a question that quotes the note
    // verbatim, and there is one place that decides whether it does.
    let question = super::invoke::compose(provider, &entry, &[probe], None)?;
    db::questions::insert(conn, &question, &entry.transcript)?;

    Ok(Enriched {
        classified: true,
        question_id: Some(question.id),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::fake::ScriptedProvider;
    use crate::model::Register;

    const CLASSIFY: &str = r#"{"title":"indexes trade writes","role":"position",
        "register":"neutral","typeId":"position","summary":"A claim about indexes.",
        "movePhrase":"trades one cost for another"}"#;

    fn corpus(transcript: &str, duration_ms: i64) -> (Connection, String) {
        let conn = db::open_in_memory().unwrap();
        let made = db::create::create(
            &conn,
            db::create::NewEntry {
                transcript: transcript.into(),
                duration_ms,
                fingerprint: vec![0.1, 0.2],
                parent_entry_id: None,
                local_only: None,
                typed: false,
            },
        )
        .unwrap();
        (conn, made.id)
    }

    const SAID: &str = "Indexes trade write performance for faster reads, \
        and that tradeoff is usually worth it for a read-heavy table.";

    /// Span offsets are UTF-16 units, so an assertion about them has to slice
    /// the way the frontend does. Slicing `SAID` as bytes happens to agree
    /// because the fixture is ASCII, which is exactly why it misleads -- the
    /// unit regression it looks like it would catch is caught by
    /// `an_anchor_is_measured_in_utf16_units` instead.
    fn js_slice(s: &str, start: u32, end: u32) -> String {
        let units: Vec<u16> = s.encode_utf16().collect();
        let lo = (start as usize).min(units.len());
        let hi = (end as usize).min(units.len()).max(lo);
        String::from_utf16_lossy(&units[lo..hi])
    }

    fn question(quote: &str) -> String {
        format!(
            r#"{{"text":"Where does that stop holding?","quote":{}}}"#,
            serde_json::to_string(quote).unwrap()
        )
    }

    /// Found in the packaged app: a note of nothing but whitespace came back
    /// titled "records something to do" and summarised as "the speaker records
    /// a note about a specific topic" -- a filing invented from no words at
    /// all. A spoken note recorded before the transcription model lands is
    /// empty in exactly this way, so it is not only a typed-note edge.
    #[test]
    fn an_empty_note_is_not_sent_to_the_model() {
        let (conn, id) = corpus("   \n\t  ", 45_000);
        let provider = ScriptedProvider::with(&[CLASSIFY, &question("faster reads")]);

        let out = run(&conn, &provider, &id).unwrap();

        assert!(!out.classified, "a blank note was classified");
        assert!(
            provider.asked.lock().unwrap().is_empty(),
            "the model was asked about a note with no words in it"
        );
        let entry = db::entries::get(&conn, &id).unwrap().unwrap();
        assert_eq!(entry.summary, None, "a summary was invented for nothing");
        assert!(db::questions::list_for(&conn, &id).unwrap().is_empty());
    }

    #[test]
    fn classification_is_persisted() {
        let (conn, id) = corpus(SAID, 45_000);
        let provider = ScriptedProvider::with(&[CLASSIFY, &question("faster reads")]);

        let out = run(&conn, &provider, &id).unwrap();
        assert!(out.classified);

        let entry = db::entries::get(&conn, &id).unwrap().unwrap();
        assert_eq!(entry.title, "indexes trade writes");
        assert_eq!(entry.register, Register::Neutral);
        assert_eq!(entry.summary.as_deref(), Some("A claim about indexes."));
    }

    #[test]
    fn a_question_is_asked_and_anchored() {
        let (conn, id) = corpus(SAID, 45_000);
        let provider = ScriptedProvider::with(&[CLASSIFY, &question("faster reads")]);

        let out = run(&conn, &provider, &id).unwrap();
        assert!(out.question_id.is_some());

        let questions = db::questions::list_for(&conn, &id).unwrap();
        assert_eq!(questions.len(), 1);
        let span = questions[0].span.as_ref().unwrap();
        assert_eq!(js_slice(SAID, span.start, span.end), "faster reads");
    }

    /// The question a capture opens with is the model's choice of move, from
    /// everything a position may be asked -- not the first tactic every time.
    #[test]
    fn the_opening_question_lets_the_model_choose_the_move() {
        let (conn, id) = corpus(SAID, 45_000);
        let provider = ScriptedProvider::with(&[CLASSIFY, &question("faster reads")]);

        run(&conn, &provider, &id).unwrap();
        let prompt = provider.asked.lock().unwrap()[1].clone();
        for tactic in [super::gate::Probe::Boundary, super::gate::Probe::Fallacy, super::gate::Probe::Assumption] {
            assert!(prompt.contains(&format!("- {}: ", tactic.id())), "{tactic:?}: {prompt}");
        }
    }

    /// The shape of the JSON proves nothing about where the words came from.
    #[test]
    fn a_quote_that_is_not_in_the_note_is_refused() {
        let (conn, id) = corpus(SAID, 45_000);
        let provider =
            ScriptedProvider::with(&[CLASSIFY, &question("something the speaker never said")]);

        let failed = run(&conn, &provider, &id);
        assert!(failed.is_err(), "an unanchored question must not land");
        assert!(db::questions::list_for(&conn, &id).unwrap().is_empty());

        // Classification still stuck: it succeeded before the question failed.
        let entry = db::entries::get(&conn, &id).unwrap().unwrap();
        assert_eq!(entry.title, "indexes trade writes");
    }

    /// The capture path has to actually persist what the classifier returned,
    /// or the vocabulary never grows and nothing is ever a candidate.
    #[test]
    fn a_capture_lands_its_tags() {
        let (conn, id) = corpus(SAID, 5_000);
        let tagged = CLASSIFY.replace(
            r#""movePhrase":"trades one cost for another""#,
            r#""movePhrase":"trades one cost for another","anchors":["Indexes","Write Performance"],"topics":["Databases"]"#,
        );
        let provider = ScriptedProvider::with(&[&tagged]);

        run(&conn, &provider, &id).unwrap();

        let names: Vec<String> = db::tags::for_entry(&conn, &id)
            .unwrap()
            .into_iter()
            .map(|t| t.name)
            .collect();
        assert_eq!(
            names,
            vec![
                "indexes".to_string(),
                "write-performance".into(),
                "databases".into()
            ],
            "anchors first, then the shelf it is filed under"
        );
    }

    /// The reuse half, end to end, and the fold with it. The second note is
    /// never shown the first note's vocabulary -- offering it as an enum froze
    /// the corpus at one tag -- so it arrives at "Database Systems" on its own.
    /// `fold_topic` puts that on the shelf that already exists rather than
    /// beside it, which is the only reason the two become candidates.
    #[test]
    fn a_second_note_joins_the_vocabulary_rather_than_doubling_it() {
        let (conn, first) = corpus(SAID, 5_000);
        let coined = CLASSIFY.replace(
            r#""movePhrase":"trades one cost for another""#,
            r#""movePhrase":"trades one cost for another","topics":["databases"]"#,
        );
        run(&conn, &ScriptedProvider::with(&[&coined]), &first).unwrap();

        let second = db::create::create(
            &conn,
            db::create::NewEntry {
                transcript: "More about indexes, months later.".into(),
                duration_ms: 5_000,
                fingerprint: vec![],
                parent_entry_id: None,
                local_only: None,
                typed: true,
            },
        )
        .unwrap()
        .id;
        let reused = CLASSIFY.replace(
            r#""movePhrase":"trades one cost for another""#,
            r#""movePhrase":"trades one cost for another","topics":["Database Systems"]"#,
        );
        run(&conn, &ScriptedProvider::with(&[&reused]), &second).unwrap();

        let shelves: Vec<String> = db::tags::all(&conn)
            .unwrap()
            .into_iter()
            .filter(|t| t.kind == db::tags::Kind::Topic)
            .map(|t| t.name)
            .collect();
        assert_eq!(
            shelves,
            vec!["databases".to_string()],
            "database-systems was shelved beside databases instead of on it"
        );
        assert_eq!(
            db::tags::sharing(&conn, &second).unwrap(),
            vec![(first, 1)],
            "the two notes are not candidates for each other"
        );
    }

    /// Found by running the eval, not by reading the code: the model returned
    /// `databases` and `database-performance` for one note, and folding only
    /// against the corpus let both land as separate shelves.
    #[test]
    fn two_names_for_one_shelf_on_one_note_fold_together() {
        let (conn, id) = corpus(SAID, 5_000);
        let reply = CLASSIFY.replace(
            r#""movePhrase":"trades one cost for another""#,
            r#""movePhrase":"trades one cost for another","topics":["Databases","Database Performance"]"#,
        );
        run(&conn, &ScriptedProvider::with(&[&reply]), &id).unwrap();

        let shelves: Vec<String> = db::tags::for_entry(&conn, &id)
            .unwrap()
            .into_iter()
            .filter(|t| t.kind == db::tags::Kind::Topic)
            .map(|t| t.name)
            .collect();
        assert_eq!(shelves, vec!["databases".to_string()]);
    }

    /// A short note is not pushed on, and classification still runs.
    #[test]
    fn a_short_note_is_classified_but_not_questioned() {
        let (conn, id) = corpus(SAID, 5_000);
        let provider = ScriptedProvider::with(&[CLASSIFY]);

        let out = run(&conn, &provider, &id).unwrap();
        assert!(out.classified);
        assert!(out.question_id.is_none());
        assert_eq!(provider.calls(), 1, "the model was asked twice");
    }

    /// Live register suppresses the question, which is what classification is
    /// gating: the placeholder would have suppressed it anyway.
    #[test]
    fn a_live_note_is_not_questioned() {
        let (conn, id) = corpus(SAID, 45_000);
        let live = CLASSIFY.replace(r#""register":"neutral""#, r#""register":"live""#);
        let provider = ScriptedProvider::with(&[&live]);

        let out = run(&conn, &provider, &id).unwrap();
        assert!(out.question_id.is_none());
        assert_eq!(provider.calls(), 1);
    }

    #[test]
    fn anchoring_refuses_someone_elses_words() {
        let mut entry = Entry {
            spans: vec![Span {
                start: 0,
                end: 7,
                attributed: true,
            }],
            ..blank()
        };
        entry.transcript = "Indexes trade write performance for faster reads.".into();

        assert!(
            anchor(&entry, "Indexes").is_none(),
            "attributed words may be quoted, not pushed on"
        );
        assert!(anchor(&entry, "faster reads").is_some());
    }

    /// The frontend slices by UTF-16 code unit, so that is what an anchor
    /// returns. `find` gives bytes, and on a transcript with an em dash in it
    /// the highlight lands two characters left of the quote.
    #[test]
    fn an_anchor_is_measured_in_utf16_units() {
        let entry = Entry {
            transcript: "Observability — not logging — is the claim here.".into(),
            ..blank()
        };
        let span = anchor(&entry, "the claim").expect("a verbatim quote anchors");

        let units: Vec<u16> = entry.transcript.encode_utf16().collect();
        let sliced = String::from_utf16_lossy(&units[span.start as usize..span.end as usize]);
        assert_eq!(sliced, "the claim");
    }

    /// The overlap check compares the new anchor against stored spans, so both
    /// have to be in the same units before it means anything.
    #[test]
    fn the_attribution_check_compares_like_with_like() {
        let quoted = "«Наблюдаемость важнее логов»";
        let transcript = format!("{quoted} — and I think that is wrong.");
        let entry = Entry {
            spans: vec![Span {
                start: 0,
                end: quoted.encode_utf16().count() as u32,
                attributed: true,
            }],
            transcript,
            ..blank()
        };

        assert!(
            anchor(&entry, "важнее логов").is_none(),
            "the quote sits inside someone else's words"
        );
        assert!(anchor(&entry, "that is wrong").is_some());
    }

    #[test]
    fn anchoring_refuses_an_empty_quote() {
        let entry = Entry {
            transcript: SAID.into(),
            ..blank()
        };
        assert!(anchor(&entry, "   ").is_none());
    }

    fn blank() -> Entry {
        Entry {
            id: "e1".into(),
            audio_path: None,
            transcript: String::new(),
            created_at: "2024-01-01T00:00:00Z".into(),
            x: 0.0,
            y: 0.0,
            parent_entry_id: None,
            answers_question_id: None,
            role: crate::model::Role::Position,
            register: Register::Neutral,
            type_id: "position".into(),
            resolved: false,
            resolution_text: None,
            title: "t".into(),
            summary: None,
            duration_ms: 45_000,
            fingerprint: Vec::new(),
            unfinished: false,
            local_only: false,
            spans: Vec::new(),
            action_items: Vec::new(),
            is_sample: None,
        }
    }
}
