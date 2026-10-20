//! Quantize, the arpeggiator's preview and full screen, in the window over a
//! real song (Ty, 2026-10-20: *"add quantize tool and make ctrl q
//! shortcut"*, *"make arpeggiator let you preview changes"*, *"add f11 to
//! toggle fullscreen"*).

mod common;

use fontelle_types::PPQN;
use fontelle_ui::canvas::{Action, RollEdit, ToolKind, ToolRow};
use fontelle_ui::document::DocumentHost;

fn a_window() -> fontelle_ui::Windowless {
    let session = common::a_session_for(common::a_clip_project(8));
    fontelle_ui::Windowless::new(
        Box::new(session),
        fontelle_ui::theme::Theme::dark_default(),
        (1600, 1000),
    )
}

fn a_note(start: i64, key: u8) -> fontelle_model::Note {
    fontelle_model::Note {
        start,
        length: PPQN / 4,
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
        lyric: String::new(),
        voice: None,
        muted: false,
    }
}

/// Three notes played a little off the sixteenths.
fn played(w: &mut fontelle_ui::Windowless) {
    w.document_mut().edit(RollEdit::Insert(vec![
        a_note(PPQN / 4 + 13, 60),
        a_note(PPQN / 2 - 9, 62),
        a_note(PPQN + 20, 64),
    ]));
    w.document_mut().end_gesture();
}

fn starts(w: &fontelle_ui::Windowless) -> Vec<i64> {
    let mut starts: Vec<i64> = w.document().notes().values().map(|n| n.start).collect();
    starts.sort();
    starts
}

/// With nothing selected Ctrl+Q quantizes the whole clip to the roll's snap,
/// keeps every note (and its id), and one undo puts them back.
#[test]
fn ctrl_q_quantizes_the_clip_as_one_undo() {
    let mut w = a_window();
    played(&mut w);
    let before = starts(&w);
    let ids: Vec<_> = w.document().notes().keys().collect();
    w.act(Action::Quantize);
    assert_eq!(starts(&w), vec![PPQN / 4, PPQN / 2, PPQN]);
    let mut after: Vec<_> = w.document().notes().keys().collect();
    let mut ids = ids;
    ids.sort();
    after.sort();
    assert_eq!(after, ids, "the notes are the same notes, moved");
    DocumentHost::undo(w.document_mut());
    assert_eq!(starts(&w), before, "one undo takes it all back");
}

/// The Arpeggiate dialog ghosts its run in the roll, and the ghost follows
/// a setting pressed.
#[test]
fn the_arpeggiators_run_is_ghosted_and_follows_its_settings() {
    let mut w = a_window();
    w.document_mut().edit(RollEdit::Insert(vec![
        fontelle_model::Note {
            length: PPQN * 2,
            ..a_note(0, 60)
        },
        fontelle_model::Note {
            length: PPQN * 2,
            ..a_note(0, 64)
        },
        fontelle_model::Note {
            length: PPQN * 2,
            ..a_note(0, 67)
        },
    ]));
    w.document_mut().end_gesture();
    w.select_all_notes();
    assert!(w.tool_ghosts().is_empty(), "no dialog, no ghost");
    w.open_tool(ToolKind::Arpeggiate);
    let first = w.tool_ghosts();
    assert!(first.len() > 3, "the run is ghosted: {first:?}");
    w.press_tool_row(ToolRow::ArpRate);
    let second = w.tool_ghosts();
    assert_ne!(first, second, "the ghost did not follow the setting");
    // Hearing it sounds the run's first note; nothing was written.
    w.press_tool_row(ToolRow::Hear);
    assert_eq!(w.document().notes().len(), 3, "hearing it wrote it");
    w.close_tool();
    assert!(w.tool_ghosts().is_empty(), "the ghost went with the dialog");
}

#[test]
fn f11_goes_full_screen_and_back() {
    let mut w = a_window();
    assert!(!w.fullscreen());
    w.act(Action::Fullscreen);
    assert!(w.fullscreen());
    w.act(Action::Fullscreen);
    assert!(!w.fullscreen());
}
