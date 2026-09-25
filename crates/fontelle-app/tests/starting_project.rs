//! What is in a project the moment it opens.
//!
//! Reported from using the window:
//!
//! > *"please make it so that instead of starting with a 3osc it starts you
//! > with a flopsynth instrument on a grand piano preset and instead of there
//! > being a clip in the arrangement already make there be no clip yet."*
//!
//! Two claims, and they are different in kind. The first is about the
//! *instrument*: a new project should arrive playing something somebody would
//! choose, and a bare three-oscillator saw is what a synth sounds like before
//! anybody has designed anything. The second is about the *arrangement*: a
//! clip is content, and content is authored. A project that hands you one has
//! already made a decision — which row, which instrument, how long — that the
//! person who opened it has not made yet, and every one of those is a thing
//! they then have to undo.

mod common;

use fontelle_app::blank_project;
use fontelle_types::InstrumentKind;

use common::SR;

const BARS: i64 = 8;

#[test]
fn a_new_project_opens_on_flopsynth() {
    let project = blank_project(BARS, 120.0, SR);
    let channel = project
        .channels
        .values()
        .next()
        .expect("a new project has a channel");
    assert_eq!(
        channel.instrument,
        Some(InstrumentKind::Flopsynth),
        "a new project arrives on the built-in synth"
    );
}

/// And on a **named** preset, not the Init patch.
///
/// The channel's name is how the rack says what is playing (see
/// `flopsynth_project`), so the two halves of this claim — that the patch is
/// the preset's and that the row says so — are one assertion each.
#[test]
fn a_new_project_opens_on_the_grand_piano() {
    let project = blank_project(BARS, 120.0, SR);
    let channel = project
        .channels
        .values()
        .next()
        .expect("a new project has a channel");
    assert_eq!(channel.name, "Grand Piano", "the rack says what is playing");

    let data = channel
        .patch_data
        .as_ref()
        .expect("the channel arrives with a patch");
    let patch = fontelle_core::Patch::from_data(data, |_| None)
        .expect("the starting patch must read back")
        .patch;
    let expected = fontelle_core::flopsynth::presets::FACTORY
        .iter()
        .find(|row| row.name == "Grand Piano")
        .map(|row| (row.build)())
        .expect("the bank has a Grand Piano");
    assert_eq!(patch, expected, "it is the bank's Grand Piano, unmodified");
}

/// The arrangement is **empty**.
///
/// Not "one empty clip" — an empty clip is still a clip: it is on a row, it
/// names an instrument, it has a length, and all three are decisions. The rows
/// stay, because a row is room to work in rather than content (see
/// `tests/lanes.rs`).
#[test]
fn a_new_project_has_nothing_in_its_arrangement() {
    let project = blank_project(BARS, 120.0, SR);
    assert_eq!(
        project.clips.len(),
        0,
        "a new project has no clips; the first one is drawn by whoever opened it"
    );
    assert!(
        project.lanes.len() >= 10,
        "the rows are still there — they are room, not content"
    );
}

/// A project with no clips in it still plays, exports and renders.
///
/// The one thing that could have made "no starting clip" a bad idea: every
/// path that compiles the document used to be exercised only against a
/// document that had at least one clip in it.
#[test]
fn an_empty_arrangement_still_compiles_and_renders() {
    let project = blank_project(BARS, 120.0, SR);
    let library = fontelle_app::SampleLibrary::new();
    let (_realised, timeline) =
        common::realise_at(&project, &library, fontelle_dsp::Interpolation::Normal);
    assert!(
        timeline.events.is_empty(),
        "nothing was written, so nothing sounds"
    );
}

/// From a Windows log: eighteen lines of *"no clip ClipId(null) in this
/// project"* — every note drawn in the roll of a new project, which has no
/// clip open, went to a clip that does not exist. The roll with nothing open
/// is refused with one sentence saying what to do, and nothing is changed.
#[test]
fn a_note_drawn_with_no_clip_open_is_refused_with_a_sentence() {
    use fontelle_ui::document::{DocumentHost, StudioHost};
    let mut session = common::a_session_for(blank_project(BARS, 120.0, SR));
    let before = session.project().sync_hash();
    let made = session.edit(fontelle_ui::canvas::RollEdit::Add {
        note: fontelle_model::Note {
            start: 0,
            length: fontelle_types::PPQN,
            key: 60,
            velocity: 100,
            pan: 0,
            fine_pitch: 0,
            release: 0,
            mod_x: 0,
            mod_y: 0,
            slide: false,
            channel: None,
        },
    });
    session.end_gesture();
    assert!(made.is_empty());
    assert_eq!(session.project().sync_hash(), before, "nothing changed");
    let said = session.take_message().expect("it says why");
    assert!(said.contains("clip"), "{said}");
    assert!(!said.contains("null"), "{said}");
}
