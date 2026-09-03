//! The waveform you see inside an audio clip (TDD §15.3).
//!
//! *"i should be able to see the waveform of the audio inside the clip."*
//!
//! Drawing a waveform is not drawing the samples: a four-bar clip at 48 kHz is
//! about four hundred thousand frames and the block it sits in is three hundred
//! pixels wide, so what is actually drawn is **the loudest and quietest sample
//! in each pixel's worth of time**. Computing that per frame per repaint is the
//! thing that makes a timeline crawl, so it is computed once, at several
//! resolutions, and the renderer picks the one that fits.
//!
//! Two properties do all the work here and both are easy to get subtly wrong:
//!
//! - **A bucket's extremes are the extremes of the samples in it.** A peak file
//!   that averages instead is a waveform that flattens as you zoom out, which
//!   is a lie about how loud the take was.
//! - **Every level covers the whole clip.** A coarser level that quietly drops
//!   the last partial bucket draws a clip that stops before it ends.

use fontelle_assets::{PEAK_BUCKET, PeakData, generate_peaks};
use fontelle_types::AssetId;

fn an_asset() -> AssetId {
    let mut arena: fontelle_model::Arena<AssetId, ()> = fontelle_model::Arena::default();
    arena.insert(())
}

#[test]
fn the_finest_level_holds_the_extremes_of_each_bucket() {
    // One bucket's worth of silence, then one bucket holding a single spike:
    // the second bucket has to say so, and the first must not.
    let mut samples = vec![0.0f32; PEAK_BUCKET * 2];
    samples[PEAK_BUCKET + 3] = 0.75;
    samples[PEAK_BUCKET + 4] = -0.5;

    let peaks = generate_peaks(an_asset(), &samples, 1);
    let finest = peaks.levels.last().expect("there is at least one level");
    assert_eq!(finest.len(), 2, "two buckets of samples make two buckets");
    assert_eq!(finest[0], (0.0, 0.0));
    assert_eq!(finest[1], (-0.5, 0.75), "min first, max second");
}

#[test]
fn a_partial_bucket_at_the_end_is_still_a_bucket() {
    // Otherwise the tail of every clip whose length is not a round number of
    // buckets is drawn as nothing.
    let samples = vec![0.5f32; PEAK_BUCKET + 1];
    let peaks = generate_peaks(an_asset(), &samples, 1);
    let finest = peaks.levels.last().expect("a level");
    assert_eq!(finest.len(), 2);
    assert_eq!(finest[1], (0.5, 0.5));
}

#[test]
fn the_levels_run_coarsest_first_and_each_is_half_the_one_after_it() {
    let samples = vec![0.0f32; PEAK_BUCKET * 64];
    let peaks = generate_peaks(an_asset(), &samples, 1);
    assert!(peaks.levels.len() > 1, "one level is not multi-resolution");
    for pair in peaks.levels.windows(2) {
        assert!(
            pair[0].len() < pair[1].len(),
            "the levels are not coarsest-first: {} then {}",
            pair[0].len(),
            pair[1].len()
        );
        assert_eq!(
            pair[0].len(),
            pair[1].len().div_ceil(2),
            "a level is not half the one after it"
        );
    }
}

#[test]
fn a_coarse_level_keeps_the_extremes_the_fine_one_found() {
    // The whole point of the coarse levels: zooming out must not quietly make
    // a loud take look like a quiet one.
    let mut samples = vec![0.0f32; PEAK_BUCKET * 16];
    samples[PEAK_BUCKET * 9 + 7] = 0.9;
    let peaks = generate_peaks(an_asset(), &samples, 1);
    for level in &peaks.levels {
        let loudest = level.iter().map(|(_, max)| *max).fold(f32::MIN, f32::max);
        assert!(
            (loudest - 0.9).abs() < 1e-6,
            "a level of {} buckets lost the peak: {loudest}",
            level.len()
        );
    }
}

#[test]
fn a_stereo_asset_is_summed_into_one_waveform() {
    // One waveform per clip, not one per channel: the block is fourteen pixels
    // tall at the height a lane ships at, and two waveforms in it are neither.
    // The extremes are taken across both channels, so a spike in one is a spike
    // in the picture.
    let mut samples = vec![0.0f32; PEAK_BUCKET * 2];
    samples[1] = 0.8; // right channel of frame 0
    let peaks = generate_peaks(an_asset(), &samples, 2);
    let finest = peaks.levels.last().expect("a level");
    assert_eq!(finest.len(), 1, "two channels of one bucket is one bucket");
    assert_eq!(finest[0].1, 0.8);
}

#[test]
fn nothing_at_all_produces_no_levels_rather_than_a_panic() {
    let peaks: PeakData = generate_peaks(an_asset(), &[], 1);
    assert!(peaks.levels.iter().all(|level| level.is_empty()));
}

#[test]
fn the_window_a_range_of_frames_falls_in_is_arithmetic_anyone_can_check() {
    // What the renderer asks: "give me `width` buckets covering frames
    // `start..end`". It picks the finest level that does not cost more than
    // that, so a clip zoomed right in is drawn from real detail and one zoomed
    // right out is drawn from the summary.
    let samples = vec![0.25f32; PEAK_BUCKET * 32];
    let peaks = generate_peaks(an_asset(), &samples, 1);

    let coarse = peaks.level_for(PEAK_BUCKET * 32, 4);
    let fine = peaks.level_for(PEAK_BUCKET * 32, 4096);
    assert!(
        peaks.levels[coarse].len() <= peaks.levels[fine].len(),
        "asking for more detail gave less"
    );
    assert_eq!(fine, peaks.levels.len() - 1, "the finest there is");
}
