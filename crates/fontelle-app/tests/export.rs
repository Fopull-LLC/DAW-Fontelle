//! Exporting the song: how long it is, and what the export options mean.
//!
//! > *"when exporting my project it wasnt exporting the time length properly
//! > it was cut short it wasnt accounting for my audio clips it was only
//! > seemingly recognizing my single instrument clip ... when i click export
//! > it prompts me with the export options so i can chose things like time
//! > selection, whole song, keep things like reverb tail or cut short, etc."*
//!
//! The length of the song used to be the end of the last **note**, and
//! nothing else: a project of audio clips was as long as its notes, which
//! was none. Now every clip that sounds counts — an audio block for its
//! whole length, a place for a prefab for the prefab's notes, a looping
//! block to its end — and the export asks which stretch and whether to keep
//! what rings past it.

mod common;

use std::path::{Path, PathBuf};

use fontelle_app::settings::Settings;
use fontelle_app::{RealiseOptions, SampleLibrary, Session, project_duration_samples};
use fontelle_engine::{graph_channel, timeline_channel};
use fontelle_model::{AddClip, Clip, ClipSource, Command, Note};
use fontelle_types::{CompiledTimeline, PPQN, Tick};
use fontelle_ui::document::{ExportOptions, ExportRange, ExportTail, StudioHost};

use common::SR;

const BAR: Tick = PPQN * 4;

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "fontelle-export-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::remove_dir_all(&path).ok();
    std::fs::create_dir_all(&path).expect("the scratch folder must be creatable");
    path
}

/// A take `seconds` long of a steady tone.
fn a_take(dir: &Path, name: &str, seconds: f32) -> PathBuf {
    let path = dir.join(name);
    let frames = (SR as f32 * seconds) as usize;
    let mut bytes = Vec::with_capacity(44 + frames * 2);
    let data = frames as u32 * 2;
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&SR.to_le_bytes());
    bytes.extend_from_slice(&(SR * 2).to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data.to_le_bytes());
    for i in 0..frames {
        let t = i as f32 / SR as f32;
        let v = ((t * 220.0 * std::f32::consts::TAU).sin() * 0.5 * i16::MAX as f32) as i16;
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    std::fs::write(&path, bytes).expect("the take must be writable");
    path
}

fn a_session(dir: &Path) -> Session {
    let settings = Settings {
        audio_dir: Some(dir.to_path_buf()),
        projects_dir: Some(dir.join("projects")),
        ..Default::default()
    };
    std::fs::write(dir.join("settings.json"), settings.to_json()).unwrap();
    let project = common::a_project_with_a_clip(2, 120.0, SR);
    let clip = Session::first_clip(&project).expect("a blank project has one clip");
    let channel_nodes = fontelle_app::channel_nodes(&project);
    let (publisher, _timeline) = timeline_channel(CompiledTimeline::empty());
    let library = SampleLibrary::new();
    let options = RealiseOptions {
        sample_rate: SR,
        block_size: fontelle_engine::BLOCK_SIZE,
        quality: fontelle_app::PLAYBACK_QUALITY,
    };
    let realised = fontelle_app::realise(&project, &library, options).expect("it must realise");
    let (graphs, _source) = graph_channel(realised.graph);
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
    .with_settings_path(dir.join("settings.json"));
    session.set_projects_dir(Some(dir.join("projects")));
    session
        .save_as("Export")
        .expect("saves, so a render has somewhere to go");
    session
}

/// The frames a rendered file holds.
fn frames_of(said: &str) -> usize {
    // "exported <path>", perhaps followed by " — n clipped samples".
    let path = said
        .strip_prefix("exported ")
        .map(|rest| rest.split(" \u{2014} ").next().unwrap_or(rest).trim())
        .expect("the status line names the file");
    let asset = fontelle_assets::import_audio(Path::new(path)).expect("the render reads back");
    asset.frames
}

// ---------------------------------------------------------------- length ---

#[test]
fn an_audio_clip_makes_the_song_as_long_as_it_is() {
    // Two bars at 120 is four seconds. A take placed at bar 3 for two bars
    // ends at bar 5: the song is eight seconds plus the tail, not the four
    // its (empty) note clip says.
    let mut project = common::a_project_with_a_clip(2, 120.0, SR);
    let lane = project.lane_ids()[0];
    let asset = fontelle_types::AudioClipData::whole(
        fontelle_types::AssetRef {
            id: fontelle_types::AssetId::default(),
            path: PathBuf::from("Take.wav"),
            content_hash: 0,
            size: 0,
            kind: fontelle_types::AssetKind::Sample,
        },
        SR as i64 * 4,
        SR,
    );
    AddClip::new(Clip {
        lane,
        start: BAR * 2,
        length: BAR * 2,
        source: ClipSource::Audio(asset),
        prefab_link: None,
        color: None,
        muted: false,
        loop_length: None,
    })
    .apply(&mut project)
    .expect("adds");
    let samples = project_duration_samples(&project, 0);
    assert_eq!(samples, SR as i64 * 8, "the take's end is the song's end");
}

#[test]
fn a_looping_note_clip_counts_to_its_end_and_a_place_for_a_prefab_counts_its_notes() {
    let mut project = common::a_project_with_a_clip(1, 120.0, SR);
    let clip = Session::first_clip(&project).unwrap();
    // One note at the start of a one-bar clip that is looped out to four
    // bars: it sounds again every bar, so the song runs to the fourth.
    fontelle_model::AddNotes::new(
        clip,
        vec![Note {
            start: 0,
            length: PPQN,
            key: 60,
            velocity: 100,
            pan: 0,
            fine_pitch: 0,
            release: 0,
            mod_x: 0,
            mod_y: 0,
            slide: false,
            channel: None,
        }],
    )
    .apply(&mut project)
    .expect("adds a note");
    fontelle_model::SetClipLoop::new(clip, Some(BAR))
        .apply(&mut project)
        .expect("loops");
    fontelle_model::ResizeClip::new(clip, BAR * 3)
        .apply(&mut project)
        .expect("grows");
    assert_eq!(
        project_duration_samples(&project, 0),
        SR as i64 * 8,
        "four bars of a looping note"
    );

    // A place for a prefab holds no notes of its own; the prefab's count.
    let lane = project.lane_ids()[0];
    let mut make = fontelle_model::MakePrefabFromClip::new(clip, "Riff");
    make.apply(&mut project).expect("makes a prefab");
    let id = make.prefab().expect("the prefab's id");
    let mut place = fontelle_model::AddPrefabInstance::new(id, lane, BAR * 6, BAR);
    place.apply(&mut project).expect("places it");
    // At bar 7 (twelve seconds in) with one beat of note: twelve and a half.
    assert_eq!(
        project_duration_samples(&project, 0),
        SR as i64 * 12 + SR as i64 / 2,
        "the place's notes are the prefab's"
    );
}

// --------------------------------------------------------------- options ---

/// **The report, as a render.** A project of nothing but a four-second
/// take: the whole song, cut at the end, is four seconds of file.
#[test]
fn exporting_the_whole_song_is_as_long_as_its_audio() {
    let dir = scratch("whole");
    let path = a_take(&dir, "Take.wav", 4.0);
    let mut session = a_session(&dir);
    session.drop_file(&path).expect("imports");
    let said = session
        .export_wav_with(ExportOptions {
            range: ExportRange::WholeSong,
            tail: ExportTail::Cut,
        })
        .expect("exports");
    let frames = frames_of(&said);
    assert!(
        (frames as i64 - SR as i64 * 4).abs() <= 2,
        "a four-second take exported as {frames} frames"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn keeping_the_tail_renders_past_the_end_and_cutting_does_not() {
    let dir = scratch("tail");
    let path = a_take(&dir, "Take.wav", 2.0);
    let mut session = a_session(&dir);
    session.drop_file(&path).expect("imports");
    let cut = frames_of(
        &session
            .export_wav_with(ExportOptions {
                range: ExportRange::WholeSong,
                tail: ExportTail::Cut,
            })
            .expect("exports"),
    );
    let kept = frames_of(
        &session
            .export_wav_with(ExportOptions {
                range: ExportRange::WholeSong,
                tail: ExportTail::Keep,
            })
            .expect("exports"),
    );
    assert!(
        (cut as i64 - SR as i64 * 2).abs() <= 2,
        "cut at the end: {cut} frames"
    );
    // A take through a dry chain rings for nothing, so the kept tail is a
    // short one — but it is *there*: more than the cut, and never the
    // whole allowance, which would be a file padded with silence.
    assert!(kept > cut, "the tail was not kept: {kept} against {cut}");
    assert!(
        kept < cut + SR as usize * 2,
        "two seconds of silence were kept as a tail: {kept}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn exporting_the_time_selection_renders_that_stretch_alone() {
    let dir = scratch("selection");
    let path = a_take(&dir, "Take.wav", 4.0);
    let mut session = a_session(&dir);
    session.drop_file(&path).expect("imports");
    // Bar 2 only: one second at 120.
    session.set_loop_range(Some((BAR, BAR * 2)));
    let frames = frames_of(
        &session
            .export_wav_with(ExportOptions {
                range: ExportRange::Selection,
                tail: ExportTail::Cut,
            })
            .expect("exports"),
    );
    assert!(
        (frames as i64 - SR as i64 * 2).abs() <= 2,
        "one bar at 120 is two seconds, not {frames} frames"
    );
    // And with no selection there is nothing to export that way.
    session.set_loop_range(None);
    assert!(
        session
            .export_wav_with(ExportOptions {
                range: ExportRange::Selection,
                tail: ExportTail::Cut,
            })
            .is_err(),
        "a selection export with no selection"
    );
    std::fs::remove_dir_all(&dir).ok();
}
