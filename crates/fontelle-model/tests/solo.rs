//! Solo is each studio's own.
//!
//! Ty, 2026-10-18: *"solo should be personal but muting should be shared
//! since thats like an arrangement tool kinda but soloing should just be a
//! local thing not a synced state"* — and *"just make it so soloing cant be
//! undoable then"*.
//!
//! The three solos — a mixer track's, a channel's, a lane's — stay on the
//! things they solo, where the engine reads them, but they are not part of
//! the song: never written, so never sent and never in the hash two
//! studios compare. A song saved before this still opens with its solos on.

use fontelle_model::{Command, Project};

/// A song with a soloed mixer track, channel and lane.
fn soloed() -> Project {
    let mut song = Project::new("solo");
    let mut lane = fontelle_model::AddLane::new("Keys");
    lane.apply(&mut song).unwrap();
    let mut channel = fontelle_model::AddChannel::new("Keys", None);
    channel.apply(&mut song).unwrap();
    for track in song.mixer.tracks.values_mut() {
        track.solo = true;
    }
    for channel in song.channels.values_mut() {
        channel.soloed = true;
    }
    for lane in song.lanes.values_mut() {
        lane.soloed = true;
    }
    song
}

fn unsoloed(mut song: Project) -> Project {
    for track in song.mixer.tracks.values_mut() {
        track.solo = false;
    }
    for channel in song.channels.values_mut() {
        channel.soloed = false;
    }
    for lane in song.lanes.values_mut() {
        lane.soloed = false;
    }
    song
}

#[test]
fn a_solo_is_not_written_into_the_song() {
    let song = soloed();
    let written: Project = serde_json::from_str(&serde_json::to_string(&song).unwrap()).unwrap();
    assert!(
        written.mixer.tracks.values().all(|t| !t.solo),
        "a track's solo was saved"
    );
    assert!(
        written.channels.values().all(|c| !c.soloed),
        "a channel's solo was saved"
    );
    assert!(
        written.lanes.values().all(|l| !l.soloed),
        "a lane's solo was saved"
    );
}

#[test]
fn two_studios_soloing_different_things_still_hold_the_same_song() {
    let song = soloed();
    assert_eq!(song.sync_hash(), unsoloed(song.clone()).sync_hash());
}

/// Puts `field: true` on every object in `json` that has `marker` — how a
/// song written before this change carries its solos.
fn mark(json: &mut serde_json::Value, marker: &str, field: &str) -> usize {
    let mut n = 0;
    match json {
        serde_json::Value::Object(map) => {
            if map.contains_key(marker) {
                map.insert(field.to_string(), true.into());
                n += 1;
            }
            for value in map.values_mut() {
                n += mark(value, marker, field);
            }
        }
        serde_json::Value::Array(items) => {
            for value in items {
                n += mark(value, marker, field);
            }
        }
        _ => {}
    }
    n
}

#[test]
fn a_song_saved_with_a_solo_still_opens_with_it_on() {
    let mut json = serde_json::to_value(unsoloed(soloed())).unwrap();
    // A mixer track has a pan law, a channel says whether its keys are
    // named, a lane has a height: each is found by what only it has.
    assert!(mark(&mut json, "pan_law", "solo") > 0);
    assert!(mark(&mut json, "named_keys", "soloed") > 0);
    assert!(mark(&mut json, "height", "soloed") > 0);
    let opened: Project = serde_json::from_value(json).unwrap();
    assert!(opened.mixer.tracks.values().all(|t| t.solo));
    assert!(opened.channels.values().all(|c| c.soloed));
    assert!(opened.lanes.values().all(|l| l.soloed));
}
