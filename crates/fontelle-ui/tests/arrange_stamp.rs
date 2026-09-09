//! What a click on empty arrangement means: nothing, a copy, or a blank.
//!
//! The first rule, from using the window:
//!
//! > *"whenever i click in the arrangement its making a new clip and i dont
//! > like that i want it to be a double click to create a new empty clip. a
//! > single click should instead place a exact copy of whatever your last
//! > selection is."*
//!
//! And the second, a day later, once a single click that always made
//! something turned out to be a click you could never make idly:
//!
//! > *"please make it so single clicking in the arrangement no longer makes
//! > anything, and instead to create a copy of your last selected item its
//! > shift + click and we leave creating a new empty clip as double click
//! > that way single click is freed up so it can be used freely for
//! > deselecting things without a hassle."*
//!
//! So: a **plain press** on empty grid chooses nothing and makes nothing —
//! it is how you let go of a selection. **Shift+press** puts down a copy of
//! the thing in hand, which is the last clip chosen, of any kind, the way
//! FL's playlist stamps the pattern in hand. A **double-click** asks for a
//! blank clip. Nothing in hand is nothing to copy, so a Shift+press with
//! nothing ever chosen does nothing rather than guessing.

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

/// A press with Shift held: the copy gesture.
fn shift_press(
    timeline: &mut Timeline,
    clips: &[ClipInfo],
    tick: Tick,
    lane: usize,
) -> Vec<ArrangeEdit> {
    timeline.set_modifiers(Modifiers {
        shift: true,
        ..Modifiers::default()
    });
    let edits = press(timeline, clips, tick, lane);
    timeline.set_modifiers(Modifiers::default());
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

// ------------------------------------------------------- a plain press ---

#[test]
fn with_nothing_ever_selected_a_press_on_empty_grid_makes_nothing() {
    let mut timeline = Timeline::new(view());
    let edits = press(&mut timeline, &[], BAR * 2, 0);
    assert!(edits.is_empty(), "a plain press asked for {edits:?}");
}

#[test]
fn a_plain_press_on_empty_grid_lets_go_of_the_selection_and_makes_nothing() {
    // *"single click is freed up so it can be used freely for deselecting
    // things without a hassle."* The whole point of the change: with a clip
    // in hand, a plain press somewhere empty is still not a request for
    // anything.
    let mut arena = Arena::default();
    let clips = vec![a_clip(&mut arena, 0, 0, ClipKind::Notes)];
    let mut timeline = Timeline::new(view());
    press(&mut timeline, &clips, PPQN, 0);
    assert_eq!(timeline.selection(), &[clips[0].id]);

    let edits = press(&mut timeline, &clips, BAR * 3, 1);
    assert!(edits.is_empty(), "a plain press asked for {edits:?}");
    assert!(timeline.selection().is_empty(), "the press should deselect");
    // And the thing in hand is still in hand, for the Shift+press to come.
    assert_eq!(timeline.stamp_source(), Some(clips[0].id));
}

// ------------------------------------------------------------- stamping ---

#[test]
fn after_choosing_a_clip_a_shift_press_on_empty_grid_stamps_a_copy_of_it() {
    let mut arena = Arena::default();
    let clips = vec![a_clip(&mut arena, 0, 0, ClipKind::Notes)];
    let mut timeline = Timeline::new(view());

    // Choose it: a click on the block.
    press(&mut timeline, &clips, PPQN, 0);
    assert_eq!(timeline.selection(), &[clips[0].id]);

    // Then Shift+press somewhere empty, on another row.
    let edits = shift_press(&mut timeline, &clips, BAR * 3, 1);
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
    let edits = shift_press(&mut timeline, &clips, BAR * 3 + PPQN / 3, 0);
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

        let edits = shift_press(&mut timeline, &clips, BAR * 2, 1);
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

    let edits = shift_press(&mut timeline, &clips, BAR * 4, 1);
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
fn the_copy_just_stamped_is_what_the_next_shift_press_stamps() {
    // Which is what makes a row of Shift+clicks a row of the same clip: the
    // copy becomes the selection, as a duplicate's does, and the selection
    // is what a press stamps.
    let mut arena = Arena::default();
    let mut clips = vec![a_clip(&mut arena, 0, 0, ClipKind::Notes)];
    let mut timeline = Timeline::new(view());
    press(&mut timeline, &clips, PPQN, 0);
    shift_press(&mut timeline, &clips, BAR * 2, 0);

    // The host made the copy and says so.
    let copy = a_clip(&mut arena, 0, BAR * 2, ClipKind::Notes);
    timeline.clips_inserted(vec![copy.id]);
    clips.push(copy);
    assert_eq!(timeline.selection(), &[clips[1].id]);

    let edits = shift_press(&mut timeline, &clips, BAR * 4, 0);
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
fn a_shift_press_with_nothing_in_hand_makes_nothing() {
    // Nothing chosen is nothing to copy. A blank clip is what a double-click
    // is for, and a Shift+press that quietly drew one instead would be a
    // copy of something you never picked.
    let mut timeline = Timeline::new(view());
    let edits = shift_press(&mut timeline, &[], BAR * 2, 0);
    assert!(edits.is_empty(), "asked for {edits:?}");
}

#[test]
fn a_clip_that_has_gone_since_it_was_chosen_is_not_stamped() {
    // Deleted from another window, or undone away: there is nothing to copy,
    // so the press makes nothing rather than guessing a blank.
    let mut arena = Arena::default();
    let clips = vec![a_clip(&mut arena, 0, 0, ClipKind::Notes)];
    let mut timeline = Timeline::new(view());
    press(&mut timeline, &clips, PPQN, 0);

    let edits = shift_press(&mut timeline, &[], BAR * 2, 0);
    assert!(edits.is_empty(), "asked for {edits:?}");
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

    let edits = shift_press(&mut timeline, &clips, BAR * 2, 1);
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
fn a_double_press_on_empty_grid_draws_a_blank_clip() {
    // The first press of the pair made nothing, so there is nothing to take
    // back: the second simply asks for the blank clip.
    let mut arena = Arena::default();
    let clips = vec![a_clip(&mut arena, 0, 0, ClipKind::Notes)];
    let mut timeline = Timeline::new(view());
    press(&mut timeline, &clips, PPQN, 0);

    let first = press(&mut timeline, &clips, BAR * 2, 1);
    assert!(first.is_empty(), "the first press asked for {first:?}");
    let second = double_press(&mut timeline, &clips, BAR * 2, 1);
    assert_eq!(
        second,
        vec![ArrangeEdit::Add {
            lane: 1,
            start: BAR * 2
        }]
    );
}

#[test]
fn a_double_press_with_nothing_in_hand_draws_a_clip() {
    let mut timeline = Timeline::new(view());
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
fn a_shift_double_press_stamps_once_and_does_not_also_draw_a_blank() {
    // Two quick Shift+clicks in one place: the first stamped a copy, and the
    // second press of the pair must not put a blank clip on top of it. One
    // gesture, one clip.
    let mut arena = Arena::default();
    let mut clips = vec![a_clip(&mut arena, 0, 0, ClipKind::Notes)];
    let mut timeline = Timeline::new(view());
    press(&mut timeline, &clips, PPQN, 0);
    timeline.set_modifiers(Modifiers {
        shift: true,
        ..Modifiers::default()
    });
    let first = press(&mut timeline, &clips, BAR * 2, 0);
    assert!(matches!(first[..], [ArrangeEdit::Stamp { .. }]));
    let copy = a_clip(&mut arena, 0, BAR * 2, ClipKind::Notes);
    timeline.clips_inserted(vec![copy.id]);
    clips.push(copy);
    let second = double_press(&mut timeline, &clips, BAR * 2, 0);
    assert!(second.is_empty(), "the second press asked for {second:?}");
}
