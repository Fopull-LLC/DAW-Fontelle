//! Turning an audio clip's stretch switch, and what the sound keeps.
//!
//! Reported from using the window: *"when shortening an audio clip that was
//! stretched faster and then trying to elongate it again to get the part back
//! you cut out its stretching the clip back to how it was before before
//! letting you extend the length of the ending its odd."*
//!
//! A stretched clip's speed is not written anywhere — it is the ratio between
//! the file and the block, worked out by the player every block. So turning
//! the switch off used to *lose* it: the clip went back to the file's own
//! rate, the block became a window onto that, and the drag that followed cut
//! a different sound from the one being heard. What the switch has to do
//! instead is write that ratio onto the clip as its own speed and pitch —
//! **freeze** the stretch — so that with stretch off the sound is exactly
//! what it was and the block is a window onto it.

use fontelle_types::{AudioClipData, ClipStretch, MAX_CLIP_SPEED, MIN_CLIP_SPEED};

use crate::clip::Clip;
use crate::project::TempoMap;

/// `data` put in `stretch`, with the sound kept where it can be.
///
/// - **To `Off`**: the rate the clip was following its block at becomes its
///   speed *and* its pitch, both — varispeed is one number, and a plain read
///   is exactly a speed and a pitch that agree (`AudioClipData::shifts_pitch`).
///   The pass is the loop's period when the clip repeats, because that is
///   what a stretched loop fills with the file.
/// - **To `Resample`**: the speed and pitch are cleared. Following the block
///   *is* the request, and a clip frozen at double speed would otherwise fill
///   its block at double speed. A pitch set on purpose with stretch off is
///   lost here, which is the honest reading: `Resample` has no pitch that is
///   not a speed.
///
/// A clip whose file length or block is unknown is put in the mode with its
/// numbers untouched — there is nothing to freeze.
pub fn with_stretch(
    data: &AudioClipData,
    clip: &Clip,
    tempo: &TempoMap,
    stretch: ClipStretch,
) -> AudioClipData {
    let mut out = data.clone();
    if data.stretch == stretch {
        return out;
    }
    out.stretch = stretch;
    match stretch {
        ClipStretch::Resample => {
            out.speed = 1.0;
            out.pitch_semitones = 0.0;
        }
        ClipStretch::Off => {
            let pass = clip.loop_length.filter(|p| *p > 0).unwrap_or(clip.length);
            let pass_samples =
                tempo.tick_to_sample(clip.start + pass) - tempo.tick_to_sample(clip.start);
            let pass_seconds = pass_samples as f64 / tempo.sample_rate_hz().max(1.0);
            let file_seconds = data.seconds();
            if pass_seconds <= 0.0 || file_seconds <= 0.0 {
                return out;
            }
            // How fast the file was being read to fill the pass, with the
            // varispeed offset it carried on top.
            let rate = (file_seconds / pass_seconds) * data.time_rate();
            let rate = rate.clamp(MIN_CLIP_SPEED, MAX_CLIP_SPEED);
            // Written in semitones first and the speed derived from *that*,
            // so the two agree to the last bit and the player sees a plain
            // read rather than a shift of a rounding error.
            let semitones = (12.0 * rate.log2()) as f32;
            out.pitch_semitones = semitones.clamp(-48.0, 48.0);
            out.speed = 2f64.powf(f64::from(out.pitch_semitones) / 12.0);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use fontelle_types::{AssetKind, AssetRef, PPQN};

    fn take(seconds: f64) -> AudioClipData {
        AudioClipData::whole(
            AssetRef {
                id: fontelle_types::AssetId::default(),
                path: "take.wav".into(),
                content_hash: 0,
                size: 0,
                kind: AssetKind::Sample,
            },
            (seconds * 48_000.0) as i64,
            48_000,
        )
    }

    fn block(length: fontelle_types::Tick, loop_length: Option<fontelle_types::Tick>) -> Clip {
        Clip {
            lane: fontelle_types::LaneId::default(),
            start: PPQN * 4,
            length,
            source: crate::ClipSource::Audio(take(1.0)),
            prefab_link: None,
            color: None,
            muted: false,
            loop_length,
        }
    }

    #[test]
    fn freezing_writes_the_rate_the_block_was_played_at() {
        // One second of file on a one-beat block at 120: read at double speed.
        let tempo = TempoMap::new(120.0, 48_000.0);
        let mut data = take(1.0);
        data.stretch = ClipStretch::Resample;
        let out = with_stretch(&data, &block(PPQN, None), &tempo, ClipStretch::Off);
        assert_eq!(out.stretch, ClipStretch::Off);
        assert!((out.speed - 2.0).abs() < 1e-6, "{}", out.speed);
        assert!((out.pitch_semitones - 12.0).abs() < 1e-4);
        assert!(!out.shifts_pitch());
    }

    #[test]
    fn a_varispeed_offset_is_folded_into_the_frozen_rate() {
        let tempo = TempoMap::new(120.0, 48_000.0);
        let mut data = take(1.0);
        data.stretch = ClipStretch::Resample;
        data.speed = 0.5;
        let out = with_stretch(&data, &block(PPQN, None), &tempo, ClipStretch::Off);
        assert!((out.speed - 1.0).abs() < 1e-6, "{}", out.speed);
    }

    #[test]
    fn a_loop_freezes_at_the_rate_of_one_pass() {
        let tempo = TempoMap::new(120.0, 48_000.0);
        let mut data = take(1.0);
        data.stretch = ClipStretch::Resample;
        let out = with_stretch(
            &data,
            &block(PPQN * 8, Some(PPQN)),
            &tempo,
            ClipStretch::Off,
        );
        assert!((out.speed - 2.0).abs() < 1e-6, "{}", out.speed);
    }

    #[test]
    fn thawing_clears_the_offsets_so_the_file_fills_the_block() {
        let tempo = TempoMap::new(120.0, 48_000.0);
        let mut data = take(1.0);
        data.speed = 2.0;
        data.pitch_semitones = 12.0;
        let out = with_stretch(&data, &block(PPQN, None), &tempo, ClipStretch::Resample);
        assert_eq!(out.stretch, ClipStretch::Resample);
        assert_eq!(out.speed, 1.0);
        assert_eq!(out.pitch_semitones, 0.0);
    }

    #[test]
    fn the_same_mode_changes_nothing() {
        let tempo = TempoMap::new(120.0, 48_000.0);
        let mut data = take(1.0);
        data.speed = 3.0;
        let out = with_stretch(&data, &block(PPQN, None), &tempo, ClipStretch::Off);
        assert_eq!(out, data);
    }
}
