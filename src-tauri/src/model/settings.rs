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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TranscriptionModel {
    Tiny,
    Base,
    Small,
}

/// What a model runs on.
///
/// `Auto` resolves against the machine; `Gpu` forces acceleration but leaves
/// the backend to whatever is present. The named backends are an explicit
/// override -- a broken driver, a second card, or isolating a backend bug --
/// and fail rather than silently falling back, so a forced choice stays forced.
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

    /// Under `Auto` the reasoning model has first claim on VRAM: it is the one
    /// a person is waiting on, while transcription runs at ~35x realtime on CPU.
    pub transcription_backend: ComputeBackend,
    pub reasoning_backend: ComputeBackend,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            hotkey: "Ctrl+Shift+Space".into(),
            discard_hotkey: "Escape".into(),
            model_id: None,
            residency: Residency::Warm,
            provider_name: "llama-server".into(),
            default_local_only: false,
            transcription_model: TranscriptionModel::Base,
            transcription_backend: ComputeBackend::Auto,
            reasoning_backend: ComputeBackend::Auto,
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
}

/// Internally tagged on `kind`, kebab-case — the one non-obvious
/// serde shape in the contract.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", rename_all_fields = "camelCase")]
pub enum ModelState {
    NotDownloaded,
    Downloading { received_bytes: u64, total_bytes: u64 },
    Ready,
    Failed { error: String },
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
}
