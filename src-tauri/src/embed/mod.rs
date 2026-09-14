//! Sentence vectors, and the pass that fills them in.
//!
//! Local unconditionally and on the CPU, which is why this is not behind the
//! provider abstraction `llm` uses -- there is no remote alternative for it to
//! be abstracted over. A note's vector is what lets it meet a note that chose
//! different words for the same idea; topics decide who is eligible and this
//! decides the order among them.
//!
//! Everything here is optional. With no embedder the shelf still proposes, so
//! a missing or failing model costs ranking quality and nothing else.

pub mod llama;

use crate::db;
use crate::error::Result;
use rusqlite::Connection;

pub trait Embedder: Send + Sync {
    /// Stored beside every vector it writes. Two models share no vector space,
    /// so a corpus embedded by one must never be ranked against the other.
    fn model_id(&self) -> String;

    fn embed(&self, text: &str) -> Result<Vec<f32>>;
}

/// Embeds entries this model has not embedded yet, oldest first.
///
/// Returns how many landed. Capped rather than unbounded because it runs
/// behind a capture: a corpus of ten thousand notes must not hold up the note
/// someone just spoke, and the next capture picks up where this left off.
pub fn backfill(conn: &Connection, embedder: &dyn Embedder, limit: usize) -> Result<usize> {
    let model = embedder.model_id();
    let mut done = 0;
    for id in db::vectors::missing(conn, &model, limit)? {
        let Some(entry) = db::entries::get(conn, &id)? else {
            continue;
        };
        // One note the model chokes on must not cost the rest of the corpus,
        // and the next pass will try it again -- it is still missing.
        match embedder.embed(&entry.transcript) {
            Ok(vector) => match db::vectors::set(conn, &id, &model, &vector) {
                Ok(()) => done += 1,
                Err(e) => eprintln!("storing a vector failed for {id}: {e}"),
            },
            Err(e) => eprintln!("embedding failed for {id}: {e}"),
        }
    }
    Ok(done)
}

/// A few per capture, not the whole corpus: this runs behind a note someone
/// just spoke, and a thousand-note backlog must not hold it up. Successive
/// captures walk through the rest.
const BACKFILL_PER_CAPTURE: usize = 5;

/// This note first, then a little of whatever the corpus still owes.
///
/// This note first because it is the one about to be proposed against; the
/// older ones can arrive over the next few captures without anyone noticing.
/// Re-embedding a corrected transcript falls out of the same call, because
/// `set` replaces.
pub fn embed_now(conn: &Connection, embedder: &dyn Embedder, entry_id: &str) -> Result<()> {
    let entry = db::entries::get(conn, entry_id)?
        .ok_or_else(|| crate::error::Error::NotFound(format!("no entry {entry_id}")))?;
    let vector = embedder.embed(&entry.transcript)?;
    db::vectors::set(conn, entry_id, &embedder.model_id(), &vector)?;
    backfill(conn, embedder, BACKFILL_PER_CAPTURE)?;
    Ok(())
}

/// Every note this model has not embedded, with no per-capture cap.
///
/// For an upload, which arrives with no vectors at all: until they are back,
/// `ask` finds nothing and candidates lose their ordering. Locks per note
/// rather than across the pass, so a few hundred notes of embedding never
/// holds enrichment up behind it.
pub fn catch_up(db: &std::sync::Mutex<Connection>, embedder: &dyn Embedder) -> Result<usize> {
    const BATCH: usize = 32;
    let lock = || db.lock().unwrap_or_else(|p| p.into_inner());
    let model = embedder.model_id();
    // A note that fails stays missing, so it is remembered here or every batch
    // would hand it back and the pass would never end.
    let mut tried = std::collections::HashSet::new();
    let mut done = 0;
    loop {
        let batch: Vec<(String, String)> = {
            let conn = lock();
            let mut batch = Vec::new();
            for id in db::vectors::missing(&conn, &model, tried.len() + BATCH)? {
                if !tried.insert(id.clone()) {
                    continue;
                }
                if let Some(entry) = db::entries::get(&conn, &id)? {
                    batch.push((id, entry.transcript));
                }
            }
            batch
        };
        if batch.is_empty() {
            return Ok(done);
        }
        for (id, transcript) in batch {
            match embedder.embed(&transcript) {
                Ok(vector) => match db::vectors::set(&lock(), &id, &model, &vector) {
                    Ok(()) => done += 1,
                    Err(e) => eprintln!("storing a vector failed for {id}: {e}"),
                },
                Err(e) => eprintln!("embedding failed for {id}: {e}"),
            }
        }
    }
}

#[cfg(test)]
pub mod fake {
    use super::*;
    use std::sync::Mutex;

    /// Deterministic from the text, so a test can assert an order rather than
    /// just a count. `refuses` is how a model that throws is exercised.
    pub struct FakeEmbedder {
        pub model: String,
        pub refuses: Mutex<Vec<String>>,
    }

    impl FakeEmbedder {
        pub fn new(model: &str) -> Self {
            Self {
                model: model.into(),
                refuses: Mutex::new(Vec::new()),
            }
        }

        pub fn refusing(model: &str, texts: &[&str]) -> Self {
            Self {
                model: model.into(),
                refuses: Mutex::new(texts.iter().map(|t| t.to_string()).collect()),
            }
        }
    }

    impl Embedder for FakeEmbedder {
        fn model_id(&self) -> String {
            self.model.clone()
        }

        fn embed(&self, text: &str) -> Result<Vec<f32>> {
            if self
                .refuses
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .iter()
                .any(|t| t == text)
            {
                return Err(crate::error::Error::Other("refused".into()));
            }
            let n = text.len() as f32;
            Ok(vec![n, 1.0])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::FakeEmbedder;
    use super::*;

    fn note(conn: &Connection, transcript: &str, created_at: &str) -> String {
        let id = db::create::create(
            conn,
            db::create::NewEntry {
                transcript: transcript.into(),
                duration_ms: 1_000,
                fingerprint: vec![],
                parent_entry_id: None,
                local_only: None,
                typed: true,
            },
        )
        .unwrap()
        .id;
        conn.execute(
            "UPDATE entries SET created_at = ?2 WHERE id = ?1",
            rusqlite::params![id, created_at],
        )
        .unwrap();
        id
    }

    /// The new note is embedded before the backlog, because it is the one
    /// about to be proposed against.
    #[test]
    fn the_new_note_is_embedded_first_then_the_backlog() {
        let conn = db::open_in_memory().unwrap();
        let old = note(&conn, "old", "2024-01-01T00:00:00Z");
        let fresh = note(&conn, "fresh", "2024-06-01T00:00:00Z");

        embed_now(&conn, &FakeEmbedder::new("m"), &fresh).unwrap();
        assert!(db::vectors::get(&conn, &fresh).unwrap().is_some());
        assert!(
            db::vectors::get(&conn, &old).unwrap().is_some(),
            "the backlog should have caught up in the same pass"
        );
    }

    #[test]
    fn a_note_already_embedded_is_not_embedded_again() {
        let conn = db::open_in_memory().unwrap();
        let done = note(&conn, "done", "2024-01-01T00:00:00Z");
        let todo = note(&conn, "todo", "2024-01-02T00:00:00Z");
        db::vectors::set(&conn, &done, "m", &[1.0, 0.0]).unwrap();

        assert_eq!(backfill(&conn, &FakeEmbedder::new("m"), 10).unwrap(), 1);
        // Untouched: the original vector, not the fake's.
        let (_, v) = db::vectors::get(&conn, &done).unwrap().unwrap();
        assert!((v[0] - 1.0).abs() < 1e-6, "an embedded note was redone");
        assert!(db::vectors::get(&conn, &todo).unwrap().is_some());
    }

    /// Switching model makes every existing vector unusable -- they share no
    /// space -- so they must come back through the backfill rather than sit
    /// there looking done.
    #[test]
    fn changing_the_model_re_embeds_the_corpus() {
        let conn = db::open_in_memory().unwrap();
        let a = note(&conn, "a", "2024-01-01T00:00:00Z");
        db::vectors::set(&conn, &a, "old-model", &[1.0, 0.0]).unwrap();

        assert_eq!(
            backfill(&conn, &FakeEmbedder::new("new-model"), 10).unwrap(),
            1
        );
        let (model, _) = db::vectors::get(&conn, &a).unwrap().unwrap();
        assert_eq!(model, "new-model");
    }

    /// It runs behind a capture, so a large corpus must not hold up the note
    /// someone just spoke. The next pass picks up where this stopped.
    #[test]
    fn the_backfill_is_capped_and_resumes_from_the_oldest() {
        let conn = db::open_in_memory().unwrap();
        let first = note(&conn, "first", "2024-01-01T00:00:00Z");
        let second = note(&conn, "second", "2024-01-02T00:00:00Z");
        let third = note(&conn, "third", "2024-01-03T00:00:00Z");

        assert_eq!(backfill(&conn, &FakeEmbedder::new("m"), 2).unwrap(), 2);
        assert!(db::vectors::get(&conn, &first).unwrap().is_some());
        assert!(db::vectors::get(&conn, &second).unwrap().is_some());
        assert!(db::vectors::get(&conn, &third).unwrap().is_none());

        assert_eq!(backfill(&conn, &FakeEmbedder::new("m"), 2).unwrap(), 1);
        assert!(db::vectors::get(&conn, &third).unwrap().is_some());
    }

    /// Found in the packaged app: an upload restores notes but no vectors, and
    /// after replacing a corpus with its own archive `ask` answered "No relevant
    /// notes found" -- 16 notes, 0 vectors -- because the only thing refilling
    /// them was the capture backfill, five notes per recording.
    #[test]
    fn an_upload_is_embedded_in_full_not_five_at_a_time() {
        let conn = db::open_in_memory().unwrap();
        let ids: Vec<String> = (0..12)
            .map(|i| {
                note(
                    &conn,
                    &format!("note {i}"),
                    &format!("2024-01-{:02}T00:00:00Z", i + 1),
                )
            })
            .collect();
        let db = std::sync::Mutex::new(conn);

        assert_eq!(catch_up(&db, &FakeEmbedder::new("m")).unwrap(), 12);
        let conn = db.lock().unwrap();
        for id in &ids {
            assert!(
                db::vectors::get(&conn, id).unwrap().is_some(),
                "{id} was left out"
            );
        }
    }

    /// A refused note stays missing, so a loop that asks for "whatever is still
    /// missing" would ask for it forever.
    #[test]
    fn a_refused_note_neither_stops_the_catch_up_nor_loops_it() {
        let conn = db::open_in_memory().unwrap();
        let bad = note(&conn, "bad", "2024-01-01T00:00:00Z");
        for i in 0..40 {
            note(
                &conn,
                &format!("good {i}"),
                &format!("2024-02-01T00:00:{i:02}Z"),
            );
        }
        let db = std::sync::Mutex::new(conn);

        let e = FakeEmbedder::refusing("m", &["bad"]);
        assert_eq!(catch_up(&db, &e).unwrap(), 40);
        assert!(db::vectors::get(&db.lock().unwrap(), &bad)
            .unwrap()
            .is_none());
    }

    /// One note the model chokes on must not cost the rest of the corpus.
    #[test]
    fn one_refusal_does_not_stop_the_pass() {
        let conn = db::open_in_memory().unwrap();
        let bad = note(&conn, "bad", "2024-01-01T00:00:00Z");
        let good = note(&conn, "good", "2024-01-02T00:00:00Z");

        let e = FakeEmbedder::refusing("m", &["bad"]);
        assert_eq!(backfill(&conn, &e, 10).unwrap(), 1);
        assert!(db::vectors::get(&conn, &bad).unwrap().is_none());
        assert!(db::vectors::get(&conn, &good).unwrap().is_some());
    }
}
