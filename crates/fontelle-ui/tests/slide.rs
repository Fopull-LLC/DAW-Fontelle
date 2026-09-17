//! Slide notes, from the roll's side (FL Studio's).
//!
//! Reported from using the window: *"I want you to expand the functionality of
//! the piano roll to encompass things like slide and portamento notes that
//! function similarly to FL Studio."*
//!
//! What a slide *is* lives in `fontelle-core/tests/glide.rs` — one voice, two
//! pitches, and the second arrived at rather than restarted — and what it
//! compiles to in `fontelle-sequencer/tests/looping.rs`. This is the half that
//! makes one: the chip that marks a selection, and the template that decides
//! what the next note you draw will be.

use fontelle_model::{Arena, Note};
use fontelle_types::{NoteId, PPQN, Tick};
use fontelle_ui::canvas::{PianoRoll, RollControl, RollEdit, RollView, SnapDivision};

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
        channel: None,
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

/// A roll over `notes`, with the first `selected` of them selected.
fn roll(notes: &Arena<NoteId, Note>, selected: &[NoteId]) -> PianoRoll {
    let mut roll = PianoRoll::new(view());
    roll.select(selected.to_vec());
    let _ = notes;
    roll
}

#[test]
fn a_fresh_roll_draws_ordinary_notes() {
    let roll = PianoRoll::new(view());
    assert!(!roll.drawing_slides());
}

#[test]
fn the_chip_with_nothing_selected_only_changes_what_you_will_draw() {
    // "Turn slide on, then draw three of them" is the gesture people try
    // first, and it has to work without selecting anything.
    let notes: Arena<NoteId, Note> = Arena::default();
    let mut roll = PianoRoll::new(view());

    let edits = roll.toggle_slide(&notes);
    assert!(edits.is_empty(), "nothing to edit: {edits:?}");
    assert!(roll.drawing_slides());

    let edits = roll.toggle_slide(&notes);
    assert!(edits.is_empty());
    assert!(!roll.drawing_slides(), "and it toggles back");
}

#[test]
fn the_chip_marks_the_selection() {
    let mut notes = Arena::default();
    let a = notes.insert(note(0, 60, false));
    let b = notes.insert(note(PPQN, 64, false));
    let mut roll = roll(&notes, &[a, b]);

    let edits = roll.toggle_slide(&notes);
    assert_eq!(
        edits,
        vec![RollEdit::SetSlide {
            ids: vec![a, b],
            slide: true
        }]
    );
}

#[test]
fn a_selection_that_all_slides_already_is_turned_off() {
    let mut notes = Arena::default();
    let a = notes.insert(note(0, 60, true));
    let b = notes.insert(note(PPQN, 64, true));
    let mut roll = roll(&notes, &[a, b]);

    let edits = roll.toggle_slide(&notes);
    assert_eq!(
        edits,
        vec![RollEdit::SetSlide {
            ids: vec![a, b],
            slide: false
        }]
    );
}

#[test]
fn a_mixed_selection_settles_on_slide_rather_than_flipping_each_note() {
    // The same rule the arrangement's Mute button follows. Flipping each note
    // against the others makes a mixed selection impossible to resolve: every
    // press swaps which half is which.
    let mut notes = Arena::default();
    let a = notes.insert(note(0, 60, true));
    let b = notes.insert(note(PPQN, 64, false));
    let mut roll = roll(&notes, &[a, b]);

    let edits = roll.toggle_slide(&notes);
    assert_eq!(
        edits,
        vec![RollEdit::SetSlide {
            ids: vec![a, b],
            slide: true
        }],
        "not all of it slides, so all of it does now"
    );
}

#[test]
fn marking_a_selection_also_sets_what_you_will_draw_next() {
    // Otherwise "make these slides, now draw another one" produces an
    // ordinary note in the middle of a run of slides.
    let mut notes = Arena::default();
    let a = notes.insert(note(0, 60, false));
    let mut roll = roll(&notes, &[a]);

    roll.toggle_slide(&notes);
    assert!(roll.drawing_slides());
}

#[test]
fn a_selection_of_notes_that_are_gone_is_no_edit() {
    // A selection can outlive the notes in it — an undo, or another panel
    // deleting them — and an edit naming ids the document does not have is a
    // command that fails at the far end.
    let notes: Arena<NoteId, Note> = Arena::default();
    let mut arena: Arena<NoteId, Note> = Arena::default();
    let ghost = arena.insert(note(0, 60, false));
    let mut roll = roll(&notes, &[ghost]);

    assert!(roll.toggle_slide(&notes).is_empty());
}

#[test]
fn the_toolbar_carries_the_slide_chip() {
    use fontelle_ui::canvas::{toolbar_hit, toolbar_layout};
    use fontelle_ui::layout::Rect;
    use fontelle_ui::theme::Theme;

    let m = Theme::dark_default().metrics;
    let bar = toolbar_layout(Rect::new(0.0, 0.0, 900.0, 26.0), &m);
    let slide = bar
        .items
        .iter()
        .find(|(control, _)| *control == RollControl::Slide)
        .map(|(_, rect)| *rect)
        .expect("the roll's toolbar has a slide chip");

    assert_eq!(
        toolbar_hit(
            &bar,
            slide.x + slide.width / 2.0,
            slide.y + slide.height / 2.0
        ),
        Some(RollControl::Slide)
    );
    assert!(
        RollControl::Slide.icon().is_some(),
        "it draws a glyph like every other verb on the bar"
    );
}
