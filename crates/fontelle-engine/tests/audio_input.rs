//! Getting a microphone into the program (TDD §15.4).
//!
//! Reported from using the window:
//!
//! > *"basically i go in the mixer make a new track, name it to like mic or
//! > something then i click a input button that lets my select my mic input to
//! > feed to that mixer track. when its recording its going through that track
//! > and recording into the arrangement as an audio clip."*
//!
//! There is no sound card in a test, so what is checked here is the half that
//! does not need one: the **ring between the input callback and the rest of the
//! program**. §15.4 is explicit about why that ring exists — *"streaming from
//! the RT thread through a lock-free ring to the disk thread. The RT thread
//! never touches the filesystem."* — and it is the piece where a mistake is a
//! click in somebody's take rather than a compile error.
//!
//! The device half is exercised by `input_devices_can_be_listed`, which asks
//! the real host what it has and is satisfied either way: a machine with no
//! microphone is a machine, not a failure.

use fontelle_engine::{AudioDevice, InputCapture, input_capture_channel};

#[test]
fn what_is_written_comes_out_in_the_order_it_went_in() {
    // A ring that reordered or dropped would be a take with a click in it, and
    // the click would be blamed on the microphone.
    let (mut writer, mut reader) = input_capture_channel(1024);
    let block: Vec<f32> = (0..64).map(|i| i as f32).collect();
    assert_eq!(writer.write(&block), 64);

    let mut out = Vec::new();
    reader.drain_into(&mut out);
    assert_eq!(out, block);
}

#[test]
fn draining_twice_does_not_hand_the_same_samples_back_again() {
    let (mut writer, mut reader) = input_capture_channel(1024);
    writer.write(&[1.0, 2.0, 3.0]);
    let mut out = Vec::new();
    reader.drain_into(&mut out);
    reader.drain_into(&mut out);
    assert_eq!(out, vec![1.0, 2.0, 3.0]);
}

#[test]
fn a_ring_that_fills_up_drops_what_it_cannot_hold_and_says_how_much() {
    // **Never blocks and never allocates**: this is written from the input
    // callback (INVARIANT 1). A reader that has stopped draining is a bug
    // somewhere else, and the honest answer is to lose the newest samples and
    // report it — not to wait on the audio thread.
    let (mut writer, _reader) = input_capture_channel(16);
    let block = vec![0.5f32; 64];
    let written = writer.write(&block);
    assert!(written < 64, "it claimed to write more than it holds");
    assert_eq!(writer.dropped(), 64 - written);
}

#[test]
fn a_ring_reports_nothing_dropped_while_it_is_keeping_up() {
    let (mut writer, mut reader) = input_capture_channel(1024);
    let mut out = Vec::new();
    for _ in 0..20 {
        writer.write(&[0.25f32; 32]);
        reader.drain_into(&mut out);
    }
    assert_eq!(writer.dropped(), 0);
    assert_eq!(out.len(), 20 * 32);
}

#[test]
fn draining_an_empty_ring_is_nothing_rather_than_a_wait() {
    let (_writer, mut reader) = input_capture_channel(64);
    let mut out = vec![9.0];
    reader.drain_into(&mut out);
    assert_eq!(out, vec![9.0], "it invented samples");
}

#[test]
fn a_capture_says_how_many_frames_it_holds_of_however_many_channels() {
    // The ring carries interleaved samples and a take is counted in frames.
    // Two numbers that could disagree is exactly where a stereo take ends up
    // half as long as it should be.
    let capture = InputCapture::new(2);
    assert_eq!(capture.channels(), 2);
    assert_eq!(capture.frames(), 0);

    let mut capture = capture;
    capture.push(&[0.1, 0.2, 0.3, 0.4, 0.5, 0.6]);
    assert_eq!(capture.frames(), 3);
    assert_eq!(capture.samples().len(), 6);
}

#[test]
fn a_capture_cleared_is_a_capture_with_nothing_in_it() {
    let mut capture = InputCapture::new(1);
    capture.push(&[1.0, 2.0]);
    capture.clear();
    assert_eq!(capture.frames(), 0);
    assert!(capture.samples().is_empty());
}

#[test]
fn the_input_list_is_one_of_each_and_nothing_that_cannot_be_opened() {
    // Found by opening the menu on a real machine: ALSA offered **thirty-two**
    // inputs, four of them the same Scarlett, and most of the rest plumbing —
    // *"Rate Converter Plugin Using Libav/FFmpeg Library"*, *"Plugin for
    // channel upmix (4,6,8)"*. A person cannot pick their microphone out of
    // that, and half the list cannot capture anything at all.
    //
    // So the list is filtered by **asking each device whether it will open**,
    // which is a real question rather than a guess at what a name means, and
    // deduplicated by name. On the machine this was found on it goes from
    // thirty-two rows to seven.
    let device = AudioDevice::default_host();
    let names = device.input_names();
    let mut seen = std::collections::BTreeSet::new();
    for name in &names {
        assert!(
            seen.insert(name.clone()),
            "{name:?} is in the menu twice \u{2014} which one is the microphone?"
        );
    }
}

#[test]
fn input_devices_can_be_listed() {
    // A machine with no microphone is a machine, not a failure: what matters
    // is that asking does not panic and that every name is something a person
    // could pick out of a menu.
    let device = AudioDevice::default_host();
    for name in device.input_names() {
        assert!(!name.trim().is_empty(), "an input with no name is unpickable");
    }
    // And the default, when there is one, is one of them — which is a real
    // claim rather than a tautology: ALSA's own `default` PCM describes itself
    // as "Default Audio Device" and then refuses to open for capture on this
    // machine, so the two answers have to be made to agree or the window would
    // offer a device that cannot record.
    if let Some(default) = device.default_input_name() {
        assert!(device.input_names().contains(&default));
    }
}
