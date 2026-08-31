//! Holding the right button erases whatever it is dragged over.
//!
//! Reported from using the window: *"when holding right click hovering over
//! anything should delete it, notes, arrangement clips, etc."*
//!
//! Both canvases already deleted on a right *press*, and that is the whole of
//! what they did: the button went down, one thing under it was removed, and
//! the gesture ended there. Sweeping across a bar of notes meant clicking
//! every one of them. Worse, a right press on empty grid left whatever gesture
//! was in `self.gesture` from before untouched, so the next pointer move
//! carried on with it.
//!
//! The shape of the fix is the shape every other gesture here has — a gesture
//! the press starts and the drag continues — and so is the trap it has to
//! avoid. See `roll_gestures.rs` and `arrange_gestures.rs`: a drag is asked
//! what it wants on *every* pointer move, the host applies the answer, and the
//! canvas is then asked again against the changed document. An erase that does
//! not notice the thing it just removed is gone asks for it again for ever.
//! Hence the "stationary pointer asks for nothing" assertions below, which are
//! this file's real content.

use fontelle_model::{Arena, Note};
use fontelle_types::{ClipId, NoteId, PPQN, Tick};
use fontelle_ui::canvas::{
    ArrangeEdit, MouseButton, PianoRoll, RollEdit, RollView, SnapDivision, Timeline, TimelineView,
    Tool, key_to_y, lane_to_y, roll_layout, tick_to_x, timeline_layout, timeline_tick_to_x,
};
use fontelle_ui::document::{ClipInfo, ClipKind};
use fontelle_ui::layout::Rect;
use fontelle_ui::theme::Theme;

// ------------------------------------------------------------ the roll ---

fn view() -> RollView {
    RollView {
        scroll_tick: 0,
        top_key: 72,
        pixels_per_tick: 0.25,
        key_height: 12.0,
        snap: SnapDivision::Step,
    }
}

fn grid() -> Rect {
    roll_layout(
        Rect::new(0.0, 0.0, 900.0, 520.0),
        &Theme::dark_default().metrics,
        0.0,
    )
    .grid
}

fn note(start: Tick, key: u8) -> Note {
    Note {
        start,
        length: PPQN,
        key,
        velocity: 100,
        pan: 0,
        fine_pitch: 0,
        release: 0,
        mod_x: 0,
        mod_y: 0,
        slide: false,
    }
}

/// Somewhere inside the block for `key` at `tick`.
fn at(v: &RollView, g: Rect, tick: Tick, key: u8) -> (f32, f32) {
    (
        tick_to_x(v, g, tick) + 4.0,
        key_to_y(v, g, key) + v.key_height / 2.0,
    )
}

/// What the window does with what the roll asks for. Removal only — anything
/// else arriving during an erase is itself the failure.
fn apply(notes: &mut Arena<NoteId, Note>, edits: &[RollEdit]) {
    for edit in edits {
        match edit {
            RollEdit::Remove(ids) => {
                for id in ids {
                    assert!(
                        notes.remove(*id).is_some(),
                        "asked to remove {id:?}, which is not there any more"
                    );
                }
            }
            other => panic!("an erase must ask only for removals, got {other:?}"),
        }
    }
}

#[test]
fn a_right_drag_erases_every_note_it_crosses() {
    let g = grid();
    let mut roll = PianoRoll::new(view());
    let mut notes = Arena::default();
    let keys = [60, 61, 62, 63];
    let ids: Vec<NoteId> = keys.iter().map(|k| notes.insert(note(0, *k))).collect();

    let (x, y) = at(&roll.view, g, 0, keys[0]);
    let first = roll.press(MouseButton::Right, x, y, g, &notes, 4);
    apply(&mut notes, &first);

    for key in &keys[1..] {
        let (x, y) = at(&roll.view, g, 0, *key);
        let edits = roll.drag(x, y, g, &notes, 4);
        apply(&mut notes, &edits);
    }

    assert!(
        notes.is_empty(),
        "the sweep left {} note(s) behind",
        notes.len()
    );
    for id in ids {
        assert!(notes.get(id).is_none());
    }
}

#[test]
fn a_right_drag_that_starts_on_empty_grid_still_erases_what_it_reaches() {
    let g = grid();
    let mut roll = PianoRoll::new(view());
    let mut notes = Arena::default();
    let id = notes.insert(note(0, 60));

    // Down on nothing, which is how a sweep across a sparse bar begins.
    let (x, y) = at(&roll.view, g, 0, 67);
    let first = roll.press(MouseButton::Right, x, y, g, &notes, 4);
    assert!(first.is_empty(), "there was nothing there to remove");

    let (x, y) = at(&roll.view, g, 0, 60);
    let edits = roll.drag(x, y, g, &notes, 4);
    apply(&mut notes, &edits);

    assert!(
        notes.get(id).is_none(),
        "the note the sweep reached is gone"
    );
}

#[test]
fn an_erase_held_still_over_bare_grid_asks_for_nothing() {
    let g = grid();
    let mut roll = PianoRoll::new(view());
    let mut notes = Arena::default();
    notes.insert(note(0, 60));

    let (x, y) = at(&roll.view, g, 0, 60);
    let first = roll.press(MouseButton::Right, x, y, g, &notes, 4);
    apply(&mut notes, &first);
    assert!(notes.is_empty());

    // The note is gone and the pointer has not moved. Asking again must not
    // ask for it again — `apply` panics on a second removal, and so would the
    // real history.
    for step in 0..5 {
        let again = roll.drag(x, y, g, &notes, 4);
        assert!(
            again.is_empty(),
            "step {step}: a stationary erase asked for {again:?}"
        );
    }
}

#[test]
fn a_right_drag_neither_moves_a_note_nor_draws_a_selection_box() {
    let g = grid();
    let mut roll = PianoRoll::new(view());
    let mut notes = Arena::default();
    notes.insert(note(PPQN * 4, 60));

    // Down on empty grid, then a long drag across it — the shape that would
    // otherwise be a marquee, or a move if it had caught a note.
    let (x, y) = at(&roll.view, g, 0, 64);
    roll.press(MouseButton::Right, x, y, g, &notes, 4);
    for step in 1..6 {
        let edits = roll.drag(x + step as f32 * 20.0, y + step as f32 * 6.0, g, &notes, 4);
        apply(&mut notes, &edits);
    }

    assert!(
        roll.marquee().is_none(),
        "the right button erases; it does not draw a selection box"
    );
    assert!(
        roll.selection().is_empty(),
        "and it selects nothing on the way"
    );
    assert_eq!(notes.len(), 1, "the note it never reached is untouched");
}

#[test]
fn the_delete_tool_sweeps_on_a_left_drag_the_same_way() {
    let g = grid();
    let mut roll = PianoRoll::new(view());
    roll.tool = Tool::Delete;
    let mut notes = Arena::default();
    for key in [60, 61, 62] {
        notes.insert(note(0, key));
    }

    let (x, y) = at(&roll.view, g, 0, 60);
    let first = roll.press(MouseButton::Left, x, y, g, &notes, 4);
    apply(&mut notes, &first);
    for key in [61, 62] {
        let (x, y) = at(&roll.view, g, 0, key);
        let edits = roll.drag(x, y, g, &notes, 4);
        apply(&mut notes, &edits);
    }

    assert!(
        notes.is_empty(),
        "the delete tool is the right button with the mouse held; it sweeps too"
    );
}

// ----------------------------------------------------- the arrangement ---

fn tl_layout() -> fontelle_ui::canvas::TimelineLayout {
    timeline_layout(
        Rect::new(0.0, 0.0, 900.0, 300.0),
        &Theme::dark_default().metrics,
    )
}

fn clips(items: &[(Tick, Tick, usize)]) -> Vec<ClipInfo> {
    let mut arena: Arena<ClipId, ()> = Arena::default();
    items
        .iter()
        .map(|(start, length, lane)| ClipInfo {
            id: arena.insert(()),
            lane: *lane,
            start: *start,
            length: *length,
            name: "Part".to_string(),
            muted: false,
            open: false,
            color: [0x4f, 0x8f, 0xd0, 0xff],
            loop_length: None,
            kind: ClipKind::Notes,
            curve: Vec::new(),
        })
        .collect()
}

fn apply_clips(clips: &mut Vec<ClipInfo>, edits: &[ArrangeEdit]) {
    for edit in edits {
        match edit {
            ArrangeEdit::Remove(ids) => {
                let before = clips.len();
                clips.retain(|c| !ids.contains(&c.id));
                assert_eq!(
                    clips.len() + ids.len(),
                    before,
                    "asked to remove a clip that is not there any more"
                );
            }
            other => panic!("an erase must ask only for removals, got {other:?}"),
        }
    }
}

fn clip_at(v: &TimelineView, l: &fontelle_ui::canvas::TimelineLayout, c: &ClipInfo) -> (f32, f32) {
    (
        timeline_tick_to_x(v, l.grid, c.start) + 4.0,
        lane_to_y(v, l.grid, c.lane) + v.lane_height / 2.0,
    )
}

#[test]
fn a_right_drag_erases_every_clip_it_crosses() {
    let l = tl_layout();
    let mut timeline = Timeline::new(TimelineView::default());
    let mut items = clips(&[(0, PPQN * 4, 0), (0, PPQN * 4, 1), (0, PPQN * 4, 2)]);

    let (x, y) = clip_at(&timeline.view, &l, &items[0]);
    let first = timeline.press(MouseButton::Right, x, y, &l, &items, 4);
    apply_clips(&mut items, &first);

    while let Some(next) = items.first().cloned() {
        let (x, y) = clip_at(&timeline.view, &l, &next);
        let edits = timeline.drag(x, y, &l, &items, 4);
        assert!(
            !edits.is_empty(),
            "the sweep reached a clip and passed over it"
        );
        apply_clips(&mut items, &edits);
    }

    assert!(items.is_empty());
}

#[test]
fn erasing_a_clip_does_not_open_it_in_the_roll() {
    let l = tl_layout();
    let mut timeline = Timeline::new(TimelineView::default());
    let items = clips(&[(0, PPQN * 4, 0)]);

    let (x, y) = clip_at(&timeline.view, &l, &items[0]);
    timeline.press(MouseButton::Right, x, y, &l, &items, 4);

    assert!(
        timeline.take_open().is_none(),
        "a clip being deleted is not a clip being edited"
    );
}

#[test]
fn an_arrangement_erase_held_still_asks_for_nothing() {
    let l = tl_layout();
    let mut timeline = Timeline::new(TimelineView::default());
    let mut items = clips(&[(0, PPQN * 4, 0)]);

    let (x, y) = clip_at(&timeline.view, &l, &items[0]);
    let first = timeline.press(MouseButton::Right, x, y, &l, &items, 4);
    apply_clips(&mut items, &first);
    assert!(items.is_empty());

    for step in 0..5 {
        let again = timeline.drag(x, y, &l, &items, 4);
        assert!(
            again.is_empty(),
            "step {step}: a stationary erase asked for {again:?}"
        );
    }
}
