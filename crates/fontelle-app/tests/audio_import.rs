//! Bringing a sound into the arrangement, through the studio the window drives.
//!
//! *"i want to also be able to record my voice into the daw or import different
//! sounds and loops and whatnot to make songs with... i should be able to see
//! the waveform of the audio inside the clip."*
//!
//! The file formats are tested in `fontelle-assets`, the placement in
//! `fontelle-sequencer`, the sound in `fontelle-engine`, the block's geometry in
//! `fontelle-ui`. This is the seam between all of them: a file on disk, a drop
//! or a click, and an arrangement with a take on it that draws and plays.

mod common;

use std::path::{Path, PathBuf};

use fontelle_app::{RealiseOptions, SampleLibrary, Session};
use fontelle_assets::fixtures::build_wav;
use fontelle_engine::{graph_channel, timeline_channel};
use fontelle_model::ClipSource;
use fontelle_types::{CompiledTimeline, FolderKind};
use fontelle_ui::document::{ClipKind, DocumentHost, StudioHost};

use common::SR;

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "fontelle-audio-import-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::remove_dir_all(&path).ok();
    std::fs::create_dir_all(&path).expect("the scratch folder must be creatable");
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

/// Half a second of a 220 Hz tone, written where a drop can find it.
fn a_take(dir: &Path, name: &str) -> PathBuf {
    let samples: Vec<f32> = (0..24_000)
        .map(|i| (i as f32 * 220.0 * std::f32::consts::TAU / 48_000.0).sin() * 0.8)
        .collect();
    let path = dir.join(name);
    std::fs::write(&path, build_wav(48_000, 1, &samples)).expect("writable");
    path
}

// ------------------------------------------------------------- the drop ---

#[test]
fn dropping_a_sound_on_the_window_puts_it_on_the_arrangement() {
    let dir = scratch("drop");
    let path = a_take(&dir, "Vocal.wav");
    let mut session = a_session(&dir);
    let before = session.clips().len();

    let said = session
        .drop_file(&path)
        .expect("a wav is something Fontelle opens");
    assert!(said.contains("Vocal"), "it said {said:?}");

    let clips = session.clips();
    assert_eq!(clips.len(), before + 1);
    let clip = clips
        .iter()
        .find(|c| c.kind == ClipKind::Audio)
        .expect("no audio clip arrived");
    assert_eq!(
        clip.name, "Vocal",
        "a take is captioned with the file it came from"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_dropped_sound_arrives_with_its_waveform_already_drawn() {
    // §15.3 allows drawing what exists while peaks generate. A file this short
    // is decoded and summarised before the drop returns, so there is no excuse
    // for an empty block — and an empty block is exactly what "it imported but
    // I can't see anything" looks like.
    let dir = scratch("waveform");
    let path = a_take(&dir, "Loop.wav");
    let mut session = a_session(&dir);
    session.drop_file(&path).expect("imports");

    let clips = session.clips();
    let clip = clips
        .iter()
        .find(|c| c.kind == ClipKind::Audio)
        .expect("a clip");
    assert!(
        !clip.audio.peaks.is_empty(),
        "the block has no waveform in it"
    );
    let loudest = clip
        .audio
        .peaks
        .iter()
        .map(|(_, high)| *high)
        .fold(0.0f32, f32::max);
    assert!(loudest > 0.5, "a loud take drew as {loudest}");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_dropped_sound_is_as_long_on_the_arrangement_as_it_is_in_seconds() {
    // Half a second at 120 bpm is one beat. A clip that arrived a bar long
    // would be a clip whose end is not where the sound ends, and every trim
    // after it would be against the wrong edge.
    let dir = scratch("length");
    let path = a_take(&dir, "Beat.wav");
    let mut session = a_session(&dir);
    session.drop_file(&path).expect("imports");

    let clips = session.clips();
    let clip = clips
        .iter()
        .find(|c| c.kind == ClipKind::Audio)
        .expect("a clip");
    assert!(
        (clip.length - fontelle_types::PPQN).abs() <= 2,
        "half a second at 120 bpm is one beat, got {} ticks",
        clip.length
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_dropped_sound_lands_on_a_row_of_its_own_rather_than_over_what_is_there() {
    let dir = scratch("row");
    let mut session = a_session(&dir);
    let lanes_before = session.lanes().len();
    session
        .drop_file(&a_take(&dir, "One.wav"))
        .expect("imports");
    session
        .drop_file(&a_take(&dir, "Two.wav"))
        .expect("imports");

    assert_eq!(session.lanes().len(), lanes_before + 2, "they shared a row");
    let clips = session.clips();
    let rows: Vec<usize> = clips
        .iter()
        .filter(|c| c.kind == ClipKind::Audio)
        .map(|c| c.lane)
        .collect();
    assert_eq!(rows.len(), 2);
    assert_ne!(rows[0], rows[1]);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_dropped_sound_is_one_undo_away_from_never_having_happened() {
    let dir = scratch("undo");
    let mut session = a_session(&dir);
    let before = session.clips().len();
    let lanes = session.lanes().len();
    session
        .drop_file(&a_take(&dir, "Oops.wav"))
        .expect("imports");
    assert_eq!(session.clips().len(), before + 1);

    session.undo();
    assert_eq!(session.clips().len(), before, "the clip is still there");
    assert_eq!(
        session.lanes().len(),
        lanes,
        "the row it made is still there"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_file_that_is_not_a_sound_still_says_what_fontelle_reads() {
    let dir = scratch("refused");
    let path = dir.join("notes.txt");
    std::fs::write(&path, b"nope").expect("writable");
    let mut session = a_session(&dir);
    let err = session
        .drop_file(&path)
        .expect_err("a text file is not a sound");
    assert!(
        err.contains(".wav"),
        "the message does not mention audio: {err}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

// ----------------------------------------------------------- the folder ---

#[test]
fn audio_is_a_folder_the_settings_remember_like_the_other_two() {
    // The import browser reads a folder per kind, and *"if i dont have a folder
    // selected yet, take me to the settings menu"* was the rule for the other
    // two. A third kind is a variant and nothing else — which is the whole
    // reason `FolderKind` exists.
    assert!(FolderKind::ALL.contains(&FolderKind::Audio));
    assert!(FolderKind::Audio.accepts(Path::new("take.wav")));
    assert!(FolderKind::Audio.accepts(Path::new("Loop.FLAC")));
    assert!(FolderKind::Audio.accepts(Path::new("song.mp3")));
    assert!(!FolderKind::Audio.accepts(Path::new("part.mid")));
    assert!(!FolderKind::Audio.accepts(Path::new("piano.sf2")));
    assert!(!FolderKind::Audio.picker_title().is_empty());
}

// -------------------------------------------------------- and it sounds ---

#[test]
fn a_dropped_sound_reaches_the_compiled_timeline() {
    // The join that would otherwise fail silently: a clip that draws
    // perfectly, is routed nowhere, and makes no sound with no error anywhere.
    let dir = scratch("timeline");
    let mut session = a_session(&dir);
    session
        .drop_file(&a_take(&dir, "Take.wav"))
        .expect("imports");

    let timeline = session.compiled();
    assert_eq!(
        timeline.audio.len(),
        1,
        "the clip is on the arrangement and not on the timeline"
    );
    assert!(timeline.audio[0].frames() > 0);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_clip_names_a_source_the_graph_can_actually_play() {
    let dir = scratch("store");
    let mut session = a_session(&dir);
    session
        .drop_file(&a_take(&dir, "Take.wav"))
        .expect("imports");

    let asset = session
        .project()
        .clips
        .values()
        .find_map(|clip| match &clip.source {
            ClipSource::Audio(data) => Some(data.asset.id),
            _ => None,
        })
        .expect("an audio clip");
    assert!(
        session.library().audio_store().get(asset).is_some(),
        "the clip names audio nothing is holding"
    );
    std::fs::remove_dir_all(&dir).ok();
}

// ------------------------------------------------------- editing one ---

#[test]
fn the_editor_reads_a_clips_properties_and_writes_them_back() {
    // The seam between the window's editor and the document. Everything either
    // side of it is tested where it lives; this is the join, and a join that
    // reads but does not write is an editor whose knobs do nothing.
    let dir = scratch("edit");
    let mut session = a_session(&dir);
    session
        .drop_file(&a_take(&dir, "Take.wav"))
        .expect("imports");
    let id = session
        .clips()
        .iter()
        .find(|c| c.kind == ClipKind::Audio)
        .expect("a clip")
        .id;

    let mut data = session.audio_clip(id).expect("the editor can read it");
    assert_eq!(data.gain_db, 0.0);
    data.gain_db = -6.0;
    data.fade_in = fontelle_types::Fade {
        frames: 4800,
        curve: fontelle_types::FadeCurve::SCurve,
        tension: 0.0,
    };
    session.set_audio_clip(id, data);

    assert_eq!(session.audio_clip(id).expect("still there").gain_db, -6.0);
    // And the block on the arrangement redrew with the fade on it, which is
    // what makes the picture the sound.
    let clips = session.clips();
    let block = clips.iter().find(|c| c.id == id).expect("the block");
    assert!(block.audio.fade_in > 0.0, "the block did not take the fade");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_editor_knows_the_rate_the_file_was_recorded_at() {
    // What lets a fade read in milliseconds rather than in frames.
    let dir = scratch("rate");
    let mut session = a_session(&dir);
    session
        .drop_file(&a_take(&dir, "Take.wav"))
        .expect("imports");
    let id = session
        .clips()
        .iter()
        .find(|c| c.kind == ClipKind::Audio)
        .expect("a clip")
        .id;
    assert_eq!(session.audio_clip_rate(id), 48_000);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_clip_that_is_not_audio_has_nothing_for_the_editor_to_show() {
    let dir = scratch("notaudio");
    let session = a_session(&dir);
    let notes = session
        .clips()
        .iter()
        .find(|c| c.kind == ClipKind::Notes)
        .expect("a blank project has a note clip")
        .id;
    assert!(session.audio_clip(notes).is_none());
    assert_eq!(session.audio_clip_rate(notes), 0);
    std::fs::remove_dir_all(&dir).ok();
}
