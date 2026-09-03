//! Bringing a sound into a document (TDD §15, §10.6).
//!
//! *"i want to also be able to record my voice into the daw or import different
//! sounds and loops and whatnot to make songs with."*
//!
//! An import is **one command**, for the reason `ImportParts` is: it makes a
//! row *and* a clip, and the clip has to name the row the same command is about
//! to mint. A `Compound` cannot do that — it holds commands built before any of
//! them ran — and two entries in the history would mean an undo that leaves an
//! empty row behind, which is worse than either half.

use fontelle_model::{AddAudioClip, ClipSource, Command, Project};
use fontelle_types::{AssetKind, AssetRef, AudioClipData, PPQN};

fn an_asset(name: &str) -> AssetRef {
    AssetRef {
        id: fontelle_types::AssetId::default(),
        path: name.into(),
        content_hash: 0,
        size: 0,
        kind: AssetKind::Sample,
    }
}

fn a_clip(name: &str) -> AudioClipData {
    AudioClipData::whole(an_asset(name), 48_000)
}

fn import(name: &str, start: i64, length: i64) -> AddAudioClip {
    AddAudioClip::new(name, a_clip(name), start, length)
}

#[test]
fn importing_a_sound_makes_a_row_and_puts_the_clip_on_it() {
    let mut project = Project::new("audio");
    let lanes = project.lanes.len();
    let mut command = import("Vocal.wav", PPQN * 4, PPQN * 8);
    command.apply(&mut project).expect("an import applies");

    assert_eq!(project.lanes.len(), lanes + 1);
    assert_eq!(project.clips.len(), 1);
    let (id, clip) = project.clips.iter().next().expect("a clip");
    assert_eq!(clip.start, PPQN * 4);
    assert_eq!(clip.length, PPQN * 8);
    assert!(matches!(clip.source, ClipSource::Audio(_)));
    assert_eq!(command.clip(), Some(id));
    // And the row is named after the file, which is what somebody scanning an
    // arrangement is looking for.
    let lane = project.lanes.get(clip.lane).expect("the row it made");
    assert_eq!(lane.name, "Vocal.wav");
}

#[test]
fn a_new_row_goes_under_what_is_already_there() {
    // A file dropped onto a song must not push the song down the arrangement.
    let mut project = Project::new("audio");
    let existing = project.lanes.insert(fontelle_model::Lane {
        name: "Keys".into(),
        height: 32.0,
        color: [0; 4],
        muted: false,
        locked: false,
        order: 7,
    });
    let mut command = import("Take.wav", 0, PPQN);
    command.apply(&mut project).expect("applies");
    let made = command.lane().expect("a row");
    assert!(project.lanes[made].order > project.lanes[existing].order);
}

#[test]
fn undoing_an_import_leaves_no_row_and_no_clip_behind() {
    // An undo that leaves an empty row is the reason this is one command.
    let mut project = Project::new("audio");
    let lanes = project.lanes.len();
    let mut command = import("Oops.wav", 0, PPQN);
    command.apply(&mut project).expect("applies");
    command.invert().apply(&mut project).expect("inverts");

    assert_eq!(project.lanes.len(), lanes);
    assert!(project.clips.is_empty());
}

#[test]
fn redoing_an_import_puts_everything_back_under_the_ids_it_first_minted() {
    // Anything stacked above this entry names those ids. A redo that minted
    // fresh ones would leave the command above it pointing at nothing.
    let mut project = Project::new("audio");
    let mut command = import("Again.wav", 0, PPQN);
    command.apply(&mut project).expect("applies");
    let (clip, lane) = (command.clip().unwrap(), command.lane().unwrap());

    command.invert().apply(&mut project).expect("inverts");
    command.apply(&mut project).expect("redoes");

    assert_eq!(command.clip(), Some(clip));
    assert_eq!(command.lane(), Some(lane));
    assert!(project.clips.contains_key(clip));
    assert!(project.lanes.contains_key(lane));
}

#[test]
fn an_import_says_what_it_is_in_the_history() {
    let command = import("Vocal.wav", 0, PPQN);
    assert!(
        command.label().contains("Vocal.wav"),
        "the history says {:?}",
        command.label()
    );
}
