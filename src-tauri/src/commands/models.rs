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

    // Not new_all: that enumerates every process on the machine, and this
    // runs on the onboarding screen.
    let mut system = System::new();
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

/// The catalogue. Sizes are the real download sizes; the RAM figure is the
/// least a model is worth recommending at, since onboarding picks the largest
/// one that fits.
///
/// Measured: Qwen3-4B is the floor for classification. At 1.7B the JSON is
/// always valid and the content is not -- wrong register, a summary that
/// misstates the note, and the move phrase copied out of the prompt. So 4B's
/// figure is what it actually needs rather than a round number: 2.4GB of
/// weights plus a KV cache is comfortable in 12GB, and a 16GB machine that
/// reports 14.9 would otherwise be handed the model known not to work.
///
/// Every figure is set against what a machine of that size actually reports,
/// not what is printed on the box. Firmware takes its share before the OS
/// sees anything, so a round number here silently excludes the machine it was
/// chosen for.
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
            6,
        ),
        model(
            "qwen3-4b-q4",
            ModelKind::Reasoning,
            "Qwen3 4B",
            "4B",
            "Q4_K_M",
            2_400_000_000,
            12,
        ),
        model(
            "qwen3-8b-q4",
            ModelKind::Reasoning,
            "Qwen3 8B",
            "8B",
            "Q4_K_M",
            4_700_000_000,
            24,
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

/// A file well under its expected size is a partial download, not a model.
/// Loading a truncated gguf fails in a way that reads as the app being broken
/// rather than the file being incomplete, so it reports as still arriving.
///
/// The tolerance is because published sizes and bytes on disk rarely agree
/// exactly, not because a tenth of a model is acceptable.
const COMPLETE_ENOUGH: f64 = 0.9;

fn state_for(expected_bytes: u64, on_disk: Option<u64>) -> ModelState {
    match on_disk {
        None => ModelState::NotDownloaded,
        Some(bytes) if (bytes as f64) >= expected_bytes as f64 * COMPLETE_ENOUGH => {
            ModelState::Ready
        }
        Some(bytes) => ModelState::Downloading {
            received_bytes: bytes,
            total_bytes: expected_bytes,
        },
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
            info.state = state_for(
                info.size_bytes,
                std::fs::metadata(&path).ok().map(|m| m.len()),
            );
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_model_that_is_not_there_is_not_downloaded() {
        assert!(matches!(state_for(1_000, None), ModelState::NotDownloaded));
    }

    #[test]
    fn a_complete_file_is_ready() {
        assert!(matches!(state_for(1_000, Some(1_000)), ModelState::Ready));
    }

    /// Published sizes and bytes on disk rarely agree exactly, so a small
    /// shortfall is still a model.
    #[test]
    fn a_slightly_smaller_file_is_still_ready() {
        assert!(matches!(state_for(1_000, Some(950)), ModelState::Ready));
    }

    /// The case this exists for: an interrupted download left on disk would
    /// otherwise load as a corrupt model and read as the app being broken.
    #[test]
    fn a_truncated_download_reports_as_still_arriving() {
        match state_for(1_000, Some(400)) {
            ModelState::Downloading {
                received_bytes,
                total_bytes,
            } => {
                assert_eq!(received_bytes, 400);
                assert_eq!(total_bytes, 1_000);
            }
            other => panic!("expected downloading, got {other:?}"),
        }
    }

    #[test]
    fn an_empty_file_is_not_a_model() {
        assert!(matches!(
            state_for(1_000, Some(0)),
            ModelState::Downloading { .. }
        ));
    }

    /// The catalogue drives the onboarding default, so its shape matters:
    /// something to transcribe with and something to reason with.
    #[test]
    fn the_catalogue_offers_both_kinds() {
        let all = catalogue();
        assert!(all.iter().any(|m| m.kind == ModelKind::Transcription));
        assert!(all.iter().any(|m| m.kind == ModelKind::Reasoning));
        assert!(all.iter().all(|m| m.size_bytes > 0));
        assert!(all.iter().all(|m| m.recommended_ram_bytes > m.size_bytes));
    }

    /// Ids are the filename on disk, so a collision would make two models the
    /// same file.
    #[test]
    fn catalogue_ids_are_unique() {
        let mut ids: Vec<String> = catalogue().into_iter().map(|m| m.id).collect();
        let count = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), count);
    }

    /// 4B is the measured floor for classification, so it has to be reachable
    /// on an ordinary laptop. A 16GB machine reports about 14.9GB once
    /// firmware has taken its share, and recommending against the sticker
    /// number hands that machine the model known not to work.
    #[test]
    fn a_sixteen_gigabyte_laptop_is_recommended_the_model_that_works() {
        let reported = 14_900_000_000u64;
        let best = catalogue()
            .into_iter()
            .filter(|m| m.kind == ModelKind::Reasoning)
            .filter(|m| m.recommended_ram_bytes <= reported)
            .max_by_key(|m| m.size_bytes)
            .unwrap();

        assert_eq!(
            best.id, "qwen3-4b-q4",
            "1.7B was measured as not good enough"
        );
    }

    /// And a genuinely small machine is still offered something.
    #[test]
    fn an_eight_gigabyte_machine_is_still_offered_a_model() {
        let reported = 7_800_000_000u64;
        assert!(catalogue()
            .into_iter()
            .any(|m| m.kind == ModelKind::Reasoning && m.recommended_ram_bytes <= reported));
    }
}
