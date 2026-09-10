//! The signature under a title (§5.2): 7-9 bars downsampled from what was
//! actually said, so every recording looks different.

/// §5.2 -- five bars is too few, too many recordings look alike.
pub const MIN_BARS: usize = 7;
pub const MAX_BARS: usize = 9;

/// Peak amplitude per bucket, normalised so the loudest bar is 1.0.
///
/// Peak rather than mean: speech is mostly gaps, and averaging flattens every
/// recording toward the same low line, which is the failure a fingerprint
/// exists to avoid.
pub fn downsample(pcm: &[f32]) -> Vec<f32> {
    if pcm.is_empty() {
        return Vec::new();
    }

    // Bar count varies with length inside the 7-9 band, so two recordings of
    // different lengths do not draw the same shape at the same width.
    let bars = MIN_BARS + (pcm.len() / 16_000).min(MAX_BARS - MIN_BARS);
    let per_bar = (pcm.len() as f64 / bars as f64).ceil().max(1.0) as usize;

    let peaks: Vec<f32> = pcm
        .chunks(per_bar)
        .map(|chunk| chunk.iter().fold(0.0_f32, |peak, s| peak.max(s.abs())))
        .collect();

    let loudest = peaks.iter().fold(0.0_f32, |m, p| m.max(*p));
    if loudest == 0.0 {
        return vec![0.0; peaks.len()];
    }
    peaks
        .iter()
        .map(|p| (p / loudest).clamp(0.0, 1.0))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(len: usize, amplitude: f32) -> Vec<f32> {
        (0..len)
            .map(|i| amplitude * (i as f32 * 0.1).sin())
            .collect()
    }

    #[test]
    fn silence_produces_no_fingerprint() {
        assert!(downsample(&[]).is_empty(), "nothing said, nothing to draw");
    }

    #[test]
    fn a_recording_gets_between_seven_and_nine_bars() {
        let bars = downsample(&tone(16_000, 0.6));
        assert!(
            (MIN_BARS..=MAX_BARS).contains(&bars.len()),
            "got {} bars",
            bars.len()
        );
    }

    #[test]
    fn every_bar_is_within_the_unit_range() {
        for bar in downsample(&tone(16_000, 0.9)) {
            assert!((0.0..=1.0).contains(&bar), "bar out of range: {bar}");
        }
    }

    /// Normalised, so a quiet recording still reads as a waveform rather than
    /// a flat line -- the shape is what distinguishes it, not the volume.
    #[test]
    fn a_quiet_recording_still_has_shape() {
        let bars = downsample(&tone(16_000, 0.02));
        let loudest = bars.iter().cloned().fold(0.0_f32, f32::max);
        assert!(
            loudest > 0.9,
            "quiet audio should still normalise, got {loudest}"
        );
    }

    /// Peak, not mean: a burst of speech surrounded by silence has to show as
    /// a tall bar, because averaging it away is what makes recordings alike.
    #[test]
    fn a_loud_burst_reads_taller_than_the_silence_around_it() {
        let mut pcm = vec![0.0_f32; 16_000];
        for sample in pcm.iter_mut().take(2_000) {
            *sample = 0.8;
        }
        let bars = downsample(&pcm);
        assert!(
            bars[0] > *bars.last().unwrap(),
            "the burst should dominate its bucket"
        );
    }

    /// A two-second note and a two-minute one both get a signature.
    #[test]
    fn a_very_short_recording_still_produces_bars() {
        let bars = downsample(&tone(200, 0.5));
        assert!(bars.len() >= MIN_BARS, "got {} bars", bars.len());
    }

    #[test]
    fn samples_shorter_than_the_bar_count_do_not_panic() {
        let bars = downsample(&[0.5, -0.5, 0.25]);
        assert!(bars.len() <= MAX_BARS);
    }

    /// Amplitude is symmetric; a negative swing is just as loud.
    #[test]
    fn negative_samples_count_as_loud() {
        let bars = downsample(&vec![-0.9_f32; 16_000]);
        assert!(
            bars.iter().all(|b| *b > 0.9),
            "sign should not change loudness"
        );
    }
}
