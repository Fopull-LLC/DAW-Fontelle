//! What the host makes of the arrangement's Stretch switch.
//!
//! `fontelle-ui/tests/stretch_toggle.rs` is the gesture: a drag on an audio
//! clip's edge says, once, which mode the clip is to be in. This is the other
//! half — that `ArrangeEdit::SetStretch` lands on the clip's own
//! `ClipStretch`, leaves a note clip alone, and that the block is told how
//! long its file really is so it can draw the file ending where it ends.

mod common;

use std::path::{Path, PathBuf};

use fontelle_app::{RealiseOptions, SampleLibrary, Session};
use fontelle_assets::fixtures::build_wav;
use fontelle_engine::{graph_channel, timeline_channel};
use fontelle_types::{ClipStretch, CompiledTimeline, PPQN};
use fontelle_ui::canvas::ArrangeEdit;
use fontelle_ui::document::{ClipKind, DocumentHost, StudioHost};

use common::SR;

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "fontelle-stretch-{name}-{}-{:?}",
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

/// A session with one audio clip of exactly half a second — one beat at
/// 120 — on the arrangement, and that clip's id.
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
fn set_stretch_lands_on_the_clips_own_mode_and_the_block_says_so() {
    let dir = scratch("mode");
    let (mut session, clip) = with_a_take(&dir);
    assert_eq!(session.audio_clip(clip).unwrap().stretch, ClipStretch::Off);
    assert!(
        !session
            .clips()
            .into_iter()
            .find(|c| c.id == clip)
            .unwrap()
            .audio
            .stretched
    );

    session.arrange(ArrangeEdit::SetStretch {
        ids: vec![clip],
        stretch: ClipStretch::Resample,
    });
    assert_eq!(
        session.audio_clip(clip).unwrap().stretch,
        ClipStretch::Resample
    );
    assert!(
        session
            .clips()
            .into_iter()
            .find(|c| c.id == clip)
            .unwrap()
            .audio
            .stretched
    );

    session.arrange(ArrangeEdit::SetStretch {
        ids: vec![clip],
        stretch: ClipStretch::Off,
    });
    assert_eq!(session.audio_clip(clip).unwrap().stretch, ClipStretch::Off);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_note_clip_in_the_list_is_left_alone_rather_than_refused() {
    let dir = scratch("notes");
    let (mut session, clip) = with_a_take(&dir);
    let part = session
        .clips()
        .into_iter()
        .find(|c| c.kind == ClipKind::Notes)
        .expect("the blank project's clip")
        .id;
    session.arrange(ArrangeEdit::SetStretch {
        ids: vec![part, clip],
        stretch: ClipStretch::Resample,
    });
    assert_eq!(
        session.audio_clip(clip).unwrap().stretch,
        ClipStretch::Resample
    );
    assert!(session.audio_clip(part).is_none(), "still a note clip");
    assert_eq!(
        session
            .clips()
            .into_iter()
            .find(|c| c.id == part)
            .unwrap()
            .kind,
        ClipKind::Notes
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_block_is_told_how_long_the_file_is_and_the_length_does_not_move_it() {
    // Half a second at 120 is one beat: the block a drop makes is exactly
    // that long, and so is the file inside it. Dragged out to two beats, the
    // block is longer and the file is not — which is what lets the block draw
    // the file ending in its middle rather than smeared across it.
    let dir = scratch("natural");
    let (mut session, clip) = with_a_take(&dir);
    let info = session.clips().into_iter().find(|c| c.id == clip).unwrap();
    assert_eq!(info.length, PPQN);
    assert_eq!(info.audio.natural_length, PPQN);

    session.arrange(ArrangeEdit::Resize {
        ids: vec![clip],
        tick_delta: PPQN,
    });
    let info = session.clips().into_iter().find(|c| c.id == clip).unwrap();
    assert_eq!(info.length, PPQN * 2);
    assert_eq!(
        info.audio.natural_length, PPQN,
        "the file did not get longer"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_files_length_follows_the_tempo_because_it_is_measured_in_ticks() {
    // At 60 the same half second is half a beat. The natural length is a
    // fact about the file *on this song*, and a tempo change moves it — the
    // opposite of a stretched clip, whose block is the constant.
    let dir = scratch("tempo");
    let (mut session, clip) = with_a_take(&dir);
    session.set_tempo(60.0);
    let info = session.clips().into_iter().find(|c| c.id == clip).unwrap();
    assert_eq!(info.audio.natural_length, PPQN / 2);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_mode_and_the_size_come_back_in_the_order_they_went() {
    // The mode arrives as the first step of a drag and the resize steps
    // follow it. The two are two commands, so they are two entries — the
    // same shape a Shift-loop leaves — and undo walks them back in order:
    // the size first, then the mode.
    let dir = scratch("undo");
    let (mut session, clip) = with_a_take(&dir);
    session.arrange(ArrangeEdit::SetStretch {
        ids: vec![clip],
        stretch: ClipStretch::Resample,
    });
    for _ in 0..4 {
        session.arrange(ArrangeEdit::Resize {
            ids: vec![clip],
            tick_delta: PPQN / 4,
        });
    }
    session.end_gesture();
    let info = session.clips().into_iter().find(|c| c.id == clip).unwrap();
    assert_eq!(info.length, PPQN * 2);

    session.undo();
    let info = session.clips().into_iter().find(|c| c.id == clip).unwrap();
    assert_eq!(info.length, PPQN, "the whole drag's growth, in one step");
    assert_eq!(
        session.audio_clip(clip).unwrap().stretch,
        ClipStretch::Resample
    );

    session.undo();
    assert_eq!(session.audio_clip(clip).unwrap().stretch, ClipStretch::Off);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_clip_played_faster_runs_out_sooner_and_the_block_is_told_so() {
    // Pitch and speed are one number until the stretch engine lands
    // (`AudioClipData::rate`), and that number is *how fast the file is read*
    // — so a clip at double speed is over in half the ticks. The block draws
    // the file ending where it ends, so it has to be told the shorter length
    // or the waveform outlasts the sound by an octave's worth of block.
    let dir = scratch("speed");
    let (mut session, clip) = with_a_take(&dir);
    assert_eq!(
        session
            .clips()
            .into_iter()
            .find(|c| c.id == clip)
            .unwrap()
            .audio
            .natural_length,
        PPQN
    );

    let mut data = session.audio_clip(clip).unwrap();
    data.speed = 2.0;
    session.set_audio_clip(clip, data);
    assert_eq!(
        session
            .clips()
            .into_iter()
            .find(|c| c.id == clip)
            .unwrap()
            .audio
            .natural_length,
        PPQN / 2,
        "twice the speed, half the time"
    );

    // And the other way: an octave down is half speed and twice the time.
    let mut data = session.audio_clip(clip).unwrap();
    data.speed = 1.0;
    data.pitch_semitones = -12.0;
    session.set_audio_clip(clip, data);
    assert_eq!(
        session
            .clips()
            .into_iter()
            .find(|c| c.id == clip)
            .unwrap()
            .audio
            .natural_length,
        PPQN * 2,
        "an octave down is half speed"
    );
    std::fs::remove_dir_all(&dir).ok();
}

// ------------------------------------------------- pitch, and the picture ---

/// A session holding a take whose front half is loud and whose back half is
/// silence, so *where the sound stops* can be read off the picture.
///
/// Half a second again, so the block is one beat at 120 and the arithmetic
/// above still applies.
fn with_a_half_loud_take(dir: &Path) -> (Session, fontelle_types::ClipId) {
    let samples: Vec<f32> = (0..24_000)
        .map(|i| {
            if i < 12_000 {
                (i as f32 * 220.0 * std::f32::consts::TAU / 48_000.0).sin() * 0.8
            } else {
                0.0
            }
        })
        .collect();
    let path = dir.join("half.wav");
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

/// How far along the picture the sound stops, 0..1 — the last bucket with
/// anything in it, over the number of buckets.
fn sound_ends_at(session: &mut Session, clip: fontelle_types::ClipId) -> f32 {
    let info = session.clips().into_iter().find(|c| c.id == clip).unwrap();
    let peaks = &info.audio.peaks;
    assert!(!peaks.is_empty(), "the block has no waveform in it");
    let last = peaks
        .iter()
        .rposition(|(lo, hi)| hi - lo > 0.05)
        .expect("a take that is loud somewhere");
    (last + 1) as f32 / peaks.len() as f32
}

#[test]
fn repitching_an_unstretched_clip_repitches_the_picture_rather_than_stretching_it() {
    // Reported from using the window: *"when stretch is off on an audio clip,
    // when i change the pitch it still is visually stretching the clip in the
    // arrangement ... but it shouldnt because it shouldnt stretch just
    // repitch in that scenario."*
    //
    // The block already gets shorter — `natural_length` divides by the rate,
    // because varispeed is what pitch *is* until the stretch engine lands
    // (TDD §3.3). The picture inside it must then still be the **whole
    // file**, drawn across whatever span that is. It was the file scaled by
    // the rate a second time: the buckets were addressed in file frames and
    // handed to `source_position`, which multiplies by the rate — so an
    // octave up drew the first half of the file over the whole strip and
    // smeared its last bucket across the rest.
    let dir = scratch("repitch");
    let (mut session, clip) = with_a_half_loud_take(&dir);
    let at_rest = sound_ends_at(&mut session, clip);
    assert!(
        (at_rest - 0.5).abs() < 0.05,
        "the file is loud for its front half, so the picture should be too — got {at_rest}"
    );

    let mut data = session.audio_clip(clip).unwrap();
    assert_eq!(data.stretch, ClipStretch::Off);
    data.pitch_semitones = 12.0;
    session.set_audio_clip(clip, data);

    let pitched = sound_ends_at(&mut session, clip);
    assert!(
        (pitched - at_rest).abs() < 0.05,
        "an octave up is the same file, drawn in less room — the sound still \
         stops half way along the picture, not at {pitched}"
    );

    // And the block itself is half as long, because that is what varispeed
    // does to a file: the picture is repitched, the block is what moves.
    let info = session.clips().into_iter().find(|c| c.id == clip).unwrap();
    assert_eq!(info.audio.natural_length, PPQN / 2);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_stretched_clip_draws_its_pitch_because_the_block_is_the_constant() {
    // The other mode, and the reason this is not one rule for both: with
    // `Resample` the block's length is fixed and the file is read to fill it,
    // so pitch *is* an offset from that — an octave up gets through the file
    // in half the block and the picture has to say so.
    let dir = scratch("repitch-stretched");
    let (mut session, clip) = with_a_half_loud_take(&dir);
    session.arrange(ArrangeEdit::SetStretch {
        ids: vec![clip],
        stretch: ClipStretch::Resample,
    });
    let at_rest = sound_ends_at(&mut session, clip);
    assert!((at_rest - 0.5).abs() < 0.05, "got {at_rest}");

    let mut data = session.audio_clip(clip).unwrap();
    data.pitch_semitones = 12.0;
    session.set_audio_clip(clip, data);
    let pitched = sound_ends_at(&mut session, clip);
    assert!(
        (pitched - 0.25).abs() < 0.05,
        "an octave up over a fixed block is through the file in half of it, \
         so the sound stops a quarter of the way along — got {pitched}"
    );
    std::fs::remove_dir_all(&dir).ok();
}
