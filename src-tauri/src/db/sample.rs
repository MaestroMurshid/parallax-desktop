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
    audio: bool,
    resolved: bool,
    resolution_text: Option<String>,
    local_only: bool,
    /// Substrings of the transcript, so an anchor is found rather than stated.
    attributed_quotes: Vec<String>,
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
    let seed: Seed = serde_json::from_str(SEED)?;

    // Placement replays in order, so the coordinates come from the real
    // algorithm rather than being authored -- the same path a live insert
    // takes, and what makes position encode when.
    let mut field: Vec<crate::scene::placement::PlacedNode> = Vec::new();

    for s in &seed.entries {
        if db::entries::get(conn, &s.id)?.is_some() {
            continue;
        }

        let (half_w, half_h) = db::create::title_box(&s.title, s.duration_ms);
        let spot = crate::scene::placement::place(
            &crate::scene::placement::Candidate {
                id: s.id.clone(),
                vec: Vec::new(),
                half_w,
                half_h,
                links: Vec::new(),
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

        db::entries::insert(conn, &entry)?;
        field.push(crate::scene::placement::PlacedNode {
            id: entry.id.clone(),
            x: spot.x,
            y: spot.y,
            half_w,
            half_h,
            isolated: spot.isolated,
        });
    }

    for (i, e) in seed.edges.iter().enumerate() {
        let created_at = seed
            .entries
            .iter()
            .find(|s| s.id == e.b)
            .map(|s| s.created_at.clone())
            .unwrap_or_default();

        db::edges::insert(
            conn,
            &Edge {
                id: format!("edge-{i}"),
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
        let Some(entry) = db::entries::get(conn, &q.entry_id)? else {
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
            conn,
            &Question {
                id: format!("question-{i}"),
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

    Ok(())
}

/// The fixture has no audio, so the bars are generated from the id -- stable
/// per entry, and honest about being a stand-in rather than a waveform.
fn synthetic_fingerprint(id: &str, duration_ms: i64) -> Vec<f32> {
    let seed = crate::scene::vector::hash32(id);
    let bars = 7 + (seed % 3) as usize;
    let density = (duration_ms as f32 / 240_000.0).min(1.0);
    (0..bars)
        .map(|i| {
            let noise = ((seed.wrapping_mul(i as u32 + 1) >> 8) % 100) as f32 / 100.0;
            (0.12 + noise * 0.88 * (0.4 + density * 0.6)).clamp(0.12, 1.0)
        })
        .collect()
}

/// Removes the sample and anything left dangling by its removal.
pub fn clear(conn: &Connection) -> Result<()> {
    conn.execute("DELETE FROM entries WHERE is_sample = 1", [])?;
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

    fn counts(conn: &Connection) -> (usize, usize, usize) {
        (
            db::entries::list(conn).unwrap().len(),
            db::edges::list(conn).unwrap().len(),
            db::questions::list(conn).unwrap().len(),
        )
    }
}
