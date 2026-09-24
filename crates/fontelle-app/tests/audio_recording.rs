//! Recording a microphone into the arrangement (TDD §15.4).
//!
//! Reported from using the window:
//!
//! > *"when i click record it prompts me what i would like to record: notes,
//! > audio from mic, automation, etc. then after the 4 tap metronome count in
//! > it starts recording from whatever i set my recording track to. basically i
//! > go in the mixer make a new track, name it to like mic or something then i
//! > click a input button that lets my select my mic input to feed to that
//! > mixer track. when its recording its going through that track and recording
//! > into the arrangement as an audio clip."*
//!
//! There is no microphone in a test, so the take is pushed in through the same
//! ring a real input callback writes to. Everything after that ring is the
//! real path: the WAV in the bundle's `recordings/`, the decode, the asset, the
//! clip, the routing, and the undo.

mod common;

use std::path::{Path, PathBuf};

use fontelle_app::{RealiseOptions, SampleLibrary, Session};
use fontelle_engine::{graph_channel, input_capture_channel, timeline_channel};
use fontelle_model::ClipSource;
use fontelle_types::CompiledTimeline;
use fontelle_ui::document::{ClipKind, DocumentHost, StudioHost};
use fontelle_ui::transport::RecordMode;

use common::SR;

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "fontelle-rec-{name}-{}-{:?}",
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
    let bundle = dir.join("Song.fontelle");
    std::fs::create_dir_all(&bundle).expect("creatable");
    Session::new(
        project,
        library,
        channel_nodes,
        publisher,
        options,
        clip,
        Some(bundle),
    )
    .with_graphs(graphs, realised.track_controls)
    .with_param_nodes(realised.param_nodes)
    .with_settings_path(dir.join("settings.json"))
}

/// The same, **unsaved**: what `cargo run` with no arguments opens onto, and
/// what somebody who has just started the studio is recording into.
fn a_session_without_a_bundle(dir: &Path) -> Session {
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

/// A session with a take already in the ring, as a real input would have left
/// one: half a second of a tone at 48 kHz, mono.
fn with_a_take(session: &mut Session, frames: usize) {
    let (mut writer, reader) = input_capture_channel(frames * 2 + 16);
    let samples: Vec<f32> = (0..frames)
        .map(|i| (i as f32 * 220.0 * std::f32::consts::TAU / 48_000.0).sin() * 0.7)
        .collect();
    assert_eq!(writer.write(&samples), frames, "the ring was too small");
    session.set_audio_input(reader, 48_000, 1);
}

// --------------------------------------------------------- what it records ---

#[test]
fn the_record_button_remembers_what_it_was_told_to_record() {
    // *"when i click record it prompts me what i would like to record"* — and
    // somebody recording eight vocal takes should answer once.
    let dir = scratch("mode");
    let mut session = a_session(&dir);
    assert_eq!(
        session.record_mode(),
        RecordMode::Notes,
        "it used to mean notes"
    );
    session.set_record_mode(RecordMode::Audio);
    assert_eq!(session.record_mode(), RecordMode::Audio);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_mixer_track_remembers_the_input_it_records_from() {
    // *"i go in the mixer make a new track, name it to like mic or something
    // then i click a input button."*
    let dir = scratch("input");
    let mut session = a_session(&dir);
    session.add_mixer_track();
    let strip = session.selected_mixer_track();
    assert_eq!(
        session.track_input(strip),
        None,
        "a fresh track records nothing"
    );

    session.set_track_input(strip, Some("Scarlett Solo".to_string()));
    assert_eq!(
        session.track_input(strip),
        Some("Scarlett Solo".to_string())
    );
    // And it survives a save and an open, because it is a name and not a
    // handle: the same microphone is there tomorrow.
    let json = serde_json::to_string(session.project()).expect("serialisable");
    assert!(json.contains("Scarlett Solo"));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn asking_for_the_inputs_does_not_need_one_to_exist() {
    let dir = scratch("list");
    let session = a_session(&dir);
    for name in session.audio_inputs() {
        assert!(!name.trim().is_empty());
    }
    std::fs::remove_dir_all(&dir).ok();
}

// ------------------------------------------------------------- the take ---

#[test]
fn a_take_becomes_an_audio_clip_on_the_arrangement() {
    let dir = scratch("take");
    let mut session = a_session(&dir);
    session.add_mixer_track();
    let strip = session.selected_mixer_track();
    session.set_track_input(strip, Some("whatever".to_string()));
    session.set_record_mode(RecordMode::Audio);
    with_a_take(&mut session, 24_000);

    let before = session.clips().len();
    let frames = session
        .keep_audio_take(0, 24_000)
        .expect("the take was not kept");
    assert_eq!(frames, 24_000, "the take came back the wrong length");

    let clips = session.clips();
    assert_eq!(clips.len(), before + 1);
    let clip = clips
        .iter()
        .find(|c| c.kind == ClipKind::Audio)
        .expect("no audio clip arrived");
    // Half a second at 120 bpm is one beat.
    assert!(
        (clip.length - fontelle_types::PPQN).abs() <= 2,
        "the clip is {} ticks long",
        clip.length
    );
    assert!(!clip.audio.peaks.is_empty(), "the take has no waveform");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_take_is_written_into_the_projects_own_recordings_folder() {
    // §17.1's layout, and INVARIANT 10: Fontelle writes nowhere the user has
    // not named. A take that landed in a temp directory would be one nobody
    // could find and one a bundle export would miss.
    let dir = scratch("folder");
    let mut session = a_session(&dir);
    with_a_take(&mut session, 4800);
    session
        .keep_audio_take(0, 4800)
        .expect("the take was not kept");

    let recordings = dir.join("Song.fontelle").join("recordings");
    let files: Vec<PathBuf> = std::fs::read_dir(&recordings)
        .expect("the recordings folder must exist")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect();
    assert_eq!(files.len(), 1, "found {files:?}");
    assert_eq!(files[0].extension().and_then(|e| e.to_str()), Some("wav"));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_second_take_does_not_overwrite_the_first() {
    // v1 records a new clip per take (§15.4). Two takes that shared a filename
    // would be one take and a clip pointing at somebody else's audio.
    let dir = scratch("second");
    let mut session = a_session(&dir);
    with_a_take(&mut session, 2400);
    session
        .keep_audio_take(0, 2400)
        .expect("the take was not kept");
    with_a_take(&mut session, 2400);
    session
        .keep_audio_take(4800, 2400)
        .expect("the take was not kept");

    let recordings = dir.join("Song.fontelle").join("recordings");
    let count = std::fs::read_dir(&recordings).expect("exists").count();
    assert_eq!(count, 2);
    assert_eq!(
        session
            .clips()
            .iter()
            .filter(|c| c.kind == ClipKind::Audio)
            .count(),
        2
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_take_lands_where_recording_started_rather_than_at_the_top_of_the_song() {
    let dir = scratch("where");
    let mut session = a_session(&dir);
    with_a_take(&mut session, 4800);
    let at = fontelle_types::PPQN * 8;
    session
        .keep_audio_take(session.sample_of_song_tick(at), 4800)
        .expect("the take was not kept");

    let clips = session.clips();
    let clip = clips
        .iter()
        .find(|c| c.kind == ClipKind::Audio)
        .expect("a clip");
    assert!((clip.start - at).abs() <= 2, "it landed at {}", clip.start);
    std::fs::remove_dir_all(&dir).ok();
}

/// A take names no row, so it arrives on the row the window is looking at
/// (`StudioHost::set_arrival_row`; the report is in `audio_import.rs`,
/// "where the row turns up") — and not under everything.
#[test]
fn a_take_arrives_on_the_row_the_window_is_looking_at() {
    let dir = scratch("arrival");
    let mut session = a_session(&dir);
    for name in ["Drums", "Bass", "Keys", "Vox"] {
        session.add_lane();
        let last = session.lanes().len() - 1;
        session.rename_lane(last, name);
    }
    let before: Vec<String> = session.lanes().into_iter().map(|l| l.name).collect();
    with_a_take(&mut session, 4800);
    session.set_arrival_row(1);
    session
        .keep_audio_take(0, 4800)
        .expect("the take was not kept");
    let after: Vec<String> = session.lanes().into_iter().map(|l| l.name).collect();
    assert_eq!(after.len(), before.len() + 1);
    assert_eq!(
        &after[2..],
        &before[1..],
        "the rows from 1 down moved down one"
    );
    assert!(
        after[1].starts_with("Take"),
        "the take's row is at the arrival index: {after:?}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_take_is_routed_to_the_track_it_was_recorded_through() {
    // *"when its recording its going through that track."* A take that arrived
    // on the master would ignore the strip you built for it.
    let dir = scratch("routed");
    let mut session = a_session(&dir);
    session.add_mixer_track();
    let strip = session.selected_mixer_track();
    session.set_track_input(strip, Some("whatever".to_string()));
    session.set_record_mode(RecordMode::Audio);
    session.select_mixer_track(strip);
    with_a_take(&mut session, 2400);
    session
        .keep_audio_take(0, 2400)
        .expect("the take was not kept");

    let track = session.mixer_track_id(strip).expect("a real track");
    let routed = session
        .project()
        .clips
        .values()
        .find_map(|clip| match &clip.source {
            ClipSource::Audio(data) => Some(data.mixer_track),
            _ => None,
        });
    assert_eq!(routed, Some(Some(track)));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn recording_nothing_at_all_makes_no_clip_and_no_file() {
    // Pressing record and stopping without playing anything is not a failure,
    // and it is not an empty clip on the arrangement either.
    let dir = scratch("silence");
    let mut session = a_session(&dir);
    let before = session.clips().len();
    assert_eq!(session.keep_audio_take(0, 0), Ok(0));
    assert_eq!(session.clips().len(), before);
    let recordings = dir.join("Song.fontelle").join("recordings");
    assert!(
        !recordings.exists() || std::fs::read_dir(&recordings).unwrap().count() == 0,
        "an empty take left a file behind"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_take_is_one_undo_away_from_never_having_happened() {
    let dir = scratch("undo");
    let mut session = a_session(&dir);
    with_a_take(&mut session, 2400);
    let before = session.clips().len();
    let lanes = session.lanes().len();
    session
        .keep_audio_take(0, 2400)
        .expect("the take was not kept");
    assert_eq!(session.clips().len(), before + 1);

    session.undo();
    assert_eq!(session.clips().len(), before);
    assert_eq!(session.lanes().len(), lanes);
    // The file stays: a take is a recording of something that happened, and
    // deleting somebody's audio on Ctrl+Z is not an undo anybody expects.
    let recordings = dir.join("Song.fontelle").join("recordings");
    assert_eq!(std::fs::read_dir(&recordings).expect("exists").count(), 1);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_take_plays_back_through_the_timeline_it_landed_on() {
    // The join that would otherwise fail silently: a take that draws and makes
    // no sound.
    let dir = scratch("plays");
    let mut session = a_session(&dir);
    with_a_take(&mut session, 4800);
    session
        .keep_audio_take(0, 4800)
        .expect("the take was not kept");

    let timeline = session.compiled();
    assert_eq!(timeline.audio.len(), 1);
    assert!(timeline.audio[0].frames() > 0);
    std::fs::remove_dir_all(&dir).ok();
}

// ------------------------------------------------- which track is armed ---

#[test]
fn the_track_that_names_an_input_is_the_one_that_records() {
    // *"i go in the mixer make a new track ... then i click a input button."*
    // Naming an input is the arming gesture; which strip happens to be
    // selected afterwards is not, or moving a fader on another track would
    // quietly redirect the next take.
    let dir = scratch("armed");
    let mut session = a_session(&dir);
    session.add_mixer_track();
    session.add_mixer_track();
    let mic = session.selected_mixer_track();
    session.set_track_input(mic, Some("whatever".to_string()));
    // Somewhere else entirely by the time record is pressed.
    session.select_mixer_track(0);
    session.set_record_mode(RecordMode::Audio);
    with_a_take(&mut session, 2400);
    session
        .keep_audio_take(0, 2400)
        .expect("the take was not kept");

    let track = session.mixer_track_id(mic).expect("a real track");
    let routed = session
        .project()
        .clips
        .values()
        .find_map(|clip| match &clip.source {
            ClipSource::Audio(data) => Some(data.mixer_track),
            _ => None,
        });
    assert_eq!(routed, Some(Some(track)));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_input_the_session_wants_open_is_the_armed_tracks_own() {
    // The device is opened because a track names one, not because record was
    // pressed: *"i should be able to hear routed input playing even when song
    // isnt playing or im not recording."*
    let dir = scratch("wanted");
    let mut session = a_session(&dir);
    assert_eq!(
        session.audio_input_wanted(),
        None,
        "a project where no track names an input must not open a microphone"
    );

    session.add_mixer_track();
    let mic = session.selected_mixer_track();
    session.set_track_input(mic, Some("Scarlett Solo".to_string()));
    assert_eq!(
        session.audio_input_wanted(),
        Some("Scarlett Solo".to_string())
    );

    session.set_track_input(mic, None);
    assert_eq!(
        session.audio_input_wanted(),
        None,
        "clearing the input has to close the device"
    );
    std::fs::remove_dir_all(&dir).ok();
}

// ---------------------------------------- a take you can actually hear ---

#[test]
fn a_take_from_a_track_that_goes_nowhere_lands_where_it_can_be_heard() {
    // > *"for ease of use make it so that if the input track has no output
    // > send it automatically will just route it to master for the clip you
    // > record putting it on that mixer track instead of the one you recorded
    // > on that way your recording will actually be audible after playing it
    // > even if you werent using monitoring."*
    let dir = scratch("audible");
    let mut session = a_session(&dir);
    session.add_mixer_track();
    let mic = session.selected_mixer_track();
    session.set_track_input(mic, Some("whatever".to_string()));
    session.set_track_output_on(mic, false);
    session.set_record_mode(RecordMode::Audio);
    with_a_take(&mut session, 2400);
    session
        .keep_audio_take(0, 2400)
        .expect("the take was not kept");

    let routed = session
        .project()
        .clips
        .values()
        .find_map(|clip| match &clip.source {
            ClipSource::Audio(data) => Some(data.mixer_track),
            _ => None,
        });
    assert_eq!(
        routed,
        Some(None),
        "a take on a track nobody can hear should have gone to the master"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_take_from_a_track_behind_a_bus_that_goes_nowhere_lands_on_the_master_too() {
    // The switch is one hop; audibility is a walk. A mic feeding a group whose
    // own output is off is exactly as inaudible as one switched off itself.
    let dir = scratch("behind");
    let mut session = a_session(&dir);
    session.add_mixer_track();
    let bus = session.selected_mixer_track();
    session.add_mixer_track();
    let mic = session.selected_mixer_track();
    session.set_track_output(mic, Some(bus));
    session.set_track_output_on(bus, false);
    session.set_track_input(mic, Some("whatever".to_string()));
    session.set_record_mode(RecordMode::Audio);
    with_a_take(&mut session, 2400);
    session
        .keep_audio_take(0, 2400)
        .expect("the take was not kept");

    let routed = session
        .project()
        .clips
        .values()
        .find_map(|clip| match &clip.source {
            ClipSource::Audio(data) => Some(data.mixer_track),
            _ => None,
        });
    assert_eq!(routed, Some(None));
    std::fs::remove_dir_all(&dir).ok();
}

// ------------------------------------------------ a take with no project ---

#[test]
fn a_take_with_nowhere_to_live_says_so_rather_than_saying_nothing_arrived() {
    // Reported from using the window: *"when i record it was working at first
    // until i pressed stop to finish the recording and the clip didint get
    // made."* The studio had been started with no arguments, so there was no
    // bundle to write the take into — and the window reported the refusal as
    // *"nothing arrived on the input"*, which was untrue and unhelpful in
    // equal measure. The reason is the return value now.
    let dir = scratch("nowhere");
    let mut session = a_session_without_a_bundle(&dir);
    session.set_projects_dir(None);
    with_a_take(&mut session, 4800);
    let said = session
        .keep_audio_take(0, 4800)
        .expect_err("a take with nowhere to go must say so");
    assert!(said.contains("projects"), "{said}");
    assert!(
        session.clips().iter().all(|c| c.kind != ClipKind::Audio),
        "a take that could not be written must not become a clip"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_take_in_an_unsaved_studio_makes_the_project_real_and_keeps_the_take() {
    // The other half of the same report. A projects folder is configured, so
    // there *is* somewhere the user has named (INVARIANT 10): the take makes
    // the project real — saved into that folder under its own name, the way
    // the Projects tab's "New" does — and lands in it. Losing a take because
    // nobody had pressed Ctrl+S yet is the wrong answer to a first recording.
    let dir = scratch("unsaved");
    let projects = dir.join("projects");
    std::fs::create_dir_all(&projects).expect("creatable");
    let mut session = a_session_without_a_bundle(&dir);
    session.set_projects_dir(Some(projects.clone()));
    assert!(
        session.bundle_path().is_none(),
        "the studio started unsaved"
    );
    with_a_take(&mut session, 4800);

    let frames = session
        .keep_audio_take(0, 4800)
        .expect("the take was not kept");
    assert_eq!(frames, 4800);
    assert_eq!(
        session
            .clips()
            .iter()
            .filter(|c| c.kind == ClipKind::Audio)
            .count(),
        1
    );
    let bundle = session
        .bundle_path()
        .expect("the take made the project real");
    assert_eq!(bundle.parent(), Some(projects.as_path()));
    assert!(bundle.join("recordings").join("Take 1.wav").is_file());
    // And it is a project the Projects tab lists, not a folder of one file.
    assert!(bundle.join("project.json").is_file());
    std::fs::remove_dir_all(&dir).ok();
}

// ------------------------------------------------------------- count-in ---
//
// > *"just put the playhead on the same spot frozen and count in, then play
// > it from there."*
//
// The ring is emptied when play is pressed, the engine counts, and at the end
// of the count the window drops exactly the count's worth from the front of
// what arrived — so the take begins on the marker and loses nothing of its
// first beat. Dropping it the frame the window noticed the count was over
// lost the first ten or twenty milliseconds of every take.

#[test]
fn the_count_ins_worth_is_dropped_from_the_front_of_the_take() {
    let dir = scratch("countfront");
    let mut session = a_session(&dir);
    session.add_mixer_track();
    let strip = session.selected_mixer_track();
    session.set_track_input(strip, Some("whatever".to_string()));
    session.set_record_mode(RecordMode::Audio);
    with_a_take(&mut session, 36_000);
    StudioHost::drop_audio_take_front(&mut session, 12_000);
    let frames = session.keep_audio_take(0, 0).expect("kept");
    assert_eq!(frames, 24_000, "the count was not taken off the front");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_count_is_measured_in_the_inputs_own_frames() {
    // A count in device frames at 48 kHz, an input running at 24 kHz: half
    // as many of its frames went by.
    let dir = scratch("countrate");
    let mut session = a_session(&dir);
    session.add_mixer_track();
    let strip = session.selected_mixer_track();
    session.set_track_input(strip, Some("whatever".to_string()));
    session.set_record_mode(RecordMode::Audio);
    let (mut writer, reader) = input_capture_channel(40_000);
    assert_eq!(writer.write(&vec![0.25f32; 18_000]), 18_000);
    session.set_audio_input(reader, 24_000, 1);
    StudioHost::drop_audio_take_front(&mut session, 24_000);
    let frames = session.keep_audio_take(0, 0).expect("kept");
    assert_eq!(frames, 6_000);
    std::fs::remove_dir_all(&dir).ok();
}
