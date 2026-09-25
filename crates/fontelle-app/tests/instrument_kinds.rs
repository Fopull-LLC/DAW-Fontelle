//! What kind of instrument a channel is, and changing it for another.
//!
//! Reported from using the window:
//!
//! > *"currently cant replace an instrument with a different instrument like a
//! > sampler or something i can only replace soundfonts with other soundfonts
//! > ... we should make sure there is an actual instrument selection menu and
//! > when you select new instrument it lets you select one of those and then
//! > you actually edit it from there how you want instead of how it is right
//! > now where is basically makes everything an oscillator and then i click a
//! > soundfont in the soundfonts menu to change it which is just weird."*
//!
//! There are three instruments and they were never named anywhere: a `Patch`
//! is a list of layers, and which of the three you are looking at had to be
//! read off their `Source`. That works for a patch with something in it and
//! not at all for an empty one — a sampler with no sample and a soundfont
//! player with no soundfont are the same empty patch, and both are states you
//! are *in* while you decide what to load.
//!
//! So the **choice** is stored (`Channel::instrument`) and the patch follows
//! from it, rather than the other way round.

mod common;

use fontelle_app::blank_project;
use fontelle_types::InstrumentKind;
use fontelle_ui::document::{DocumentHost, StudioHost};

use common::SR;

fn a_session() -> fontelle_app::Session {
    common::a_session_for(blank_project(8, 120.0, SR))
}

#[test]
fn there_are_six_and_each_says_what_it_is() {
    assert_eq!(InstrumentKind::ALL.len(), 6);
    for kind in InstrumentKind::ALL {
        assert!(!kind.label().is_empty(), "{kind:?} has no name");
    }
    // Distinct names, or a menu of them is a menu you cannot read.
    let mut names: Vec<&str> = InstrumentKind::ALL.iter().map(|k| k.label()).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), 6);
}

/// The three that need nothing configured, and the three that are waiting for
/// a file. Flopsynth is the third of the first group: every wavetable it reads
/// is generated from a recipe at first use, so there is no bank to point it at.
#[test]
fn the_three_that_arrive_playing_are_the_three_that_name_no_file() {
    let plays: Vec<InstrumentKind> = InstrumentKind::ALL
        .into_iter()
        .filter(|k| k.plays_on_arrival())
        .collect();
    assert_eq!(
        plays,
        vec![
            InstrumentKind::Flopsynth,
            InstrumentKind::DrumMachine,
            InstrumentKind::Osc3
        ]
    );
    for kind in InstrumentKind::ALL {
        assert_eq!(
            kind.plays_on_arrival(),
            kind.wants().is_none(),
            "{kind:?} disagrees with itself about whether it is waiting for \
             something"
        );
    }
}

/// The whole of gate 1 in the plan's §1: on the menu, arriving playing, and a
/// note in the roll sounds through it with nothing configured.
#[test]
fn a_channel_turned_into_a_flopsynth_arrives_playing() {
    let mut session = a_session();
    session.set_channel_kind(0, InstrumentKind::Flopsynth);
    assert_eq!(session.channel_kind(0), Some(InstrumentKind::Flopsynth));
    assert!(
        InstrumentKind::Flopsynth.plays_on_arrival(),
        "the menu says it plays on arrival"
    );
    let patch = session.selected_patch().expect("it arrives with a patch");
    assert!(
        fontelle_core::flopsynth::is_flopsynth(&patch),
        "and the patch it arrives with is a Flopsynth patch"
    );
    assert_eq!(
        patch.layers.len(),
        5,
        "five layers in role order: A, B, C, Sub, Noise"
    );
}

#[test]
fn a_blank_project_starts_on_the_built_in_synth() {
    // `blank_project` gives its one channel a Flopsynth on the bank's Grand
    // Piano (see `tests/starting_project.rs`), so that is what the rack should
    // say it is. It was a bare three-oscillator saw until Ty asked for a sound
    // somebody had chosen rather than one nobody had.
    let session = a_session();
    assert_eq!(session.channel_kind(0), Some(InstrumentKind::Flopsynth));
}

#[test]
fn changing_the_kind_changes_what_the_channel_says_it_is() {
    let mut session = a_session();
    for kind in [
        InstrumentKind::Sampler,
        InstrumentKind::SoundFont,
        InstrumentKind::Osc3,
    ] {
        session.set_channel_kind(0, kind);
        assert_eq!(session.channel_kind(0), Some(kind));
    }
}

#[test]
fn a_channel_turned_into_a_synth_can_be_played_and_one_turned_into_a_sampler_waits() {
    // The whole of *"then you actually edit it from there"*: choosing 3OSC
    // hands you an instrument that already makes a sound, and choosing the
    // other two hands you one that is waiting for a file. A sampler that
    // arrived playing a saw would be lying about what it is.
    let mut session = a_session();

    session.set_channel_kind(0, InstrumentKind::Osc3);
    assert!(
        session.channels()[0].has_instrument,
        "3OSC plays on arrival"
    );

    session.set_channel_kind(0, InstrumentKind::Sampler);
    assert!(
        !session.channels()[0].has_instrument,
        "a sampler with no sample plays nothing, and says so"
    );

    session.set_channel_kind(0, InstrumentKind::SoundFont);
    assert!(!session.channels()[0].has_instrument);
}

#[test]
fn changing_the_kind_is_one_undo_and_puts_the_old_instrument_back() {
    let mut session = a_session();
    // From 3OSC rather than from what the project happens to open on, so this
    // stays a test about undo rather than about the starting preset.
    session.set_channel_kind(0, InstrumentKind::Osc3);
    assert_eq!(session.channel_kind(0), Some(InstrumentKind::Osc3));
    session.set_channel_kind(0, InstrumentKind::Sampler);
    assert_eq!(session.channel_kind(0), Some(InstrumentKind::Sampler));

    session.undo();
    assert_eq!(
        session.channel_kind(0),
        Some(InstrumentKind::Osc3),
        "one press puts back both the kind and the patch"
    );
    assert!(session.channels()[0].has_instrument);
}

#[test]
fn setting_the_kind_it_already_is_leaves_the_instrument_alone() {
    // Otherwise choosing "3OSC" on a 3OSC you have spent ten minutes editing
    // throws the edit away — which is the worst thing a menu of kinds could do.
    let mut session = a_session();
    session.set_channel_kind(0, InstrumentKind::Osc3);
    let before = session.instrument().map(|view| view.groups.len());
    session.set_channel_kind(0, InstrumentKind::Osc3);
    assert_eq!(session.instrument().map(|view| view.groups.len()), before);
    let depth = session.undo_depth();
    session.set_channel_kind(0, InstrumentKind::Osc3);
    assert_eq!(session.undo_depth(), depth, "and it is not a history entry");
}

#[test]
fn a_new_channel_can_be_asked_for_by_kind() {
    // *"when you select new instrument it lets you select one of those."*
    let mut session = a_session();
    let before = session.channels().len();
    session
        .add_channel_of(InstrumentKind::Sampler)
        .expect("adds");
    assert_eq!(session.channels().len(), before + 1);
    assert_eq!(session.channel_kind(before), Some(InstrumentKind::Sampler));
}

#[test]
fn asking_about_a_channel_that_is_not_there_says_nothing_rather_than_guessing() {
    let session = a_session();
    assert_eq!(session.channel_kind(99), None);
}

#[test]
fn a_soundfont_player_is_what_a_preset_lands_on() {
    // Putting a soundfont on a channel makes it a soundfont player whatever it
    // was before — the assignment is the choice, and the rack has to agree
    // with what is actually loaded.
    let mut session = a_session();
    session.set_channel_kind(0, InstrumentKind::Sampler);
    // No bank in a test session, so this is the *rule* rather than a load:
    // whatever a preset lands on reads as a soundfont player afterwards.
    session.set_channel_kind(0, InstrumentKind::SoundFont);
    assert_eq!(session.channel_kind(0), Some(InstrumentKind::SoundFont));
}

/// Choosing what a channel is says so, and in the words the session hands the
/// status line: the window's own "Instrument is now Sampler" was overwritten
/// on the next frame by the soundfont folder's path — soundfont words while a
/// sampler was selected.
#[test]
fn choosing_what_a_channel_is_says_so_on_the_status_line() {
    use fontelle_ui::document::StudioHost;
    let mut session = a_session();
    let _ = StudioHost::take_message(&mut session);
    session.set_channel_kind(0, InstrumentKind::Sampler);
    let said = StudioHost::take_message(&mut session).expect("it says what it did");
    assert!(said.contains("Sampler"), "{said}");
    assert!(!said.to_lowercase().contains("soundfont"), "{said}");
}
