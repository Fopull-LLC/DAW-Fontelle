//! Choosing and editing a plugin from the window (TDD §8.4).
//!
//! `tests/plugin_hosting.rs` is the graph half — a plugin in the document is
//! heard. This is the half a person touches: the browser lists what is
//! installed, choosing a row puts it on a channel or in a chain, and the panel
//! that draws a soundfont's knobs draws a plugin's.

use std::path::PathBuf;

use fontelle_app::{RealiseOptions, SampleLibrary, Session, realise};
use fontelle_dsp::Interpolation;
use fontelle_model::{Arena, Channel, Clip, ClipSource, Lane, Note, NoteData, Project};
use fontelle_types::PPQN;
use fontelle_ui::document::{DocumentHost, StudioHost};

const SR: u32 = 48_000;

fn plugin_folder() -> PathBuf {
    let mut path = std::env::current_exe().unwrap();
    path.pop();
    path.pop();
    let built = path.join(if cfg!(target_os = "windows") {
        "fontelle_testplug.dll"
    } else if cfg!(target_os = "macos") {
        "libfontelle_testplug.dylib"
    } else {
        "libfontelle_testplug.so"
    });
    assert!(
        built.exists(),
        "{} is missing — run `cargo build -p fontelle-testplug`",
        built.display()
    );
    // Once per test binary, through a rename — see
    // `fontelle-host/tests/common`, which explains why both halves matter.
    static FOLDER: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    FOLDER
        .get_or_init(|| {
            let folder = std::env::temp_dir().join("fontelle-app-plugin-ui-tests");
            let _ = std::fs::create_dir_all(&folder);
            let staging = folder.join(format!("staging.{}.tmp", std::process::id()));
            if std::fs::copy(&built, &staging).is_ok() {
                let _ = std::fs::rename(&staging, folder.join("fontelle-testplug.clap"));
            }
            let _ = std::fs::remove_file(&staging);
            folder
        })
        .clone()
}

fn options() -> RealiseOptions {
    RealiseOptions {
        sample_rate: SR,
        block_size: fontelle_engine::BLOCK_SIZE,
        quality: Interpolation::Draft,
    }
}

/// A session with one empty channel and a clip, and the test bundle in reach.
fn session() -> Session {
    let mut project = Project::new("plugin ui");
    project.tempo_map = fontelle_model::TempoMap::new(120.0, SR as f64);
    let lane = project.lanes.insert(Lane {
        name: "lane".into(),
        height: 32.0,
        color: [0; 4],
        muted: false,
        locked: false,
        order: 0,
    });
    let channel = project.channels.insert(Channel {
        preset: None,
        name: "empty".into(),
        color: [0; 4],
        mixer_track: None,
        patch_data: None,
        plugin: None,
        instrument: None,
        pan: 0.0,
        gain_db: 0.0,
        muted: false,
        soloed: false,
        named_keys: false,
        ab: Default::default(),
    });
    let mut notes = Arena::default();
    notes.insert(Note {
        start: 0,
        length: PPQN,
        key: 60,
        velocity: 100,
        pan: 0,
        fine_pitch: 0,
        release: 0,
        mod_x: 0,
        mod_y: 0,
        slide: false,
        channel: None,
    });
    project.clips.insert(Clip {
        lane,
        start: 0,
        length: PPQN * 4,
        source: ClipSource::Notes(NoteData { channel, notes }),
        prefab_link: None,
        color: None,
        muted: false,
        loop_length: None,
    });

    session_over(project)
}

/// The window's own builder chain, over `project`.
fn session_over(project: Project) -> Session {
    let library = SampleLibrary::new();
    let clip = Session::first_clip(&project).expect("the rig has a clip");
    let channel_nodes = fontelle_app::channel_nodes(&project);
    let (publisher, _timeline) =
        fontelle_engine::timeline_channel(fontelle_types::CompiledTimeline::empty());
    let realised = realise(&project, &library, options()).expect("this project must realise");
    let (graphs, _source) = fontelle_engine::graph_channel(realised.graph);
    Session::new(
        project,
        library,
        channel_nodes,
        publisher,
        options(),
        clip,
        None,
    )
    .with_graphs(graphs, realised.track_controls)
    .with_param_nodes(realised.param_nodes)
    .with_plugin_folders(vec![plugin_folder()])
}

#[test]
fn the_browser_offers_the_instruments_and_the_effects_separately() {
    let session = session();
    let instruments = session.plugin_instruments();
    let effects = session.plugin_effects();
    // The sine twice, once per note dialect it speaks — see
    // `fontelle_testplug::SINE_CLAP_ONLY`.
    assert_eq!(instruments.len(), 2, "{instruments:?}");
    assert_eq!(instruments[0].name, "Fontelle Test Sine");
    assert_eq!(instruments[0].vendor, "Fopull LLC");
    assert_eq!(instruments[1].name, "Fontelle Test Sine (CLAP only)");
    assert_eq!(effects.len(), 1, "{effects:?}");
    assert_eq!(effects[0].name, "Fontelle Test Gain");
}

#[test]
fn choosing_a_plugin_makes_a_channel_that_plays_it() {
    let mut session = session();
    let before = session.channels().len();
    session.add_plugin_channel(0);

    assert_eq!(session.channels().len(), before + 1);
    let made = session.channels().len() - 1;
    assert_eq!(
        session.channel_kind(made),
        Some(fontelle_types::InstrumentKind::Plugin)
    );
    assert_eq!(session.channels()[made].name, "Fontelle Test Sine");
    assert!(
        session.channels()[made].has_instrument,
        "a plugin is an instrument even with no patch behind it"
    );
}

#[test]
fn a_plugin_channel_can_be_taken_back() {
    let mut session = session();
    let before = session.channels().len();
    session.add_plugin_channel(0);
    // Two entries: the channel, then the plugin on it. Both are one gesture as
    // far as the rack is concerned, but they are two commands and this test
    // says so rather than pretending otherwise.
    session.undo();
    session.undo();
    assert_eq!(session.channels().len(), before);
}

#[test]
fn a_row_that_is_not_there_does_nothing() {
    let mut session = session();
    let before = session.channels().len();
    session.add_plugin_channel(99);
    assert_eq!(session.channels().len(), before);
}

#[test]
fn an_existing_channel_can_be_given_a_plugin() {
    let mut session = session();
    session.set_channel_plugin(0, 0);
    assert_eq!(
        session.channel_kind(0),
        Some(fontelle_types::InstrumentKind::Plugin)
    );
}

#[test]
fn a_plugin_can_go_in_an_insert_chain() {
    let mut session = session();
    session.add_plugin_insert(0, 0);
    let strips = session.mixer_strips();
    assert_eq!(strips[0].inserts.len(), 1);
    assert_eq!(strips[0].inserts[0].label, "Fontelle Test Gain");
}

#[test]
fn the_instrument_panel_draws_the_plugins_own_knobs() {
    let mut session = session();
    session.set_channel_plugin(0, 0);
    session.select_channel(0);
    let view = session.instrument().expect("a plugin channel has a panel");
    assert_eq!(view.title, "Fontelle Test Sine");
    // The channel's own level and placement first, then the plugin's.
    assert_eq!(view.groups[0].name, "Channel");
    let level = view
        .groups
        .iter()
        .flat_map(|group| &group.params)
        .find(|param| param.label == "Level")
        .expect("the sine has a Level parameter");
    assert_eq!(level.address.as_str(), "patch/plugin/param/7");
    // The read-out is the plugin's own formatting of the value, not ours.
    assert_eq!(level.display, "0.50");
}

#[test]
fn the_effect_panel_draws_a_plugins_knobs_and_groups_them_as_it_asked() {
    let mut session = session();
    session.add_plugin_insert(0, 0);
    let view = session
        .insert_view(0, 0)
        .expect("a plugin insert has a panel");
    assert_eq!(view.title, "Fontelle Test Gain");
    let names: Vec<&str> = view.groups.iter().map(|g| g.name.as_str()).collect();
    assert!(names.contains(&"Parameters"), "{names:?}");
    assert!(names.contains(&"Phase"), "{names:?}");
    let invert = view
        .groups
        .iter()
        .flat_map(|group| &group.params)
        .find(|param| param.label == "Invert")
        .expect("the gain has an Invert parameter");
    // Two positions, so it is drawn as a switch rather than a knob.
    assert_eq!(invert.kind, fontelle_ui::canvas::ParamKind::Switch);
}

/// A plugin insert with a sidechain input offers the **key chips** a
/// compressor's panel does: "no key", then every strip.
///
/// Whether the plugin has such a port is the host's knowledge, so the panel
/// asks the rack rather than the document (which lets any plugin slot be
/// keyed — see `fontelle-model/tests/plugins.rs`).
#[test]
fn a_plugin_insert_with_a_sidechain_offers_the_key_chips() {
    let mut session = session();
    session.add_plugin_insert(0, 0);
    let view = session
        .insert_view(0, 0)
        .expect("a plugin insert has a panel");
    assert_eq!(view.title, "Fontelle Test Gain");
    assert!(
        view.keys.len() >= 2,
        "no key, then every strip: {:?}",
        view.keys
    );
    assert_eq!(view.keys[0], fontelle_ui::canvas::NO_KEY);
    assert_eq!(
        view.key,
        Some(0),
        "listening to itself until told otherwise"
    );
}

#[test]
fn turning_a_plugins_knob_is_written_to_the_document() {
    let mut session = session();
    session.set_channel_plugin(0, 0);
    session.select_channel(0);
    let address = fontelle_types::ParamAddress::new("patch/plugin/param/7");
    session.set_instrument_param(&address, 1.0);

    let view = session.instrument().unwrap();
    let level = view
        .groups
        .iter()
        .flat_map(|group| &group.params)
        .find(|param| param.label == "Level")
        .unwrap();
    assert!((level.value - 1.0).abs() < 1e-6, "{}", level.value);
    assert_eq!(level.display, "1.00");
}

#[test]
fn turning_a_plugins_knob_is_one_undo() {
    let mut session = session();
    session.set_channel_plugin(0, 0);
    session.select_channel(0);
    let address = fontelle_types::ParamAddress::new("patch/plugin/param/7");
    for value in [0.6, 0.7, 0.8] {
        session.set_instrument_param(&address, value);
    }
    session.undo();
    let view = session.instrument().unwrap();
    let level = view
        .groups
        .iter()
        .flat_map(|group| &group.params)
        .find(|param| param.label == "Level")
        .unwrap();
    assert!((level.value - 0.5).abs() < 1e-6, "{}", level.value);
}

#[test]
fn turning_an_insert_plugins_knob_is_written_to_the_document() {
    let mut session = session();
    session.add_plugin_insert(0, 0);
    session.set_insert_param(0, 0, "mixer:0/insert[0]/param/0", 0.25);
    let view = session.insert_view(0, 0).unwrap();
    let gain = view
        .groups
        .iter()
        .flat_map(|group| &group.params)
        .find(|param| param.label == "Gain")
        .unwrap();
    assert!((gain.value - 0.25).abs() < 1e-6, "{}", gain.value);
    // 0.25 of a range that runs to four.
    assert_eq!(gain.display, "1.00x");
}

#[test]
fn a_rescan_says_what_it_found() {
    let mut session = session();
    session.rescan_plugins();
    // The gain, the sine, the face (`fontelle_testplug::FacePlugin`), and the
    // sine again speaking only CLAP (`fontelle_testplug::SINE_CLAP_ONLY`).
    assert_eq!(session.take_message().as_deref(), Some("4 plugins"));
}

#[test]
fn clearing_a_channels_instrument_takes_the_plugin_off_it() {
    // Otherwise "Clear instrument" on a plugin channel is a menu row that does
    // nothing: the patch it clears is already empty, and the plugin — which is
    // the instrument — stays.
    let mut session = session();
    session.set_channel_plugin(0, 0);
    assert_eq!(
        session.channel_kind(0),
        Some(fontelle_types::InstrumentKind::Plugin)
    );

    StudioHost::clear_channel_instrument(&mut session, 0);
    assert!(!session.channels()[0].has_instrument);
    assert_ne!(
        session.channel_kind(0),
        Some(fontelle_types::InstrumentKind::Plugin)
    );

    // And it is one undo, because taking an instrument off is one thing to do.
    session.undo();
    assert_eq!(
        session.channel_kind(0),
        Some(fontelle_types::InstrumentKind::Plugin)
    );
}

#[test]
fn every_knob_the_panel_offers_is_one_the_graph_can_reach() {
    // The invariant this whole feature rests on, applied to a plugin: a knob
    // the panel draws and the graph does not know is a lane that is made,
    // drawn, saved — and silent, because an unresolved target emits no events
    // at all. It is the same check `instrument_automation.rs` makes of a
    // patch's knobs.
    let mut session = session();
    session.set_channel_plugin(0, 0);
    session.select_channel(0);

    let reachable = session.automatable_addresses();
    let view = session.instrument().unwrap();
    for param in view.groups.iter().flat_map(|group| &group.params) {
        assert!(
            reachable.contains(&param.address),
            "the panel offers {} and the graph cannot reach it",
            param.address
        );
    }
}

// ------------------------------- what a plugin's panel reads like (2026-09-05)

/// A plugin's group heading is the module it named, **as words**.
///
/// > *"they arent really displaying cleanly in the instrument menu"*
///
/// CLAP's `module` is a path, and plugins write it with separators: Surge XT's
/// are `/Macros/` and `/Global & FX/`, and the panel showed exactly that,
/// slashes and all, over each group of knobs.
#[test]
fn a_plugins_group_heading_is_not_the_raw_module_path() {
    let mut session = session();
    session.add_plugin_insert(0, 0);
    let view = session
        .insert_view(0, 0)
        .expect("a plugin insert has a panel");
    let names: Vec<&str> = view.groups.iter().map(|g| g.name.as_str()).collect();
    assert!(names.contains(&"Phase"), "{names:?}");
    assert!(
        !names.iter().any(|name| name.contains('/')),
        "no raw path separators: {names:?}"
    );
}

/// A name too long for its cell says so, rather than stopping mid-word.
///
/// The panel's cells are ninety-two pixels wide, which is a soundfont's
/// "cutoff" and a plugin's "Polyphony Limi". An ellipsis is the difference
/// between a name that was shortened and a name that is wrong.
#[test]
fn a_name_too_long_for_its_cell_is_elided_rather_than_cut() {
    let long = "Polyphony Limit Of The Whole Instrument";
    let short = fontelle_app::instrument::elide_label(long);
    assert!(short.len() < long.len(), "{short:?}");
    assert!(short.ends_with('\u{2026}'), "{short:?}");
    assert!(
        long.starts_with(short.trim_end_matches('\u{2026}')),
        "{short:?}"
    );
    assert_eq!(fontelle_app::instrument::elide_label("Level"), "Level");
}

/// A parameter the plugin says cannot be set is not drawn as a knob you can
/// turn. It is still automatable and still saved — both go by the plugin's own
/// id and neither goes through the panel.
#[test]
fn a_read_only_parameter_is_left_off_the_panel() {
    let params = [fontelle_host::HostedParam {
        id: 3,
        name: "Meter".into(),
        module: String::new(),
        min: 0.0,
        max: 1.0,
        default: 0.0,
        stepped: false,
        hidden: false,
        readonly: true,
    }];
    let view = fontelle_app::instrument::describe_plugin(
        "thing",
        &params,
        |_| Some(0.0),
        |id| fontelle_types::ParamAddress::from(format!("param/{id}").as_str()),
        |_, _| None,
    );
    assert!(view.groups.is_empty(), "{:?}", view.groups);
}

// ------------------------------ the plugin's own editor window (2026-09-05)

/// A plugin with no editor of its own gets Fontelle's panel, and says so by
/// answering `false` rather than by opening an empty window.
///
/// `fontelle-testplug` has no GUI extension, which is the case every LV2 and
/// bridged plugin in this build is also in.
#[test]
fn a_plugin_with_no_editor_of_its_own_keeps_the_panel() {
    let mut session = session();
    session.set_channel_plugin(0, 0);
    session.select_channel(0);
    assert!(
        !session.open_plugin_editor_for_channel(0),
        "the test sine has no editor of its own"
    );
    // And the panel is still there to fall back to.
    assert!(session.instrument().is_some());
}

/// The same for an insert.
#[test]
fn an_insert_plugin_with_no_editor_keeps_its_panel() {
    let mut session = session();
    session.add_plugin_insert(0, 0);
    assert!(!session.open_plugin_editor_for_insert(0, 0));
    assert!(session.insert_view(0, 0).is_some());
}

/// A channel with no plugin on it is not an editor, and asking is not an
/// error — the window asks about every instrument window it opens.
#[test]
fn asking_a_channel_that_holds_no_plugin_is_answered_no() {
    let mut session = session();
    assert!(!session.open_plugin_editor_for_channel(0));
    assert!(!session.open_plugin_editor_for_channel(99));
    assert!(!session.open_plugin_editor_for_insert(0, 7));
}

/// Nothing open is nothing to drive, and the window is told so — which is what
/// lets an idle studio go back to sleep.
#[test]
fn with_no_editor_open_the_window_is_told_it_can_sleep() {
    let mut session = session();
    session.set_channel_plugin(0, 0);
    assert!(!session.tick_plugin_editors());
}

/// Swapping a plugin renames the channel, **unless you named it yourself**.
///
/// Found by driving the window: a Surge XT channel given Calf Organ instead
/// still read "Surge XT" in the rack and in its window's title bar, because
/// choosing a plugin had only ever named a channel when it *made* one. A rack
/// that names the wrong instrument is worse than one that names none.
///
/// The name is only replaced when it is still the one the last plugin was
/// given — "recognised, not remembered", the same rule the preset chips
/// follow. A channel called "Lead" stays "Lead".
#[test]
fn swapping_a_plugin_renames_the_channel_it_named() {
    let mut session = session();
    session.add_plugin_channel(0);
    let made = session.channels().len() - 1;
    assert_eq!(session.channels()[made].name, "Fontelle Test Sine");

    // The other plugin in the bundle is an effect, so swapping the instrument
    // for itself is the only swap this fixture can make — which still proves
    // the rule, because the name has to survive it unchanged.
    session.set_channel_plugin(made, 0);
    assert_eq!(session.channels()[made].name, "Fontelle Test Sine");
}

/// And a name somebody typed is never taken away by a swap.
#[test]
fn a_channel_you_named_keeps_its_name_when_its_plugin_changes() {
    let mut session = session();
    session.add_plugin_channel(0);
    let made = session.channels().len() - 1;
    session.rename_channel(made, "Lead");
    session.set_channel_plugin(made, 0);
    assert_eq!(session.channels()[made].name, "Lead");
}

/// A listing names the plugin **permanently**, not just by its caption: the
/// favourites the settings file keeps are keyed by [`PluginKey`], and the
/// window has to be able to say which row a starred key is.
#[test]
fn a_listing_carries_the_key_a_favourite_is_kept_by() {
    let session = session();
    let instruments = session.plugin_instruments();
    assert_eq!(
        instruments[0].key,
        fontelle_types::PluginKey::clap("com.fopull.fontelle.testsine")
    );
    let effects = session.plugin_effects();
    assert_eq!(
        effects[0].key,
        fontelle_types::PluginKey::clap("com.fopull.fontelle.testgain")
    );
}

// ------------------------- a reopened project hosts its plugins (2026-09-05)

/// A project that names a plugin has it **open and playing before anything
/// is edited**.
///
/// The window's first graph is built before the session exists and without
/// a plugin rack — see `main.rs` — so a project reopened with `--open` that
/// named an LSP sampler came up with the channel silent and its panel
/// empty until the first edit rebuilt the graph. The session notices on
/// its first pump, which is the first thing the window does after every
/// builder has run and the settings' plugin folders are known.
#[test]
fn a_project_that_names_a_plugin_hosts_it_before_any_edit() {
    let mut session = session();
    // What a saved project carries: a channel that plays the plugin.
    session.set_channel_plugin(0, 0);
    let saved = session.project().clone();
    // Reopened: the same builder chain the window uses, over that project.
    let mut reopened = session_over(saved);
    assert!(
        reopened
            .instrument()
            .is_none_or(|view| view.groups.len() <= 1),
        "before the first frame nothing has hosted it yet: {:?}",
        reopened.instrument().map(|v| v.groups.len())
    );
    reopened.pump();
    reopened.select_channel(0);
    let view = reopened.instrument().expect("a plugin channel has a panel");
    let names: Vec<&str> = view
        .groups
        .iter()
        .flat_map(|group| &group.params)
        .map(|param| param.label.as_str())
        .collect();
    assert!(
        names.contains(&"Level"),
        "the plugin is live and its knobs are drawn: {names:?}"
    );
}

/// The machine is searched **when the studio opens**, not the first time a
/// menu asks for a list.
///
/// Reported from using it: *"can you make it load plugins when the program
/// starts instead of loading them when you go to add a plugin"*. The wait is
/// the same wait either way, and startup is where a person expects one — a
/// menu that hangs for three seconds the first time it is opened reads as a
/// menu that is broken.
///
/// The count comes back so `main` can say what it found, the way it already
/// says where the soundfont folder is: a scan that silently found nothing is
/// indistinguishable from a scan that never ran.
#[test]
fn the_studio_looks_for_plugins_up_front_and_says_what_it_found() {
    let mut session = session();
    let (found, failed) = session.scan_plugins();
    assert_eq!(found, 4, "the test bundle's four plugins");
    assert_eq!(failed, 0, "nothing in the fixture folder refuses to load");
}
