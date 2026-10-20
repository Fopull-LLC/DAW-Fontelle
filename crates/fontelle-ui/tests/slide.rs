//! FL-style slide notes — the triangles — can no longer be made.
//!
//! Ty, 2026-10-01: *"currently i dont like slide notes, they have all the
//! same limitations of fl studio slide notes."* A note slides by its own path
//! now (S while drawing — `tests/note_paths.rs`), and the FL chip came off
//! the toolbar then. The `A` key stayed, and so did a back door: clicking an
//! old slide note made it the template, so every note drawn after it was a
//! triangle too. Ty again, testing on Windows: *"you can still make triangle
//! notes when we have a new system for slide notes already triangle notes
//! should not exist."*
//!
//! So nothing in the roll makes one. Slide notes **already in a song** still
//! load, play and draw exactly as they did — `Note::slide` is in the file and
//! on the wire, both append-only, and there is no faithful conversion to a
//! path: an FL slide bends *whatever the channel is playing* to its key, which
//! may be a chord, a note in another clip, or nothing at all, while a path
//! belongs to one note. Converting would change how old songs sound; keeping
//! them as they are changes nothing anybody wrote.

use fontelle_model::{Arena, Note};
use fontelle_types::{NoteId, PPQN, Tick};
use fontelle_ui::canvas::{
    Action, Keymap, MouseButton, PianoRoll, RollEdit, RollView, SnapDivision, Tool, key_to_y,
    tick_to_x,
};
use fontelle_ui::layout::Rect;

fn note(start: Tick, key: u8, slide: bool) -> Note {
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
        slide,
        path: Vec::new(),
        channel: None,
        lyric: String::new(),
        voice: None,
        muted: false,
    }
}

fn view() -> RollView {
    RollView {
        scroll_tick: 0,
        top_key: 72,
        key_offset: 0.0,
        pixels_per_tick: 0.25,
        key_height: 14.0,
        snap: SnapDivision::Step,
    }
}

fn grid() -> Rect {
    Rect::new(60.0, 30.0, 800.0, 400.0)
}

fn at(view: &RollView, tick: Tick, key: u8) -> (f32, f32) {
    (
        tick_to_x(view, grid(), tick) + 3.0,
        key_to_y(view, grid(), key) + 3.0,
    )
}

/// What a press on empty grid draws, after first clicking the old slide note.
fn drawn_after_clicking_a_slide(tool: Tool) -> Vec<Note> {
    let mut notes: Arena<NoteId, Note> = Arena::default();
    notes.insert(note(0, 60, true));
    let mut roll = PianoRoll::new(view());
    roll.tool = tool;
    // Click the triangle: it becomes the template, as any clicked note does.
    let (x, y) = at(&roll.view, 0, 60);
    roll.press(MouseButton::Left, x, y, grid(), &notes);
    roll.release();
    // Then draw somewhere empty.
    let (x, y) = at(&roll.view, PPQN * 2, 64);
    let edits = roll.press(MouseButton::Left, x, y, grid(), &notes);
    roll.release();
    edits
        .into_iter()
        .filter_map(|edit| match edit {
            RollEdit::Add { note } => Some(note),
            _ => None,
        })
        .collect()
}

#[test]
fn a_note_drawn_after_clicking_an_old_slide_note_is_an_ordinary_note() {
    let drawn = drawn_after_clicking_a_slide(Tool::Draw);
    assert_eq!(drawn.len(), 1);
    assert!(!drawn[0].slide, "the pencil drew a triangle");
}

#[test]
fn nor_does_the_brush_paint_them() {
    let drawn = drawn_after_clicking_a_slide(Tool::Paint);
    assert_eq!(drawn.len(), 1);
    assert!(!drawn[0].slide, "the brush painted a triangle");
}

#[test]
fn a_template_handed_in_with_slide_on_draws_without_it() {
    // A session restoring the roll's template from before this change.
    let mut roll = PianoRoll::new(view());
    roll.set_template(note(0, 60, true));
    assert!(!roll.template().slide);
}

#[test]
fn no_key_marks_a_slide_note_any_more() {
    assert!(
        Action::ALL.iter().all(|action| action.id() != "slide"),
        "an action still toggles FL slide notes"
    );
}

#[test]
fn a_settings_file_that_rebound_the_old_slide_key_still_loads_the_rest() {
    // Saved by a build that had the action: the line is passed over, and the
    // person's other bindings are kept.
    let keymap = Keymap::with_overrides(&[
        ("slide".to_string(), "Q".to_string()),
        ("draw-tool".to_string(), "Y".to_string()),
    ]);
    assert_eq!(keymap.label(Action::DrawTool), "Y");
}
