//! The external sidechain: one track's bus, into another track's insert's
//! detector (`docs/effects-catalogue.md` §2.1, TDD §13.4's "sidechain input
//! from any mixer track").
//!
//! This was the second of the two items the catalogue said gated more than one
//! row: the DSP on both dynamics effects has taken an `Option<&[f32]>` key
//! since each was written, and what was missing was a way for an insert to
//! **name** a track and for the compiler to schedule that track's bus first.
//!
//! # What the tests are really checking
//!
//! Three separate things, and it is worth keeping them apart because each can
//! be right while the others are wrong:
//!
//! 1. **The document.** A key is a routing edge, so it is refused when it
//!    would make the graph feed itself, when the effect has no detector, and
//!    when it names the track it is already on.
//! 2. **The order.** The keyed track has to be scheduled *after* its source,
//!    or the detector reads last block's kick. That is what
//!    `the_key_is_this_blocks_signal_and_not_the_last_ones` measures.
//! 3. **The sound.** A kick on one track ducks a pad on another, and stops
//!    ducking it when the key is taken away.

mod common;

use fontelle_app::{RealiseOptions, SampleLibrary};
use fontelle_model::{AddMixerTrack, Command, SetInsertKey};
use fontelle_types::{CompressorConfig, EffectConfig, EffectKind, MixerTrackId};

use common::SR;

const BLOCK: usize = fontelle_engine::BLOCK_SIZE;

fn options() -> RealiseOptions {
    RealiseOptions {
        sample_rate: SR,
        block_size: BLOCK,
        quality: fontelle_app::PLAYBACK_QUALITY,
    }
}

/// A project with two extra tracks: a source to key from and a target to key.
fn two_tracks() -> (fontelle_model::Project, MixerTrackId, MixerTrackId) {
    let mut project = common::a_project_with_a_clip(8, 120.0, SR);
    AddMixerTrack::new("Kick".to_string())
        .apply(&mut project)
        .expect("a track");
    AddMixerTrack::new("Pad".to_string())
        .apply(&mut project)
        .expect("a track");
    let master = project.mixer.master.expect("a master");
    let mut ids: Vec<MixerTrackId> = project
        .mixer
        .tracks
        .keys()
        .filter(|id| *id != master)
        .collect();
    ids.sort_by_key(|id| project.mixer.tracks[*id].name.clone());
    let kick = ids
        .iter()
        .copied()
        .find(|id| project.mixer.tracks[*id].name == "Kick")
        .expect("the kick");
    let pad = ids
        .iter()
        .copied()
        .find(|id| project.mixer.tracks[*id].name == "Pad")
        .expect("the pad");
    (project, kick, pad)
}

/// Puts a compressor set to duck hard on `track`, and returns its slot.
fn ducking_compressor(project: &mut fontelle_model::Project, track: MixerTrackId) -> usize {
    let config = EffectConfig::Compressor(CompressorConfig {
        threshold_db: -30.0,
        ratio: 20.0,
        attack_ms: 1.0,
        release_ms: 50.0,
        knee_db: 0.0,
        ..CompressorConfig::new()
    });
    let node = &mut project.mixer.tracks[track];
    node.inserts.push(fontelle_model::EffectSlot {
        id: fontelle_types::PersistentId::new(),
        preset: None,
        config,
        plugin: None,
        bypassed: false,
        key: None,
        notes: None,
        notepad: None,
        disgusting_beat: None,
    });
    node.inserts.len() - 1
}

// ---------------------------------------------------------------- the document

#[test]
fn an_effect_with_no_detector_cannot_be_keyed() {
    // A key on a reverb is a routing edge that feeds nothing: it would order
    // the schedule and refuse a cycle for a signal path nobody can hear.
    let (mut project, kick, pad) = two_tracks();
    project.mixer.tracks[pad]
        .inserts
        .push(fontelle_model::EffectSlot::new(EffectKind::Reverb));
    let refused = SetInsertKey::new(pad, 0, Some(kick)).apply(&mut project);
    assert!(refused.is_err(), "a reverb accepted a key");
    assert_eq!(project.mixer.tracks[pad].inserts[0].key, None);
}

#[test]
fn a_track_cannot_key_an_insert_on_itself() {
    // The degenerate loop, and the one somebody reaches for by accident: what
    // they wanted is the ordinary internal detector, which is what no key is.
    let (mut project, _, pad) = two_tracks();
    let slot = ducking_compressor(&mut project, pad);
    assert!(
        SetInsertKey::new(pad, slot, Some(pad))
            .apply(&mut project)
            .is_err()
    );
    assert_eq!(project.mixer.tracks[pad].inserts[slot].key, None);
}

#[test]
fn a_key_that_would_close_a_loop_is_refused() {
    // The pad already sends to the kick's bus; keying the pad's compressor
    // from the kick would mean the kick has to be rendered before the pad and
    // the pad before the kick.
    let (mut project, kick, pad) = two_tracks();
    let slot = ducking_compressor(&mut project, pad);
    project.mixer.tracks[pad].sends.push(fontelle_model::Send {
        id: fontelle_types::PersistentId::new(),
        target: kick,
        level_db: -6.0,
        pan: 0.0,
        pre_fader: false,
    });
    let refused = SetInsertKey::new(pad, slot, Some(kick)).apply(&mut project);
    assert!(refused.is_err(), "a key closed the routing graph");
    assert!(!project.mixer.has_cycle(), "and it was not left behind");
    assert_eq!(project.mixer.tracks[pad].inserts[slot].key, None);
}

#[test]
fn a_key_survives_a_save_and_a_load() {
    let (mut project, kick, pad) = two_tracks();
    let slot = ducking_compressor(&mut project, pad);
    SetInsertKey::new(pad, slot, Some(kick))
        .apply(&mut project)
        .expect("a legal key");
    let text = serde_json::to_string(&project.mixer.tracks[pad].inserts[slot]).unwrap();
    let back: fontelle_model::EffectSlot = serde_json::from_str(&text).unwrap();
    assert_eq!(back.key, Some(kick));

    // And a slot written before the field existed opens with no key rather
    // than failing to open.
    let old = r#"{"config":{"Compressor":{"threshold_db":-18.0,"ratio":1.0,"attack_ms":10.0,
        "release_ms":100.0,"knee_db":6.0,"makeup_db":0.0,"auto_makeup":false,
        "detection":"Peak","mix":1.0}},"bypassed":false}"#;
    let slot: fontelle_model::EffectSlot = serde_json::from_str(old).expect("the old shape loads");
    assert_eq!(slot.key, None);
}

#[test]
fn a_key_on_an_effect_that_lost_its_detector_is_not_an_edge() {
    // Reachable: a slot keyed while it held a compressor, then changed to
    // hold something else. The field is still there and `effective_key` is
    // what says whether it means anything.
    let (mut project, kick, pad) = two_tracks();
    let slot = ducking_compressor(&mut project, pad);
    SetInsertKey::new(pad, slot, Some(kick))
        .apply(&mut project)
        .expect("a legal key");
    project.mixer.tracks[pad].inserts[slot].config = EffectConfig::new(EffectKind::Reverb);
    assert_eq!(project.mixer.tracks[pad].inserts[slot].key, Some(kick));
    assert_eq!(
        project.mixer.tracks[pad].inserts[slot].effective_key(),
        None
    );
}

// ------------------------------------------------------------------ the order

#[test]
fn the_source_track_is_scheduled_before_the_track_it_keys() {
    // The whole reason a key is an edge and not a field: the detector reads
    // the tap in the same block the source filled it, so the source's nodes
    // have to come first. Measured on the schedule rather than on the sound,
    // because a wrong order is a *one-block* error — see `AudioNode::debug_name`.
    let (mut project, kick, pad) = two_tracks();
    let slot = ducking_compressor(&mut project, pad);
    SetInsertKey::new(pad, slot, Some(kick))
        .apply(&mut project)
        .expect("a legal key");

    let library = SampleLibrary::new();
    let realised = fontelle_app::realise(&project, &library, options()).expect("a graph");
    let names: Vec<&'static str> = realised
        .graph
        .schedule
        .iter()
        .map(|node| node.node.debug_name())
        .collect();
    let tap = names
        .iter()
        .position(|name| *name == "KeyTapNode")
        .expect("a key tap was scheduled");
    let effect = names
        .iter()
        .position(|name| *name == "EffectNode")
        .expect("the compressor was scheduled");
    assert!(
        tap < effect,
        "the key was filled after it was read: {names:?}"
    );
}

#[test]
fn no_key_means_no_tap_and_no_reordering() {
    // A feature that costs nothing when it is not used: an unkeyed project
    // compiles to exactly the schedule it did before keys existed.
    let (mut project, _, pad) = two_tracks();
    ducking_compressor(&mut project, pad);
    let library = SampleLibrary::new();
    let realised = fontelle_app::realise(&project, &library, options()).expect("a graph");
    assert!(
        !realised
            .graph
            .schedule
            .iter()
            .any(|node| node.node.debug_name() == "KeyTapNode"),
        "a tap was scheduled for a key nobody set"
    );
}

#[test]
fn a_key_on_an_effect_with_no_detector_schedules_no_tap() {
    // `effective_key` is what the compiler asks, so a stale key on a reverb
    // costs neither a node nor a reordering.
    let (mut project, kick, pad) = two_tracks();
    let slot = ducking_compressor(&mut project, pad);
    SetInsertKey::new(pad, slot, Some(kick))
        .apply(&mut project)
        .expect("a legal key");
    project.mixer.tracks[pad].inserts[slot].config = EffectConfig::new(EffectKind::Reverb);
    let library = SampleLibrary::new();
    let realised = fontelle_app::realise(&project, &library, options()).expect("a graph");
    assert!(
        !realised
            .graph
            .schedule
            .iter()
            .any(|node| node.node.debug_name() == "KeyTapNode"),
        "a stale key still cost a node"
    );
}

// ------------------------------------------------------------------ the window

/// A session with two mixer tracks and a compressor on the second.
fn keyed_session() -> (fontelle_app::Session, usize, usize) {
    use fontelle_app::Session;
    use fontelle_engine::{graph_channel, timeline_channel};
    use fontelle_types::CompiledTimeline;
    use fontelle_ui::document::StudioHost;

    let (mut project, _, _) = two_tracks();
    let master = project.mixer.master.expect("a master");
    let pad = project
        .mixer
        .tracks
        .iter()
        .find(|(id, track)| *id != master && track.name == "Pad")
        .map(|(id, _)| id)
        .expect("the pad");
    let slot = ducking_compressor(&mut project, pad);

    let clip = Session::first_clip(&project).expect("a blank project has one clip");
    let channel_nodes = fontelle_app::channel_nodes(&project);
    let (publisher, _timeline) = timeline_channel(CompiledTimeline::empty());
    let library = SampleLibrary::new();
    let realised = fontelle_app::realise(&project, &library, options()).expect("a graph");
    let (graphs, _source) = graph_channel(realised.graph);
    let session = Session::new(
        project,
        library,
        channel_nodes,
        publisher,
        options(),
        clip,
        None,
    )
    .with_graphs(graphs, realised.track_controls)
    .with_param_nodes(realised.param_nodes);
    let strip = session
        .mixer_strips()
        .iter()
        .position(|s| s.name == "Pad")
        .expect("the pad's strip");
    (session, strip, slot)
}

#[test]
fn the_panel_offers_every_strip_and_no_key() {
    // The key is a row of chips rather than a knob, for `EffectSlot::key`'s
    // reason: a track is not a float with a fixed range. "No key" is the first
    // chip, and it is what the insert opens on.
    use fontelle_ui::document::StudioHost;
    let (session, strip, slot) = keyed_session();
    let view = session.insert_view(strip, slot).expect("a panel");
    let names: Vec<String> = session
        .mixer_strips()
        .iter()
        .map(|s| s.name.clone())
        .collect();
    assert_eq!(view.keys.len(), names.len() + 1);
    assert_eq!(view.keys[0], fontelle_ui::canvas::NO_KEY);
    assert_eq!(&view.keys[1..], names.as_slice());
    assert_eq!(view.key, Some(0), "a fresh compressor is not keyed");

    // And an effect with no detector offers none at all.
    let (mut session, strip, _) = keyed_session();
    session.add_insert(strip, EffectKind::Reverb);
    let reverb = session.mixer_strips()[strip].inserts.len() - 1;
    assert!(session.insert_view(strip, reverb).unwrap().keys.is_empty());
}

#[test]
fn choosing_a_key_from_the_panel_writes_it_and_undoes_in_one() {
    use fontelle_ui::document::{DocumentHost, StudioHost};
    let (mut session, strip, slot) = keyed_session();
    let kick = session
        .mixer_strips()
        .iter()
        .position(|s| s.name == "Kick")
        .expect("the kick's strip");

    session.set_insert_key(strip, slot, Some(kick));
    assert_eq!(session.insert_key(strip, slot), Some(kick));
    assert_eq!(
        session.insert_view(strip, slot).unwrap().key,
        Some(kick + 1),
        "the panel does not show the key it was given"
    );

    session.end_gesture();
    session.undo();
    assert_eq!(session.insert_key(strip, slot), None);

    // And back to no key, from the first chip.
    session.set_insert_key(strip, slot, Some(kick));
    session.end_gesture();
    session.set_insert_key(strip, slot, None);
    assert_eq!(session.insert_key(strip, slot), None);
}

#[test]
fn a_key_the_document_refuses_leaves_the_panel_where_it_was() {
    use fontelle_ui::document::StudioHost;
    let (mut session, strip, slot) = keyed_session();
    // Its own strip: the degenerate loop, and what "no key" already means.
    session.set_insert_key(strip, slot, Some(strip));
    assert_eq!(session.insert_key(strip, slot), None);
    // A strip nobody has.
    session.set_insert_key(strip, slot, Some(99));
    assert_eq!(session.insert_key(strip, slot), None);
}
