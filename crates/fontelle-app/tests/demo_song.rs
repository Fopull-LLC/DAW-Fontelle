//! The demo phrase `--play-sf2` plays is the thing a human actually judges
//! Fontelle by right now, so it gets tested like anything else: that it
//! compiles to the notes it claims, that its chord really is simultaneous
//! (the case that exercises polyphony), and that its reported duration
//! actually covers the music.

use fontelle_app::demo_song;
use fontelle_types::{EventPayload, PPQN};

const SR: u32 = 48_000;
const BPM: f64 = 120.0;

#[test]
fn the_demo_phrase_compiles_to_a_run_followed_by_a_chord() {
    let song = demo_song(60, BPM, SR);
    let timeline = song.compile();

    let note_ons: Vec<_> = timeline
        .events
        .iter()
        .filter(|e| matches!(e.payload, EventPayload::NoteOn { .. }))
        .collect();

    assert_eq!(
        note_ons.len(),
        6,
        "expected 3 run notes + a 3-note chord, got {}",
        note_ons.len()
    );

    // Every note-on has a matching note-off.
    let note_offs = timeline
        .events
        .iter()
        .filter(|e| matches!(e.payload, EventPayload::NoteOff { .. }))
        .count();
    assert_eq!(note_offs, note_ons.len(), "every note-on needs a note-off");

    // The run: three notes at distinct, ascending times and pitches.
    let run: Vec<(i64, u8)> = note_ons
        .iter()
        .take(3)
        .map(|e| match e.payload {
            EventPayload::NoteOn { key, .. } => (e.sample, key),
            _ => unreachable!(),
        })
        .collect();
    for window in run.windows(2) {
        assert!(
            window[1].0 > window[0].0,
            "run notes must be sequential in time: {run:?}"
        );
        assert!(
            window[1].1 > window[0].1,
            "run notes must ascend in pitch: {run:?}"
        );
    }
    assert_eq!(run[0].1, 60, "the run starts on the root key it was given");
}

#[test]
fn the_chord_notes_all_start_on_the_same_sample() {
    let song = demo_song(60, BPM, SR);
    let timeline = song.compile();

    let chord: Vec<(i64, u8)> = timeline
        .events
        .iter()
        .filter_map(|e| match e.payload {
            EventPayload::NoteOn { key, .. } => Some((e.sample, key)),
            _ => None,
        })
        .skip(3)
        .collect();

    assert_eq!(chord.len(), 3, "the chord is a triad");
    let start = chord[0].0;
    for (sample, key) in &chord {
        assert_eq!(
            *sample, start,
            "chord note {key} starts at {sample}, not {start} — a chord must be simultaneous"
        );
    }

    let mut keys: Vec<u8> = chord.iter().map(|(_, k)| *k).collect();
    keys.sort_unstable();
    assert_eq!(keys, vec![60, 64, 67], "a C major triad from root 60");
}

#[test]
fn the_transposed_song_shifts_every_key_but_keeps_the_rhythm() {
    let base = demo_song(60, BPM, SR).compile();
    let up = demo_song(67, BPM, SR).compile();

    let keys = |t: &fontelle_types::CompiledTimeline| -> Vec<u8> {
        t.events
            .iter()
            .filter_map(|e| match e.payload {
                EventPayload::NoteOn { key, .. } => Some(key),
                _ => None,
            })
            .collect()
    };
    let times = |t: &fontelle_types::CompiledTimeline| -> Vec<i64> {
        t.events.iter().map(|e| e.sample).collect()
    };

    assert_eq!(
        times(&base),
        times(&up),
        "transposing must not change the rhythm"
    );
    for (low, high) in keys(&base).iter().zip(keys(&up).iter()) {
        assert_eq!(*high - *low, 7, "root 60 -> 67 is a fixed +7 transpose");
    }
}

#[test]
fn reported_duration_covers_the_last_note_off_plus_its_tail() {
    let song = demo_song(60, BPM, SR);
    let timeline = song.compile();
    let tail = PPQN; // one beat of ring-out

    let last_event = timeline
        .events
        .iter()
        .map(|e| e.sample)
        .max()
        .expect("the demo song has events");

    let duration = song.duration_samples(tail);
    assert!(
        duration > last_event,
        "duration {duration} must outlast the final event at {last_event}"
    );

    // One beat at 120bpm is 0.5s = 24000 samples; the tail should be about
    // that much past the last event, not an arbitrary padding.
    let overshoot = duration - last_event;
    assert!(
        (overshoot - 24_000).abs() < 100,
        "expected ~24000 samples of tail past the last event, got {overshoot}"
    );
}

/// Velocity now drives loudness (SF2's default velocity -> attenuation
/// modulator, `fontelle_core::velocity_to_gain`), so the demo phrase should
/// actually demonstrate it: the opening run is a crescendo. Without this the
/// only way to hear the feature is to edit the source.
#[test]
fn the_opening_run_is_a_crescendo_so_velocity_response_is_audible() {
    let song = demo_song(60, BPM, SR);
    let timeline = song.compile();

    let velocities: Vec<u8> = timeline
        .events
        .iter()
        .filter_map(|e| match e.payload {
            EventPayload::NoteOn { velocity, .. } => Some(velocity),
            _ => None,
        })
        .take(3)
        .collect();

    assert_eq!(velocities.len(), 3, "expected the three run notes");
    assert!(
        velocities[0] < velocities[1] && velocities[1] < velocities[2],
        "the run should rise in velocity, got {velocities:?}"
    );
    assert!(
        velocities[2] as f32 / velocities[0].max(1) as f32 >= 2.0,
        "a crescendo you can actually hear needs more than a few units of \
         velocity between first and last, got {velocities:?}"
    );
}
