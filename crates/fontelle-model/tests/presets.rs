//! Applying a preset to a device, and remembering which one it was
//! (`docs/flopsynth-plan.md` §P.5).
//!
//! Two commands hold the whole system on the document side. [`ApplyPreset`]
//! writes a device's state *and* the name it came from in **one** entry,
//! because loading a preset is one thing a person did — the argument
//! `SetInsertPreset` already made for the constructor presets it replaces,
//! now made for every device at once. [`SetPresetRef`] writes only the name,
//! which is what a *save* does after the file is on disk: the state did not
//! change, only what it is now called.
//!
//! The one behaviour worth stating out loud is the **kind switch**. Choosing a
//! Flopsynth preset on a soundfont channel makes it a Flopsynth channel, the
//! way FL Studio's browser does, because the alternative — refusing, or
//! loading a patch the channel's kind cannot play — is a preset that quietly
//! does nothing. An *insert* is the opposite and for a stated reason: a slot's
//! kind comes from the "+ effect" menu, so a preset for another effect is
//! refused rather than turning a reverb into a gate.

use fontelle_model::{
    AddInsert, ApplyPreset, Command, History, MixerTrack, PresetTarget, Project, SetPresetRef,
};
use fontelle_types::{
    DeviceKind, EffectConfig, EffectKind, InstrumentKind, PatchData, Preset, PresetOrigin,
    PresetPayload, PresetRef,
};

fn a_patch(marker: &str) -> PatchData {
    PatchData {
        format_version: 1,
        body: serde_json::json!({ "marker": marker }),
    }
}

fn an_instrument_preset(kind: InstrumentKind, name: &str) -> Preset {
    Preset::new(
        DeviceKind::Instrument(kind),
        name,
        "Pad",
        PresetPayload::Patch(a_patch(name)),
    )
}

fn an_effect_preset(config: EffectConfig, name: &str) -> Preset {
    Preset::new(
        DeviceKind::Effect(config.kind()),
        name,
        "Colour",
        PresetPayload::Effect(config),
    )
}

fn a_project() -> (
    Project,
    fontelle_types::ChannelId,
    fontelle_types::MixerTrackId,
) {
    let mut project = Project::new("presets");
    let master = project.mixer.tracks.insert(MixerTrack::new("Master"));
    project.mixer.master = Some(master);
    let channel = project.channels.insert(fontelle_model::Channel {
        name: "Keys".into(),
        color: [1, 2, 3, 4],
        mixer_track: None,
        patch_data: Some(a_patch("what was there before")),
        instrument: Some(InstrumentKind::SoundFont),
        plugin: None,
        preset: None,
        pan: 0.0,
        muted: false,
        soloed: false,
        named_keys: true,
        gain_db: 0.0,
    });
    (project, channel, master)
}

// --------------------------------------------------------------- a channel

#[test]
fn a_device_starts_remembering_no_preset() {
    // Not "Init": a channel that has never been given a preset came from
    // nowhere, and saying so is what lets the bar draw "— no preset —" rather
    // than a name that was never chosen.
    let (project, channel, _) = a_project();
    assert_eq!(project.channels[channel].preset, None);
}

#[test]
fn applying_a_preset_writes_the_state_and_the_name_it_came_from() {
    let (mut project, channel, _) = a_project();
    ApplyPreset::new(
        PresetTarget::Channel(channel),
        an_instrument_preset(InstrumentKind::SoundFont, "Glass"),
    )
    .apply(&mut project)
    .unwrap();
    assert_eq!(project.channels[channel].patch_data, Some(a_patch("Glass")));
    assert_eq!(
        project.channels[channel].preset,
        Some(PresetRef::new("Glass", "Pad", PresetOrigin::Factory))
    );
}

#[test]
fn a_preset_for_another_instrument_switches_the_channel_to_it() {
    // FL Studio's behaviour, and the only one that is not a lie: the
    // alternative is a Flopsynth patch sitting on a channel whose kind says
    // soundfont, which plays nothing and says nothing about why.
    let (mut project, channel, _) = a_project();
    ApplyPreset::new(
        PresetTarget::Channel(channel),
        an_instrument_preset(InstrumentKind::Flopsynth, "Choir Ahh"),
    )
    .apply(&mut project)
    .unwrap();
    assert_eq!(
        project.channels[channel].instrument,
        Some(InstrumentKind::Flopsynth)
    );
}

#[test]
fn one_entry_takes_back_the_kind_the_patch_and_the_name_together() {
    // The whole reason this is a command and not three: a preset is one thing
    // a person did, so one Ctrl+Z has to be able to take all of it back.
    let (mut project, channel, _) = a_project();
    let mut history = History::new();
    history
        .apply(
            Box::new(ApplyPreset::new(
                PresetTarget::Channel(channel),
                an_instrument_preset(InstrumentKind::Flopsynth, "Choir Ahh"),
            )),
            &mut project,
        )
        .unwrap();
    history.undo(&mut project).unwrap().unwrap();
    assert_eq!(
        project.channels[channel].instrument,
        Some(InstrumentKind::SoundFont),
        "the kind"
    );
    assert_eq!(
        project.channels[channel].patch_data,
        Some(a_patch("what was there before")),
        "the patch"
    );
    assert_eq!(project.channels[channel].preset, None, "the name");
}

#[test]
fn an_effect_preset_on_a_channel_is_refused() {
    let (mut project, channel, _) = a_project();
    assert!(
        ApplyPreset::new(
            PresetTarget::Channel(channel),
            an_effect_preset(EffectConfig::new(EffectKind::Reverb), "Hall"),
        )
        .apply(&mut project)
        .is_err()
    );
}

#[test]
fn a_preset_whose_payload_does_not_match_its_device_is_refused() {
    // The bank lists such a file as unreadable, but a preset can also reach
    // this command from a project file or a hand-edited one, and the command
    // is the last place it can be stopped before it writes nonsense.
    let (mut project, channel, _) = a_project();
    let wrong = Preset::new(
        DeviceKind::Instrument(InstrumentKind::Flopsynth),
        "Liar",
        "Pad",
        PresetPayload::Effect(EffectConfig::new(EffectKind::Reverb)),
    );
    assert!(
        ApplyPreset::new(PresetTarget::Channel(channel), wrong)
            .apply(&mut project)
            .is_err()
    );
}

// ---------------------------------------------------------------- an insert

#[test]
fn applying_an_effect_preset_writes_the_config_and_the_name() {
    let (mut project, _, master) = a_project();
    AddInsert::new(master, EffectKind::Distortion)
        .apply(&mut project)
        .unwrap();
    let mut config = EffectConfig::new(EffectKind::Distortion);
    if let EffectConfig::Distortion(distortion) = &mut config {
        distortion.drive_db = 18.0;
    }
    ApplyPreset::new(
        PresetTarget::Insert {
            track: master,
            index: 0,
        },
        an_effect_preset(config, "Fuzz"),
    )
    .apply(&mut project)
    .unwrap();
    let slot = &project.mixer.tracks[master].inserts[0];
    assert_eq!(slot.config, config);
    assert_eq!(
        slot.preset,
        Some(PresetRef::new("Fuzz", "Colour", PresetOrigin::Factory))
    );
}

#[test]
fn a_preset_for_a_different_effect_is_refused_rather_than_switching_the_slot() {
    // The opposite of the channel, and deliberately: a slot's kind is chosen
    // from the "+ effect" menu, so a reverb preset dropped on a gate is a
    // mistake, not an instruction. A channel has no such menu — its kind
    // *is* what you loaded.
    let (mut project, _, master) = a_project();
    AddInsert::new(master, EffectKind::Gate)
        .apply(&mut project)
        .unwrap();
    assert!(
        ApplyPreset::new(
            PresetTarget::Insert {
                track: master,
                index: 0,
            },
            an_effect_preset(EffectConfig::new(EffectKind::Reverb), "Hall"),
        )
        .apply(&mut project)
        .is_err()
    );
    assert_eq!(
        project.mixer.tracks[master].inserts[0].config.kind(),
        EffectKind::Gate
    );
}

#[test]
fn undoing_an_effect_preset_puts_the_knobs_and_the_name_back() {
    let (mut project, _, master) = a_project();
    AddInsert::new(master, EffectKind::Distortion)
        .apply(&mut project)
        .unwrap();
    let before = project.mixer.tracks[master].inserts[0].config;
    let mut history = History::new();
    let mut config = EffectConfig::new(EffectKind::Distortion);
    if let EffectConfig::Distortion(distortion) = &mut config {
        distortion.drive_db = 18.0;
    }
    history
        .apply(
            Box::new(ApplyPreset::new(
                PresetTarget::Insert {
                    track: master,
                    index: 0,
                },
                an_effect_preset(config, "Fuzz"),
            )),
            &mut project,
        )
        .unwrap();
    history.undo(&mut project).unwrap().unwrap();
    assert_eq!(project.mixer.tracks[master].inserts[0].config, before);
    assert_eq!(project.mixer.tracks[master].inserts[0].preset, None);
}

// ------------------------------------------------------------------ the ref

#[test]
fn a_save_writes_the_name_without_touching_the_sound() {
    // What "Save as…" runs once the file is on disk. The payload is already
    // what the file holds — that is what was written — so this command must
    // not touch it.
    let (mut project, channel, _) = a_project();
    let before = project.channels[channel].patch_data.clone();
    SetPresetRef::new(
        PresetTarget::Channel(channel),
        Some(PresetRef::new("My Pad", "User", PresetOrigin::User)),
    )
    .apply(&mut project)
    .unwrap();
    assert_eq!(project.channels[channel].patch_data, before);
    assert_eq!(
        project.channels[channel].preset,
        Some(PresetRef::new("My Pad", "User", PresetOrigin::User))
    );
}

#[test]
fn undoing_a_save_puts_the_old_name_back() {
    let (mut project, channel, _) = a_project();
    let mut history = History::new();
    history
        .apply(
            Box::new(ApplyPreset::new(
                PresetTarget::Channel(channel),
                an_instrument_preset(InstrumentKind::SoundFont, "Glass"),
            )),
            &mut project,
        )
        .unwrap();
    history
        .apply(
            Box::new(SetPresetRef::new(
                PresetTarget::Channel(channel),
                Some(PresetRef::new("Glass 2", "User", PresetOrigin::User)),
            )),
            &mut project,
        )
        .unwrap();
    history.undo(&mut project).unwrap().unwrap();
    assert_eq!(
        project.channels[channel].preset,
        Some(PresetRef::new("Glass", "Pad", PresetOrigin::Factory))
    );
}

#[test]
fn a_preset_from_the_future_is_refused() {
    let (mut project, channel, _) = a_project();
    let mut preset = an_instrument_preset(InstrumentKind::Flopsynth, "Tomorrow");
    preset.format_version = fontelle_types::PRESET_FORMAT_VERSION + 1;
    assert!(
        ApplyPreset::new(PresetTarget::Channel(channel), preset)
            .apply(&mut project)
            .is_err()
    );
}
