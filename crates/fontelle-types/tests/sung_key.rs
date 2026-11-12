//! Tune's *Listen for key* (Ty, 2026-11-12: *"add the ability in the
//! autotune plugin to sing and it automatically guesses during that singing
//! callibration what scale youre trying to have so put a scale callibration
//! feature"*): what was sung, as the corrector heard it, to the key it is in.

use fontelle_types::{TUNE_VOICED, TuneFrame, sung_key};

/// `frames` hops of a held note at MIDI `key`, with a little vibrato.
fn held(key: u8, frames: usize) -> Vec<TuneFrame> {
    (0..frames)
        .map(|i| TuneFrame {
            sung_cents: f32::from(key) * 100.0 + 18.0 * ((i as f32) * 0.7).sin(),
            out_cents: 0.0,
            target_cents: 0.0,
            flags: TUNE_VOICED,
        })
        .collect()
}

fn breath(frames: usize) -> Vec<TuneFrame> {
    (0..frames)
        .map(|_| TuneFrame {
            sung_cents: 0.0,
            out_cents: 0.0,
            target_cents: 0.0,
            flags: 0,
        })
        .collect()
}

fn phrase(keys: &[(u8, usize)]) -> Vec<TuneFrame> {
    let mut out = Vec::new();
    for &(key, frames) in keys {
        out.extend(held(key, frames));
        out.extend(breath(3));
    }
    out
}

#[test]
fn a_melody_in_a_minor_is_heard_as_a_minor() {
    // A B C D E F G, landing on A — sung over an octave, A heaviest.
    let frames = phrase(&[
        (57, 40),
        (59, 20),
        (60, 25),
        (62, 20),
        (64, 30),
        (65, 15),
        (67, 20),
        (69, 45),
        (64, 20),
        (57, 50),
    ]);
    let found = sung_key(&frames).expect("enough was sung");
    assert_eq!(found.key.root, 9, "{found:?}");
    assert_eq!(found.key.scale, "natural-minor", "{found:?}");
    assert!(found.fit > 0.95, "{found:?}");
    assert!(!found.few_notes);
}

#[test]
fn breaths_and_blips_are_not_notes() {
    // C major, with an F# lasting two hops — a scoop, not a note.
    let mut frames = phrase(&[
        (60, 40),
        (62, 20),
        (64, 30),
        (65, 20),
        (67, 30),
        (69, 20),
        (71, 20),
        (72, 40),
    ]);
    frames.extend(held(66, 2));
    frames.extend(breath(40));
    frames.extend(held(60, 40));
    let found = sung_key(&frames).unwrap();
    assert_eq!(found.key.root, 0, "{found:?}");
    assert_eq!(found.key.scale, "major", "{found:?}");
}

#[test]
fn it_never_answers_chromatic() {
    // Every semitone, evenly: the honest answer is the scale that fits
    // most of it, never the one that fits anything.
    let keys: Vec<(u8, usize)> = (60..72).map(|k| (k, 20)).collect();
    let found = sung_key(&phrase(&keys)).unwrap();
    assert_ne!(found.key.scale, "chromatic");
    assert!(found.fit < 1.0);
}

#[test]
fn too_little_singing_is_no_answer() {
    assert!(sung_key(&breath(200)).is_none());
    assert!(sung_key(&phrase(&[(60, 6)])).is_none());
    // Two notes is a guess, and says so.
    let found = sung_key(&phrase(&[(60, 40), (67, 40)])).unwrap();
    assert!(found.few_notes);
}
