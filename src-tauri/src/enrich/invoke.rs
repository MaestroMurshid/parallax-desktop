//! A question someone asked for, rather than one the app opened with.
//!
//! §3.2 hands the invoked path to the user, so register and duration do not
//! gate here. Role, provenance and the anchor still do, and all three are
//! decided in this file: the frontend computes the same rules to decide whether
//! to draw a button, and a button is not permission. `run_probe` is on the IPC
//! surface like everything else.

use super::gate::{self, Probe};
use crate::db;
use crate::error::{Error, Result};
use crate::llm::LlmProvider;
use crate::model::{Entry, Question, Span};
use rusqlite::Connection;

/// One question about `entry_id`, because someone asked for one.
///
/// `probe` is `None` on the path the UI actually takes -- §3.6 offers one door,
/// not a menu of techniques -- and naming one is kept for replay and evaluation.
pub fn ask(
    conn: &Connection,
    provider: &dyn LlmProvider,
    entry_id: &str,
    probe: Option<Probe>,
    selection: Option<Span>,
) -> Result<Question> {
    let entry = db::entries::get(conn, entry_id)?
        .ok_or_else(|| Error::NotFound(format!("no entry {entry_id}")))?;

    let allowed = gate::invoked_probes(&entry);
    let probe = match probe {
        Some(named) if !allowed.contains(&named) => {
            return Err(Error::Other(format!(
                "{} may not be asked of {entry_id}",
                named.id()
            )))
        }
        Some(named) => named,
        // Rotated by what the entry already carries rather than always taking
        // the first: §3.4 bans regeneration, so asking a second time has to be
        // a different move and not another run at the same one.
        None => {
            let asked_before = db::questions::list_for(conn, entry_id)?.len();
            *allowed
                .get(asked_before % allowed.len().max(1))
                .ok_or_else(|| Error::Other(format!("nothing may be asked of {entry_id}")))?
        }
    };

    let question = compose(provider, &entry, &[probe], selection.as_ref())?;
    db::questions::insert(conn, &question, &entry.transcript)?;
    Ok(question)
}

/// Asks the model and anchors what came back.
///
/// Shared with the automatic pass, which owes the same two things: a question
/// that quotes the note, and a quote that is the speaker's own.
pub fn compose(
    provider: &dyn LlmProvider,
    entry: &Entry,
    tactics: &[Probe],
    selection: Option<&Span>,
) -> Result<Question> {
    let passage = match selection {
        // §7.3 at span level. Pointing at a sentence does not make it yours,
        // and the entry-level gate lets a mixed note through -- most notes
        // about a book contain a real position of the reader's own.
        Some(span) if borrowed(entry, span) => {
            return Err(Error::Other(
                "that passage is someone else's words".to_string(),
            ))
        }
        // A zero-width selection is not a passage. Asked about the note as a
        // whole is the honest fallback; an empty quotation in the prompt is not.
        Some(span) => {
            Some(db::entries::quoted(&entry.transcript, span)).filter(|t| !t.trim().is_empty())
        }
        None => None,
    };

    let asked = super::ask_about_passage(provider, entry, tactics, passage.as_deref())?;
    let Some(span) = super::run::anchor(entry, &asked.quote) else {
        return Err(Error::Other(format!(
            "the question quoted something not in the note: {:?}",
            asked.quote
        )));
    };

    Ok(Question {
        id: uuid::Uuid::new_v4().to_string(),
        entry_id: entry.id.clone(),
        text: asked.text,
        span: Some(span),
        answered: false,
        dismissed: false,
        // Which move produced it, carried where the panel already looks.
        // `Question` has no probe field and the fixture backend has always put
        // it here, so this keeps one wire shape rather than adding a column.
        provider_name: format!("{} · {}", provider.name(), asked.tactic.id()),
        created_at: chrono::Utc::now().to_rfc3339(),
    })
}

/// Whether a selection sits in someone else's words. `anchor` cannot answer
/// this: it locates text by its first occurrence, and the passage the user
/// picked may not be that one.
fn borrowed(entry: &Entry, span: &Span) -> bool {
    entry
        .spans
        .iter()
        .any(|s| s.attributed && s.start < span.end && span.start < s.end)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::fake::ScriptedProvider;
    use crate::model::{Register, Role};

    const SAID: &str = "Indexes trade write performance for faster reads, \
        and that tradeoff is usually worth it for a read-heavy table.";

    /// A position with words of its own: everything the invoked gate allows.
    /// Register is deliberately left where creation put it -- §3.2 does not gate
    /// this path on it, and a test that classified first would hide that.
    fn corpus() -> (Connection, String) {
        let conn = db::open_in_memory().unwrap();
        let made = db::create::create(
            &conn,
            db::create::NewEntry {
                transcript: SAID.into(),
                duration_ms: 45_000,
                fingerprint: vec![0.1],
                parent_entry_id: None,
                local_only: None,
                typed: false,
            },
        )
        .unwrap();
        (conn, made.id)
    }

    fn as_role(conn: &Connection, id: &str, role: Role) {
        db::entries::set_classification(
            conn,
            id,
            "indexes trade writes",
            role,
            Register::Neutral,
            "position",
            None,
            None,
        )
        .unwrap();
    }

    /// Marks a stretch of the transcript as someone else's words.
    fn attribute(conn: &Connection, id: &str, start: u32, end: u32) {
        conn.execute(
            "INSERT INTO spans (entry_id, start_offset, end_offset, attributed, quoted_text)
             VALUES (?1, ?2, ?3, 1, '')",
            rusqlite::params![id, start, end],
        )
        .unwrap();
    }

    fn reply(quote: &str) -> String {
        format!(
            r#"{{"text":"Where does that stop holding?","quote":{}}}"#,
            serde_json::to_string(quote).unwrap()
        )
    }

    fn reply_as(tactic: &str, quote: &str) -> String {
        format!(
            r#"{{"tactic":"{tactic}","quote":{},"text":"What would change that?"}}"#,
            serde_json::to_string(quote).unwrap()
        )
    }

    /// The offer as the model sees it: one line per move.
    fn offers(prompt: &str, tactic: Probe) -> bool {
        prompt.contains(&format!("- {}: ", tactic.id()))
    }

    fn span_over(quote: &str) -> Span {
        let at = SAID.find(quote).expect("the fixture contains it");
        Span {
            start: crate::text::byte_to_utf16(SAID, at),
            end: crate::text::byte_to_utf16(SAID, at + quote.len()),
            attributed: false,
        }
    }

    #[test]
    fn an_invoked_question_lands_anchored() {
        let (conn, id) = corpus();
        let provider = ScriptedProvider::with(&[&reply("faster reads")]);

        let question = ask(&conn, &provider, &id, None, None).unwrap();

        let stored = db::questions::list_for(&conn, &id).unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].id, question.id);
        let span = question.span.as_ref().expect("§3.4 -- every claim quotes");
        assert_eq!(
            &SAID[span.start as usize..span.end as usize],
            "faster reads"
        );
    }

    /// §3.6 rule 2: a note offers nothing to push on, and asking anyway is the
    /// intrusion the tiers exist to prevent. The UI does not draw the button;
    /// that is not what stops it.
    #[test]
    fn a_note_reaches_nothing_even_when_invited() {
        let (conn, id) = corpus();
        as_role(&conn, &id, Role::Note);
        let provider = ScriptedProvider::with(&[&reply("faster reads")]);

        assert!(ask(&conn, &provider, &id, None, None).is_err());
        assert!(db::questions::list_for(&conn, &id).unwrap().is_empty());
        assert_eq!(provider.calls(), 0, "the model was asked anyway");
    }

    /// The heavy probes are reachable only by name, and only here.
    #[test]
    fn a_heavy_probe_can_be_named() {
        let (conn, id) = corpus();
        let provider = ScriptedProvider::with(&[&reply("faster reads")]);

        ask(&conn, &provider, &id, Some(Probe::Steelman), None).unwrap();
        let prompt = provider.asked.lock().unwrap().last().unwrap().clone();
        assert!(prompt.contains(Probe::Steelman.hint()), "{prompt}");
        assert!(!offers(&prompt, Probe::Boundary), "a named move offered others: {prompt}");
    }

    /// The gate decides, not the caller. Evidence may be asked to explain
    /// itself and may not be steelmanned, and `run_probe` is invokable from
    /// anywhere the webview can reach.
    #[test]
    fn a_probe_the_gate_forbids_is_refused_in_rust() {
        let (conn, id) = corpus();
        as_role(&conn, &id, Role::Evidence);
        let provider = ScriptedProvider::with(&[&reply("faster reads")]);

        assert!(ask(&conn, &provider, &id, Some(Probe::Steelman), None).is_err());
        assert!(ask(&conn, &provider, &id, Some(Probe::Feynman), None).is_ok());
    }

    /// "Ask another" has to be another move, not another run at the same one,
    /// so what this note has already been asked is not offered again.
    #[test]
    fn asking_again_does_not_offer_a_move_already_used() {
        let (conn, id) = corpus();
        let provider = ScriptedProvider::with(&[
            &reply_as("boundary", "faster reads"),
            &reply_as("definition", "faster reads"),
        ]);

        let first = ask(&conn, &provider, &id, None, None).unwrap();
        ask(&conn, &provider, &id, None, None).unwrap();

        assert!(first.provider_name.ends_with("· boundary"), "{}", first.provider_name);
        let prompts = provider.asked.lock().unwrap().clone();
        assert!(offers(&prompts[0], Probe::Boundary), "{}", prompts[0]);
        assert!(offers(&prompts[0], Probe::Fallacy), "{}", prompts[0]);
        assert!(!offers(&prompts[1], Probe::Boundary), "{}", prompts[1]);
        assert!(offers(&prompts[1], Probe::Definition), "{}", prompts[1]);
    }

    /// Once every move has been made on a note, asking again is still allowed
    /// -- it offers them all rather than nothing.
    #[test]
    fn once_every_move_is_used_they_are_all_offered_again() {
        let (conn, id) = corpus();
        let mut replies: Vec<String> = Probe::ALL
            .iter()
            .map(|t| reply_as(t.id(), "faster reads"))
            .collect();
        replies.push(reply_as("boundary", "faster reads"));
        let provider =
            ScriptedProvider::with(&replies.iter().map(String::as_str).collect::<Vec<_>>());

        for _ in 0..=Probe::ALL.len() {
            ask(&conn, &provider, &id, None, None).unwrap();
        }
        let last = provider.asked.lock().unwrap().last().unwrap().clone();
        for tactic in Probe::ALL {
            assert!(offers(&last, tactic), "{tactic:?} missing: {last}");
        }
    }

    /// §3.4 -- a quote the note does not contain cannot be checked, so the
    /// question does not exist. The shape of the JSON proves nothing.
    /// Which move produced a question is the only way to tell a boundary probe
    /// from a steelman after the fact, and `Question` has no field for it --
    /// the fixture backend puts it in `providerName`, and the panel renders
    /// that. Rust dropped it, so natively every question read "llama-server"
    /// and §3.2's tiers were invisible in the one place they are observable.
    #[test]
    fn a_question_says_which_probe_produced_it() {
        let (conn, id) = corpus();
        let provider = ScriptedProvider::with(&[&reply("faster reads")]);

        let question = ask(&conn, &provider, &id, Some(Probe::Steelman), None).unwrap();
        assert_eq!(
            question.provider_name,
            format!("{} · steelman", provider.name())
        );
    }

    #[test]
    fn an_unanchored_question_does_not_land() {
        let (conn, id) = corpus();
        let provider = ScriptedProvider::with(&[&reply("something nobody said")]);

        assert!(ask(&conn, &provider, &id, None, None).is_err());
        assert!(db::questions::list_for(&conn, &id).unwrap().is_empty());
    }

    /// The selection is the whole point of the invoked path: the user already
    /// said what the question is about.
    #[test]
    fn a_selected_passage_reaches_the_model() {
        let (conn, id) = corpus();
        let provider = ScriptedProvider::with(&[&reply("faster reads")]);

        ask(
            &conn,
            &provider,
            &id,
            None,
            Some(span_over("that tradeoff is usually worth it")),
        )
        .unwrap();

        // Twice: once inside the transcript, which goes with it because a
        // sentence pulled out of the note is not enough to ask a question that
        // lands, and once as the passage. Counted rather than matched, so the
        // assertion does not pass on the transcript alone.
        let prompt = provider.asked.lock().unwrap().last().unwrap().clone();
        assert_eq!(
            prompt.matches("that tradeoff is usually worth it").count(),
            2,
            "{prompt}"
        );
    }

    /// §7.3 at span level. Pointing at a sentence does not make it yours, and
    /// a mixed note passes the entry-level gate -- most notes about a book
    /// contain a real position -- so the selection has to be checked too.
    #[test]
    fn a_selection_in_someone_elses_words_is_refused() {
        let (conn, id) = corpus();
        let borrowed = span_over("Indexes trade write performance");
        attribute(&conn, &id, borrowed.start, borrowed.end);
        let provider = ScriptedProvider::with(&[&reply("faster reads")]);

        assert!(ask(&conn, &provider, &id, None, Some(borrowed)).is_err());
        assert_eq!(provider.calls(), 0);
        // The note still has words of its own, so the rest of it is fair game.
        assert!(ask(&conn, &provider, &id, None, Some(span_over("faster reads"))).is_ok());
    }

    /// Provenance gates both paths, unlike register and duration.
    #[test]
    fn a_wholly_borrowed_entry_is_refused() {
        let (conn, id) = corpus();
        attribute(&conn, &id, 0, SAID.encode_utf16().count() as u32);
        let provider = ScriptedProvider::with(&[&reply("faster reads")]);

        assert!(ask(&conn, &provider, &id, Some(Probe::Boundary), None).is_err());
        assert_eq!(provider.calls(), 0);
    }

    #[test]
    fn an_entry_that_is_not_there_is_not_found() {
        let (conn, _) = corpus();
        let provider = ScriptedProvider::with(&[&reply("faster reads")]);
        assert!(ask(&conn, &provider, "nobody", None, None).is_err());
    }
}
