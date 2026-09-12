//! Process-wide state, and where the corpus lives on disk.

use crate::audio::recorder::Recording;
use crate::db;
use crate::error::Result;
use crate::llm::LlmProvider;
use crate::model::TranscriptionModel;
use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// A recording thrown away but not yet gone. §4 keeps it for a minute rather
/// than asking "are you sure", because a confirmation dialog on every discard
/// is worse than an undo nobody uses.
pub struct Discarded {
    pub pcm: Vec<f32>,
    pub duration_ms: i64,
    pub at: std::time::Instant,
}

/// A take and the window it belongs to.
///
/// The owner rides with the recording rather than in a field beside it, so the
/// two cannot disagree: a stale owner would route the stop to a window that
/// is not recording, which is the bug this exists to fix.
pub struct InFlight {
    pub take: Recording,
    pub owner: String,
}

pub struct AppState {
    /// One connection behind a lock. SQLite serialises writes anyway, and the
    /// commands are short; a pool would buy nothing at this scale.
    pub conn: Mutex<Connection>,
    /// Kept so the app can say where its data is rather than making the user
    /// guess, and so the audio directory hangs off the same root.
    pub root: PathBuf,
    /// At most one recording at a time -- there is one microphone and one
    /// hotkey, and a second concurrent take has no meaning.
    pub recording: Mutex<Option<InFlight>>,
    pub discarded: Mutex<Option<Discarded>>,
    /// Where the bundled llama-server sits. `None` in a dev build that has not
    /// fetched it; an explicit setting still overrides either way.
    pub llama_dir: Mutex<Option<PathBuf>>,
    /// Started on first use and kept, because loading 2.5GB per question is
    /// the difference between the question existing and not.
    pub llama: Mutex<Option<crate::llm::llama_server::LlamaServer>>,
}

impl AppState {
    pub fn open(root: PathBuf) -> Result<Self> {
        let conn = db::open(&root.join("corpus.db"))?;
        std::fs::create_dir_all(root.join("audio"))?;
        Ok(Self {
            conn: Mutex::new(conn),
            root,
            recording: Mutex::new(None),
            discarded: Mutex::new(None),
            llama_dir: Mutex::new(None),
            llama: Mutex::new(None),
        })
    }

    /// A panic inside one command poisons the mutex, and every later command
    /// would then panic on a lock it could otherwise have used. The connection
    /// itself is not left inconsistent -- SQLite rolls back an incomplete
    /// statement -- so recovering is better than bricking the session.
    pub fn db(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// The window a take belongs to, or `None` when nothing is recording.
    pub fn recording_owner(&self) -> Option<String> {
        self.recording
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
            .map(|in_flight| in_flight.owner.clone())
    }

    pub fn audio_dir(&self) -> PathBuf {
        self.root.join("audio")
    }

    /// Matches --fit's own floor, so the fit never has to shrink it.
    pub const REASONING_CONTEXT: u32 = 4096;

    /// The reasoning model onboarding chose, if its file is there.
    pub fn reasoning_model(&self, model_id: Option<&str>) -> Option<PathBuf> {
        let path = self.models_dir().join(format!("{}.gguf", model_id?));
        path.is_file().then_some(path)
    }

    /// The embedding model, if one was chosen and its file is there.
    ///
    /// `None` is an ordinary state, not an error: topics propose candidates on
    /// their own and cosine only reorders them, so an absent embedder costs
    /// ranking quality and nothing else.
    pub fn embedding_model(&self, model_id: Option<&str>) -> Option<PathBuf> {
        let path = self.models_dir().join(format!("{}.gguf", model_id?));
        path.is_file().then_some(path)
    }

    /// The llama-server binary, if there is one to find.
    pub fn llama_binary(&self) -> Option<PathBuf> {
        let explicit = db::settings::get(&self.db())
            .ok()
            .and_then(|s| s.llama_server_path)
            .map(PathBuf::from);
        let bundled = self
            .llama_dir
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        crate::llm::binary::resolve(explicit.as_deref(), bundled.as_deref())
            .or_else(crate::llm::binary::on_path)
    }

    /// Starts the reasoning server if it is not already up, and runs `f` against
    /// it. Returns `Ok(None)` when there is no binary or no model: enrichment is
    /// allowed to be absent (§9.4), and absent is not an error.
    pub fn with_reasoning<T>(
        &self,
        f: impl FnOnce(&dyn crate::llm::LlmProvider) -> Result<T>,
    ) -> Result<Option<T>> {
        let Some(binary) = self.llama_binary() else {
            return Ok(None);
        };
        let settings = db::settings::get(&self.db())?;
        let Some(model) = self.reasoning_model(settings.model_id.as_deref()) else {
            return Ok(None);
        };

        let mut slot = self.llama.lock().unwrap_or_else(|p| p.into_inner());
        if slot.as_ref().is_none_or(|s| !s.ready()) {
            let devices = crate::llm::binary::devices(&binary);
            let device = crate::llm::binary::best_device(&devices).map(|d| d.id.clone());
            *slot = Some(crate::llm::llama_server::LlamaServer::spawn(
                &binary,
                &model,
                settings.reasoning_backend,
                device.as_deref(),
                Self::REASONING_CONTEXT,
            )?);
        }
        let server = slot.as_ref().expect("just started");
        f(server).map(Some)
    }

    /// Unlinks a recording, and only ever one inside the corpus.
    ///
    /// A stored path is data; an imported one is data from a stranger. Reads
    /// have been checked since the start, and deleting on an unchecked path is
    /// strictly worse -- a crafted export naming `audio/../../something` had
    /// replace delete it. Silent on refusal: a path that does not resolve into
    /// the corpus has no file of ours behind it to report on.
    pub fn remove_audio(&self, relative: &str) {
        let (Ok(full), Ok(audio_dir)) = (
            self.root.join(relative).canonicalize(),
            self.audio_dir().canonicalize(),
        ) else {
            return;
        };
        if full.starts_with(&audio_dir) {
            let _ = std::fs::remove_file(full);
        }
    }

    /// The row goes first: a failed unlink orphans a file, while the reverse
    /// destroys the recording of an entry that still exists.
    pub fn delete_entry(&self, id: &str) -> Result<()> {
        let audio = {
            let conn = self.db();
            let path = db::entries::audio_path(&conn, id)?;
            db::entries::delete(&conn, id)?;
            path
        };
        if let Some(relative) = audio {
            self.remove_audio(&relative);
        }
        Ok(())
    }

    /// Everything that outlives a destructor. Called from the exit handler
    /// rather than `Drop`, because Tauri exits through `std::process::exit`.
    pub fn shutdown(&self) {
        // Explicit, because Tauri exits through std::process::exit and Drop
        // never runs -- which is how a llama-server child got orphaned before.
        if let Some(server) = self.llama.lock().unwrap_or_else(|p| p.into_inner()).take() {
            server.stop();
        }
        // A recording in flight is dropped, which stops the stream. Nothing is
        // written: an app being quit mid-sentence did not ask for a note.
        self.recording
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take();
    }

    pub fn models_dir(&self) -> PathBuf {
        self.root.join("models")
    }

    /// The transcription model the user chose, if it is actually there.
    ///
    /// Named rather than enumerated: with tiny and base both present, taking
    /// whichever the filesystem yields first would silently ignore the setting.
    /// `None` is a normal state, not an error -- capture works without it, and
    /// the audio is the record the transcript is derived from.
    pub fn transcription_model(&self, chosen: TranscriptionModel) -> Option<PathBuf> {
        let name = match chosen {
            TranscriptionModel::Tiny => "whisper-tiny",
            TranscriptionModel::Base => "whisper-base",
            TranscriptionModel::Small => "whisper-small",
        };
        let path = self.models_dir().join(format!("{name}.gguf"));
        path.is_file().then_some(path)
    }
}

/// A `data` directory beside the executable wins if it exists, which is the
/// whole portable-install mechanism: make the folder and the corpus follows
/// the binary onto a stick. Otherwise the platform's app-data directory, which
/// is where an installed app belongs and survives an upgrade.
///
/// Discoverable on purpose. A tool holding your private thinking should not
/// hide where it keeps it.
pub fn resolve_root(app_data: &Path) -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let portable = dir.join("data");
            if portable.is_dir() {
                return portable;
            }
        }
    }
    app_data.to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_data_is_the_default() {
        let app_data = Path::new("C:/app-data/parallax");
        assert_eq!(resolve_root(app_data), app_data.to_path_buf());
    }

    /// An import supplies its own audio paths, so a crafted export must not be
    /// able to name a file outside the corpus and have replace delete it. The
    /// read path has been checked since the start; the unlink was not.
    #[test]
    fn a_crafted_audio_path_is_not_deleted() {
        let dir = std::env::temp_dir().join(format!("parallax-unlink-{}", uuid::Uuid::new_v4()));
        let state = AppState::open(dir.clone()).unwrap();

        let outside = dir
            .parent()
            .unwrap()
            .join(format!("hostage-{}.wav", uuid::Uuid::new_v4()));
        std::fs::write(&outside, b"not yours").unwrap();
        let escape = format!(
            "audio/../../{}",
            outside.file_name().unwrap().to_string_lossy()
        );

        state.remove_audio(&escape);
        assert!(outside.is_file(), "an unlink escaped the corpus");

        let inside = state.audio_dir().join("e1.wav");
        std::fs::write(&inside, b"mine").unwrap();
        state.remove_audio("audio/e1.wav");
        assert!(
            !inside.exists(),
            "a recording inside the corpus must still go"
        );

        let _ = std::fs::remove_file(outside);
        drop(state);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn opening_creates_the_corpus_and_the_audio_directory() {
        let dir = std::env::temp_dir().join(format!("parallax-test-{}", uuid::Uuid::new_v4()));
        let state = AppState::open(dir.clone()).unwrap();

        assert!(dir.join("corpus.db").is_file());
        assert!(state.audio_dir().is_dir());

        drop(state);
        let _ = std::fs::remove_dir_all(dir);
    }
}
