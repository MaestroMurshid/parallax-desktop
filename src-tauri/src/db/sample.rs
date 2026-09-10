//! The seeded corpus.
//!
//! Every mechanic that distinguishes this product needs time depth to be
//! visible at all -- cross-time connection, contradiction, returning to
//! something. Opening an empty app shows none of it, so the sample is a
//! deliverable rather than a shortcut.
//!
//! Entries are marked `is_sample` so they can never be mistaken for the
//! user's own, and loading is idempotent: doing it twice used to produce two
//! copies of every edge.

use crate::db;
use crate::error::Result;
use crate::model::{Edge, EdgeStatus, Entry, Question, Register, Relation, Role, Span};
use rusqlite::Connection;
use serde::Deserialize;

const SEED: &str = include_str!("../../../fixtures/corpus.json");

#[derive(Debug, Deserialize)]
struct Seed {
    entries: Vec<SeedEntry>,
    edges: Vec<SeedEdge>,
    questions: Vec<SeedQuestion>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SeedEntry {
    id: String,
    created_at: String,
    transcript: String,
    title: String,
    summary: Option<String>,
    role: Role,
    register: Register,
    type_id: String,
    duration_ms: i64,
    #[serde(default)]
    audio: bool,
    #[serde(default)]
    resolved: bool,
    resolution_text: Option<String>,
    #[serde(default)]
    local_only: bool,
    /// Substrings of the transcript, so an anchor is found rather than stated.
    /// Defaulted because they are empty in most records and are the first
    /// thing an author omits when adding an entry by hand -- and a missing
    /// array would otherwise fail the whole load, not just that entry.
    #[serde(default)]
    attributed_quotes: Vec<String>,
    #[serde(default)]
    action_items: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct SeedEdge {
    a: String,
    b: String,
    relation: Relation,
    status: EdgeStatus,
    question: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SeedQuestion {
    entry_id: String,
    text: String,
    span_quote: Option<String>,
    #[serde(default)]
    answered: bool,
    provider_name: String,
}

/// Finds a quote in the transcript rather than trusting an offset, which is
/// the same rule corrections follow. A quote that is not there is dropped.
fn span_for(transcript: &str, quote: &str, attributed: bool) -> Option<Span> {
    transcript.find(quote).map(|at| Span {
        start: at as u32,
        end: (at + quote.len()) as u32,
        attributed,
    })
}

pub fn load(conn: &Connection) -> Result<()> {
    load_seed(conn, SEED)
}

/// Takes the seed as an argument so a test can exercise paths the shipped
/// fixture happens not to reach -- a question on a suppressed entry, an edge
/// naming an id that is not there.
pub fn load_seed(conn: &Connection, raw: &str) -> Result<()> {
    let seed: Seed = serde_json::from_str(raw)?;

    // One transaction. A failure part-way used to leave entries written and
    // questions absent, and the next attempt was worse than a no-op: the
    // already-exists guard skipped the entries while anything that had failed
    // got re-placed against a field that no longer matched.
    let tx = conn.unchecked_transaction()?;

    // Placed against what is already there, not against nothing. Loading the
    // sample is offered from settings with a live corpus behind it, and
    // placing as if the canvas were empty puts the first sample entry exactly
    // where the user's first entry sits -- permanently, since positions freeze.
    let mut field: Vec<crate::scene::placement::PlacedNode> = db::entries::list(&tx)?
        .into_iter()
        .map(|e| {
            let (half_w, half_h) = db::create::title_box(&e.title, e.duration_ms);
            crate::scene::placement::PlacedNode {
                id: e.id,
                x: e.x,
                y: e.y,
                half_w,
                half_h,
                isolated: false,
            }
        })
        .collect();

    // Which entries the fixture actually contains, so an edge naming a
    // mistyped id is dropped rather than aborting the load on a foreign key.
    let known: std::collections::HashSet<&str> =
        seed.entries.iter().map(|e| e.id.as_str()).collect();

    // Placement order is the order every read uses, so the field is solved in
    // the order it will be seen in.
    let mut ordered: Vec<&SeedEntry> = seed.entries.iter().collect();
    ordered.sort_by(|a, b| a.created_at.cmp(&b.created_at));

    for s in ordered {
        if db::entries::get(&tx, &s.id)?.is_some() {
            continue;
        }

        let (half_w, half_h) = db::create::title_box(&s.title, s.duration_ms);
        // A stated relation is better evidence than a cosine guess, and it is
        // the thing that gets drawn -- a line long enough to cross the map is
        // a line nobody can trace. The fixture has the edges; use them.
        let links: Vec<String> = seed
            .edges
            .iter()
            .filter_map(|e| match (e.a.as_str(), e.b.as_str()) {
                (a, b) if a == s.id => Some(b.to_string()),
                (a, b) if b == s.id => Some(a.to_string()),
                _ => None,
            })
            .filter(|id| field.iter().any(|n| &n.id == id))
            .collect();

        let spot = crate::scene::placement::place(
            &crate::scene::placement::Candidate {
                id: s.id.clone(),
                vec: Vec::new(),
                half_w,
                half_h,
                links,
            },
            &field,
            &std::collections::HashMap::new(),
            crate::scene::placement::Options::default(),
        );

        let spans: Vec<Span> = s
            .attributed_quotes
            .iter()
            .filter_map(|q| span_for(&s.transcript, q, true))
            .collect();

        let action_items = s
            .action_items
            .iter()
            .enumerate()
            .filter_map(|(i, text)| {
                span_for(&s.transcript, text, false).map(|span| crate::model::ActionItem {
                    id: format!("{}-task-{i}", s.id),
                    entry_id: s.id.clone(),
                    span,
                    text: text.clone(),
                    done: false,
                })
            })
            .collect();

        let entry = Entry {
            id: s.id.clone(),
            audio_path: if s.audio {
                Some(format!("audio/{}.wav", s.id))
            } else {
                None
            },
            transcript: s.transcript.clone(),
            created_at: s.created_at.clone(),
            x: spot.x,
            y: spot.y,
            parent_entry_id: None,
            answers_question_id: None,
            role: s.role,
            register: s.register,
            type_id: s.type_id.clone(),
            resolved: s.resolved,
            resolution_text: s.resolution_text.clone(),
            title: s.title.clone(),
            // Belt and braces over the fixture: a live entry never carries one.
            summary: if s.register == Register::Live {
                None
            } else {
                s.summary.clone()
            },
            duration_ms: s.duration_ms,
            fingerprint: if s.audio {
                synthetic_fingerprint(&s.id, s.duration_ms)
            } else {
                Vec::new()
            },
            unfinished: db::create::detect_unfinished(&s.transcript),
            local_only: s.local_only,
            spans,
            action_items,
            is_sample: Some(true),
        };

        db::entries::insert(&tx, &entry)?;
        field.push(crate::scene::placement::PlacedNode {
            id: entry.id.clone(),
            x: spot.x,
            y: spot.y,
            half_w,
            half_h,
            isolated: spot.isolated,
        });
    }

    for e in &seed.edges {
        // An edge naming an entry the fixture does not contain is a typo, not
        // a connection. Dropped, rather than aborting on a foreign key.
        if !known.contains(e.a.as_str()) || !known.contains(e.b.as_str()) {
            continue;
        }
        let created_at = seed
            .entries
            .iter()
            .find(|s| s.id == e.b)
            .map(|s| s.created_at.clone())
            .unwrap_or_default();

        db::edges::insert(
            &tx,
            &Edge {
                // Derived from what the edge is, not where it sits in the
                // file. A positional id means adding one edge renumbers the
                // rest, and re-loading then collides an old id with a new pair.
                id: format!("edge-{}-{}", e.a, e.b),
                entry_a: e.a.clone(),
                entry_b: e.b.clone(),
                relation: e.relation,
                question: e.question.clone(),
                status: e.status,
                created_at,
            },
        )?;
    }

    for (i, q) in seed.questions.iter().enumerate() {
        let Some(entry) = db::entries::get(&tx, &q.entry_id)? else {
            continue;
        };
        // §3.2 is structural, not advisory: the three facets decide what may
        // carry a question, whatever the fixture says.
        if crate::enrich::gate::automatic_probes(&entry).is_empty() {
            continue;
        }

        let span = q
            .span_quote
            .as_ref()
            .and_then(|quote| span_for(&entry.transcript, quote, false));

        db::questions::insert(
            &tx,
            &Question {
                id: format!("question-{}-{i}", q.entry_id),
                entry_id: q.entry_id.clone(),
                text: q.text.clone(),
                span,
                answered: q.answered,
                dismissed: false,
                provider_name: q.provider_name.clone(),
                created_at: entry.created_at.clone(),
            },
            &entry.transcript,
        )?;
    }

    tx.commit()?;
    Ok(())
}

/// The fixture has no audio, so the bars are generated -- stable per entry,
/// and honest about being a stand-in rather than a waveform.
///
/// Duration gates how likely a bar is to be a breath gap, not how tall the
/// bars are. Scaling height by duration made every sample cap at about half
/// its box, because the longest entry in the fixture is 43 seconds and the
/// reference is four minutes.
fn synthetic_fingerprint(id: &str, duration_ms: i64) -> Vec<f32> {
    let bars = 7 + (crate::scene::vector::hash32(id) % 3) as usize;
    let density = (duration_ms.max(0) as f32 / 240_000.0).clamp(0.0, 1.0);

    (0..bars)
        .map(|i| {
            // Re-hashed per bar. Multiplying one seed by the index makes the
            // bars an arithmetic progression, which draws as a staircase.
            let jitter =
                (crate::scene::vector::hash32(&format!("{id}:{i}")) % 1000) as f32 / 1000.0;

            // Onset, body, tail -- speech does not start at full volume.
            let t = (i as f32 + 0.5) / bars as f32;
            let envelope = (t * std::f32::consts::PI).sin().max(0.45);

            // A short recording is more likely to show gaps than a long one.
            if jitter < 0.12 * (1.0 - density) {
                0.12
            } else {
                ((0.35 + jitter * 0.65) * envelope).clamp(0.12, 1.0)
            }
        })
        .collect()
}

/// Removes the sample and anything left dangling by its removal.
pub fn clear(conn: &Connection) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    // An answer the user recorded against a sample question outlives it, and
    // would otherwise keep pointing at a question that no longer exists.
    tx.execute(
        "UPDATE entries SET answers_question_id = NULL
         WHERE answers_question_id IN (SELECT id FROM questions WHERE entry_id IN
             (SELECT id FROM entries WHERE is_sample = 1))",
        [],
    )?;
    tx.execute("DELETE FROM entries WHERE is_sample = 1", [])?;
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;

    #[test]
    fn the_sample_loads() {
        let conn = open_in_memory().unwrap();
        load(&conn).unwrap();

        let entries = db::entries::list(&conn).unwrap();
        assert!(entries.len() >= 10, "got {}", entries.len());
        assert!(entries.iter().all(|e| e.is_sample == Some(true)));
        assert!(!db::edges::list(&conn).unwrap().is_empty());
    }

    /// Loading twice used to produce two copies of every edge, and a repeated
    /// React key strands the element it cannot match.
    #[test]
    fn loading_twice_changes_nothing() {
        let conn = open_in_memory().unwrap();
        load(&conn).unwrap();
        let (entries, edges, questions) = counts(&conn);

        load(&conn).unwrap();
        assert_eq!(counts(&conn), (entries, edges, questions));
    }

    #[test]
    fn no_two_sample_entries_share_a_position() {
        let conn = open_in_memory().unwrap();
        load(&conn).unwrap();

        let all = db::entries::list(&conn).unwrap();
        for a in &all {
            for b in &all {
                if a.id != b.id {
                    assert!(
                        (a.x - b.x).abs() > 1.0 || (a.y - b.y).abs() > 1.0,
                        "{} and {} overlap",
                        a.id,
                        b.id
                    );
                }
            }
        }
    }

    /// A seeded question on an entry the gate would silence must not survive
    /// the loader -- suppression is structural, not advisory.
    #[test]
    fn seeded_questions_still_pass_the_gate() {
        let conn = open_in_memory().unwrap();
        load(&conn).unwrap();

        for q in db::questions::list(&conn).unwrap() {
            let entry = db::entries::get(&conn, &q.entry_id).unwrap().unwrap();
            assert!(
                !crate::enrich::gate::automatic_probes(&entry).is_empty(),
                "{} carries a question the gate would suppress",
                entry.id
            );
        }
    }

    #[test]
    fn clearing_removes_the_sample() {
        let conn = open_in_memory().unwrap();
        load(&conn).unwrap();
        clear(&conn).unwrap();

        assert!(db::entries::list(&conn).unwrap().is_empty());
        assert!(db::edges::list(&conn).unwrap().is_empty());
        assert!(db::questions::list(&conn).unwrap().is_empty());
    }

    /// The destructive case, and the one no earlier test could see: loading
    /// the sample is offered from settings with a live corpus behind it.
    #[test]
    fn loading_into_a_corpus_does_not_land_on_what_is_already_there() {
        let conn = open_in_memory().unwrap();
        let mine = db::create::create(
            &conn,
            db::create::NewEntry {
                transcript: "Something I said first.".into(),
                duration_ms: 31_000,
                fingerprint: vec![0.4],
                parent_entry_id: None,
                local_only: None,
                typed: false,
            },
        )
        .unwrap();

        load(&conn).unwrap();

        let all = db::entries::list(&conn).unwrap();
        let mine_after = all.iter().find(|e| e.id == mine.id).unwrap();
        assert_eq!(
            (mine_after.x, mine_after.y),
            (mine.x, mine.y),
            "my entry moved"
        );

        for a in &all {
            for b in &all {
                if a.id != b.id {
                    assert!(
                        (a.x - b.x).abs() > 1.0 || (a.y - b.y).abs() > 1.0,
                        "{} and {} share a position",
                        a.id,
                        b.id
                    );
                }
            }
        }
    }

    /// `clear` removes the sample. It must not remove the corpus.
    #[test]
    fn clearing_spares_what_the_user_recorded() {
        let conn = open_in_memory().unwrap();
        load(&conn).unwrap();
        let mine = db::create::create(
            &conn,
            db::create::NewEntry {
                transcript: "Mine, not the sample's.".into(),
                duration_ms: 31_000,
                fingerprint: vec![],
                parent_entry_id: None,
                local_only: None,
                typed: true,
            },
        )
        .unwrap();

        clear(&conn).unwrap();

        let left = db::entries::list(&conn).unwrap();
        assert_eq!(left.len(), 1, "only mine should remain");
        assert_eq!(left[0].id, mine.id);
    }

    /// An answer to a sample question outlives the question, and must not be
    /// left pointing at one that no longer exists.
    #[test]
    fn clearing_unlinks_answers_to_sample_questions() {
        let conn = open_in_memory().unwrap();
        load(&conn).unwrap();
        let question = db::questions::list(&conn).unwrap().remove(0);

        let answer = db::create::create(
            &conn,
            db::create::NewEntry {
                transcript: "What I said back.".into(),
                duration_ms: 31_000,
                fingerprint: vec![],
                parent_entry_id: None,
                local_only: None,
                typed: true,
            },
        )
        .unwrap();
        conn.execute(
            "UPDATE entries SET answers_question_id = ?2 WHERE id = ?1",
            rusqlite::params![answer.id, question.id],
        )
        .unwrap();

        clear(&conn).unwrap();

        let dangling: Option<String> = conn
            .query_row(
                "SELECT answers_question_id FROM entries WHERE id = ?1",
                rusqlite::params![answer.id],
                |r| r.get(0),
            )
            .unwrap();
        assert!(dangling.is_none(), "it points at a question that is gone");
    }

    /// Suppression is structural, not advisory. The shipped fixture has no
    /// question the gate would stop, so this hands the loader one.
    #[test]
    fn a_seeded_question_on_a_suppressed_entry_is_dropped() {
        let conn = open_in_memory().unwrap();
        let crafted = serde_json::json!({
            "entries": [{
                "id": "brief", "createdAt": "2024-01-01T00:00:00Z",
                "transcript": "Buy a cable.", "title": "buy a cable",
                "summary": null, "role": "note", "register": "neutral",
                "typeId": "note", "durationMs": 5000, "audio": false,
                "resolved": false, "resolutionText": null, "localOnly": false
            }],
            "edges": [],
            "questions": [{
                "entryId": "brief", "text": "Why that cable?",
                "spanQuote": null, "answered": false, "providerName": "test"
            }]
        })
        .to_string();

        load_seed(&conn, &crafted).unwrap();

        assert_eq!(db::entries::list(&conn).unwrap().len(), 1);
        assert!(
            db::questions::list(&conn).unwrap().is_empty(),
            "a five-second note carries no automatic question, whatever the fixture says"
        );
    }

    /// A mistyped id used to abort the whole load on a foreign key, after the
    /// entries had already been written.
    #[test]
    fn an_edge_naming_an_unknown_entry_is_dropped_not_fatal() {
        let conn = open_in_memory().unwrap();
        let crafted = serde_json::json!({
            "entries": [{
                "id": "real", "createdAt": "2024-01-01T00:00:00Z",
                "transcript": "A position I hold, at some length.", "title": "a position",
                "summary": "s", "role": "position", "register": "neutral",
                "typeId": "position", "durationMs": 40000, "audio": true,
                "resolved": false, "resolutionText": null, "localOnly": false
            }],
            "edges": [{ "a": "real", "b": "typo", "relation": "extends",
                        "status": "proposed", "question": null }],
            "questions": []
        })
        .to_string();

        load_seed(&conn, &crafted).unwrap();

        assert_eq!(
            db::entries::list(&conn).unwrap().len(),
            1,
            "the entry still lands"
        );
        assert!(
            db::edges::list(&conn).unwrap().is_empty(),
            "the bad edge does not"
        );
    }

    /// An entry missing its optional arrays must not take the load with it.
    #[test]
    fn a_fixture_omitting_optional_arrays_still_loads() {
        let conn = open_in_memory().unwrap();
        let crafted = serde_json::json!({
            "entries": [{
                "id": "sparse", "createdAt": "2024-01-01T00:00:00Z",
                "transcript": "Said without ceremony.", "title": "said plainly",
                "summary": null, "role": "note", "register": "neutral",
                "typeId": "note", "durationMs": 4000,
                "resolutionText": null
            }],
            "edges": [], "questions": []
        })
        .to_string();

        load_seed(&conn, &crafted).unwrap();
        assert_eq!(db::entries::list(&conn).unwrap().len(), 1);
    }

    /// Scaling height by duration capped every bar in the fixture at about
    /// half its box, because the longest entry is 43 seconds.
    #[test]
    fn a_short_recording_still_draws_a_full_height_bar() {
        let tallest = (0..200)
            .map(|i| synthetic_fingerprint(&format!("entry-{i}"), 20_000))
            .flatten()
            .fold(0.0_f32, f32::max);
        assert!(tallest > 0.9, "tallest bar across the corpus was {tallest}");
    }

    #[test]
    fn a_fingerprint_is_always_seven_to_nine_bars_within_range() {
        for duration in [0_i64, -1, 5_000, 43_000, 600_000, i64::MAX] {
            for id in ["", "a", "free-will-own-reasoning", "日本語"] {
                let bars = synthetic_fingerprint(id, duration);
                assert!(
                    (7..=9).contains(&bars.len()),
                    "{id}/{duration}: {} bars",
                    bars.len()
                );
                for b in bars {
                    assert!((0.12..=1.0).contains(&b), "{id}/{duration}: bar {b}");
                    assert!(!b.is_nan());
                }
            }
        }
    }

    fn counts(conn: &Connection) -> (usize, usize, usize) {
        (
            db::entries::list(conn).unwrap().len(),
            db::edges::list(conn).unwrap().len(),
            db::questions::list(conn).unwrap().len(),
        )
    }
}
