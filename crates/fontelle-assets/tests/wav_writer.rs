//! Writing a take to disk while it is still being recorded (TDD §15.4).
//!
//! > *"Recording writes directly to the configured recordings directory as
//! > WAV, streaming from the RT thread through a lock-free ring to the disk
//! > thread. The RT thread never touches the filesystem. ... A recording in
//! > progress is crash-safe: the WAV header is finalised incrementally so a
//! > killed process leaves a playable file."*
//!
//! Both halves of that last sentence are tested here, because both are easy to
//! get subtly wrong and neither fails loudly: a header written once at the end
//! produces a file that is silently zero seconds long if anything goes wrong,
//! and a header rewritten per block that gets the arithmetic wrong produces a
//! file that plays as noise.

use fontelle_assets::{WavWriter, read_audio};

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "fontelle-wav-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).expect("creatable");
    dir
}

/// A ramp, so every sample is distinguishable from every other.
fn ramp(n: usize) -> Vec<f32> {
    (0..n).map(|i| (i as f32 / n as f32) * 2.0 - 1.0).collect()
}

#[test]
fn what_is_written_reads_back_as_what_was_written() {
    let dir = scratch("roundtrip");
    let path = dir.join("take.wav");
    let wanted = ramp(1000);
    {
        let mut writer = WavWriter::create(&path, 48_000, 1).expect("creatable");
        writer.write(&wanted).expect("writable");
        writer.finish().expect("closeable");
    }
    let back = read_audio(&std::fs::read(&path).unwrap(), "take.wav").expect("readable");
    assert_eq!(back.sample_rate, 48_000);
    assert_eq!(back.channels, 1);
    assert_eq!(back.frames, wanted.len());
    for (i, (got, want)) in back.samples.iter().zip(&wanted).enumerate() {
        assert!(
            (got - want).abs() < 1.0 / 16_384.0,
            "frame {i}: {got} vs {want}"
        );
    }
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_take_written_in_many_blocks_is_one_continuous_take() {
    // What a recording actually does: a block per callback, for minutes.
    let dir = scratch("blocks");
    let path = dir.join("long.wav");
    let wanted = ramp(4096);
    {
        let mut writer = WavWriter::create(&path, 44_100, 2).expect("creatable");
        for block in wanted.chunks(128) {
            writer.write(block).expect("writable");
        }
        writer.finish().expect("closeable");
    }
    let back = read_audio(&std::fs::read(&path).unwrap(), "long.wav").expect("readable");
    assert_eq!(back.channels, 2);
    assert_eq!(back.frames, wanted.len() / 2);
    assert_eq!(back.samples.len(), wanted.len());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_file_abandoned_mid_take_is_still_a_playable_file() {
    // The crash-safety claim. The writer is dropped without `finish` — which
    // is what a killed process amounts to — and what is on disk still opens
    // and still holds what had been written.
    let dir = scratch("crash");
    let path = dir.join("interrupted.wav");
    let wanted = ramp(512);
    {
        let mut writer = WavWriter::create(&path, 48_000, 1).expect("creatable");
        writer.write(&wanted).expect("writable");
        // No `finish`, and no flush of our own: whatever the header says now is
        // what a killed process would leave behind.
        std::mem::forget(writer);
    }
    let back = read_audio(&std::fs::read(&path).unwrap(), "interrupted.wav")
        .expect("an abandoned take must still be readable");
    assert_eq!(
        back.frames,
        wanted.len(),
        "the header did not know about the audio behind it"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn an_empty_take_is_a_valid_file_with_nothing_in_it() {
    let dir = scratch("empty");
    let path = dir.join("nothing.wav");
    {
        let writer = WavWriter::create(&path, 48_000, 1).expect("creatable");
        writer.finish().expect("closeable");
    }
    // A zero-length `data` chunk is legal WAV, and the decoder refuses it as
    // "nothing at all" rather than crashing — which is the honest pair of
    // answers.
    assert!(std::fs::metadata(&path).expect("it exists").len() >= 44);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_writer_says_how_many_frames_it_has_taken() {
    let dir = scratch("count");
    let path = dir.join("count.wav");
    let mut writer = WavWriter::create(&path, 48_000, 2).expect("creatable");
    assert_eq!(writer.frames(), 0);
    writer.write(&[0.0; 8]).expect("writable");
    assert_eq!(writer.frames(), 4, "frames, not samples");
    writer.finish().expect("closeable");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_path_that_cannot_be_written_says_so_rather_than_pretending() {
    let err = WavWriter::create(std::path::Path::new("/nowhere/at/all/take.wav"), 48_000, 1)
        .expect_err("an unwritable path");
    assert!(!err.0.is_empty());
}

#[test]
fn samples_past_full_scale_are_clamped_rather_than_wrapped() {
    // A wrapped sample is a full-scale click in the middle of a take, and an
    // input that peaks over is an ordinary thing that happens.
    let dir = scratch("clip");
    let path = dir.join("hot.wav");
    {
        let mut writer = WavWriter::create(&path, 48_000, 1).expect("creatable");
        writer.write(&[2.0, -2.0, 1.0, -1.0]).expect("writable");
        writer.finish().expect("closeable");
    }
    let back = read_audio(&std::fs::read(&path).unwrap(), "hot.wav").expect("readable");
    for sample in &back.samples {
        assert!(
            (-1.0..=1.0).contains(sample),
            "a sample wrapped to {sample}"
        );
    }
    assert!(
        back.samples[0] > 0.9,
        "the loud one came out quiet or inverted"
    );
    assert!(back.samples[1] < -0.9);
    std::fs::remove_dir_all(&dir).ok();
}
