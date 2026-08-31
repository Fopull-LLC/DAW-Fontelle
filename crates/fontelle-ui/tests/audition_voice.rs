//! The one note the mouse is holding down, as a state machine.
//!
//! Reported from using the window, twice:
//!
//! - *"when placing a note sometimes it would play a different note on hold
//!   that wouldn't stop until I replayed again."* A hung voice, and here is
//!   how it got hung. `Sampler::note_off` releases **one** voice — the first
//!   active one matching the key and the voice context. The window's audition
//!   only sent a note-off when the *key changed*, so sounding the same key
//!   twice sent two note-ons and, on the way back up, one note-off. The second
//!   voice was never addressed again and sustained until something else in the
//!   engine happened to take it.
//! - *"clicking a note on the piano roll I'm still not hearing it cleanly,
//!   just a flicker."* The minimum was a flat 180 ms for everything, so a
//!   click on a half note sounded for a fifth of a beat. What you click should
//!   sound for as long as it *is*.
//!
//! Both live here rather than in `WindowApp` because neither of them is about
//! having a window: it is bookkeeping over two note events and a clock, which
//! is §2.5's definition of something that belongs in a pure function with the
//! tests beside it.

use std::time::{Duration, Instant};

use fontelle_ui::audition::{AuditionAction, Auditions, MAX_AUDITION, MIN_AUDITION};

fn on(key: u8) -> AuditionAction {
    AuditionAction::On {
        key,
        velocity: 100,
        pan: 0,
    }
}

fn off(key: u8) -> AuditionAction {
    AuditionAction::Off { key }
}

// ------------------------------------------------------- the hung voice ---

#[test]
fn sounding_the_same_key_twice_releases_the_first_voice_first() {
    // The bug, exactly. Two note-ons and one note-off leaves a voice sounding
    // for ever, because a note-off only ever releases one of them.
    let mut voice = Auditions::default();
    let t0 = Instant::now();

    assert_eq!(voice.start(60, 100, 0, MIN_AUDITION, t0), vec![on(60)]);

    let again = voice.start(60, 100, 0, MIN_AUDITION, t0 + Duration::from_millis(500));
    assert_eq!(
        again,
        vec![off(60), on(60)],
        "the key already sounding has to be let go before it is struck again, \
         or the first voice is one nobody can address"
    );
}

#[test]
fn every_note_on_is_matched_by_exactly_one_note_off() {
    // The property the bug broke, said directly and over a whole session of
    // clicking: whatever the sequence, no key is left holding voices.
    let mut voice = Auditions::default();
    let mut now = Instant::now();
    let mut held: std::collections::HashMap<u8, i32> = std::collections::HashMap::new();

    let record = |actions: Vec<AuditionAction>, held: &mut std::collections::HashMap<u8, i32>| {
        for action in actions {
            match action {
                AuditionAction::On { key, .. } => *held.entry(key).or_default() += 1,
                AuditionAction::Off { key } => *held.entry(key).or_default() -= 1,
            }
        }
    };

    for key in [60, 60, 64, 64, 64, 67, 60] {
        record(voice.start(key, 100, 0, MIN_AUDITION, now), &mut held);
        now += Duration::from_millis(20);
        voice.release(now);
        now += Duration::from_millis(5);
        // Half the time the next click comes before the release is even due,
        // which is what a person double-clicking a note actually does.
        record(voice.settle(now).into_iter().collect(), &mut held);
        now += Duration::from_millis(30);
    }
    record(voice.silence().into_iter().collect(), &mut held);

    for (key, count) in held {
        assert_eq!(count, 0, "key {key} was left holding {count} voice(s)");
    }
}

#[test]
fn a_new_note_cancels_the_release_the_old_one_had_scheduled() {
    // Otherwise the release lands after the new note-on and cuts it off — the
    // other half of "just a flicker".
    let mut voice = Auditions::default();
    let t0 = Instant::now();
    voice.start(60, 100, 0, MIN_AUDITION, t0);
    voice.release(t0 + Duration::from_millis(10));

    let started = voice.start(64, 100, 0, MIN_AUDITION, t0 + Duration::from_millis(20));
    assert_eq!(started, vec![off(60), on(64)]);
    assert_eq!(
        voice.settle(t0 + Duration::from_secs(10)),
        None,
        "the old note's release must not fire against the new note"
    );
    assert_eq!(voice.due(), None, "and nothing is left scheduled");
}

// ------------------------------------------------------ the flicker ---

#[test]
fn a_click_sounds_for_the_length_of_what_was_clicked() {
    // A quick click on a half note has to sound like a half note, not like
    // 180 ms of the front of one.
    let mut voice = Auditions::default();
    let t0 = Instant::now();
    let half = Duration::from_millis(1000);
    voice.start(60, 100, 0, half, t0);
    voice.release(t0 + Duration::from_millis(30));

    assert_eq!(voice.due(), Some(t0 + half));
    assert_eq!(voice.settle(t0 + Duration::from_millis(900)), None);
    assert_eq!(voice.settle(t0 + half), Some(off(60)));
    assert_eq!(voice.settle(t0 + Duration::from_secs(5)), None, "once");
}

#[test]
fn holding_the_button_down_holds_the_note() {
    // The minimum is a floor, not a length.
    let mut voice = Auditions::default();
    let t0 = Instant::now();
    voice.start(60, 100, 0, MIN_AUDITION, t0);
    assert_eq!(voice.settle(t0 + Duration::from_secs(30)), None);
    assert_eq!(voice.key(), Some(60));

    let held_until = t0 + Duration::from_secs(30);
    voice.release(held_until);
    assert_eq!(voice.settle(held_until), Some(off(60)));
}

#[test]
fn a_hold_is_never_shorter_than_the_minimum_or_longer_than_the_maximum() {
    let mut voice = Auditions::default();
    let t0 = Instant::now();

    // A thirty-second at speed is still not a click.
    voice.start(60, 100, 0, Duration::from_millis(1), t0);
    voice.release(t0);
    assert_eq!(voice.due(), Some(t0 + MIN_AUDITION));
    voice.settle(t0 + MIN_AUDITION);

    // And clicking a note four bars long does not leave it droning.
    voice.start(60, 100, 0, Duration::from_secs(60), t0);
    voice.release(t0);
    assert_eq!(voice.due(), Some(t0 + MAX_AUDITION));
}

#[test]
fn releasing_a_note_that_has_already_outstayed_its_hold_stops_it_at_once() {
    let mut voice = Auditions::default();
    let t0 = Instant::now();
    voice.start(60, 100, 0, MIN_AUDITION, t0);
    let late = t0 + Duration::from_secs(2);
    voice.release(late);
    assert_eq!(voice.settle(late), Some(off(60)));
}

// ------------------------------------------------------------ the rest ---

#[test]
fn silence_stops_whatever_is_sounding_and_forgets_it() {
    // What the window calls when the compositor takes the pointer away — an
    // alt-tab mid-gesture must not leave a note ringing.
    let mut voice = Auditions::default();
    let t0 = Instant::now();
    voice.start(72, 100, 0, MIN_AUDITION, t0);
    assert_eq!(voice.silence(), Some(off(72)));
    assert_eq!(voice.silence(), None, "and there is nothing left to stop");
    assert_eq!(voice.key(), None);
    assert_eq!(voice.due(), None);
}

#[test]
fn releasing_when_nothing_is_sounding_schedules_nothing() {
    let mut voice = Auditions::default();
    let t0 = Instant::now();
    voice.release(t0);
    assert_eq!(voice.due(), None);
    assert_eq!(voice.settle(t0 + Duration::from_secs(1)), None);
}

#[test]
fn the_velocity_asked_for_is_the_velocity_sounded() {
    let mut voice = Auditions::default();
    assert_eq!(
        voice.start(48, 17, 0, MIN_AUDITION, Instant::now()),
        vec![AuditionAction::On {
            key: 48,
            velocity: 17,
            pan: 0,
        }]
    );
}

/// The last leg of §16.5's per-note pan. A note's pan reaches the *timeline*
/// through the compiled event stream; clicking that same note in the roll goes
/// down the live path instead, and a live path that centres everything means
/// the only way to hear what you just drew is to press play.
#[test]
fn an_audition_carries_the_pan_of_the_note_it_is_sounding() {
    let mut auditions = Auditions::default();
    let actions = auditions.start(60, 100, -100, Duration::from_millis(200), Instant::now());

    assert_eq!(
        actions,
        vec![AuditionAction::On {
            key: 60,
            velocity: 100,
            pan: -100,
        }]
    );
}

/// And the note-off does not carry one, because it does not need one: a
/// release addresses a voice that already knows where it is.
#[test]
fn a_release_is_still_just_a_key() {
    let mut auditions = Auditions::default();
    let now = Instant::now();
    auditions.start(60, 100, -100, Duration::ZERO, now);
    auditions.release(now);

    assert_eq!(
        auditions.settle(now + MAX_AUDITION),
        Some(AuditionAction::Off { key: 60 })
    );
}
