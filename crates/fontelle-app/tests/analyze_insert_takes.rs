//! Analyze Musically as a mixer insert, the app half
//! (`docs/analyze-musically-plan.md` §6.1): the live graph gives every
//! Analyze insert a capture and keeps it across a rebuild, renders record
//! nothing, and the take writer puts each take in a WAV of its own.

mod common;

use std::path::PathBuf;
use std::sync::Arc;

use fontelle_app::insert_takes::{InsertTakeWriter, TakeFiles};
use fontelle_app::{KeptTaps, RealiseOptions, SampleLibrary, realise_hosting, render_offline};
use fontelle_engine::{AnalyzeCapture, AnalyzeTapPoint, TransportSnapshot, TransportState};
use fontelle_model::{AddInsert, Command};
use fontelle_types::{AnalyzeConfig, ArmMode, EffectConfig};

use common::SR;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "fontelle-analyze-insert-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// Plays `blocks` blocks of `level` through `capture` as the insert would.
fn feed(
    capture: &AnalyzeCapture,
    config: &AnalyzeConfig,
    blocks: usize,
    level: f32,
    at: i64,
    rolling: bool,
) {
    capture.configure(config, false);
    for block in 0..blocks {
        let (mut left, mut right) = (vec![level; 128], vec![-level; 128]);
        capture.capture(
            &[&mut left, &mut right],
            AnalyzeTapPoint::PreFader,
            &TransportSnapshot {
                state: if rolling {
                    TransportState::Playing
                } else {
                    TransportState::Stopped
                },
                position_sample: at + (block * 128) as i64,
                ..Default::default()
            },
            SR as f32,
        );
    }
}

#[test]
fn each_take_lands_in_a_wav_of_its_own() {
    let dir = scratch("files");
    let capture = AnalyzeCapture::new(48_000);
    let mut files = TakeFiles::new(dir.clone(), SR);
    let now = AnalyzeConfig {
        arm: ArmMode::Now,
        ..AnalyzeConfig::new()
    };
    capture.arm(true);
    feed(&capture, &now, 10, 0.5, 0, false);
    // Drained part-way through: the file grows, the take is not over.
    assert!(files.pump(&capture).is_empty());
    feed(&capture, &now, 5, 0.5, 0, false);
    capture.arm(false);
    feed(&capture, &now, 1, 0.0, 0, false);
    let on_play = AnalyzeConfig::new();
    capture.arm(true);
    feed(&capture, &on_play, 4, 0.25, 96_000, true);
    feed(&capture, &on_play, 1, 0.0, 0, false);

    let takes: Vec<_> = files
        .pump(&capture)
        .into_iter()
        .map(|t| t.expect("written"))
        .collect();
    assert_eq!(takes.len(), 2);
    assert_eq!(takes[0].frames, 15 * 128);
    assert_eq!(takes[0].song_sample, None);
    assert_eq!(takes[1].frames, 4 * 128);
    assert_eq!(takes[1].song_sample, Some(96_000));
    assert_ne!(takes[0].path, takes[1].path);
    for take in &takes {
        assert!(take.path.starts_with(&dir));
        assert_eq!(take.dropped_frames, 0);
        let audio = fontelle_assets::import_audio(&take.path).expect("a take reads back");
        assert_eq!(audio.channels, 2);
        assert_eq!(audio.frames, take.frames);
        assert_eq!(audio.sample_rate, SR);
    }
    let first = fontelle_assets::import_audio(&takes[0].path).expect("reads");
    assert!((first.samples[0] - 0.5).abs() < 1e-3 && (first.samples[1] + 0.5).abs() < 1e-3);
    assert!(files.finish().is_none(), "nothing left open");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_writer_thread_drains_and_stopping_it_closes_an_open_take() {
    let dir = scratch("thread");
    let capture = Arc::new(AnalyzeCapture::new(48_000));
    let writer = InsertTakeWriter::spawn(Arc::clone(&capture), dir.clone(), SR);
    let now = AnalyzeConfig {
        arm: ArmMode::Now,
        ..AnalyzeConfig::new()
    };
    capture.arm(true);
    // Far more than the ring holds, a ring's worth at a time with time
    // between for the thread to drain: nothing is lost.
    for _ in 0..4 {
        feed(&capture, &now, 300, 0.3, 0, false);
        std::thread::sleep(InsertTakeWriter::PERIOD * 5);
    }
    assert!(writer.poll().is_empty(), "the take is still running");
    let takes: Vec<_> = writer
        .stop()
        .into_iter()
        .map(|t| t.expect("written"))
        .collect();
    assert_eq!(takes.len(), 1);
    assert_eq!(takes[0].frames, 4 * 300 * 128);
    assert_eq!(takes[0].dropped_frames, 0);
    assert_eq!(capture.dropped_frames(), 0);
    let audio = fontelle_assets::import_audio(&takes[0].path).expect("reads back");
    assert_eq!(audio.frames, takes[0].frames);
    std::fs::remove_dir_all(&dir).ok();
}

/// The demo document with an Analyze insert on the master, set as `config`.
fn with_an_analyze_insert(config: AnalyzeConfig) -> (fontelle_model::Project, SampleLibrary) {
    let library = SampleLibrary::new();
    let mut project = common::demo_with(&fontelle_core::Patch::basic_synth(), &library);
    let master = project.mixer.master.expect("a master");
    AddInsert::with_config(master, EffectConfig::Analyze(config))
        .apply(&mut project)
        .expect("an insert");
    (project, library)
}

fn options() -> RealiseOptions {
    RealiseOptions {
        sample_rate: SR,
        block_size: fontelle_engine::BLOCK_SIZE,
        quality: fontelle_app::PLAYBACK_QUALITY,
    }
}

fn live(
    project: &fontelle_model::Project,
    library: &SampleLibrary,
    kept: &std::collections::HashMap<(fontelle_types::MixerTrackId, usize), Arc<AnalyzeCapture>>,
) -> fontelle_app::Realised {
    realise_hosting(
        project,
        library,
        options(),
        &Default::default(),
        None,
        &KeptTaps {
            analyze: Some(kept.clone()),
            ..KeptTaps::default()
        },
        None,
        None,
        &Default::default(),
    )
    .expect("realises")
}

#[test]
fn a_live_graph_gives_the_insert_a_capture_and_keeps_it_across_a_rebuild() {
    let (project, library) = with_an_analyze_insert(AnalyzeConfig::new());
    let master = project.mixer.master.expect("a master");
    let first = live(&project, &library, &Default::default());
    let capture = first
        .analyze_captures
        .get(&(master, 0))
        .cloned()
        .expect("the insert has a capture");
    let second = live(&project, &library, &first.analyze_captures);
    assert!(Arc::ptr_eq(
        &capture,
        &second.analyze_captures[&(master, 0)]
    ));
    // And a render's graph records nothing: it is given none.
    let render = fontelle_app::realise(&project, &library, options()).expect("realises");
    assert!(render.analyze_captures.is_empty());
}

#[test]
fn the_live_graph_records_what_plays_through_the_track_pre_and_post_fader() {
    for post_fader in [false, true] {
        let (mut project, library) = with_an_analyze_insert(AnalyzeConfig {
            arm: ArmMode::Now,
            post_fader,
            ..AnalyzeConfig::new()
        });
        // The master fader down 6 dB: pre-fader hears the song as it is,
        // post-fader at half.
        let master = project.mixer.master.expect("a master");
        project.mixer.tracks[master].gain_db = -6.0206;
        let mut realised = live(&project, &library, &Default::default());
        let capture = Arc::clone(&realised.analyze_captures[&(master, 0)]);
        capture.arm(true);
        let timeline =
            fontelle_sequencer::compile(&project, &realised.channel_nodes, &realised.param_nodes);
        let frames = 48_000;
        let out = render_offline(&timeline, &mut realised.graph, frames);
        let mut recorded = Vec::new();
        capture.drain(&mut |event| {
            if let fontelle_engine::AnalyzeCaptureEvent::Audio(audio) = event {
                recorded.extend_from_slice(audio);
            }
        });
        assert_eq!(recorded.len(), out.len(), "every frame that played");
        let peak = |x: &[f32]| x.iter().fold(0.0f32, |a, b| a.max(b.abs()));
        assert!(peak(&out) > 0.01, "the demo is silent");
        let ratio = peak(&recorded) / peak(&out);
        if post_fader {
            assert!(
                (ratio - 1.0).abs() < 0.05,
                "post-fader recorded {ratio} of the output"
            );
        } else {
            assert!(
                (ratio - 2.0).abs() < 0.1,
                "pre-fader recorded {ratio} of the output"
            );
        }
    }
}
