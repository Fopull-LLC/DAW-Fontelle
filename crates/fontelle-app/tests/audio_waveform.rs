//! What the waveform on a block shows, after the edits people actually make.
//!
//! > *"shortening an audio clip and then looping it and then unlooping it and
//! > then trying to elongate it again, is making the audio show completely
//! > blank after that even though it actually does have content and is playing
//! > audio."*
//!
//! Two things have to agree for a block to be right, and both are measured
//! here rather than by eye: the **preview** the session builds from the peak
//! summary, and the **columns** the canvas turns it into. A picture that is
//! blank where the player is loud is worse than no picture, because somebody
//! trims against it.

mod common;

use std::path::{Path, PathBuf};

use fontelle_app::settings::Settings;
use fontelle_app::{RealiseOptions, SampleLibrary, Session};
use fontelle_engine::{graph_channel, timeline_channel};
use fontelle_types::CompiledTimeline;
use fontelle_ui::canvas::{TimelineView, clip_rect, clip_waveform, timeline_layout};
use fontelle_ui::document::{ClipKind, StudioHost};
use fontelle_ui::layout::Rect;
use fontelle_ui::theme::Theme;

use common::SR;

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "fontelle-waveform-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::remove_dir_all(&path).ok();
    std::fs::create_dir_all(&path).expect("the scratch folder must be creatable");
    path
}

fn a_session(dir: &Path) -> Session {
    let settings = Settings {
        audio_dir: Some(dir.to_path_buf()),
        ..Default::default()
    };
    std::fs::write(dir.join("settings.json"), settings.to_json()).unwrap();
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
    let realised = fontelle_app::realise(&project, &library, options).expect("it must realise");
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

/// Four seconds of a loud tone — long enough that a bar of it is a fraction of
/// the file, which is what the shorten-and-grow sequence needs.
fn a_take(dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    let frames = (SR as usize) * 4;
    let mut bytes = Vec::with_capacity(44 + frames * 2);
    let data = frames as u32 * 2;
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&(SR).to_le_bytes());
    bytes.extend_from_slice(&(SR * 2).to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data.to_le_bytes());
    for i in 0..frames {
        let t = i as f32 / SR as f32;
        let v = ((t * 220.0 * std::f32::consts::TAU).sin() * 0.9 * i16::MAX as f32) as i16;
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    std::fs::write(&path, bytes).expect("the take must be writable");
    path
}

/// How many columns of the drawn waveform have any height to them.
fn drawn_columns(session: &Session) -> usize {
    let clips = session.clips();
    let clip = clips
        .iter()
        .find(|c| c.kind == ClipKind::Audio)
        .expect("an audio clip");
    let l = timeline_layout(
        Rect::new(0.0, 0.0, 1200.0, 400.0),
        &Theme::dark_default().metrics,
    );
    // Zoomed so the whole clip is on screen whatever it has been dragged to.
    let view = TimelineView {
        pixels_per_tick: 900.0 / (clip.start + clip.length).max(1) as f32,
        ..TimelineView::default()
    };
    let block = clip_rect(&view, l.grid, clip);
    clip_waveform(block, l.grid, clip)
        .iter()
        .filter(|column| column.height > 1.5)
        .count()
}

fn the_audio_clip(session: &Session) -> fontelle_types::ClipId {
    session
        .project()
        .clips
        .iter()
        .find(|(_, clip)| matches!(clip.source, fontelle_model::ClipSource::Audio(_)))
        .map(|(id, _)| id)
        .expect("an audio clip")
}

/// **The reported sequence, exactly.** Shorten, loop, unloop, grow — and the
/// block still shows the sound that is still playing.
#[test]
fn a_clip_shortened_looped_unlooped_and_grown_still_draws_its_sound() {
    let dir = scratch("cycle");
    let path = a_take(&dir, "Take.wav");
    let mut session = a_session(&dir);
    session.drop_file(&path).expect("imports");
    let id = the_audio_clip(&session);
    let full = session.project().clips[id].length;
    assert!(
        drawn_columns(&session) > 20,
        "it did not draw when it landed"
    );

    // Shorten to a quarter of the take.
    session.arrange(fontelle_ui::canvas::ArrangeEdit::Resize {
        ids: vec![id],
        tick_delta: -(full - full / 4),
    });
    assert!(drawn_columns(&session) > 20, "a shortened clip went blank");

    // Loop it, then take the loop off again.
    let period = session.project().clips[id].length;
    session.arrange(fontelle_ui::canvas::ArrangeEdit::SetLoop {
        ids: vec![id],
        loop_length: Some(period),
    });
    assert!(drawn_columns(&session) > 20, "a looped clip went blank");
    session.arrange(fontelle_ui::canvas::ArrangeEdit::SetLoop {
        ids: vec![id],
        loop_length: None,
    });
    assert!(drawn_columns(&session) > 20, "an unlooped clip went blank");

    // And grow it back out.
    session.arrange(fontelle_ui::canvas::ArrangeEdit::Resize {
        ids: vec![id],
        tick_delta: full,
    });
    let after = drawn_columns(&session);
    assert!(
        after > 20,
        "the block is blank after shorten/loop/unloop/grow — it drew {after} columns"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// A clip that has been cut draws **its own** part of the take, starting at
/// its own left edge.
///
/// > *"when I cut audio clips that the waveform will appear offset."*
#[test]
fn each_half_of_a_cut_draws_its_own_half_of_the_take() {
    let dir = scratch("cut");
    let path = a_take(&dir, "Take.wav");
    let mut session = a_session(&dir);
    session.drop_file(&path).expect("imports");
    let id = the_audio_clip(&session);
    let start = session.project().clips[id].start;
    let length = session.project().clips[id].length;

    session.arrange(fontelle_ui::canvas::ArrangeEdit::Split {
        cuts: vec![(id, start + length / 2)],
    });
    let clips = session.clips();
    let halves: Vec<_> = clips.iter().filter(|c| c.kind == ClipKind::Audio).collect();
    assert_eq!(halves.len(), 2, "the cut made two clips");
    for half in halves {
        assert!(
            !half.audio.peaks.is_empty(),
            "a half of the cut has no waveform"
        );
        // Each half covers its own frames, so its peaks are its own: a half
        // whose picture began at the file's start would be drawing the other
        // half's sound.
        assert!(
            half.audio.natural_length > 0,
            "a half of the cut says its take is no length at all"
        );
    }
    std::fs::remove_dir_all(&dir).ok();
}

/// **The sequence with the stretch switch in it**, which is what an edge drag
/// really sends: `SetStretch` first, then `Resize`.
///
/// `with_stretch(Off)` works the file's speed out from the pass it is filling
/// *at that moment* — so a clip shortened while stretching is written down as
/// playing four times too fast, and every later drag is measured against that.
/// The picture then covers a quarter of the block and the rest is blank.
#[test]
fn the_stretch_switch_does_not_leave_the_block_blank_when_it_is_grown_back() {
    let dir = scratch("stretch-cycle");
    let path = a_take(&dir, "Take.wav");
    let mut session = a_session(&dir);
    session.drop_file(&path).expect("imports");
    let id = the_audio_clip(&session);
    let full = session.project().clips[id].length;

    // Shorten it *while stretching*, which is what the Stretch switch does.
    session.arrange(fontelle_ui::canvas::ArrangeEdit::SetStretch {
        ids: vec![id],
        stretch: fontelle_types::ClipStretch::Resample,
    });
    session.arrange(fontelle_ui::canvas::ArrangeEdit::Resize {
        ids: vec![id],
        tick_delta: -(full - full / 4),
    });
    // Loop, unloop — each of which changes what "the pass" means.
    let period = session.project().clips[id].length;
    session.arrange(fontelle_ui::canvas::ArrangeEdit::SetLoop {
        ids: vec![id],
        loop_length: Some(period),
    });
    session.arrange(fontelle_ui::canvas::ArrangeEdit::SetLoop {
        ids: vec![id],
        loop_length: None,
    });
    // Now take stretching off and grow it, which is the ordinary drag.
    session.arrange(fontelle_ui::canvas::ArrangeEdit::SetStretch {
        ids: vec![id],
        stretch: fontelle_types::ClipStretch::Off,
    });
    session.arrange(fontelle_ui::canvas::ArrangeEdit::Resize {
        ids: vec![id],
        tick_delta: full,
    });

    let after = drawn_columns(&session);
    let clips = session.clips();
    let clip = clips.iter().find(|c| c.kind == ClipKind::Audio).unwrap();
    assert!(
        after > 20,
        "the block drew {after} columns: natural {} against a block of {}",
        clip.audio.natural_length,
        clip.length
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// A **diagnostic**, not a claim: what the clip's numbers actually are at each
/// step of the reported sequence.
///
/// Kept because the fault has not been reproduced and these are the six
/// numbers that decide whether a block draws — `length`, the loop period, the
/// stretch mode, the speed, `natural_length` and how many columns come out.
/// When the sequence is pinned down, this is where to look first.
#[test]
fn what_the_numbers_do_through_the_reported_sequence() {
    let dir = scratch("diagnostic");
    let path = a_take(&dir, "Take.wav");
    let mut session = a_session(&dir);
    session.drop_file(&path).expect("imports");
    let id = the_audio_clip(&session);
    let full = session.project().clips[id].length;

    let report = |session: &Session, step: &str| {
        let clip = &session.project().clips[id];
        let fontelle_model::ClipSource::Audio(data) = &clip.source else {
            return;
        };
        let infos = session.clips();
        let info = infos.iter().find(|c| c.kind == ClipKind::Audio).unwrap();
        println!(
            "{step:<22} length {:>6}  loop {:>7}  {:?}  speed {:.3}  natural {:>6}  columns {}",
            clip.length,
            format!("{:?}", clip.loop_length),
            data.stretch,
            data.speed,
            info.audio.natural_length,
            drawn_columns(session),
        );
    };
    report(&session, "imported");
    session.arrange(fontelle_ui::canvas::ArrangeEdit::SetStretch {
        ids: vec![id],
        stretch: fontelle_types::ClipStretch::Resample,
    });
    session.arrange(fontelle_ui::canvas::ArrangeEdit::Resize {
        ids: vec![id],
        tick_delta: -(full - full / 4),
    });
    report(&session, "shortened (stretch)");
    let period = session.project().clips[id].length;
    session.arrange(fontelle_ui::canvas::ArrangeEdit::SetLoop {
        ids: vec![id],
        loop_length: Some(period),
    });
    report(&session, "looped");
    session.arrange(fontelle_ui::canvas::ArrangeEdit::SetLoop {
        ids: vec![id],
        loop_length: None,
    });
    report(&session, "unlooped");
    session.arrange(fontelle_ui::canvas::ArrangeEdit::SetStretch {
        ids: vec![id],
        stretch: fontelle_types::ClipStretch::Off,
    });
    report(&session, "stretch off");
    session.arrange(fontelle_ui::canvas::ArrangeEdit::Resize {
        ids: vec![id],
        tick_delta: full,
    });
    report(&session, "grown back");
    std::fs::remove_dir_all(&dir).ok();
}

/// **A block longer than its take says so.**
///
/// The diagnostic above is what found this: shortening with Stretch on and
/// then switching it off writes the compression into the clip's `speed`, so
/// growing the block back leaves the file covering a fifth of it. The
/// arithmetic is right — the take really is over — and the picture said
/// nothing at all about why, which is what reads as "completely blank".
///
/// `content_end` is where the take stops inside the block, and it is `None`
/// when there is nothing to say: a block the file fills, and a looping block,
/// whose gaps are a rhythm rather than an ending.
#[test]
fn a_block_longer_than_its_take_marks_where_the_take_ends() {
    let dir = scratch("ends");
    let path = a_take(&dir, "Take.wav");
    let mut session = a_session(&dir);
    session.drop_file(&path).expect("imports");
    let id = the_audio_clip(&session);
    let full = session.project().clips[id].length;

    let block_of = |session: &Session| {
        let clips = session.clips();
        let clip = clips
            .iter()
            .find(|c| c.kind == ClipKind::Audio)
            .expect("an audio clip")
            .clone();
        let l = timeline_layout(
            Rect::new(0.0, 0.0, 1200.0, 400.0),
            &Theme::dark_default().metrics,
        );
        let view = TimelineView {
            pixels_per_tick: 900.0 / (clip.start + clip.length).max(1) as f32,
            ..TimelineView::default()
        };
        (clip_rect(&view, l.grid, &clip), clip)
    };

    // As it lands, the file fills the block: there is no end to mark.
    let (block, clip) = block_of(&session);
    assert_eq!(
        fontelle_ui::canvas::content_end(block, &clip),
        None,
        "a block its file fills marked an ending"
    );

    // Grow it to twice its length and the take now stops halfway.
    session.arrange(fontelle_ui::canvas::ArrangeEdit::Resize {
        ids: vec![id],
        tick_delta: full,
    });
    let (block, clip) = block_of(&session);
    let at = fontelle_ui::canvas::content_end(block, &clip)
        .expect("a block twice its take's length has an ending to mark");
    let middle = block.x + block.width / 2.0;
    assert!(
        (at - middle).abs() < 2.0,
        "the take ends at {at} and the block's middle is {middle}"
    );

    // A looping block has no ending: its gaps are a rhythm.
    session.arrange(fontelle_ui::canvas::ArrangeEdit::SetLoop {
        ids: vec![id],
        loop_length: Some(full),
    });
    let (block, clip) = block_of(&session);
    assert_eq!(
        fontelle_ui::canvas::content_end(block, &clip),
        None,
        "a looping block marked an ending it does not have"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// **A trim is never lossy.** Stretch a clip shorter, drag it back out, and
/// the whole take is there again.
///
/// This is the invariant behind both reports about this corner. What broke it
/// was that an ordinary edge drag *silently* converted a stretched clip to
/// "off", which freezes the rate it was being played at into the clip's own
/// `speed` — after which the file really is only a fifth as long and no drag
/// can bring the rest back.
///
/// A drag no longer changes a clip's mode away from stretching. Turning
/// stretching off is the deliberate act it reads as: the switch does it, and
/// doing it deliberately still freezes the sound where it is.
#[test]
fn a_stretched_clip_dragged_short_and_long_again_gets_its_whole_take_back() {
    let dir = scratch("reversible");
    let path = a_take(&dir, "Take.wav");
    let mut session = a_session(&dir);
    session.drop_file(&path).expect("imports");
    let id = the_audio_clip(&session);
    let full = session.project().clips[id].length;

    // Stretching on, and shorten it to a quarter: it now plays four times as
    // fast, which is what stretching a block shorter means.
    session.arrange(fontelle_ui::canvas::ArrangeEdit::SetStretch {
        ids: vec![id],
        stretch: fontelle_types::ClipStretch::Resample,
    });
    session.arrange(fontelle_ui::canvas::ArrangeEdit::Resize {
        ids: vec![id],
        tick_delta: -(full - full / 4),
    });
    // Loop and unloop, which is the reported sequence.
    let period = session.project().clips[id].length;
    session.arrange(fontelle_ui::canvas::ArrangeEdit::SetLoop {
        ids: vec![id],
        loop_length: Some(period),
    });
    session.arrange(fontelle_ui::canvas::ArrangeEdit::SetLoop {
        ids: vec![id],
        loop_length: None,
    });
    // And drag it back out to where it started.
    session.arrange(fontelle_ui::canvas::ArrangeEdit::Resize {
        ids: vec![id],
        tick_delta: full - full / 4,
    });

    let clips = session.clips();
    let clip = clips
        .iter()
        .find(|c| c.kind == ClipKind::Audio)
        .expect("an audio clip");
    assert_eq!(clip.length, full, "it is back to the length it started at");
    assert!(
        clip.audio.stretched,
        "the drag turned stretching off behind the user's back"
    );
    // The whole block draws, because the file fills it again.
    let covered = drawn_columns(&session);
    assert!(
        covered > 700,
        "only {covered} columns of the block drew - the take did not come back"
    );
    std::fs::remove_dir_all(&dir).ok();
}

// ------------------------------------------------------------ resolution ---
//
// > *"audio shown in clips doesnt seem to actually be matching when
// > visualised to me its showing going up when theres not actual volume
// > there like it looks like i start talking sooner than i actually do
// > audibly."*
//
// The preview used to be a fixed 512 buckets across the clip whatever its
// length, so a long take's bucket was the loudest sample in a good part of a
// second — and a bucket that held the first word was drawn loud from its own
// start, the better part of a second before the voice. The picture has to
// be built fine enough that a sound starts where it starts.

/// `seconds` of a take shaped by `sample(t)`, as a 16-bit mono WAV.
fn a_shaped_take(dir: &Path, name: &str, seconds: f32, sample: impl Fn(f32) -> f32) -> PathBuf {
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
    bytes.extend_from_slice(&(SR).to_le_bytes());
    bytes.extend_from_slice(&(SR * 2).to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data.to_le_bytes());
    for i in 0..frames {
        let t = i as f32 / SR as f32;
        let v = (sample(t).clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    std::fs::write(&path, bytes).expect("the take must be writable");
    path
}

fn the_audio_preview(session: &Session) -> fontelle_ui::document::AudioPreview {
    session
        .clips()
        .into_iter()
        .find(|c| c.kind == ClipKind::Audio)
        .expect("an audio clip")
        .audio
}

/// **The report.** Two minutes of take, silent until 30.1 s and then a
/// voice: the picture's first loud bucket has to be within ten milliseconds
/// of where the voice is, not the better part of a second before it.
#[test]
fn a_long_takes_picture_starts_where_the_sound_does() {
    let dir = scratch("onset");
    const ONSET: f32 = 30.1;
    let path = a_shaped_take(&dir, "Take.wav", 120.0, |t| {
        if t < ONSET {
            0.0
        } else {
            (t * 220.0 * std::f32::consts::TAU).sin() * 0.8
        }
    });
    let mut session = a_session(&dir);
    session.drop_file(&path).expect("imports");
    let preview = the_audio_preview(&session);
    let peaks = &preview.peaks;
    let first_loud = peaks
        .iter()
        .position(|(lo, hi)| hi.abs() > 0.3 || lo.abs() > 0.3)
        .expect("the voice is in the picture");
    let drawn_onset = first_loud as f32 / peaks.len() as f32 * 120.0;
    assert!(
        (drawn_onset - ONSET).abs() < 0.010,
        "the picture goes loud at {drawn_onset:.3} s; the voice starts at {ONSET} s \
         ({} buckets over the take)",
        peaks.len()
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// The picture is **shared, not rebuilt**, by every edit that does not
/// change what it shows: a fine picture of a long take costs real time to
/// fold, and a clip dragged along the arrangement is redrawn every pointer
/// move. Moving the clip keeps the very same buckets; cutting it makes new
/// ones, because each half then shows something else.
#[test]
fn moving_a_clip_keeps_its_picture_and_cutting_it_makes_new_ones() {
    let dir = scratch("share");
    let path = a_shaped_take(&dir, "Take.wav", 20.0, |t| {
        (t * 220.0 * std::f32::consts::TAU).sin() * 0.8
    });
    let mut session = a_session(&dir);
    session.drop_file(&path).expect("imports");
    let id = the_audio_clip(&session);
    let before = the_audio_preview(&session);

    session.arrange(fontelle_ui::canvas::ArrangeEdit::Move {
        ids: vec![id],
        tick_delta: fontelle_types::PPQN * 4,
        lane_delta: 0,
    });
    let moved = the_audio_preview(&session);
    assert!(
        std::sync::Arc::ptr_eq(&before.peaks, &moved.peaks),
        "a move rebuilt the picture"
    );

    let clip = &session.project().clips[id];
    let cut = clip.start + clip.length / 2;
    session.arrange(fontelle_ui::canvas::ArrangeEdit::Split {
        cuts: vec![(id, cut)],
    });
    let halves: Vec<_> = session
        .clips()
        .into_iter()
        .filter(|c| c.kind == ClipKind::Audio)
        .collect();
    assert_eq!(halves.len(), 2, "the cut made two clips");
    for half in &halves {
        assert!(
            !std::sync::Arc::ptr_eq(&before.peaks, &half.audio.peaks),
            "a half of the cut kept the whole take's picture"
        );
    }
    std::fs::remove_dir_all(&dir).ok();
}

/// The picture carries the take's **loudness** beside its extremes — the
/// core the block draws inside the outline. A 0.8 tone reads 0.8 over root
/// two, and the same number of buckets as the outline has.
#[test]
fn the_picture_carries_the_takes_loudness_bucket_for_bucket() {
    let dir = scratch("loudness");
    let path = a_shaped_take(&dir, "Take.wav", 10.0, |t| {
        (t * 220.0 * std::f32::consts::TAU).sin() * 0.8
    });
    let mut session = a_session(&dir);
    session.drop_file(&path).expect("imports");
    let preview = the_audio_preview(&session);
    assert_eq!(preview.rms.len(), preview.peaks.len());
    let middle = preview.rms[preview.rms.len() / 2];
    assert!(
        (middle - 0.8 / 2f32.sqrt()).abs() < 0.05,
        "a 0.8 tone reads {middle} RMS in the picture"
    );
    let (lo, hi) = preview.peaks[preview.peaks.len() / 2];
    assert!(
        middle < hi && middle < -lo,
        "the core is not inside the outline"
    );
    std::fs::remove_dir_all(&dir).ok();
}
