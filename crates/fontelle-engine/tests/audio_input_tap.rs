//! One microphone, two listeners (`docs/analyze-musically-plan.md` §5 R4):
//! an interface usually cannot be opened for capture twice, so Analyze
//! Musically's Record page reads the ring the mixer's armed track already
//! has open, through a tap on its writer.

use fontelle_engine::{RtGuardAllocator, input_capture_channel};

#[global_allocator]
static ALLOCATOR: RtGuardAllocator = RtGuardAllocator;

#[test]
fn a_closed_tap_hears_nothing_and_an_open_one_hears_what_the_track_hears() {
    let (writer, mut reader) = input_capture_channel(4_096);
    let (mut writer, mut tap) = writer.with_tap(4_096);
    let block: Vec<f32> = (0..256).map(|i| i as f32).collect();
    writer.write(&block);
    let mut heard = Vec::new();
    tap.drain_into(&mut heard);
    assert!(
        heard.is_empty(),
        "a closed tap heard {} samples",
        heard.len()
    );

    tap.open();
    assert!(tap.is_open());
    writer.write(&block);
    let (mut track, mut study) = (Vec::new(), Vec::new());
    reader.drain_into(&mut track);
    tap.drain_into(&mut study);
    assert_eq!(&track[256..], &block[..], "the track's take is untouched");
    assert_eq!(study, block, "the study hears the same samples");

    tap.close();
    writer.write(&block);
    study.clear();
    tap.drain_into(&mut study);
    assert!(study.is_empty());
}

#[test]
fn a_full_tap_drops_its_own_samples_and_never_the_tracks() {
    let (writer, mut reader) = input_capture_channel(10_000);
    let (mut writer, mut tap) = writer.with_tap(100);
    tap.open();
    let block = vec![0.5f32; 1_000];
    assert_eq!(
        writer.write(&block),
        1_000,
        "the track's ring took all of it"
    );
    assert_eq!(tap.dropped(), 900);
    assert_eq!(writer.dropped(), 0);
    let mut track = Vec::new();
    reader.drain_into(&mut track);
    assert_eq!(track.len(), 1_000);
}

#[test]
fn opening_the_tap_starts_it_clean() {
    let (writer, _reader) = input_capture_channel(4_096);
    let (mut writer, mut tap) = writer.with_tap(4_096);
    tap.open();
    writer.write(&[1.0; 64]);
    tap.close();
    tap.open();
    writer.write(&[2.0; 64]);
    let mut heard = Vec::new();
    tap.drain_into(&mut heard);
    assert_eq!(
        heard,
        vec![2.0; 64],
        "the last take's tail came with the next"
    );
}

#[test]
fn writing_with_a_tap_open_does_not_allocate() {
    let (writer, _reader) = input_capture_channel(1_024);
    let (mut writer, mut tap) = writer.with_tap(64);
    tap.open();
    let block = vec![0.25f32; 256];
    fontelle_engine::mark_current_thread_rt();
    for _ in 0..16 {
        writer.write(&block);
    }
    fontelle_engine::unmark_current_thread_rt();
    assert!(tap.dropped() > 0, "the full path ran too");
}
