//! Which earlier notes this one is compared against.
//!
//! Two stages, because they do different jobs and neither can do both.
//! Topics decide *who is eligible* -- they are the shelf, they carry a reason
//! a person can read, and they go into the MDX frontmatter. Cosine decides
//! *what order*, because a shelf with forty notes on it still needs the eight
//! worth spending a judge call on.
//!
//! The cap is the whole point. Judging is one model call per pair, so an
//! uncapped candidate set is one call per note in the corpus.
//!
//! Topics are coined freely, so one subject lands on several shelves
//! (`theology`, `religion`). The few nearest notes by meaning are eligible
//! whatever shelf they sit on, or the closest pair can be the one never judged.

use crate::db;
use crate::error::Result;
use rusqlite::Connection;
use std::collections::HashSet;

/// How many notes off this note's shelves are judged anyway, nearest first.
pub const NEAREST_OFF_SHELF: usize = 3;

/// At most `k` earlier notes, best first.
///
/// A note with no vector yet is still a candidate. Embedding is a separate
/// pass that can lag or fail, and dropping an unembedded note would silently
/// shrink the corpus to whatever the embedder had got to.
pub fn candidates(conn: &Connection, entry_id: &str, k: usize) -> Result<Vec<String>> {
    if k == 0 {
        return Ok(Vec::new());
    }
    let eligible = db::tags::sharing(conn, entry_id)?;
    if eligible.is_empty() {
        // No usable shelf: either nothing shares a topic, or the corpus is so
        // single-minded that its one topic is on more than half of it and was
        // declined. Cosine alone then, because the alternative is a corpus
        // about one subject never proposing anything for as long as it lives.
        // The cap still holds, which is what the cost actually depends on.
        return Ok(db::vectors::similar(conn, entry_id, k)?
            .into_iter()
            .map(|(id, _)| id)
            .collect());
    }
    let on_shelf: HashSet<&str> = eligible.iter().map(|(id, _)| id.as_str()).collect();

    // The whole corpus is ranked in one pass rather than the shelf being
    // ranked on its own: the scan is arithmetic over unit vectors, about 3ms
    // at ten thousand notes, and doing it this way leaves one ranking rule
    // instead of two that can disagree. Empty when this note has no vector.
    let ranked = db::vectors::similar(conn, entry_id, usize::MAX)?;
    // The nearest few off the shelf join it, still in cosine order, so a
    // crowded shelf only pushes one out by being closer.
    let nearest_elsewhere: HashSet<&str> = ranked
        .iter()
        .map(|(id, _)| id.as_str())
        .filter(|id| !on_shelf.contains(id))
        .take(NEAREST_OFF_SHELF)
        .collect();
    let mut out: Vec<String> = ranked
        .iter()
        .map(|(id, _)| id.as_str())
        .filter(|id| on_shelf.contains(id) || nearest_elsewhere.contains(id))
        .map(str::to_string)
        .collect();

    // Whatever cosine could not order keeps the shelf's own order, most
    // shared topics first, and follows behind.
    for (id, _) in &eligible {
        if !out.contains(id) {
            out.push(id.clone());
        }
    }
    out.truncate(k);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tags::Kind;

    fn note(conn: &Connection, transcript: &str) -> String {
        db::create::create(
            conn,
            db::create::NewEntry {
                transcript: transcript.into(),
                duration_ms: 5_000,
                fingerprint: vec![],
                parent_entry_id: None,
                local_only: None,
                typed: true,
            },
        )
        .unwrap()
        .id
    }

    fn shelve(conn: &Connection, id: &str, topics: &[&str]) {
        let names: Vec<String> = topics.iter().map(|t| t.to_string()).collect();
        let ids = db::tags::upsert(conn, &names, Kind::Topic).unwrap();
        db::tags::set_for_entry(conn, id, &ids).unwrap();
    }

    /// Found in the packaged app: two notes about god sat on different shelves
    /// (`theology` against `religion`), the closest pair in the corpus by
    /// meaning, and were never judged because each had a shelf of its own.
    #[test]
    fn a_close_note_on_another_shelf_is_still_a_candidate() {
        let conn = db::open_in_memory().unwrap();
        let me = note(&conn, "the problem of evil");
        let same = note(&conn, "free will");
        let near = note(&conn, "religion survives socially");
        shelve(&conn, &me, &["theology"]);
        shelve(&conn, &same, &["theology"]);
        shelve(&conn, &near, &["religion"]);

        db::vectors::set(&conn, &me, "m", &[1.0, 0.0]).unwrap();
        db::vectors::set(&conn, &same, "m", &[0.0, 1.0]).unwrap();
        db::vectors::set(&conn, &near, "m", &[1.0, 0.0]).unwrap();

        let got = candidates(&conn, &me, 8).unwrap();
        assert!(
            got.contains(&near),
            "the nearest note by meaning was not judged"
        );
        assert!(got.contains(&same), "the shelf still counts");
    }

    /// The shelf stays the filter: meaning adds only the few nearest, so a
    /// large corpus on other subjects does not buy a model call per note.
    #[test]
    fn only_the_nearest_few_off_the_shelf_are_added() {
        let conn = db::open_in_memory().unwrap();
        let me = note(&conn, "me");
        let mate = note(&conn, "mate");
        shelve(&conn, &me, &["databases"]);
        shelve(&conn, &mate, &["databases"]);
        db::vectors::set(&conn, &me, "m", &[1.0, 0.0]).unwrap();
        db::vectors::set(&conn, &mate, "m", &[0.0, 1.0]).unwrap();

        let mut elsewhere = Vec::new();
        for i in 0..6 {
            let id = note(&conn, &format!("other {i}"));
            shelve(&conn, &id, &["cooking"]);
            // Closer to `me` the lower `i` is.
            let x = 0.95 - i as f32 * 0.1;
            let y = (1.0 - x * x).sqrt();
            db::vectors::set(&conn, &id, "m", &[x, y]).unwrap();
            elsewhere.push(id);
        }

        let got = candidates(&conn, &me, 8).unwrap();
        assert!(got.contains(&mate));
        for near in &elsewhere[..NEAREST_OFF_SHELF] {
            assert!(got.contains(near), "one of the nearest was left out");
        }
        for far in &elsewhere[NEAREST_OFF_SHELF..] {
            assert!(!got.contains(far), "a distant note off the shelf was added");
        }
    }

    #[test]
    fn cosine_orders_the_shelf() {
        let conn = db::open_in_memory().unwrap();
        let me = note(&conn, "me");
        let near = note(&conn, "near");
        let mid = note(&conn, "mid");
        let far = note(&conn, "far");
        for id in [&me, &near, &mid, &far] {
            shelve(&conn, id, &["databases"]);
        }
        db::vectors::set(&conn, &me, "m", &[1.0, 0.0]).unwrap();
        db::vectors::set(&conn, &near, "m", &[0.99, 0.14]).unwrap();
        db::vectors::set(&conn, &mid, "m", &[0.7, 0.7]).unwrap();
        db::vectors::set(&conn, &far, "m", &[0.0, 1.0]).unwrap();

        assert_eq!(candidates(&conn, &me, 2).unwrap(), vec![near, mid]);
    }

    /// Embedding is a separate pass that can lag or fail. An unembedded note
    /// must still be judged, or the corpus silently shrinks to whatever the
    /// embedder reached -- it just goes after the notes that can be ranked.
    #[test]
    fn a_note_with_no_vector_is_still_a_candidate() {
        let conn = db::open_in_memory().unwrap();
        let me = note(&conn, "me");
        let ranked = note(&conn, "ranked");
        let unembedded = note(&conn, "unembedded");
        for id in [&me, &ranked, &unembedded] {
            shelve(&conn, id, &["databases"]);
        }
        db::vectors::set(&conn, &me, "m", &[1.0, 0.0]).unwrap();
        db::vectors::set(&conn, &ranked, "m", &[0.0, 1.0]).unwrap();

        let got = candidates(&conn, &me, 8).unwrap();
        assert_eq!(
            got,
            vec![ranked, unembedded],
            "the unembedded note vanished"
        );
    }

    /// Before the embedder has run at all, topics alone still propose.
    #[test]
    fn with_no_vectors_the_shelf_alone_still_proposes() {
        let conn = db::open_in_memory().unwrap();
        let me = note(&conn, "me");
        let a = note(&conn, "a");
        for id in [&me, &a] {
            shelve(&conn, id, &["databases"]);
        }
        assert_eq!(candidates(&conn, &me, 8).unwrap(), vec![a]);
    }

    /// One model call per pair, so the cap is the only thing keeping the cost
    /// of a capture off the size of the corpus.
    #[test]
    fn the_candidate_set_is_capped() {
        let conn = db::open_in_memory().unwrap();
        let me = note(&conn, "me");
        shelve(&conn, &me, &["databases"]);
        db::vectors::set(&conn, &me, "m", &[1.0, 0.0]).unwrap();
        // Ten on the shelf and ten off it, so `databases` stays under half the
        // corpus and is still a usable key.
        for i in 0..20 {
            let other = note(&conn, "other");
            shelve(
                &conn,
                &other,
                if i < 10 {
                    &["databases"]
                } else {
                    &["elsewhere"]
                },
            );
            db::vectors::set(&conn, &other, "m", &[1.0, i as f32 / 20.0]).unwrap();
        }
        assert_eq!(candidates(&conn, &me, 8).unwrap().len(), 8);
    }

    /// A corpus about one thing. Its single topic covers more than half of it,
    /// so it is declined as a key and there is no shelf left to stand on --
    /// and the app must still propose, or it proposes nothing for ever.
    #[test]
    fn a_single_minded_corpus_still_gets_candidates() {
        let conn = db::open_in_memory().unwrap();
        let me = note(&conn, "me");
        shelve(&conn, &me, &["databases"]);
        db::vectors::set(&conn, &me, "m", &[1.0, 0.0]).unwrap();
        for i in 0..20 {
            let other = note(&conn, "other");
            shelve(&conn, &other, &["databases"]);
            db::vectors::set(&conn, &other, "m", &[1.0, i as f32 / 20.0]).unwrap();
        }
        assert!(
            db::tags::sharing(&conn, &me).unwrap().is_empty(),
            "the shelf should have been declined for covering the corpus"
        );
        assert_eq!(candidates(&conn, &me, 8).unwrap().len(), 8);
    }
}
