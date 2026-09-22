//! Recalling a whole mixer track's chain as one thing.
//!
//! A track preset is not a device preset: it replaces a *rack* of them, plus
//! the level and the placement the rack sits behind. What is measured here is
//! that it lands as **one** undo entry, that it leaves the things a chain must
//! never carry alone (the name, the sends, the routing), and that it refuses
//! rather than half-applies.

use fontelle_model::{AddInsert, ApplyTrackChain, Command, History, MixerTrack, Project, Send};
use fontelle_types::{EffectConfig, EffectKind, PresetOrigin, PresetRef, TrackChain, TrackInsert};

/// A track with a name, a fader, a send and two inserts on it.
fn a_track() -> (Project, fontelle_types::MixerTrackId) {
    let mut project = Project::new("chains");
    let master = project.mixer.master.expect("a master");
    let track = project.mixer.tracks.insert(MixerTrack::new("Verse 2"));
    project.mixer.tracks[track].output = Some(master);
    project.mixer.tracks[track].gain_db = -3.0;
    project.mixer.tracks[track].pan = 0.25;
    project.mixer.tracks[track].sends.push(Send {
        target: master,
        level_db: -12.0,
        pan: 0.0,
        pre_fader: false,
    });
    AddInsert::new(track, EffectKind::Gate)
        .apply(&mut project)
        .expect("a gate");
    (project, track)
}

fn a_chain() -> TrackChain {
    TrackChain {
        gain_db: -6.0,
        pan: -0.5,
        phase_invert: true,
        inserts: vec![
            TrackInsert {
                config: EffectConfig::new(EffectKind::Compressor),
                bypassed: false,
                preset: Some(PresetRef::new("Vocal Glue", "Vocal", PresetOrigin::User)),
                lapse: None,
            },
            TrackInsert {
                config: EffectConfig::new(EffectKind::Reverb),
                bypassed: true,
                preset: None,
                lapse: None,
            },
        ],
    }
}

#[test]
fn applying_a_chain_replaces_the_inserts_the_level_and_the_placement() {
    let (mut project, track) = a_track();
    ApplyTrackChain::new(track, a_chain())
        .apply(&mut project)
        .expect("the chain applies");
    let after = &project.mixer.tracks[track];
    assert_eq!(after.gain_db, -6.0);
    assert_eq!(after.pan, -0.5);
    assert!(after.phase_invert);
    assert_eq!(
        after.inserts.len(),
        2,
        "the old chain is gone, not appended"
    );
    assert_eq!(after.inserts[0].config.kind(), EffectKind::Compressor);
    assert_eq!(
        after.inserts[0].preset.as_ref().map(|p| p.name.as_str()),
        Some("Vocal Glue")
    );
    assert!(after.inserts[1].bypassed, "a bypass is part of the sound");
}

/// The three things a chain must not carry (`TrackChain`'s own doc says why).
#[test]
fn a_chain_leaves_the_name_the_sends_and_the_routing_alone() {
    let (mut project, track) = a_track();
    let master = project.mixer.master.expect("a master");
    ApplyTrackChain::new(track, a_chain())
        .apply(&mut project)
        .expect("the chain applies");
    let after = &project.mixer.tracks[track];
    assert_eq!(
        after.name, "Verse 2",
        "the preset names the sound, not the part"
    );
    assert_eq!(
        after.sends.len(),
        1,
        "a send points at this project's tracks"
    );
    assert_eq!(
        after.output,
        Some(master),
        "routing is this session's wiring"
    );
}

#[test]
fn applying_a_chain_is_one_undo_entry() {
    let (mut project, track) = a_track();
    let mut history = History::new();
    history
        .apply(
            Box::new(ApplyTrackChain::new(track, a_chain())),
            &mut project,
        )
        .expect("the chain applies");
    assert_eq!(project.mixer.tracks[track].inserts.len(), 2);
    history
        .undo(&mut project)
        .expect("one undo")
        .expect("it undid");
    let back = &project.mixer.tracks[track];
    assert_eq!(back.inserts.len(), 1, "the gate is back");
    assert_eq!(back.inserts[0].config.kind(), EffectKind::Gate);
    assert_eq!(back.gain_db, -3.0);
    assert_eq!(back.pan, 0.25);
    assert!(!back.phase_invert);
}

#[test]
fn a_chain_for_a_track_that_is_not_there_is_refused() {
    let (mut project, track) = a_track();
    project.mixer.tracks.remove(track);
    ApplyTrackChain::new(track, a_chain())
        .apply(&mut project)
        .expect_err("no such track");
}

/// An empty chain is a real thing to save and a real thing to load: it is
/// "this track, with nothing on it".
#[test]
fn an_empty_chain_clears_the_track() {
    let (mut project, track) = a_track();
    ApplyTrackChain::new(track, TrackChain::new())
        .apply(&mut project)
        .expect("the chain applies");
    assert!(project.mixer.tracks[track].inserts.is_empty());
    assert_eq!(project.mixer.tracks[track].gain_db, 0.0);
}
