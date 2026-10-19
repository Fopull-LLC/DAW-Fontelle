//! The arrangement's paint brush: drag along a row to lay copies of the clip
//! in hand end to end, the way FL's playlist paint tool does.
//!
//! Ty, using the window:
//!
//! > *"no brush tool in the arrangement. and when using the brush tool right
//! > now youre able to move your mouse up and down for it to go to other note
//! > lanes. when dragging out the brush left and right it should be locked to
//! > the lane youre clicking in ... maybe a key should be held to make the
//! > lane lock not apply"*
//!
//! What it paints is what the pencil's Shift+press stamps — **the clip in
//! hand**, the last one chosen — and with nothing in hand, what the pencil's
//! double-click draws: a blank clip on the rack's selection (the host's to
//! decide, `ArrangeEdit::Paint { source: None }`). Copies go down a clip's
//! length apart from where the stroke began, on the grid, and never on top of
//! a clip that is already there. The row is the press's, whatever the hand
//! does, unless Shift is held — the roll's brush keeps the same rule with the
//! same key. The right button erases, as it does with every tool.

use fontelle_model::Arena;
use fontelle_types::{ClipId, PPQN, Tick};
use fontelle_ui::canvas::{
    Action, ArrangeEdit, Modifiers, MouseButton, Timeline, TimelineControl, TimelineLayout,
    TimelineTool, TimelineView, lane_to_y, timeline_layout, timeline_tick_to_x,
    timeline_toolbar_layout,
};
use fontelle_ui::document::{ClipInfo, ClipKind};
use fontelle_ui::layout::Rect;
use fontelle_ui::theme::Theme;

const BAR: Tick = PPQN * 4;

fn layout() -> TimelineLayout {
    timeline_layout(
        Rect::new(0.0, 0.0, 2400.0, 400.0),
        &Theme::dark_default().metrics,
    )
}

fn view() -> TimelineView {
    TimelineView {
        pixels_per_tick: 0.05,
        ..TimelineView::default()
    }
}

fn a_clip(arena: &mut Arena<ClipId, ()>, lane: usize, start: Tick, length: Tick) -> ClipInfo {
    ClipInfo {
        id: arena.insert(()),
        lane,
        start,
        length,
        name: "Hats".to_string(),
        muted: false,
        open: false,
        color: [0x4f, 0x8f, 0xd0, 0xff],
        loop_length: None,
        kind: ClipKind::Notes,
        curve: Vec::new(),
        notes: Vec::new(),
        audio: Default::default(),
        prefab: None,
    }
}

/// Where `(tick, lane)` is on screen, a little inside the cell.
fn at(timeline: &Timeline, tick: Tick, lane: usize) -> (f32, f32) {
    let l = layout();
    (
        timeline_tick_to_x(&timeline.view, l.grid, tick) + 2.0,
        lane_to_y(&timeline.view, l.grid, lane) + timeline.view.lane_height / 2.0,
    )
}

fn painter() -> Timeline {
    let mut timeline = Timeline::new(view());
    timeline.set_tool(TimelineTool::Paint);
    timeline
}

/// Chooses `clip` the way a person does: a click on it with the pencil.
fn pick(timeline: &mut Timeline, clips: &[ClipInfo], clip: &ClipInfo) {
    let tool = timeline.tool();
    timeline.set_tool(TimelineTool::Draw);
    let (x, y) = at(timeline, clip.start + PPQN, clip.lane);
    timeline.press(MouseButton::Left, x, y, &layout(), clips);
    timeline.release();
    timeline.set_tool(tool);
}

/// A left-button stroke: pressed at the first point, dragged through the
/// rest, released. Every edit it asked for, in order.
fn stroke(
    timeline: &mut Timeline,
    clips: &[ClipInfo],
    points: &[(Tick, usize)],
) -> Vec<ArrangeEdit> {
    let l = layout();
    let (x, y) = at(timeline, points[0].0, points[0].1);
    let mut edits = timeline.press(MouseButton::Left, x, y, &l, clips);
    for (tick, lane) in &points[1..] {
        let (x, y) = at(timeline, *tick, *lane);
        edits.extend(timeline.drag(x, y, &l, clips));
    }
    let (x, y) = at(
        timeline,
        points[points.len() - 1].0,
        points[points.len() - 1].1,
    );
    edits.extend(timeline.release_over(x, y, &l, clips));
    edits
}

fn painted(edits: &[ArrangeEdit]) -> Vec<(usize, Tick)> {
    edits
        .iter()
        .filter_map(|edit| match edit {
            ArrangeEdit::Paint { lane, start, .. } => Some((*lane, *start)),
            _ => None,
        })
        .collect()
}

// ------------------------------------------------------------- the tool ---

#[test]
fn the_arrangement_has_a_paint_tool_with_the_rolls_brush() {
    let bar = timeline_toolbar_layout(layout().toolbar, &Theme::dark_default().metrics);
    assert!(
        bar.items.iter().any(|(c, _)| *c == TimelineControl::Paint),
        "no paint button on the arrangement's toolbar"
    );
    assert_eq!(
        TimelineControl::Paint.icon(),
        Some(fontelle_ui::icon::Icon::Brush),
        "the same picture as the roll's brush"
    );
    assert_eq!(TimelineControl::Paint.action(), Some(Action::PaintTool));
    let tip = TimelineControl::Paint.tip().expect("a tip");
    assert!(
        tip.contains("Shift"),
        "{tip:?} does not say how to cross rows"
    );
    assert!(tip.len() <= 64, "{tip:?} is a paragraph");
    // The tools come first, together: the brush sits with the pencil.
    let order: Vec<_> = bar.items.iter().map(|(c, _)| *c).collect();
    let draw = order
        .iter()
        .position(|c| *c == TimelineControl::Draw)
        .unwrap();
    assert_eq!(order[draw + 1], TimelineControl::Paint);
}

// ---------------------------------------------------- what gets painted ---

#[test]
fn a_stroke_paints_copies_of_the_clip_in_hand_along_the_row_it_began_on() {
    let mut arena = Arena::default();
    let clips = vec![a_clip(&mut arena, 0, 0, BAR)];
    let mut timeline = painter();
    pick(&mut timeline, &clips, &clips[0]);

    // Along row 2, the hand drifting down to row 3 and 4 on the way.
    let edits = stroke(
        &mut timeline,
        &clips,
        &[
            (BAR * 2, 2),
            (BAR * 3, 3),
            (BAR * 4 + PPQN, 4),
            (BAR * 5, 3),
        ],
    );
    assert_eq!(
        painted(&edits),
        vec![(2, BAR * 2), (2, BAR * 3), (2, BAR * 4), (2, BAR * 5)],
        "every copy on the row the press was on"
    );
    for edit in &edits {
        if let ArrangeEdit::Paint { source, .. } = edit {
            assert_eq!(*source, Some(clips[0].id), "copies of the clip in hand");
        }
    }
}

#[test]
fn with_shift_held_the_stroke_follows_the_pointer_across_rows() {
    let mut arena = Arena::default();
    let clips = vec![a_clip(&mut arena, 0, 0, BAR)];
    let mut timeline = painter();
    pick(&mut timeline, &clips, &clips[0]);
    timeline.set_modifiers(Modifiers {
        shift: true,
        ..Modifiers::default()
    });
    let edits = stroke(
        &mut timeline,
        &clips,
        &[(BAR * 2, 1), (BAR * 3, 2), (BAR * 4, 3)],
    );
    assert_eq!(
        painted(&edits),
        vec![(1, BAR * 2), (2, BAR * 3), (3, BAR * 4)]
    );
}

#[test]
fn a_longer_clip_is_laid_end_to_end_not_on_every_bar() {
    let mut arena = Arena::default();
    let clips = vec![a_clip(&mut arena, 0, 0, BAR * 2)];
    let mut timeline = painter();
    pick(&mut timeline, &clips, &clips[0]);
    let edits = stroke(
        &mut timeline,
        &clips,
        &[
            (BAR * 4, 1),
            (BAR * 5, 1),
            (BAR * 6, 1),
            (BAR * 7, 1),
            (BAR * 8, 1),
        ],
    );
    assert_eq!(
        painted(&edits),
        vec![(1, BAR * 4), (1, BAR * 6), (1, BAR * 8)]
    );
}

#[test]
fn a_fast_stroke_fills_the_copies_it_jumped_over() {
    // At a zoomed-out view one pointer event can cross several bars.
    let mut arena = Arena::default();
    let clips = vec![a_clip(&mut arena, 0, 0, BAR)];
    let mut timeline = painter();
    pick(&mut timeline, &clips, &clips[0]);
    let edits = stroke(&mut timeline, &clips, &[(BAR, 1), (BAR * 5, 1)]);
    assert_eq!(
        painted(&edits),
        vec![
            (1, BAR),
            (1, BAR * 2),
            (1, BAR * 3),
            (1, BAR * 4),
            (1, BAR * 5)
        ]
    );
}

#[test]
fn a_stroke_never_paints_over_a_clip_already_there() {
    let mut arena = Arena::default();
    let clips = vec![
        a_clip(&mut arena, 0, 0, BAR),
        a_clip(&mut arena, 1, BAR * 3, BAR),
    ];
    let mut timeline = painter();
    pick(&mut timeline, &clips, &clips[0]);
    let edits = stroke(
        &mut timeline,
        &clips,
        &[(BAR, 1), (BAR * 2, 1), (BAR * 3, 1), (BAR * 4, 1)],
    );
    assert_eq!(painted(&edits), vec![(1, BAR), (1, BAR * 2), (1, BAR * 4)]);
}

#[test]
fn back_over_the_same_ground_paints_nothing_twice() {
    let mut arena = Arena::default();
    let clips = vec![a_clip(&mut arena, 0, 0, BAR)];
    let mut timeline = painter();
    pick(&mut timeline, &clips, &clips[0]);
    let edits = stroke(
        &mut timeline,
        &clips,
        &[(BAR, 1), (BAR * 3, 1), (BAR * 2, 1), (BAR, 1), (BAR * 3, 1)],
    );
    assert_eq!(painted(&edits), vec![(1, BAR), (1, BAR * 2), (1, BAR * 3)]);
}

#[test]
fn a_stroke_paints_leftwards_too_but_never_before_the_song() {
    let mut arena = Arena::default();
    let clips = vec![a_clip(&mut arena, 0, BAR * 6, BAR)];
    let mut timeline = painter();
    pick(&mut timeline, &clips, &clips[0]);
    let edits = stroke(&mut timeline, &clips, &[(BAR * 2, 1), (BAR, 1), (0, 1)]);
    assert_eq!(painted(&edits), vec![(1, BAR * 2), (1, BAR), (1, 0)]);
}

#[test]
fn pressing_a_clip_with_the_brush_takes_it_in_hand_and_paints_on_after_it() {
    // FL: press the pattern you want with the paint tool and drag it out.
    let mut arena = Arena::default();
    let clips = vec![
        a_clip(&mut arena, 0, 0, BAR),
        a_clip(&mut arena, 2, BAR * 2, BAR),
    ];
    let mut timeline = painter();
    pick(&mut timeline, &clips, &clips[0]);
    let edits = stroke(
        &mut timeline,
        &clips,
        &[
            (BAR * 2 + PPQN, 2),
            (BAR * 3 + PPQN, 3),
            (BAR * 4 + PPQN, 2),
        ],
    );
    assert_eq!(painted(&edits), vec![(2, BAR * 3), (2, BAR * 4)]);
    for edit in &edits {
        if let ArrangeEdit::Paint { source, .. } = edit {
            assert_eq!(
                *source,
                Some(clips[1].id),
                "the clip pressed is the one painted"
            );
        }
    }
    assert_eq!(timeline.stamp_source(), Some(clips[1].id));
}

#[test]
fn with_nothing_in_hand_the_brush_asks_the_host_for_blank_bars() {
    let mut timeline = painter();
    let edits = stroke(&mut timeline, &[], &[(0, 0), (BAR, 0), (BAR * 2, 0)]);
    assert_eq!(
        edits,
        vec![
            ArrangeEdit::Paint {
                source: None,
                lane: 0,
                start: 0
            },
            ArrangeEdit::Paint {
                source: None,
                lane: 0,
                start: BAR
            },
            ArrangeEdit::Paint {
                source: None,
                lane: 0,
                start: BAR * 2
            },
        ]
    );
}

#[test]
fn a_clip_in_hand_that_has_been_deleted_is_nothing_in_hand() {
    let mut arena = Arena::default();
    let clips = vec![a_clip(&mut arena, 0, 0, BAR)];
    let mut timeline = painter();
    pick(&mut timeline, &clips, &clips[0]);
    let edits = stroke(&mut timeline, &[], &[(BAR, 1)]);
    assert_eq!(
        edits,
        vec![ArrangeEdit::Paint {
            source: None,
            lane: 1,
            start: BAR
        }]
    );
}

#[test]
fn the_right_button_still_erases_with_the_brush() {
    let mut arena = Arena::default();
    let clips = vec![a_clip(&mut arena, 1, BAR, BAR)];
    let mut timeline = painter();
    let (x, y) = at(&timeline, BAR + PPQN, 1);
    let edits = timeline.press(MouseButton::Right, x, y, &layout(), &clips);
    assert_eq!(edits, vec![ArrangeEdit::Remove(vec![clips[0].id])]);
}

#[test]
fn ctrl_with_the_brush_is_still_the_marquee() {
    let mut timeline = painter();
    timeline.set_modifiers(Modifiers {
        ctrl: true,
        ..Modifiers::default()
    });
    let edits = stroke(&mut timeline, &[], &[(0, 0), (BAR * 2, 1)]);
    assert!(painted(&edits).is_empty());
}

#[test]
fn the_paint_key_picks_the_brush_on_the_arrangement_too() {
    // B was the roll's alone; the keymap's description no longer says so.
    assert!(
        !Action::PaintTool.does().contains("piano roll"),
        "{:?}",
        Action::PaintTool.does()
    );
}

#[test]
fn the_shortcuts_page_says_how_to_cross_rows_with_a_brush() {
    let keymap = fontelle_ui::canvas::Keymap::default();
    let found = fontelle_ui::canvas::KEYBIND_SECTIONS.iter().any(|section| {
        section.binds.iter().any(|bind| {
            bind.keys(&keymap).contains("Shift") && bind.does().to_lowercase().contains("brush")
        })
    });
    assert!(
        found,
        "no line on the shortcuts page for Shift with the brush"
    );
}
