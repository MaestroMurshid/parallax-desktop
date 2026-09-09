//! Creating an entry: place it against the existing field, then persist.
//!
//! Position is solved once here and frozen (§5.1). Everything else an entry
//! eventually carries -- title, role, register, summary, spans -- is enrichment
//! and arrives later; what this owes the caller is a row that exists and a
//! place on the canvas.

use crate::error::Result;
use crate::model::{Entry, Register, Role};
use crate::scene::placement::{place, Candidate, Options, PlacedNode};
use rusqlite::Connection;
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewEntry {
    pub transcript: String,
    pub duration_ms: i64,
    #[serde(default)]
    pub fingerprint: Vec<f32>,
    /// Set when this was recorded as an answer.
    #[serde(default, rename = "parentEdge")]
    pub parent_entry_id: Option<String>,
    #[serde(default)]
    pub local_only: Option<bool>,
    /// Nobody dictates a list (§4). A typed entry has no audio and so no
    /// fingerprint, which is a free visual distinction.
    #[serde(default)]
    pub typed: bool,
}

pub fn create(_conn: &Connection, _draft: NewEntry) -> Result<Entry> {
    todo!("create")
}

/// Title size follows duration, so the box a new entry has to fit is known
/// before it is placed. Mirrors `titleSizeForDuration` in lexicon.ts.
pub fn title_box(_title: &str, _duration_ms: i64) -> (f64, f64) {
    todo!("title_box")
}

#[allow(dead_code)]
fn field_from(_conn: &Connection) -> Result<(Vec<PlacedNode>, HashMap<String, Vec<f32>>)> {
    todo!("field")
}

#[allow(dead_code)]
fn unused(_: Role, _: Register, _: Candidate, _: Options) {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{entries, open_in_memory};

    fn draft(transcript: &str) -> NewEntry {
        NewEntry {
            transcript: transcript.into(),
            duration_ms: 31_000,
            fingerprint: vec![0.2, 0.7, 0.5],
            parent_entry_id: None,
            local_only: None,
            typed: false,
        }
    }

    #[test]
    fn a_new_entry_is_persisted_and_readable() {
        let conn = open_in_memory().unwrap();
        let made = create(&conn, draft("Indexes trade writes for reads.")).unwrap();

        let all = entries::list(&conn).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].id, made.id);
        assert_eq!(all[0].transcript, "Indexes trade writes for reads.");
    }

    #[test]
    fn the_transcript_is_stored_verbatim() {
        let conn = open_in_memory().unwrap();
        let said = "  I don't think free will requires that.  ";
        let made = create(&conn, draft(said)).unwrap();
        assert_eq!(made.transcript, said, "the record is never tidied");
    }

    /// A typed entry has no audio row, so no path and no fingerprint, without
    /// anything having to remember to null them.
    #[test]
    fn a_typed_entry_carries_no_audio() {
        let conn = open_in_memory().unwrap();
        let mut d = draft("Buy a cable. Renew the token.");
        d.typed = true;
        d.fingerprint = vec![0.9, 0.9];

        let made = create(&conn, d).unwrap();
        assert!(made.audio_path.is_none());
        assert!(made.fingerprint.is_empty(), "a typed entry has no waveform");
    }

    #[test]
    fn a_voice_entry_keeps_its_audio_path_and_fingerprint() {
        let conn = open_in_memory().unwrap();
        let made = create(&conn, draft("Spoken.")).unwrap();
        assert!(made.audio_path.is_some());
        assert_eq!(made.fingerprint, vec![0.2, 0.7, 0.5]);
    }

    /// §5.1 -- the first entry has nothing to be near.
    #[test]
    fn the_first_entry_lands_at_the_origin() {
        let conn = open_in_memory().unwrap();
        let made = create(&conn, draft("First.")).unwrap();
        assert_eq!((made.x, made.y), (0.0, 0.0));
    }

    /// The whole point of freezing: adding an entry must not move any that
    /// were already placed.
    #[test]
    fn placing_a_new_entry_moves_nothing_already_there() {
        let conn = open_in_memory().unwrap();
        create(&conn, draft("First.")).unwrap();
        create(&conn, draft("Second.")).unwrap();
        let before: Vec<(String, f64, f64)> = entries::list(&conn)
            .unwrap()
            .into_iter()
            .map(|e| (e.id, e.x, e.y))
            .collect();

        create(&conn, draft("Third.")).unwrap();

        let after = entries::list(&conn).unwrap();
        for (id, x, y) in before {
            let now = after.iter().find(|e| e.id == id).unwrap();
            assert_eq!((now.x, now.y), (x, y), "{id} moved");
        }
    }

    #[test]
    fn entries_do_not_land_on_top_of_each_other() {
        let conn = open_in_memory().unwrap();
        for i in 0..6 {
            create(&conn, draft(&format!("Note number {i}."))).unwrap();
        }
        let all = entries::list(&conn).unwrap();
        for a in &all {
            for b in &all {
                if a.id == b.id {
                    continue;
                }
                assert!(
                    (a.x - b.x).abs() > 1.0 || (a.y - b.y).abs() > 1.0,
                    "{} and {} share a position",
                    a.id,
                    b.id
                );
            }
        }
    }

    /// Under uncertainty register defaults to live, which suppresses the
    /// automatic question: a missed question costs less than a probe on
    /// something raw. Classification replaces this when enrichment runs.
    #[test]
    fn an_unenriched_entry_defaults_to_live() {
        let conn = open_in_memory().unwrap();
        let made = create(&conn, draft("Something just said.")).unwrap();
        assert_eq!(made.register, Register::Live);
        assert!(made.summary.is_none(), "no summary is written for a live entry");
    }

    /// Something has to be readable on the canvas before the model has looked
    /// at it, so a placeholder title is derived from the user's own words.
    #[test]
    fn an_unenriched_entry_still_has_a_readable_title() {
        let conn = open_in_memory().unwrap();
        let made = create(&conn, draft("Database indexes trade write performance for reads."))
            .unwrap();
        assert!(!made.title.is_empty());
        assert!(made.title.len() <= 40, "a title is a handle, not a summary");
    }

    #[test]
    fn ids_are_unique_across_entries() {
        let conn = open_in_memory().unwrap();
        let a = create(&conn, draft("One.")).unwrap();
        let b = create(&conn, draft("Two.")).unwrap();
        assert_ne!(a.id, b.id);
    }

    /// Detected by regex, never by a model (§5.3).
    #[test]
    fn a_hedge_marks_the_entry_unfinished() {
        let conn = open_in_memory().unwrap();
        let hedged = create(&conn, draft("I'm not sure what this means yet.")).unwrap();
        let plain = create(&conn, draft("Indexes cost writes.")).unwrap();
        assert!(hedged.unfinished);
        assert!(!plain.unfinished);
    }

    #[test]
    fn local_only_falls_back_to_the_setting() {
        let conn = open_in_memory().unwrap();
        crate::db::settings::merge(&conn, serde_json::json!({ "defaultLocalOnly": true }))
            .unwrap();
        let made = create(&conn, draft("Private.")).unwrap();
        assert!(made.local_only);
    }

    #[test]
    fn an_answer_records_the_entry_it_answers() {
        let conn = open_in_memory().unwrap();
        let parent = create(&conn, draft("The original thought.")).unwrap();

        let mut d = draft("What I said back.");
        d.parent_entry_id = Some(parent.id.clone());
        let answer = create(&conn, d).unwrap();

        assert_eq!(answer.parent_entry_id, Some(parent.id.clone()));
        let children = entries::children_of(&conn, &parent.id).unwrap();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].id, answer.id);
    }

    #[test]
    fn a_longer_recording_gets_a_bigger_box() {
        let (short_w, _) = title_box("same title", 5_000);
        let (long_w, _) = title_box("same title", 240_000);
        assert!(long_w > short_w, "duration drives title size (§5.2)");
    }
}
