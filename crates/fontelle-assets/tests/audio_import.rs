//! Bringing a sound in from a file (TDD §15).
//!
//! Reported from using the window:
//!
//! > *"right now we can basically only do things with soundfonts but i want to
//! > also be able to record my voice into the daw or import different sounds
//! > and loops and whatnot to make songs with."*
//!
//! Both halves of that start here: an audio clip is a **reference to decoded
//! samples**, and everything after it — the waveform in the block, the fades,
//! the filter, the recording that lands as a new file — is a thing done to
//! those samples. So this file is about the two claims the rest rests on.
//!
//! - **What comes out is what went in.** A file of a known signal decodes to
//!   that signal, at its own rate, with its own channel count. Sample-accurate,
//!   because "close enough" in a decoder is a click at the seam of every loop.
//! - **A file that is not one is refused rather than guessed at.** The window
//!   can say so; it cannot recover from half a buffer of noise.
//!
//! The fixtures are written here rather than checked in, for the reason every
//! other fixture in this crate is: a test that depends on a binary blob nobody
//! can read is a test nobody can change.

use fontelle_assets::fixtures::{build_wav, write_fixture_to_temp_file};
use fontelle_assets::{AudioAsset, import_audio, read_audio};

/// A ramp from -1 to +1 across `frames`, so every sample is distinguishable
/// from every other and an off-by-one in the decoder cannot hide.
fn ramp(frames: usize) -> Vec<f32> {
    (0..frames)
        .map(|i| (i as f32 / (frames - 1) as f32) * 2.0 - 1.0)
        .collect()
}

#[test]
fn a_mono_wav_decodes_to_the_samples_it_was_written_from() {
    let wanted = ramp(1000);
    let bytes = build_wav(48_000, 1, &wanted);
    let asset = read_audio(&bytes, "ramp.wav").expect("a wav this crate wrote must read back");

    assert_eq!(asset.sample_rate, 48_000);
    assert_eq!(asset.channels, 1);
    assert_eq!(asset.frames, wanted.len());
    assert_eq!(asset.samples.len(), wanted.len());
    for (i, (got, want)) in asset.samples.iter().zip(&wanted).enumerate() {
        // 16-bit PCM: a sample is quantised to 1/32768, so this is exact to
        // the format's own resolution rather than to f32's.
        assert!(
            (got - want).abs() < 1.0 / 16_384.0,
            "frame {i} decoded as {got}, written as {want}"
        );
    }
}

#[test]
fn a_stereo_wav_keeps_its_channels_apart() {
    // Interleaved, and the two channels deliberately differ: a decoder that
    // dropped one and duplicated the other passes every mono test there is.
    let frames = 500;
    let mut interleaved = Vec::with_capacity(frames * 2);
    for i in 0..frames {
        interleaved.push(i as f32 / frames as f32);
        interleaved.push(-(i as f32 / frames as f32));
    }
    let bytes = build_wav(44_100, 2, &interleaved);
    let asset = read_audio(&bytes, "stereo.wav").expect("a stereo wav must read back");

    assert_eq!(asset.sample_rate, 44_100);
    assert_eq!(asset.channels, 2);
    assert_eq!(asset.frames, frames);
    assert_eq!(asset.samples.len(), frames * 2);
    for i in 0..frames {
        let (l, r) = (asset.sample(i, 0), asset.sample(i, 1));
        assert!((l + r).abs() < 1.0 / 16_384.0, "frame {i} is {l} and {r}");
    }
}

#[test]
fn the_duration_is_the_frames_over_the_rate() {
    let asset = AudioAsset {
        sample_rate: 48_000,
        channels: 2,
        frames: 24_000,
        samples: vec![0.0; 48_000],
    };
    assert!((asset.seconds() - 0.5).abs() < 1e-9);
}

#[test]
fn a_file_that_is_not_audio_is_refused_rather_than_guessed_at() {
    let err =
        read_audio(b"this is not a sound", "notes.txt").expect_err("a text file is not a sound");
    assert!(
        err.0.to_lowercase().contains("notes.txt"),
        "the message has to name the file: {}",
        err.0
    );
}

#[test]
fn an_empty_file_is_refused_rather_than_read_as_silence() {
    // A zero-length clip is a clip that cannot be dragged, resized or heard,
    // and it would arrive on the arrangement looking like a bug in the
    // arrangement.
    assert!(read_audio(&[], "empty.wav").is_err());
}

#[test]
fn a_wav_on_disk_reads_the_same_as_one_in_memory() {
    // `import_audio` is the whole path — read the file, decode it, say what it
    // is — and the only part of it this crate's other tests do not exercise.
    let wanted = ramp(300);
    let path = write_fixture_to_temp_file("disk.wav", &build_wav(22_050, 1, &wanted));
    let asset = import_audio(&path).expect("the file was just written");
    assert_eq!(asset.sample_rate, 22_050);
    assert_eq!(asset.frames, wanted.len());
    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_file_that_is_not_there_says_so() {
    let err = import_audio(std::path::Path::new("/nowhere/at/all.wav"))
        .expect_err("a missing file cannot be imported");
    assert!(err.0.contains("all.wav"), "{}", err.0);
}
