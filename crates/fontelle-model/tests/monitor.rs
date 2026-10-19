//! A track's Monitor switch: hear what comes in, or record it unheard.
//! On by default, so every song written before keeps being heard, and
//! written only when off, so those songs read and write the same.

use fontelle_model::{Command, FlagTarget, History, Project, SetFlag};

fn a_song() -> (Project, fontelle_types::MixerTrackId) {
    let mut song = Project::new("song");
    fontelle_model::AddMixerTrack::new("Mic")
        .apply(&mut song)
        .unwrap();
    let track = song
        .mixer
        .tracks
        .keys()
        .find(|id| Some(*id) != song.mixer.master)
        .unwrap();
    (song, track)
}

#[test]
fn a_track_monitors_unless_told_not_to_and_says_so_only_then() {
    let (mut song, track) = a_song();
    assert!(song.mixer.tracks[track].monitor);
    let written = serde_json::to_string(&song).unwrap();
    let at = written.find("\"monitor\"");
    assert!(
        at.is_none(),
        "on is not written: {}",
        &written[at.unwrap_or(0).saturating_sub(80)..(at.unwrap_or(0) + 40).min(written.len())]
    );
    let mut history = History::new();
    history
        .apply(
            Box::new(SetFlag::new(FlagTarget::TrackMonitor(track), false)),
            &mut song,
        )
        .unwrap();
    assert!(!song.mixer.tracks[track].monitor);
    let read: Project = serde_json::from_str(&serde_json::to_string(&song).unwrap()).unwrap();
    assert!(!read.mixer.tracks[track].monitor, "off is kept");
    history.undo(&mut song).unwrap().unwrap();
    assert!(song.mixer.tracks[track].monitor, "one undo puts it back");
}
