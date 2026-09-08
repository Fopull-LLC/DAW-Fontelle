//! The preset bar's other half: what it *says*, and what pressing it does
//! (`docs/flopsynth-plan.md` §P.6, §P.7).
//!
//! The geometry is `fontelle-ui/tests/preset_bar.rs`. This is the seam — the
//! session, the bank and the document — and the one rule it holds is §P.6's:
//!
//! > **The name is remembered; the cleanliness is recognised.**
//!
//! A device carries the `PresetRef` it was loaded from through every edit, and
//! whether it is *dirty* is never stored: it is worked out by comparing the
//! device's current state with the bank's copy of that file. Which is why an
//! undo makes the `*` go out with nothing to remember, and why that is a test
//! down here rather than a promise in a doc comment.

mod common;

use std::path::{Path, PathBuf};

use fontelle_app::settings::Settings;
use fontelle_app::{RealiseOptions, SampleLibrary, Session};
use fontelle_engine::{graph_channel, timeline_channel};
use fontelle_types::{CompiledTimeline, EffectKind, InstrumentKind, PresetOrigin};
use fontelle_ui::canvas::PresetDevice;
use fontelle_ui::document::{DocumentHost, StudioHost};

use common::SR;

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "fontelle-preset-bar-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::remove_dir_all(&path).ok();
    std::fs::create_dir_all(&path).expect("the scratch folder must be creatable");
    path
}

/// A session whose user preset bank is a folder of this test's own.
fn a_session(dir: &Path) -> Session {
    let settings = Settings {
        preset_dir: Some(dir.join("presets")),
        ..Default::default()
    };
    std::fs::write(dir.join("settings.json"), settings.to_json()).unwrap();

    let project = common::a_project_with_a_clip(8, 120.0, SR);
    let clip = Session::first_clip(&project).expect("a blank project has one clip");
    let channel_nodes = fontelle_app::channel_nodes(&project);
    let (publisher, _timeline) = timeline_channel(CompiledTimeline::empty());
    let library = SampleLibrary::new();
    let options = RealiseOptions {
        sample_rate: SR,
        block_size: fontelle_engine::BLOCK_SIZE,
        quality: fontelle_app::PLAYBACK_QUALITY,
    };
    let realised =
        fontelle_app::realise(&project, &library, options).expect("an empty project must realise");
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
    .with_settings_path(dir.join("settings.json"))
}

const INSTRUMENT: PresetDevice = PresetDevice::Instrument;

/// The position of a Flopsynth preset in the selected channel's list.
fn a_flopsynth_preset(session: &Session, name: &str) -> usize {
    session
        .preset_choices(INSTRUMENT)
        .iter()
        .position(|choice| choice.name == name)
        .unwrap_or_else(|| panic!("no preset called {name}"))
}

// ------------------------------------------------------------- what it says

#[test]
fn a_channel_that_came_from_nowhere_says_so() {
    let dir = scratch("fresh");
    let session = a_session(&dir);
    let bar = session.preset_bar(INSTRUMENT);
    assert_eq!(bar.name, None);
    assert!(!bar.can_save, "there is no file to save over");
}

#[test]
fn loading_a_preset_puts_its_name_on_the_bar() {
    let dir = scratch("name");
    let mut session = a_session(&dir);
    session.set_channel_kind(0, InstrumentKind::Flopsynth);
    let at = a_flopsynth_preset(&session, "Choir Ahh");
    session.apply_preset(INSTRUMENT, at);
    let bar = session.preset_bar(INSTRUMENT);
    assert_eq!(bar.name.as_deref(), Some("Choir Ahh"));
    assert!(
        !bar.dirty,
        "straight after loading, nothing has been changed"
    );
    assert_eq!(bar.origin, Some(PresetOrigin::Factory));
}

#[test]
fn a_knob_makes_the_star_come_out_and_an_undo_makes_it_go_back_in() {
    // §P.6, which is the whole of the `*` rule: dirtiness is *recognised* by
    // comparing with the file, so the undo needs nothing to remember and the
    // two can never disagree.
    let dir = scratch("dirty");
    let mut session = a_session(&dir);
    session.set_channel_kind(0, InstrumentKind::Flopsynth);
    session.apply_preset(INSTRUMENT, a_flopsynth_preset(&session, "Choir Ahh"));

    let address = fontelle_types::ParamAddress::new("patch/filter[0]/cutoff");
    session.set_instrument_param(&address, 0.25);
    fontelle_ui::document::DocumentHost::end_gesture(&mut session);
    assert!(session.preset_bar(INSTRUMENT).dirty, "a knob moved");

    session.undo();
    assert!(
        !session.preset_bar(INSTRUMENT).dirty,
        "the patch is the file's again, so the star goes out"
    );
    assert_eq!(
        session.preset_bar(INSTRUMENT).name.as_deref(),
        Some("Choir Ahh"),
        "and the name it came from is still remembered"
    );
}

#[test]
fn save_is_refused_on_a_factory_preset_and_offered_on_your_own() {
    let dir = scratch("read-only");
    let mut session = a_session(&dir);
    session.set_channel_kind(0, InstrumentKind::Flopsynth);
    session.apply_preset(INSTRUMENT, a_flopsynth_preset(&session, "Choir Ahh"));
    assert!(
        !session.preset_bar(INSTRUMENT).can_save,
        "factory presets are read-only"
    );

    session.save_preset_as(INSTRUMENT, "Mine", "Pad");
    let bar = session.preset_bar(INSTRUMENT);
    assert_eq!(bar.name.as_deref(), Some("Mine"));
    assert_eq!(bar.origin, Some(PresetOrigin::User));
    assert!(bar.can_save, "your own preset can be saved over");
    assert!(!bar.dirty, "what was just written is what is loaded");
}

#[test]
fn save_as_writes_a_file_and_save_writes_over_it() {
    let dir = scratch("save");
    let mut session = a_session(&dir);
    session.set_channel_kind(0, InstrumentKind::Flopsynth);
    session.apply_preset(INSTRUMENT, a_flopsynth_preset(&session, "Choir Ahh"));
    session.save_preset_as(INSTRUMENT, "Mine", "Pad");
    let path = dir.join("presets/flopsynth/Pad/Mine.json");
    assert!(path.is_file(), "expected a file at {}", path.display());

    let address = fontelle_types::ParamAddress::new("patch/filter[0]/cutoff");
    session.set_instrument_param(&address, 0.25);
    fontelle_ui::document::DocumentHost::end_gesture(&mut session);
    assert!(session.preset_bar(INSTRUMENT).dirty);

    session.save_preset(INSTRUMENT);
    assert!(
        !session.preset_bar(INSTRUMENT).dirty,
        "the file now holds what the device holds"
    );
}

#[test]
fn a_device_with_no_preset_that_has_been_edited_still_says_it_is_dirty() {
    // §P.6's third case. A channel that never had a preset and has been
    // turned away from its init state has unsaved work, and the bar has to
    // say so or "Save as…" looks like it is for somebody else.
    let dir = scratch("unnamed-dirty");
    let mut session = a_session(&dir);
    // Away and back, because the project *opens* on a Flopsynth playing a
    // named preset now: setting the kind a channel already is deliberately
    // leaves its patch alone, so asking for Flopsynth on a Flopsynth would
    // leave the Grand Piano sitting there and this would be a test about a
    // preset rather than about an untouched Init.
    session.set_channel_kind(0, InstrumentKind::Osc3);
    session.set_channel_kind(0, InstrumentKind::Flopsynth);
    assert!(
        !session.preset_bar(INSTRUMENT).dirty,
        "an untouched Init is not dirty"
    );
    let address = fontelle_types::ParamAddress::new("patch/filter[0]/cutoff");
    session.set_instrument_param(&address, 0.25);
    fontelle_ui::document::DocumentHost::end_gesture(&mut session);
    assert!(session.preset_bar(INSTRUMENT).dirty);
}

// -------------------------------------------------------------- prev / next

#[test]
fn the_arrows_walk_the_bank_and_wrap() {
    let dir = scratch("walk");
    let mut session = a_session(&dir);
    session.set_channel_kind(0, InstrumentKind::Flopsynth);
    let choices = session.preset_choices(INSTRUMENT);
    assert!(choices.len() > 2, "the bank should have several");
    let first = choices[0].name.clone();
    let last = choices[choices.len() - 1].name.clone();

    session.apply_preset(INSTRUMENT, 0);
    assert_eq!(
        session.preset_bar(INSTRUMENT).name.as_deref(),
        Some(first.as_str())
    );
    session.step_preset(INSTRUMENT, -1);
    assert_eq!(
        session.preset_bar(INSTRUMENT).name.as_deref(),
        Some(last.as_str()),
        "back from the first wraps to the last"
    );
    session.step_preset(INSTRUMENT, 1);
    assert_eq!(
        session.preset_bar(INSTRUMENT).name.as_deref(),
        Some(first.as_str())
    );
}

#[test]
fn each_step_costs_exactly_one_undo() {
    let dir = scratch("undo");
    let mut session = a_session(&dir);
    session.set_channel_kind(0, InstrumentKind::Flopsynth);
    session.apply_preset(INSTRUMENT, 0);
    let name = session.preset_bar(INSTRUMENT).name;
    session.step_preset(INSTRUMENT, 1);
    assert_ne!(session.preset_bar(INSTRUMENT).name, name);
    session.undo();
    assert_eq!(
        session.preset_bar(INSTRUMENT).name,
        name,
        "one Ctrl+Z should take the whole load back"
    );
}

// -------------------------------------------------------------- the devices

#[test]
fn a_preset_for_another_instrument_switches_the_channel_to_it() {
    // FL's behaviour, through the session: the browser and the bar both reach
    // `ApplyPreset`, which is where the kind switch lives.
    let dir = scratch("switch");
    let mut session = a_session(&dir);
    // Off Flopsynth first — it is what a new project opens on — so that the
    // switch this test is about is a switch.
    session.set_channel_kind(0, InstrumentKind::Osc3);
    assert_ne!(session.channel_kind(0), Some(InstrumentKind::Flopsynth));
    let at = session
        .preset_choices(INSTRUMENT)
        .iter()
        .position(|c| c.name == "Choir Ahh");
    if let Some(at) = at {
        session.apply_preset(INSTRUMENT, at);
        assert_eq!(session.channel_kind(0), Some(InstrumentKind::Flopsynth));
    }
}

#[test]
fn an_effect_preset_lands_in_the_insert_whose_window_is_open() {
    let dir = scratch("insert");
    let mut session = a_session(&dir);
    session.add_insert(0, EffectKind::Distortion);
    let device = PresetDevice::Insert { strip: 0, slot: 0 };
    let choices = session.preset_choices(device);
    assert!(
        !choices.is_empty(),
        "the distortion ships presets and they are files now"
    );
    session.apply_preset(device, 0);
    assert_eq!(
        session.preset_bar(device).name.as_deref(),
        Some(choices[0].name.as_str())
    );
    assert!(!session.preset_bar(device).dirty);
}

#[test]
fn an_inserts_bank_is_its_own_effects_and_nobody_elses() {
    let dir = scratch("only-mine");
    let mut session = a_session(&dir);
    session.add_insert(0, EffectKind::Distortion);
    session.add_insert(0, EffectKind::Bitcrush);
    let one = session.preset_choices(PresetDevice::Insert { strip: 0, slot: 0 });
    let two = session.preset_choices(PresetDevice::Insert { strip: 0, slot: 1 });
    assert!(!one.is_empty() && !two.is_empty());
    assert_ne!(
        one.iter().map(|c| c.name.clone()).collect::<Vec<_>>(),
        two.iter().map(|c| c.name.clone()).collect::<Vec<_>>()
    );
}

// -------------------------------------------------------------- favourites

#[test]
fn a_preset_can_be_starred_and_the_star_is_in_the_settings_file() {
    let dir = scratch("star");
    let mut session = a_session(&dir);
    session.set_channel_kind(0, InstrumentKind::Flopsynth);
    session.apply_preset(INSTRUMENT, a_flopsynth_preset(&session, "Choir Ahh"));
    assert!(!session.preset_bar(INSTRUMENT).favourite);
    session.toggle_preset_favorite(INSTRUMENT);
    assert!(session.preset_bar(INSTRUMENT).favourite);

    let written = std::fs::read_to_string(dir.join("settings.json")).unwrap();
    assert!(
        written.contains("Choir Ahh"),
        "a star is a fact about the person, so it goes in their settings"
    );
}
