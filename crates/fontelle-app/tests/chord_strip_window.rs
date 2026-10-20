//! Smart chords end to end, in the window over a real song: turn them on,
//! place a note, hover a chip (heard), move away (silent), click one
//! (written, one undo).

mod common;

use fontelle_ui::canvas::RollControl;
use fontelle_ui::document::{DocumentHost, StudioHost};

fn centre(r: fontelle_ui::layout::Rect) -> (f32, f32) {
    (r.x + r.width / 2.0, r.y + r.height / 2.0)
}

fn a_window() -> fontelle_ui::Windowless {
    let mut session = common::a_session_for(common::a_clip_project(8));
    StudioHost::set_song_key(
        &mut session,
        Some(fontelle_types::KeyScale::new(0, "major")),
        Vec::new(),
        Vec::new(),
    );
    fontelle_ui::Windowless::new(
        Box::new(session),
        fontelle_ui::theme::Theme::dark_default(),
        (1600, 1000),
    )
}

fn notes(w: &fontelle_ui::Windowless) -> Vec<u8> {
    let mut keys: Vec<u8> = w.document().notes().values().map(|n| n.key).collect();
    keys.sort();
    keys
}

#[test]
fn a_placed_note_offers_chords_that_are_heard_and_written_as_one_undo() {
    let mut w = a_window();
    let before = notes(&w);
    let chip = w.roll_chip(RollControl::Chords).expect("the chip");
    let (x, y) = centre(chip);
    w.click_studio(x, y);
    // A G, on the first beat.
    let (x, y) = w.roll_point(0, 67);
    w.click_studio(x, y);
    assert_eq!(notes(&w).len(), before.len() + 1, "the note was placed");
    let strip = w.chord_strip().expect("the strip opens on the note");
    assert_eq!(w.chord_names()[0], "G", "{:?}", w.chord_names());

    // Hovering a chip sounds it; moving off stops it.
    let (x, y) = centre(strip.chips[1]);
    w.hover_studio(x, y);
    let heard = w.chord_sounding().to_vec();
    assert!(heard.len() >= 3, "{heard:?}");
    assert!(heard.contains(&67), "the chord is voiced around the note");
    w.hover_studio(strip.frame.x - 40.0, strip.frame.y - 40.0);
    assert!(w.chord_sounding().is_empty(), "still sounding");

    // Clicking the first writes it: the G stays, B and D go in beside it.
    let (x, y) = centre(strip.chips[0]);
    w.hover_studio(x, y);
    w.click_studio(x, y);
    assert!(w.chord_strip().is_none(), "the strip is put away");
    assert!(w.chord_sounding().is_empty(), "the chord is let go of");
    let written = notes(&w);
    assert_eq!(written.len(), before.len() + 3, "{written:?}");
    for key in [67, 71, 74] {
        assert!(written.contains(&key), "{key} missing from {written:?}");
    }
    // One undo takes the chord away and leaves the note.
    DocumentHost::undo(w.document_mut());
    assert_eq!(notes(&w).len(), before.len() + 1);
}

/// Off, a placed note is only a note.
#[test]
fn with_smart_chords_off_no_strip() {
    let mut w = a_window();
    let (x, y) = w.roll_point(0, 67);
    w.click_studio(x, y);
    assert!(w.chord_strip().is_none());
}

/// Ty, 2026-10-20: *"chords tool is constantly moving around when clicking
/// through the settings."* Every control reshapes the chips — more of them,
/// fewer, reaching higher or lower — and the strip used to follow, so the
/// next control was no longer under the pointer. It stays where it opened.
#[test]
fn the_strip_stays_put_while_its_controls_are_pressed() {
    let mut w = a_window();
    let chip = w.roll_chip(RollControl::Chords).expect("the chip");
    let (x, y) = centre(chip);
    w.click_studio(x, y);
    let (x, y) = w.roll_point(0, 67);
    w.click_studio(x, y);
    let opened = w.chord_strip().expect("the strip opens on the note");
    let mut names = std::collections::HashSet::new();
    for _round in 0..4 {
        for control in [
            opened.extension,
            opened.voicing,
            opened.inversion,
            opened.mode,
        ] {
            let (x, y) = centre(control);
            w.click_studio(x, y);
            let now = w.chord_strip().expect("a control press keeps it open");
            names.insert(w.chord_names());
            // Its corner stays; only its right edge follows the chips.
            assert_eq!(
                (now.frame.x, now.frame.y, now.frame.height),
                (opened.frame.x, opened.frame.y, opened.frame.height),
                "the strip moved"
            );
            assert_eq!(
                (
                    now.mode,
                    now.extension,
                    now.voicing,
                    now.inversion,
                    now.type_it
                ),
                (
                    opened.mode,
                    opened.extension,
                    opened.voicing,
                    opened.inversion,
                    opened.type_it
                ),
                "a control moved"
            );
            for chip in &now.chips {
                assert!(
                    chip.intersection(&now.frame) == *chip,
                    "a chip outside the strip"
                );
            }
        }
    }
    assert!(names.len() > 1, "the controls never changed the chords");
}
