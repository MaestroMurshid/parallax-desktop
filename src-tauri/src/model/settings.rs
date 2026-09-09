//! Settings, system profile, and the model catalogue.

use serde::{Deserialize, Serialize};

/// §9.4 -- the residency fork. Only the post-recording question is
/// latency-sensitive; everything else batches and can run cold.
///
/// Day-0 measurement: 4B Q4 answers in 1.8s on the GPU and 9.0s on CPU, so
/// `Warm` is only meaningful with GPU offload.
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub hotkey: String,
    pub discard_hotkey: String,
    /// `None` is the "not yet onboarded" sentinel the frontend keys off.
    pub model_id: Option<String>,
    pub residency: Residency,
    pub provider_name: String,
    pub default_local_only: bool,
    pub transcription_model: TranscriptionModel,
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

/// The one non-obvious serde shape in the whole contract: an internally
/// tagged union, discriminated on `kind`, with kebab-case tags.
///
/// TypeScript:
///   | { kind: 'not-downloaded' }
///   | { kind: 'downloading'; receivedBytes: number; totalBytes: number }
///   | { kind: 'ready' }
///   | { kind: 'failed'; error: string }
///
/// Java analogue: a sealed interface with @JsonTypeInfo(property = "kind").
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
    /// Drives the onboarding default: the largest model this machine can hold.
    pub recommended_ram_bytes: u64,
    pub state: ModelState,
}
