//! A plugin somebody else wrote, end to end: chosen in the document, opened by
//! the rack, scheduled by `realise`, and heard (TDD §8.4).
//!
//! `fontelle-host` proves the ABI, `fontelle-engine` proves the node. This is
//! the seam that makes them a feature: which plugins the document is asking
//! for, and what happens to them when the graph is rebuilt underneath.

use std::path::PathBuf;

use fontelle_app::{PluginRack, PluginSlot, RealiseOptions, SampleLibrary, realise_hosting};
use fontelle_dsp::Interpolation;
use fontelle_model::{Arena, Channel, Clip, ClipSource, EffectSlot, Lane, Note, NoteData, Project};
use fontelle_types::{InstrumentKind, PPQN, PluginKey, PluginState};

const SR: u32 = 48_000;
const GAIN: &str = "com.fopull.fontelle.testgain";
const SINE: &str = "com.fopull.fontelle.testsine";

/// What cargo names the test plugin's `cdylib` on this platform — the scan
/// renames it to `.clap`, which is the real rule (see
/// `fontelle-host/tests/common`).
fn testplug_library() -> &'static str {
    if cfg!(target_os = "windows") {
        "fontelle_testplug.dll"
    } else if cfg!(target_os = "macos") {
        "libfontelle_testplug.dylib"
    } else {
        "libfontelle_testplug.so"
    }
}

/// A folder holding nothing but the test bundle, so a scan of it is a known
/// list rather than whatever happens to be installed on this machine.
fn plugin_folder() -> PathBuf {
    let mut path = std::env::current_exe().unwrap();
    path.pop();
    path.pop();
    let built = path.join(testplug_library());
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
            let folder = std::env::temp_dir().join("fontelle-app-plugin-tests");
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

fn fresh_rack() -> PluginRack {
    let mut rack = PluginRack::new();
    // **Only** the fixture's folder. Without this the rack also walks the
    // folders CLAP and LV2 nominate, and a test that counts what it found is
    // counting whatever this machine happens to have installed — which went
    // from nothing to 370 the day real plugins were put on it, and took four
    // tests with it. The same argument `fontelle-testplug` exists for.
    rack.search_standard_folders(false);
    rack.set_folders(vec![plugin_folder()]);
    rack.rescan();
    rack
}

#[test]
fn a_rack_can_be_told_to_look_only_where_it_is_pointed() {
    // The switch the fixtures rely on, stated as its own claim: with the
    // standard folders off, the list is exactly what was put in front of it.
    let mut only = PluginRack::new();
    only.search_standard_folders(false);
    only.set_folders(vec![plugin_folder()]);
    only.rescan();
    assert!(
        only.folders().iter().all(|f| f == &plugin_folder()),
        "{:?}",
        only.folders()
    );
    // The gain, the sine, the face, and the sine again speaking only CLAP.
    assert_eq!(only.scan().plugins.len(), 4, "{:#?}", only.scan().plugins);

    // And with them on it looks in the places an installer uses, which is
    // what the studio needs and what makes the switch worth having.
    let mut standard = PluginRack::new();
    standard.set_folders(Vec::new());
    assert!(
        standard.folders().len() > 1,
        "the studio stopped searching the standard folders: {:?}",
        standard.folders()
    );
}

fn options() -> RealiseOptions {
    RealiseOptions {
        sample_rate: SR,
        block_size: fontelle_engine::BLOCK_SIZE,
        quality: Interpolation::Draft,
    }
}

/// A project with one channel, one lane, and a held note on it.
fn project_with_a_held_note() -> (Project, fontelle_types::ChannelId) {
    let mut project = Project::new("plugins");
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
        name: "plug".into(),
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
        length: PPQN * 8,
        key: 69,
        velocity: 127,
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
        length: PPQN * 8,
        source: ClipSource::Notes(NoteData { channel, notes }),
        prefab_link: None,
        color: None,
        muted: false,
        loop_length: None,
    });
    (project, channel)
}

/// Renders a quarter of a second of `project` with `rack`'s plugins in it.
fn render(project: &Project, rack: &mut PluginRack) -> Vec<f32> {
    let library = SampleLibrary::new();
    let wiring = rack.realise(project, SR as f64, fontelle_engine::BLOCK_SIZE as u32);
    let mut realised = realise_hosting(
        project,
        &library,
        options(),
        &Default::default(),
        None,
        &Default::default(),
        None,
        None,
        &wiring,
    )
    .expect("this project must realise");
    let timeline =
        fontelle_sequencer::compile(project, &realised.channel_nodes, &Default::default());
    fontelle_app::render_offline(&timeline, &mut realised.graph, SR as i64 / 4)
}

fn peak(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0f32, |m, s| m.max(s.abs()))
}

#[test]
fn the_rack_lists_what_is_in_the_folders_it_is_given() {
    let rack = fresh_rack();
    let ids: Vec<_> = rack
        .scan()
        .plugins
        .iter()
        .map(|p| p.key.id.as_str())
        .collect();
    assert!(ids.contains(&GAIN), "{ids:?}");
    assert!(ids.contains(&SINE), "{ids:?}");
    // Two instruments: the sine twice, once per note dialect it speaks —
    // see `fontelle_testplug::SINE_CLAP_ONLY`.
    assert_eq!(rack.scan().instruments().count(), 2);
    assert_eq!(rack.scan().effects().count(), 1);
}

#[test]
fn a_channel_playing_a_plugin_is_heard_in_the_realised_graph() {
    let (mut project, channel) = project_with_a_held_note();
    project.channels[channel].instrument = Some(InstrumentKind::Plugin);
    project.channels[channel].plugin = Some(PluginState::new(PluginKey::clap(SINE), "Sine"));

    let mut rack = fresh_rack();
    let out = render(&project, &mut rack);
    assert!(peak(&out) > 0.05, "{}", peak(&out));
}

#[test]
fn a_plugins_own_parameter_decides_what_it_sounds_like() {
    let (mut project, channel) = project_with_a_held_note();
    let mut state = PluginState::new(PluginKey::clap(SINE), "Sine");
    state.set_param(7, 0.1);
    project.channels[channel].instrument = Some(InstrumentKind::Plugin);
    project.channels[channel].plugin = Some(state.clone());

    let mut rack = fresh_rack();
    let quiet = peak(&render(&project, &mut rack));

    state.set_param(7, 0.8);
    project.channels[channel].plugin = Some(state);
    let mut rack = fresh_rack();
    let loud = peak(&render(&project, &mut rack));

    assert!(loud > quiet * 2.0, "quiet {quiet}, loud {loud}");
}

#[test]
fn an_insert_holding_a_plugin_processes_the_bus() {
    let (mut project, channel) = project_with_a_held_note();
    project.channels[channel].instrument = Some(InstrumentKind::Plugin);
    project.channels[channel].plugin = Some(PluginState::new(PluginKey::clap(SINE), "Sine"));
    let master = project.mixer.master.unwrap();

    let mut rack = fresh_rack();
    let dry = peak(&render(&project, &mut rack));

    let mut gain = PluginState::new(PluginKey::clap(GAIN), "Gain");
    gain.set_param(0, 0.25);
    project.mixer.tracks[master]
        .inserts
        .push(EffectSlot::hosting(gain));

    let mut rack = fresh_rack();
    let quartered = peak(&render(&project, &mut rack));
    assert!(
        quartered < dry * 0.5,
        "dry {dry}, through the plugin {quartered}"
    );
}

#[test]
fn a_bypassed_plugin_insert_leaves_the_bus_alone() {
    let (mut project, channel) = project_with_a_held_note();
    project.channels[channel].instrument = Some(InstrumentKind::Plugin);
    project.channels[channel].plugin = Some(PluginState::new(PluginKey::clap(SINE), "Sine"));
    let master = project.mixer.master.unwrap();

    let mut rack = fresh_rack();
    let dry = peak(&render(&project, &mut rack));

    let mut gain = PluginState::new(PluginKey::clap(GAIN), "Gain");
    gain.set_param(0, 0.25);
    let mut slot = EffectSlot::hosting(gain);
    slot.bypassed = true;
    project.mixer.tracks[master].inserts.push(slot);

    let mut rack = fresh_rack();
    let bypassed = peak(&render(&project, &mut rack));
    assert!((bypassed - dry).abs() < 1e-4, "dry {dry}, off {bypassed}");
}

#[test]
fn a_bypass_is_heard_without_the_graph_being_rebuilt() {
    // A bypass is a switch somebody flicks while listening, and rebuilding the
    // graph to flick it would reload every soundfont in the project. So it
    // reaches the running node the way a fader does — see `PluginWiring`.
    let (mut project, channel) = project_with_a_held_note();
    project.channels[channel].instrument = Some(InstrumentKind::Plugin);
    project.channels[channel].plugin = Some(PluginState::new(PluginKey::clap(SINE), "Sine"));
    let master = project.mixer.master.unwrap();
    let mut gain = PluginState::new(PluginKey::clap(GAIN), "Gain");
    gain.set_param(0, 0.25);
    project.mixer.tracks[master]
        .inserts
        .push(EffectSlot::hosting(gain));

    let mut rack = fresh_rack();
    let library = SampleLibrary::new();
    let wiring = rack.realise(&project, SR as f64, fontelle_engine::BLOCK_SIZE as u32);
    let mut realised = realise_hosting(
        &project,
        &library,
        options(),
        &Default::default(),
        None,
        &Default::default(),
        None,
        None,
        &wiring,
    )
    .unwrap();
    let timeline =
        fontelle_sequencer::compile(&project, &realised.channel_nodes, &Default::default());

    let quartered = peak(&fontelle_app::render_offline(
        &timeline,
        &mut realised.graph,
        SR as i64 / 8,
    ));
    rack.set_bypassed(
        PluginSlot::Insert {
            track: master,
            slot: 0,
        },
        true,
    );
    let through = peak(&fontelle_app::render_offline(
        &timeline,
        &mut realised.graph,
        SR as i64 / 8,
    ));
    assert!(
        through > quartered * 2.0,
        "quartered {quartered}, bypassed {through}"
    );
}

#[test]
fn a_plugin_channels_own_level_is_applied_to_what_the_plugin_wrote() {
    // The channel's, not its mixer track's: several channels may share a
    // track, so a panel that reached for the fader would be two channels'
    // panels turning one knob. See `Channel::gain_db`.
    let (mut project, channel) = project_with_a_held_note();
    project.channels[channel].instrument = Some(InstrumentKind::Plugin);
    project.channels[channel].plugin = Some(PluginState::new(PluginKey::clap(SINE), "Sine"));

    let mut rack = fresh_rack();
    let unity = peak(&render(&project, &mut rack));

    project.channels[channel].gain_db = -12.0;
    let mut rack = fresh_rack();
    let quieter = peak(&render(&project, &mut rack));

    let ratio = quieter / unity;
    assert!(
        (ratio - 0.251).abs() < 0.02,
        "unity {unity}, at -12 dB {quieter} (ratio {ratio})"
    );
}

#[test]
fn a_plugin_channel_can_be_placed_in_the_stereo_field() {
    let (mut project, channel) = project_with_a_held_note();
    project.channels[channel].instrument = Some(InstrumentKind::Plugin);
    project.channels[channel].plugin = Some(PluginState::new(PluginKey::clap(SINE), "Sine"));
    project.channels[channel].pan = -1.0;

    let mut rack = fresh_rack();
    let out = render(&project, &mut rack);
    let left = out.iter().step_by(2).fold(0.0f32, |m, s| m.max(s.abs()));
    let right = out
        .iter()
        .skip(1)
        .step_by(2)
        .fold(0.0f32, |m, s| m.max(s.abs()));
    assert!(left > 0.05, "{left}");
    assert!(right < left * 0.05, "left {left}, right {right}");
}

#[test]
fn a_plugin_that_is_not_installed_leaves_a_silent_channel_and_the_project_still_opens() {
    let (mut project, channel) = project_with_a_held_note();
    project.channels[channel].instrument = Some(InstrumentKind::Plugin);
    project.channels[channel].plugin = Some(PluginState::new(
        PluginKey::clap("com.example.not.here"),
        "Missing",
    ));

    let mut rack = fresh_rack();
    let out = render(&project, &mut rack);
    assert!(peak(&out) < 1e-6, "{}", peak(&out));
}

#[test]
fn a_project_realised_with_no_host_at_all_still_renders() {
    // Every offline path — a bounce, a test, `--render` — goes through an
    // entry point that has no plugins. It must produce a file rather than an
    // error.
    let (mut project, channel) = project_with_a_held_note();
    project.channels[channel].instrument = Some(InstrumentKind::Plugin);
    project.channels[channel].plugin = Some(PluginState::new(PluginKey::clap(SINE), "Sine"));
    let library = SampleLibrary::new();
    let realised = fontelle_app::realise(&project, &library, options());
    assert!(realised.is_ok());
}

#[test]
fn a_plugin_is_kept_open_across_a_rebuild() {
    let (mut project, channel) = project_with_a_held_note();
    project.channels[channel].instrument = Some(InstrumentKind::Plugin);
    project.channels[channel].plugin = Some(PluginState::new(PluginKey::clap(SINE), "Sine"));

    let mut rack = fresh_rack();
    let first = rack.realise(&project, SR as f64, fontelle_engine::BLOCK_SIZE as u32);
    let before = rack
        .plugin(PluginSlot::Channel(channel))
        .map(|p| p.name().to_string());
    assert_eq!(before.as_deref(), Some("Fontelle Test Sine"));

    let second = rack.realise(&project, SR as f64, fontelle_engine::BLOCK_SIZE as u32);
    assert_eq!(rack.counts(), (1, 0));
    // The same bay, which is what says the plugin was not reopened: a fresh
    // one would be a fresh instance and a reloaded sampler.
    assert!(std::sync::Arc::ptr_eq(
        &first[&PluginSlot::Channel(channel)].bay,
        &second[&PluginSlot::Channel(channel)].bay
    ));
}

#[test]
fn a_plugin_whose_slot_has_gone_is_closed() {
    let (mut project, channel) = project_with_a_held_note();
    project.channels[channel].instrument = Some(InstrumentKind::Plugin);
    project.channels[channel].plugin = Some(PluginState::new(PluginKey::clap(SINE), "Sine"));

    let mut rack = fresh_rack();
    let wiring = rack.realise(&project, SR as f64, fontelle_engine::BLOCK_SIZE as u32);
    assert_eq!(rack.counts(), (1, 0));
    drop(wiring);

    project.channels[channel].instrument = None;
    project.channels[channel].plugin = None;
    rack.realise(&project, SR as f64, fontelle_engine::BLOCK_SIZE as u32);
    assert_eq!(rack.counts(), (0, 0), "nothing open and nothing waiting");
}

#[test]
fn a_plugin_swapped_for_another_one_is_the_other_one() {
    let (mut project, _channel) = project_with_a_held_note();
    let master = project.mixer.master.unwrap();
    project.mixer.tracks[master]
        .inserts
        .push(EffectSlot::hosting(PluginState::new(
            PluginKey::clap(GAIN),
            "Gain",
        )));

    let mut rack = fresh_rack();
    rack.realise(&project, SR as f64, fontelle_engine::BLOCK_SIZE as u32);
    let slot = PluginSlot::Insert {
        track: master,
        slot: 0,
    };
    assert_eq!(
        rack.plugin(slot).map(|p| p.name()),
        Some("Fontelle Test Gain")
    );

    project.mixer.tracks[master].inserts[0] =
        EffectSlot::hosting(PluginState::new(PluginKey::clap(SINE), "Sine"));
    rack.realise(&project, SR as f64, fontelle_engine::BLOCK_SIZE as u32);
    assert_eq!(
        rack.plugin(slot).map(|p| p.name()),
        Some("Fontelle Test Sine")
    );
}

#[test]
fn what_a_plugin_was_set_to_comes_back_after_a_reopen() {
    let (mut project, _channel) = project_with_a_held_note();
    let master = project.mixer.master.unwrap();
    project.mixer.tracks[master]
        .inserts
        .push(EffectSlot::hosting(PluginState::new(
            PluginKey::clap(GAIN),
            "Gain",
        )));

    let mut rack = fresh_rack();
    let slot = PluginSlot::Insert {
        track: master,
        slot: 0,
    };
    rack.realise(&project, SR as f64, fontelle_engine::BLOCK_SIZE as u32);
    rack.set_param(slot, 0, 3.5);
    let snapshot = rack.snapshot(slot).unwrap();
    assert_eq!(snapshot.param(0), Some(3.5));

    // Written back to the document, saved, reopened — which is a fresh rack
    // reading the file.
    project.mixer.tracks[master].inserts[0].plugin = Some(snapshot);
    let json = serde_json::to_string(&project).unwrap();
    let reopened: Project = serde_json::from_str(&json).unwrap();

    let mut rack = fresh_rack();
    rack.realise(&reopened, SR as f64, fontelle_engine::BLOCK_SIZE as u32);
    assert_eq!(rack.snapshot(slot).unwrap().param(0), Some(3.5));
}

#[test]
fn a_plugin_knob_moved_now_is_heard_without_a_rebuild() {
    let (mut project, channel) = project_with_a_held_note();
    project.channels[channel].instrument = Some(InstrumentKind::Plugin);
    project.channels[channel].plugin = Some(PluginState::new(PluginKey::clap(SINE), "Sine"));

    let mut rack = fresh_rack();
    let library = SampleLibrary::new();
    let wiring = rack.realise(&project, SR as f64, fontelle_engine::BLOCK_SIZE as u32);
    let mut realised = realise_hosting(
        &project,
        &library,
        options(),
        &Default::default(),
        None,
        &Default::default(),
        None,
        None,
        &wiring,
    )
    .unwrap();
    let timeline =
        fontelle_sequencer::compile(&project, &realised.channel_nodes, &Default::default());

    rack.set_param(PluginSlot::Channel(channel), 7, 0.05);
    let quiet = peak(&fontelle_app::render_offline(
        &timeline,
        &mut realised.graph,
        SR as i64 / 8,
    ));
    rack.set_param(PluginSlot::Channel(channel), 7, 0.9);
    let loud = peak(&fontelle_app::render_offline(
        &timeline,
        &mut realised.graph,
        SR as i64 / 8,
    ));
    assert!(loud > quiet * 2.0, "quiet {quiet}, loud {loud}");
}

// ----------------------------------- a plugin's sidechain, from a track (2026-09-05)

/// A plugin insert keyed to another track **hears** that track.
///
/// The machinery is the one the built-in compressor already rides: the
/// slot names a track, the compiler schedules that track first and leaves
/// its bus in a `KeyTap`, and the insert reads the tap. What is new is the
/// last step — the tap goes into the plugin's sidechain port rather than a
/// detector this program wrote.
#[test]
fn a_plugin_insert_keyed_to_another_track_hears_that_track() {
    use fontelle_model::{AddMixerTrack, Command};
    let (mut project, pad_channel) = project_with_a_held_note();
    // Two tracks: the pad, whose insert listens, and the kick, which it
    // listens to. Both carry a sine channel holding the same note.
    AddMixerTrack::new("Kick".to_string())
        .apply(&mut project)
        .expect("a track");
    AddMixerTrack::new("Pad".to_string())
        .apply(&mut project)
        .expect("a track");
    let master = project.mixer.master.unwrap();
    let find = |project: &Project, name: &str| {
        project
            .mixer
            .tracks
            .iter()
            .find(|(_, track)| track.name == name)
            .map(|(id, _)| id)
            .unwrap()
    };
    let (kick, pad) = (find(&project, "Kick"), find(&project, "Pad"));
    assert_ne!(kick, master);
    project.channels[pad_channel].instrument = Some(InstrumentKind::Plugin);
    project.channels[pad_channel].plugin = Some(PluginState::new(PluginKey::clap(SINE), "Sine"));
    project.channels[pad_channel].mixer_track = Some(pad);
    // The kick channel plays the same clip, at full level, on a track whose
    // **output is off**: it still keys — a key is an edge of its own — but
    // what reaches the master is the pad alone, so the pad's ducking is
    // measured and not the kick's loudness.
    let kick_channel = project
        .channels
        .insert(project.channels[pad_channel].clone());
    project.channels[kick_channel].mixer_track = Some(kick);
    let mut loud = PluginState::new(PluginKey::clap(SINE), "Sine");
    loud.set_param(7, 1.0);
    project.channels[kick_channel].plugin = Some(loud);
    project.mixer.tracks[kick].output_on = false;
    if let Some(clip) = project.clips.values_mut().next()
        && let ClipSource::Notes(data) = &mut clip.source
    {
        let mut kick_notes = data.notes.clone();
        for note in kick_notes.values_mut() {
            note.channel = Some(kick_channel);
        }
        for (_, note) in kick_notes.iter() {
            data.notes.insert(*note);
        }
    }
    // The pad's insert: the fixture gain, which ducks by whatever is on its
    // sidechain.
    project.mixer.tracks[pad]
        .inserts
        .push(EffectSlot::hosting(PluginState::new(
            PluginKey::clap(GAIN),
            "Gain",
        )));

    let mut rack = fresh_rack();
    let unkeyed = peak(&render(&project, &mut rack));
    assert!(unkeyed > 0.1, "{unkeyed}");

    fontelle_model::SetInsertKey::new(pad, 0, Some(kick))
        .apply(&mut project)
        .expect("a plugin insert takes a key");
    let mut rack = fresh_rack();
    let keyed = peak(&render(&project, &mut rack));
    assert!(
        keyed < unkeyed * 0.75,
        "the pad was not ducked by the kick: {unkeyed} unkeyed, {keyed} keyed"
    );
}

// LV2 is hosted on Linux only (`fontelle-host`'s `lv2_stub.rs` says why), and
// so is the bridge, which is a `.so` the rack dlopens. Everything that needs
// either lives in this module.
#[cfg(target_os = "linux")]
mod linux_only {
    use super::*;

    // ------------------------------------------------------------------ LV2 ---
    //
    // > *"i agree with implementing LV2 for sure"*
    //
    // The same seam, the second format. An LV2 bundle in a plugin folder is
    // found, chosen in the document by its URI, opened, realised and heard —
    // through exactly the code above, because a format is an arm and not a
    // second host.

    const LV2_GAIN: &str = fontelle_testlv2::GAIN_URI;
    const LV2_SINE: &str = fontelle_testlv2::SINE_URI;

    /// A folder holding nothing but the LV2 test bundle. Its own folder rather
    /// than the CLAP one's, so the tests above that count what a CLAP-only
    /// folder holds keep counting one of each.
    fn lv2_folder() -> PathBuf {
        let mut path = std::env::current_exe().unwrap();
        path.pop();
        path.pop();
        let built = path.join("libfontelle_testlv2.so");
        assert!(
            built.exists(),
            "{} is missing — run `cargo build -p fontelle-testlv2`",
            built.display()
        );
        static FOLDER: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
        FOLDER
            .get_or_init(|| {
                let folder = std::env::temp_dir().join("fontelle-app-lv2-tests");
                let _ = std::fs::create_dir_all(&folder);
                let staging = folder.join(format!("lv2-staging.{}", std::process::id()));
                let _ = std::fs::remove_dir_all(&staging);
                std::fs::create_dir_all(&staging).unwrap();
                std::fs::copy(&built, staging.join(fontelle_testlv2::BINARY_NAME)).unwrap();
                std::fs::write(staging.join("manifest.ttl"), fontelle_testlv2::MANIFEST_TTL)
                    .unwrap();
                std::fs::write(staging.join("testlv2.ttl"), fontelle_testlv2::PLUGIN_TTL).unwrap();
                let bundle = folder.join("fontelle-testlv2.lv2");
                let _ = std::fs::remove_dir_all(&bundle);
                std::fs::rename(&staging, &bundle).unwrap();
                folder
            })
            .clone()
    }

    fn rack_with_both_formats() -> PluginRack {
        let mut rack = PluginRack::new();
        rack.search_standard_folders(false);
        rack.set_folders(vec![plugin_folder(), lv2_folder()]);
        rack.rescan();
        rack
    }

    fn lv2(id: &str) -> PluginKey {
        PluginKey::new(fontelle_types::PluginFormat::Lv2, id)
    }

    #[test]
    fn the_rack_lists_lv2_plugins_beside_clap_ones() {
        let rack = rack_with_both_formats();
        let keys: Vec<String> = rack
            .scan()
            .plugins
            .iter()
            .map(|p| p.key.to_string())
            .collect();
        assert!(keys.contains(&format!("lv2:{LV2_GAIN}")), "{keys:?}");
        assert!(keys.contains(&format!("lv2:{LV2_SINE}")), "{keys:?}");
        assert!(keys.contains(&format!("clap:{SINE}")), "{keys:?}");
        // The LV2 sine and the CLAP sine twice, once per note dialect.
        assert_eq!(rack.scan().instruments().count(), 3);
        // Four effects: both gains, and the LV2 gain twice more — under the name
        // that ships no editor (`fontelle_testlv2::PLAIN_URI`) and the one whose
        // editor listens to nothing (`fontelle_testlv2::DEAF_URI`).
        assert_eq!(rack.scan().effects().count(), 4);
    }

    #[test]
    fn a_channel_playing_an_lv2_plugin_is_heard_in_the_realised_graph() {
        let (mut project, channel) = project_with_a_held_note();
        project.channels[channel].instrument = Some(InstrumentKind::Plugin);
        project.channels[channel].plugin = Some(PluginState::new(lv2(LV2_SINE), "Sine"));
        let mut rack = rack_with_both_formats();
        let out = render(&project, &mut rack);
        assert!(peak(&out) > 0.05, "{}", peak(&out));
    }

    #[test]
    fn an_insert_holding_an_lv2_plugin_processes_the_bus() {
        let (mut project, channel) = project_with_a_held_note();
        project.channels[channel].instrument = Some(InstrumentKind::Plugin);
        project.channels[channel].plugin = Some(PluginState::new(lv2(LV2_SINE), "Sine"));
        let master = project.mixer.master.unwrap();

        let mut rack = rack_with_both_formats();
        let dry = peak(&render(&project, &mut rack));

        let mut gain = PluginState::new(lv2(LV2_GAIN), "Gain");
        gain.set_param(2, 0.25);
        project.mixer.tracks[master]
            .inserts
            .push(EffectSlot::hosting(gain));
        let mut rack = rack_with_both_formats();
        let quartered = peak(&render(&project, &mut rack));
        assert!(
            quartered < dry * 0.5,
            "dry {dry}, through the plugin {quartered}"
        );
    }

    #[test]
    fn what_an_lv2_plugin_was_set_to_comes_back_after_a_reopen() {
        // The document keeps the control ports; a new rack puts them back.
        let (mut project, channel) = project_with_a_held_note();
        project.channels[channel].instrument = Some(InstrumentKind::Plugin);
        let mut state = PluginState::new(lv2(LV2_SINE), "Sine");
        state.set_param(2, 0.1);
        project.channels[channel].plugin = Some(state);

        let mut rack = rack_with_both_formats();
        let quiet = peak(&render(&project, &mut rack));
        assert!(quiet > 0.03 && quiet < 0.12, "{quiet}");
        let saved = rack
            .snapshot(PluginSlot::Channel(channel))
            .expect("a snapshot");
        assert_eq!(saved.param(2), Some(0.1));
        assert!(saved.blob.is_none());
    }

    // ------------------------------- a sampler's file, saved while it plays (2026-09-05)

    /// An LV2 plugin's own state is captured **while the graph is playing it**,
    /// and the silence that costs is a handful of blocks.
    ///
    /// `state:interface` is on the instance, the instance is in the processor,
    /// the processor is out in a graph on another thread, and LV2 forbids
    /// calling `save` while `run` executes. So the rack asks for the processor
    /// back, the node parks it at the top of its next block, the state is read
    /// on this thread, and the processor goes back to be picked up. Ctrl+S on a
    /// playing project costs a few milliseconds of one plugin, against a
    /// sampler whose file was gone the next time the project opened.
    #[test]
    fn an_lv2_plugins_own_state_is_saved_while_the_graph_is_playing_it() {
        let (mut project, channel) = project_with_a_held_note();
        project.channels[channel].instrument = Some(InstrumentKind::Plugin);
        project.channels[channel].plugin = Some(PluginState::new(lv2(LV2_SINE), "Sine"));
        let master = project.mixer.master.unwrap();
        project.mixer.tracks[master]
            .inserts
            .push(EffectSlot::hosting(PluginState::new(lv2(LV2_GAIN), "Gain")));
        let slot = PluginSlot::Insert {
            track: master,
            slot: 0,
        };

        let mut rack = rack_with_both_formats();
        let library = SampleLibrary::new();
        let wiring = rack.realise(&project, SR as f64, fontelle_engine::BLOCK_SIZE as u32);
        let realised = realise_hosting(
            &project,
            &library,
            options(),
            &Default::default(),
            None,
            &Default::default(),
            None,
            None,
            &wiring,
        )
        .expect("this project must realise");
        let mut graph = realised.graph;
        graph.prepare(SR as f32, fontelle_engine::BLOCK_SIZE as u32);

        // The audio thread: one block every so often, for as long as it is told.
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let blocks = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let audio = {
            let (stop, blocks) = (std::sync::Arc::clone(&stop), std::sync::Arc::clone(&blocks));
            std::thread::spawn(move || {
                let mut at = 0i64;
                while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                    let block = fontelle_engine::BLOCK_SIZE as i64;
                    graph.process_block(
                        &[],
                        fontelle_engine::TransportSnapshot {
                            state: fontelle_engine::TransportState::Playing,
                            position_sample: at,
                            bpm: 120.0,
                        },
                        at..at + block,
                    );
                    at += block;
                    blocks.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
                graph
            })
        };
        // Long enough that the node has claimed the processor and run it.
        while blocks.load(std::sync::atomic::Ordering::Relaxed) < 20 {
            std::thread::sleep(std::time::Duration::from_millis(2));
        }

        let started = std::time::Instant::now();
        let state = rack.snapshot(slot).expect("a snapshot");
        let took = started.elapsed();
        let blob =
            fontelle_types::decode_base64(state.blob.as_deref().expect("the gain keeps state"))
                .unwrap();
        let decoded = fontelle_host::Lv2State::decode(&blob).unwrap();
        let runs = decoded
            .properties
            .iter()
            .find(|property| property.key == fontelle_testlv2::STATE_RUNS_KEY)
            .map(|property| i32::from_ne_bytes(property.value[..4].try_into().unwrap()))
            .expect("the run counter");
        assert!(
            runs >= 20,
            "the state was read off the running instance: {runs} runs"
        );
        assert!(
            took < std::time::Duration::from_millis(500),
            "bounded: the snapshot took {took:?}"
        );

        // And it carried on afterwards.
        let before = blocks.load(std::sync::atomic::Ordering::Relaxed);
        while blocks.load(std::sync::atomic::Ordering::Relaxed) < before + 20 {
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        let later = rack.snapshot(slot).expect("a second snapshot");
        let later_blob = fontelle_types::decode_base64(later.blob.as_deref().unwrap()).unwrap();
        let later_runs = fontelle_host::Lv2State::decode(&later_blob)
            .unwrap()
            .properties
            .into_iter()
            .find(|property| property.key == fontelle_testlv2::STATE_RUNS_KEY)
            .map(|property| i32::from_ne_bytes(property.value[..4].try_into().unwrap()))
            .unwrap();
        assert!(
            later_runs > runs,
            "the processor went back and kept running: {runs} then {later_runs}"
        );

        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        let graph = audio.join().unwrap();
        drop(graph);
    }

    /// What a plugin was given comes back out of the rack even on a machine
    /// where the audio never started — the processor sits in the bay, and a
    /// snapshot simply borrows it.
    #[test]
    fn an_lv2_plugins_own_state_is_saved_when_nothing_is_playing_it() {
        let (mut project, channel) = project_with_a_held_note();
        project.channels[channel].instrument = Some(InstrumentKind::Plugin);
        project.channels[channel].plugin = Some(PluginState::new(lv2(LV2_SINE), "Sine"));
        let master = project.mixer.master.unwrap();
        project.mixer.tracks[master]
            .inserts
            .push(EffectSlot::hosting(PluginState::new(lv2(LV2_GAIN), "Gain")));
        let slot = PluginSlot::Insert {
            track: master,
            slot: 0,
        };
        let mut rack = rack_with_both_formats();
        let _ = rack.realise(&project, SR as f64, fontelle_engine::BLOCK_SIZE as u32);
        let state = rack.snapshot(slot).expect("a snapshot");
        assert!(
            state.blob.is_some(),
            "the blob was read off the parked processor"
        );
    }

    // -------------------------------------------------------------- bridges ---
    //
    // > *"keeps it completely separate to the open source stuff and never gets
    // > included with it"*
    //
    // A bridge in Fontelle's own folder makes a format this build refuses into
    // one the rack hosts. `fontelle-testbridge` is one with no SDK in it; the
    // point here is that the rack finds it, and that what it offers rides the
    // same seam.

    fn bridge_folder() -> PathBuf {
        let mut path = std::env::current_exe().unwrap();
        path.pop();
        path.pop();
        let built = path.join("libfontelle_testbridge.so");
        assert!(built.exists(), "run `cargo build -p fontelle-testbridge`");
        static FOLDER: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
        FOLDER
            .get_or_init(|| {
                let folder = std::env::temp_dir().join("fontelle-app-bridge-tests");
                let _ = std::fs::create_dir_all(&folder);
                let staging = folder.join(format!("staging.{}.tmp", std::process::id()));
                if std::fs::copy(&built, &staging).is_ok() {
                    let _ = std::fs::rename(&staging, folder.join("libfontelle_testbridge.so"));
                }
                let _ = std::fs::remove_file(&staging);
                // And a bundle of the bridge's format beside it, in its own
                // plugin folder.
                let bundle = folder
                    .join("plugins")
                    .join(format!("Test.{}", fontelle_testbridge::EXTENSION));
                let _ = std::fs::create_dir_all(&bundle);
                std::fs::write(
                    bundle.join(fontelle_testbridge::MANIFEST),
                    format!(
                        "{}\n{}\n",
                        fontelle_testbridge::GAIN_ID,
                        fontelle_testbridge::SINE_ID
                    ),
                )
                .unwrap();
                folder
            })
            .clone()
    }

    fn bridged_rack() -> PluginRack {
        let mut rack = PluginRack::new();
        rack.search_standard_folders(false);
        rack.set_bridge_folders(vec![bridge_folder()]);
        rack.set_folders(vec![bridge_folder().join("plugins")]);
        rack.rescan();
        rack
    }

    #[test]
    fn a_rack_with_no_bridge_lists_no_bridged_plugins() {
        let mut rack = PluginRack::new();
        rack.search_standard_folders(false);
        rack.set_bridge_folders(Vec::new());
        rack.set_folders(vec![bridge_folder().join("plugins")]);
        rack.rescan();
        assert!(rack.scan().plugins.is_empty(), "{:#?}", rack.scan().plugins);
        assert!(rack.bridges().is_empty());
    }

    #[test]
    fn a_rack_with_a_bridge_hosts_the_bridged_format() {
        let rack = bridged_rack();
        assert_eq!(rack.bridges(), vec!["Fontelle Test Bridge".to_string()]);
        let keys: Vec<String> = rack
            .scan()
            .plugins
            .iter()
            .map(|p| p.key.to_string())
            .collect();
        assert!(
            keys.contains(&format!("vst2:{}", fontelle_testbridge::SINE_ID)),
            "{keys:?}"
        );
        assert_eq!(rack.scan().instruments().count(), 1);
        assert_eq!(rack.scan().effects().count(), 1);
    }

    #[test]
    fn a_channel_playing_a_bridged_plugin_is_heard_in_the_realised_graph() {
        let (mut project, channel) = project_with_a_held_note();
        project.channels[channel].instrument = Some(InstrumentKind::Plugin);
        project.channels[channel].plugin = Some(PluginState::new(
            PluginKey::new(
                fontelle_types::PluginFormat::Vst2,
                fontelle_testbridge::SINE_ID,
            ),
            "Sine",
        ));
        let mut rack = bridged_rack();
        let out = render(&project, &mut rack);
        assert!(peak(&out) > 0.05, "{}", peak(&out));
    }

    #[test]
    fn a_bridge_that_will_not_load_is_reported_rather_than_ignored() {
        // The bridge somebody just installed is the one they most need told
        // about — the same argument `PluginScan` makes for keeping failures.
        let dir =
            std::env::temp_dir().join(format!("fontelle-broken-bridge-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("libbroken.so"), "not a library").unwrap();
        let mut rack = PluginRack::new();
        rack.set_bridge_folders(vec![dir.clone()]);
        let message = rack.take_message().expect("the broken bridge is mentioned");
        assert!(message.contains("libbroken.so"), "{message}");
        assert!(rack.bridges().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

// -------------------------------- editors and the plugins they belong to ---

/// A plugin the document has stopped asking for is retired — **and its editor
/// goes with it**.
///
/// `PluginRack` owns the editor window beside the plugin for exactly this
/// reason: a window drawn by a plugin that has been freed is a window drawing
/// into nothing. The test fixture has no editor of its own to open, so what is
/// checked here is the half that holds whatever a real plugin would have: the
/// rack answers honestly about it, and a sweep leaves nothing behind.
#[test]
fn a_plugin_with_no_editor_says_so_and_sweeps_cleanly() {
    let (mut project, channel) = project_with_a_held_note();
    let mut rack = fresh_rack();
    project.channels[channel].plugin = Some(PluginState::new(PluginKey::clap(SINE), "Sine"));
    project.channels[channel].instrument = Some(InstrumentKind::Plugin);
    let _ = rack.realise(&project, SR as f64, fontelle_engine::BLOCK_SIZE as u32);

    let slot = PluginSlot::Channel(channel);
    assert!(!rack.has_editor(slot), "the fixture ships no GUI");
    assert!(!rack.editor_is_open(slot));
    assert!(!rack.open_editor(slot).expect("asking is not an error"));
    assert!(!rack.tick_editors(), "nothing open is nothing to drive");

    // And taking the plugin off the channel still retires it cleanly.
    project.channels[channel].plugin = None;
    project.channels[channel].instrument = None;
    let _ = rack.realise(&project, SR as f64, fontelle_engine::BLOCK_SIZE as u32);
    assert!(!rack.editor_is_open(slot));
}

/// Asking about a slot that holds no plugin is answered, not refused.
#[test]
fn an_empty_slot_has_no_editor_and_says_so() {
    let mut rack = fresh_rack();
    let slot = PluginSlot::Channel(fontelle_model::Project::new("empty").channels.insert(
        fontelle_model::Channel {
            preset: None,
            name: "none".into(),
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
        },
    ));
    assert!(!rack.has_editor(slot));
    assert!(!rack.editor_is_open(slot));
    assert!(!rack.open_editor(slot).expect("asking is not an error"));
}

// ------------------------------------------------ the offline bounce (2026-09-06)

/// A quarter of a second of `project`, the way `--render-wav` renders it: the
/// rack opens what the document names, the graph is built around it, and
/// the plugins are handed back when the render is done.
fn bounce(project: &Project, rack: &mut PluginRack) -> fontelle_app::Bounced {
    let library = SampleLibrary::new();
    let transport = fontelle_engine::Transport::new();
    fontelle_app::bounce(
        project,
        &library,
        rack,
        &transport,
        fontelle_app::BounceOptions {
            quality: Interpolation::Draft,
            total_samples: SR as i64 / 4,
        },
    )
    .expect("this project must bounce")
}

/// `--render-wav` used to build its graph with no rack at all, so a channel
/// playing a plugin bounced as silence — a mistake nobody noticed until they
/// listened to the file. The bounce goes through the same rack the studio
/// plays through.
#[test]
fn an_offline_bounce_hosts_the_plugins_the_project_names() {
    let (mut project, channel) = project_with_a_held_note();
    project.channels[channel].instrument = Some(InstrumentKind::Plugin);
    project.channels[channel].plugin = Some(PluginState::new(PluginKey::clap(SINE), "Sine"));

    let mut rack = fresh_rack();
    let bounced = bounce(&project, &mut rack);
    assert_eq!(
        bounced.pcm.len(),
        (SR as usize / 4) * 2,
        "stereo, interleaved"
    );
    assert!(peak(&bounced.pcm) > 0.05, "{}", peak(&bounced.pcm));
    assert!(bounced.message.is_none(), "{:?}", bounced.message);
}

/// §17.4's rule for a missing file, applied to a missing plugin: the bounce
/// still renders — the channel silent — and says which plugin it wanted.
#[test]
fn a_bounce_of_a_plugin_that_is_not_installed_is_silent_and_says_which() {
    let (mut project, channel) = project_with_a_held_note();
    project.channels[channel].instrument = Some(InstrumentKind::Plugin);
    project.channels[channel].plugin = Some(PluginState::new(PluginKey::clap(SINE), "Sine"));

    let empty = std::env::temp_dir().join(format!("fontelle-no-plugins-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&empty);
    let mut rack = PluginRack::new();
    rack.search_standard_folders(false);
    rack.set_folders(vec![empty.clone()]);
    rack.rescan();

    let bounced = bounce(&project, &mut rack);
    assert_eq!(bounced.pcm.len(), (SR as usize / 4) * 2);
    assert!(peak(&bounced.pcm) < 1e-6, "{}", peak(&bounced.pcm));
    let message = bounced.message.expect("the missing plugin is named");
    assert!(
        message.contains("Sine") && message.contains("not installed"),
        "{message}"
    );
    let _ = std::fs::remove_dir_all(&empty);
}

/// When the render is over the graph is gone, so every processor is back in
/// its bay and the rack can close every plugin cleanly — nothing is out in a
/// graph, nothing leaks. `close_all` is what a headless run does on its way
/// out.
#[test]
fn a_bounce_hands_every_plugin_back_when_it_is_done() {
    let (mut project, channel) = project_with_a_held_note();
    project.channels[channel].instrument = Some(InstrumentKind::Plugin);
    project.channels[channel].plugin = Some(PluginState::new(PluginKey::clap(SINE), "Sine"));

    let mut rack = fresh_rack();
    let _ = bounce(&project, &mut rack);
    assert_eq!(
        rack.counts(),
        (1, 0),
        "open, and nothing waiting to be freed"
    );
    rack.close_all();
    assert_eq!(
        rack.counts(),
        (0, 0),
        "every plugin retired: its processor had come home"
    );
}

// ------------------------------------------------ latency (2026-09-06)

/// A plugin that declares latency has it **compensated**, like a built-in
/// insert that looks ahead (TDD §5.5): the number reaches the graph builder,
/// which holds every other track back to meet it and says what the whole
/// configuration costs.
///
/// The fixture gain declares 137 samples — see `GAIN_LATENCY_SAMPLES`. It
/// does not actually delay anything, which is why this measures what the
/// graph was *told* rather than where a click lands; the audible half of
/// compensation is `fontelle-app/tests/latency_compensation.rs`, over a
/// built-in insert whose latency is real.
#[test]
fn a_plugins_declared_latency_reaches_the_graph() {
    let (mut project, _) = project_with_a_held_note();
    let master = project.mixer.master.expect("a master");
    let mut rack = fresh_rack();

    let library = SampleLibrary::new();
    let plain = {
        let wiring = rack.realise(&project, SR as f64, fontelle_engine::BLOCK_SIZE as u32);
        realise_hosting(
            &project,
            &library,
            options(),
            &Default::default(),
            None,
            &Default::default(),
            None,
            None,
            &wiring,
        )
        .expect("a graph")
        .latency_samples
    };

    project.mixer.tracks[master]
        .inserts
        .push(EffectSlot::hosting(PluginState::new(
            PluginKey::clap(GAIN),
            "Gain",
        )));
    let mut rack = fresh_rack();
    let wiring = rack.realise(&project, SR as f64, fontelle_engine::BLOCK_SIZE as u32);
    let hosted = realise_hosting(
        &project,
        &library,
        options(),
        &Default::default(),
        None,
        &Default::default(),
        None,
        None,
        &wiring,
    )
    .expect("a graph");

    assert_eq!(
        hosted.latency_samples,
        plain + fontelle_testplug::GAIN_LATENCY_SAMPLES,
        "the plugin's own number, in what the graph costs"
    );
}

// ------------------------------------------------------------------ VST 3 ---
//
// > *"maximum compatibility is what's most important to me"*
//
// The same seam, the third format (`docs/vst-plan.md` §2). A `.vst3` bundle
// in a plugin folder is found, chosen in the document by its class id,
// opened, realised and heard — through exactly the code above.
mod vst3 {
    use super::*;

    const VST3_GAIN: &str = fontelle_testvst3::GAIN_ID;
    const VST3_SINE: &str = fontelle_testvst3::SINE_ID;

    fn vst3_library() -> &'static str {
        if cfg!(target_os = "windows") {
            "fontelle_testvst3.dll"
        } else if cfg!(target_os = "macos") {
            "libfontelle_testvst3.dylib"
        } else {
            "libfontelle_testvst3.so"
        }
    }

    /// A folder holding nothing but the VST 3 test bundle, laid out the way
    /// the SDK lays a bundle out: `Name.vst3/Contents/<arch>/Name.so`.
    fn vst3_folder() -> PathBuf {
        let mut path = std::env::current_exe().unwrap();
        path.pop();
        path.pop();
        let built = path.join(vst3_library());
        assert!(
            built.exists(),
            "{} is missing — run `cargo build -p fontelle-testvst3`",
            built.display()
        );
        let (arch, file) = if cfg!(target_os = "windows") {
            ("Contents/x86_64-win", "fontelle-testvst3.vst3")
        } else if cfg!(target_os = "macos") {
            ("Contents/MacOS", "fontelle-testvst3")
        } else {
            ("Contents/x86_64-linux", "fontelle-testvst3.so")
        };
        static FOLDER: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
        FOLDER
            .get_or_init(|| {
                let folder = std::env::temp_dir().join("fontelle-app-vst3-tests");
                let _ = std::fs::create_dir_all(&folder);
                let staging = folder.join(format!("vst3-staging.{}", std::process::id()));
                let _ = std::fs::remove_dir_all(&staging);
                std::fs::create_dir_all(staging.join(arch)).unwrap();
                std::fs::copy(&built, staging.join(arch).join(file)).unwrap();
                let bundle = folder.join("fontelle-testvst3.vst3");
                let _ = std::fs::remove_dir_all(&bundle);
                std::fs::rename(&staging, &bundle).unwrap();
                folder
            })
            .clone()
    }

    fn rack_with_vst3() -> PluginRack {
        let mut rack = PluginRack::new();
        rack.search_standard_folders(false);
        rack.set_folders(vec![plugin_folder(), vst3_folder()]);
        rack.rescan();
        rack
    }

    fn vst3(id: &str) -> PluginKey {
        PluginKey::new(fontelle_types::PluginFormat::Vst3, id)
    }

    #[test]
    fn the_rack_lists_vst3_plugins_beside_clap_ones() {
        let rack = rack_with_vst3();
        let keys: Vec<String> = rack
            .scan()
            .plugins
            .iter()
            .map(|p| p.key.to_string())
            .collect();
        assert!(keys.contains(&format!("vst3:{VST3_GAIN}")), "{keys:?}");
        assert!(keys.contains(&format!("vst3:{VST3_SINE}")), "{keys:?}");
        assert!(keys.contains(&format!("clap:{SINE}")), "{keys:?}");
        assert!(
            rack.scan().failures.is_empty(),
            "{:#?}",
            rack.scan().failures
        );
    }

    #[test]
    fn a_channel_playing_a_vst3_plugin_is_heard_in_the_realised_graph() {
        let (mut project, channel) = project_with_a_held_note();
        project.channels[channel].instrument = Some(InstrumentKind::Plugin);
        project.channels[channel].plugin = Some(PluginState::new(vst3(VST3_SINE), "Sine"));
        let mut rack = rack_with_vst3();
        let out = render(&project, &mut rack);
        assert!(peak(&out) > 0.05, "{}", peak(&out));
    }

    #[test]
    fn an_insert_holding_a_vst3_plugin_processes_the_bus() {
        let (mut project, channel) = project_with_a_held_note();
        project.channels[channel].instrument = Some(InstrumentKind::Plugin);
        project.channels[channel].plugin = Some(PluginState::new(vst3(VST3_SINE), "Sine"));
        let master = project.mixer.master.unwrap();
        let mut rack = rack_with_vst3();
        let dry = peak(&render(&project, &mut rack));

        // A normalised gain of 1/16 is a quarter on the fixture.
        let mut gain = PluginState::new(vst3(VST3_GAIN), "Gain");
        gain.set_param(0, 0.0625);
        project.mixer.tracks[master]
            .inserts
            .push(EffectSlot::hosting(gain));
        let mut rack = rack_with_vst3();
        let quartered = peak(&render(&project, &mut rack));
        assert!(
            quartered < dry * 0.5,
            "dry {dry}, through the plugin {quartered}"
        );
    }

    #[test]
    fn what_a_vst3_plugin_was_set_to_comes_back_after_a_reopen() {
        let (mut project, channel) = project_with_a_held_note();
        project.channels[channel].instrument = Some(InstrumentKind::Plugin);
        let mut state = PluginState::new(vst3(VST3_SINE), "Sine");
        state.set_param(7, 0.1);
        project.channels[channel].plugin = Some(state);
        let mut rack = rack_with_vst3();
        let quiet = peak(&render(&project, &mut rack));
        assert!(quiet > 0.03 && quiet < 0.12, "{quiet}");
        let saved = rack
            .snapshot(PluginSlot::Channel(channel))
            .expect("a snapshot");
        assert_eq!(saved.param(7), Some(0.1));
        assert!(
            saved.blob.is_some(),
            "a VST 3 component always has a stream"
        );
    }
}
