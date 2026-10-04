//! A built-in instrument keeps its preset — the name on the bar and the
//! sound under it — through working on a saved project, saving it, and
//! opening it again.
//!
//! > *"sometimes they'll just revert back to the init preset when working on
//! > a saved project witch I find causes confusion leading me to have to re
//! > interrelate each track to its presset"*
//!
//! The report names plugins and "the ones you include". The hosted half is
//! `plugin_hosting.rs` and `real_plugin_sessions.rs`; this walks every kind
//! of built-in instrument through the same day, several channels at once,
//! so one channel's preset turning up on another would show too.

mod common;

use std::path::{Path, PathBuf};

use fontelle_app::Session;
use fontelle_app::settings::Settings;
use fontelle_types::InstrumentKind;
use fontelle_ui::canvas::PresetDevice;
use fontelle_ui::document::{DocumentHost, StudioHost};

use common::SR;

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "fontelle-preset-survival-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::remove_dir_all(&path).ok();
    std::fs::create_dir_all(&path).expect("creatable");
    path
}

fn settings(dir: &Path) -> PathBuf {
    let settings = Settings {
        preset_dir: Some(dir.join("presets")),
        projects_dir: Some(dir.join("projects")),
        ..Default::default()
    };
    let path = dir.join("settings.json");
    std::fs::write(&path, settings.to_json()).unwrap();
    path
}

fn a_session(dir: &Path) -> Session {
    let mut session = common::a_session_for(common::a_project_with_a_clip(2, 120.0, SR))
        .with_settings_path(settings(dir));
    session.set_projects_dir(Some(dir.join("projects")));
    session
}

const KINDS: [InstrumentKind; 5] = [
    InstrumentKind::Flopsynth,
    InstrumentKind::Osc3,
    InstrumentKind::DrumMachine,
    InstrumentKind::Sampler,
    InstrumentKind::SoundFont,
];

fn channel(index: usize) -> PresetDevice {
    PresetDevice::Channel { index }
}

/// What a channel's bar says (its preset's name, and whether it reads as
/// edited), and the whole of what the document holds for it apart from its
/// name and id — the sound, as far as a file can say.
type Held = (Option<String>, bool, String);

fn what(session: &Session, index: usize) -> Held {
    let bar = session.preset_bar(channel(index));
    let id = session.project().channels.keys().nth(index).unwrap();
    let mut held = serde_json::to_value(&session.project().channels[id]).unwrap();
    if let Some(map) = held.as_object_mut() {
        map.remove("name");
        map.remove("id");
    }
    (bar.name, bar.dirty, held.to_string())
}

/// A factory preset for the channel's instrument that is not the first in
/// its list, and so not the patch it starts on.
fn a_preset(session: &Session, index: usize, nth: usize) -> Option<usize> {
    let choices = session.preset_choices(channel(index));
    let factory: Vec<usize> = choices
        .iter()
        .enumerate()
        .filter(|(_, c)| c.origin == fontelle_types::PresetOrigin::Factory)
        .map(|(at, _)| at)
        .collect();
    (factory.len() > 1).then(|| factory[(1 + nth) % factory.len()])
}

/// One channel of each kind, each on a preset of its own.
fn a_song(dir: &Path) -> (Session, Vec<(usize, InstrumentKind)>) {
    let mut session = a_session(dir);
    let mut made = Vec::new();
    for (n, kind) in KINDS.iter().enumerate() {
        if n > 0 {
            session.add_channel().expect("adds");
        }
        let index = session.project().channels.len() - 1;
        session.set_channel_kind(index, *kind);
        match a_preset(&session, index, n) {
            Some(at) => {
                session.apply_preset(channel(index), at);
                made.push((index, *kind));
            }
            // No factory presets for this one: one of your own, which is
            // what such a channel would be on.
            None => {
                let name = format!("Mine {kind:?}");
                session.save_preset_as(channel(index), &name, "Mine");
                if let Some(why) = session.take_message() {
                    // An empty sampler, a soundfont player with no file:
                    // nothing to keep. The soundfont's own presets are
                    // `browsing.rs`'s.
                    eprintln!("{kind:?}: {why}; skipped");
                    continue;
                }
                let at = session
                    .preset_choices(channel(index))
                    .iter()
                    .position(|c| c.name == name)
                    .expect("the preset just saved is offered");
                session.add_channel().expect("adds");
                session.undo();
                session.apply_preset(channel(index), at);
                made.push((index, *kind));
            }
        }
    }
    assert!(!made.is_empty(), "no instrument had presets to load");
    eprintln!("on presets: {made:?}");
    (session, made)
}

fn assert_kept(what_was: &[(usize, Held)], session: &Session, when: &str) {
    for (index, before) in what_was {
        let now = what(session, *index);
        assert_eq!(now.0, before.0, "channel {index}'s preset name, {when}");
        assert!(!now.1, "channel {index} reads as edited, {when}");
        assert_eq!(now.2, before.2, "channel {index}'s sound, {when}");
    }
}

#[test]
fn every_built_in_instrument_keeps_its_preset_through_a_working_day() {
    let dir = scratch("day");
    let (mut session, made) = a_song(&dir);
    let before: Vec<_> = made.iter().map(|(i, _)| (*i, what(&session, *i))).collect();
    for (index, (name, dirty, _)) in &before {
        assert!(
            name.is_some(),
            "channel {index} has no preset name after loading one"
        );
        assert!(!dirty);
    }

    session.save_as("Day").expect("saves");
    assert_kept(&before, &session, "after a save");

    // Working on it: things that rebuild, and their undos.
    session.add_channel().expect("adds");
    assert_kept(&before, &session, "after adding a channel");
    session.undo();
    assert_kept(&before, &session, "after undoing it");
    session.redo();
    session.undo();
    session.select_channel(made[0].0);
    session.select_channel(made[made.len() - 1].0);
    assert_kept(&before, &session, "after selecting channels");
    session.add_mixer_track();
    session.undo();
    assert_kept(&before, &session, "after a mixer track came and went");

    session.save().expect("saves again");
    let bundle = session.bundle_path().unwrap().to_path_buf();
    drop(session);

    let mut again = a_session(&dir);
    again.open_project_path(&bundle).expect("opens");
    assert_kept(&before, &again, "after a reopen");
    again.add_channel().expect("adds");
    again.undo();
    assert_kept(&before, &again, "after a reopen and an undo");
    again.save().expect("saves a third time");
    drop(again);
    let mut third = a_session(&dir);
    third.open_project_path(&bundle).expect("opens again");
    assert_kept(&before, &third, "after a second reopen");
}

#[test]
fn a_duplicated_channel_keeps_its_preset_and_so_does_the_copy() {
    let dir = scratch("dup");
    let (mut session, made) = a_song(&dir);
    for (index, _) in made.iter().rev() {
        let before = what(&session, *index);
        session.duplicate_channel(*index);
        // The copy is the channel selected afterwards; the original keeps
        // its place.
        assert_eq!(what(&session, *index).0, before.0);
        let at = session.selected_channel();
        assert_ne!(at, *index, "the copy is selected");
        let copy = what(&session, at);
        assert_eq!(copy.0, before.0, "the copy of channel {index}");
        assert_eq!(copy.2, before.2);
        session.undo();
    }
}

/// A project saved by this version opens with the same presets in a session
/// at another sample rate — what a different audio device gives it.
#[test]
fn a_reopen_at_another_rate_keeps_every_preset() {
    let dir = scratch("rate");
    let (mut session, made) = a_song(&dir);
    let before: Vec<_> = made.iter().map(|(i, _)| (*i, what(&session, *i))).collect();
    session.save_as("Rate").expect("saves");
    let bundle = session.bundle_path().unwrap().to_path_buf();
    drop(session);

    let project = common::a_project_with_a_clip(2, 120.0, 44_100);
    let mut again = common::a_session_for(project).with_settings_path(settings(&dir));
    again.open_project_path(&bundle).expect("opens");
    assert_kept(&before, &again, "at 44.1 kHz");
}
