//! The EQ window, and the wet/dry knob, driven through the traits the window
//! drives them through.
//!
//! Reported from using the studio: *"the eq effect is uninteractable i just
//! see a flat line, however it does SOUND like it is making an audible change
//! on how the track sounds even though im not visually seeing anything change
//! in the plugin eq window."*
//!
//! Both halves of that sentence are one bug and it is in this file's subject.
//! `set_eq_band` wrote the document and published the new config to the
//! running graph — which is why it was audible — and left the **revision**
//! where it was. The window re-reads the studio's lists only when the revision
//! moves (see `WindowApp::refresh_studio`, which exists because asking for
//! them every frame allocates), so the config the editor drew from was the one
//! it had cached when the window opened: eight flat bands, for ever. Clicking
//! the curve *did* add a band, and the band *was* heard, and the picture never
//! changed — so the editor looked dead while working.
//!
//! Every dragged control in this session already bumps the revision for
//! exactly this reason — `set_send_level` and `publish_mixer` do it on the
//! line after they publish. The EQ was the one that did not.

mod common;

use fontelle_app::{RealiseOptions, SampleLibrary, Session};
use fontelle_engine::{graph_channel, timeline_channel};
use fontelle_model::{AddMixerTrack, Command};
use fontelle_types::{BandChannel, BandType, CompiledTimeline, EffectKind, EqBand};
use fontelle_ui::document::{DocumentHost, StudioHost};

use common::SR;

/// A studio with one mixer track besides the master, and a live graph behind
/// it — so a knob can be shown to reach the sound without a rebuild.
fn studio() -> Session {
    let mut project = common::a_project_with_a_clip(8, 120.0, SR);
    AddMixerTrack::new("Keys".to_string())
        .apply(&mut project)
        .expect("a mixer track must be addable");
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
    Session::new(project, library, channel_nodes, publisher, options, clip, None)
        .with_graphs(graphs, realised.track_controls)
        .with_param_nodes(realised.param_nodes)
}

/// The strip an EQ is put on, with the EQ on it. Index 0 is the first track.
fn with_an_eq() -> (Session, usize, usize) {
    let mut session = studio();
    let strip = 0;
    session.add_insert(strip, EffectKind::Eq);
    let slot = session.mixer_strips()[strip].inserts.len() - 1;
    (session, strip, slot)
}

fn a_band(freq_hz: f32, gain_db: f32) -> EqBand {
    EqBand {
        band_type: BandType::Bell,
        freq_hz,
        gain_db,
        q: 1.0,
        enabled: true,
        solo: false,
        channel: BandChannel::Stereo,
    }
}

// ------------------------------------------------- the curve that redrew ---

#[test]
fn editing_a_band_moves_the_revision_so_the_window_redraws() {
    let (mut session, strip, slot) = with_an_eq();
    let before = session.revision();

    session.set_eq_band(strip, slot, 0, a_band(1_000.0, 6.0));

    assert_ne!(
        session.revision(),
        before,
        "the window re-reads on the revision, so a band that does not move it \
         is a curve that never redraws"
    );
}

#[test]
fn the_band_that_was_set_is_the_band_that_is_read_back() {
    let (mut session, strip, slot) = with_an_eq();
    session.set_eq_band(strip, slot, 2, a_band(4_000.0, -8.0));

    let config = session
        .eq_config(strip, slot)
        .expect("that slot holds an EQ");
    assert_eq!(config.bands[2].freq_hz, 4_000.0);
    assert_eq!(config.bands[2].gain_db, -8.0);
    assert!(config.bands[2].enabled);
    assert!(!config.bands[0].enabled, "only the band that was set moved");
}

#[test]
fn a_band_reaches_the_running_graph_without_a_rebuild() {
    // The other half of the same edit, and the reason the bug was invisible in
    // the tests that existed: this part always worked.
    let (mut session, strip, slot) = with_an_eq();
    session.set_eq_band(strip, slot, 0, a_band(2_500.0, 5.0));

    let fontelle_types::EffectConfig::Eq(live) = session
        .live_effect_config(strip, slot)
        .expect("the insert has a live end")
    else {
        unreachable!("this slot holds an EQ")
    };
    assert_eq!(live.bands[0].freq_hz, 2_500.0);
    assert_eq!(live.bands[0].gain_db, 5.0);
}

// --------------------------------------------------------- the mix knob ---

#[test]
fn every_insert_starts_fully_wet_and_says_so() {
    let (session, strip, _) = with_an_eq();
    let strips = session.mixer_strips();
    assert_eq!(strips[strip].inserts[0].mix, 1.0);
}

#[test]
fn the_mix_knob_reaches_the_document_and_the_running_graph() {
    let (mut session, strip, slot) = with_an_eq();
    session.set_insert_mix(strip, slot, 0.3);

    assert!(
        (session.mixer_strips()[strip].inserts[slot].mix - 0.3).abs() < 1e-6,
        "the strip's own row has to show where the knob is"
    );
    let live = session
        .live_effect_config(strip, slot)
        .expect("the insert has a live end");
    assert!(
        (live.mix() - 0.3).abs() < 1e-6,
        "a wet/dry knob has to be heard while it is moving, like a fader"
    );
}

#[test]
fn a_whole_mix_drag_is_one_undo_entry() {
    let (mut session, strip, slot) = with_an_eq();
    for step in 0..20 {
        session.set_insert_mix(strip, slot, 1.0 - step as f32 / 40.0);
    }
    session.undo();
    assert_eq!(
        session.mixer_strips()[strip].inserts[slot].mix, 1.0,
        "undo goes back to before the drag, not into the middle of it"
    );
}

#[test]
fn moving_the_mix_moves_the_revision() {
    let (mut session, strip, slot) = with_an_eq();
    let before = session.revision();
    session.set_insert_mix(strip, slot, 0.5);
    assert_ne!(session.revision(), before);
}

#[test]
fn a_mix_on_a_slot_that_is_not_there_does_nothing_rather_than_panicking() {
    let (mut session, strip, _) = with_an_eq();
    session.set_insert_mix(strip, 9, 0.5);
    session.set_insert_mix(99, 0, 0.5);
    assert_eq!(session.mixer_strips()[strip].inserts[0].mix, 1.0);
}
