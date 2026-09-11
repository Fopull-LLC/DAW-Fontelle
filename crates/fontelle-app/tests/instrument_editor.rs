//! The instrument panel's own controls, driven through the trait the window
//! drives them through.
//!
//! Reported from using the studio: *"i noticed changing the instrument settings
//! for one instrument was actually affecting the wrong instrument, so i changed
//! the volume on my hold choir channel instrument and it was changing the volume
//! on my bright yamaha piano grand that had chords in another clip."*
//!
//! Not a mix-up over *which channel is selected* — the panel had the right one
//! all along. The panel's volume and pan were the **mixer track's**, and a
//! channel with no track of its own falls back to the master (see
//! `Channel::mixer_track`, which is `None` for every channel a soundfont is
//! dropped on). Two channels on the master are two panels writing to one
//! fader, so turning either one down turned the whole song down and the other
//! panel's read-out moved with it.
//!
//! A channel's level and placement are the channel's, beside the `pan` that has
//! always lived there — and they reach the sound at the sampler, before the bus,
//! which is also what makes them independent of how many channels share a track.

mod common;

use fontelle_app::{RealiseOptions, SampleLibrary, Session};
use fontelle_engine::{graph_channel, timeline_channel};
use fontelle_types::CompiledTimeline;
use fontelle_ui::document::StudioHost;

/// The automation block in hand, as the arrangement draws it.
///
/// The curve editor's window is gone: an automation clip is edited inside its
/// own block now (`fontelle-ui/tests/automation_blocks.rs`), so what a test
/// reads is the same flattened block the canvas draws.
fn open_lane(session: &fontelle_app::Session) -> fontelle_ui::document::ClipInfo {
    use fontelle_ui::document::{ClipKind, StudioHost};
    session
        .clips()
        .into_iter()
        .find(|clip| clip.kind == ClipKind::Automation && clip.open)
        .expect("the clip that was just made is the block in hand")
}

use common::SR;

/// A studio with two channels on the master, which is where every channel goes
/// until somebody routes it somewhere else. The second is added the way the
/// rack's button adds one — blank, playing the built-in synth.
fn two_channels() -> Session {
    let project = common::a_project_with_a_clip(8, 120.0, SR);
    let clip = Session::first_clip(&project).expect("a blank project has one clip");
    let channel_nodes = fontelle_app::channel_nodes(&project);
    let (publisher, _timeline) = timeline_channel(CompiledTimeline::empty());
    let options = RealiseOptions {
        sample_rate: SR,
        block_size: fontelle_engine::BLOCK_SIZE,
        quality: fontelle_app::PLAYBACK_QUALITY,
    };
    let library = SampleLibrary::new();
    let realised =
        fontelle_app::realise(&project, &library, options).expect("a blank project must realise");
    let (graphs, _source) = graph_channel(realised.graph);
    let mut session = Session::new(
        project,
        library,
        channel_nodes,
        publisher,
        options,
        clip,
        None,
    )
    .with_graphs(graphs, realised.track_controls)
    .with_param_nodes(realised.param_nodes);
    session
        .add_channel()
        .expect("the rack's button makes a channel out of nothing");
    session
}

/// The button, on its own terms: a blank channel comes up playing something.
#[test]
fn a_new_channel_plays_the_built_in_synth() {
    let session = two_channels();
    let channels = session.channels();
    assert_eq!(channels.len(), 2, "the button made one");
    assert!(
        channels.iter().all(|c| c.has_instrument),
        "and it is not a silent row: {channels:?}"
    );
    assert_eq!(
        session.selected_channel(),
        1,
        "the new channel is the selected one — you made it to play it"
    );
    let view = session
        .instrument()
        .expect("a blank channel has a panel of knobs, because it has an instrument");
    assert!(
        view.groups.iter().any(|g| g.name == "Oscillators"),
        "and the panel has the synth's oscillators on it: {:?}",
        view.groups.iter().map(|g| &g.name).collect::<Vec<_>>()
    );
}

/// What the panel reads back for `address` on whichever channel is selected.
fn shown(session: &Session, address: &str) -> f32 {
    let view = session
        .instrument()
        .expect("the selected channel has a panel");
    view.groups
        .iter()
        .flat_map(|group| group.params.iter())
        .find(|param| param.address.as_str() == address)
        .unwrap_or_else(|| panic!("{address} is on the panel"))
        .value
}

// ------------------------------------------------- one channel at a time ---

/// The report. Turn one channel's volume down; the other one must not move.
#[test]
fn one_channels_volume_is_not_another_channels() {
    let mut session = two_channels();
    let address = fontelle_types::ParamAddress::new(fontelle_app::instrument::MIXER_GAIN);

    session.select_channel(1);
    let before = shown(&session, fontelle_app::instrument::MIXER_GAIN);
    session.set_instrument_param(&address, 0.25);
    let after = shown(&session, fontelle_app::instrument::MIXER_GAIN);
    assert!(
        (after - 0.25).abs() < 1e-4,
        "the channel that was turned down reads back what it was set to, got {after}"
    );

    session.select_channel(0);
    assert!(
        (shown(&session, fontelle_app::instrument::MIXER_GAIN) - before).abs() < 1e-6,
        "the other channel's volume did not move"
    );
}

/// And the same for the panel's pan, which had the same fallback.
#[test]
fn one_channels_pan_is_not_another_channels() {
    let mut session = two_channels();
    let address = fontelle_types::ParamAddress::new(fontelle_app::instrument::MIXER_PAN);

    session.select_channel(1);
    session.set_instrument_param(&address, 1.0);
    assert!(
        (shown(&session, fontelle_app::instrument::MIXER_PAN) - 1.0).abs() < 1e-4,
        "hard right on the channel it was set on"
    );

    session.select_channel(0);
    let other = shown(&session, fontelle_app::instrument::MIXER_PAN);
    assert!(
        (other - 0.5).abs() < 1e-4,
        "the other channel is still centred, got {other}"
    );
}

/// The mixer is a different control and stays where it was. A channel fader
/// that quietly moved the master is what the report was.
#[test]
fn the_master_fader_is_not_touched_by_a_channels_volume() {
    let mut session = two_channels();
    let master = session.mixer_strips()[0].gain_db;
    session.select_channel(1);
    session.set_instrument_param(
        &fontelle_types::ParamAddress::new(fontelle_app::instrument::MIXER_GAIN),
        0.1,
    );
    assert_eq!(
        session.mixer_strips()[0].gain_db,
        master,
        "the master fader is the mixer's, not a channel panel's"
    );
}

/// It is one undo entry per gesture, like every other knob, and it comes back.
#[test]
fn a_channels_volume_undoes() {
    use fontelle_ui::document::DocumentHost;
    let mut session = two_channels();
    session.select_channel(1);
    let before = shown(&session, fontelle_app::instrument::MIXER_GAIN);
    session.set_instrument_param(
        &fontelle_types::ParamAddress::new(fontelle_app::instrument::MIXER_GAIN),
        0.2,
    );
    session.end_gesture();
    session.undo();
    let after = shown(&session, fontelle_app::instrument::MIXER_GAIN);
    assert!(
        (after - before).abs() < 1e-6,
        "undo puts the level back, got {after} against {before}"
    );
}

// ------------------------------------------------- turning a knob into a lane ---
//
// *"right now theres no way to actually turn a knob into an automation clip. i
// want to be able to right click on a knob and select create automation clip
// with value and then it appears in my timeline and im able to draw it."*
//
// The channel's own volume and pan were the first two to become addressable;
// these are the rest of the panel — the knobs **inside** the instrument, which
// needed §8.2's `channel:<id>/patch/...` addresses and a way for the audio
// thread to apply one (`fontelle_core::patch_params`).

/// Every control the panel draws can be turned into a lane. Not "most of
/// them", and not "the ones somebody remembered to wire": the panel's own list
/// is the list, which is what stops the two drifting.
#[test]
fn every_knob_on_the_panel_can_be_automated() {
    let mut session = two_channels();
    let view = session.instrument().expect("the panel is there");
    let addresses: Vec<fontelle_types::ParamAddress> = view
        .groups
        .iter()
        .flat_map(|group| group.params.iter())
        .map(|param| param.address.clone())
        .collect();
    assert!(
        addresses.len() > 10,
        "the built-in synth has a panel full of them"
    );

    for address in &addresses {
        let before = session.clips().len();
        session.automate_instrument_param(address, 0);
        assert!(
            session.clips().len() > before,
            "{address} made no automation clip"
        );
    }
}

/// And the lane starts where the knob is, so creating one changes nothing you
/// can hear — a lane that jumped the parameter the moment it was made is a
/// lane nobody trusts.
#[test]
fn a_new_lane_starts_at_the_value_the_knob_is_on() {
    let mut session = two_channels();
    let cutoff = fontelle_types::ParamAddress::new("patch/filter[0]/cutoff");
    // Somewhere that is not a default, so a lane sitting at zero would show.
    session.set_instrument_param(&cutoff, 0.25);
    fontelle_ui::document::DocumentHost::end_gesture(&mut session);

    session.automate_instrument_param(&cutoff, 0);
    let first = open_lane(&session)
        .curve
        .first()
        .map(|point| point.value)
        .expect("a new lane is not empty");
    assert!(
        (first - 0.25).abs() < 0.01,
        "the lane starts where the knob is, got {first}"
    );
}

/// The clip lands on the arrangement as an automation block, which is what
/// *"then it appears in my timeline and im able to draw it"* asks for.
#[test]
fn the_lane_appears_on_the_arrangement_as_a_curve() {
    use fontelle_ui::document::ClipKind;
    let mut session = two_channels();
    session.automate_instrument_param(&fontelle_types::ParamAddress::new("patch/env[0]/attack"), 0);
    let curves: Vec<_> = session
        .clips()
        .into_iter()
        .filter(|clip| clip.kind == ClipKind::Automation)
        .collect();
    assert_eq!(curves.len(), 1, "one block, on a row of its own");
    assert!(
        curves[0].name.contains("attack"),
        "and it says which control it moves: {}",
        curves[0].name
    );
}

/// Asking twice does not make two lanes: a parameter has one, and a second
/// right-click opens the one that is there.
#[test]
fn a_second_right_click_reuses_the_lane_that_is_already_there() {
    let mut session = two_channels();
    let address = fontelle_types::ParamAddress::new("patch/filter[0]/cutoff");
    session.automate_instrument_param(&address, 0);
    let after_one = session.clips().len();
    session.automate_instrument_param(&address, 0);
    assert_eq!(session.clips().len(), after_one, "still one lane");
}
