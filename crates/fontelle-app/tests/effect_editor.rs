//! The window an effect that is **not** the EQ opens.
//!
//! Reported alongside the rest: clicking an insert in the track-options column
//! did nothing at all unless it happened to be an EQ, because the EQ was the
//! only effect with a panel. A compressor on a track was a row you could add,
//! bypass and delete, and never open — which is the same "the button does
//! nothing" the add-instrument button was.
//!
//! An effect already answers the only question a panel needs — `specs()`, its
//! list of parameters with a range, a unit and a taper each (§8.2) — so the
//! panel is built from that rather than hand-drawn per effect. Every effect
//! added after this one gets a window for free, and every knob on it is
//! automatable for free, because both are read off the same list.

mod common;

use fontelle_app::{RealiseOptions, SampleLibrary, Session, blank_project};
use fontelle_engine::{graph_channel, timeline_channel};
use fontelle_model::{AddMixerTrack, Command};
use fontelle_types::{CompiledTimeline, EffectKind};
use fontelle_ui::document::StudioHost;

use common::SR;

fn studio() -> Session {
    let mut project = blank_project(8, 120.0, SR);
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

/// A studio with a compressor on the first track.
fn with_a_compressor() -> (Session, usize, usize) {
    let mut session = studio();
    session.add_insert(0, EffectKind::Compressor);
    let slot = session.mixer_strips()[0].inserts.len() - 1;
    (session, 0, slot)
}

#[test]
fn a_compressor_has_a_panel_of_its_own_parameters() {
    let (session, strip, slot) = with_a_compressor();
    let view = session
        .insert_view(strip, slot)
        .expect("an insert the window can open");
    assert!(
        view.title.contains("Comp"),
        "the panel says what it is: {}",
        view.title
    );
    let names: Vec<&str> = view
        .groups
        .iter()
        .flat_map(|group| group.params.iter())
        .map(|param| param.label.as_str())
        .collect();
    for wanted in ["Threshold", "Ratio", "Attack", "Release", "Makeup"] {
        assert!(names.contains(&wanted), "{wanted} is missing from {names:?}");
    }
}

/// Its read-outs carry the unit the parameter is in, so a threshold reads as
/// decibels and a ratio as a ratio.
#[test]
fn the_read_outs_are_in_the_parameters_own_units() {
    let (session, strip, slot) = with_a_compressor();
    let view = session.insert_view(strip, slot).unwrap();
    let read = |label: &str| {
        view.groups
            .iter()
            .flat_map(|group| group.params.iter())
            .find(|param| param.label == label)
            .map(|param| param.display.clone())
            .unwrap_or_default()
    };
    assert!(read("Threshold").ends_with(" dB"), "{}", read("Threshold"));
    assert!(read("Ratio").ends_with(":1"), "{}", read("Ratio"));
    assert!(
        read("Auto makeup") == "off" || read("Auto makeup") == "on",
        "a switch reads as a switch, not a number: {}",
        read("Auto makeup")
    );
}

/// Turning one is a document edit like any other: undoable, and it reaches the
/// running graph without a rebuild.
#[test]
fn a_knob_on_the_panel_writes_through_the_history() {
    use fontelle_ui::document::DocumentHost;
    let (mut session, strip, slot) = with_a_compressor();
    let before = session
        .insert_view(strip, slot)
        .unwrap()
        .groups
        .iter()
        .flat_map(|group| group.params.iter())
        .find(|param| param.label == "Threshold")
        .unwrap()
        .value;

    session.set_insert_param(strip, slot, "threshold", 0.1);
    let after = session
        .insert_view(strip, slot)
        .unwrap()
        .groups
        .iter()
        .flat_map(|group| group.params.iter())
        .find(|param| param.label == "Threshold")
        .unwrap()
        .value;
    assert!(
        (after - 0.1).abs() < 0.01,
        "the panel reads back what was written, got {after}"
    );
    assert!((before - after).abs() > 0.1, "and it moved");

    session.end_gesture();
    session.undo();
    let undone = session
        .insert_view(strip, slot)
        .unwrap()
        .groups
        .iter()
        .flat_map(|group| group.params.iter())
        .find(|param| param.label == "Threshold")
        .unwrap()
        .value;
    assert!(
        (undone - before).abs() < 1e-4,
        "one Ctrl+Z puts it back, got {undone} against {before}"
    );
}

/// The EQ has a curve of its own and does **not** get the generic grid: two
/// panels for one effect is two places to change it.
#[test]
fn the_eq_keeps_its_curve() {
    let mut session = studio();
    session.add_insert(0, EffectKind::Eq);
    let slot = session.mixer_strips()[0].inserts.len() - 1;
    assert!(
        session.eq_config(0, slot).is_some(),
        "an EQ is still drawn as an EQ"
    );
    assert!(
        session.insert_view(0, slot).is_none(),
        "and it does not also offer a grid of forty-nine knobs"
    );
}

/// A slot nothing is in has no panel, and asking is not an error — the window
/// asks whenever its selection moves.
#[test]
fn an_insert_that_is_not_there_has_no_panel() {
    let (session, strip, _slot) = with_a_compressor();
    assert!(session.insert_view(strip, 9).is_none());
    assert!(session.insert_view(99, 0).is_none());
}

/// The panel labels its controls with the **full** automation address, because
/// that is what makes right-clicking one need no translation — and writing one
/// takes either that or the effect's own id for it.
///
/// Getting this wrong is silent in the worst way: the id nests inside a second
/// address, the command finds no such parameter, the knob does not move, and
/// nothing says why. It showed up as an automation lane titled
/// `mixer:4294967296/insert[0]/param/threshold`.
#[test]
fn a_knob_can_be_written_by_its_address_or_by_its_id() {
    let (mut session, strip, slot) = with_a_compressor();
    let address = session
        .insert_view(strip, slot)
        .unwrap()
        .groups
        .iter()
        .flat_map(|group| group.params.iter())
        .find(|param| param.label == "Threshold")
        .unwrap()
        .address
        .clone();
    assert!(
        address.as_str().ends_with("/param/threshold"),
        "the panel addresses a control the way a saved lane does: {address}"
    );

    session.set_insert_param(strip, slot, address.as_str(), 0.2);
    let by_address = threshold(&session, strip, slot);
    session.set_insert_param(strip, slot, "threshold", 0.8);
    let by_id = threshold(&session, strip, slot);

    assert!((by_address - 0.2).abs() < 0.01, "by address: {by_address}");
    assert!((by_id - 0.8).abs() < 0.01, "by id: {by_id}");
}

fn threshold(session: &Session, strip: usize, slot: usize) -> f32 {
    session
        .insert_view(strip, slot)
        .unwrap()
        .groups
        .iter()
        .flat_map(|group| group.params.iter())
        .find(|param| param.label == "Threshold")
        .unwrap()
        .value
}

// ---------------------------------------------------------------- presets

/// A studio with a distortion on the first track — the effect with the most
/// knobs and the most named places to start.
fn with_a_distortion() -> (Session, usize, usize) {
    let mut session = studio();
    session.add_insert(0, EffectKind::Distortion);
    let slot = session.mixer_strips()[0].inserts.len() - 1;
    (session, 0, slot)
}

/// Every control on the panel, by name and normalised value — which is the
/// whole of what a preset is allowed to change.
fn knobs(session: &Session, strip: usize, slot: usize) -> Vec<(String, f32)> {
    session
        .insert_view(strip, slot)
        .expect("a panel")
        .groups
        .iter()
        .flat_map(|group| group.params.iter())
        .map(|param| (param.label.clone(), param.value))
        .collect()
}

/// The same list, read straight off a config — so a panel can be compared
/// against what a constructor says without going through the session.
fn knobs_of(config: &fontelle_types::EffectConfig) -> Vec<(String, f32)> {
    config
        .specs()
        .iter()
        .map(|spec| {
            (
                spec.name.to_string(),
                config.normalised(spec.id).expect("a declared parameter"),
            )
        })
        .collect()
}

#[test]
fn the_panel_carries_the_effects_named_starting_points() {
    // Soften has had four presets and no way to choose one since it was
    // written; the distortion and the bitcrush now have seven and six. The
    // panel is built from `presets()` the same way its knobs are built from
    // `specs()`, so an effect that ships presets gets a picker for free.
    let (session, strip, slot) = with_a_distortion();
    let view = session.insert_view(strip, slot).expect("a panel");
    assert_eq!(
        view.presets,
        fontelle_types::DistortionPreset::ALL
            .iter()
            .map(|preset| preset.label().to_string())
            .collect::<Vec<_>>()
    );

    // And an effect with none has an empty row rather than a row of nothing.
    let (session, strip, slot) = with_a_compressor();
    assert!(session.insert_view(strip, slot).unwrap().presets.is_empty());
}

#[test]
fn choosing_a_preset_writes_every_knob_it_stands_for() {
    let (mut session, strip, slot) = with_a_distortion();
    let before = knobs(&session, strip, slot);

    // "fuzz", which is a diode curve with bias, sag and a high-pass in front
    // of it — nothing a person would find by turning one knob.
    let fuzz = fontelle_types::DistortionPreset::ALL
        .iter()
        .position(|preset| preset.label() == "fuzz")
        .expect("the fuzz preset");
    session.set_insert_preset(strip, slot, fuzz);

    let wanted = fontelle_types::EffectConfig::Distortion(
        fontelle_types::DistortionConfig::from_preset(fontelle_types::DistortionPreset::Fuzz),
    );
    assert_eq!(
        knobs(&session, strip, slot),
        knobs_of(&wanted),
        "the panel is not where the constructor puts it"
    );
    assert_ne!(knobs(&session, strip, slot), before, "nothing moved");
}

#[test]
fn a_preset_is_one_thing_to_undo() {
    // The whole reason it is a command of its own: fourteen entries on the
    // history for one click is a history nobody can walk.
    use fontelle_ui::document::DocumentHost;
    let (mut session, strip, slot) = with_a_distortion();
    let before = knobs(&session, strip, slot);

    session.set_insert_preset(strip, slot, 1);
    session.end_gesture();
    let after = knobs(&session, strip, slot);
    assert_ne!(after, before);

    session.undo();
    assert_eq!(
        knobs(&session, strip, slot),
        before,
        "one Ctrl+Z did not put the whole panel back"
    );
    // And forward again, because a preset is a thing that redoes.
    session.redo();
    assert_eq!(knobs(&session, strip, slot), after);
}

#[test]
fn a_preset_an_effect_does_not_have_changes_nothing() {
    // Reachable from a panel built before the slot's kind changed, and from a
    // project file. It is a refusal rather than a silent write of preset zero.
    let (mut session, strip, slot) = with_a_compressor();
    let before = knobs(&session, strip, slot);
    session.set_insert_preset(strip, slot, 0);
    session.set_insert_preset(strip, slot, 99);
    assert_eq!(knobs(&session, strip, slot), before);
}
