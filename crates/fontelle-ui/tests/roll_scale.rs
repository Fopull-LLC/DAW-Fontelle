//! The piano roll's scale tool.
//!
//! > *"a scale tool so you can chose between any note and the mode or
//! > whatever and it will snap all of your notes to that scale and dim out
//! > all the lanes that arent in that scale."*
//!
//! Three things, each a pure function or a gesture here: which rows are
//! dimmed, which key each note fits to, and where a note being drawn or
//! dragged lands while a scale is on. The chooser is a menu, and its rows
//! are a function of what was typed.

use fontelle_model::{Arena, Note};
use fontelle_types::{KeyScale, NoteId, PPQN, SCALES, Tick};
use fontelle_ui::canvas::{
    CHOSEN_MARK, KeyStyle, Modifiers, MouseButton, PianoRoll, RollControl, RollEdit, RollScale,
    RollView, RowShade, ScaleMenuRow, SnapDivision, key_to_y, root_caption, root_menu, row_shade,
    scale_caption, scale_fit, scale_menu, tick_to_x, toolbar_layout,
};
use fontelle_ui::layout::Rect;
use fontelle_ui::theme::Theme;

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
    Rect::new(60.0, 30.0, 800.0, 400.0)
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
        path: Vec::new(),
        channel: None,
    }
}

fn at(tick: Tick, key: u8) -> (f32, f32) {
    (
        tick_to_x(&view(), grid(), tick) + 1.0,
        key_to_y(&view(), grid(), key) + 4.0,
    )
}

fn c_major() -> RollScale {
    RollScale::of(&KeyScale::new(0, "major")).unwrap()
}

// ------------------------------------------------------------ the rows ---

#[test]
fn out_of_scale_rows_are_dimmed_and_the_root_is_marked() {
    let scale = Some(c_major());
    assert_eq!(row_shade(60, true, KeyStyle::Piano, scale), RowShade::Root);
    assert_eq!(row_shade(62, true, KeyStyle::Piano, scale), RowShade::Plain);
    assert_eq!(
        row_shade(61, true, KeyStyle::Piano, scale),
        RowShade::OutOfScale
    );
    // A row that does not sound says so whatever the scale thinks.
    assert_eq!(row_shade(61, false, KeyStyle::Piano, scale), RowShade::Dead);
    assert_eq!(row_shade(60, false, KeyStyle::Piano, scale), RowShade::Dead);
}

#[test]
fn with_no_scale_the_rows_are_the_keyboard_they_always_were() {
    assert_eq!(
        row_shade(61, true, KeyStyle::Piano, None),
        RowShade::Accidental
    );
    assert_eq!(row_shade(60, true, KeyStyle::Piano, None), RowShade::Plain);
    assert_eq!(row_shade(61, true, KeyStyle::Names, None), RowShade::Plain);
    assert_eq!(row_shade(61, false, KeyStyle::Piano, None), RowShade::Dead);
}

// ------------------------------------------------------------- fitting ---

#[test]
fn fitting_moves_only_the_notes_that_are_out_and_each_by_its_own_amount() {
    let mut notes: Arena<NoteId, Note> = Arena::default();
    let c = notes.insert(note(0, 60));
    let c_sharp = notes.insert(note(PPQN, 61));
    let f_sharp = notes.insert(note(PPQN * 2, 66));
    let mut fitted = scale_fit(&notes, &[], c_major().mask);
    fitted.sort();
    let mut want = vec![(c_sharp, 60), (f_sharp, 65)];
    want.sort();
    assert_eq!(fitted, want, "C is left alone");
    let _ = c;

    // With a selection, only the selection.
    assert_eq!(
        scale_fit(&notes, &[f_sharp], c_major().mask),
        vec![(f_sharp, 65)]
    );
    // Already in the scale: nothing to do.
    let d_major = RollScale::of(&KeyScale::new(2, "major")).unwrap();
    assert!(scale_fit(&notes, &[c_sharp, f_sharp], d_major.mask).is_empty());
}

// ------------------------------------------------ drawing and dragging ---

#[test]
fn a_note_drawn_on_a_dimmed_row_lands_on_the_nearest_row_in_the_scale() {
    let mut roll = PianoRoll::new(view());
    roll.scale = Some(c_major());
    let notes: Arena<NoteId, Note> = Arena::default();
    let (x, y) = at(0, 66);
    let edits = roll.press(MouseButton::Left, x, y, grid(), &notes, 4);
    let key = edits.iter().find_map(|e| match e {
        RollEdit::Add { note } => Some(note.key),
        _ => None,
    });
    assert_eq!(key, Some(65));
}

#[test]
fn alt_draws_exactly_where_it_points() {
    let mut roll = PianoRoll::new(view());
    roll.scale = Some(c_major());
    roll.set_modifiers(Modifiers {
        alt: true,
        ..Modifiers::default()
    });
    let notes: Arena<NoteId, Note> = Arena::default();
    let (x, y) = at(0, 66);
    let edits = roll.press(MouseButton::Left, x, y, grid(), &notes, 4);
    assert!(
        edits
            .iter()
            .any(|e| matches!(e, RollEdit::Add { note } if note.key == 66)),
        "{edits:?}"
    );
}

#[test]
fn a_note_dragged_up_and_down_steps_through_the_scale() {
    let mut roll = PianoRoll::new(view());
    roll.scale = Some(c_major());
    let mut notes: Arena<NoteId, Note> = Arena::default();
    let e = notes.insert(note(0, 64));
    let (x, y) = at(PPQN / 2, 64);
    roll.press(MouseButton::Left, x, y, grid(), &notes, 4);
    assert!(roll.selection().contains(&e));

    let moved = |edits: &[RollEdit]| -> i16 {
        edits
            .iter()
            .map(|e| match e {
                RollEdit::Move { key_delta, .. } => *key_delta,
                _ => 0,
            })
            .sum()
    };
    // One row up from E is F, in the scale: it goes.
    let (_, up_one) = at(PPQN / 2, 65);
    assert_eq!(moved(&roll.drag(x, up_one, grid(), &notes, 4)), 1);
    // One more is F#, out: it stays on F.
    let (_, up_two) = at(PPQN / 2, 66);
    assert_eq!(moved(&roll.drag(x, up_two, grid(), &notes, 4)), 0);
    // And G is next.
    let (_, up_three) = at(PPQN / 2, 67);
    assert_eq!(moved(&roll.drag(x, up_three, grid(), &notes, 4)), 2);
}

// --------------------------------------------------------- the chips ---

#[test]
fn the_toolbar_has_a_scale_chip_and_a_root_chip() {
    let l = toolbar_layout(
        Rect::new(0.0, 0.0, 1200.0, 28.0),
        &Theme::dark_default().metrics,
    );
    for control in [RollControl::Scale, RollControl::Root] {
        let rect = l
            .items
            .iter()
            .find(|(c, _)| *c == control)
            .map(|(_, r)| *r)
            .unwrap_or_else(|| panic!("{control:?} is on the toolbar"));
        assert!(rect.width >= 30.0 && rect.right() <= 1200.0, "{rect:?}");
        assert!(control.tip().is_some(), "{control:?} explains itself");
    }
    assert_eq!(scale_caption(None), "scale \u{25be}");
    assert_eq!(
        scale_caption(Some(&KeyScale::new(9, "natural-minor"))),
        "natural minor \u{25be}"
    );
    // A long name is cut, not spilled over the next chip.
    let long = scale_caption(Some(&KeyScale::new(0, "lydian-augmented-sharp2")));
    assert!(long.chars().count() <= 16, "{long}");
    assert_eq!(root_caption(1), "C# \u{25be}");
}

// --------------------------------------------------------- the menus ---

#[test]
fn the_scale_menu_lists_every_scale_once_under_its_family() {
    let (entries, rows) = scale_menu("", None);
    assert_eq!(entries.len(), rows.len());
    let scales: Vec<&str> = rows
        .iter()
        .filter_map(|r| match r {
            ScaleMenuRow::Scale(id) => Some(*id),
            _ => None,
        })
        .collect();
    assert_eq!(scales.len(), SCALES.len());
    assert!(rows.contains(&ScaleMenuRow::NoScale));
    // Headings are rows that cannot be pressed.
    for (entry, row) in entries.iter().zip(&rows) {
        if *row != ScaleMenuRow::FitNotes {
            assert_eq!(
                entry.enabled,
                *row != ScaleMenuRow::Heading,
                "{}",
                entry.label
            );
        }
    }
    // With no key, there is nothing to fit to yet.
    let fit = rows.iter().position(|r| *r == ScaleMenuRow::FitNotes);
    assert!(fit.is_none_or(|i| !entries[i].enabled));
}

#[test]
fn typing_filters_the_scale_menu_by_any_name() {
    // Freygish is one scale's name only. (Hijaz is two: Hijaz itself, and
    // Hijaz Kar, the double harmonic — both rightly found.)
    let (entries, rows) = scale_menu("freygish", Some(&KeyScale::new(0, "major")));
    let scales: Vec<&str> = rows
        .iter()
        .filter_map(|r| match r {
            ScaleMenuRow::Scale(id) => Some(*id),
            _ => None,
        })
        .collect();
    assert_eq!(scales, ["phrygian-dominant"]);
    assert!(
        entries[0].label.contains("freygish"),
        "{}",
        entries[0].label
    );
    // The one heading shown is its own family's.
    let headings = rows.iter().filter(|r| **r == ScaleMenuRow::Heading).count();
    assert_eq!(headings, 2, "the menu's title and one family");
}

#[test]
fn the_current_scale_is_ticked_and_can_be_fitted_to() {
    let current = KeyScale::new(9, "dorian");
    let (entries, rows) = scale_menu("", Some(&current));
    let at = rows
        .iter()
        .position(|r| *r == ScaleMenuRow::Scale("dorian"))
        .unwrap();
    assert!(
        entries[at].label.starts_with(CHOSEN_MARK),
        "{}",
        entries[at].label
    );
    let major = rows
        .iter()
        .position(|r| *r == ScaleMenuRow::Scale("major"))
        .unwrap();
    assert!(!entries[major].label.starts_with(CHOSEN_MARK));
    let fit = rows
        .iter()
        .position(|r| *r == ScaleMenuRow::FitNotes)
        .unwrap();
    assert!(entries[fit].enabled);
}

#[test]
fn the_root_menu_is_the_twelve_notes_with_the_current_one_ticked() {
    let entries = root_menu(4);
    assert_eq!(entries.len(), 12);
    assert!(entries[4].label.starts_with(CHOSEN_MARK));
    assert!(entries[4].label.ends_with('E'));
    assert!(entries[0].label.ends_with('C'));
}

#[test]
fn the_arrow_keys_step_each_note_to_its_next_row_in_the_scale() {
    let mut roll = PianoRoll::new(view());
    roll.scale = Some(c_major());
    let mut notes: Arena<NoteId, Note> = Arena::default();
    let e = notes.insert(note(0, 64));
    let g = notes.insert(note(PPQN, 67));
    roll.select(vec![e, g]);
    // Up: E to F is a semitone, G to A a tone — each its own step.
    assert_eq!(
        roll.nudge(&notes, 0, 1),
        vec![RollEdit::SetKeys {
            ids: vec![e, g],
            keys: vec![65, 69],
        }]
    );
    // Down: E to D, G to F.
    assert_eq!(
        roll.nudge(&notes, 0, -1),
        vec![RollEdit::SetKeys {
            ids: vec![e, g],
            keys: vec![62, 65],
        }]
    );
    // An octave is an octave, in any scale.
    assert!(matches!(
        roll.nudge(&notes, 0, 12).as_slice(),
        [RollEdit::Move { key_delta: 12, .. }]
    ));
    // And with no scale, a semitone is a semitone.
    roll.scale = None;
    assert!(matches!(
        roll.nudge(&notes, 0, 1).as_slice(),
        [RollEdit::Move { key_delta: 1, .. }]
    ));
}
