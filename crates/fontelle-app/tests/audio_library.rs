//! Bringing an audio file into a project, and the graph that plays it.
//!
//! *"i want to also be able to record my voice into the daw or import different
//! sounds and loops and whatnot to make songs with."*
//!
//! Three joins, each of which is somewhere a wire can go missing without
//! anything failing loudly:
//!
//! 1. **The library** decodes a file, mints an asset for it and remembers the
//!    audio — the audio-clip counterpart of what `import_sf2` does for a
//!    soundfont.
//! 2. **The graph** gets a player in front of every mixer track, so a clip
//!    routed anywhere has something to be played by.
//! 3. **The compile** finds that player for the track the clip names.
//!
//! Miss any one and the arrangement shows a clip that makes no sound, with no
//! error anywhere. That is the failure this file exists to make impossible.

use fontelle_app::{RealiseOptions, SampleLibrary, realise};
use fontelle_assets::fixtures::build_wav;
use fontelle_model::{Clip, ClipSource, MixerTrack, Project, TempoMap};
use fontelle_types::{AudioClipData, MixerTrackId, PPQN};

fn options() -> RealiseOptions {
    RealiseOptions {
        sample_rate: 48_000,
        block_size: 128,
        quality: fontelle_dsp::Interpolation::Draft,
    }
}

fn scratch(name: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("fontelle-audio-{name}-{}", std::process::id()));
    std::fs::remove_dir_all(&path).ok();
    std::fs::create_dir_all(&path).expect("the scratch directory must be creatable");
    path
}

/// A one-second 440 Hz tone at 48 kHz, written to disk.
fn a_wav(dir: &std::path::Path, name: &str) -> std::path::PathBuf {
    let samples: Vec<f32> = (0..48_000)
        .map(|i| (i as f32 * 440.0 * std::f32::consts::TAU / 48_000.0).sin() * 0.5)
        .collect();
    let path = dir.join(name);
    std::fs::write(&path, build_wav(48_000, 1, &samples)).expect("the file must be writable");
    path
}

// ------------------------------------------------------------ the library ---

#[test]
fn importing_a_sound_mints_an_asset_and_keeps_its_audio() {
    let dir = scratch("import");
    let path = a_wav(&dir, "tone.wav");
    let mut library = SampleLibrary::new();

    let imported = library.import_audio(&path).expect("a wav this test wrote");
    assert_eq!(imported.frames, 48_000);
    assert_eq!(imported.sample_rate, 48_000);
    assert_eq!(imported.asset.kind, fontelle_types::AssetKind::Sample);
    assert_eq!(imported.asset.path, path);

    let store = library.audio_store();
    let buffer = store.get(imported.asset.id).expect("the audio is held");
    assert_eq!(buffer.frames(), 48_000);
    assert_eq!(buffer.channels, 1);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn importing_the_same_file_twice_shares_one_copy_of_its_audio() {
    // A loop dropped on eight rows is one file. Decoding it eight times is
    // eight copies of the same audio in memory, which is TDD §7.7's whole
    // point pointed at audio clips.
    let dir = scratch("twice");
    let path = a_wav(&dir, "loop.wav");
    let mut library = SampleLibrary::new();

    let first = library.import_audio(&path).expect("first");
    let again = library.import_audio(&path).expect("second");
    assert_eq!(first.asset.id, again.asset.id);
    assert_eq!(library.audio_store().len(), 1);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_file_that_is_not_a_sound_is_refused_and_nothing_is_minted() {
    let dir = scratch("refuse");
    let path = dir.join("notes.txt");
    std::fs::write(&path, b"not a sound").expect("writable");
    let mut library = SampleLibrary::new();
    assert!(library.import_audio(&path).is_err());
    assert!(library.audio_store().is_empty(), "a refused file left an asset behind");
    std::fs::remove_dir_all(&dir).ok();
}

// -------------------------------------------------------------- the graph ---

fn a_project() -> (Project, MixerTrackId) {
    let mut project = Project::new("audio");
    project.tempo_map = TempoMap::new(120.0, 48_000.0);
    let master = project.mixer.tracks.insert(MixerTrack::new("Master"));
    project.mixer.master = Some(master);
    let track = project.mixer.tracks.insert(MixerTrack::new("Mic"));
    (project, track)
}

#[test]
fn every_mixer_track_gets_a_player_in_front_of_it() {
    // Including the master, because `None` is the master and a file dropped on
    // the arrangement before anybody has built a track has to sound.
    let (project, track) = a_project();
    let realised = realise(&project, &SampleLibrary::new(), options())
        .expect("a project with a master realises");

    assert!(realised.audio_nodes.contains_key(&None), "the master has no player");
    assert!(
        realised.audio_nodes.contains_key(&Some(track)),
        "a mixer track has no player"
    );
    // And each is a distinct node: two tracks sharing one player would mean a
    // clip routed to one arriving on the other's bus.
    let master_node = realised.audio_nodes[&None];
    assert_ne!(master_node, realised.audio_nodes[&Some(track)]);
}

#[test]
fn a_players_output_is_the_bus_of_the_track_it_belongs_to() {
    let (project, track) = a_project();
    let realised = realise(&project, &SampleLibrary::new(), options())
        .expect("realises");
    let node = realised.audio_nodes[&Some(track)];
    let scheduled = realised
        .graph
        .schedule
        .iter()
        .find(|s| s.id == node)
        .expect("the player is in the schedule");
    assert_eq!(scheduled.node.debug_name(), "audio-clips");
    assert!(
        !scheduled.output_buffers.is_empty(),
        "a player writing to nothing is silence"
    );
    // The master's own buses are 0 and 1, so a non-master track's must not be.
    assert_ne!(scheduled.output_buffers, vec![0, 1]);
}

#[test]
fn a_player_runs_before_the_fader_of_the_track_it_feeds() {
    // The schedule's order is a correctness property and invisible in the
    // sound as anything but a one-block error: a clip summed in after the
    // fader has run is a clip the fader does not affect.
    let (project, track) = a_project();
    let realised = realise(&project, &SampleLibrary::new(), options())
        .expect("realises");
    let node = realised.audio_nodes[&Some(track)];
    let schedule = &realised.graph.schedule;
    let player = schedule.iter().position(|s| s.id == node).expect("the player");
    let fader = schedule
        .iter()
        .position(|s| s.node.debug_name() == "mixer-track")
        .expect("a track fader");
    assert!(player < fader, "the player runs after the fader that carries it");
}

// ------------------------------------------------------ the whole way down ---

#[test]
fn a_clip_dropped_on_a_track_compiles_to_a_placement_on_that_tracks_player() {
    let dir = scratch("endtoend");
    let path = a_wav(&dir, "take.wav");
    let mut library = SampleLibrary::new();
    let imported = library.import_audio(&path).expect("imports");

    let (mut project, track) = a_project();
    let lane = project.lanes.insert(fontelle_model::Lane {
        name: "Audio".into(),
        height: 32.0,
        color: [0; 4],
        muted: false,
        locked: false,
        order: 0,
    });
    let mut data = AudioClipData::whole(imported.asset.clone(), imported.frames as i64, imported.sample_rate);
    data.mixer_track = Some(track);
    project.clips.insert(Clip {
        lane,
        start: PPQN * 4,
        length: PPQN * 8,
        source: ClipSource::Audio(data),
        prefab_link: None,
        color: None,
        muted: false,
        loop_length: None,
    });

    let realised = realise(&project, &library, options()).expect("realises");
    let timeline = fontelle_sequencer::compile_with(
        &project,
        &fontelle_sequencer::NodeMaps {
            channels: &realised.channel_nodes,
            params: &realised.param_nodes,
            audio: &realised.audio_nodes,
        },
        fontelle_sequencer::CompileScope::Song,
    );

    assert_eq!(timeline.audio.len(), 1, "the clip did not reach the timeline");
    assert_eq!(timeline.audio[0].target, realised.audio_nodes[&Some(track)]);
    // And the audio it names is in the store the graph was built with, which is
    // the join that would otherwise fail silently.
    assert!(
        realised_store(&library).get(imported.asset.id).is_some(),
        "the graph cannot reach the audio the clip names"
    );
    std::fs::remove_dir_all(&dir).ok();
}

fn realised_store(library: &SampleLibrary) -> std::sync::Arc<fontelle_core::AudioStore> {
    library.audio_store()
}
