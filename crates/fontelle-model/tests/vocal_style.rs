//! The Fontelle Singer's instrument settings: what a vocal track's
//! instrument panel sets for the whole track — the voice (formant, breath,
//! tension, voicing offsets), the expression a note without its own gets
//! (auto vibrato, a default glide, consonant speed), the backend's own
//! settings, lane bases and note-option defaults. They live on the
//! **channel**, like a patch, are changed by one command (a knob's drag one
//! undo step), cross the wire, and are left out of the file while untouched
//! so every older song opens and writes back as it was.
//!
//! > *"theres no instrument visuals for each yet, please go and create the
//! > generic vocal plugin instrument fully"* — Ty

use fontelle_model::{
    AutoGlide, AutoVibrato, Channel, Command, Edit, History, Project, SetVocalStyle, VocalStyle,
    VocalVoice, load_project, save_project,
};
use fontelle_sing::{GlideShape, Lane, Portamento, Vibrato};
use fontelle_types::{ChannelId, InstrumentKind};

fn a_vocal_channel(project: &mut Project) -> ChannelId {
    project.channels.insert(Channel {
        preset: None,
        instrument: Some(InstrumentKind::Vocal),
        name: "Voice".into(),
        color: [0; 4],
        mixer_track: None,
        patch_data: None,
        plugin: None,
        pan: 0.0,
        muted: false,
        soloed: false,
        named_keys: false,
        ab: Default::default(),
        gain_db: 0.0,
        vocal: Some(VocalVoice::new("fontelle-utau", "Teto", "ja")),
        vocal_style: VocalStyle::default(),
    })
}

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "fontelle-vocal-style-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn styled() -> VocalStyle {
    let mut style = VocalStyle::default();
    style.lanes.insert(Lane::Gender, 120.0);
    style.lanes.insert(Lane::Breathiness, 0.3);
    style.vibrato = Some(AutoVibrato {
        enabled: true,
        vibrato: Vibrato {
            depth: 40.0,
            rate: 6.0,
            delay: 0.3,
            fade_in: 0.2,
            fade_out: 0.1,
        },
        min_length: 0.4,
    });
    style.glide = Some(AutoGlide {
        enabled: true,
        portamento: Portamento {
            length: 0.08,
            position: 0.5,
            shape: GlideShape::S,
        },
    });
    style.consonant_speed = 1.5;
    style
        .settings
        .entry("fontelle-utau".into())
        .or_default()
        .insert("phonemizer".into(), 3.0);
    style
        .extra
        .entry("fontelle-diffsinger".into())
        .or_default()
        .insert("expressiveness".into(), 60.0);
    style
        .options
        .entry("fontelle-utau".into())
        .or_default()
        .insert("colour".into(), "soft".into());
    style
}

#[test]
fn an_untouched_style_is_the_default_and_sings_as_the_notes_say() {
    let style = VocalStyle::default();
    assert!(style.is_default());
    assert!(style.lanes.is_empty());
    assert_eq!(style.vibrato, None);
    assert_eq!(style.glide, None);
    assert_eq!(style.consonant_speed, 1.0);
    assert!(!styled().is_default());
}

#[test]
fn a_style_is_set_on_the_channel_and_undone_in_one_step() {
    let mut project = Project::new("style");
    let channel = a_vocal_channel(&mut project);
    let mut history = History::default();
    history
        .apply(
            Box::new(SetVocalStyle::new(channel, styled())),
            &mut project,
        )
        .unwrap();
    assert_eq!(project.channels[channel].vocal_style, styled());
    history.undo(&mut project).unwrap().unwrap();
    assert!(project.channels[channel].vocal_style.is_default());
    history.redo(&mut project).unwrap().unwrap();
    assert_eq!(project.channels[channel].vocal_style, styled());
}

/// A knob dragged across its range is many writes and one undo step.
#[test]
fn a_knob_drag_is_one_undo_step() {
    let mut project = Project::new("drag");
    let channel = a_vocal_channel(&mut project);
    let mut history = History::default();
    for cents in [10.0, 50.0, 90.0, 140.0] {
        let mut style = project.channels[channel].vocal_style.clone();
        style.lanes.insert(Lane::Gender, cents);
        history
            .apply(Box::new(SetVocalStyle::new(channel, style)), &mut project)
            .unwrap();
    }
    history.break_gesture();
    assert_eq!(history.depth(), 1, "one step");
    assert_eq!(
        project.channels[channel]
            .vocal_style
            .lanes
            .get(&Lane::Gender),
        Some(&140.0)
    );
    history.undo(&mut project).unwrap().unwrap();
    assert!(project.channels[channel].vocal_style.is_default());
    // A second gesture is a second step.
    let style = VocalStyle {
        consonant_speed: 2.0,
        ..VocalStyle::default()
    };
    history
        .apply(Box::new(SetVocalStyle::new(channel, style)), &mut project)
        .unwrap();
    assert_eq!(history.depth(), 1);
}

#[test]
fn a_style_that_is_not_a_number_or_names_no_channel_changes_nothing() {
    let mut project = Project::new("bad");
    let channel = a_vocal_channel(&mut project);
    let mut style = VocalStyle::default();
    style.lanes.insert(Lane::Tension, f32::NAN);
    assert!(
        SetVocalStyle::new(channel, style)
            .apply(&mut project)
            .is_err()
    );
    let style = VocalStyle {
        consonant_speed: f32::INFINITY,
        ..VocalStyle::default()
    };
    assert!(
        SetVocalStyle::new(channel, style)
            .apply(&mut project)
            .is_err()
    );
    assert!(project.channels[channel].vocal_style.is_default());
    let gone = {
        let mut other = Project::new("other");
        a_vocal_channel(&mut other);
        let id = a_vocal_channel(&mut other);
        other.channels.remove(id);
        id
    };
    assert!(
        SetVocalStyle::new(gone, styled())
            .apply(&mut Project::new("none"))
            .is_err()
    );
}

#[test]
fn setting_a_style_crosses_the_wire_as_itself() {
    let mut here = Project::new("wire");
    let channel = a_vocal_channel(&mut here);
    let mut there = here.clone();
    let mut command = SetVocalStyle::new(channel, styled());
    command.apply(&mut here).unwrap();
    let edit = Edit::from_bytes(&command.to_edit().to_bytes()).unwrap();
    assert_eq!(edit.tag(), "SetVocalStyle");
    edit.into_command().apply(&mut there).unwrap();
    assert_eq!(
        there.channels[channel].vocal_style,
        here.channels[channel].vocal_style
    );
    // Its inverse too: an undo is an edit like any other.
    let undo = command.invert();
    let edit = Edit::from_bytes(&undo.to_edit().to_bytes()).unwrap();
    edit.into_command().apply(&mut there).unwrap();
    assert!(there.channels[channel].vocal_style.is_default());
}

#[test]
fn a_styled_song_is_format_3_and_comes_back_whole() {
    let mut project = Project::new("saved");
    let channel = a_vocal_channel(&mut project);
    SetVocalStyle::new(channel, styled())
        .apply(&mut project)
        .unwrap();
    assert_eq!(project.format_needed(), 3);
    let dir = scratch("v3");
    save_project(&project, &dir).unwrap();
    let back = load_project(&dir).unwrap();
    assert_eq!(back.channels[channel].vocal_style, styled());
    let _ = std::fs::remove_dir_all(&dir);
}

/// What every song written before the panel existed has: no style in the
/// file. It opens as the default and is written back without one.
#[test]
fn a_song_without_a_style_opens_as_the_default_and_writes_none() {
    let mut project = Project::new("old");
    let channel = a_vocal_channel(&mut project);
    assert_eq!(
        project.format_needed(),
        2,
        "a voice alone is still format 2"
    );
    let dir = scratch("old");
    save_project(&project, &dir).unwrap();
    let text = std::fs::read_to_string(dir.join("project.json")).unwrap();
    assert!(!text.contains("vocal_style"), "nothing written untouched");
    // And a file from before the field, edited by hand to be sure.
    let mut json: serde_json::Value = serde_json::from_str(&text).unwrap();
    fn strip(v: &mut serde_json::Value) {
        match v {
            serde_json::Value::Object(map) => {
                map.remove("vocal_style");
                map.values_mut().for_each(strip);
            }
            serde_json::Value::Array(items) => items.iter_mut().for_each(strip),
            _ => {}
        }
    }
    strip(&mut json);
    std::fs::write(
        dir.join("project.json"),
        serde_json::to_string(&json).unwrap(),
    )
    .unwrap();
    let back = load_project(&dir).unwrap();
    assert!(back.channels[channel].vocal_style.is_default());
    let _ = std::fs::remove_dir_all(&dir);
}

/// Only what differs from the default is written: a style that sets one
/// knob is one key in the file, not every field.
#[test]
fn a_style_writes_only_what_was_set() {
    let mut style = VocalStyle::default();
    style.lanes.insert(Lane::Tension, 0.5);
    let json = serde_json::to_value(&style).unwrap();
    let keys: Vec<&String> = json.as_object().unwrap().keys().collect();
    assert_eq!(keys, vec!["lanes"]);
    let back: VocalStyle = serde_json::from_value(json).unwrap();
    assert_eq!(back, style);
    let empty: VocalStyle = serde_json::from_str("{}").unwrap();
    assert!(empty.is_default());
}

/// Duplicating a track keeps how it sings.
#[test]
fn a_duplicated_channel_keeps_its_style() {
    let mut project = Project::new("dup");
    let channel = a_vocal_channel(&mut project);
    SetVocalStyle::new(channel, styled())
        .apply(&mut project)
        .unwrap();
    let copy = project.channels[channel].clone();
    assert_eq!(copy.vocal_style, styled());
}
