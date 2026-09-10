//! The whole thing, against a real database on disk.
//!
//! The unit tests each cover one seam. This walks the path the app walks:
//! open a corpus, load the sample, record something, correct it, search for
//! it, and reopen everything from disk to prove it was actually written.

use parallax_lib::{db, state::AppState};

fn corpus() -> (AppState, std::path::PathBuf) {
    let root = std::env::temp_dir().join(format!("parallax-e2e-{}", uuid::Uuid::new_v4()));
    let state = AppState::open(root.clone()).unwrap();
    (state, root)
}

#[test]
fn a_corpus_survives_being_closed_and_reopened() {
    let (state, root) = corpus();

    let made = {
        let conn = state.db();
        db::create::create(
            &conn,
            db::create::NewEntry {
                transcript: "Indexes trade write performance for faster reads.".into(),
                duration_ms: 19_000,
                fingerprint: vec![0.3, 0.8, 0.5],
                parent_entry_id: None,
                local_only: None,
                typed: false,
            },
        )
        .unwrap()
    };
    drop(state);

    // A new AppState over the same directory, as a restart would be.
    let reopened = AppState::open(root.clone()).unwrap();
    let conn = reopened.db();
    let all = db::entries::list(&conn).unwrap();

    assert_eq!(all.len(), 1);
    assert_eq!(all[0].id, made.id);
    assert_eq!(all[0].transcript, made.transcript);
    assert_eq!((all[0].x, all[0].y), (made.x, made.y), "position is frozen");
    assert_eq!(all[0].fingerprint, vec![0.3, 0.8, 0.5]);

    drop(conn);
    drop(reopened);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn the_sample_corpus_loads_and_is_searchable() {
    let (state, root) = corpus();
    {
        let conn = state.db();
        db::sample::load(&conn).unwrap();

        let entries = db::entries::list(&conn).unwrap();
        assert!(entries.len() >= 10, "got {} entries", entries.len());
        assert!(!db::edges::list(&conn).unwrap().is_empty(), "edges too");

        // Something from the fixture, found by a phrase half-remembered.
        let hits = db::search::search(&conn, "reasoning").unwrap();
        assert!(!hits.is_empty(), "search found nothing in the sample");
        for hit in &hits {
            let entry = db::entries::get(&conn, &hit.entry_id).unwrap().unwrap();
            assert_eq!(
                &entry.transcript[hit.start as usize..hit.end as usize].to_lowercase(),
                "reasoning",
                "the offsets have to point at the match"
            );
        }
    }
    drop(state);
    let _ = std::fs::remove_dir_all(root);
}

/// New entries land against the sample rather than on top of it.
#[test]
fn recording_into_a_loaded_sample_does_not_collide() {
    let (state, root) = corpus();
    {
        let conn = state.db();
        db::sample::load(&conn).unwrap();

        let before: Vec<(String, f64, f64)> = db::entries::list(&conn)
            .unwrap()
            .into_iter()
            .map(|e| (e.id, e.x, e.y))
            .collect();

        let made = db::create::create(
            &conn,
            db::create::NewEntry {
                transcript: "Something I just said out loud.".into(),
                duration_ms: 31_000,
                fingerprint: vec![0.5],
                parent_entry_id: None,
                local_only: None,
                typed: false,
            },
        )
        .unwrap();

        let after = db::entries::list(&conn).unwrap();
        for (id, x, y) in &before {
            let now = after.iter().find(|e| &e.id == id).unwrap();
            assert_eq!((now.x, now.y), (*x, *y), "{id} moved when it should not");
        }
        for (id, x, y) in &before {
            assert!(
                (made.x - x).abs() > 1.0 || (made.y - y).abs() > 1.0,
                "the new entry landed on {id}"
            );
        }
    }
    drop(state);
    let _ = std::fs::remove_dir_all(root);
}

/// Correcting a transcript must not cost an exchange that actually happened.
#[test]
fn a_correction_keeps_the_question_and_moves_its_anchor() {
    let (state, root) = corpus();
    {
        let conn = state.db();
        let made = db::create::create(
            &conn,
            db::create::NewEntry {
                transcript: "Indexes trade write perfrmance for faster reads.".into(),
                duration_ms: 40_000,
                fingerprint: vec![],
                parent_entry_id: None,
                local_only: None,
                typed: true,
            },
        )
        .unwrap();

        db::questions::insert(
            &conn,
            &parallax_lib::model::Question {
                id: "q1".into(),
                entry_id: made.id.clone(),
                text: "What does that buy?".into(),
                span: Some(parallax_lib::model::Span {
                    start: 8,
                    end: 13,
                    attributed: false,
                }),
                answered: true,
                dismissed: false,
                provider_name: "llama-server".into(),
                created_at: "2024-01-01T00:00:00Z".into(),
            },
            &made.transcript,
        )
        .unwrap();

        db::entries::correct_transcript(
            &conn,
            &made.id,
            "Database indexes trade write performance for faster reads.",
        )
        .unwrap();

        let questions = db::questions::list_for(&conn, &made.id).unwrap();
        assert_eq!(questions.len(), 1, "the question survives");
        assert!(
            questions[0].answered,
            "and so does the fact it was answered"
        );

        let span = questions[0].span.as_ref().expect("its anchor was re-found");
        let corrected = db::entries::get(&conn, &made.id)
            .unwrap()
            .unwrap()
            .transcript;
        assert_eq!(
            &corrected[span.start as usize..span.end as usize],
            "trade",
            "the anchor followed its words rather than keeping its offsets"
        );
    }
    drop(state);
    let _ = std::fs::remove_dir_all(root);
}

/// Settings are the one thing read before anything else exists.
#[test]
fn settings_round_trip_through_a_restart() {
    let (state, root) = corpus();
    {
        let conn = state.db();
        db::settings::merge(&conn, serde_json::json!({ "hotkey": "Ctrl+Alt+K" })).unwrap();
    }
    drop(state);

    let reopened = AppState::open(root.clone()).unwrap();
    {
        let conn = reopened.db();
        let settings = db::settings::get(&conn).unwrap();
        assert_eq!(settings.hotkey, "Ctrl+Alt+K");
        assert_eq!(
            settings.discard_hotkey, "Escape",
            "untouched fields keep their defaults"
        );
    }
    drop(reopened);
    let _ = std::fs::remove_dir_all(root);
}
