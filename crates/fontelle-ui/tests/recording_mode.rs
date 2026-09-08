//! What the record button records, and the four taps before it starts.
//!
//! Reported from using the window:
//!
//! > *"audio should work similar to fl studio so when i click record it prompts
//! > me what i would like to record: notes, audio from mic, automation, etc.
//! > then after the 4 tap metronome count in it starts recording from whatever
//! > i set my recording track to."*
//!
//! Two decisions, both pure, and both worth having out here rather than inside
//! an event loop where nothing could check them.
//!
//! # The question the button asks
//!
//! Arming used to mean one thing — *keep the notes* — because notes were the
//! only thing there was to keep. There are three now, and which one you meant
//! is not something the button can guess, so it asks. The answer is remembered:
//! somebody recording eight vocal takes should answer once.
//!
//! # The four taps
//!
//! A count-in is **not** a delay before the transport rolls. The transport
//! rolls, the click sounds, and the tape starts a bar later — which is what
//! makes the first beat of the take land on the first beat of the bar rather
//! than a hand's reaction time after it.

use fontelle_ui::transport::{
    COUNT_IN_BEATS, RECORD_MODE_MARK, RecordMode, count_in_samples, record_menu_entries,
    record_menu_entries_for,
};

#[test]
fn the_button_asks_about_all_three_kinds() {
    // *"notes, audio from mic, automation, etc."*
    for wanted in [RecordMode::Notes, RecordMode::Audio, RecordMode::Automation] {
        assert!(
            RecordMode::ALL.contains(&wanted),
            "{wanted:?} cannot be chosen"
        );
    }
    assert_eq!(record_menu_entries().len(), RecordMode::ALL.len());
    for entry in record_menu_entries() {
        assert!(!entry.label.is_empty());
    }
}

#[test]
fn every_mode_says_what_it_is_and_what_it_will_do() {
    for mode in RecordMode::ALL {
        assert!(!mode.label().is_empty(), "{mode:?} has no name");
        assert!(!mode.tip().is_empty(), "{mode:?} says nothing about itself");
        // Distinct: two modes with one name is a menu you cannot answer.
        for other in RecordMode::ALL {
            if other != mode {
                assert_ne!(other.label(), mode.label());
            }
        }
    }
}

#[test]
fn notes_is_what_it_opens_on_because_that_is_what_it_used_to_be() {
    // Arming has meant "keep the notes" since MIDI recording landed, and a
    // build that silently started recording a microphone instead would be a
    // surprise of the worst kind.
    assert_eq!(RecordMode::default(), RecordMode::Notes);
}

// ------------------------------------------------------------ the count ---

#[test]
fn the_count_is_four_taps_of_the_bar_it_is_counting_in() {
    // Four, because that is what was asked for and what every studio does —
    // but four *beats of this bar*, so a count-in in 3/4 is three and a
    // count-in in 6/8 is six. A fixed four over a waltz counts you in wrong.
    assert_eq!(COUNT_IN_BEATS, 4);
    let beat = 24_000i64; // half a second, 120 bpm at 48 kHz
    assert_eq!(count_in_samples(beat, 4), beat * 4);
    assert_eq!(count_in_samples(beat, 3), beat * 3);
    assert_eq!(count_in_samples(beat, 6), beat * 6);
}

#[test]
fn a_project_with_no_tempo_yet_counts_in_nothing_rather_than_dividing_by_it() {
    assert_eq!(count_in_samples(0, 4), 0);
    assert_eq!(count_in_samples(-1, 4), 0);
    assert_eq!(count_in_samples(24_000, 0), 0);
}

#[test]
fn the_count_is_never_so_long_that_it_looks_like_a_hang() {
    // A bar at 20 bpm is twelve seconds. Somebody who set a slow tempo to work
    // out a part and then pressed record would think the button was broken.
    let very_slow_beat = 48_000i64 * 3; // 20 bpm
    let count = count_in_samples(very_slow_beat, 4);
    assert!(
        count <= 48_000 * 8,
        "a count-in of {} samples is {} seconds",
        count,
        count as f64 / 48_000.0
    );
}

// ------------------------------------------------- asking a second time ---

#[test]
fn the_mode_already_chosen_can_be_chosen_again() {
    // Reported from using the window: *"i pressed the record button again but
    // i was locked out of the audio option and i couldnt record again."*
    //
    // The menu used to grey out whichever mode was current, which read as
    // "which one is on" to the person who wrote it and as "you cannot have
    // this" to the person recording a second take. The current mode is
    // **marked**, and every mode stays choosable — choosing it is how you arm
    // again.
    for current in RecordMode::ALL {
        let entries = record_menu_entries_for(current);
        assert_eq!(entries.len(), RecordMode::ALL.len());
        for (entry, mode) in entries.iter().zip(RecordMode::ALL) {
            assert!(entry.enabled, "{current:?} on: {mode:?} was greyed out");
            assert!(
                entry.label.ends_with(mode.label()),
                "{current:?} on: {mode:?} reads {:?}",
                entry.label
            );
            assert_eq!(
                entry.label.starts_with(RECORD_MODE_MARK),
                mode == current,
                "{current:?} on: {mode:?} reads {:?}",
                entry.label
            );
        }
    }
    // And the unmarked list is the marked list with nothing on.
    for (plain, marked) in record_menu_entries()
        .iter()
        .zip(record_menu_entries_for(RecordMode::Notes))
    {
        assert!(marked.label.ends_with(&plain.label));
    }
}
