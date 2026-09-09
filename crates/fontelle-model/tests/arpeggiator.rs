//! The arpeggiator: a chord turned into a run of single notes.
//!
//! FL Studio's tool, and the settings people expect from it — direction,
//! step length, gate, octave range, repeats — plus the two it does not have
//! that are worth having: **swing**, because an arp on a straight grid is the
//! most obviously machine-made thing in a project, and a **velocity ramp**,
//! because a run that never changes weight reads as a preset rather than as a
//! part.
//!
//! Everything here is pure arithmetic on notes, so it is checkable without a
//! roll, a clip or a window — which is the whole reason it lives in this
//! crate and not in the canvas.

use fontelle_model::{ArpDirection, ArpSpec, Note, arpeggiated};
use fontelle_types::PPQN;

/// A C-major triad, one bar long, all three notes struck together.
fn a_chord() -> Vec<Note> {
    [60u8, 64, 67]
        .into_iter()
        .map(|key| Note {
            start: 0,
            length: PPQN * 4,
            key,
            velocity: 100,
            pan: 0,
            fine_pitch: 0,
            release: 0,
            mod_x: 0,
            mod_y: 0,
            slide: false,
            channel: None,
        })
        .collect()
}

fn spec() -> ArpSpec {
    ArpSpec {
        step: PPQN / 4,
        direction: ArpDirection::Up,
        octaves: 1,
        gate: 0.5,
        repeats: 1,
        swing: 0.0,
        velocity_ramp: 0.0,
    }
}

#[test]
fn a_chord_becomes_a_run_of_single_notes_up_the_chord() {
    let out = arpeggiated(&a_chord(), spec());
    // A bar of sixteenths.
    assert_eq!(out.len(), 16, "a bar at a sixteenth is sixteen steps");
    // Up: C, E, G, C, E, G …
    assert_eq!(
        out.iter().take(6).map(|n| n.key).collect::<Vec<_>>(),
        vec![60, 64, 67, 60, 64, 67]
    );
    // Each on its own step, in order, none overlapping the next.
    for pair in out.windows(2) {
        assert_eq!(pair[1].start - pair[0].start, PPQN / 4);
        assert!(
            pair[0].start + pair[0].length <= pair[1].start,
            "two arp notes overlap"
        );
    }
}

#[test]
fn the_run_stays_inside_the_chord_it_came_from() {
    let out = arpeggiated(&a_chord(), spec());
    let last = out.last().expect("some notes");
    assert!(
        last.start + last.length <= PPQN * 4,
        "the arp ran past the chord it replaced"
    );
    assert_eq!(out[0].start, 0, "and it starts where the chord did");
}

#[test]
fn every_direction_walks_the_chord_its_own_way() {
    let take = |direction: ArpDirection, n: usize| {
        arpeggiated(
            &a_chord(),
            ArpSpec {
                direction,
                ..spec()
            },
        )
        .iter()
        .take(n)
        .map(|note| note.key)
        .collect::<Vec<_>>()
    };
    assert_eq!(take(ArpDirection::Up, 4), vec![60, 64, 67, 60]);
    assert_eq!(take(ArpDirection::Down, 4), vec![67, 64, 60, 67]);
    // Up then down **without repeating the ends**, which is what makes it read
    // as a turn rather than as a stutter: C E G E | C E G E.
    assert_eq!(
        take(ArpDirection::UpDown, 8),
        vec![60, 64, 67, 64, 60, 64, 67, 64]
    );
    assert_eq!(
        take(ArpDirection::DownUp, 8),
        vec![67, 64, 60, 64, 67, 64, 60, 64]
    );
    // As played keeps the order the notes were written in, which for a chord
    // written low to high is the same as Up — so the test that means anything
    // is that a chord written *high first* comes out high first.
    let reversed: Vec<Note> = a_chord().into_iter().rev().collect();
    let played = arpeggiated(
        &reversed,
        ArpSpec {
            direction: ArpDirection::AsPlayed,
            ..spec()
        },
    );
    assert_eq!(
        played.iter().take(3).map(|n| n.key).collect::<Vec<_>>(),
        vec![67, 64, 60],
        "As played did not keep the order they were written in"
    );
}

#[test]
fn octaves_extend_the_run_above_the_chord() {
    let out = arpeggiated(
        &a_chord(),
        ArpSpec {
            octaves: 2,
            ..spec()
        },
    );
    assert_eq!(
        out.iter().take(6).map(|n| n.key).collect::<Vec<_>>(),
        vec![60, 64, 67, 72, 76, 79],
        "two octaves is the chord and the chord an octave up"
    );
    // And nothing runs off the top of the keyboard.
    assert!(out.iter().all(|note| note.key <= 127));
}

#[test]
fn gate_decides_how_long_each_step_sounds() {
    let short = arpeggiated(
        &a_chord(),
        ArpSpec {
            gate: 0.25,
            ..spec()
        },
    );
    let long = arpeggiated(
        &a_chord(),
        ArpSpec {
            gate: 1.0,
            ..spec()
        },
    );
    assert_eq!(short[0].length, PPQN / 16, "a quarter of a sixteenth");
    assert_eq!(long[0].length, PPQN / 4, "a full step");
    assert!(long[0].length > short[0].length * 3);
}

#[test]
fn repeats_hold_each_note_for_more_than_one_step() {
    let out = arpeggiated(
        &a_chord(),
        ArpSpec {
            repeats: 2,
            ..spec()
        },
    );
    assert_eq!(
        out.iter().take(6).map(|n| n.key).collect::<Vec<_>>(),
        vec![60, 60, 64, 64, 67, 67],
        "each pitch twice before the next"
    );
}

/// **Swing**, which FL's arp does not have and every arp part needs: the
/// off-beat steps land late, so the run breathes instead of marching.
#[test]
fn swing_pushes_every_other_step_late() {
    let straight = arpeggiated(&a_chord(), spec());
    let swung = arpeggiated(
        &a_chord(),
        ArpSpec {
            swing: 0.6,
            ..spec()
        },
    );
    assert_eq!(
        straight[0].start, swung[0].start,
        "the downbeat does not move"
    );
    assert!(
        swung[1].start > straight[1].start,
        "the off-beat did not move late"
    );
    assert_eq!(
        straight[2].start, swung[2].start,
        "the next downbeat moved, so the grid drifted"
    );
    // And nothing ends up on top of its neighbour.
    for pair in swung.windows(2) {
        assert!(pair[0].start + pair[0].length <= pair[1].start);
        assert!(pair[1].start > pair[0].start, "swing reordered the run");
    }
}

/// A ramp across the pattern, so the run has a shape. Positive climbs,
/// negative falls, zero is flat.
#[test]
fn a_velocity_ramp_gives_the_run_a_shape() {
    let flat = arpeggiated(&a_chord(), spec());
    assert!(
        flat.iter().all(|note| note.velocity == 100),
        "a ramp of zero changed the weights"
    );
    let up = arpeggiated(
        &a_chord(),
        ArpSpec {
            velocity_ramp: 1.0,
            ..spec()
        },
    );
    assert!(
        up.last().unwrap().velocity > up[0].velocity,
        "the ramp did not climb"
    );
    let down = arpeggiated(
        &a_chord(),
        ArpSpec {
            velocity_ramp: -1.0,
            ..spec()
        },
    );
    assert!(
        down.last().unwrap().velocity < down[0].velocity,
        "the ramp did not fall"
    );
    assert!(
        up.iter().all(|n| n.velocity > 0) && down.iter().all(|n| n.velocity > 0),
        "a ramp silenced a note"
    );
}

/// **Nothing to arpeggiate is nothing done.** A single note is already a run
/// of one, and an empty selection has no chord in it — neither may produce an
/// edit, because an edit that does nothing is still an undo step.
#[test]
fn nothing_to_arpeggiate_makes_no_notes() {
    assert!(arpeggiated(&[], spec()).is_empty());
    let step_of_nothing = ArpSpec { step: 0, ..spec() };
    assert!(
        arpeggiated(&a_chord(), step_of_nothing).is_empty(),
        "a step of no length would be an infinite run"
    );
}

/// Two chords one after the other are two runs, each inside its own span —
/// not one run across the gap between them.
#[test]
fn two_chords_in_sequence_arpeggiate_separately() {
    let mut notes = a_chord();
    for key in [65u8, 69, 72] {
        notes.push(Note {
            start: PPQN * 4,
            length: PPQN * 4,
            key,
            velocity: 100,
            pan: 0,
            fine_pitch: 0,
            release: 0,
            mod_x: 0,
            mod_y: 0,
            slide: false,
            channel: None,
        });
    }
    let out = arpeggiated(&notes, spec());
    let first: Vec<u8> = out
        .iter()
        .filter(|n| n.start < PPQN * 4)
        .take(3)
        .map(|n| n.key)
        .collect();
    let second: Vec<u8> = out
        .iter()
        .filter(|n| n.start >= PPQN * 4)
        .take(3)
        .map(|n| n.key)
        .collect();
    assert_eq!(first, vec![60, 64, 67], "the first chord's run");
    assert_eq!(second, vec![65, 69, 72], "the second chord's own run");
}
