//! The whole thing, against a real database on disk.
//!
//! The unit tests each cover one seam. This walks the path the app walks:
//! open a corpus, load the sample, record something, correct it, search for
//! it, and reopen everything from disk to prove it was actually written.

use parallax_lib::{commands::capture, db, state::AppState};

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

/// The schema cascades the audio metadata; the recording is a file on disk and
/// nothing was removing it.
#[test]
fn deleting_an_entry_removes_its_recording() {
    let (state, root) = corpus();

    let made = {
        let conn = state.db();
        db::create::create(
            &conn,
            db::create::NewEntry {
                transcript: "Something said out loud.".into(),
                duration_ms: 4_000,
                fingerprint: vec![0.1, 0.2, 0.3],
                parent_entry_id: None,
                local_only: None,
                typed: false,
            },
        )
        .unwrap()
    };

    // `create` records the path; the capture command is what writes the bytes.
    let wav = root.join(made.audio_path.clone().unwrap());
    std::fs::create_dir_all(wav.parent().unwrap()).unwrap();
    std::fs::write(&wav, b"pretend wav").unwrap();

    state.delete_entry(&made.id).unwrap();

    let conn = state.db();
    assert!(db::entries::list(&conn).unwrap().is_empty());
    assert!(!wav.exists(), "the recording outlived its entry");
}

#[test]
fn deleting_a_typed_entry_is_not_an_error() {
    let (state, _root) = corpus();

    let made = {
        let conn = state.db();
        db::create::create(
            &conn,
            db::create::NewEntry {
                transcript: "Typed, so there is no audio at all.".into(),
                duration_ms: 0,
                fingerprint: vec![],
                parent_entry_id: None,
                local_only: None,
                typed: true,
            },
        )
        .unwrap()
    };
    assert!(made.audio_path.is_none());

    state.delete_entry(&made.id).unwrap();
    let conn = state.db();
    assert!(db::entries::list(&conn).unwrap().is_empty());
}

/// Children are orphaned rather than deleted, so their recordings have to stay.
#[test]
fn deleting_a_parent_keeps_its_children_and_their_recordings() {
    let (state, root) = corpus();

    let (parent, child) = {
        let conn = state.db();
        let parent = db::create::create(
            &conn,
            db::create::NewEntry {
                transcript: "The question that started it.".into(),
                duration_ms: 5_000,
                fingerprint: vec![0.2, 0.4],
                parent_entry_id: None,
                local_only: None,
                typed: false,
            },
        )
        .unwrap();
        let child = db::create::create(
            &conn,
            db::create::NewEntry {
                transcript: "The answer, which is still something I said.".into(),
                duration_ms: 6_000,
                fingerprint: vec![0.5, 0.6],
                parent_entry_id: Some(parent.id.clone()),
                local_only: None,
                typed: false,
            },
        )
        .unwrap();
        (parent, child)
    };

    let mut written = Vec::new();
    for entry in [&parent, &child] {
        let wav = root.join(entry.audio_path.clone().unwrap());
        std::fs::create_dir_all(wav.parent().unwrap()).unwrap();
        std::fs::write(&wav, b"pretend wav").unwrap();
        written.push(wav);
    }

    state.delete_entry(&parent.id).unwrap();

    let conn = state.db();
    let left = db::entries::list(&conn).unwrap();
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].id, child.id);
    assert!(
        !written[0].exists(),
        "the parent's recording should be gone"
    );
    assert!(written[1].exists(), "the child's recording should survive");
}

fn wavs_in(state: &AppState) -> Vec<std::path::PathBuf> {
    std::fs::read_dir(state.audio_dir())
        .map(|rd| {
            rd.filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.extension().is_some_and(|x| x == "wav"))
                .collect()
        })
        .unwrap_or_default()
}

/// The recording is the record. A model that cannot load costs the transcript,
/// never the audio.
#[test]
fn a_failing_transcription_keeps_the_recording() {
    let (state, _root) = corpus();

    // Present, so it is selected, and invalid, so loading it fails.
    std::fs::create_dir_all(state.models_dir()).unwrap();
    for name in ["whisper-tiny", "whisper-base", "whisper-small"] {
        std::fs::write(
            state.models_dir().join(format!("{name}.gguf")),
            b"not a model",
        )
        .unwrap();
    }

    let pcm = vec![0.1f32; 16_000];
    let failed = capture::finish(&state, pcm, 1_000, None, None);
    assert!(failed.is_err(), "an invalid model should fail the capture");

    assert_eq!(wavs_in(&state).len(), 1, "the recording was not written");
    assert!(
        state
            .discarded
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .is_some(),
        "the staged copy was consumed by a failure"
    );
    let conn = state.db();
    assert!(db::entries::list(&conn).unwrap().is_empty());
}

/// Settings are read after staging, so an unreadable settings table cannot cost
/// the samples.
#[test]
fn a_settings_failure_keeps_the_staged_samples() {
    let (state, _root) = corpus();
    {
        let conn = state.db();
        conn.execute("DROP TABLE settings", []).unwrap();
    }

    let failed = capture::finish(&state, vec![0.2f32; 8_000], 500, None, None);
    assert!(failed.is_err());
    assert!(
        state
            .discarded
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .is_some(),
        "the samples were dropped before anything could fail"
    );
}

/// Against the binary the installer ships, when it has been fetched. Skipped
/// rather than failed when absent: scripts/fetch-llama.ps1 is not a build step.
#[test]
fn the_bundled_llama_server_reports_its_devices() {
    let bundled = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("binaries/llama");
    let Some(binary) = parallax_lib::llm::binary::resolve(None, Some(&bundled)) else {
        eprintln!("skipped: no bundled llama-server");
        return;
    };

    let found = parallax_lib::llm::binary::devices(&binary);
    assert!(!found.is_empty(), "a Vulkan build should report something");
    let best = parallax_lib::llm::binary::best_device(&found).unwrap();
    eprintln!("devices: {found:?}\nchose: {} ({})", best.id, best.name);
    assert!(best.free_mib > 0);
}

/// The real URL, the real file, the real loader. Ignored because it pulls ~44MB:
/// run with `cargo test --test end_to_end -- --ignored --nocapture`.
#[test]
#[ignore = "downloads a model over the network"]
fn a_real_whisper_model_downloads_and_transcribes() {
    use parallax_lib::model::download;

    let (state, _root) = corpus();
    let url = "https://huggingface.co/handy-computer/whisper-tiny-gguf/resolve/main/whisper-tiny-Q4_K_M.gguf";
    let dest = state.models_dir().join("whisper-tiny.gguf");

    let started = std::time::Instant::now();
    let mut last = 0u64;
    download::fetch(url, &dest, &mut |got, total| {
        if got == total {
            last = total;
        }
    })
    .unwrap();
    let bytes = std::fs::metadata(&dest).unwrap().len();
    eprintln!("downloaded {bytes} bytes in {:?}", started.elapsed());
    assert_eq!(
        bytes, last,
        "progress total disagreed with the file on disk"
    );

    // The catalogue says 43.6MB; the 90% rule in list_models depends on it.
    assert!(
        (41_000_000..46_000_000).contains(&bytes),
        "unexpected size {bytes}"
    );

    // Found by the same lookup the capture path uses.
    let found = state
        .transcription_model(parallax_lib::model::TranscriptionModel::Tiny)
        .expect("the downloaded model should be discoverable");
    assert_eq!(found, dest);

    // A second of quiet: proves the loader accepts the file, not that it hears.
    let loaded = std::time::Instant::now();
    let out = parallax_lib::stt::transcribe(
        &found,
        &vec![0.0f32; 16_000],
        parallax_lib::model::ComputeBackend::Cpu,
    )
    .unwrap();
    eprintln!(
        "loaded and ran in {:?}, text: {:?}",
        loaded.elapsed(),
        out.text
    );
}
