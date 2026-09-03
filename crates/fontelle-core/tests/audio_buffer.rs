//! The decoded audio an audio clip plays from.
//!
//! `SampleBuffer` beside it is **mono** — every SF2 sample is, and everything
//! that reads one assumes it. A take off a microphone is mono and a loop off
//! disk is usually not, so a clip needs a buffer that knows how many channels
//! it has and can be asked for a fractional position in one of them.
//!
//! Fractional, because the player reads at whatever ratio the device asks for
//! — §7.6's argument, and the reason import does not resample. Between two
//! frames it interpolates; **off either end it is silent**, because the
//! alternative is a panic on the audio thread.

use std::sync::Arc;

use fontelle_core::{AudioBuffer, AudioStore};
use fontelle_types::AssetId;

/// A stereo buffer whose left channel counts up and whose right counts down,
/// so a channel mix-up cannot pass.
fn stereo(frames: usize) -> AudioBuffer {
    let mut data = Vec::with_capacity(frames * 2);
    for i in 0..frames {
        data.push(i as f32);
        data.push(-(i as f32));
    }
    AudioBuffer {
        data: Arc::from(data),
        sample_rate: 48_000,
        channels: 2,
    }
}

/// A fresh id. `SampleStore` mints them the same way, and the store below is a
/// secondary map, so any id of the right type indexes it.
fn an_id() -> AssetId {
    let mut map: slotmap::SlotMap<AssetId, ()> = slotmap::SlotMap::with_key();
    map.insert(())
}

#[test]
fn a_buffers_length_is_its_data_over_its_channels() {
    assert_eq!(stereo(100).frames(), 100);
    assert_eq!(
        AudioBuffer {
            data: Arc::from(vec![0.0; 9]),
            sample_rate: 48_000,
            channels: 2,
        }
        .frames(),
        4,
        "a half frame at the end is not a frame"
    );
}

#[test]
fn a_frame_is_read_out_of_the_channel_it_was_written_into() {
    let b = stereo(10);
    assert_eq!(b.sample(3, 0), 3.0);
    assert_eq!(b.sample(3, 1), -3.0);
}

#[test]
fn reading_off_either_end_is_silence_rather_than_a_panic() {
    // This runs on the audio thread, where an index out of bounds is not a
    // wrong sound but a dead process.
    let b = stereo(10);
    assert_eq!(b.sample(10, 0), 0.0);
    assert_eq!(b.sample(1000, 0), 0.0);
    assert_eq!(b.sample(0, 5), 0.0);
    assert_eq!(b.at(-1.0, 0), 0.0);
    assert_eq!(b.at(1e9, 0), 0.0);
}

#[test]
fn a_whole_position_reads_exactly_that_frame() {
    let b = stereo(10);
    assert_eq!(b.at(4.0, 0), 4.0);
    assert_eq!(b.at(4.0, 1), -4.0);
}

#[test]
fn a_position_between_two_frames_is_between_their_values() {
    let b = stereo(10);
    assert!((b.at(4.25, 0) - 4.25).abs() < 1e-6);
    assert!((b.at(4.75, 1) + 4.75).abs() < 1e-6);
}

#[test]
fn the_last_frame_reads_itself_rather_than_fading_into_silence() {
    // Interpolating towards a frame that is not there halves the last sample,
    // which on a loop is a dip at every seam.
    let b = stereo(10);
    assert_eq!(b.at(9.0, 0), 9.0);
}

#[test]
fn a_mono_buffer_answers_for_a_channel_it_does_not_have() {
    // A mono take on a stereo track: the player asks for the right channel and
    // has to be given the only one there is, not silence. A mono file that
    // played only out of the left speaker is the classic version of this.
    let b = AudioBuffer {
        data: Arc::from(vec![0.5, 0.25]),
        sample_rate: 48_000,
        channels: 1,
    };
    assert_eq!(b.sample(0, 0), 0.5);
    assert_eq!(b.sample(0, 1), 0.5, "mono is heard on both sides");
    assert_eq!(b.at(0.0, 1), 0.5);
}

// ------------------------------------------------------------- the store ---

#[test]
fn the_store_gives_back_what_was_put_in_under_the_id_it_was_put_in_under() {
    let mut store = AudioStore::default();
    let id = an_id();
    store.insert(id, stereo(5));
    assert_eq!(store.get(id).map(|b| b.frames()), Some(5));
}

#[test]
fn an_id_the_store_has_never_seen_is_absent_rather_than_a_panic() {
    let store = AudioStore::default();
    assert!(store.get(an_id()).is_none());
}

#[test]
fn cloning_the_store_shares_the_audio_rather_than_copying_it() {
    // The same property `SampleStore` has and for the same reason (TDD §7.7):
    // a graph rebuild clones the index while the audio thread is still holding
    // the old one, and copying a hundred megabytes of take per fader move is
    // not a rebuild anybody can do while playing.
    let mut store = AudioStore::default();
    let id = an_id();
    store.insert(id, stereo(1000));
    let copy = store.clone();
    let a = store.get(id).expect("it is there").data.as_ptr();
    let b = copy.get(id).expect("and in the copy").data.as_ptr();
    assert_eq!(a, b, "the clone copied the audio");
}
