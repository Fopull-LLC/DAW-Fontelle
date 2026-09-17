//! The Stretch switch on the arrangement's toolbar, and what an edge drag
//! means when it is on and when it is off.
//!
//! Reported from using the window:
//!
//! > *"i cannot loop clips, whenever i drag them it is ALWAYS stretching
//! > them. we should make it so theres a stretch on/off toggle control with
//! > the arrangement controls and that defines whether it cuts the clip or
//! > stretches it and then also resolve the issue of it trying to stretch
//! > while looping and whatnot so it all works together cleanly."*
//!
//! Two halves, both here. The **gesture**: a drag on an audio clip's edge
//! decides, at the press, whether the clip follows its block (stretch on) or
//! keeps playing at its own rate and is merely shown more or less of (stretch
//! off), and says so with one `ArrangeEdit::SetStretch` on the first step —
//! the same shape the Shift-loop already has. The **picture**: the waveform
//! is drawn per pass and per mode, so a clip that is not stretched stops
//! where its file stops and a loop shows the file again at every seam, rather
//! than one picture smeared over the whole block whatever the sound does.

use fontelle_model::Arena;
use fontelle_types::{ClipId, ClipStretch, PPQN, Tick};
use fontelle_ui::canvas::{
    ArrangeEdit, FadeEnd, Modifiers, MouseButton, Timeline, TimelineControl, TimelineView,
    clip_bands, clip_rect, clip_waveform, fade_anatomy, timeline_layout, timeline_toolbar_layout,
};
use fontelle_ui::document::{AudioPreview, ClipInfo, ClipKind};
use fontelle_ui::layout::Rect;
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

fn grid() -> Rect {
    layout().grid
}

fn new_id() -> ClipId {
    let mut arena: Arena<ClipId, ()> = Arena::default();
    arena.insert(())
}

fn ids(n: usize) -> Vec<ClipId> {
    let mut arena: Arena<ClipId, ()> = Arena::default();
    (0..n).map(|_| arena.insert(())).collect()
}

/// An audio clip `length` long on lane 0 from tick 0, whose file would take
/// `natural` ticks at its own rate, stretched or not.
fn audio(id: ClipId, length: Tick, natural: Tick, stretched: bool) -> ClipInfo {
    ClipInfo {
        id,
        lane: 0,
        start: 0,
        length,
        name: "Take".to_string(),
        muted: false,
        open: false,
        color: [0x4f, 0x8f, 0xd0, 0xff],
        loop_length: None,
        kind: ClipKind::Audio,
        curve: Vec::new(),
        notes: Vec::new(),
        audio: AudioPreview {
            peaks: vec![(-0.5, 0.5); 64].into(),
            natural_length: natural,
            stretched,
            ..AudioPreview::default()
        },
        prefab: None,
    }
}

fn notes(id: ClipId, lane: usize, length: Tick) -> ClipInfo {
    ClipInfo {
        id,
        lane,
        start: 0,
        length,
        name: "Part".to_string(),
        muted: false,
        open: false,
        color: [0x4f, 0x8f, 0xd0, 0xff],
        loop_length: None,
        kind: ClipKind::Notes,
        curve: Vec::new(),
        notes: Vec::new(),
        audio: AudioPreview::default(),
        prefab: None,
    }
}

/// Presses the right-hand grip of `clip` and drags it `bars` further.
fn drag_edge(
    timeline: &mut Timeline,
    clips: &[ClipInfo],
    clip: &ClipInfo,
    bars: f32,
) -> Vec<ArrangeEdit> {
    let l = layout();
    let block = clip_rect(&timeline.view, l.grid, clip);
    let y = block.y + block.height * 0.6;
    timeline.press(MouseButton::Left, block.right() - 2.0, y, &l, clips, 4);
    timeline.drag(
        block.right() + BAR as f32 * bars * timeline.view.pixels_per_tick,
        y,
        &l,
        clips,
        4,
    )
}

fn stretch_edits(edits: &[ArrangeEdit]) -> Vec<(Vec<ClipId>, ClipStretch)> {
    edits
        .iter()
        .filter_map(|e| match e {
            ArrangeEdit::SetStretch { ids, stretch } => Some((ids.clone(), *stretch)),
            _ => None,
        })
        .collect()
}

fn has_resize(edits: &[ArrangeEdit]) -> bool {
    edits
        .iter()
        .any(|e| matches!(e, ArrangeEdit::Resize { .. }))
}

// ------------------------------------------------------------ the switch ---

#[test]
fn the_toolbar_carries_a_stretch_switch_with_a_tip_and_a_word_on_it() {
    let m = Theme::dark_default().metrics;
    let bar = timeline_toolbar_layout(layout().toolbar, &m);
    let items: Vec<TimelineControl> = bar.items.iter().map(|(c, _)| *c).collect();
    assert!(items.contains(&TimelineControl::Stretch), "{items:?}");
    assert_eq!(TimelineControl::Stretch.label(), "Stretch");
    assert!(
        TimelineControl::Stretch.tip().is_some(),
        "a switch whose meaning is not written down is a switch people flip to find out"
    );
    // A word, like the snap chip: a state is a value, and the value is what
    // you read.
    assert_eq!(TimelineControl::Stretch.icon(), None);
}

#[test]
fn stretch_is_off_until_it_is_turned_on_and_a_second_press_turns_it_back_off() {
    // Off by default, because the report is that everything stretched and a
    // take dropped on the arrangement has to sound like the take.
    let mut timeline = Timeline::new(view());
    assert!(!timeline.stretch());
    // Nothing selected, so nothing to freeze either way — the switch is just
    // a switch here. What it does to a *stretched selection* on the way off is
    // `turning_the_switch_off_freezes_the_selection`.
    assert!(timeline.toggle_stretch(&[]).is_empty());
    assert!(timeline.stretch());
    assert!(timeline.toggle_stretch(&[]).is_empty());
    assert!(!timeline.stretch());
    timeline.set_stretch(true);
    assert!(timeline.stretch());
}

// ------------------------------------------------------------ the gesture ---

#[test]
fn with_stretch_off_an_edge_drag_on_an_unstretched_clip_only_resizes() {
    let id = new_id();
    let clips = vec![audio(id, BAR, BAR, false)];
    let mut timeline = Timeline::new(view());
    let edits = drag_edge(&mut timeline, &clips, &clips[0], 1.0);
    assert!(has_resize(&edits), "{edits:?}");
    assert!(
        stretch_edits(&edits).is_empty(),
        "nothing to change, so nothing is sent: {edits:?}"
    );
}

#[test]
fn with_stretch_on_the_first_step_makes_the_clip_follow_its_block_and_then_resizes() {
    let id = new_id();
    let clips = vec![audio(id, BAR, BAR, false)];
    let mut timeline = Timeline::new(view());
    timeline.set_stretch(true);
    let edits = drag_edge(&mut timeline, &clips, &clips[0], 1.0);
    assert_eq!(
        stretch_edits(&edits),
        vec![(vec![id], ClipStretch::Resample)]
    );
    assert!(has_resize(&edits), "{edits:?}");
    // The mode before the size: the resize has to land on a clip that
    // already knows what a longer block means.
    let mode_at = edits
        .iter()
        .position(|e| matches!(e, ArrangeEdit::SetStretch { .. }))
        .unwrap();
    let resize_at = edits
        .iter()
        .position(|e| matches!(e, ArrangeEdit::Resize { .. }))
        .unwrap();
    assert!(mode_at < resize_at, "{edits:?}");
}

#[test]
fn the_mode_is_sent_once_and_the_rest_of_the_drag_is_only_resizes() {
    let id = new_id();
    let clips = vec![audio(id, BAR, BAR, false)];
    let mut timeline = Timeline::new(view());
    timeline.set_stretch(true);
    let l = layout();
    let block = clip_rect(&timeline.view, l.grid, &clips[0]);
    let y = block.y + block.height * 0.6;
    let step = BAR as f32 * timeline.view.pixels_per_tick;
    timeline.press(MouseButton::Left, block.right() - 2.0, y, &l, &clips, 4);
    let first = timeline.drag(block.right() + step, y, &l, &clips, 4);
    assert_eq!(stretch_edits(&first).len(), 1);
    let second = timeline.drag(block.right() + step * 2.0, y, &l, &clips, 4);
    assert!(stretch_edits(&second).is_empty(), "{second:?}");
    assert!(has_resize(&second), "{second:?}");
}

#[test]
fn a_clip_already_in_the_mode_the_switch_names_is_not_told_again() {
    let id = new_id();
    let clips = vec![audio(id, BAR, BAR, true)];
    let mut timeline = Timeline::new(view());
    timeline.set_stretch(true);
    let edits = drag_edge(&mut timeline, &clips, &clips[0], 1.0);
    assert!(stretch_edits(&edits).is_empty(), "{edits:?}");
    assert!(has_resize(&edits));
}

/// **A drag never turns a clip's stretch off.** Superseding the rule that used
/// to be here, and the reason is worth keeping rather than replacing.
///
/// The old rule read the switch as *"what the drag does, not a filter on which
/// clips it does it to"*, so a drag with the switch off turned a stretched
/// clip off first and then cut it. That is coherent, and it made trims
/// **lossy**: turning a stretched clip off freezes the rate it was being
/// played at into its own `speed` (`fontelle_model::with_stretch`, which does
/// that so the sound does not jump). A clip stretched down to a quarter and
/// then dragged became a clip genuinely playing four times too fast, whose
/// take really was a quarter as long — and no drag could bring the rest back.
///
/// Two reports came out of that: *"its stretching the clip back to how it was
/// before before letting you extend the length of the ending"* and *"making
/// the audio show completely blank after that even though it actually does
/// have content"*. Both are the same fault seen from two sides.
///
/// So the switch's off position means *"drags trim"*, and turning it off is
/// the deliberate act that freezes — see
/// `turning_the_switch_off_freezes_the_stretched_clips_in_the_selection`.
#[test]
fn with_stretch_off_a_drag_on_a_stretched_clip_leaves_its_stretch_alone() {
    let id = new_id();
    let clips = vec![audio(id, BAR, BAR, true)];
    let mut timeline = Timeline::new(view());
    let edits = drag_edge(&mut timeline, &clips, &clips[0], 1.0);
    assert!(
        stretch_edits(&edits).is_empty(),
        "the drag froze the clip's stretch without being asked: {edits:?}"
    );
    assert!(has_resize(&edits));
}

/// And this is where a stretched clip comes back down: the switch, pressed on
/// purpose, with the clip selected.
#[test]
fn turning_the_switch_off_freezes_the_stretched_clips_in_the_selection() {
    let id = new_id();
    let clips = vec![audio(id, BAR, BAR, true)];
    let mut timeline = Timeline::new(view());
    timeline.select(vec![id]);
    timeline.set_stretch(true);
    // Off: the one gesture that freezes.
    let edits = timeline.toggle_stretch(&clips);
    assert_eq!(stretch_edits(&edits), vec![(vec![id], ClipStretch::Off)]);
    assert!(!timeline.stretch());
    // And on again asks for nothing: what a longer block means is settled when
    // the block is dragged, not when the switch is flipped.
    assert!(timeline.toggle_stretch(&clips).is_empty());
    assert!(timeline.stretch());
}

#[test]
fn only_the_audio_clips_in_a_selection_are_given_a_mode() {
    // A note clip's edge drag has always cut, and the switch has nothing to
    // say to it — but it must not stop the audio clip beside it hearing.
    let ids = ids(2);
    let clips = vec![audio(ids[0], BAR, BAR, false), notes(ids[1], 1, BAR)];
    let mut timeline = Timeline::new(view());
    timeline.set_stretch(true);
    timeline.select(ids.clone());
    let edits = drag_edge(&mut timeline, &clips, &clips[0], 1.0);
    assert_eq!(
        stretch_edits(&edits),
        vec![(vec![ids[0]], ClipStretch::Resample)]
    );
    let resized: Vec<ClipId> = edits
        .iter()
        .find_map(|e| match e {
            ArrangeEdit::Resize { ids, .. } => Some(ids.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(resized, ids, "both still grow");
}

#[test]
fn a_selection_of_only_note_clips_sends_no_mode_at_all() {
    let id = new_id();
    let clips = vec![notes(id, 0, BAR)];
    let mut timeline = Timeline::new(view());
    timeline.set_stretch(true);
    let edits = drag_edge(&mut timeline, &clips, &clips[0], 1.0);
    assert!(stretch_edits(&edits).is_empty(), "{edits:?}");
    assert!(has_resize(&edits));
}

#[test]
fn the_switch_is_read_at_the_press_and_not_again_during_the_drag() {
    // The same rule Shift follows on this grip: a switch flipped halfway
    // through a drag must not change what the drag has been doing.
    let id = new_id();
    let clips = vec![audio(id, BAR, BAR, false)];
    let mut timeline = Timeline::new(view());
    timeline.set_stretch(true);
    let l = layout();
    let block = clip_rect(&timeline.view, l.grid, &clips[0]);
    let y = block.y + block.height * 0.6;
    timeline.press(MouseButton::Left, block.right() - 2.0, y, &l, &clips, 4);
    timeline.set_stretch(false);
    let edits = timeline.drag(
        block.right() + BAR as f32 * timeline.view.pixels_per_tick,
        y,
        &l,
        &clips,
        4,
    );
    assert_eq!(
        stretch_edits(&edits),
        vec![(vec![id], ClipStretch::Resample)]
    );
}

#[test]
fn a_shift_drag_with_stretch_off_loops_the_clip_at_its_own_rate_and_stretches_nothing() {
    // The report's second half: looping and stretching had become one
    // gesture. With the switch off, Shift-dragging loops the clip and grows
    // it, and leaves its stretch exactly as it found it — a drag does not
    // freeze a stretch (see
    // `with_stretch_off_a_drag_on_a_stretched_clip_leaves_its_stretch_alone`).
    let id = new_id();
    let clips = vec![audio(id, BAR, BAR, true)];
    let mut timeline = Timeline::new(view());
    timeline.set_modifiers(Modifiers {
        shift: true,
        ..Modifiers::default()
    });
    let edits = drag_edge(&mut timeline, &clips, &clips[0], 3.0);
    assert!(stretch_edits(&edits).is_empty(), "{edits:?}");
    let kinds: Vec<&str> = edits
        .iter()
        .map(|e| match e {
            ArrangeEdit::SetStretch { .. } => "stretch",
            ArrangeEdit::SetLoop { .. } => "loop",
            ArrangeEdit::Resize { .. } => "resize",
            _ => "other",
        })
        .collect();
    assert_eq!(kinds, vec!["loop", "resize"]);
    let period = edits.iter().find_map(|e| match e {
        ArrangeEdit::SetLoop { loop_length, .. } => Some(*loop_length),
        _ => None,
    });
    assert_eq!(period, Some(Some(BAR)), "the period is the length it had");
}

#[test]
fn a_shift_drag_with_stretch_on_loops_a_stretched_pass() {
    let id = new_id();
    let clips = vec![audio(id, BAR, BAR, false)];
    let mut timeline = Timeline::new(view());
    timeline.set_stretch(true);
    timeline.set_modifiers(Modifiers {
        shift: true,
        ..Modifiers::default()
    });
    let edits = drag_edge(&mut timeline, &clips, &clips[0], 3.0);
    assert_eq!(
        stretch_edits(&edits),
        vec![(vec![id], ClipStretch::Resample)]
    );
    assert!(
        edits
            .iter()
            .any(|e| matches!(e, ArrangeEdit::SetLoop { .. }))
    );
    assert!(has_resize(&edits));
}

// ------------------------------------------------------------ the picture ---

/// The columns of `clip`'s waveform, with the whole grid visible.
fn columns(clip: &ClipInfo) -> Vec<Rect> {
    let block = clip_rect(&view(), grid(), clip);
    clip_waveform(block, grid(), clip)
}

fn content_of(clip: &ClipInfo) -> Rect {
    clip_bands(clip_rect(&view(), grid(), clip)).1
}

/// The column at `x`, if one was built there.
fn column_at(columns: &[Rect], x: f32) -> Option<Rect> {
    columns
        .iter()
        .copied()
        .find(|c| x >= c.x && x < c.x + c.width)
}

#[test]
fn an_unstretched_clip_longer_than_its_file_is_blank_where_the_file_has_run_out() {
    // Two bars of block, one bar of file, not stretched: the sound stops at
    // the bar line, and so does the picture. Nothing drawn is the honest
    // answer here — the take is not quiet there, it is over.
    let clip = audio(new_id(), BAR * 2, BAR, false);
    let content = content_of(&clip);
    let cols = columns(&clip);
    let bar_px = BAR as f32 * view().pixels_per_tick;
    assert!(!cols.is_empty());
    let last = cols.iter().map(|c| c.x + c.width).fold(0.0f32, f32::max);
    assert!(
        (last - (content.x + bar_px)).abs() <= 1.5,
        "the picture ends where the file does: {last} vs {}",
        content.x + bar_px
    );
    assert!(column_at(&cols, content.x + bar_px + 20.0).is_none());
    assert!(column_at(&cols, content.x + bar_px - 20.0).is_some());
}

#[test]
fn a_stretched_clip_fills_its_block_whatever_the_file_would_have_taken() {
    let clip = audio(new_id(), BAR * 2, BAR, true);
    let content = content_of(&clip);
    let cols = columns(&clip);
    let last = cols.iter().map(|c| c.x + c.width).fold(0.0f32, f32::max);
    assert!(
        (last - content.right()).abs() <= 1.5,
        "{last} vs {}",
        content.right()
    );
}

#[test]
fn an_unstretched_clip_with_no_known_file_length_still_fills_its_block() {
    // §15.3: draw what exists. A clip whose rate is not known yet has no
    // natural length, and a blank block would say the take is empty.
    let clip = audio(new_id(), BAR * 2, 0, false);
    let content = content_of(&clip);
    let cols = columns(&clip);
    let last = cols.iter().map(|c| c.x + c.width).fold(0.0f32, f32::max);
    assert!((last - content.right()).abs() <= 1.5);
}

#[test]
fn an_unstretched_clip_shorter_than_its_file_shows_the_start_of_it_only() {
    // Half a bar of block over a bar of file: the block is a window and the
    // window shows the first half — the columns run over the first half of
    // the peaks, not squeezed over all of them.
    let mut clip = audio(new_id(), BAR / 2, BAR, false);
    // Peaks that grow along the file, so *which* buckets are drawn shows.
    clip.audio.peaks = (0..64)
        .map(|i| (-(i as f32) / 64.0, i as f32 / 64.0))
        .collect();
    let content = content_of(&clip);
    let cols = columns(&clip);
    let at_end = column_at(&cols, content.right() - 2.0).expect("drawn to the block's end");
    let at_start = column_at(&cols, content.x + 2.0).expect("drawn from the start");
    // A column of bucket `i` is `i/64` of the band tall, so the file's
    // middle draws half of it and the file's end would draw all of it. The
    // block's last column has to be the former: squeezing the whole file
    // into the block is exactly the bug being fixed.
    let at_file_middle = content.height * (32.0 / 64.0);
    let at_file_end = content.height * (63.0 / 64.0);
    assert!(
        (at_end.height - at_file_middle).abs() <= 2.0,
        "the block's end is the file's middle ({at_file_middle}), not its end ({at_file_end}): {}",
        at_end.height
    );
    assert!(at_end.height > at_start.height);
}

#[test]
fn a_loop_that_is_not_stretched_draws_its_file_again_at_every_seam() {
    let mut clip = audio(new_id(), BAR * 4, BAR, false);
    clip.loop_length = Some(BAR);
    clip.audio.peaks = (0..64)
        .map(|i| (-(i as f32) / 64.0, i as f32 / 64.0))
        .collect();
    let content = content_of(&clip);
    let cols = columns(&clip);
    let bar_px = BAR as f32 * view().pixels_per_tick;
    let first = column_at(&cols, content.x + 10.0).unwrap();
    let again = column_at(&cols, content.x + bar_px + 10.0).unwrap();
    assert!(
        (first.height - again.height).abs() <= 1.0,
        "{first:?} vs {again:?}"
    );
    // Tall just before the seam, short just after: the file comes round.
    let before = column_at(&cols, content.x + bar_px - 3.0).unwrap();
    let after = column_at(&cols, content.x + bar_px + 3.0).unwrap();
    assert!(
        before.height > after.height * 2.0,
        "{before:?} vs {after:?}"
    );
    // And it is drawn in the last pass too, not only the first.
    assert!(column_at(&cols, content.x + bar_px * 3.0 + 10.0).is_some());
}

#[test]
fn a_loop_whose_period_outlasts_its_file_is_blank_between_the_file_and_the_next_seam() {
    let mut clip = audio(new_id(), BAR * 4, BAR / 2, false);
    clip.loop_length = Some(BAR);
    let content = content_of(&clip);
    let cols = columns(&clip);
    let bar_px = BAR as f32 * view().pixels_per_tick;
    assert!(column_at(&cols, content.x + bar_px * 0.25).is_some());
    assert!(
        column_at(&cols, content.x + bar_px * 0.75).is_none(),
        "the file is over"
    );
    assert!(
        column_at(&cols, content.x + bar_px * 1.25).is_some(),
        "and back at the seam"
    );
    assert!(column_at(&cols, content.x + bar_px * 1.75).is_none());
}

#[test]
fn a_stretched_loop_fills_every_pass_with_the_whole_file() {
    let mut clip = audio(new_id(), BAR * 4, BAR / 2, true);
    clip.loop_length = Some(BAR);
    clip.audio.peaks = (0..64)
        .map(|i| (-(i as f32) / 64.0, i as f32 / 64.0))
        .collect();
    let content = content_of(&clip);
    let cols = columns(&clip);
    let bar_px = BAR as f32 * view().pixels_per_tick;
    assert!(column_at(&cols, content.x + bar_px * 0.75).is_some());
    let before = column_at(&cols, content.x + bar_px - 3.0).unwrap();
    let after = column_at(&cols, content.x + bar_px + 3.0).unwrap();
    assert!(
        before.height > after.height * 2.0,
        "{before:?} vs {after:?}"
    );
}

#[test]
fn the_fades_are_measured_along_the_file_not_along_the_block() {
    // A fade is frames of the file, and the player applies it against the
    // file. A fade over the whole of a one-bar file on a two-bar unstretched
    // block reaches full at the bar line, and the picture has to agree.
    let mut clip = audio(new_id(), BAR * 2, BAR, false);
    clip.audio.fade_in = 1.0;
    let content = content_of(&clip);
    let cols = columns(&clip);
    let bar_px = BAR as f32 * view().pixels_per_tick;
    let quarter = column_at(&cols, content.x + bar_px * 0.25).unwrap();
    let three_quarters = column_at(&cols, content.x + bar_px * 0.75).unwrap();
    let full = column_at(&cols, content.x + bar_px - 2.0).unwrap();
    assert!(quarter.height < three_quarters.height);
    assert!(three_quarters.height < full.height);
    // At half the block the fade is done, not half done.
    assert!(
        (full.height - content.height * 0.5).abs() <= 2.0,
        "{full:?}"
    );
}

#[test]
fn the_fade_handles_sit_on_the_file_too() {
    let mut clip = audio(new_id(), BAR * 2, BAR, false);
    clip.audio.fade_in = 0.5;
    clip.audio.fade_out = 0.25;
    let block = clip_rect(&view(), grid(), &clip);
    let bar_px = BAR as f32 * view().pixels_per_tick;
    let anatomy = fade_anatomy(block, &clip).unwrap();
    let centre = |r: Rect| r.x + r.width / 2.0;
    assert!(
        (centre(anatomy.handle_in) - (block.x + bar_px * 0.5)).abs() <= 1.0,
        "half the file, a quarter of the block: {:?}",
        anatomy.handle_in
    );
    assert!(
        (centre(anatomy.handle_out) - (block.x + bar_px * 0.75)).abs() <= 1.0,
        "measured back from where the file ends: {:?}",
        anatomy.handle_out
    );
}

#[test]
fn dragging_a_fade_handle_asks_for_a_fraction_of_the_file() {
    let clip = audio(new_id(), BAR * 2, BAR, false);
    let clips = vec![clip.clone()];
    let mut timeline = Timeline::new(view());
    let l = layout();
    let block = clip_rect(&timeline.view, l.grid, &clip);
    let bar_px = BAR as f32 * timeline.view.pixels_per_tick;
    let anatomy = fade_anatomy(block, &clip).unwrap();
    let handle = anatomy.handle_in;
    timeline.press(
        MouseButton::Left,
        handle.x + handle.width / 2.0,
        handle.y + handle.height / 2.0,
        &l,
        &clips,
        4,
    );
    // Half the file is a quarter of the block: the file takes one bar of a
    // two-bar block. A fade measured against the block would call this 0.25.
    let edits = timeline.drag(
        block.x + bar_px * 0.5,
        handle.y + handle.height / 2.0,
        &l,
        &clips,
        4,
    );
    let fraction = edits.iter().find_map(|e| match e {
        ArrangeEdit::SetFade {
            end: FadeEnd::In,
            fraction,
            ..
        } => Some(*fraction),
        _ => None,
    });
    let fraction = fraction.expect("a fade was asked for");
    assert!(
        (fraction - 0.5).abs() < 1e-3,
        "a quarter of the block is half the file: {fraction}"
    );
}
