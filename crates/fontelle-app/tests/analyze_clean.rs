//! Analyze Musically's Clean and Slice pages through the studio
//! (`docs/analyze-musically-plan.md` §2.7, §3.7, P3): trim, fades, gain and
//! the denoiser are the study's, one undo a gesture; a render applies them,
//! and trim becomes the clip's span rather than a cut file; a noise capture
//! says how loud the noise was; markers are the study's frames.
//!
//! The audio is synthesised here into a scratch folder; nothing is
//! committed.

mod common;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use fontelle_app::Session;
use fontelle_assets::fixtures::build_wav;
use fontelle_model::ClipSource;
use fontelle_types::{ClipId, StudyClean, StudyMarker};
use fontelle_ui::document::{ClipKind, DocumentHost, JobPoll, StudioHost};

use common::SR;

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "fontelle-analyze-clean-{name}-{}",
        std::process::id()
    ));
    std::fs::remove_dir_all(&path).ok();
    std::fs::create_dir_all(&path).expect("creatable");
    path
}

/// A little deterministic hiss, about `level` RMS.
fn hiss(n: usize, level: f32, seed: u32) -> Vec<f32> {
    let mut state = seed.wrapping_mul(2_654_435_761).max(1);
    (0..n)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            // Uniform in -1..1 has an RMS of 1/√3.
            ((state as f32 / u32::MAX as f32) * 2.0 - 1.0) * level * 3f32.sqrt()
        })
        .collect()
}

/// One second of hiss alone, then two of an A3 over the same hiss.
fn noisy_take() -> Vec<f32> {
    let n = SR as usize * 3;
    let noise = hiss(n, 0.01, 7);
    (0..n)
        .map(|i| {
            let t = i as f32 / SR as f32;
            let tone = if t >= 1.0 {
                0.3 * (std::f32::consts::TAU * 220.0 * t).sin()
            } else {
                0.0
            };
            tone + noise[i]
        })
        .collect()
}

/// A saved song with `samples` dropped on it as an audio clip, analysed.
fn studied(name: &str, samples: &[f32]) -> (PathBuf, Session, ClipId) {
    let dir = scratch(name);
    let mut session = common::a_session_in(
        common::a_project_with_a_clip(16, 120.0, SR),
        Some(dir.join("Song")),
    );
    let path = dir.join("Take.wav");
    std::fs::write(&path, build_wav(SR, 1, samples)).expect("writable");
    session
        .drop_file_on(&path, i64::from(SR) * 4, Some(0))
        .expect("the drop lands");
    let clip = session
        .clips()
        .into_iter()
        .find(|c| c.kind == ClipKind::Audio)
        .expect("an audio clip")
        .id;
    session.analyze_musically(clip).unwrap();
    let started = Instant::now();
    while matches!(session.poll_analysis(), JobPoll::Running(_)) {
        assert!(started.elapsed() < Duration::from_secs(120));
        std::thread::sleep(Duration::from_millis(2));
    }
    (dir, session, clip)
}

/// A render started is waited for, off the window's thread, and how it went.
fn rendered(session: &mut Session) -> Result<String, String> {
    let started = Instant::now();
    loop {
        match session.poll_analysis_render() {
            JobPoll::Finished(result) => return result,
            JobPoll::Running(_) => {}
            JobPoll::Idle => panic!("no render was running"),
        }
        assert!(started.elapsed() < Duration::from_secs(60));
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn audio(session: &Session, clip: ClipId) -> fontelle_types::AudioClipData {
    match &session.project().clips[clip].source {
        ClipSource::Audio(data) => data.clone(),
        _ => panic!("audio"),
    }
}

fn read_mono(path: &Path) -> Vec<f32> {
    let decoded = fontelle_assets::import_audio(path).expect("a WAV");
    let channels = usize::from(decoded.channels.max(1));
    decoded
        .samples
        .chunks(channels)
        .map(|f| f.iter().sum::<f32>() / channels as f32)
        .collect()
}

fn rms(samples: &[f32]) -> f32 {
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len().max(1) as f32).sqrt()
}

fn seconds(samples: &[f32], from: f64, to: f64) -> &[f32] {
    let a = (from * f64::from(SR)) as usize;
    let b = ((to * f64::from(SR)) as usize).min(samples.len());
    &samples[a.min(b)..b]
}

fn db(ratio: f32) -> f32 {
    20.0 * ratio.max(1e-9).log10()
}

fn the_render(session: &Session, dir: &Path, clip: ClipId) -> Vec<f32> {
    let asset = audio(session, clip).asset;
    let path = if asset.path.is_absolute() {
        asset.path.clone()
    } else {
        dir.join("Song").join(&asset.path)
    };
    read_mono(&path)
}

/// Plan §2.7: *"Trim maps onto the clip's source_start/end at render, so the
/// file is not cut"* — and the render is one undo.
#[test]
fn trim_becomes_the_clips_span_on_render_and_is_one_undo() {
    let (dir, mut session, clip) = studied("trim", &noisy_take());
    let before = audio(&session, clip);
    let length_before = session.project().clips[clip].length;
    let view = session.analyze_view().unwrap();
    let (a, b) = (view.frame_of(1.25), view.frame_of(2.5));
    session
        .set_analysis_clean(
            StudyClean {
                trim: Some((a, b)),
                ..StudyClean::default()
            },
            false,
        )
        .expect("a trim is a clean");
    session.end_gesture();
    assert_eq!(session.analyze_view().unwrap().clean.trim, Some((a, b)));
    let said = session.render_analysis(false).expect("a render starts");
    assert!(!said.is_empty());
    rendered(&mut session).expect("the render lands");
    let after = audio(&session, clip);
    assert_ne!(after.asset, before.asset, "the clip plays the render");
    assert_eq!((after.source_start, after.source_end), (a, b));
    // The file is the whole length: the trim is the clip's span, not a cut.
    let render = the_render(&session, &dir, clip);
    assert_eq!(render.len(), noisy_take().len());
    let length_after = session.project().clips[clip].length;
    let want =
        length_before as f64 * (b - a) as f64 / (before.source_end - before.source_start) as f64;
    assert!(
        (length_after as f64 - want).abs() <= 2.0,
        "{length_before} -> {length_after}, wanted {want}"
    );
    session.undo();
    let back = audio(&session, clip);
    assert_eq!(back.asset, before.asset, "one undo");
    assert_eq!(
        (back.source_start, back.source_end),
        (before.source_start, before.source_end)
    );
    assert_eq!(session.project().clips[clip].length, length_before);
    let study = session.project().studies.values().next().unwrap().clone();
    assert!(study.rendered.is_none());
    assert_eq!(study.clean.trim, Some((a, b)), "the trim itself is kept");
    std::fs::remove_dir_all(&dir).ok();
}

/// The Noise tool: a span with nothing but the noise in it is captured, and
/// its level is said. The denoiser then takes the hiss out of the render and
/// leaves the tone.
#[test]
fn a_captured_noise_is_taken_out_of_the_render() {
    let take = noisy_take();
    let (dir, mut session, clip) = studied("denoise", &take);
    let said = session
        .capture_analysis_noise(0.1, 0.9)
        .expect("a noise-only span captures");
    println!("{said}");
    assert!(said.contains("dB"), "{said}");
    let view = session.analyze_view().unwrap();
    let noise = view.clean.denoise.noise.clone().expect("captured");
    let level = db(rms(seconds(&take, 0.1, 0.9)));
    assert!(
        (noise.level_db - level).abs() < 1.0,
        "{} vs {level}",
        noise.level_db
    );
    // Too short to tell noise from anything is refused, and says so.
    assert!(session.capture_analysis_noise(0.1, 0.105).is_err());

    let mut clean = view.clean.clone();
    clean.denoise.on = true;
    clean.denoise.reduce_db = 24.0;
    session.set_analysis_clean(clean, false).unwrap();
    session.end_gesture();
    session.render_analysis(false).unwrap();
    rendered(&mut session).expect("renders");
    let render = the_render(&session, &dir, clip);
    let hiss_before = rms(seconds(&take, 0.2, 0.8));
    let hiss_after = rms(seconds(&render, 0.2, 0.8));
    let tone_before = rms(seconds(&take, 1.5, 2.5));
    let tone_after = rms(seconds(&render, 1.5, 2.5));
    println!(
        "hiss {:.1} -> {:.1} dB, tone {:.1} -> {:.1} dB",
        db(hiss_before),
        db(hiss_after),
        db(tone_before),
        db(tone_after)
    );
    assert!(db(hiss_before) - db(hiss_after) > 12.0);
    assert!((db(tone_before) - db(tone_after)).abs() < 1.0);
    std::fs::remove_dir_all(&dir).ok();
}

/// Gain and fades are in the render: -6 dB is half, a fade in starts in
/// silence at the trim's start.
#[test]
fn gain_and_fades_are_rendered_from_the_trims_ends() {
    let take: Vec<f32> = (0..SR as usize * 2)
        .map(|i| 0.4 * (std::f32::consts::TAU * 330.0 * i as f32 / SR as f32).sin())
        .collect();
    let (dir, mut session, clip) = studied("gain", &take);
    let view = session.analyze_view().unwrap();
    let start = view.frame_of(0.5);
    session
        .set_analysis_clean(
            StudyClean {
                trim: Some((start, view.frame_of(2.0))),
                fade_in: i64::from(SR) / 4,
                gain_db: -6.0206,
                ..StudyClean::default()
            },
            false,
        )
        .unwrap();
    session.end_gesture();
    session.render_analysis(false).unwrap();
    rendered(&mut session).expect("renders");
    let render = the_render(&session, &dir, clip);
    let half = rms(seconds(&render, 1.0, 1.9)) / rms(seconds(&take, 1.0, 1.9));
    assert!((half - 0.5).abs() < 0.02, "{half}");
    let first = start as usize;
    assert!(render[first].abs() < 1e-3, "the fade starts at the trim");
    assert!(rms(&render[first..first + 480]) < 0.02);
    std::fs::remove_dir_all(&dir).ok();
}

/// A knob's drag on the Clean page is many changes and one undo — and the
/// first one, on a clip with no study yet, starts it in the same entry.
#[test]
fn a_clean_knobs_drag_is_one_undo() {
    let (dir, mut session, _) = studied("knob", &noisy_take());
    assert!(session.project().studies.is_empty());
    for db in [-1.0, -2.0, -4.5] {
        session
            .set_analysis_clean(
                StudyClean {
                    gain_db: db,
                    ..StudyClean::default()
                },
                true,
            )
            .unwrap();
    }
    session.end_gesture();
    assert_eq!(session.analyze_view().unwrap().clean.gain_db, -4.5);
    session.undo();
    assert!(session.project().studies.is_empty(), "one undo");
    assert_eq!(session.analyze_view().unwrap().clean, StudyClean::default());
    std::fs::remove_dir_all(&dir).ok();
}

/// Markers set on the lane are the study's, in frames of its original.
#[test]
fn markers_are_the_studys_frames_and_one_undo() {
    let (dir, mut session, _) = studied("markers", &noisy_take());
    let view = session.analyze_view().unwrap();
    let at = view.frame_of(1.0);
    session
        .set_analysis_markers(
            vec![StudyMarker {
                id: 1,
                at,
                name: String::new(),
            }],
            false,
        )
        .unwrap();
    session.end_gesture();
    let view = session.analyze_view().unwrap();
    assert_eq!(view.markers.len(), 1);
    assert!((view.seconds_of(view.markers[0].at) - 1.0).abs() < 1e-3);
    let study = session.project().studies.values().next().unwrap();
    assert_eq!(study.markers[0].at, at);
    session.undo();
    assert!(session.analyze_view().unwrap().markers.is_empty());
    let _ = DocumentHost::save(&mut session);
    std::fs::remove_dir_all(&dir).ok();
}

/// The view says where transients are, strongest first-class: a click track
/// has one per click.
#[test]
fn the_view_carries_the_transients_for_the_slice_page() {
    let mut take = vec![0.0f32; SR as usize * 2];
    for k in 0..4 {
        let at = SR as usize / 4 + k * SR as usize * 2 / 5;
        for i in 0..400 {
            take[at + i] = 0.8 * (-(i as f32) / 60.0).exp() * if i % 2 == 0 { 1.0 } else { -1.0 };
        }
    }
    let (dir, session, _) = studied("onsets", &take);
    let view = session.analyze_view().unwrap();
    let strong: Vec<f64> = view
        .onsets
        .iter()
        .filter(|(_, s)| *s > 0.3)
        .map(|(t, _)| *t)
        .collect();
    assert_eq!(strong.len(), 4, "{:?}", view.onsets);
    for (k, t) in strong.iter().enumerate() {
        let want = 0.25 + k as f64 * 0.4;
        assert!((t - want).abs() < 0.01, "{t} vs {want}");
    }
    std::fs::remove_dir_all(&dir).ok();
}
