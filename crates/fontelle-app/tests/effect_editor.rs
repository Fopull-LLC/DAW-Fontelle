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

use fontelle_app::{RealiseOptions, SampleLibrary, Session};
use fontelle_engine::{graph_channel, timeline_channel};
use fontelle_model::{AddMixerTrack, Command};
use fontelle_types::{CompiledTimeline, EffectKind};
use fontelle_ui::document::StudioHost;

use common::SR;

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
    Session::new(
        project,
        library,
        channel_nodes,
        publisher,
        options,
        clip,
        None,
    )
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
        assert!(
            names.contains(&wanted),
            "{wanted} is missing from {names:?}"
        );
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
fn an_effects_preset_comes_out_of_the_bank_and_writes_every_knob() {
    // The panel's chip row is gone (`docs/flopsynth-plan.md` §P.9): the seven
    // distortions are **files** now, and the preset bar in the window's header
    // loads one the same way it loads a synth patch or a drum kit. What has
    // not changed is what a preset *is* — a constructor that writes the whole
    // panel, so a preset that left some knobs where it found them would be a
    // preset whose sound depends on what was there before it.
    let (mut session, strip, slot) = with_a_distortion();
    let before = knobs(&session, strip, slot);
    let device = fontelle_ui::canvas::PresetDevice::Insert { strip, slot };

    // "fuzz", which is a diode curve with bias, sag and a high-pass in front
    // of it — nothing a person would find by turning one knob.
    let at = session
        .preset_choices(device)
        .iter()
        .position(|choice| choice.name == "fuzz")
        .expect("the distortion ships a fuzz");
    session.apply_preset(device, at);

    let wanted = fontelle_types::EffectConfig::Distortion(
        fontelle_types::DistortionConfig::from_preset(fontelle_types::DistortionPreset::Fuzz),
    );
    assert_eq!(
        knobs(&session, strip, slot),
        knobs_of(&wanted),
        "the file is not where the recipe that wrote it puts the panel"
    );
    assert_ne!(knobs(&session, strip, slot), before, "nothing moved");
}

#[test]
fn a_preset_is_one_thing_to_undo() {
    // The whole reason it is a command of its own: fourteen entries on the
    // history for one click is a history nobody can walk.
    use fontelle_ui::document::DocumentHost;
    let (mut session, strip, slot) = with_a_distortion();
    let device = fontelle_ui::canvas::PresetDevice::Insert { strip, slot };
    let before = knobs(&session, strip, slot);

    session.apply_preset(device, 1);
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
fn every_built_in_effect_ships_a_bank_worth_opening() {
    // > *"please also ensure that every built in effect plugin has a bunch
    // > of presets that will be generally useful in a wide variety of
    // > situations especially the compressor which im noticing has no
    // > presets right now."*
    //
    // Through the bank the window reads — the files the export tool wrote,
    // not the recipes — so a recipe the tool was never taught about shows
    // up here as an empty list.
    let mut session = studio();
    for kind in fontelle_types::EffectKind::ALL {
        session.add_insert(0, kind);
        let slot = session.mixer_strips()[0].inserts.len() - 1;
        let device = fontelle_ui::canvas::PresetDevice::Insert { strip: 0, slot };
        let choices = session.preset_choices(device);
        let floor = match kind {
            fontelle_types::EffectKind::Compressor => 12,
            fontelle_types::EffectKind::Soften => 4,
            _ => 6,
        };
        assert!(
            choices.len() >= floor,
            "{kind:?} lists {} presets; at least {floor} were promised",
            choices.len()
        );
        // Applying the first one moves the panel: a preset that is the wire
        // is not a preset. The EQ has no knob panel (its editor is the
        // curve), so its presets are held flat-or-not in
        // `fontelle-types/tests/effect_presets.rs` instead.
        //
        // **DisgustingBeat moves its curves rather than its knobs**, and
        // that is not
        // an exception to the rule but the rule applied to the right state:
        // its bank *is* the device. A preset of its that left the lanes flat
        // would be the wire with a name on it, which is what this asserts.
        if kind == fontelle_types::EffectKind::DisgustingBeat {
            let before = session
                .disgusting_beat_view(0, slot)
                .expect("a DisgustingBeat has a view");
            session.apply_preset(device, 0);
            let after = session
                .disgusting_beat_view(0, slot)
                .expect("and still has one");
            assert_ne!(
                after.lanes, before.lanes,
                "DisgustingBeat's first preset draws nothing"
            );
        } else if session.insert_view(0, slot).is_some() {
            let before = knobs(&session, 0, slot);
            session.apply_preset(device, 0);
            assert_ne!(
                knobs(&session, 0, slot),
                before,
                "{kind:?}'s first preset does nothing"
            );
        } else {
            session.apply_preset(device, 0);
        }
    }
}
