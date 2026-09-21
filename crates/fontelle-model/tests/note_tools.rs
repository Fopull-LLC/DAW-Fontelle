//! The piano roll's tools, as commands and as arithmetic.
//!
//! Three things the roll asks for that had no way to say themselves before:
//!
//! - **A relative change.** *"Take everything I have selected and add ten to
//!   its velocity"* — which is not `SetNoteProperty`, because that writes one
//!   value over every note and so flattens exactly the differences the person
//!   is adjusting.
//! - **A value each.** What a randomizer produces: one number per note, all
//!   different, applied as a single undoable edit rather than as N of them.
//! - **The randomizer itself**, which is arithmetic and therefore testable
//!   without a document at all — the reason it is a pure function taking a
//!   seed rather than something that reaches for a random number generator.

use fontelle_model::{
    Arena, Clip, ClipSource, Command, History, Lane, Note, NoteData, NoteProperty,
    NudgeNoteProperty, Project, RandomMode, RandomSpec, SetNotePropertyEach, TempoMap, randomised,
};
use fontelle_types::{ChannelId, ClipId, NoteId, PPQN};

fn a_note(start: i64, key: u8, velocity: u8) -> Note {
    Note {
        start,
        length: PPQN,
        key,
        velocity,
        pan: 0,
        fine_pitch: 0,
        release: 0,
        mod_x: 0,
        mod_y: 0,
        slide: false,
        channel: None,
    }
}

struct Fixture {
    project: Project,
    clip: ClipId,
    notes: Vec<NoteId>,
}

/// A clip of three notes at three different velocities, so a change that
/// flattens them is visible as a failure rather than as a coincidence.
fn fixture() -> Fixture {
    let mut project = Project::new("tools");
    project.tempo_map = TempoMap::new(120.0, 48_000.0);
    let channel: ChannelId = project.channels.insert(fontelle_model::Channel {
        preset: None,
        instrument: None,
        name: "Part".into(),
        color: [0; 4],
        mixer_track: None,
        patch_data: None,
        plugin: None,
        pan: 0.0,
        muted: false,
        soloed: false,
        named_keys: false,
        ab: Default::default(),
        gain_db: 0.0,
    });
    let lane = project.lanes.insert(Lane {
        name: "Lane".into(),
        height: 32.0,
        color: [0; 4],
        muted: false,
        locked: false,
        order: 0,
    });
    let mut notes = Arena::default();
    let ids = vec![
        notes.insert(a_note(0, 60, 40)),
        notes.insert(a_note(PPQN, 64, 100)),
        notes.insert(a_note(PPQN * 2, 67, 120)),
    ];
    let clip = project.clips.insert(Clip {
        lane,
        start: 0,
        length: PPQN * 4,
        source: ClipSource::Notes(NoteData { channel, notes }),
        prefab_link: None,
        color: None,
        muted: false,
        loop_length: None,
    });
    Fixture {
        project,
        clip,
        notes: ids,
    }
}

fn velocities(project: &Project, clip: ClipId, ids: &[NoteId]) -> Vec<u8> {
    let ClipSource::Notes(data) = &project.clips[clip].source else {
        panic!("the fixture's clip holds notes");
    };
    ids.iter().map(|id| data.notes[*id].velocity).collect()
}

fn snapshot(project: &Project) -> serde_json::Value {
    serde_json::to_value(project).expect("a project must serialise")
}

// --------------------------------------------------------------- nudging ---

#[test]
fn a_nudge_moves_every_note_by_the_same_amount_and_keeps_them_apart() {
    // The whole difference from `SetNoteProperty`: the notes were at 40, 100
    // and 120 and they must still be sixty and twenty apart afterwards. A
    // command that wrote one value would make a phrase somebody shaped by
    // hand perfectly flat.
    let mut f = fixture();
    let mut command = NudgeNoteProperty::new(f.clip, f.notes.clone(), NoteProperty::Velocity, 5);
    command.apply(&mut f.project).expect("applies");
    assert_eq!(velocities(&f.project, f.clip, &f.notes), vec![45, 105, 125]);
}

#[test]
fn a_nudge_the_other_way_subtracts() {
    let mut f = fixture();
    let mut command = NudgeNoteProperty::new(f.clip, f.notes.clone(), NoteProperty::Velocity, -10);
    command.apply(&mut f.project).expect("applies");
    assert_eq!(velocities(&f.project, f.clip, &f.notes), vec![30, 90, 110]);
}

#[test]
fn a_nudge_past_the_end_stops_at_the_end_rather_than_being_refused() {
    // Somebody holding the button down means "as loud as it goes", not "an
    // error". The notes that have room keep moving, which is what makes a
    // held press feel like a fader rather than like a wall.
    let mut f = fixture();
    let mut command = NudgeNoteProperty::new(f.clip, f.notes.clone(), NoteProperty::Velocity, 100);
    command.apply(&mut f.project).expect("applies");
    assert_eq!(
        velocities(&f.project, f.clip, &f.notes),
        vec![127, 127, 127]
    );
}

#[test]
fn a_nudge_that_hit_the_end_still_undoes_to_where_each_note_was() {
    // The sharp one. Once three notes have all been clamped to 127 their
    // differences are gone from the document, so the inverse cannot be
    // "subtract 100" — it has to be the values each note actually had.
    let mut f = fixture();
    let before = snapshot(&f.project);
    let mut command: Box<dyn Command> = Box::new(NudgeNoteProperty::new(
        f.clip,
        f.notes.clone(),
        NoteProperty::Velocity,
        100,
    ));
    command.apply(&mut f.project).expect("applies");
    assert_ne!(before, snapshot(&f.project));
    command
        .invert()
        .apply(&mut f.project)
        .expect("the inverse applies");
    assert_eq!(
        before,
        snapshot(&f.project),
        "back exactly where it started"
    );
}

#[test]
fn a_run_of_nudges_is_one_undo_entry() {
    // Holding a stepper down is one edit, not forty. `History::break_gesture`
    // is what ends it, exactly as for a dragged fader.
    let mut f = fixture();
    let mut history = History::new();
    for _ in 0..4 {
        history
            .apply(
                Box::new(NudgeNoteProperty::new(
                    f.clip,
                    f.notes.clone(),
                    NoteProperty::Velocity,
                    1,
                )),
                &mut f.project,
            )
            .expect("applies");
    }
    assert_eq!(history.depth(), 1, "one entry for the whole press");
    assert_eq!(velocities(&f.project, f.clip, &f.notes), vec![44, 104, 124]);

    history
        .undo(&mut f.project)
        .expect("there is something to undo")
        .expect("the undo applies");
    assert_eq!(
        velocities(&f.project, f.clip, &f.notes),
        vec![40, 100, 120],
        "one undo takes the whole run back"
    );
}

#[test]
fn a_nudge_of_nothing_changes_nothing() {
    let mut f = fixture();
    let before = snapshot(&f.project);
    let mut command = NudgeNoteProperty::new(f.clip, f.notes.clone(), NoteProperty::Velocity, 0);
    command.apply(&mut f.project).expect("applies");
    assert_eq!(before, snapshot(&f.project));
}

#[test]
fn a_nudge_naming_a_note_that_is_not_there_is_refused_before_it_writes_anything() {
    // A dead id out of this very clip's arena, which is what a stale
    // selection is: the note was deleted and the roll still names it.
    let mut f = fixture();
    let gone = {
        let ClipSource::Notes(data) = &mut f.project.clips[f.clip].source else {
            panic!("the fixture's clip holds notes");
        };
        let id = data.notes.insert(a_note(0, 72, 100));
        data.notes.remove(id);
        id
    };
    let before = snapshot(&f.project);
    let mut command =
        NudgeNoteProperty::new(f.clip, vec![f.notes[0], gone], NoteProperty::Velocity, 5);
    assert!(command.apply(&mut f.project).is_err());
    assert_eq!(before, snapshot(&f.project), "nothing half-written");
}

#[test]
fn a_nudge_says_what_it_did_in_words() {
    let f = fixture();
    let command = NudgeNoteProperty::new(f.clip, f.notes.clone(), NoteProperty::Pan, 5);
    assert!(
        command.label().contains("pan"),
        "the history entry should name the property: {}",
        command.label()
    );
}

// ---------------------------------------------------------- a value each ---

#[test]
fn a_value_each_writes_a_value_each() {
    let mut f = fixture();
    let mut command = SetNotePropertyEach::new(
        f.clip,
        f.notes.clone(),
        NoteProperty::Velocity,
        vec![10, 20, 30],
    );
    command.apply(&mut f.project).expect("applies");
    assert_eq!(velocities(&f.project, f.clip, &f.notes), vec![10, 20, 30]);
}

#[test]
fn a_value_each_undoes_to_the_values_that_were_there() {
    let mut f = fixture();
    let before = snapshot(&f.project);
    let mut command: Box<dyn Command> = Box::new(SetNotePropertyEach::new(
        f.clip,
        f.notes.clone(),
        NoteProperty::Velocity,
        vec![10, 20, 30],
    ));
    command.apply(&mut f.project).expect("applies");
    command.invert().apply(&mut f.project).expect("inverts");
    assert_eq!(before, snapshot(&f.project));
}

#[test]
fn a_value_each_clamps_rather_than_refusing() {
    let mut f = fixture();
    let mut command = SetNotePropertyEach::new(
        f.clip,
        f.notes.clone(),
        NoteProperty::Velocity,
        vec![-50, 500, 60],
    );
    command.apply(&mut f.project).expect("applies");
    assert_eq!(velocities(&f.project, f.clip, &f.notes), vec![1, 127, 60]);
}

#[test]
fn a_value_each_with_the_wrong_number_of_values_is_refused() {
    // A list that does not line up with its notes would write the second
    // note's value onto the third, which is a scramble rather than an error.
    let mut f = fixture();
    let before = snapshot(&f.project);
    let mut command = SetNotePropertyEach::new(
        f.clip,
        f.notes.clone(),
        NoteProperty::Velocity,
        vec![10, 20],
    );
    assert!(command.apply(&mut f.project).is_err());
    assert_eq!(before, snapshot(&f.project));
}

#[test]
fn two_rolls_of_the_randomizer_are_two_undo_entries() {
    // Unlike a nudge: rolling again is a new answer to the same question, and
    // undo should walk back through them one at a time.
    let mut f = fixture();
    let mut history = History::new();
    for values in [vec![10, 20, 30], vec![40, 50, 60]] {
        history
            .apply(
                Box::new(SetNotePropertyEach::new(
                    f.clip,
                    f.notes.clone(),
                    NoteProperty::Velocity,
                    values,
                )),
                &mut f.project,
            )
            .expect("applies");
    }
    assert_eq!(history.depth(), 2);
}

// -------------------------------------------------------- the randomizer ---

/// The three velocities the fixture holds, as the randomizer sees them.
const STARTING: [i32; 3] = [40, 100, 120];

#[test]
fn the_same_seed_gives_the_same_answer_every_time() {
    // Which is what makes a randomizer testable, undoable and re-rollable:
    // the roll keeps a seed and steps it, rather than a hidden generator
    // nobody can ask about.
    let spec = RandomSpec {
        amount: 50,
        mode: RandomMode::Around,
    };
    let once = randomised(&STARTING, NoteProperty::Velocity, spec, 12345);
    let twice = randomised(&STARTING, NoteProperty::Velocity, spec, 12345);
    assert_eq!(once, twice);
}

#[test]
fn a_different_seed_gives_a_different_answer() {
    let spec = RandomSpec {
        amount: 50,
        mode: RandomMode::Around,
    };
    let once = randomised(&STARTING, NoteProperty::Velocity, spec, 1);
    let twice = randomised(&STARTING, NoteProperty::Velocity, spec, 2);
    assert_ne!(once, twice, "re-rolling has to actually re-roll");
}

#[test]
fn no_amount_changes_nothing_whatever_the_seed() {
    // The identity, at both ends of the dial: a tool whose zero is not the
    // identity is one you cannot back away from.
    for seed in [0, 1, 999, u64::MAX] {
        for mode in [RandomMode::Around, RandomMode::Anywhere] {
            let out = randomised(
                &STARTING,
                NoteProperty::Velocity,
                RandomSpec { amount: 0, mode },
                seed,
            );
            assert_eq!(out, STARTING.to_vec(), "mode {mode:?}, seed {seed}");
        }
    }
}

#[test]
fn every_value_it_produces_is_one_the_property_can_hold() {
    // A randomizer that can produce a velocity of 0 is one that can silently
    // delete a note, since a note-on at velocity 0 is a note-off.
    for property in [
        NoteProperty::Velocity,
        NoteProperty::Pan,
        NoteProperty::FinePitch,
        NoteProperty::Release,
        NoteProperty::ModX,
        NoteProperty::ModY,
    ] {
        let (min, max) = property.range();
        let starting: Vec<i32> = (0..64).map(|i| min + (max - min) * i / 63).collect();
        for mode in [RandomMode::Around, RandomMode::Anywhere] {
            for seed in 0..16u64 {
                let out = randomised(&starting, property, RandomSpec { amount: 100, mode }, seed);
                assert_eq!(out.len(), starting.len());
                for value in out {
                    assert!(
                        (min..=max).contains(&value),
                        "{} produced {value}, outside {min}..={max}",
                        property.label()
                    );
                }
            }
        }
    }
}

#[test]
fn around_stays_near_where_each_note_was() {
    // The humanising mode: a phrase you shaped by hand keeps its shape, and
    // every note wobbles. A small amount must not be able to send a quiet
    // note to the top of the range.
    let spec = RandomSpec {
        amount: 10,
        mode: RandomMode::Around,
    };
    let (min, max) = NoteProperty::Velocity.range();
    let reach = (max - min) * spec.amount / 100;
    for seed in 0..64u64 {
        let out = randomised(&STARTING, NoteProperty::Velocity, spec, seed);
        for (was, now) in STARTING.iter().zip(&out) {
            assert!(
                (now - was).abs() <= reach,
                "{was} moved to {now}, further than {reach}"
            );
        }
    }
}

#[test]
fn anywhere_at_full_amount_ignores_where_the_note_was() {
    // The other mode: not a wobble but a re-roll. The claim that separates it
    // from `Around` is that the *starting value stops mattering* — two very
    // different notes get exactly the same answer from the same seed.
    let spec = RandomSpec {
        amount: 100,
        mode: RandomMode::Anywhere,
    };
    let quiet: Vec<i32> = (0..128u64)
        .map(|seed| randomised(&[1], NoteProperty::Velocity, spec, seed)[0])
        .collect();
    let loud: Vec<i32> = (0..128u64)
        .map(|seed| randomised(&[127], NoteProperty::Velocity, spec, seed)[0])
        .collect();
    assert_eq!(
        quiet, loud,
        "at a full amount the answer cannot depend on what was there"
    );
}

#[test]
fn a_half_amount_of_anywhere_lands_between_the_note_and_the_roll() {
    // The dial has to be continuous, or it is two tools with a jump in the
    // middle. At half, a note is half-way between where it was and where a
    // full re-roll would have put it.
    let full = randomised(
        &STARTING,
        NoteProperty::Velocity,
        RandomSpec {
            amount: 100,
            mode: RandomMode::Anywhere,
        },
        7,
    );
    let half = randomised(
        &STARTING,
        NoteProperty::Velocity,
        RandomSpec {
            amount: 50,
            mode: RandomMode::Anywhere,
        },
        7,
    );
    for ((was, all), part) in STARTING.iter().zip(&full).zip(&half) {
        let expected = was + (all - was) / 2;
        assert!(
            (part - expected).abs() <= 1,
            "from {was} towards {all}, half way is about {expected}, got {part}"
        );
    }
}

#[test]
fn randomising_nothing_produces_nothing() {
    let out = randomised(
        &[],
        NoteProperty::Velocity,
        RandomSpec {
            amount: 100,
            mode: RandomMode::Around,
        },
        1,
    );
    assert!(out.is_empty());
}

#[test]
fn an_amount_over_the_top_of_the_dial_is_the_top_of_the_dial() {
    let spec = RandomSpec {
        amount: 400,
        mode: RandomMode::Anywhere,
    };
    let capped = RandomSpec {
        amount: 100,
        mode: RandomMode::Anywhere,
    };
    assert_eq!(
        randomised(&STARTING, NoteProperty::Velocity, spec, 3),
        randomised(&STARTING, NoteProperty::Velocity, capped, 3),
    );
}

#[test]
fn two_notes_that_started_the_same_do_not_stay_the_same() {
    // Every note gets its own roll. A generator seeded once per *command*
    // rather than once per note would move a chord as a block, which is the
    // one thing a humaniser must not do.
    let same = [100, 100, 100, 100, 100, 100, 100, 100];
    let out = randomised(
        &same,
        NoteProperty::Velocity,
        RandomSpec {
            amount: 60,
            mode: RandomMode::Around,
        },
        42,
    );
    assert!(
        out.iter().any(|value| *value != out[0]),
        "identical notes must not all get the same answer: {out:?}"
    );
}

// ------------------------------------------------------------- legato ---
//
// > *"if i press ctrl l with a note selection in the piano roll it makes all
// > the notes lengths not have gaps like how it does in fl studio with that
// > same keybind. just makes all the notes cleanly connect to eachother
// > basically in length."*
//
// FL's Quick Legato. The arithmetic is here and pure, so "what does a chord
// do" and "what does the last note do" are answered once rather than in a
// window nobody can test.

use fontelle_model::{SetNoteLengths, legato_lengths};
use fontelle_types::Tick;

/// `(start, length)` pairs, the shape the tool takes.
fn spans(pairs: &[(Tick, Tick)]) -> Vec<(Tick, Tick)> {
    pairs.to_vec()
}

#[test]
fn a_run_of_notes_is_stretched_until_each_one_touches_the_next() {
    // Three sixteenths a beat apart: each becomes a beat long, and the gaps
    // between them close.
    let out = legato_lengths(&spans(&[
        (0, PPQN / 4),
        (PPQN, PPQN / 4),
        (PPQN * 2, PPQN / 4),
    ]));
    assert_eq!(out, vec![PPQN, PPQN, PPQN / 4]);
}

#[test]
fn a_note_that_overlaps_the_next_is_pulled_back_to_it() {
    // Legato is "touch", not "at least touch": a note running under the one
    // after it is shortened, or the tool could only ever add and a phrase you
    // ran through it twice would keep growing.
    let out = legato_lengths(&spans(&[(0, PPQN * 4), (PPQN, PPQN / 4)]));
    assert_eq!(out, vec![PPQN, PPQN / 4]);
}

#[test]
fn the_last_note_keeps_the_length_it_had() {
    // There is nothing after it to touch, and guessing a length for it — the
    // previous gap, a beat, the clip's end — would be the tool inventing
    // something nobody asked for.
    let out = legato_lengths(&spans(&[(0, PPQN / 8), (PPQN * 2, PPQN * 3)]));
    assert_eq!(out, vec![PPQN * 2, PPQN * 3]);
}

#[test]
fn a_chord_moves_as_one_because_its_notes_share_a_start() {
    // Three notes at the same tick are one musical event: they all reach the
    // next event, and none of them is "the next note" for the other two.
    let out = legato_lengths(&spans(&[
        (0, PPQN / 4),
        (0, PPQN / 2),
        (0, PPQN / 8),
        (PPQN * 2, PPQN / 4),
    ]));
    assert_eq!(out, vec![PPQN * 2, PPQN * 2, PPQN * 2, PPQN / 4]);
}

#[test]
fn the_order_the_notes_arrive_in_does_not_change_the_answer() {
    // The window hands over whatever order the selection is in, which is the
    // order they were clicked. Sorting is the tool's job.
    let forwards = legato_lengths(&spans(&[(0, 10), (PPQN, 10), (PPQN * 2, 10)]));
    let backwards = legato_lengths(&spans(&[(PPQN * 2, 10), (PPQN, 10), (0, 10)]));
    assert_eq!(backwards, vec![10, PPQN, PPQN]);
    assert_eq!(forwards, vec![PPQN, PPQN, 10]);
}

#[test]
fn one_note_and_no_notes_are_both_left_alone() {
    assert_eq!(legato_lengths(&spans(&[(PPQN, PPQN / 4)])), vec![PPQN / 4]);
    assert!(legato_lengths(&[]).is_empty());
}

#[test]
fn no_note_is_ever_shortened_to_nothing() {
    // A note of zero length is a note-on and a note-off on the same sample,
    // which `SetNoteLengths` refuses and the sampler cannot sound. Distinct
    // starts are at least a tick apart, so this holds by arithmetic — the
    // test is here so it stays that way.
    let out = legato_lengths(&spans(&[(0, PPQN), (1, PPQN), (2, PPQN)]));
    assert!(out.iter().all(|length| *length >= 1), "{out:?}");
    assert_eq!(out, vec![1, 1, PPQN]);
}

#[test]
fn setting_lengths_writes_each_one_and_undo_puts_them_all_back() {
    let mut f = fixture();
    let before: Vec<Tick> = f
        .notes
        .iter()
        .map(|id| notes_of(&f.project, f.clip).get(*id).unwrap().length)
        .collect();
    let mut history = History::new();
    history
        .apply(
            Box::new(SetNoteLengths::new(
                f.clip,
                f.notes.clone(),
                vec![PPQN * 2, PPQN / 2, PPQN * 3],
            )),
            &mut f.project,
        )
        .expect("lengths are writable");
    let after: Vec<Tick> = f
        .notes
        .iter()
        .map(|id| notes_of(&f.project, f.clip).get(*id).unwrap().length)
        .collect();
    assert_eq!(after, vec![PPQN * 2, PPQN / 2, PPQN * 3]);

    history
        .undo(&mut f.project)
        .expect("one entry to undo")
        .expect("and it inverts");
    let back: Vec<Tick> = f
        .notes
        .iter()
        .map(|id| notes_of(&f.project, f.clip).get(*id).unwrap().length)
        .collect();
    assert_eq!(back, before, "one gesture, one undo");
}

#[test]
fn a_length_of_nothing_is_refused_rather_than_written() {
    let mut f = fixture();
    let mut command = SetNoteLengths::new(f.clip, f.notes.clone(), vec![PPQN, 0, PPQN]);
    assert!(
        command.apply(&mut f.project).is_err(),
        "a note of zero length is a note-on and note-off on the same sample"
    );
    assert_eq!(
        notes_of(&f.project, f.clip).get(f.notes[0]).unwrap().length,
        PPQN,
        "and nothing was written before it refused"
    );
}

#[test]
fn a_list_that_does_not_line_up_with_its_notes_is_refused() {
    let mut f = fixture();
    let mut command = SetNoteLengths::new(f.clip, f.notes.clone(), vec![PPQN, PPQN]);
    assert!(command.apply(&mut f.project).is_err());
}

/// The clip's notes, for reading a length back out.
fn notes_of(project: &Project, clip: ClipId) -> &Arena<NoteId, Note> {
    let ClipSource::Notes(data) = &project.clips.get(clip).unwrap().source else {
        panic!("a note clip")
    };
    &data.notes
}
