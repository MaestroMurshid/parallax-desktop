//! Settings, system profile, and the model catalogue.

use serde::{Deserialize, Serialize};

/// §9.4. Only the post-recording question is latency-sensitive; everything
/// else batches. Measured: 4B Q4 answers in 1.8s on GPU, 9.0s on CPU, so
/// `Warm` is only meaningful with offload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Residency {
    Warm,
    Cold,
}

impl Residency {
    /// How long the reasoning model is kept loaded after it was last used.
    ///
    /// Neither keeps it forever. Measured on the RTX 3050: a loaded model holds
    /// the card powered on at about 6W for as long as it stays, while an unused
    /// card powers off entirely -- the difference between a laptop that lasts
    /// the afternoon and one that does not. A cold start costs about 4s, so a
    /// release is cheap to undo.
    pub fn keep_for(self) -> std::time::Duration {
        match self {
            // A session: notes recorded minutes apart each get their question
            // in about two seconds.
            Residency::Warm => std::time::Duration::from_secs(10 * 60),
            // Long enough to outlast one capture, whose classify, question and
            // connection judge run back to back; short enough to give the card
            // back as soon as that is done.
            Residency::Cold => std::time::Duration::from_secs(30),
        }
    }
}

#[cfg(test)]
mod residency_tests {
    use super::*;
    use std::time::Duration;

    /// Warm is for a session: notes recorded minutes apart each get their
    /// question in about two seconds rather than paying a load first.
    #[test]
    fn warm_outlasts_a_recording_session() {
        assert!(Residency::Warm.keep_for() >= Duration::from_secs(5 * 60));
    }

    /// Cold gives the card back almost at once, but not inside one capture:
    /// classify, question and the connection judge run back to back, and
    /// releasing between them would reload the model for each.
    #[test]
    fn cold_releases_soon_but_not_mid_capture() {
        let keep = Residency::Cold.keep_for();
        assert!(keep <= Duration::from_secs(60), "{keep:?}");
        assert!(keep >= Duration::from_secs(15), "{keep:?}");
    }

    #[test]
    fn warm_keeps_longer_than_cold() {
        assert!(Residency::Warm.keep_for() > Residency::Cold.keep_for());
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TranscriptionModel {
    Tiny,
    Base,
    Small,
}

impl TranscriptionModel {
    /// The catalogue id, which is also the file's name on disk.
    pub fn model_id(self) -> &'static str {
        match self {
            TranscriptionModel::Tiny => "whisper-tiny",
            TranscriptionModel::Base => "whisper-base",
            TranscriptionModel::Small => "whisper-small",
        }
    }
}

/// What a model runs on.
///
/// `Auto` resolves against the machine; `Gpu` forces acceleration but leaves
/// the backend to whatever is present. The named backends are an explicit
/// override -- a broken driver, a second card, or isolating a backend bug --
/// and fail rather than silently falling back, so a forced choice stays forced.
/// The window's colour scheme. `System` is the default and the only one that
/// can change without the app being told -- it tracks `prefers-color-scheme`
/// in CSS. Stored so the capture panel, a second webview with its own JS
/// runtime, starts on the same choice as the main window instead of quietly
/// defaulting on its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ComputeBackend {
    Auto,
    Cpu,
    Gpu,
    Cuda,
    Vulkan,
    Metal,
    Rocm,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub hotkey: String,
    pub discard_hotkey: String,
    /// `None` is the "not yet onboarded" sentinel.
    pub model_id: Option<String>,
    pub residency: Residency,
    pub provider_name: String,
    pub default_local_only: bool,
    pub transcription_model: TranscriptionModel,
    /// `None` until chosen. Connections still work without it -- topics alone
    /// propose -- so this is an upgrade to the ordering, never a dependency.
    #[serde(default)]
    pub embedding_model_id: Option<String>,

    /// Under `Auto` the reasoning model has first claim on VRAM: it is the one
    /// a person is waiting on, while transcription runs at ~35x realtime on CPU.
    pub transcription_backend: ComputeBackend,
    pub reasoning_backend: ComputeBackend,
    /// Overrides the bundled llama-server. Set by someone who wants their own
    /// build -- a CUDA one, most likely; absent for everyone else.
    #[serde(default)]
    pub llama_server_path: Option<String>,

    /// Whether the live register is consulted at all.
    ///
    /// On by default, because §3.2's asymmetry is the safe direction. Off is
    /// for the case that made this a setting: a note arguing an impersonal
    /// point *through* a personal example reads as live to the classifier --
    /// the example is about the speaker's life -- so the note that most wanted
    /// a question is the one that silently never gets one. Turning it off
    /// suppresses nothing and rewrites nothing; it stops the facet being read.
    #[serde(default = "yes")]
    pub live_register: bool,

    /// Which webview reads this back matters: the panel is a second window
    /// with its own store, so this is what lets it open on the theme the main
    /// window is already showing rather than always starting light.
    #[serde(default)]
    pub theme: Theme,
}

fn yes() -> bool {
    true
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            hotkey: "Ctrl+Shift+Space".into(),
            discard_hotkey: "Ctrl+Shift+Backspace".into(),
            model_id: None,
            residency: Residency::Warm,
            provider_name: "llama-server".into(),
            default_local_only: false,
            transcription_model: TranscriptionModel::Base,
            embedding_model_id: None,
            transcription_backend: ComputeBackend::Auto,
            reasoning_backend: ComputeBackend::Auto,
            llama_server_path: None,
            live_register: true,
            theme: Theme::System,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemProfile {
    pub total_ram_bytes: u64,
    pub available_ram_bytes: u64,
    pub cpu_name: String,
    pub cpu_cores: u32,
    pub gpu_name: Option<String>,
    pub vram_bytes: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelKind {
    Transcription,
    Reasoning,
    /// Sentence vectors, which are what let two notes about one idea meet
    /// without having chosen the same words. Local unconditionally, tiny, and
    /// on the CPU -- it never competes with reasoning for the card.
    Embedding,
}

/// Internally tagged on `kind`, kebab-case — the one non-obvious
/// serde shape in the contract.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum ModelState {
    NotDownloaded,
    Downloading {
        received_bytes: u64,
        total_bytes: u64,
    },
    Ready,
    Failed {
        error: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub id: String,
    pub kind: ModelKind,
    pub name: String,
    pub params: String,
    pub quantization: String,
    pub size_bytes: u64,
    /// Drives the onboarding default.
    pub recommended_ram_bytes: u64,
    pub state: ModelState,
    /// Where the file comes from. Served to the frontend so the user can see
    /// what the app is about to fetch before it fetches it.
    pub url: String,
}
