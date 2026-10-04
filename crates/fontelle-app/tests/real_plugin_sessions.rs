//! The installed plugins themselves, through a whole session: chosen, set,
//! worked beside, saved and reopened.
//!
//! > *"sometimes they'll just revert back to the init preset when working on
//! > a saved project witch I find causes confusion leading me to have to re
//! > interrelate each track to its presset."*
//!
//! `fontelle-host/tests/real_plugins.rs` proves a plugin survives the host.
//! This is the layer above, where the report was: what the **document** and
//! the rack do to a real plugin's patch between a save and the next one.
//!
//! Ignored, because what they find depends on what is installed:
//!
//! `cargo test -p fontelle-app --test real_plugin_sessions -- --ignored --nocapture`
//!
//! `FONTELLE_REAL_ONLY=Surge` narrows it to names containing that; the
//! default is `Surge XT`, which the reporter uses and CI does not have.

mod common;

use std::path::{Path, PathBuf};

use fontelle_app::settings::Settings;
use fontelle_app::{PluginSlot, Session};
use fontelle_types::PluginState;
use fontelle_ui::canvas::PresetDevice;
use fontelle_ui::document::{DocumentHost, StudioHost};

use common::SR;

const CHANNEL: PresetDevice = PresetDevice::Channel { index: 0 };

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "fontelle-real-sessions-{name}-{}",
        std::process::id()
    ));
    std::fs::remove_dir_all(&path).ok();
    std::fs::create_dir_all(&path).expect("the scratch folder must be creatable");
    path
}

/// A rack that scans the way the studio does: in child processes of the
/// studio's own binary, so the test process loads only the plugins it plays.
fn studio_rack() -> fontelle_app::PluginRack {
    let mut rack = fontelle_app::PluginRack::new();
    rack.set_prober(Some(studio_prober()));
    rack
}

/// One for the whole test binary, so what one walk read the next does not
/// read again.
fn studio_prober() -> std::sync::Arc<fontelle_host::BundleProber> {
    static PROBER: std::sync::OnceLock<std::sync::Arc<fontelle_host::BundleProber>> =
        std::sync::OnceLock::new();
    std::sync::Arc::clone(PROBER.get_or_init(|| {
        std::sync::Arc::new(fontelle_host::BundleProber::new(PathBuf::from(env!(
            "CARGO_BIN_EXE_fontelle"
        ))))
    }))
}

fn wanted() -> String {
    std::env::var("FONTELLE_REAL_ONLY").unwrap_or_else(|_| "Surge XT".to_string())
}

/// A session that sees what is installed, with a projects folder of its own.
fn a_session(dir: &Path) -> Session {
    let settings = Settings {
        preset_dir: Some(dir.join("presets")),
        projects_dir: Some(dir.join("projects")),
        ..Default::default()
    };
    std::fs::write(dir.join("settings.json"), settings.to_json()).unwrap();
    let mut session = common::a_session_for(common::a_project_with_a_clip(2, 120.0, SR))
        .with_settings_path(dir.join("settings.json"))
        .with_headless_plugin_editors()
        .with_plugin_prober(studio_prober());
    session.set_projects_dir(Some(dir.join("projects")));
    session.scan_plugins_once();
    session
}

/// Every installed instrument whose name matches, by its place in the list
/// and the format it speaks — Surge XT is there once as CLAP and once as
/// VST 3, and the report did not say which.
fn instruments(session: &Session) -> Vec<(usize, String)> {
    let wanted = wanted();
    session
        .plugin_instruments()
        .iter()
        .enumerate()
        .filter(|(_, listing)| listing.name.contains(&wanted))
        .map(|(at, listing)| (at, format!("{listing:?}")))
        .collect()
}

fn slot(session: &Session) -> PluginSlot {
    fontelle_app::plugin_slots(session.project())[0]
}

fn live(session: &mut Session) -> PluginState {
    // What the window does every frame: frees the graphs the audio thread
    // handed back, so a processor in one is free for the graph after it.
    // Without it a plugin can sit in a retired graph, unprocessed, and a
    // state it queued for its next block never lands.
    session.pump();
    let slot = slot(session);
    session
        .plugin_rack_mut()
        .snapshot(slot)
        .expect("the plugin is open")
}

fn document(session: &Session) -> PluginState {
    let PluginSlot::Channel(channel) = slot(session) else {
        panic!("the plugin is a channel's");
    };
    session.project().channels[channel]
        .plugin
        .clone()
        .expect("the channel holds the plugin")
}

/// A parameter two states disagree on: its id, and each one's value.
type Differing = (u32, Option<f64>, Option<f64>);

/// How many parameters two states disagree on, and the first few.
fn differences(a: &PluginState, b: &PluginState) -> (usize, Vec<Differing>) {
    let mut found = Vec::new();
    for param in &a.params {
        let other = b.param(param.id);
        if other.is_none_or(|v| (v - param.value).abs() > 1e-6) {
            found.push((param.id, Some(param.value), other));
        }
    }
    (found.len(), found.into_iter().take(5).collect())
}

fn assert_same(what: &str, a: &PluginState, b: &PluginState) {
    let (count, first) = differences(a, b);
    if count > 0 {
        eprintln!("   {what}: differs in {first:?}");
    }
    assert_eq!(
        count, 0,
        "{what}: {count} parameters differ, e.g. {first:?}"
    );
}

/// Two of the plugin's own presets that differ from each other and from the
/// patch it opens on, as places in the preset list.
fn two_presets(session: &mut Session) -> Option<(usize, usize)> {
    session.settle_plugin_presets();
    let own: Vec<usize> = session
        .preset_choices(CHANNEL)
        .iter()
        .enumerate()
        .filter(|(_, choice)| choice.origin == fontelle_types::PresetOrigin::Plugin)
        .map(|(at, _)| at)
        .collect();
    // Well apart in the list, so they are not two takes on one patch.
    (own.len() >= 8).then(|| (own[own.len() / 3], own[2 * own.len() / 3]))
}

// ------------------------------------------------- with the audio running
//
// Everything above has no audio thread, and a plugin with nothing calling
// its `process` does what it is asked at once. In the studio the callback
// is running, and Surge XT queues a patch for its next block — so what is
// read straight after a preset load is the patch **before** it.

/// A session whose graph something is processing, the way the device's
/// callback does: a thread taking each published graph and running blocks.
fn a_running_session(dir: &Path) -> (Session, std::sync::Arc<std::sync::atomic::AtomicBool>) {
    use fontelle_app::{RealiseOptions, SampleLibrary};
    use fontelle_engine::{BLOCK_SIZE, TransportSnapshot, TransportState};
    use fontelle_types::CompiledTimeline;

    let settings = Settings {
        preset_dir: Some(dir.join("presets")),
        projects_dir: Some(dir.join("projects")),
        ..Default::default()
    };
    std::fs::write(dir.join("settings.json"), settings.to_json()).unwrap();
    let project = common::a_project_with_a_clip(2, 120.0, SR);
    let clip = Session::first_clip(&project).unwrap_or_default();
    let channel_nodes = fontelle_app::channel_nodes(&project);
    let (publisher, _timeline) = fontelle_engine::timeline_channel(CompiledTimeline::empty());
    let library = SampleLibrary::new();
    let options = RealiseOptions {
        sample_rate: SR,
        block_size: BLOCK_SIZE,
        quality: fontelle_app::PLAYBACK_QUALITY,
    };
    let realised = fontelle_app::realise(&project, &library, options).expect("realises");
    let (graphs, mut source) = fontelle_engine::graph_channel(realised.graph);
    let mut session = Session::new(
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
    .with_headless_plugin_editors()
    .with_plugin_prober(studio_prober());
    session.set_projects_dir(Some(dir.join("projects")));
    session.scan_plugins_once();

    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stopped = std::sync::Arc::clone(&stop);
    std::thread::spawn(move || {
        let empty = CompiledTimeline::empty();
        let mut at = 0i64;
        while !stopped.load(std::sync::atomic::Ordering::Relaxed) {
            source.take_update();
            let transport = TransportSnapshot {
                state: if std::env::var_os("FONTELLE_REAL_PLAYING").is_some() {
                    TransportState::Playing
                } else {
                    TransportState::Stopped
                },
                position_sample: at,
                bpm: 120.0,
                ..Default::default()
            };
            let mut cursor = 0usize;
            let range = at..at + BLOCK_SIZE as i64;
            let events = empty.events_for_block(&mut cursor, range.clone());
            source.current().process_block(events, transport, range);
            at += BLOCK_SIZE as i64;
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    });
    (session, stop)
}

fn settle() {
    std::thread::sleep(std::time::Duration::from_millis(300));
}

#[test]
#[ignore]
fn a_preset_loaded_while_the_audio_runs_is_what_the_document_holds() {
    let dir = scratch("running");
    let found = instruments(&a_session(&dir));
    assert!(
        !found.is_empty(),
        "nothing installed matches {:?}",
        wanted()
    );
    for (which, listing) in found {
        eprintln!("== {listing}");
        let dir = scratch("running");
        let (mut session, stop) = a_running_session(&dir);
        session.set_channel_plugin(0, which);
        settle();
        let init = live(&mut session);
        let Some((first, _)) = two_presets(&mut session) else {
            eprintln!("   lists no library of its own here; skipped");
            stop.store(true, std::sync::atomic::Ordering::Relaxed);
            continue;
        };
        let name = session.preset_choices(CHANNEL)[first].name.clone();
        session.apply_preset(CHANNEL, first);
        eprintln!("   {name}: said {:?}", session.take_message());
        let held = document(&session);
        eprintln!(
            "   at once: document vs init {} ; live vs init {}",
            differences(&held, &init).0,
            differences(&live(&mut session), &init).0
        );
        settle();
        let playing = live(&mut session);
        eprintln!(
            "   settled: live vs init {}",
            differences(&playing, &init).0
        );
        assert!(
            differences(&playing, &init).0 > 0,
            "the preset never reached the plugin"
        );
        assert_same("the document, against what is playing", &held, &playing);

        // And through a save and a reopen, with something done in between.
        session.add_channel().expect("adds");
        session.undo();
        settle();
        assert_same("after a rebuild", &live(&mut session), &playing);
        session.save_as("Running").expect("saves");
        let bundle = session.bundle_path().expect("saved").to_path_buf();
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        settle();
        drop(session);
        let (mut again, stop) = a_running_session(&dir);
        again.open_project_path(&bundle).expect("opens");
        settle();
        assert_same("after a reopen", &live(&mut again), &playing);
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        settle();
    }
}

/// The whole of the report, as one walk per installed format of the plugin.
#[test]
#[ignore]
fn a_real_instruments_patch_survives_working_saving_and_reopening() {
    let dir = scratch("patch");
    let found = instruments(&a_session(&dir));
    assert!(
        !found.is_empty(),
        "nothing installed matches {:?}",
        wanted()
    );
    for (which, listing) in found {
        eprintln!("== {listing}");
        let dir = scratch("patch");
        let (mut session, stop) = a_running_session(&dir);
        session.set_channel_plugin(0, which);
        settle();
        let init = live(&mut session);
        let Some((first, second)) = two_presets(&mut session) else {
            eprintln!("   lists no library of its own here; skipped");
            stop.store(true, std::sync::atomic::Ordering::Relaxed);
            continue;
        };

        // What the second preset is, read off the plugin and put back.
        session.apply_preset(CHANNEL, second);
        let second_state = live(&mut session);
        assert!(
            differences(&second_state, &init).0 > 0,
            "the second preset is the init patch"
        );
        session.undo();

        // A preset from Fontelle's strip, and a save.
        session.apply_preset(CHANNEL, first);
        let first_state = live(&mut session);
        assert!(differences(&first_state, &init).0 > 0);
        assert!(differences(&first_state, &second_state).0 > 0);
        assert_same(
            "the document after a preset",
            &document(&session),
            &first_state,
        );
        session.save_as("Patch").expect("saves");

        // Something else is done, and undone: two rebuilds.
        session.add_channel().expect("adds");
        session.undo();
        assert_same("after a rebuild", &live(&mut session), &first_state);

        // A patch picked in the plugin's **own** browser: the document hears
        // nothing of it. Acted out by loading a state from outside, which
        // for a CLAP plugin is the road its own browser takes (Surge's patch
        // loader, then parameter events). A VST 3's own browser tells the
        // host through `restartComponent` instead, which a load from outside
        // does not exercise, so that half is the knob walk's.
        if !listing.contains("format: Clap") {
            stop.store(true, std::sync::atomic::Ordering::Relaxed);
            settle();
            continue;
        }
        let bytes = fontelle_types::decode_base64(second_state.blob.as_ref().unwrap()).unwrap();
        let at = slot(&session);
        assert!(
            session
                .plugin_rack_mut()
                .plugin_mut(at)
                .unwrap()
                .load_state(&bytes)
        );
        // Its editor is open while that happens, so its audio is running
        // and the patch lands a block or so later. What the plugin makes of
        // it is the plugin's: Surge sets a few parameters that only one
        // oscillator type uses ("Osc 2 Unison Voices") by the patch it is
        // leaving as well as the one it loads. What is Fontelle's is that
        // nothing it does afterwards moves the plugin off it.
        let waited = std::time::Instant::now();
        let mut chosen = live(&mut session);
        while differences(&chosen, &second_state).0 * 50 >= chosen.params.len().max(50)
            && waited.elapsed() < std::time::Duration::from_secs(3)
        {
            std::thread::sleep(std::time::Duration::from_millis(20));
            chosen = live(&mut session);
        }
        let (moved, _) = differences(&chosen, &second_state);
        eprintln!(
            "   its own browser: {moved} differ after {:?}",
            waited.elapsed()
        );
        assert!(
            moved * 50 < chosen.params.len().max(50),
            "its own browser: the patch did not go in ({moved} differ)"
        );
        assert!(differences(&chosen, &first_state).0 > 0);
        let second_state = chosen;
        session.add_channel().expect("adds");
        assert_same(
            "its own browser, then a rebuild",
            &live(&mut session),
            &second_state,
        );
        session.undo();
        assert_same(
            "its own browser, then an undo of something else",
            &live(&mut session),
            &second_state,
        );

        // Saved, closed, opened again.
        session.save().expect("saves");
        let bundle = session.bundle_path().expect("it was saved").to_path_buf();
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        settle();
        drop(session);
        let (mut again, stop) = a_running_session(&dir);
        again.open_project_path(&bundle).expect("opens");
        settle();
        assert_same("after a reopen", &live(&mut again), &second_state);
        // And working on the reopened project leaves it.
        again.add_channel().expect("adds");
        again.undo();
        assert_same(
            "after a reopen and a rebuild",
            &live(&mut again),
            &second_state,
        );
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        settle();
    }
}

/// The same walk for a plugin that lists **no library** here — Surge XT as
/// VST 3, anything through a bridge — where a patch is whatever was done in
/// the plugin's own window. Its knobs are turned behind the document's back,
/// which is all its window does as far as the host can tell.
#[test]
#[ignore]
fn what_was_done_in_a_real_plugins_window_survives_working_saving_and_reopening() {
    let dir = scratch("window");
    let found = instruments(&a_session(&dir));
    assert!(
        !found.is_empty(),
        "nothing installed matches {:?}",
        wanted()
    );
    for (which, listing) in found {
        eprintln!("== {listing}");
        let dir = scratch("window");
        let (mut session, stop) = a_running_session(&dir);
        session.set_channel_plugin(0, which);
        settle();
        let init = live(&mut session);
        session.save_as("Window").expect("saves");

        // A dozen knobs, each moved a third of its range from where it was.
        let at = slot(&session);
        let turned: Vec<(u32, f64)> = session
            .plugin_rack_mut()
            .params(at)
            .iter()
            .filter(|param| !param.hidden && !param.readonly && !param.stepped)
            .filter(|param| param.max > param.min)
            .take(12)
            .map(|param| {
                let was = init.param(param.id).unwrap_or(param.default);
                let span = param.max - param.min;
                let to = param.min + (was - param.min + span / 3.0).rem_euclid(span);
                (param.id, to)
            })
            .collect();
        if turned.is_empty() {
            // A drum kit of fixed samples, a MIDI tool: nothing to turn.
            eprintln!("   has no knobs to turn; skipped");
            stop.store(true, std::sync::atomic::Ordering::Relaxed);
            settle();
            continue;
        }
        for (id, to) in &turned {
            session
                .plugin_rack_mut()
                .plugin_mut(at)
                .unwrap()
                .set_param(*id, *to);
        }
        settle();
        let set = live(&mut session);
        let moved = differences(&set, &init).0;
        eprintln!("   {} knobs turned, {moved} values moved", turned.len());
        assert!(moved > 0, "turning its knobs changed nothing");

        session.add_channel().expect("adds");
        settle();
        assert_same("then a rebuild", &live(&mut session), &set);
        session.undo();
        settle();
        assert_same("then an undo of something else", &live(&mut session), &set);

        session.save().expect("saves");
        let bundle = session.bundle_path().expect("it was saved").to_path_buf();
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        settle();
        drop(session);
        let (mut again, stop) = a_running_session(&dir);
        again.open_project_path(&bundle).expect("opens");
        settle();
        assert_same("after a reopen", &live(&mut again), &set);
        again.add_channel().expect("adds");
        again.undo();
        settle();
        assert_same("after a reopen and a rebuild", &live(&mut again), &set);
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        settle();
    }
}

// ------------------------------------------------------------- a render
//
// > *"I find I have trouble rendering midi to audio. most times it just
// > renders with nothing"*

fn peak_of(path: &Path) -> f32 {
    let asset = fontelle_assets::import_audio(path).expect("the render reads back");
    asset.samples.iter().fold(0.0f32, |m, s| m.max(s.abs()))
}

fn renders(session: &Session) -> Vec<PathBuf> {
    let bundle = session.bundle_path().expect("it was saved");
    let mut found: Vec<PathBuf> = std::fs::read_dir(bundle.join("renders"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .collect();
    found.sort();
    found
}

#[test]
#[ignore]
fn a_real_instrument_is_in_an_export_and_in_a_row_rendered_to_audio() {
    let dir = scratch("render");
    let found = instruments(&a_session(&dir));
    assert!(
        !found.is_empty(),
        "nothing installed matches {:?}",
        wanted()
    );
    for (which, listing) in found {
        eprintln!("== {listing}");
        let dir = scratch("render");
        let (mut session, stop) = a_running_session(&dir);
        session.set_channel_plugin(0, which);
        settle();
        // What it sounds like as it opens, in a rack of its own: a plugin
        // that is silent then (a sampler with nothing loaded) cannot show a
        // silent render.
        let mut alone = session.project().clone();
        let channel = alone.channels.keys().next().unwrap();
        let clip = alone.clips.keys().next().unwrap();
        if let fontelle_model::ClipSource::Notes(data) = &mut alone.clips[clip].source {
            data.notes.insert(fontelle_model::Note {
                start: 0,
                length: fontelle_types::PPQN * 2,
                key: 60,
                velocity: 100,
                pan: 0,
                fine_pitch: 0,
                release: 0,
                mod_x: 0,
                mod_y: 0,
                slide: false,
                path: Vec::new(),
                channel: None,
            });
        }
        let _ = channel;
        let mut rack = studio_rack();
        let reference = offline_peak(&alone, &mut rack);
        rack.close_all();
        if reference <= 0.01 {
            eprintln!("   silent as it opens; skipped");
            stop.store(true, std::sync::atomic::Ordering::Relaxed);
            settle();
            continue;
        }
        session.edit(fontelle_ui::canvas::RollEdit::Add {
            note: fontelle_model::Note {
                start: 0,
                length: fontelle_types::PPQN * 4,
                key: 60,
                velocity: 100,
                pan: 0,
                fine_pitch: 0,
                release: 0,
                mod_x: 0,
                mod_y: 0,
                slide: false,
                path: Vec::new(),
                channel: None,
            },
        });
        session.save_as("Render").expect("saves");

        session.export_wav().expect("exports");
        let exported = renders(&session);
        let peak = peak_of(exported.last().unwrap());
        eprintln!("   export peaks at {peak}");
        assert!(peak > 0.01, "the export is silent: {peak}");

        session.render_lane(0, None).expect("renders the row");
        let row = renders(&session)
            .into_iter()
            .find(|path| !exported.contains(path))
            .expect("a second file was written");
        let peak = peak_of(&row);
        eprintln!("   the row peaks at {peak}");
        assert!(peak > 0.01, "the row rendered silent: {peak}");

        // And the studio's own instance is still there, and still the one
        // being played.
        settle();
        let _ = live(&mut session);
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        settle();
    }
}

// ------------------------------------------- the same sound from its state
//
// The walks above compare parameter lists. This one listens: a plugin opened
// fresh, and a second opened from the state the first one saved, play the
// same note and must be as loud as each other.

fn offline_peak(project: &fontelle_model::Project, rack: &mut fontelle_app::PluginRack) -> f32 {
    let library = fontelle_app::SampleLibrary::new();
    let wiring = rack.realise(project, SR as f64, fontelle_engine::BLOCK_SIZE as u32);
    let mut realised = fontelle_app::realise_hosting(
        project,
        &library,
        fontelle_app::RealiseOptions {
            sample_rate: SR,
            block_size: fontelle_engine::BLOCK_SIZE,
            quality: fontelle_app::RENDER_QUALITY,
        },
        &Default::default(),
        None,
        &Default::default(),
        None,
        None,
        &wiring,
    )
    .expect("realises");
    let timeline =
        fontelle_sequencer::compile(project, &realised.channel_nodes, &Default::default());
    let pcm = fontelle_app::render_offline(&timeline, &mut realised.graph, SR as i64 * 2);
    pcm.iter().fold(0.0f32, |m, s| m.max(s.abs()))
}

#[test]
#[ignore]
fn a_real_instrument_opened_from_its_saved_state_sounds_the_same() {
    use fontelle_model::{ClipSource, Note};
    let dir = scratch("same");
    let session = a_session(&dir);
    let wanted = wanted();
    let keys: Vec<fontelle_types::PluginKey> = session
        .plugin_instruments()
        .iter()
        .filter(|listing| listing.name.contains(&wanted))
        .map(|listing| listing.key.clone())
        .collect();
    drop(session);
    assert!(!keys.is_empty());
    for key in keys {
        eprintln!("== {key:?}");
        let mut project = common::a_project_with_a_clip(2, 120.0, SR);
        let channel = project.channels.keys().next().unwrap();
        project.channels[channel].instrument = Some(fontelle_types::InstrumentKind::Plugin);
        project.channels[channel].plugin =
            Some(PluginState::new(key.clone(), "Under test".to_string()));
        let clip = project.clips.keys().next().unwrap();
        let ClipSource::Notes(data) = &mut project.clips[clip].source else {
            unreachable!()
        };
        data.notes.insert(Note {
            start: 0,
            length: fontelle_types::PPQN * 2,
            key: 60,
            velocity: 100,
            pan: 0,
            fine_pitch: 0,
            release: 0,
            mod_x: 0,
            mod_y: 0,
            slide: false,
            path: Vec::new(),
            channel: None,
        });

        let mut rack = studio_rack();
        let fresh = offline_peak(&project, &mut rack);
        let slot = PluginSlot::Channel(channel);
        let state = rack.snapshot(slot).expect("it is open");
        eprintln!(
            "   fresh {fresh}; state: {} params, blob {} bytes",
            state.params.len(),
            state.blob.as_ref().map_or(0, String::len)
        );
        rack.close_all();

        project.channels[channel].plugin = Some(state.clone());
        let mut rack = studio_rack();
        let reopened = offline_peak(&project, &mut rack);
        eprintln!("   reopened from its state {reopened}");

        // And which half of the state does it: the blob alone, and the
        // parameters alone.
        let mut only_blob = state.clone();
        only_blob.params.clear();
        project.channels[channel].plugin = Some(only_blob);
        let mut rack = studio_rack();
        eprintln!(
            "   from the blob alone {}",
            offline_peak(&project, &mut rack)
        );
        // What the plugin says its parameters are, opened from the blob,
        // against the list that was saved beside that blob.
        let says = rack.snapshot(slot).expect("it is open");
        let names: std::collections::HashMap<u32, String> = rack
            .params(slot)
            .iter()
            .map(|param| {
                (
                    param.id,
                    format!("{} [{}..{}]", param.name, param.min, param.max),
                )
            })
            .collect();
        let (count, _) = differences(&state, &says);
        eprintln!("   the saved list differs from the blob in {count} parameters");
        for param in &state.params {
            let other = says.param(param.id);
            if other.is_none_or(|v| (v - param.value).abs() > 1e-6) {
                eprintln!(
                    "     {:<50} saved {:<12} blob says {:?}",
                    names.get(&param.id).map_or("?", String::as_str),
                    param.value,
                    other
                );
            }
        }
        rack.close_all();

        if fresh <= 0.01 {
            // A sampler with nothing loaded, a drum machine with no kit: a
            // plugin that is silent as it opens has nothing to compare.
            eprintln!("   silent as it opens; skipped");
            continue;
        }
        assert!(
            (reopened / fresh) > 0.5 && (reopened / fresh) < 2.0,
            "fresh {fresh}, reopened {reopened}"
        );
    }
}

/// Every installed instrument, one per line as `FORMAT<tab>name` — what
/// `tools/real-plugin-sweep.sh` reads to run the walks on each in a
/// process of its own, so one plugin that crashes takes only itself down.
#[test]
#[ignore]
fn list_installed_instruments() {
    let dir = scratch("list");
    let session = a_session(&dir);
    for listing in session.plugin_instruments() {
        println!("SWEEP\t{:?}\t{}", listing.key.format, listing.name);
    }
}

/// The studio's scan of everything installed, as the studio does it: in
/// child processes of its own binary, cached. Prints what would not load and
/// how long a first start and a second take.
#[test]
#[ignore]
fn the_studios_scan_of_this_machine() {
    let dir = scratch("machine-scan");
    let cache = dir.join("plugin-scan.json");
    let prober = || {
        std::sync::Arc::new(
            fontelle_host::BundleProber::new(PathBuf::from(env!("CARGO_BIN_EXE_fontelle")))
                .with_cache_file(cache.clone()),
        )
    };
    let mut rack = studio_rack();
    rack.set_prober(Some(prober()));
    let started = std::time::Instant::now();
    rack.rescan();
    let first = started.elapsed();
    eprintln!(
        "first scan: {} plugins, {} would not load, in {first:?}",
        rack.scan().plugins.len(),
        rack.scan().failures.len()
    );
    for failure in &rack.scan().failures {
        eprintln!("  {} — {}", failure.path.display(), failure.why);
    }
    let mut again = fontelle_app::PluginRack::new();
    again.set_prober(Some(prober()));
    let started = std::time::Instant::now();
    again.rescan();
    eprintln!(
        "next start: {:?}: {} plugins, {} failures, folders {:?}",
        started.elapsed(),
        again.scan().plugins.len(),
        again.scan().failures.len(),
        again.folders()
    );
    for (a, b) in rack.scan().plugins.iter().zip(&again.scan().plugins) {
        if a != b {
            eprintln!("first difference:\n  {a:?}\n  {b:?}");
            break;
        }
    }
    assert!(
        rack.scan().plugins == again.scan().plugins,
        "the next start lists something else"
    );
}

/// The smallest thing that plays an installed instrument through the
/// studio's rack: one note, offline, nothing else. For a plugin that will
/// not survive the walks above, under a memory checker.
#[test]
#[ignore]
fn one_note_through_the_rack() {
    use fontelle_model::{ClipSource, Note};
    let key = fontelle_types::PluginKey::parse(&std::env::var("FONTELLE_REAL_KEY").unwrap())
        .expect("FONTELLE_REAL_KEY is format:id");
    let mut project = common::a_project_with_a_clip(2, 120.0, SR);
    let channel = project.channels.keys().next().unwrap();
    project.channels[channel].instrument = Some(fontelle_types::InstrumentKind::Plugin);
    project.channels[channel].plugin = Some(PluginState::new(key, "Under test".to_string()));
    let clip = project.clips.keys().next().unwrap();
    let ClipSource::Notes(data) = &mut project.clips[clip].source else {
        unreachable!()
    };
    data.notes.insert(Note {
        start: 0,
        length: std::env::var("FONTELLE_REAL_BEATS")
            .ok()
            .and_then(|b| b.parse::<i64>().ok())
            .unwrap_or(1)
            * fontelle_types::PPQN,
        key: 60,
        velocity: 100,
        pan: 0,
        fine_pitch: 0,
        release: 0,
        mod_x: 0,
        mod_y: 0,
        slide: false,
        path: Vec::new(),
        channel: None,
    });
    if std::env::var_os("FONTELLE_REAL_SESSION_FIRST").is_some() {
        let dir = scratch("one-note");
        let session = a_session(&dir);
        eprintln!(
            "session saw {} instruments",
            session.plugin_instruments().len()
        );
        drop(session);
    }
    let mut rack = studio_rack();
    eprintln!("peak {}", offline_peak(&project, &mut rack));
}
