//! An audio clip's left edge is a grip.
//!
//! > *"i cant even drag in clips from the left too."*
//!
//! The press on the left edge of an audio block takes the **left grip**, and
//! dragging it emits `ArrangeEdit::TrimStart` — snapped, never past the song's
//! start, never leaving the block shorter than a snap unit. A note or
//! automation block's left edge is still its body, because a part has nowhere
//! to put the notes an edge dragged in would uncover.
//!
//! And a looping clip that begins part-way through its pass — the right half
//! of a cut, or a left edge trimmed in — **draws** from there
//! (`AudioPreview::loop_offset`), because `content_fraction` is where the
//! waveform, the fades and the crossfades all ask.

use fontelle_model::Arena;
use fontelle_types::{ClipId, PPQN, Tick};
use fontelle_ui::canvas::{
    ArrangeEdit, ClipPart, MouseButton, SnapDivision, Timeline, TimelineHit, TimelineView,
    clip_rect, content_fraction, timeline_hit, timeline_layout,
};
use fontelle_ui::document::{AudioPreview, ClipInfo, ClipKind};
use fontelle_ui::layout::Rect;
use fontelle_ui::pointer::Pointer;
use fontelle_ui::theme::Theme;

const BAR: Tick = PPQN * 4;

fn view() -> TimelineView {
    TimelineView {
        pixels_per_tick: 0.05,
        lane_height: 60.0,
        ..TimelineView::default()
    }
}

fn layout() -> fontelle_ui::canvas::TimelineLayout {
    timeline_layout(
        Rect::new(0.0, 0.0, 1200.0, 300.0),
        &Theme::dark_default().metrics,
    )
}

fn new_id() -> ClipId {
    let mut arena: Arena<ClipId, ()> = Arena::default();
    arena.insert(())
}

fn block(id: ClipId, kind: ClipKind, start: Tick, length: Tick) -> ClipInfo {
    ClipInfo {
        id,
        lane: 0,
        start,
        length,
        name: "Take".to_string(),
        muted: false,
        open: false,
        color: [0x4f, 0x8f, 0xd0, 0xff],
        loop_length: None,
        kind,
        curve: Vec::new(),
        notes: Vec::new(),
        audio: AudioPreview {
            peaks: vec![(-0.5, 0.5); 64].into(),
            natural_length: length,
            ..AudioPreview::default()
        },
        prefab: None,
    }
}

/// Mid-height, a couple of points in from the left edge: below the fade-in
/// handle in the top corner.
fn left_edge(clip: &ClipInfo) -> (f32, f32) {
    let b = clip_rect(&view(), layout().grid, clip);
    (b.x + 2.0, b.y + b.height * 0.6)
}

#[test]
fn the_left_edge_of_an_audio_block_is_its_left_grip() {
    let clip = block(new_id(), ClipKind::Audio, BAR * 2, BAR * 2);
    let (x, y) = left_edge(&clip);
    let hit = timeline_hit(&view(), &layout(), std::slice::from_ref(&clip), x, y);
    assert_eq!(hit, TimelineHit::Clip(clip.id, ClipPart::LeftEdge));
    assert_eq!(
        fontelle_ui::pointer::timeline_pointer(hit),
        Pointer::ResizeX,
        "the left grip does not show the resize pointer"
    );
}

#[test]
fn a_note_blocks_left_edge_is_still_its_body() {
    let clip = block(new_id(), ClipKind::Notes, BAR * 2, BAR * 2);
    let (x, y) = left_edge(&clip);
    let hit = timeline_hit(&view(), &layout(), std::slice::from_ref(&clip), x, y);
    assert_eq!(hit, TimelineHit::Clip(clip.id, ClipPart::Body));
}

fn drag_left(timeline: &mut Timeline, clips: &[ClipInfo], dx_ticks: Tick) -> Vec<ArrangeEdit> {
    let (x, y) = left_edge(&clips[0]);
    let l = layout();
    timeline.press(MouseButton::Left, x, y, &l, clips, 4);
    timeline.drag(
        x + dx_ticks as f32 * timeline.view.pixels_per_tick,
        y,
        &l,
        clips,
        4,
    )
}

fn trims(edits: &[ArrangeEdit]) -> Tick {
    edits
        .iter()
        .map(|e| match e {
            ArrangeEdit::TrimStart { tick_delta, .. } => *tick_delta,
            _ => 0,
        })
        .sum()
}

#[test]
fn dragging_the_left_grip_trims_the_front_on_the_grid() {
    let clip = block(new_id(), ClipKind::Audio, BAR * 2, BAR * 2);
    let mut timeline = Timeline::new(view());
    timeline.view.snap = SnapDivision::Beat;
    let clips = vec![clip];
    // A beat and a bit in: lands on the beat.
    let edits = drag_left(&mut timeline, &clips, PPQN + PPQN / 5);
    assert_eq!(trims(&edits), PPQN);
    assert!(
        !edits
            .iter()
            .any(|e| matches!(e, ArrangeEdit::Move { .. } | ArrangeEdit::Resize { .. })),
        "a left-grip drag moved or resized instead: {edits:?}"
    );
}

#[test]
fn the_left_grip_never_goes_before_the_song_or_past_the_other_edge() {
    let clip = block(new_id(), ClipKind::Audio, BAR, BAR);
    let clips = vec![clip];
    let mut timeline = Timeline::new(view());
    timeline.view.snap = SnapDivision::Beat;
    let out = trims(&drag_left(&mut timeline, &clips, -BAR * 3));
    assert_eq!(out, -BAR, "the front went before the start of the song");

    let mut timeline = Timeline::new(view());
    timeline.view.snap = SnapDivision::Beat;
    let edits = drag_left(&mut timeline, &clips, BAR * 3);
    assert_eq!(
        trims(&edits),
        BAR - PPQN,
        "the front went past the end, or left less than a beat"
    );
}

#[test]
fn a_looping_block_that_starts_mid_pass_draws_from_there() {
    let mut clip = block(new_id(), ClipKind::Audio, 0, BAR * 3);
    clip.loop_length = Some(BAR);
    clip.audio.natural_length = BAR;
    let plain = content_fraction(&clip, (PPQN * 2) as f32).unwrap();
    clip.audio.loop_offset = PPQN * 2;
    let phased = content_fraction(&clip, 0.0).unwrap();
    assert!((plain - phased).abs() < 1e-4, "{plain} vs {phased}");
    // And the seam comes round a pass *later* than the block's start.
    let seam = content_fraction(&clip, (PPQN * 2) as f32).unwrap();
    assert!(seam < 0.01, "the next pass begins two beats in, at {seam}");
}

#[test]
fn a_looping_block_that_starts_mid_pass_marks_its_seams_where_the_cycle_comes_round() {
    let mut clip = block(new_id(), ClipKind::Audio, 0, BAR * 3);
    clip.loop_length = Some(BAR);
    clip.audio.loop_offset = PPQN * 2;
    let v = view();
    let marks = fontelle_ui::canvas::loop_marks(&v, layout().grid, &clip);
    let b = clip_rect(&v, layout().grid, &clip);
    let at = |ticks: Tick| b.x + ticks as f32 * v.pixels_per_tick;
    assert_eq!(marks.len(), 3, "{marks:?}");
    // Half a pass to the first seam, then a pass each.
    for (mark, ticks) in marks.iter().zip([PPQN * 2, PPQN * 6, PPQN * 10]) {
        assert!((mark - at(ticks)).abs() < 0.5, "{marks:?}");
    }
}
