//! What runs after a capture lands: classify, then at most one question.
//!
//! Order matters. Creation sets register to live, which suppresses the automatic
//! question, so the entry is re-read after classification and gated on what the
//! model decided rather than on the placeholder.

use super::gate;
use crate::db;
use crate::error::{Error, Result};
use crate::llm::LlmProvider;
use crate::model::{Entry, Question, Span};
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

/// Classify the entry, then ask one question if the gates allow it.
///
/// The entry is already saved before this runs, so every failure here costs
/// enrichment and nothing else.
pub fn run(conn: &Connection, provider: &dyn LlmProvider, entry_id: &str) -> Result<Enriched> {
    let entry = db::entries::get(conn, entry_id)?
        .ok_or_else(|| Error::NotFound(format!("no entry {entry_id}")))?;

    let type_ids = db::entries::type_ids(conn)?;
    let classification = super::classify(provider, &entry.transcript, &type_ids)?;
    db::entries::set_classification(
        conn,
        entry_id,
        &classification.title,
        classification.role,
        classification.register,
        &classification.type_id,
        classification.summary.as_deref(),
    )?;

    // Re-read: the gates below read role and register, which only just changed.
    let entry = db::entries::get(conn, entry_id)?
        .ok_or_else(|| Error::NotFound(format!("no entry {entry_id}")))?;

    let Some(probe) = gate::automatic_probes(&entry).first().copied() else {
        return Ok(Enriched {
            classified: true,
            question_id: None,
        });
    };

    let asked = super::ask_about(provider, &entry, probe.hint())?;
    let Some(span) = anchor(&entry, &asked.quote) else {
        return Err(Error::Other(format!(
            "the question quoted something not in the note: {:?}",
            asked.quote
        )));
    };

    let question = Question {
        id: uuid::Uuid::new_v4().to_string(),
        entry_id: entry.id.clone(),
        text: asked.text,
        span: Some(span),
        answered: false,
        dismissed: false,
        provider_name: provider.name(),
        created_at: chrono::Utc::now().to_rfc3339(),
    };
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

    fn question(quote: &str) -> String {
        format!(
            r#"{{"text":"Where does that stop holding?","quote":{}}}"#,
            serde_json::to_string(quote).unwrap()
        )
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
        assert_eq!(
            &SAID[span.start as usize..span.end as usize],
            "faster reads"
        );
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
