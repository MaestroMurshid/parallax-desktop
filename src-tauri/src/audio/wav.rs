//! Writing a recording to disk.
//!
//! 16 kHz mono, which is what transcribe.cpp wants and therefore what the
//! capture stream is configured to produce -- no resampling step anywhere.

use crate::error::Result;
use std::path::Path;

pub const SAMPLE_RATE: u32 = 16_000;

/// PCM as 16-bit samples. §9.5 keeps the audio permanently as the record the
/// transcript is derived from, so this is lossless rather than compressed --
/// opus would cut a year of daily use from about 7GB to 650MB and is the
/// obvious next step, but it adds a C encoder and the swap is one function.
pub fn write(pcm: &[f32], path: &Path) -> Result<u64> {
    let _ = (pcm, path);
    todo!("write")
}

pub fn read(path: &Path) -> Result<Vec<f32>> {
    let _ = path;
    todo!("read")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("parallax-wav-tests");
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(format!("{}-{}.wav", name, uuid::Uuid::new_v4()))
    }

    #[test]
    fn a_recording_round_trips() {
        let path = temp("round-trip");
        let pcm: Vec<f32> = (0..1000).map(|i| (i as f32 * 0.01).sin() * 0.5).collect();

        write(&pcm, &path).unwrap();
        let back = read(&path).unwrap();

        assert_eq!(back.len(), pcm.len());
        for (a, b) in pcm.iter().zip(&back) {
            assert!((a - b).abs() < 0.001, "{a} became {b}");
        }
        let _ = std::fs::remove_file(path);
    }

    /// transcribe.cpp wants 16 kHz mono. Writing anything else means a
    /// transcript of the wrong thing, slowed down or sped up.
    #[test]
    fn it_writes_sixteen_kilohertz_mono() {
        let path = temp("format");
        write(&[0.1, 0.2, 0.3], &path).unwrap();

        let spec = hound::WavReader::open(&path).unwrap().spec();
        assert_eq!(spec.sample_rate, SAMPLE_RATE);
        assert_eq!(spec.channels, 1);
        let _ = std::fs::remove_file(path);
    }

    /// A clipped microphone hands back samples outside the range. Wrapping
    /// them would turn a loud moment into a burst of noise.
    #[test]
    fn samples_beyond_the_range_are_clamped_not_wrapped() {
        let path = temp("clip");
        write(&[2.0, -2.0, 0.0], &path).unwrap();

        let back = read(&path).unwrap();
        assert!(back[0] > 0.99, "positive clipping stays positive: {}", back[0]);
        assert!(back[1] < -0.99, "negative clipping stays negative: {}", back[1]);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn writing_reports_the_size_on_disk() {
        let path = temp("size");
        let bytes = write(&vec![0.0; SAMPLE_RATE as usize], &path).unwrap();

        assert_eq!(bytes, std::fs::metadata(&path).unwrap().len());
        // One second of 16-bit mono is about 32KB, plus a small header.
        assert!(bytes > 32_000 && bytes < 33_000, "unexpected size {bytes}");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn an_empty_recording_still_writes_a_valid_file() {
        let path = temp("empty");
        write(&[], &path).unwrap();
        assert!(read(&path).unwrap().is_empty());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn a_missing_directory_is_created() {
        let dir = std::env::temp_dir().join(format!("parallax-nested-{}", uuid::Uuid::new_v4()));
        let path = dir.join("deeper").join("take.wav");

        write(&[0.1], &path).unwrap();
        assert!(path.is_file());
        let _ = std::fs::remove_dir_all(dir);
    }
}
