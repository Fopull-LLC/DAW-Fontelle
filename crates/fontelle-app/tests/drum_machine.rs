//! The drum machine as an instrument on a channel.
//!
//! > *"i want you to create a new instrument, a built in general purpose drum
//! > machine ... you can play them all in the piano roll all labeled and stuff
//! > should have lots of presets for different styles and genres of kits.
//! > should be encorperated like any other vst would be."*
//!
//! `fontelle-core` has the kit and `fontelle-dsp` has the hits; this is the
//! half that makes it an **instrument** — that the New Instrument menu offers
//! it, that a channel of that kind arrives playing, that the piano roll labels
//! its keys, and that the panel offers the kits as presets. *"Like any other
//! vst"* is checkable, and this is where.

mod common;

use std::path::{Path, PathBuf};

use fontelle_app::{RealiseOptions, SampleLibrary, Session};
use fontelle_core::{DrumKitStyle, Source};
use fontelle_engine::{graph_channel, timeline_channel};
use fontelle_types::{CompiledTimeline, InstrumentKind};
use fontelle_ui::document::{DocumentHost, StudioHost};

use common::SR;

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "fontelle-drums-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::remove_dir_all(&path).ok();
    std::fs::create_dir_all(&path).expect("creatable");
    path
}

fn a_session(dir: &Path) -> Session {
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

// -------------------------------------------------------- it is an instrument ---

#[test]
fn the_new_instrument_menu_offers_it() {
    // *"should be encorperated like any other vst would be"* — which starts
    // with being on the list of things you can make.
    assert!(
        InstrumentKind::ALL.contains(&InstrumentKind::DrumMachine),
        "the drum machine is not on the menu"
    );
    assert!(!InstrumentKind::DrumMachine.label().is_empty());
    let mut names: Vec<&str> = InstrumentKind::ALL.iter().map(|k| k.label()).collect();
    names.sort_unstable();
    let before = names.len();
    names.dedup();
    assert_eq!(before, names.len(), "two instruments share a name");
}

#[test]
fn a_drum_machine_arrives_playing_rather_than_waiting_for_a_file() {
    // The soundfont player and the sampler both sit silent until they are
    // given a file. A kit has nothing to be given — which makes it the only
    // instrument here that works on a fresh install with no bank configured.
    assert!(InstrumentKind::DrumMachine.plays_on_arrival());
    assert_eq!(InstrumentKind::DrumMachine.wants(), None);

    let dir = scratch("arrives");
    let mut session = a_session(&dir);
    session
        .add_channel_of(InstrumentKind::DrumMachine)
        .expect("adds");
    let made = session.channels().len() - 1;
    assert_eq!(
        session.channel_kind(made),
        Some(InstrumentKind::DrumMachine)
    );
    assert!(
        session.channels()[made].has_instrument,
        "a fresh kit has nothing in it"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_fresh_kit_is_the_neutral_one_and_it_is_a_real_kit() {
    let dir = scratch("neutral");
    let mut session = a_session(&dir);
    session
        .add_channel_of(InstrumentKind::DrumMachine)
        .expect("adds");
    let made = session.channels().len() - 1;
    let patch = kit_of(&mut session, made);
    assert_eq!(
        patch.layers.len(),
        fontelle_core::GM_DRUM_MAP.len(),
        "a fresh kit is not the full map"
    );
    assert!(
        patch
            .layers
            .iter()
            .all(|l| matches!(l.source, Source::Drum(_))),
        "a fresh kit is not all drums"
    );
    std::fs::remove_dir_all(&dir).ok();
}

// ------------------------------------------------------------- the labels ---

#[test]
fn the_piano_roll_labels_every_hit_and_greys_the_rest() {
    // *"you can play them all in the piano roll all labeled."* The mechanism
    // is the one drum soundfonts already use — a kit's layers are one-key
    // zones, so `key_map` reads it as a key map — and this is the check that
    // the names actually arrive.
    let dir = scratch("labels");
    let mut session = a_session(&dir);
    session
        .add_channel_of(InstrumentKind::DrumMachine)
        .expect("adds");
    session.select_channel(session.channels().len() - 1);

    let map = session.key_map();
    for slot in fontelle_core::drum_slots(DrumKitStyle::Studio) {
        assert!(
            map.plays(slot.key),
            "{} on key {} is greyed",
            slot.name,
            slot.key
        );
        assert_eq!(
            map.name(slot.key),
            Some(slot.name),
            "key {} is not labelled {}",
            slot.key,
            slot.name
        );
    }
    // And the keys the kit does not cover say so, which is what the greying
    // is for.
    for key in [12u8, 24, 100, 120] {
        assert!(
            !map.plays(key),
            "key {key} is not in the kit and is not greyed"
        );
        assert_eq!(map.name(key), None);
    }
    std::fs::remove_dir_all(&dir).ok();
}

// ------------------------------------------------------------ the presets ---

#[test]
fn every_kit_is_in_the_bank_under_the_drum_machine() {
    // *"lots of presets for different styles and genres of kits."* They were a
    // row of chips on the panel; they are **files** now, in the bank every
    // device shares (`docs/flopsynth-plan.md` §P.9), reached from the preset
    // bar in the window's header and from the browser's Presets tab. The
    // recipes stayed where they were — `DrumKitStyle` is what the export tool
    // runs — they simply no longer sit behind a panel of their own.
    let dir = scratch("presets");
    let mut session = a_session(&dir);
    session
        .add_channel_of(InstrumentKind::DrumMachine)
        .expect("adds");
    session.select_channel(session.channels().len() - 1);

    let names: Vec<String> = session
        .preset_choices(fontelle_ui::canvas::PresetDevice::Instrument)
        .into_iter()
        .map(|choice| choice.name)
        .collect();
    for style in DrumKitStyle::ALL {
        assert!(
            names.iter().any(|name| name == style.label()),
            "{} is not in the bank: {names:?}",
            style.label()
        );
    }
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_channel_is_only_offered_its_own_devices_presets() {
    // A row of drum kits over a soundfont player would be a row about a
    // different instrument — the rule the chip row followed, kept, and now
    // enforced by the bank rather than by the panel.
    let dir = scratch("no-chips");
    let mut session = a_session(&dir);
    session.select_channel(0);
    // Onto 3OSC first: what the project *opens* on is Flopsynth now, and this
    // is a test about which device's presets a channel is offered, not about
    // which device a new project picks.
    session.set_channel_kind(0, InstrumentKind::Osc3);
    assert_eq!(session.channel_kind(0), Some(InstrumentKind::Osc3));
    let names: Vec<String> = session
        .preset_choices(fontelle_ui::canvas::PresetDevice::Instrument)
        .into_iter()
        .map(|choice| choice.name)
        .collect();
    for style in DrumKitStyle::ALL {
        assert!(
            !names.iter().any(|name| name == style.label()),
            "3OSC was offered a drum kit"
        );
    }
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn choosing_a_kit_writes_the_hits_and_is_one_undo_away() {
    // A preset writes the knobs and then has nothing further to say (rule
    // 10), and it is one thing somebody did — so one press of Ctrl+Z puts the
    // kit that was there back.
    let dir = scratch("choose");
    let mut session = a_session(&dir);
    session
        .add_channel_of(InstrumentKind::DrumMachine)
        .expect("adds");
    let made = session.channels().len() - 1;
    session.select_channel(made);

    let kick_of = |session: &mut Session| {
        let patch = kit_of(session, made);
        let layer = patch
            .layers
            .iter()
            .find(|l| l.key_range == (36, 36))
            .expect("a kick on 36");
        let Source::Drum(voice) = layer.source else {
            panic!("the kick is not a drum")
        };
        voice
    };
    let before = kick_of(&mut session);

    // The 808, whose whole character is a kick that rings.
    let device = fontelle_ui::canvas::PresetDevice::Instrument;
    let at = session
        .preset_choices(device)
        .iter()
        .position(|choice| choice.name == DrumKitStyle::EightOhEight.label())
        .expect("the 808 is in the bank");
    session.apply_preset(device, at);

    let after = kick_of(&mut session);
    assert_ne!(before, after, "choosing the 808 changed nothing");
    assert!(
        after.decay_s > before.decay_s * 2.0,
        "an 808 kick rings: {} against {}",
        after.decay_s,
        before.decay_s
    );

    session.undo();
    assert_eq!(kick_of(&mut session), before, "one gesture, one undo");
    std::fs::remove_dir_all(&dir).ok();
}

// ------------------------------------------------------------ it survives ---

#[test]
fn a_kit_survives_being_written_to_a_project_and_read_back() {
    // *"like any other vst"* includes being in `project.json`. A drum layer
    // names no file, so it is the one instrument that needs no relinking —
    // and that is worth a test, because "needs nothing" is exactly the kind
    // of claim that quietly stops being true.
    let dir = scratch("roundtrip");
    let mut session = a_session(&dir);
    session
        .add_channel_of(InstrumentKind::DrumMachine)
        .expect("adds");
    let made = session.channels().len() - 1;
    session.select_channel(made);
    let device = fontelle_ui::canvas::PresetDevice::Instrument;
    let at = session
        .preset_choices(device)
        .iter()
        .position(|choice| choice.name == DrumKitStyle::Chiptune.label())
        .expect("chiptune is in the bank");
    session.apply_preset(device, at);
    let before = kit_of(&mut session, made);

    let data = before.to_data(&Default::default()).expect("writable");
    let back = fontelle_core::Patch::from_data(&data, |_| None).expect("readable");
    assert!(back.unresolved.is_empty(), "a kit asked for a file");
    assert_eq!(back.patch.layers.len(), before.layers.len());
    for (a, b) in back.patch.layers.iter().zip(&before.layers) {
        assert_eq!(a.source, b.source, "a hit changed on the way through");
        assert_eq!(a.key_range, b.key_range);
    }
    std::fs::remove_dir_all(&dir).ok();
}

/// The patch on `channel`, read the way the panel reads it — by selecting the
/// channel and asking the session, which is the only route the window has.
fn kit_of(session: &mut Session, channel: usize) -> fontelle_core::Patch {
    session.select_channel(channel);
    session.selected_patch().expect("a kit on that channel")
}

#[test]
fn a_kit_is_still_a_kit_after_it_has_been_edited_by_hand() {
    // A preset writes the knobs and then has nothing further to say (rule
    // 10). So a hit tuned by hand afterwards must survive — nothing goes back
    // and re-derives the kit from the style, because nothing remembers the
    // style. This is the difference between a preset and a mode.
    let dir = scratch("edited");
    let mut session = a_session(&dir);
    session
        .add_channel_of(InstrumentKind::DrumMachine)
        .expect("adds");
    let made = session.channels().len() - 1;
    session.select_channel(made);

    let mut patch = kit_of(&mut session, made);
    let layer = patch
        .layers
        .iter_mut()
        .find(|l| l.key_range == (36, 36))
        .expect("a kick on 36");
    let Source::Drum(voice) = &mut layer.source else {
        panic!("the kick is not a drum")
    };
    voice.tune_hz = 33.0;
    let data = patch.to_data(&Default::default()).expect("writable");
    let back = fontelle_core::Patch::from_data(&data, |_| None).expect("readable");
    let kick = back
        .patch
        .layers
        .iter()
        .find(|l| l.key_range == (36, 36))
        .expect("still a kick");
    let Source::Drum(voice) = kick.source else {
        panic!("not a drum any more")
    };
    assert_eq!(voice.tune_hz, 33.0, "a hand edit did not survive");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_kit_reads_back_as_a_drum_machine_rather_than_as_a_synth() {
    // `kind_of` decides what the rack calls a channel from its layers, and a
    // kit whose layers say "drum" but whose row says "3OSC" is a rack you
    // cannot navigate.
    let dir = scratch("kind");
    let mut session = a_session(&dir);
    session
        .add_channel_of(InstrumentKind::DrumMachine)
        .expect("adds");
    let made = session.channels().len() - 1;
    // Read through the patch rather than through `Channel::instrument`, which
    // is the stored answer — the derived one has to agree with it.
    let patch = kit_of(&mut session, made);
    assert!(
        patch
            .layers
            .iter()
            .all(|l| matches!(l.source, Source::Drum(_)))
    );
    assert_eq!(
        session.channel_kind(made),
        Some(InstrumentKind::DrumMachine)
    );
    std::fs::remove_dir_all(&dir).ok();
}

// --------------------------------------------------- the chip that lights ---

#[test]
fn the_bar_says_which_kit_is_on_the_channel_and_when_it_has_been_edited() {
    // > *"not noticing much feedback for when i actually change a selection
    // > of kit visually like not much user feedback."*
    //
    // §P.6's rule, on the drum machine: the **name is remembered** and the
    // cleanliness is **recognised**. Choosing a kit puts its name on the bar;
    // moving a hit puts a `*` beside it; undoing takes the `*` away again with
    // nothing to remember.
    let dir = scratch("marked");
    let mut session = a_session(&dir);
    session
        .add_channel_of(InstrumentKind::DrumMachine)
        .expect("adds");
    session.select_channel(session.channels().len() - 1);
    let device = fontelle_ui::canvas::PresetDevice::Instrument;

    let at = session
        .preset_choices(device)
        .iter()
        .position(|choice| choice.name == DrumKitStyle::EightOhEight.label())
        .expect("the 808 is in the bank");
    session.apply_preset(device, at);
    let bar = session.preset_bar(device);
    assert_eq!(
        bar.name.as_deref(),
        Some(DrumKitStyle::EightOhEight.label())
    );
    assert!(
        !bar.dirty,
        "straight after loading, nothing has been changed"
    );

    session.undo();
    assert_eq!(
        session.preset_bar(device).name,
        None,
        "undo did not take the name back"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_kit_touched_by_hand_wears_a_star() {
    // §P.6: the `*` is the truth about the knobs, not a memory of a click.
    // Once a hit is edited the channel is no longer the file it came from,
    // and a bar that stayed clean would be a bar that lies.
    let dir = scratch("touched");
    let mut session = a_session(&dir);
    session
        .add_channel_of(InstrumentKind::DrumMachine)
        .expect("adds");
    let made = session.channels().len() - 1;
    session.select_channel(made);

    // The kick's own layer gain, through the same call a knob on the panel
    // makes — the one route an edit to a hit has today.
    let patch = kit_of(&mut session, made);
    let kick = patch
        .layers
        .iter()
        .position(|l| l.key_range == (36, 36))
        .expect("a kick on 36");
    let address = fontelle_types::ParamAddress::new(format!("patch/layer[{kick}]/gain"));
    session.set_instrument_param(&address, 0.9);

    assert!(
        session
            .preset_bar(fontelle_ui::canvas::PresetDevice::Instrument)
            .dirty,
        "an edited kit still reads as clean"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_synth_that_came_from_no_preset_says_so() {
    let dir = scratch("synth-mark");
    let mut session = a_session(&dir);
    // A **new** Flopsynth channel, not the starting one: a project opens on
    // the Grand Piano and says so now (`docs/flopsynth-next.md` §1.4(1)),
    // and this test is about a synth nobody chose a preset for.
    session
        .add_channel_of(InstrumentKind::Flopsynth)
        .expect("adds");
    let made = session.channels().len() - 1;
    session.select_channel(made);
    assert_eq!(
        session
            .preset_bar(fontelle_ui::canvas::PresetDevice::Instrument)
            .name,
        None
    );
    std::fs::remove_dir_all(&dir).ok();
}
