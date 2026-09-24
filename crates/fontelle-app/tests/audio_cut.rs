//! Cutting and trimming a take: what you hear, measured.
//!
//! > *"when cutting up audio clips it actually moves the start of the audio
//! > clip to where i cut it so i literally cannot cut clips how id expect to
//! > right now ... if i edit my clips enough and dragging them in after
//! > cutting them it removes the content of the audio for the section after
//! > the cutoff if i try to expand it again its weird. please remove any odd
//! > interactions like this."*
//!
//! Every test here is one rule of what an edit to a take may do to the sound,
//! checked by rendering the song before and after and comparing the two:
//!
//! - **A cut changes nothing you hear.** Two halves side by side are the one
//!   clip they came from.
//! - **A trim is a window, not a deletion.** Drag an edge in and the sound
//!   past it goes; drag it back out and the sound comes back, however many
//!   cuts and drags came in between.
//! - **The picture agrees**: a block grown back out draws the sound it plays.

mod common;

use std::path::{Path, PathBuf};

use fontelle_app::settings::Settings;
use fontelle_app::{RealiseOptions, SampleLibrary, Session};
use fontelle_engine::{graph_channel, timeline_channel};
use fontelle_types::{ClipId, ClipStretch, CompiledTimeline, Tick};
use fontelle_ui::canvas::ArrangeEdit;
use fontelle_ui::document::{ClipKind, StudioHost};

use common::SR;

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "fontelle-cut-{name}-{}-{:?}",
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

/// Four seconds of a tone that **gets louder as it goes**, so a piece of it
/// played from the wrong place in the file is a different loudness and not
/// the same sine a period along.
fn a_take(dir: &Path) -> PathBuf {
    let path = dir.join("Take.wav");
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
        let level = 0.08 + 0.4 * t / 4.0;
        let v = ((t * 220.0 * std::f32::consts::TAU).sin() * level * i16::MAX as f32) as i16;
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    std::fs::write(&path, bytes).expect("the take must be writable");
    path
}

fn audio_clips(session: &Session) -> Vec<ClipId> {
    let mut ids: Vec<(Tick, ClipId)> = session
        .project()
        .clips
        .iter()
        .filter(|(_, clip)| matches!(clip.source, fontelle_model::ClipSource::Audio(_)))
        .map(|(id, clip)| (clip.start, id))
        .collect();
    ids.sort();
    ids.into_iter().map(|(_, id)| id).collect()
}

fn a_session_with_the_take(dir: &Path) -> (Session, ClipId) {
    let path = a_take(dir);
    let mut session = a_session(dir);
    session.drop_file(&path).expect("imports");
    let id = audio_clips(&session)[0];
    (session, id)
}

/// The song, rendered, left channel — from the start to `until` ticks.
fn render(session: &Session, until: Tick) -> Vec<f32> {
    let project = session.project();
    let options = RealiseOptions {
        sample_rate: SR,
        block_size: fontelle_engine::BLOCK_SIZE,
        quality: fontelle_app::PLAYBACK_QUALITY,
    };
    let mut realised =
        fontelle_app::realise(project, session.library(), options).expect("it must realise");
    let timeline = fontelle_sequencer::compile_with(
        project,
        &fontelle_sequencer::NodeMaps {
            channels: &realised.channel_nodes,
            params: &realised.param_nodes,
            audio: &realised.audio_nodes,
        },
        fontelle_sequencer::CompileScope::Song,
    );
    let frames = project.tempo_map.tick_to_sample(until);
    fontelle_app::render_offline(&timeline, &mut realised.graph, frames)
        .chunks(2)
        .map(|f| f[0])
        .collect()
}

fn frame_of(session: &Session, tick: Tick) -> usize {
    session.project().tempo_map.tick_to_sample(tick).max(0) as usize
}

/// How many frames of `a` and `b` differ audibly, leaving out a few
/// milliseconds either side of each of `seams` — an edge may be de-clicked,
/// and that is not what these tests are about.
fn differing(a: &[f32], b: &[f32], seams: &[usize]) -> usize {
    let guard = SR as usize / 250; // 4 ms
    a.iter()
        .zip(b)
        .enumerate()
        .filter(|(i, _)| seams.iter().all(|s| i.abs_diff(*s) > guard))
        .filter(|(_, (x, y))| (*x - *y).abs() > 0.01)
        .count()
}

fn loud(frames: &[f32]) -> bool {
    frames.iter().any(|v| v.abs() > 0.05)
}

fn end_of(session: &Session, id: ClipId) -> Tick {
    let clip = &session.project().clips[id];
    clip.start + clip.length
}

// ---------------------------------------------------------------- the cut ---

#[test]
fn a_cut_changes_nothing_you_hear() {
    let dir = scratch("nothing");
    let (mut session, id) = a_session_with_the_take(&dir);
    let start = session.project().clips[id].start;
    let end = end_of(&session, id);
    let before = render(&session, end + fontelle_types::PPQN);

    // Off the grid on purpose: a blade lands wherever the snap is set to.
    let at = start + (end - start) * 3 / 7;
    session.arrange(ArrangeEdit::Split {
        cuts: vec![(id, at)],
    });
    assert_eq!(audio_clips(&session).len(), 2, "the cut made two clips");
    let after = render(&session, end + fontelle_types::PPQN);

    let seam = frame_of(&session, at);
    let wrong = differing(&before, &after, &[seam]);
    assert_eq!(wrong, 0, "{wrong} frames changed when the clip was cut");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_cut_changes_nothing_you_hear_with_stretch_on() {
    let dir = scratch("stretched");
    let (mut session, id) = a_session_with_the_take(&dir);
    session.arrange(ArrangeEdit::SetStretch {
        ids: vec![id],
        stretch: ClipStretch::Resample,
    });
    let start = session.project().clips[id].start;
    let end = end_of(&session, id);
    let before = render(&session, end + fontelle_types::PPQN);
    let at = start + (end - start) * 3 / 7;
    session.arrange(ArrangeEdit::Split {
        cuts: vec![(id, at)],
    });
    let after = render(&session, end + fontelle_types::PPQN);
    let wrong = differing(&before, &after, &[frame_of(&session, at)]);
    assert_eq!(
        wrong, 0,
        "{wrong} frames changed when a stretched clip was cut"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn cutting_twice_and_deleting_the_middle_leaves_the_outsides_where_they_were() {
    let dir = scratch("middle");
    let (mut session, id) = a_session_with_the_take(&dir);
    let start = session.project().clips[id].start;
    let end = end_of(&session, id);
    let before = render(&session, end + fontelle_types::PPQN);
    let (a, b) = (start + (end - start) / 4, start + (end - start) * 3 / 4);
    session.arrange(ArrangeEdit::Split {
        cuts: vec![(id, a)],
    });
    let right = *audio_clips(&session).last().unwrap();
    session.arrange(ArrangeEdit::Split {
        cuts: vec![(right, b)],
    });
    let middle = audio_clips(&session)[1];
    session.arrange(ArrangeEdit::Remove(vec![middle]));
    let after = render(&session, end + fontelle_types::PPQN);

    let (fa, fb) = (frame_of(&session, a), frame_of(&session, b));
    let outside: Vec<usize> = (0..before.len()).filter(|i| *i < fa || *i >= fb).collect();
    let wrong = outside
        .iter()
        .filter(|i| i.abs_diff(fa) > 200 && i.abs_diff(fb) > 200)
        .filter(|i| (before[**i] - after[**i]).abs() > 0.01)
        .count();
    assert_eq!(
        wrong, 0,
        "{wrong} frames outside the removed middle changed"
    );
    assert!(
        !loud(&after[fa + 200..fb - 200]),
        "the removed middle still sounds"
    );
    std::fs::remove_dir_all(&dir).ok();
}

// ------------------------------------------------------ growing back out ---

#[test]
fn a_right_edge_dragged_in_and_back_out_gives_the_sound_back() {
    let dir = scratch("regrow");
    let (mut session, id) = a_session_with_the_take(&dir);
    let end = end_of(&session, id);
    let length = session.project().clips[id].length;
    let before = render(&session, end + fontelle_types::PPQN);

    session.arrange(ArrangeEdit::Resize {
        ids: vec![id],
        tick_delta: -length / 2,
    });
    let short = render(&session, end + fontelle_types::PPQN);
    let edge = frame_of(&session, end_of(&session, id));
    assert!(
        !loud(&short[edge + 400..]),
        "the sound past a trimmed edge still plays"
    );

    session.arrange(ArrangeEdit::Resize {
        ids: vec![id],
        tick_delta: length / 2,
    });
    let after = render(&session, end + fontelle_types::PPQN);
    let wrong = differing(&before, &after, &[edge]);
    assert_eq!(
        wrong, 0,
        "{wrong} frames did not come back when it was grown"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_front_half_of_a_cut_grown_back_out_plays_what_was_after_the_cut() {
    // The report: cut, take the tail away, drag the head back out — and the
    // part after the cut is gone.
    let dir = scratch("head");
    let (mut session, id) = a_session_with_the_take(&dir);
    let start = session.project().clips[id].start;
    let end = end_of(&session, id);
    let before = render(&session, end + fontelle_types::PPQN);
    let at = start + (end - start) / 3;
    session.arrange(ArrangeEdit::Split {
        cuts: vec![(id, at)],
    });
    let tail = *audio_clips(&session).last().unwrap();
    session.arrange(ArrangeEdit::Remove(vec![tail]));
    session.arrange(ArrangeEdit::Resize {
        ids: vec![id],
        tick_delta: end - at,
    });
    let after = render(&session, end + fontelle_types::PPQN);
    let wrong = differing(&before, &after, &[frame_of(&session, at)]);
    assert_eq!(wrong, 0, "{wrong} frames of the take did not come back");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_trimmed_edge_fades_where_the_block_ends_not_where_the_take_did() {
    // A fade-out belongs to the block's right edge. Trimmed in, the fade has
    // to move in with it — otherwise it fades somewhere past the block, where
    // nothing plays, and the edge is a hard cut.
    let dir = scratch("fade");
    let (mut session, id) = a_session_with_the_take(&dir);
    let length = session.project().clips[id].length;
    session.arrange(ArrangeEdit::SetFade {
        clip: id,
        end: fontelle_ui::canvas::FadeEnd::Out,
        // An eighth of a four-second take: half a second.
        fraction: 0.125,
    });
    session.arrange(ArrangeEdit::Resize {
        ids: vec![id],
        tick_delta: -length / 2,
    });
    let end = end_of(&session, id);
    let heard = render(&session, end + fontelle_types::PPQN);
    let edge = frame_of(&session, end);
    let peak = |r: std::ops::Range<usize>| heard[r].iter().fold(0.0f32, |m, v| m.max(v.abs()));
    let early = peak(edge - SR as usize..edge - SR as usize * 3 / 4);
    let last = peak(edge - SR as usize / 40..edge);
    assert!(
        last < early * 0.25,
        "no fade at the trimmed edge: {last} at the edge against {early} before the fade"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_grown_block_draws_the_sound_it_plays() {
    let dir = scratch("picture");
    let (mut session, id) = a_session_with_the_take(&dir);
    let start = session.project().clips[id].start;
    let end = end_of(&session, id);
    let at = start + (end - start) / 3;
    session.arrange(ArrangeEdit::Split {
        cuts: vec![(id, at)],
    });
    let tail = *audio_clips(&session).last().unwrap();
    session.arrange(ArrangeEdit::Remove(vec![tail]));
    session.arrange(ArrangeEdit::Resize {
        ids: vec![id],
        tick_delta: end - at,
    });
    let clips = session.clips();
    let clip = clips
        .iter()
        .find(|c| c.kind == ClipKind::Audio)
        .expect("the clip");
    let natural = clip.audio.natural_length;
    assert!(
        natural >= clip.length - fontelle_types::PPQN / 8,
        "the picture covers {natural} ticks of a {}-tick block that is all sound",
        clip.length
    );
    std::fs::remove_dir_all(&dir).ok();
}

// ------------------------------------------------------- a looping clip ---
//
// A clip made to repeat (Shift on the grip) was cut "in the arrangement":
// both halves kept the whole take, so the right half **started the take over
// at the blade** — the report's *"it actually moves the start of the audio
// clip to where i cut it"*. A cut through a pass has to keep the pass's phase.

fn a_looping_take(dir: &Path) -> (Session, ClipId) {
    let (mut session, id) = a_session_with_the_take(dir);
    // One bar of the take, repeated to four.
    let bar = fontelle_types::PPQN * 4;
    let length = session.project().clips[id].length;
    session.arrange(ArrangeEdit::Resize {
        ids: vec![id],
        tick_delta: bar - length,
    });
    session.arrange(ArrangeEdit::SetLoop {
        ids: vec![id],
        loop_length: Some(bar),
    });
    session.arrange(ArrangeEdit::Resize {
        ids: vec![id],
        tick_delta: bar * 3,
    });
    (session, id)
}

#[test]
fn a_cut_through_a_looping_clip_mid_pass_changes_nothing_you_hear() {
    let dir = scratch("loopcut");
    let (mut session, id) = a_looping_take(&dir);
    let start = session.project().clips[id].start;
    let end = end_of(&session, id);
    let before = render(&session, end + fontelle_types::PPQN);
    // Two passes and a bit in: the middle of a pass, not a seam.
    let at = start + fontelle_types::PPQN * 4 * 2 + fontelle_types::PPQN * 3 / 2;
    session.arrange(ArrangeEdit::Split {
        cuts: vec![(id, at)],
    });
    let after = render(&session, end + fontelle_types::PPQN);
    let wrong = differing(&before, &after, &[frame_of(&session, at)]);
    assert_eq!(
        wrong, 0,
        "{wrong} frames changed when a looping clip was cut mid-pass"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_right_half_of_a_looping_cut_draws_from_where_the_pass_was() {
    let dir = scratch("loopdraw");
    let (mut session, id) = a_looping_take(&dir);
    let start = session.project().clips[id].start;
    let at = start + fontelle_types::PPQN * 4 * 2 + fontelle_types::PPQN * 3 / 2;
    session.arrange(ArrangeEdit::Split {
        cuts: vec![(id, at)],
    });
    let clips = session.clips();
    let tail = clips
        .iter()
        .filter(|c| c.kind == ClipKind::Audio)
        .max_by_key(|c| c.start)
        .unwrap();
    // A tick into the right half is a tick and a half beats into its pass.
    let fraction = fontelle_ui::canvas::content_fraction(tail, 0.0).expect("sound there");
    let expected = fontelle_ui::canvas::content_fraction(
        clips.iter().find(|c| c.id == id).unwrap(),
        (fontelle_types::PPQN * 3 / 2) as f32,
    )
    .expect("and there");
    assert!(
        (fraction - expected).abs() < 0.01,
        "the right half draws from {fraction} of its take, not from {expected}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

// ------------------------------------------------------- the left edge ---
//
// > *"i cant even drag in clips from the left too."*

#[test]
fn a_left_edge_dragged_in_silences_only_what_it_uncovers() {
    let dir = scratch("left");
    let (mut session, id) = a_session_with_the_take(&dir);
    let start = session.project().clips[id].start;
    let end = end_of(&session, id);
    let before = render(&session, end + fontelle_types::PPQN);
    let d = (end - start) / 3;
    session.arrange(ArrangeEdit::TrimStart {
        ids: vec![id],
        tick_delta: d,
    });
    assert_eq!(
        session.project().clips[id].start,
        start + d,
        "the block's front moved"
    );
    assert_eq!(end_of(&session, id), end, "the block's end stayed put");
    let after = render(&session, end + fontelle_types::PPQN);
    let edge = frame_of(&session, start + d);
    assert!(
        !loud(&after[frame_of(&session, start)..edge - 200]),
        "what the left edge uncovered still sounds"
    );
    let wrong = (edge + 200..after.len())
        .filter(|i| (before[*i] - after[*i]).abs() > 0.01)
        .count();
    assert_eq!(
        wrong, 0,
        "{wrong} frames after the trimmed edge moved or changed"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_left_edge_dragged_in_and_back_out_gives_the_sound_back() {
    let dir = scratch("leftback");
    let (mut session, id) = a_session_with_the_take(&dir);
    let start = session.project().clips[id].start;
    let end = end_of(&session, id);
    let before = render(&session, end + fontelle_types::PPQN);
    let d = (end - start) / 3;
    session.arrange(ArrangeEdit::TrimStart {
        ids: vec![id],
        tick_delta: d,
    });
    session.arrange(ArrangeEdit::TrimStart {
        ids: vec![id],
        tick_delta: -d,
    });
    let after = render(&session, end + fontelle_types::PPQN);
    let wrong = differing(&before, &after, &[frame_of(&session, start + d)]);
    assert_eq!(wrong, 0, "{wrong} frames did not come back");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_back_half_of_a_cut_dragged_out_to_the_left_plays_what_was_before_the_cut() {
    let dir = scratch("tailleft");
    let (mut session, id) = a_session_with_the_take(&dir);
    let start = session.project().clips[id].start;
    let end = end_of(&session, id);
    let before = render(&session, end + fontelle_types::PPQN);
    let at = start + (end - start) / 2;
    session.arrange(ArrangeEdit::Split {
        cuts: vec![(id, at)],
    });
    let tail = *audio_clips(&session).last().unwrap();
    session.arrange(ArrangeEdit::Remove(vec![id]));
    session.arrange(ArrangeEdit::TrimStart {
        ids: vec![tail],
        tick_delta: -(at - start),
    });
    assert_eq!(session.project().clips[tail].start, start);
    let after = render(&session, end + fontelle_types::PPQN);
    let wrong = differing(&before, &after, &[frame_of(&session, at)]);
    assert_eq!(wrong, 0, "{wrong} frames of the front did not come back");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_left_edge_stops_at_the_start_of_the_take() {
    // There is nothing before the file's first frame to show: dragged further,
    // the edge stops there rather than making a block of silence whose sound
    // then plays late.
    let dir = scratch("leftstop");
    let (mut session, id) = a_session_with_the_take(&dir);
    session.arrange(ArrangeEdit::Move {
        ids: vec![id],
        tick_delta: fontelle_types::PPQN * 8,
        lane_delta: 0,
    });
    let start = session.project().clips[id].start;
    let end = end_of(&session, id);
    let before = render(&session, end + fontelle_types::PPQN);
    session.arrange(ArrangeEdit::TrimStart {
        ids: vec![id],
        tick_delta: -fontelle_types::PPQN * 4,
    });
    assert_eq!(
        session.project().clips[id].start,
        start,
        "the edge went past the start of the take"
    );
    let after = render(&session, end + fontelle_types::PPQN);
    assert_eq!(differing(&before, &after, &[]), 0);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_left_edge_trim_on_a_looping_clip_keeps_the_rest_in_place() {
    let dir = scratch("leftloop");
    let (mut session, id) = a_looping_take(&dir);
    let start = session.project().clips[id].start;
    let end = end_of(&session, id);
    let before = render(&session, end + fontelle_types::PPQN);
    let d = fontelle_types::PPQN * 5;
    session.arrange(ArrangeEdit::TrimStart {
        ids: vec![id],
        tick_delta: d,
    });
    let after = render(&session, end + fontelle_types::PPQN);
    let edge = frame_of(&session, start + d);
    let wrong = (edge + 200..after.len())
        .filter(|i| (before[*i] - after[*i]).abs() > 0.01)
        .count();
    assert_eq!(wrong, 0, "{wrong} frames after the trimmed edge changed");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_right_edge_grown_past_the_end_of_the_take_adds_only_silence() {
    let dir = scratch("pastend");
    let (mut session, id) = a_session_with_the_take(&dir);
    let end = end_of(&session, id);
    let before = render(&session, end + fontelle_types::PPQN * 8);
    session.arrange(ArrangeEdit::Resize {
        ids: vec![id],
        tick_delta: fontelle_types::PPQN * 4,
    });
    // And back in by less than it went out: still the whole take.
    session.arrange(ArrangeEdit::Resize {
        ids: vec![id],
        tick_delta: -fontelle_types::PPQN * 2,
    });
    let after = render(&session, end + fontelle_types::PPQN * 8);
    assert_eq!(differing(&before, &after, &[]), 0);
    std::fs::remove_dir_all(&dir).ok();
}

// ---------------------------------------------------- an older project ---

#[test]
fn a_project_saved_with_a_clip_grown_past_its_trim_opens_healed() {
    // What the old build left behind: a head half grown back out, whose trim
    // still ends at the cut. It played the rest and drew nothing; opened now,
    // it draws what it plays.
    let dir = scratch("heal");
    let (mut session, id) = a_session_with_the_take(&dir);
    let start = session.project().clips[id].start;
    let end = end_of(&session, id);
    let mut project = session.project().clone();
    let asset = match &project.clips[id].source {
        fontelle_model::ClipSource::Audio(data) => data.asset.clone(),
        _ => unreachable!(),
    };
    {
        let clip = &mut project.clips[id];
        if let fontelle_model::ClipSource::Audio(data) = &mut clip.source {
            data.source_end = data.source_start + SR as i64; // a second, as a cut left it
        }
    }
    let bundle = dir.join("Old.fontelle");
    fontelle_app::save_project(&project, &bundle).expect("saves");
    // The file the clip names has to be where the bundle says.
    let _ = asset;
    session.open_project_path(&bundle).expect("opens");
    let id = audio_clips(&session)[0];
    assert_eq!(end_of(&session, id), end);
    assert_eq!(session.project().clips[id].start, start);
    let clips = session.clips();
    let clip = clips.iter().find(|c| c.kind == ClipKind::Audio).unwrap();
    assert!(
        clip.audio.natural_length >= clip.length - fontelle_types::PPQN / 8,
        "the reopened clip still draws {} of {} ticks",
        clip.audio.natural_length,
        clip.length
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_right_half_of_a_looping_cut_unlooped_still_plays_what_it_did() {
    // Dragged back inside one pass, a loop stops being one — and the half
    // that began mid-pass has to go on beginning there, its phase becoming
    // where in the take it starts.
    let dir = scratch("unloopphase");
    let (mut session, id) = a_looping_take(&dir);
    let start = session.project().clips[id].start;
    let end = end_of(&session, id);
    let before = render(&session, end + fontelle_types::PPQN);
    let at = start + fontelle_types::PPQN * 4 * 2 + fontelle_types::PPQN * 2;
    session.arrange(ArrangeEdit::Split {
        cuts: vec![(id, at)],
    });
    let tail = *audio_clips(&session).last().unwrap();
    session.arrange(ArrangeEdit::SetLoop {
        ids: vec![tail],
        loop_length: None,
    });
    session.arrange(ArrangeEdit::Resize {
        ids: vec![tail],
        tick_delta: -(end - at) + fontelle_types::PPQN * 2,
    });
    let after = render(&session, end + fontelle_types::PPQN);
    let (from, to) = (
        frame_of(&session, at),
        frame_of(&session, at + fontelle_types::PPQN * 2),
    );
    let wrong = (from + 200..to - 200)
        .filter(|i| (before[*i] - after[*i]).abs() > 0.01)
        .count();
    assert_eq!(wrong, 0, "{wrong} frames of the unlooped half changed");
    std::fs::remove_dir_all(&dir).ok();
}
