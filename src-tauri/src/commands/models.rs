//! What this machine is, and which models it has.
//!
//! §9.4 -- downloading gates nothing. Capture and transcription work without a
//! reasoning model, and the question simply arrives when one lands.

use crate::error::Result;
use crate::model::{ModelInfo, ModelKind, ModelState, SystemProfile};
use crate::state::AppState;
use tauri::State;

/// Drives the onboarding default: the largest model this machine can hold.
#[tauri::command]
pub fn get_system_profile() -> SystemProfile {
    use sysinfo::System;

    let mut system = System::new_all();
    system.refresh_memory();
    system.refresh_cpu_all();

    let cpu = system.cpus().first();

    SystemProfile {
        total_ram_bytes: system.total_memory(),
        available_ram_bytes: system.available_memory(),
        cpu_name: cpu
            .map(|c| c.brand().trim().to_string())
            .filter(|b| !b.is_empty())
            .unwrap_or_else(|| "unknown".into()),
        cpu_cores: system.cpus().len() as u32,
        // Not read from the system yet. Reporting a card without knowing its
        // VRAM would make the onboarding default worse than admitting nothing:
        // the recommendation is sized against memory it cannot see.
        gpu_name: None,
        vram_bytes: None,
    }
}

/// The catalogue. Sizes are the real download sizes, and the RAM figures are
/// what each is worth running at rather than the minimum it will load in.
///
/// Measured: Qwen3-4B is the floor for classification. At 1.7B the JSON is
/// always valid and the content is not -- wrong register, a summary that
/// misstates the note, and the move phrase copied out of the prompt.
fn catalogue() -> Vec<ModelInfo> {
    vec![
        model(
            "whisper-tiny",
            ModelKind::Transcription,
            "tiny",
            "39M",
            "q5_1",
            75_000_000,
            2,
        ),
        model(
            "whisper-base",
            ModelKind::Transcription,
            "base",
            "74M",
            "q5_1",
            140_000_000,
            4,
        ),
        model(
            "whisper-small",
            ModelKind::Transcription,
            "small",
            "244M",
            "q5_1",
            470_000_000,
            8,
        ),
        model(
            "qwen3-1.7b-q4",
            ModelKind::Reasoning,
            "Qwen3 1.7B",
            "1.7B",
            "Q4_K_M",
            1_050_000_000,
            8,
        ),
        model(
            "qwen3-4b-q4",
            ModelKind::Reasoning,
            "Qwen3 4B",
            "4B",
            "Q4_K_M",
            2_400_000_000,
            16,
        ),
        model(
            "qwen3-8b-q4",
            ModelKind::Reasoning,
            "Qwen3 8B",
            "8B",
            "Q4_K_M",
            4_700_000_000,
            32,
        ),
    ]
}

fn model(
    id: &str,
    kind: ModelKind,
    name: &str,
    params: &str,
    quantization: &str,
    size_bytes: u64,
    recommended_gb: u64,
) -> ModelInfo {
    ModelInfo {
        id: id.into(),
        kind,
        name: name.into(),
        params: params.into(),
        quantization: quantization.into(),
        size_bytes,
        recommended_ram_bytes: recommended_gb * 1_000_000_000,
        state: ModelState::NotDownloaded,
    }
}

/// State comes from the disk rather than a record of what was asked for: a
/// half-finished download that was interrupted is not a model, and a file
/// copied in by hand is.
#[tauri::command]
pub fn list_models(state: State<AppState>) -> Vec<ModelInfo> {
    let dir = state.models_dir();
    catalogue()
        .into_iter()
        .map(|mut info| {
            let path = dir.join(format!("{}.gguf", info.id));
            if let Ok(meta) = std::fs::metadata(&path) {
                // Within a tenth of the expected size. A truncated download
                // loads as a corrupt model, which reads as the app being
                // broken rather than the file being incomplete.
                let expected = info.size_bytes as f64;
                if (meta.len() as f64) >= expected * 0.9 {
                    info.state = ModelState::Ready;
                } else {
                    info.state = ModelState::Downloading {
                        received_bytes: meta.len(),
                        total_bytes: info.size_bytes,
                    };
                }
            }
            info
        })
        .collect()
}

#[tauri::command]
pub fn models_location(state: State<AppState>) -> Result<String> {
    let dir = state.models_dir();
    std::fs::create_dir_all(&dir)?;
    Ok(dir.display().to_string())
}
