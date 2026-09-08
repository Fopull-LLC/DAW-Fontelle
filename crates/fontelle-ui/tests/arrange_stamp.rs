//! A click on the arrangement puts down a copy of the last thing you chose.
//!
//! Reported from using the window:
//!
//! > *"whenever i click in the arrangement its making a new clip and i dont
//! > like that i want it to be a double click to create a new empty clip. a
//! > single click should instead place a exact copy of whatever your last
//! > selection is, whether its another clip, an audio clip, automation clip,
//! > etc. it should just work this makes it easier to draw out patterns
//! > similarly to fl studios controls."*
//!
//! FL's playlist works this way: the pattern in hand is what a click stamps
//! down. Here the thing in hand is **the last clip selected**, of any kind,
//! and a press on empty grid asks the host for a copy of it at that place.
//! A double-click asks for a blank clip instead — and takes back the copy
//! the first press of the pair put down, because a double-click is one
//! gesture and must not leave two clips behind.
//!
//! Nothing ever selected is nothing to stamp, so the very first press in an
//! empty arrangement still draws a clip: there is nothing else it could mean.

use fontelle_model::Arena;
use fontelle_types::{ClipId, PPQN, Tick};
use fontelle_ui::canvas::{
    ArrangeEdit, Modifiers, MouseButton, Timeline, TimelineView, lane_to_y, timeline_layout,
    timeline_tick_to_x,
};
use fontelle_ui::document::{ClipInfo, ClipKind};
use fontelle_ui::layout::Rect;
use fontelle_ui::theme::Theme;

const BAR: Tick = PPQN * 4;

fn layout() -> fontelle_ui::canvas::TimelineLayout {
    timeline_layout(
        Rect::new(0.0, 0.0, 900.0, 300.0),
        &Theme::dark_default().metrics,
    )
}

fn view() -> TimelineView {
    TimelineView {
        pixels_per_tick: 0.05,
        ..TimelineView::default()
    }
}

fn a_clip(arena: &mut Arena<ClipId, ()>, lane: usize, start: Tick, kind: ClipKind) -> ClipInfo {
    ClipInfo {
        id: arena.insert(()),
        lane,
        start,
        length: BAR,
        name: "Part".to_string(),
        muted: false,
        open: false,
        color: [0x4f, 0x8f, 0xd0, 0xff],
        loop_length: None,
        kind,
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

fn press(timeline: &mut Timeline, clips: &[ClipInfo], tick: Tick, lane: usize) -> Vec<ArrangeEdit> {
    let (x, y) = at(timeline, tick, lane);
    let edits = timeline.press(MouseButton::Left, x, y, &layout(), clips, 4);
    timeline.release();
    edits
}

fn double_press(
    timeline: &mut Timeline,
    clips: &[ClipInfo],
    tick: Tick,
    lane: usize,
) -> Vec<ArrangeEdit> {
    let (x, y) = at(timeline, tick, lane);
    let edits = timeline.double_press(MouseButton::Left, x, y, &layout(), clips, 4);
    timeline.release();
    edits
}

// ------------------------------------------------------------ the first ---

#[test]
fn with_nothing_ever_selected_a_press_on_empty_grid_draws_a_clip() {
    let mut timeline = Timeline::new(view());
    let edits = press(&mut timeline, &[], BAR * 2, 0);
    assert_eq!(
        edits,
        vec![ArrangeEdit::Add {
            lane: 0,
            start: BAR * 2
        }]
    );
}

// ------------------------------------------------------------- stamping ---

#[test]
fn after_choosing_a_clip_a_press_on_empty_grid_stamps_a_copy_of_it() {
    let mut arena = Arena::default();
    let clips = vec![a_clip(&mut arena, 0, 0, ClipKind::Notes)];
    let mut timeline = Timeline::new(view());

    // Choose it: a click on the block.
    press(&mut timeline, &clips, PPQN, 0);
    assert_eq!(timeline.selection(), &[clips[0].id]);

    // Then somewhere empty, on another row.
    let edits = press(&mut timeline, &clips, BAR * 3, 1);
    assert_eq!(
        edits,
        vec![ArrangeEdit::Stamp {
            source: clips[0].id,
            lane: 1,
            start: BAR * 3,
        }]
    );
}

#[test]
fn a_stamp_lands_on_the_grid_like_a_drawn_clip() {
    let mut arena = Arena::default();
    let clips = vec![a_clip(&mut arena, 0, 0, ClipKind::Notes)];
    let mut timeline = Timeline::new(view());
    press(&mut timeline, &clips, PPQN, 0);

    // A hair past bar 3, with the snap on bars.
    let edits = press(&mut timeline, &clips, BAR * 3 + PPQN / 3, 0);
    assert_eq!(
        edits,
        vec![ArrangeEdit::Stamp {
            source: clips[0].id,
            lane: 0,
            start: BAR * 3,
        }]
    );
}

#[test]
fn any_kind_of_clip_can_be_stamped() {
    // *"whether its another clip, an audio clip, automation clip, etc."*
    for kind in [ClipKind::Audio, ClipKind::Automation] {
        let mut arena = Arena::default();
        let clips = vec![a_clip(&mut arena, 0, 0, kind)];
        let mut timeline = Timeline::new(view());
        // A press on the caption band chooses the block whatever its kind.
        let l = layout();
        let x = timeline_tick_to_x(&timeline.view, l.grid, PPQN);
        let y = lane_to_y(&timeline.view, l.grid, 0) + 3.0;
        timeline.press(MouseButton::Left, x, y, &l, &clips, 4);
        timeline.release();
        assert_eq!(
            timeline.selection(),
            &[clips[0].id],
            "{kind:?} was not chosen"
        );

        let edits = press(&mut timeline, &clips, BAR * 2, 1);
        assert_eq!(
            edits,
            vec![ArrangeEdit::Stamp {
                source: clips[0].id,
                lane: 1,
                start: BAR * 2,
            }],
            "{kind:?} did not stamp"
        );
    }
}

#[test]
fn a_marquee_selection_is_something_to_stamp_too() {
    let mut arena = Arena::default();
    let clips = vec![a_clip(&mut arena, 0, BAR, ClipKind::Notes)];
    let mut timeline = Timeline::new(view());
    let l = layout();

    // Ctrl-drag a box around the block.
    timeline.set_modifiers(Modifiers {
        ctrl: true,
        ..Modifiers::default()
    });
    let (x0, y0) = at(&timeline, 0, 0);
    let (x1, y1) = at(&timeline, BAR * 3, 0);
    timeline.press(MouseButton::Left, x0, y0 - 5.0, &l, &clips, 4);
    timeline.drag(x1, y1 + 5.0, &l, &clips, 4);
    timeline.release_over(x1, y1 + 5.0, &l, &clips, 4);
    timeline.set_modifiers(Modifiers::default());
    assert_eq!(timeline.selection(), &[clips[0].id]);

    let edits = press(&mut timeline, &clips, BAR * 4, 1);
    assert_eq!(
        edits,
        vec![ArrangeEdit::Stamp {
            source: clips[0].id,
            lane: 1,
            start: BAR * 4,
        }]
    );
}

#[test]
fn the_copy_just_stamped_is_what_the_next_press_stamps() {
    // Which is what makes a row of clicks a row of the same clip: the copy
    // becomes the selection, as a duplicate's does, and the selection is
    // what a press stamps.
    let mut arena = Arena::default();
    let mut clips = vec![a_clip(&mut arena, 0, 0, ClipKind::Notes)];
    let mut timeline = Timeline::new(view());
    press(&mut timeline, &clips, PPQN, 0);
    press(&mut timeline, &clips, BAR * 2, 0);

    // The host made the copy and says so.
    let copy = a_clip(&mut arena, 0, BAR * 2, ClipKind::Notes);
    timeline.clips_inserted(vec![copy.id]);
    clips.push(copy);
    assert_eq!(timeline.selection(), &[clips[1].id]);

    let edits = press(&mut timeline, &clips, BAR * 4, 0);
    assert_eq!(
        edits,
        vec![ArrangeEdit::Stamp {
            source: clips[1].id,
            lane: 0,
            start: BAR * 4,
        }]
    );
}

#[test]
fn a_clip_that_has_gone_since_it_was_chosen_is_not_stamped() {
    // Deleted from another window, or undone away: a copy of nothing is a
    // blank clip, so that is what the press asks for.
    let mut arena = Arena::default();
    let clips = vec![a_clip(&mut arena, 0, 0, ClipKind::Notes)];
    let mut timeline = Timeline::new(view());
    press(&mut timeline, &clips, PPQN, 0);

    let edits = press(&mut timeline, &[], BAR * 2, 0);
    assert_eq!(
        edits,
        vec![ArrangeEdit::Add {
            lane: 0,
            start: BAR * 2
        }]
    );
}

#[test]
fn clearing_the_selection_does_not_forget_what_was_last_chosen() {
    // Escape drops the selection; the thing in hand stays in hand, the way
    // FL keeps the pattern you picked after you click away from it.
    let mut arena = Arena::default();
    let clips = vec![a_clip(&mut arena, 0, 0, ClipKind::Notes)];
    let mut timeline = Timeline::new(view());
    press(&mut timeline, &clips, PPQN, 0);
    timeline.clear_selection();
    assert!(timeline.selection().is_empty());

    let edits = press(&mut timeline, &clips, BAR * 2, 1);
    assert_eq!(
        edits,
        vec![ArrangeEdit::Stamp {
            source: clips[0].id,
            lane: 1,
            start: BAR * 2,
        }]
    );
}

// --------------------------------------------------------- double-click ---

#[test]
fn a_double_press_takes_back_the_copy_and_draws_a_blank_clip_in_its_place() {
    let mut arena = Arena::default();
    let mut clips = vec![a_clip(&mut arena, 0, 0, ClipKind::Notes)];
    let mut timeline = Timeline::new(view());
    press(&mut timeline, &clips, PPQN, 0);

    // The first press of the pair stamps, as any press does...
    let first = press(&mut timeline, &clips, BAR * 2, 1);
    assert!(matches!(first[..], [ArrangeEdit::Stamp { .. }]));
    let copy = a_clip(&mut arena, 1, BAR * 2, ClipKind::Notes);
    timeline.clips_inserted(vec![copy.id]);
    let copy_id = copy.id;
    clips.push(copy);

    // ...and the second, landing on the copy it just made, replaces it.
    let second = double_press(&mut timeline, &clips, BAR * 2, 1);
    assert_eq!(
        second,
        vec![
            ArrangeEdit::Remove(vec![copy_id]),
            ArrangeEdit::Add {
                lane: 1,
                start: BAR * 2
            },
        ]
    );
}

#[test]
fn a_double_press_with_nothing_in_hand_draws_a_clip() {
    let mut timeline = Timeline::new(view());
    // The first press drew one; the host has not reported it yet.
    press(&mut timeline, &[], BAR * 2, 0);
    let edits = double_press(&mut timeline, &[], BAR * 2, 0);
    assert_eq!(
        edits,
        vec![ArrangeEdit::Add {
            lane: 0,
            start: BAR * 2
        }]
    );
}

#[test]
fn a_double_press_on_some_other_clip_is_not_a_request_for_a_new_one() {
    // Double-clicking an existing block opens it — that is the window's
    // business — and must not delete it or draw over it.
    let mut arena = Arena::default();
    let clips = vec![a_clip(&mut arena, 0, 0, ClipKind::Notes)];
    let mut timeline = Timeline::new(view());
    press(&mut timeline, &clips, PPQN, 0);
    let edits = double_press(&mut timeline, &clips, PPQN, 0);
    assert!(edits.is_empty(), "asked for {edits:?}");
}

#[test]
fn a_stamp_is_one_press_and_the_copy_is_not_taken_back_by_a_later_double_press_elsewhere() {
    // A double-click a bar away is a new blank clip *there*; the copy from a
    // moment ago stays where it was put.
    let mut arena = Arena::default();
    let mut clips = vec![a_clip(&mut arena, 0, 0, ClipKind::Notes)];
    let mut timeline = Timeline::new(view());
    press(&mut timeline, &clips, PPQN, 0);
    press(&mut timeline, &clips, BAR * 2, 0);
    let copy = a_clip(&mut arena, 0, BAR * 2, ClipKind::Notes);
    timeline.clips_inserted(vec![copy.id]);
    clips.push(copy);

    // Somewhere else: the single press of the pair stamps again, then the
    // double press replaces *that* one only.
    press(&mut timeline, &clips, BAR * 4, 0);
    let second_copy = a_clip(&mut arena, 0, BAR * 4, ClipKind::Notes);
    timeline.clips_inserted(vec![second_copy.id]);
    let second_id = second_copy.id;
    clips.push(second_copy);
    let edits = double_press(&mut timeline, &clips, BAR * 4, 0);
    assert_eq!(
        edits,
        vec![
            ArrangeEdit::Remove(vec![second_id]),
            ArrangeEdit::Add {
                lane: 0,
                start: BAR * 4
            },
        ]
    );
}
