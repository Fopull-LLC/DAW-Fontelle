//! The cut tool: a line drawn across the grid, and where it crosses.
//!
//! Reported from using the window: *"We need a cut tool in the piano roll (C)
//! that works basically the same as the FL Studio cut tool."* `Tool::Slice`
//! has been in the tool list since the roll was written, was bound to `5`, and
//! did nothing.
//!
//! # Why a line and not a click
//!
//! Because that is what makes it a *tool* rather than a context menu. One
//! stroke across a chord cuts every note it crosses, and a **diagonal** stroke
//! cuts them at different times — which is a musical gesture rather than an
//! artefact, and the reason the crossing point is computed per note instead of
//! one tick being used for all of them.
//!
//! The rule, in one sentence: a note is cut where the line crosses **the
//! middle of its own row**, and only if that lands strictly inside the note.

use fontelle_model::{Arena, Note};
use fontelle_types::{NoteId, PPQN, Tick};
use fontelle_ui::canvas::{RollView, SnapDivision, key_to_y, roll_layout, slice_cuts, tick_to_x};
use fontelle_ui::layout::Rect;
use fontelle_ui::theme::Theme;

fn view() -> RollView {
    RollView {
        scroll_tick: 0,
        top_key: 72,
        pixels_per_tick: 0.25,
        key_height: 14.0,
        snap: SnapDivision::Step,
    }
}

fn grid() -> Rect {
    let m = Theme::dark_default().metrics;
    roll_layout(Rect::new(0.0, 0.0, 900.0, 500.0), &m, 0.0).grid
}

fn note(start: Tick, length: Tick, key: u8) -> Note {
    Note {
        start,
        length,
        key,
        velocity: 100,
        pan: 0,
        fine_pitch: 0,
        release: 0,
        mod_x: 0,
        mod_y: 0,
        slide: false,
        channel: None,
    }
}

/// The x of a tick, and the y of the middle of a key's row.
fn at(v: &RollView, tick: Tick, key: u8) -> (f32, f32) {
    (
        tick_to_x(v, grid(), tick),
        key_to_y(v, grid(), key) + v.key_height / 2.0,
    )
}

#[test]
fn a_line_straight_down_through_a_note_cuts_it_where_it_crossed() {
    let v = view();
    let mut notes = Arena::default();
    let id = notes.insert(note(0, PPQN * 4, 60));

    // From above the row to below it, at two beats in.
    let (x, y) = at(&v, PPQN * 2, 60);
    let cuts = slice_cuts(&v, grid(), &notes, (x, y - 40.0), (x, y + 40.0));

    assert_eq!(cuts.len(), 1);
    assert_eq!(cuts[0].0, id);
    assert!(
        (cuts[0].1 - PPQN * 2).abs() <= 2,
        "cut at {} rather than {}",
        cuts[0].1,
        PPQN * 2
    );
}

#[test]
fn a_line_that_misses_the_note_cuts_nothing() {
    let v = view();
    let mut notes: Arena<NoteId, Note> = Arena::default();
    notes.insert(note(PPQN * 4, PPQN * 4, 60));

    // Before it in time.
    let (x, y) = at(&v, PPQN, 60);
    assert!(slice_cuts(&v, grid(), &notes, (x, y - 40.0), (x, y + 40.0)).is_empty());

    // And on another row entirely.
    let (x, _) = at(&v, PPQN * 6, 60);
    let (_, y) = at(&v, 0, 48);
    assert!(slice_cuts(&v, grid(), &notes, (x, y - 5.0), (x, y + 5.0)).is_empty());
}

#[test]
fn a_line_across_a_chord_cuts_every_note_in_it() {
    let v = view();
    let mut notes = Arena::default();
    let a = notes.insert(note(0, PPQN * 4, 60));
    let b = notes.insert(note(0, PPQN * 4, 64));
    let c = notes.insert(note(0, PPQN * 4, 67));

    // One vertical stroke from above the top note to below the bottom one.
    let (x, top) = at(&v, PPQN * 2, 67);
    let (_, bottom) = at(&v, PPQN * 2, 60);
    let cuts = slice_cuts(&v, grid(), &notes, (x, top - 10.0), (x, bottom + 10.0));

    let mut ids: Vec<NoteId> = cuts.iter().map(|(id, _)| *id).collect();
    ids.sort();
    let mut expected = vec![a, b, c];
    expected.sort();
    assert_eq!(ids, expected);
    for (_, tick) in &cuts {
        assert!((tick - PPQN * 2).abs() <= 2, "cut at {tick}");
    }
}

#[test]
fn a_diagonal_stroke_cuts_each_note_at_its_own_time() {
    // The whole reason it is a line: the cuts are staggered, and that is the
    // gesture rather than a rounding error.
    let v = view();
    let mut notes = Arena::default();
    let low = notes.insert(note(0, PPQN * 8, 60));
    let high = notes.insert(note(0, PPQN * 8, 67));

    // Down and to the right: early on the top row, late on the bottom one.
    let (x0, y0) = at(&v, PPQN, 67);
    let (x1, y1) = at(&v, PPQN * 5, 60);
    let cuts = slice_cuts(&v, grid(), &notes, (x0, y0 - 10.0), (x1, y1 + 10.0));

    let of = |id| cuts.iter().find(|(n, _)| *n == id).map(|(_, t)| *t);
    let high_at = of(high).expect("the top note was crossed");
    let low_at = of(low).expect("and the bottom one");
    assert!(
        high_at < low_at,
        "a stroke going right should cut the top note earlier: {high_at} then {low_at}"
    );
}

#[test]
fn a_cut_landing_on_a_notes_own_edge_is_not_offered() {
    // A zero-length note is silence you can neither see nor select, so the
    // canvas does not ask for one — the command refuses too, and both is
    // right: this keeps the gesture from looking like it did something.
    let v = view();
    let mut notes: Arena<NoteId, Note> = Arena::default();
    notes.insert(note(PPQN * 2, PPQN * 2, 60));

    for tick in [PPQN * 2, PPQN * 4] {
        let (x, y) = at(&v, tick, 60);
        assert!(
            slice_cuts(&v, grid(), &notes, (x, y - 30.0), (x, y + 30.0)).is_empty(),
            "a cut at {tick} is on an edge"
        );
    }
}

#[test]
fn a_stroke_that_does_not_cross_the_row_does_not_cut_it() {
    // A line drawn *along* a row, inside the note's own rectangle, has no
    // crossing point — and picking one arbitrarily would make a horizontal
    // wiggle cut notes at a place nobody aimed at.
    let v = view();
    let mut notes: Arena<NoteId, Note> = Arena::default();
    notes.insert(note(0, PPQN * 8, 60));

    let (x0, y) = at(&v, PPQN, 60);
    let (x1, _) = at(&v, PPQN * 6, 60);
    assert!(slice_cuts(&v, grid(), &notes, (x0, y), (x1, y)).is_empty());
}

#[test]
fn a_stroke_of_no_length_cuts_nothing() {
    // A click with the tool selected is not a cut: it is somebody putting the
    // pointer down.
    let v = view();
    let mut notes: Arena<NoteId, Note> = Arena::default();
    notes.insert(note(0, PPQN * 4, 60));
    let (x, y) = at(&v, PPQN * 2, 60);
    assert!(slice_cuts(&v, grid(), &notes, (x, y), (x, y)).is_empty());
}

#[test]
fn each_note_is_cut_at_most_once_by_one_stroke() {
    // A stroke that wanders back over a row would otherwise offer two cuts for
    // one note, and the second would be measured against a length the first
    // has already changed.
    let v = view();
    let mut notes: Arena<NoteId, Note> = Arena::default();
    notes.insert(note(0, PPQN * 8, 60));

    let (x, y) = at(&v, PPQN * 2, 60);
    let cuts = slice_cuts(&v, grid(), &notes, (x, y - 30.0), (x + 4.0, y + 30.0));
    assert_eq!(cuts.len(), 1);
}

// ------------------------------------------------------------- the gesture ---

#[test]
fn the_tool_is_on_the_toolbar_and_answers_to_c() {
    use fontelle_ui::canvas::{RollControl, Tool, toolbar_hit, toolbar_layout};

    let m = Theme::dark_default().metrics;
    let bar = toolbar_layout(Rect::new(0.0, 0.0, 900.0, 26.0), &m);
    let chip = bar
        .items
        .iter()
        .find(|(c, _)| *c == RollControl::Tool(Tool::Slice))
        .map(|(_, r)| *r)
        .expect("the roll's toolbar has a cut tool");

    assert_eq!(
        toolbar_hit(&bar, chip.x + chip.width / 2.0, chip.y + chip.height / 2.0),
        Some(RollControl::Tool(Tool::Slice))
    );
    assert_eq!(RollControl::Tool(Tool::Slice).shortcut(), Some("C"));
    assert!(
        RollControl::Tool(Tool::Slice).icon().is_some(),
        "it draws a glyph like every other tool"
    );
}

#[test]
fn dragging_with_the_cut_tool_produces_a_slice_and_not_a_note() {
    use fontelle_ui::canvas::{Modifiers, MouseButton, PianoRoll, RollEdit, Tool};

    let v = view();
    let mut notes = Arena::default();
    let id = notes.insert(note(0, PPQN * 4, 60));

    let m = Theme::dark_default().metrics;
    let layout = roll_layout(Rect::new(0.0, 0.0, 900.0, 500.0), &m, 0.0);
    let mut roll = PianoRoll::new(v);
    roll.tool = Tool::Slice;
    roll.set_modifiers(Modifiers::default());

    let (x, y) = at(&v, PPQN * 2, 60);
    let edits = roll.press(MouseButton::Left, x, y - 40.0, layout.grid, &notes, 4);
    assert!(edits.is_empty(), "a cut happens on release, not on press");

    roll.drag(x, y + 40.0, layout.grid, &notes, 4);
    let edits = roll.release_over(x, y + 40.0, layout.grid, &notes);

    match edits.as_slice() {
        [RollEdit::Slice { cuts }] => {
            assert_eq!(cuts.len(), 1);
            assert_eq!(cuts[0].0, id);
        }
        other => panic!("expected one slice, got {other:?}"),
    }
}

#[test]
fn the_stroke_is_visible_while_it_is_being_drawn() {
    // A tool whose gesture leaves no mark on screen is one you have to aim
    // blind — which on a grid where notes are eight pixels apart is a guess.
    use fontelle_ui::canvas::{Modifiers, MouseButton, PianoRoll, Tool};

    let v = view();
    let notes: Arena<NoteId, Note> = Arena::default();
    let m = Theme::dark_default().metrics;
    let layout = roll_layout(Rect::new(0.0, 0.0, 900.0, 500.0), &m, 0.0);
    let mut roll = PianoRoll::new(v);
    roll.tool = Tool::Slice;
    roll.set_modifiers(Modifiers::default());

    assert_eq!(roll.slice_stroke(), None, "nothing is being drawn yet");
    roll.press(MouseButton::Left, 100.0, 100.0, layout.grid, &notes, 4);
    roll.drag(240.0, 180.0, layout.grid, &notes, 4);
    assert_eq!(roll.slice_stroke(), Some(((100.0, 100.0), (240.0, 180.0))));

    roll.release_over(240.0, 180.0, layout.grid, &notes);
    assert_eq!(roll.slice_stroke(), None, "and it is gone when let go");
}

/// And it is on screen while it is being drawn, which is a different claim.
///
/// Reported from using the window: *"the cut tool's visuals are often totally
/// invisible for me? still works though."* Both halves of that were true and
/// they are the same bug. The stroke was drawn correctly and the *cut* landed
/// correctly — but the window only repainted a drag when the drag was a
/// marquee, and a slice produces neither a marquee nor a document edit. So
/// nothing dirtied the panel and the last painted frame was the one from
/// before the stroke existed. The "often" is the giveaway: with the transport
/// rolling the playhead repaints the panel anyway, and the line appears.
///
/// A gesture that paints something the document does not know about has to say
/// so, and there is exactly one predicate for it rather than a list at each
/// call site — a list is what went stale when the cut tool was added.
#[test]
fn a_gesture_that_paints_its_own_mark_asks_for_the_frame_it_needs() {
    use fontelle_ui::canvas::{Modifiers, MouseButton, PianoRoll, Tool};

    let v = view();
    let notes: Arena<NoteId, Note> = Arena::default();
    let m = Theme::dark_default().metrics;
    let layout = roll_layout(Rect::new(0.0, 0.0, 900.0, 500.0), &m, 0.0);
    let mut roll = PianoRoll::new(v);
    roll.set_modifiers(Modifiers::default());

    assert!(
        !roll.draws_overlay(),
        "an idle roll paints nothing of its own"
    );

    roll.tool = Tool::Slice;
    roll.press(MouseButton::Left, 100.0, 100.0, layout.grid, &notes, 4);
    roll.drag(240.0, 180.0, layout.grid, &notes, 4);
    assert!(
        roll.draws_overlay(),
        "a half-drawn cut is on screen and nothing in the document says so, \
         so the window never asks for the frame that would show it"
    );

    roll.release_over(240.0, 180.0, layout.grid, &notes);
    assert!(!roll.draws_overlay(), "and it is gone when let go");

    // The marquee is the other one, and the only one the window used to know
    // about.
    roll.tool = Tool::Select;
    roll.press(MouseButton::Left, 100.0, 100.0, layout.grid, &notes, 4);
    roll.drag(240.0, 180.0, layout.grid, &notes, 4);
    assert!(roll.draws_overlay());
}

/// The arrangement has both of the same two gestures and had the same bug.
#[test]
fn the_arrangements_own_marks_ask_for_their_frames_too() {
    use fontelle_ui::canvas::{
        Modifiers, MouseButton, Timeline, TimelineTool, TimelineView, timeline_layout,
    };

    let m = Theme::dark_default().metrics;
    let l = timeline_layout(Rect::new(0.0, 0.0, 900.0, 300.0), &m);
    let mut timeline = Timeline::new(TimelineView::default());
    timeline.set_modifiers(Modifiers::default());
    assert!(!timeline.draws_overlay());

    timeline.set_tool(TimelineTool::Slice);
    let (x, y) = (l.grid.x + 40.0, l.grid.y + 20.0);
    timeline.press(MouseButton::Left, x, y, &l, &[], 4);
    timeline.drag(x + 60.0, y + 30.0, &l, &[], 4);
    assert!(
        timeline.draws_overlay(),
        "a half-drawn cut across the arrangement is on screen and nothing \
         asks for the frame that would show it"
    );

    timeline.release_over(x + 60.0, y + 30.0, &l, &[], 4);
    assert!(!timeline.draws_overlay());
}

// ------------------------------------------------------------ the marks ---
//
// > *"it would be nice when im cutting in the piano roll it drew the line
// > that cut the notes i was cutting with the cut tool to visualize it
// > cleanly."*
//
// The arrangement has had this since its blade was reported drawing the
// gesture rather than its consequence: a faint stroke for the hand, and a
// bright mark on every clip it will actually divide. The roll drew the
// stroke alone. The marks are worked out by the same call that will make the
// cuts, so what is shown and what lands cannot disagree.

/// One mark per note the stroke will cut, each across **its own** row at the
/// tick that note is cut on — which, on a diagonal, is a different x per row.
#[test]
fn the_blade_marks_every_note_it_will_cut_on_that_notes_own_row() {
    use fontelle_ui::canvas::note_marks as slice_marks;

    let v = view();
    let mut notes = Arena::default();
    let a = notes.insert(note(0, PPQN * 4, 60));
    let b = notes.insert(note(0, PPQN * 4, 64));
    let c = notes.insert(note(0, PPQN * 4, 67));

    // From two beats in above the top note to three beats in below the bottom.
    let (x0, y_top) = at(&v, PPQN * 2, 67);
    let (x1, y_bottom) = at(&v, PPQN * 3, 60);
    let from = (x0, y_top - 20.0);
    let to = (x1, y_bottom + 20.0);

    let cuts = slice_cuts(&v, grid(), &notes, from, to);
    assert_eq!(cuts.len(), 3, "the stroke crosses three notes");
    let marks = slice_marks(&v, grid(), &notes, from, to);
    assert_eq!(marks.len(), 3, "three cuts, three marks");

    for (id, tick) in cuts {
        let key = notes.get(id).unwrap().key;
        let row = key_to_y(&v, grid(), key);
        let want = tick_to_x(&v, grid(), tick);
        let mark = marks
            .iter()
            .find(|m| (m.y - row).abs() < 0.5)
            .unwrap_or_else(|| panic!("no mark on key {key}'s row"));
        let centre = mark.x + mark.width / 2.0;
        assert!(
            (centre - want).abs() < 1.5,
            "key {key}: the mark is at {centre} and the cut lands at {want}"
        );
        assert!(
            (mark.height - v.key_height).abs() < 0.5,
            "the mark spans the row, not the stroke"
        );
        assert!(mark.width >= 1.0 && mark.width <= 3.0, "a cut has no width");
    }
    let _ = (a, b, c);
}

/// A stroke that cuts nothing marks nothing: the marks and the cuts are the
/// same answer.
#[test]
fn a_stroke_that_cuts_nothing_marks_nothing_in_the_roll() {
    use fontelle_ui::canvas::note_marks as slice_marks;

    let v = view();
    let mut notes = Arena::default();
    notes.insert(note(0, PPQN * 4, 60));
    // Straight down through the row, but past the note's end.
    let (x, y) = at(&v, PPQN * 6, 60);
    let marks = slice_marks(&v, grid(), &notes, (x, y - 40.0), (x, y + 40.0));
    assert!(marks.is_empty(), "a stroke past the note marked something");
    // And a stroke along the row, which crosses no middle.
    let (x0, y) = at(&v, PPQN, 60);
    let (x1, _) = at(&v, PPQN * 3, 60);
    let marks = slice_marks(&v, grid(), &notes, (x0, y), (x1, y));
    assert!(marks.is_empty(), "a stroke along the row marked something");
}
