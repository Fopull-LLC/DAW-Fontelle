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
        .with_headless_plugin_editors();
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
    .with_headless_plugin_editors();
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
        // nothing of it.
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
        // and the patch lands a block or so later.
        settle();
        assert_same("its own browser", &live(&mut session), &second_state);
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
        assert!(!turned.is_empty(), "it has no knobs to turn");
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
