//! A channel's A/B pair (`docs/flopsynth-next.md` §3.2): two patches, one
//! playing, held **in the document** so that switching is a command like
//! any other patch change — undone the way it was done, with the
//! indicator following — rather than a window's memory that an undo
//! could leave pointing the wrong way.
//!
//! What the pair is *not*: a second preset. A save writes the playing
//! slot; the other is scratch, omitted from the file while it is fresh,
//! and dropped by a preset load, since a B from before the load has
//! nothing to do with what is playing.

use fontelle_model::{
    ApplyPreset, Command, CopyChannelAb, History, MixerTrack, PresetTarget, Project,
    SwitchChannelAb,
};
use fontelle_types::{DeviceKind, InstrumentKind, PatchData, Preset, PresetPayload};

fn a_patch(marker: &str) -> PatchData {
    PatchData {
        format_version: 1,
        body: serde_json::json!({ "marker": marker }),
    }
}

fn a_project() -> (Project, fontelle_types::ChannelId) {
    let mut project = Project::new("ab");
    let master = project.mixer.tracks.insert(MixerTrack::new("Master"));
    project.mixer.master = Some(master);
    let channel = project.channels.insert(fontelle_model::Channel {
        name: "Keys".into(),
        color: [1, 2, 3, 4],
        mixer_track: None,
        patch_data: Some(a_patch("a")),
        instrument: Some(InstrumentKind::Flopsynth),
        plugin: None,
        preset: None,
        pan: 0.0,
        muted: false,
        soloed: false,
        named_keys: true,
        gain_db: 0.0,
        ab: Default::default(),
    });
    (project, channel)
}

#[test]
fn a_channel_starts_on_a_with_nothing_in_b() {
    let (project, channel) = a_project();
    let ab = &project.channels[channel].ab;
    assert!(!ab.on_b);
    assert_eq!(ab.other, None);
    // Fresh, the pair is not written to the file at all.
    let text = serde_json::to_string(&project.channels[channel]).unwrap();
    assert!(!text.contains("\"ab\""), "{text}");
}

#[test]
fn switching_the_first_time_copies_a_into_b() {
    let (mut project, channel) = a_project();
    SwitchChannelAb::new(channel).apply(&mut project).unwrap();
    let ch = &project.channels[channel];
    assert!(ch.ab.on_b);
    assert_eq!(ch.patch_data, Some(a_patch("a")), "B is A's copy");
    assert_eq!(ch.ab.other, Some(a_patch("a")), "and A is kept");
}

#[test]
fn switching_swaps_and_undoes_as_a_swap() {
    let (mut project, channel) = a_project();
    let mut history = History::new();
    history
        .apply(Box::new(SwitchChannelAb::new(channel)), &mut project)
        .unwrap();
    // An edit on B.
    project.channels[channel].patch_data = Some(a_patch("b"));
    history
        .apply(Box::new(SwitchChannelAb::new(channel)), &mut project)
        .unwrap();
    let ch = &project.channels[channel];
    assert!(!ch.ab.on_b);
    assert_eq!(ch.patch_data, Some(a_patch("a")));
    assert_eq!(ch.ab.other, Some(a_patch("b")));
    // Undo: back on B, with B's edit playing and A kept.
    history.undo(&mut project).unwrap().unwrap();
    let ch = &project.channels[channel];
    assert!(ch.ab.on_b, "the indicator follows the undo");
    assert_eq!(ch.patch_data, Some(a_patch("b")));
    assert_eq!(ch.ab.other, Some(a_patch("a")));
    // The edit was not a command here, so the next undo is the first
    // switch: A playing, and B **empty again**, as it was.
    history.undo(&mut project).unwrap().unwrap();
    let ch = &project.channels[channel];
    assert!(!ch.ab.on_b);
    assert_eq!(ch.ab.other, None, "undoing the first switch empties B");
}

#[test]
fn copying_writes_this_slot_over_the_other_without_switching() {
    let (mut project, channel) = a_project();
    let mut history = History::new();
    history
        .apply(Box::new(SwitchChannelAb::new(channel)), &mut project)
        .unwrap();
    project.channels[channel].patch_data = Some(a_patch("b"));
    history
        .apply(Box::new(CopyChannelAb::new(channel)), &mut project)
        .unwrap();
    let ch = &project.channels[channel];
    assert!(ch.ab.on_b, "still on B");
    assert_eq!(ch.ab.other, Some(a_patch("b")), "A is now B's copy");
    history.undo(&mut project).unwrap().unwrap();
    assert_eq!(project.channels[channel].ab.other, Some(a_patch("a")));
}

#[test]
fn a_preset_load_starts_the_pair_over_and_undo_brings_it_back() {
    let (mut project, channel) = a_project();
    let mut history = History::new();
    history
        .apply(Box::new(SwitchChannelAb::new(channel)), &mut project)
        .unwrap();
    let preset = Preset::new(
        DeviceKind::Instrument(InstrumentKind::Flopsynth),
        "Glass",
        "Pad",
        PresetPayload::Patch(a_patch("glass")),
    );
    history
        .apply(
            Box::new(ApplyPreset::new(PresetTarget::Channel(channel), preset)),
            &mut project,
        )
        .unwrap();
    let ch = &project.channels[channel];
    assert!(!ch.ab.on_b);
    assert_eq!(ch.ab.other, None);
    history.undo(&mut project).unwrap().unwrap();
    let ch = &project.channels[channel];
    assert!(ch.ab.on_b, "the pair comes back with the undo");
    assert_eq!(ch.ab.other, Some(a_patch("a")));
}

#[test]
fn a_file_without_a_pair_opens_on_a() {
    let (project, channel) = a_project();
    let text = serde_json::to_string(&project.channels[channel]).unwrap();
    let back: fontelle_model::Channel = serde_json::from_str(&text).unwrap();
    assert!(!back.ab.on_b);
    assert_eq!(back.ab.other, None);
}
