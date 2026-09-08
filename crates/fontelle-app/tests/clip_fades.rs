//! What the host makes of a fade handle: frames of the file, and a bend.
//!
//! `fontelle-ui/tests/clip_fades.rs` is the gesture, which speaks in
//! fractions of the block because that is all a canvas can see. This is the
//! other half: a fraction becomes frames of the clip's own audio, the way
//! the block draws the fade back (`AudioPreview::fade_in` is the same
//! fraction), and a bend becomes the tension the player reads.

mod common;

use std::path::{Path, PathBuf};

use fontelle_app::{RealiseOptions, SampleLibrary, Session};
use fontelle_assets::fixtures::build_wav;
use fontelle_engine::{graph_channel, timeline_channel};
use fontelle_types::{CompiledTimeline, PPQN};
use fontelle_ui::canvas::{ArrangeEdit, FadeEnd};
use fontelle_ui::document::{ClipKind, DocumentHost, StudioHost};

use common::SR;

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "fontelle-fades-{name}-{}-{:?}",
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

/// A session with one audio clip of exactly 24 000 frames on the
/// arrangement, and that clip's id.
fn with_a_take(dir: &Path) -> (Session, fontelle_types::ClipId) {
    let samples: Vec<f32> = (0..24_000)
        .map(|i| (i as f32 * 220.0 * std::f32::consts::TAU / 48_000.0).sin() * 0.8)
        .collect();
    let path = dir.join("take.wav");
    std::fs::write(&path, build_wav(48_000, 1, &samples)).expect("writable");
    let mut session = a_session(dir);
    session.drop_file(&path).expect("the take imports");
    let id = session
        .clips()
        .into_iter()
        .find(|c| c.kind == ClipKind::Audio)
        .expect("the take is on the arrangement")
        .id;
    (session, id)
}

#[test]
fn a_fade_dragged_over_a_quarter_of_the_block_is_a_quarter_of_the_audio() {
    let dir = scratch("quarter");
    let (mut session, clip) = with_a_take(&dir);
    session.arrange(ArrangeEdit::SetFade {
        clip,
        end: FadeEnd::In,
        fraction: 0.25,
    });
    let data = session.audio_clip(clip).expect("an audio clip");
    assert_eq!(data.fade_in.frames, 6000);
    assert_eq!(data.fade_out.frames, 0);
    // And the block draws it back as the same quarter.
    let info = session.clips().into_iter().find(|c| c.id == clip).unwrap();
    assert_eq!(info.kind, ClipKind::Audio);
    assert!((info.audio.fade_in - 0.25).abs() < 1e-6);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_out_fade_is_measured_from_the_end_and_the_two_are_independent() {
    let dir = scratch("out");
    let (mut session, clip) = with_a_take(&dir);
    session.arrange(ArrangeEdit::SetFade {
        clip,
        end: FadeEnd::In,
        fraction: 0.1,
    });
    session.arrange(ArrangeEdit::SetFade {
        clip,
        end: FadeEnd::Out,
        fraction: 0.5,
    });
    let data = session.audio_clip(clip).unwrap();
    assert_eq!(data.fade_in.frames, 2400);
    assert_eq!(data.fade_out.frames, 12_000);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_fade_drag_is_one_undo_however_many_steps_it_took() {
    let dir = scratch("undo");
    let (mut session, clip) = with_a_take(&dir);
    for step in 1..=10 {
        session.arrange(ArrangeEdit::SetFade {
            clip,
            end: FadeEnd::In,
            fraction: step as f32 * 0.05,
        });
    }
    session.end_gesture();
    assert_eq!(session.audio_clip(clip).unwrap().fade_in.frames, 12_000);
    session.undo();
    assert_eq!(
        session.audio_clip(clip).unwrap().fade_in.frames,
        0,
        "one undo takes the whole drag back"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn bending_a_fade_sets_its_tension_and_the_block_draws_the_bend() {
    let dir = scratch("bend");
    let (mut session, clip) = with_a_take(&dir);
    session.arrange(ArrangeEdit::SetFade {
        clip,
        end: FadeEnd::In,
        fraction: 0.5,
    });
    session.arrange(ArrangeEdit::SetFadeTension {
        clip,
        end: FadeEnd::In,
        tension: 0.6,
    });
    let data = session.audio_clip(clip).unwrap();
    assert!((data.fade_in.tension - 0.6).abs() < 1e-6);
    assert_eq!(
        data.fade_in.frames, 12_000,
        "the length is untouched by a bend"
    );
    let info = session.clips().into_iter().find(|c| c.id == clip).unwrap();
    assert!((info.audio.fade_in_tension - 0.6).abs() < 1e-6);
    assert_eq!(info.audio.fade_out_tension, 0.0);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_fade_on_a_note_clip_is_refused_rather_than_invented() {
    let dir = scratch("notes");
    let mut session = a_session(&dir);
    let clip = session.clips()[0].id;
    let before = session.revision();
    session.arrange(ArrangeEdit::SetFade {
        clip,
        end: FadeEnd::In,
        fraction: 0.5,
    });
    assert_eq!(session.clips()[0].kind, ClipKind::Notes);
    let _ = before;
    assert!(session.audio_clip(clip).is_none());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_fade_reaches_the_compiled_placement() {
    // The player reads the placement's copy of the clip, so a fade that did
    // not reach it would be drawn and not heard.
    let dir = scratch("compiled");
    let (mut session, clip) = with_a_take(&dir);
    session.arrange(ArrangeEdit::SetFade {
        clip,
        end: FadeEnd::Out,
        fraction: 0.25,
    });
    session.arrange(ArrangeEdit::SetFadeTension {
        clip,
        end: FadeEnd::Out,
        tension: -0.5,
    });
    let placed = session
        .compiled()
        .audio
        .into_iter()
        .find(|p| p.clip == clip)
        .expect("placed");
    assert_eq!(placed.data.fade_out.frames, 6000);
    assert!((placed.data.fade_out.tension + 0.5).abs() < 1e-6);
    let _ = PPQN;
    std::fs::remove_dir_all(&dir).ok();
}
