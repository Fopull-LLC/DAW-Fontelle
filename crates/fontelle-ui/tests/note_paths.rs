//! Drawing a note's **path** in the roll (`docs/note-paths-plan.md` §3).
//!
//! Ty: *"when dragging out a note, you can press the s key to place a point
//! there so the note extends to that point, then past that point it becomes a
//! slide note, sliding to wherever you decide to place the second part of the
//! note ... id want to be able to make a note slide as many times as i
//! want."*
//!
//! The roll is driven the way the window drives it: press, drag, the S
//! action ([`PianoRoll::path_point`]), release — with a small host below
//! that applies what the roll asks for, so each step sees the last land.

use fontelle_model::{Arena, Note, PathPoint};
use fontelle_types::{NoteId, PPQN, Tick};
use fontelle_ui::canvas::{
    DrawDrag, MouseButton, NotePart, PianoRoll, RollEdit, RollHit, RollView, SnapDivision,
    hit_test, key_to_y, tick_to_x,
};
use fontelle_ui::layout::Rect;

fn view() -> RollView {
    RollView {
        scroll_tick: 0,
        top_key: 72,
        key_offset: 0.0,
        pixels_per_tick: 0.25,
        key_height: 12.0,
        snap: SnapDivision::Step,
    }
}

fn grid() -> Rect {
    Rect::new(60.0, 30.0, 1600.0, 400.0)
}

fn point(at: Tick, offset: i8) -> PathPoint {
    PathPoint { at, offset }
}

/// The middle of the cell at `tick`, `key`.
fn at(tick: Tick, key: u8) -> (f32, f32) {
    let view = view();
    (
        tick_to_x(&view, grid(), tick) + 1.0,
        key_to_y(&view, grid(), key) + view.key_height / 2.0,
    )
}

/// The window's half of the conversation: what `fontelle-app` does with each
/// edit, minus the history.
struct Host {
    notes: Arena<NoteId, Note>,
    roll: PianoRoll,
}

impl Host {
    fn new(drag: DrawDrag) -> Self {
        let mut roll = PianoRoll::new(view());
        roll.draw_drag = drag;
        Self {
            notes: Arena::default(),
            roll,
        }
    }

    fn apply(&mut self, edits: Vec<RollEdit>) {
        for edit in edits {
            match edit {
                RollEdit::Add { note } => {
                    let id = self.notes.insert(note);
                    self.roll.note_added(id);
                }
                RollEdit::Move {
                    ids,
                    tick_delta,
                    key_delta,
                } => {
                    for id in ids {
                        let n = &mut self.notes[id];
                        n.start += tick_delta;
                        n.key = (i16::from(n.key) + key_delta) as u8;
                    }
                }
                RollEdit::Resize { ids, tick_delta } => {
                    for id in ids {
                        self.notes[id].length += tick_delta;
                    }
                }
                RollEdit::Shape { id, length, path } => {
                    let n = &mut self.notes[id];
                    n.length = length;
                    n.path = path;
                }
                other => panic!("not expected here: {other:?}"),
            }
        }
    }

    fn press(&mut self, tick: Tick, key: u8) {
        let (x, y) = at(tick, key);
        let edits = self
            .roll
            .press(MouseButton::Left, x, y, grid(), &self.notes, 4);
        self.apply(edits);
    }

    fn drag(&mut self, tick: Tick, key: u8) {
        let (x, y) = at(tick, key);
        let edits = self.roll.drag(x, y, grid(), &self.notes, 4);
        self.apply(edits);
    }

    fn s(&mut self) {
        let edits = self.roll.path_point(&self.notes);
        self.apply(edits);
    }

    fn backspace(&mut self) {
        let edits = self.roll.path_point_back(&self.notes);
        self.apply(edits);
    }

    fn release(&mut self, tick: Tick, key: u8) {
        let (x, y) = at(tick, key);
        let edits = self.roll.release_over(x, y, grid(), &self.notes);
        self.apply(edits);
    }

    fn double(&mut self, tick: Tick, key: u8) {
        let (x, y) = at(tick, key);
        let edits = self.roll.double_press(x, y, grid(), &self.notes, 4);
        self.apply(edits);
    }

    fn only(&self) -> &Note {
        assert_eq!(self.notes.len(), 1, "one note, however many slides");
        self.notes.values().next().unwrap()
    }
}

#[test]
fn hold_then_slide_dragging_out_the_end() {
    // Draw at C4, drag the end out a beat, S, then up a fifth over the next
    // beat, and let go.
    let mut host = Host::new(DrawDrag::Resize);
    host.press(0, 60);
    host.drag(PPQN, 60);
    assert!(
        host.roll.takes_path_points(),
        "a note being drawn takes points"
    );
    host.s();
    host.drag(PPQN * 2, 67);
    host.release(PPQN * 2, 67);

    let note = host.only();
    assert_eq!((note.start, note.key), (0, 60));
    assert_eq!(note.length, PPQN * 2, "the note ends where the drag let go");
    assert_eq!(note.path, vec![point(PPQN, 0), point(PPQN * 2, 7)]);
}

#[test]
fn the_default_drag_holds_to_the_notes_own_end_first() {
    // With the default, the drag after drawing carries the note, and the
    // pointer is somewhere on it when S comes — so the first point is the
    // note's end, and what follows slides from there.
    let mut host = Host::new(DrawDrag::Move);
    host.press(0, 60);
    let drawn = host.only().length;
    host.s();
    host.drag(drawn + PPQN, 64);
    host.release(drawn + PPQN, 64);

    let note = host.only();
    assert_eq!((note.start, note.key), (0, 60), "S stopped the note moving");
    assert_eq!(note.path, vec![point(drawn, 0), point(drawn + PPQN, 4)]);
    assert_eq!(note.length, drawn + PPQN);
}

#[test]
fn a_whole_melody_from_one_note() {
    let mut host = Host::new(DrawDrag::Resize);
    host.press(0, 60);
    host.drag(PPQN, 60);
    host.s();
    host.drag(PPQN * 2, 62);
    host.s();
    host.drag(PPQN * 3, 62);
    host.s();
    host.drag(PPQN * 4, 59);
    host.s();
    host.drag(PPQN * 5, 64);
    host.release(PPQN * 5, 64);

    let note = host.only();
    assert_eq!(
        note.path,
        vec![
            point(PPQN, 0),
            point(PPQN * 2, 2),
            point(PPQN * 3, 2),
            point(PPQN * 4, -1),
            point(PPQN * 5, 4),
        ]
    );
    assert_eq!(note.length, PPQN * 5);
}

#[test]
fn a_hold_after_the_last_slide_is_the_note_running_on() {
    // Slide up, S, carry on flat: the flat end is the note's length, not
    // another point.
    let mut host = Host::new(DrawDrag::Resize);
    host.press(0, 60);
    host.drag(PPQN, 60);
    host.s();
    host.drag(PPQN * 2, 67);
    host.s();
    host.drag(PPQN * 4, 67);
    host.release(PPQN * 4, 67);

    let note = host.only();
    assert_eq!(note.path, vec![point(PPQN, 0), point(PPQN * 2, 7)]);
    assert_eq!(note.length, PPQN * 4);
}

#[test]
fn s_and_letting_go_without_moving_leaves_a_plain_note() {
    let mut host = Host::new(DrawDrag::Resize);
    host.press(0, 60);
    host.drag(PPQN, 60);
    host.s();
    host.release(PPQN, 60);
    let note = host.only();
    assert!(note.path.is_empty(), "{:?}", note.path);
    assert_eq!(note.length, PPQN);
}

#[test]
fn backspace_takes_the_last_point_back() {
    let mut host = Host::new(DrawDrag::Resize);
    host.press(0, 60);
    host.drag(PPQN, 60);
    host.s();
    host.drag(PPQN * 2, 67);
    host.s();
    host.drag(PPQN * 3, 72);
    host.backspace();
    // The second point is gone: the live segment runs from the first point
    // to the pointer again.
    host.drag(PPQN * 3, 65);
    host.release(PPQN * 3, 65);

    let note = host.only();
    assert_eq!(note.path, vec![point(PPQN, 0), point(PPQN * 3, 5)]);
}

#[test]
fn the_pointer_behind_the_last_point_cannot_take_the_path_back_in_time() {
    let mut host = Host::new(DrawDrag::Resize);
    host.press(0, 60);
    host.drag(PPQN * 2, 60);
    host.s();
    host.drag(PPQN, 64);
    let note = host.only();
    assert!(
        note.path.windows(2).all(|pair| pair[0].at <= pair[1].at),
        "{:?}",
        note.path
    );
    assert!(note.length >= PPQN * 2);
}

#[test]
fn s_does_nothing_when_no_note_is_being_drawn() {
    let mut host = Host::new(DrawDrag::Resize);
    assert!(!host.roll.takes_path_points());
    assert!(host.roll.path_point(&host.notes).is_empty());
    assert!(host.roll.path_point_back(&host.notes).is_empty());
}

#[test]
fn s_while_moving_an_existing_note_does_nothing() {
    // Dragging a note that was already there moves it; a point there would
    // be a surprise. Its end's resize is where a path is extended.
    let mut host = Host::new(DrawDrag::Move);
    host.notes.insert({
        let mut n = Note {
            start: 0,
            length: PPQN * 2,
            key: 60,
            velocity: 100,
            pan: 0,
            fine_pitch: 0,
            release: 0,
            mod_x: 0,
            mod_y: 0,
            slide: false,
            path: Vec::new(),
            channel: None,
        };
        n.start = 0;
        n
    });
    host.press(PPQN / 2, 60);
    assert!(!host.roll.takes_path_points());
}

#[test]
fn a_path_note_is_hit_along_its_slide_and_not_on_the_row_it_left() {
    let mut notes = Arena::default();
    let id = notes.insert(Note {
        start: 0,
        length: PPQN * 4,
        key: 60,
        velocity: 100,
        pan: 0,
        fine_pitch: 0,
        release: 0,
        mod_x: 0,
        mod_y: 0,
        slide: false,
        path: vec![point(PPQN, 0), point(PPQN * 2, 8)],
        channel: None,
    });
    let view = view();
    let hit = |tick, key| {
        let (x, y) = at(tick, key);
        hit_test(&view, grid(), &notes, x, y)
    };
    assert!(matches!(hit(PPQN / 2, 60), RollHit::Note(n, _) if n == id));
    assert!(
        matches!(hit(PPQN + PPQN / 2, 64), RollHit::Note(n, _) if n == id),
        "halfway up the slide"
    );
    assert!(
        matches!(hit(PPQN * 3, 68), RollHit::Note(n, _) if n == id),
        "where it landed"
    );
    assert!(
        matches!(hit(PPQN * 3, 60), RollHit::Empty { .. }),
        "the row it left is empty after the slide"
    );
}

/// A note with a hold and a slide, already in the document, selected.
fn shaped() -> Host {
    let mut host = Host::new(DrawDrag::Move);
    let id = host.notes.insert(Note {
        start: 0,
        length: PPQN * 3,
        key: 60,
        velocity: 100,
        pan: 0,
        fine_pitch: 0,
        release: 0,
        mod_x: 0,
        mod_y: 0,
        slide: false,
        path: vec![point(PPQN, 0), point(PPQN * 2, 7)],
        channel: None,
    });
    host.roll.select(vec![id]);
    host
}

#[test]
fn a_selected_notes_points_can_be_caught() {
    let host = shaped();
    let view = view();
    let (x, y) = at(PPQN * 2, 67);
    assert!(matches!(
        hit_test(&view, grid(), &host.notes, x, y),
        RollHit::Note(_, NotePart::Point(1))
    ));
}

#[test]
fn dragging_a_point_moves_it_and_the_lines_either_side_follow() {
    let mut host = shaped();
    host.press(PPQN * 2, 67);
    host.drag(PPQN * 2 + PPQN / 2, 69);
    host.release(PPQN * 2 + PPQN / 2, 69);
    let note = host.only();
    assert_eq!(
        note.path,
        vec![point(PPQN, 0), point(PPQN * 2 + PPQN / 2, 9)]
    );
    assert_eq!((note.start, note.key), (0, 60), "the note itself stays");
}

#[test]
fn a_point_cannot_be_dragged_past_its_neighbours() {
    let mut host = shaped();
    host.press(PPQN * 2, 67);
    host.drag(PPQN / 4, 67);
    let note = host.only();
    assert!(note.path[1].at >= note.path[0].at, "{:?}", note.path);
}

#[test]
fn double_clicking_a_point_takes_it_out() {
    let mut host = shaped();
    host.double(PPQN * 2, 67);
    assert_eq!(host.only().path, vec![point(PPQN, 0)]);
}

#[test]
fn double_clicking_a_note_puts_a_point_there() {
    // A plain note's way in: a point, dragged up, is a slide.
    let mut host = Host::new(DrawDrag::Move);
    let id = host.notes.insert(Note {
        start: 0,
        length: PPQN * 4,
        key: 60,
        velocity: 100,
        pan: 0,
        fine_pitch: 0,
        release: 0,
        mod_x: 0,
        mod_y: 0,
        slide: false,
        path: Vec::new(),
        channel: None,
    });
    host.roll.select(vec![id]);
    host.double(PPQN * 2, 60);
    assert_eq!(host.only().path, vec![point(PPQN * 2, 0)]);
}

/// A note sliding from C4 up a minor sixth over its second beat.
fn sliding() -> (Arena<NoteId, Note>, NoteId) {
    let mut notes = Arena::default();
    let id = notes.insert(Note {
        start: 0,
        length: PPQN * 4,
        key: 60,
        velocity: 100,
        pan: 0,
        fine_pitch: 0,
        release: 0,
        mod_x: 0,
        mod_y: 0,
        slide: false,
        path: vec![point(PPQN, 0), point(PPQN * 2, 8)],
        channel: None,
    });
    (notes, id)
}

#[test]
fn the_cut_tool_cuts_a_slide_where_the_stroke_crosses_its_line() {
    let (notes, id) = sliding();
    let (x, top) = at(PPQN + PPQN / 2, 70);
    let (_, bottom) = at(PPQN + PPQN / 2, 56);
    let cuts = fontelle_ui::canvas::slice_cuts(&view(), grid(), &notes, (x, top), (x, bottom));
    assert_eq!(cuts.len(), 1, "{cuts:?}");
    assert_eq!(cuts[0].0, id);
    assert!(
        (cuts[0].1 - (PPQN + PPQN / 2)).abs() <= 8,
        "cut where the stroke is: {}",
        cuts[0].1
    );
}

#[test]
fn a_cut_over_the_row_a_slide_left_misses() {
    let (notes, _) = sliding();
    let (x, top) = at(PPQN * 3, 61);
    let (_, bottom) = at(PPQN * 3, 59);
    assert!(
        fontelle_ui::canvas::slice_cuts(&view(), grid(), &notes, (x, top), (x, bottom)).is_empty()
    );
}

#[test]
fn a_box_around_where_a_slide_landed_selects_the_note() {
    let (notes, id) = sliding();
    let mut roll = PianoRoll::new(view());
    roll.tool = fontelle_ui::canvas::Tool::Select;
    let (x0, y0) = at(PPQN * 3, 70);
    let (x1, y1) = at(PPQN * 3 + PPQN / 2, 66);
    roll.press(MouseButton::Left, x0, y0, grid(), &notes, 4);
    roll.drag(x1, y1, grid(), &notes, 4);
    roll.release_over(x1, y1, grid(), &notes);
    assert_eq!(roll.selection(), &[id]);

    // And a box over the row it left, after it left it, does not.
    let mut roll = PianoRoll::new(view());
    roll.tool = fontelle_ui::canvas::Tool::Select;
    let (x0, y0) = at(PPQN * 3, 61);
    let (x1, y1) = at(PPQN * 3 + PPQN / 2, 59);
    roll.press(MouseButton::Left, x0, y0, grid(), &notes, 4);
    roll.drag(x1, y1, grid(), &notes, 4);
    roll.release_over(x1, y1, grid(), &notes);
    assert!(roll.selection().is_empty());
}

/// Ty, from using it: dragging a drawn note's end back out and pressing S
/// *"just changes the snapping grid instead of making a new slide note
/// point"*. The end of a path is a point, so grabbing it there is a point
/// drag — and S on the **last** point carries on drawing the path from it.
#[test]
fn s_while_dragging_a_paths_last_point_carries_on_drawing() {
    let mut host = Host::new(DrawDrag::Resize);
    host.press(0, 60);
    host.drag(PPQN, 60);
    host.s();
    host.drag(PPQN * 2, 67);
    host.release(PPQN * 2, 67);
    assert_eq!(host.only().path, vec![point(PPQN, 0), point(PPQN * 2, 7)]);

    // Grab the end again and pull it out: the point moves, so the slide now
    // lands a beat later. S fixes it there and the pointer leads on.
    host.press(PPQN * 2, 67);
    host.drag(PPQN * 3, 67);
    assert!(host.roll.takes_path_points(), "the last point takes S");
    host.s();
    host.drag(PPQN * 4, 62);
    host.release(PPQN * 4, 62);

    let note = host.only();
    assert_eq!(
        note.path,
        vec![point(PPQN, 0), point(PPQN * 3, 7), point(PPQN * 4, 2)]
    );
    assert_eq!(note.length, PPQN * 4);

    // S straight after grabbing the end, before moving, keeps the slide
    // where it was and holds on from it.
    host.press(PPQN * 4, 62);
    host.s();
    host.drag(PPQN * 5, 62);
    host.release(PPQN * 5, 62);
    let note = host.only();
    assert_eq!(note.path.last(), Some(&point(PPQN * 4, 2)));
    assert_eq!(note.length, PPQN * 5, "held on to the pointer");
}

#[test]
fn s_while_dragging_a_point_in_the_middle_does_nothing() {
    let mut host = shaped();
    host.press(PPQN, 60);
    assert!(!host.roll.takes_path_points());
}

/// The next note copies the last one's length (FL's rule), but a sliding
/// note's length is the whole of its slides. What a note drawn after one
/// copies is its **first hold** — the note part — so a second slide drawn
/// straight after the first starts sliding as soon, rather than holding
/// for the whole of the last one's melody first.
#[test]
fn a_note_drawn_after_a_slide_is_as_long_as_its_first_hold() {
    let mut host = Host::new(DrawDrag::Move);
    host.press(0, 60);
    let first = host.only().length;
    host.s();
    host.drag(first + PPQN * 2, 67);
    host.release(first + PPQN * 2, 67);

    host.press(PPQN * 5, 55);
    let second = host
        .notes
        .values()
        .find(|n| n.start == PPQN * 5)
        .unwrap_or_else(|| panic!("{:?}", host.notes.values().collect::<Vec<_>>()))
        .clone();
    assert_eq!(second.length, first, "the hold, not the whole slide");
    assert!(second.path.is_empty());
}
