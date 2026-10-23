//! The roll's tools in the window, over a real song (`docs/roll-tools-plan.md`,
//! Ty 2026-10-20: *"great plan now follow through"*).
//!
//! The pure halves are `fontelle-ui/tests/tools_panel.rs` and the model's
//! tool tests. What only the window can get wrong is here: one undo per run,
//! which notes a tool acts on, the selection afterwards, the keys, where the
//! dialog sits, what it shows while open, and what it remembers.

mod common;

use fontelle_model::Note;
use fontelle_types::{NoteId, PPQN};
use fontelle_ui::canvas::{Action, RollEdit, ToolKind, ToolRow, Tools};
use fontelle_ui::document::{DocumentHost, StudioHost};

const BAR: i64 = PPQN * 4;

fn a_window() -> fontelle_ui::Windowless {
    window_over(common::a_session_for(common::a_clip_project(8)))
}

fn window_over(session: fontelle_app::Session) -> fontelle_ui::Windowless {
    let mut w = fontelle_ui::Windowless::new(
        Box::new(session),
        fontelle_ui::theme::Theme::dark_default(),
        (1600, 1000),
    );
    // One frame, as a window draws before anybody touches it: what the
    // session had to say at the start is said then, not over a tool's line.
    w.frame(0.0);
    w
}

fn a_note(start: i64, length: i64, key: u8) -> Note {
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
        path: Vec::new(),
        channel: None,
        lyric: String::new(),
        voice: None,
        muted: false,
    }
}

fn put(w: &mut fontelle_ui::Windowless, notes: Vec<Note>) -> Vec<NoteId> {
    let ids = w.document_mut().edit(RollEdit::Insert(notes));
    w.document_mut().end_gesture();
    w.select_notes(&[]);
    ids
}

/// Two triads, a bar each. The first three ids are the first chord.
fn chords(w: &mut fontelle_ui::Windowless) -> Vec<NoteId> {
    put(
        w,
        vec![
            a_note(0, BAR, 60),
            a_note(0, BAR, 64),
            a_note(0, BAR, 67),
            a_note(BAR, BAR, 62),
            a_note(BAR, BAR, 65),
            a_note(BAR, BAR, 69),
        ],
    )
}

fn shape(w: &fontelle_ui::Windowless) -> Vec<(i64, i64, u8)> {
    let mut v: Vec<(i64, i64, u8)> = w
        .document()
        .notes()
        .values()
        .map(|n| (n.start, n.length, n.key))
        .collect();
    v.sort();
    v
}

#[test]
fn arpeggiate_is_one_undo_and_bursts_nothing() {
    let mut w = a_window();
    chords(&mut w);
    let before = shape(&w);
    w.select_all_notes();
    w.open_tool(ToolKind::Arpeggiate);
    w.press_tool_row(ToolRow::Apply);
    assert!(w.document().notes().len() > 6, "the chords became runs");
    assert_eq!(w.flecks(), 0, "an arpeggio is not a delete");
    DocumentHost::undo(w.document_mut());
    assert_eq!(shape(&w), before, "one undo puts the chords back");
}

#[test]
fn every_dialog_is_one_undo() {
    for kind in ToolKind::ALL {
        let mut w = a_window();
        chords(&mut w);
        let before = shape(&w);
        w.select_all_notes();
        w.open_tool(kind);
        w.press_tool_row(ToolRow::Apply);
        if shape(&w) == before {
            continue;
        }
        DocumentHost::undo(w.document_mut());
        assert_eq!(shape(&w), before, "{kind:?} took more than one undo");
    }
}

#[test]
fn with_nothing_selected_a_tool_takes_the_whole_clip_and_says_so() {
    let mut w = a_window();
    chords(&mut w);
    w.open_tool(ToolKind::Strum);
    let title = w.tool_dialog().unwrap().title_text;
    assert!(title.contains("whole clip"), "{title}");
    w.press_tool_row(ToolRow::Apply);
    let starts: Vec<i64> = shape(&w).iter().map(|n| n.0).collect();
    assert_eq!(starts, vec![0, 60, 120, BAR, BAR + 60, BAR + 120]);
}

#[test]
fn with_a_selection_only_it_changes_and_the_result_stays_selected() {
    let mut w = a_window();
    let ids = chords(&mut w);
    w.select_notes(&ids[..3]);
    w.open_tool(ToolKind::Strum);
    assert!(w.tool_dialog().unwrap().title_text.contains('3'));
    w.press_tool_row(ToolRow::Apply);
    let second: Vec<i64> = ids[3..]
        .iter()
        .map(|id| w.document().notes().get(*id).unwrap().start)
        .collect();
    assert_eq!(
        second,
        vec![BAR, BAR, BAR],
        "the second chord was not chosen"
    );
    let mut chosen = w.selected_notes();
    chosen.sort();
    let mut first = ids[..3].to_vec();
    first.sort();
    assert_eq!(chosen, first, "the same notes, still chosen");

    w.close_tool();
    w.open_tool(ToolKind::Arpeggiate);
    w.press_tool_row(ToolRow::Apply);
    let run = w.selected_notes();
    assert!(run.len() > 3, "the run is what is chosen now: {run:?}");
    assert!(run.iter().all(|id| w.document().notes().get(*id).is_some()));
}

#[test]
fn enter_applies_and_escape_closes() {
    let mut w = a_window();
    chords(&mut w);
    w.select_all_notes();
    w.open_tool(ToolKind::Chop);
    w.press_enter();
    assert!(w.document().notes().len() > 6, "Enter chopped");
    assert!(
        w.tool_dialog().is_some(),
        "and the dialog stays for another go"
    );
    w.press_escape();
    assert!(w.tool_dialog().is_none());
}

#[test]
fn an_undo_after_applying_leaves_the_dialog_open() {
    let mut w = a_window();
    chords(&mut w);
    let before = shape(&w);
    w.select_all_notes();
    w.open_tool(ToolKind::Flip);
    w.press_tool_row(ToolRow::Apply);
    w.act(Action::Undo);
    assert_eq!(shape(&w), before);
    assert!(w.tool_dialog().is_some(), "try again with other settings");
}

#[test]
fn the_arrows_step_both_ways_and_a_drag_scrubs() {
    let mut w = a_window();
    chords(&mut w);
    w.open_tool(ToolKind::Strum);
    let at = w.tools().strum.time;
    w.press_tool_step(ToolRow::StrumTime, 1);
    let up = w.tools().strum.time;
    assert!(up > at);
    w.press_tool_step(ToolRow::StrumTime, -1);
    assert_eq!(w.tools().strum.time, at);
    w.press_tool_row(ToolRow::StrumTime);
    assert_eq!(w.tools().strum.time, up, "a click on the value steps it on");
    let before = w.tools().strum.time;
    w.drag_tool_value(ToolRow::StrumTime, 80.0);
    let after = w.tools().strum.time;
    assert!(after > before, "dragging right raised it");
    let steps = Tools {
        strum: fontelle_model::StrumSpec {
            time: before,
            ..Default::default()
        },
        ..Tools::default()
    };
    let mut once = steps;
    once.nudge(ToolRow::StrumTime, 1);
    assert!(
        after > once.strum.time,
        "a long drag goes more than one place"
    );
}

#[test]
fn the_dialog_moves_by_its_title_and_opens_there_again() {
    let mut w = a_window();
    chords(&mut w);
    w.open_tool(ToolKind::Levels);
    let was = w.tool_dialog().unwrap().frame;
    w.drag_tool_title(-120.0, 40.0);
    let moved = w.tool_dialog().unwrap().frame;
    assert!(
        (moved.x - (was.x - 120.0)).abs() < 1.0,
        "{was:?} -> {moved:?}"
    );
    assert!((moved.y - (was.y + 40.0)).abs() < 1.0);
    w.close_tool();
    w.open_tool(ToolKind::Chop);
    let again = w.tool_dialog().unwrap().frame;
    assert_eq!((again.x, again.y), (moved.x, moved.y));
}

#[test]
fn the_dialog_does_not_cover_the_notes_it_changes() {
    for start in [0, BAR] {
        let mut w = a_window();
        let ids = put(
            &mut w,
            vec![a_note(start, PPQN, 60), a_note(start, PPQN, 64)],
        );
        w.select_notes(&ids);
        w.open_tool(ToolKind::Strum);
        let frame = w.tool_dialog().unwrap().frame;
        for key in [60, 64] {
            let (x0, y) = w.roll_point(start, key);
            let (x1, _) = w.roll_point(start + PPQN, key);
            let covered = frame.contains(x0 + 1.0, y) || frame.contains(x1 - 1.0, y);
            assert!(!covered, "the dialog {frame:?} sits on the note at {start}");
        }
    }
}

#[test]
fn while_a_dialog_is_open_what_it_changes_is_dimmed_and_ghosted() {
    let mut w = a_window();
    let ids = chords(&mut w);
    w.select_notes(&ids[..3]);
    assert!(w.tool_dimmed().is_empty());
    w.open_tool(ToolKind::Strum);
    let mut dimmed = w.tool_dimmed();
    dimmed.sort();
    let mut moved = vec![ids[1], ids[2]];
    moved.sort();
    assert_eq!(dimmed, moved, "the bottom string does not move");
    let ghosts: Vec<i64> = w.tool_ghosts().iter().map(|g| g.0).collect();
    assert!(ghosts.contains(&60) && ghosts.contains(&120), "{ghosts:?}");
    w.close_tool();
    w.open_tool(ToolKind::Levels);
    assert!(
        w.tool_previews().iter().all(|n| n.velocity == 110),
        "a change of level is previewed as notes with their new level"
    );
    w.close_tool();
    assert!(w.tool_dimmed().is_empty() && w.tool_previews().is_empty());
}

#[test]
fn reset_puts_the_dialog_back() {
    let mut w = a_window();
    w.open_tool(ToolKind::Strum);
    w.press_tool_step(ToolRow::StrumTime, 1);
    w.press_tool_step(ToolRow::StrumTension, 1);
    w.press_tool_row(ToolRow::Reset);
    assert_eq!(w.tools().strum, Tools::default().strum);
}

#[test]
fn the_last_tool_runs_again_from_the_keyboard() {
    let mut w = a_window();
    chords(&mut w);
    w.select_all_notes();
    w.open_tool(ToolKind::Transpose);
    w.press_tool_row(ToolRow::Apply);
    w.close_tool();
    w.act(Action::RepeatTool);
    let keys: Vec<u8> = shape(&w).iter().map(|n| n.2).collect();
    assert_eq!(
        keys,
        vec![84, 88, 91, 86, 89, 93],
        "twelve, then twelve more"
    );
}

#[test]
fn chop_glue_and_quantize_starts_from_the_keyboard() {
    let mut w = a_window();
    put(&mut w, vec![a_note(0, PPQN, 60)]);
    w.act(Action::QuickChop);
    assert_eq!(w.document().notes().len(), 4, "a beat in sixteenths");
    w.select_notes(&[]);
    w.act(Action::Glue);
    assert_eq!(shape(&w), vec![(0, PPQN, 60)], "glued back");
    let mut w = a_window();
    put(&mut w, vec![a_note(PPQN / 4 + 30, 100, 60)]);
    w.act(Action::QuantizeStarts);
    assert_eq!(shape(&w), vec![(PPQN / 4, 100, 60)]);
}

#[test]
fn the_dialog_keys_open_their_dialogs() {
    let mut w = a_window();
    for (action, kind) in [
        (Action::OpenArpeggiate, ToolKind::Arpeggiate),
        (Action::OpenChop, ToolKind::Chop),
        (Action::OpenLengths, ToolKind::Lengths),
        (Action::OpenFlip, ToolKind::Flip),
        (Action::OpenFlam, ToolKind::Flam),
        (Action::OpenThinOut, ToolKind::ThinOut),
        (Action::OpenLevels, ToolKind::Levels),
    ] {
        w.act(action);
        assert_eq!(w.tool_dialog().map(|d| d.kind), Some(kind));
    }
}

#[test]
fn the_select_rows_choose_notes() {
    let mut w = a_window();
    let ids = put(
        &mut w,
        vec![a_note(0, PPQN / 4, 60), a_note(PPQN / 4, PPQN / 4, 62)],
    );
    w.choose_tool_menu("Off the beat");
    assert_eq!(w.selected_notes(), vec![ids[1]]);
    w.choose_tool_menu("Invert selection");
    assert_eq!(w.selected_notes(), vec![ids[0]]);
}

#[test]
fn a_heading_on_the_menu_does_nothing() {
    let mut w = a_window();
    chords(&mut w);
    let before = shape(&w);
    w.choose_tool_menu("Timing");
    assert!(w.tool_dialog().is_none());
    assert_eq!(shape(&w), before);
}

#[test]
fn doubles_and_overlaps_are_cleaned_from_the_menu() {
    let mut w = a_window();
    put(
        &mut w,
        vec![
            a_note(0, PPQN, 60),
            a_note(0, PPQN, 60),
            a_note(PPQN / 2, PPQN, 64),
            a_note(PPQN, PPQN, 64),
        ],
    );
    w.choose_tool_menu("Remove doubles");
    assert_eq!(w.document().notes().len(), 3);
    w.choose_tool_menu("Trim overlaps");
    assert!(
        shape(&w).contains(&(PPQN / 2, PPQN / 2, 64)),
        "{:?}",
        shape(&w)
    );
}

#[test]
fn the_rows_of_a_dialog_explain_themselves() {
    let mut w = a_window();
    w.open_tool(ToolKind::Strum);
    let rect = w.tool_row_rect(ToolRow::StrumTension);
    w.hover_studio(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
    assert_eq!(
        w.tip_now().as_deref(),
        Some(Tools::default().tip(ToolRow::StrumTension))
    );
}

#[test]
fn the_tools_chip_says_what_it_holds_now() {
    let mut w = a_window();
    let chip = w
        .roll_chip(fontelle_ui::canvas::RollControl::Tools)
        .unwrap();
    w.hover_studio(chip.x + chip.width / 2.0, chip.y + chip.height / 2.0);
    let tip = w.tip_now().unwrap_or_default();
    assert!(tip.contains("chop"), "{tip}");
    assert!(!tip.contains("adjust"), "the old menu's words: {tip}");
}

#[test]
fn tool_settings_survive_a_restart() {
    let mut w = a_window();
    w.open_tool(ToolKind::Chop);
    w.press_tool_step(ToolRow::ChopGate, -1);
    let gate = w.tools().chop.gate;
    assert!(gate < 1.0);
    w.close_tool();
    let saved = w
        .document()
        .roll_tool_settings()
        .expect("closing the dialog remembers it");
    assert_eq!(Tools::from_json(&saved).unwrap().chop.gate, gate);

    let mut session = common::a_session_for(common::a_clip_project(8));
    session.set_roll_tool_settings(saved);
    let again = window_over(session);
    assert_eq!(again.tools().chop.gate, gate);
}

#[test]
fn notes_mute_from_the_menu_as_one_undo() {
    let mut w = a_window();
    let ids = chords(&mut w);
    w.select_notes(&ids[..3]);
    w.choose_tool_menu("Mute or unmute");
    let muted: Vec<bool> = ids
        .iter()
        .map(|id| w.document().notes().get(*id).unwrap().muted)
        .collect();
    assert_eq!(muted, vec![true, true, true, false, false, false]);
    DocumentHost::undo(w.document_mut());
    assert!(w.document().notes().values().all(|n| !n.muted));
}

/// The singer renders by its own road, which does not know mute: a hollow
/// note that still sang would be worse than saying no.
#[test]
fn a_sung_note_is_not_muted_and_the_window_says_why() {
    let scratch = std::env::temp_dir().join(format!("fontelle-mute-vocal-{}", std::process::id()));
    std::fs::create_dir_all(&scratch).unwrap();
    let mut session = common::a_session_for(common::a_clip_project(8));
    session.set_singer_folders(vec![scratch.join("no-singers")]);
    session.set_voicebanks(scratch.join("no-voicebanks"), Vec::new());
    session
        .add_channel_of(fontelle_types::InstrumentKind::Vocal)
        .unwrap();
    let mut w = window_over(session);
    let ids = put(&mut w, vec![a_note(0, PPQN, 60)]);
    w.frame(0.0);
    w.select_notes(&ids);
    w.choose_tool_menu("Mute or unmute");
    assert!(!w.document().notes().get(ids[0]).unwrap().muted);
    std::fs::remove_dir_all(&scratch).ok();
}

/// The title says what the tool acts on *now*: after an undo took away the
/// notes it had just made and left chosen, it is the whole clip again (seen
/// on `:99` — "132 selected" over a clip of twelve).
#[test]
fn the_title_follows_an_undo() {
    let mut w = a_window();
    chords(&mut w);
    w.select_all_notes();
    w.open_tool(ToolKind::Chop);
    w.press_tool_row(ToolRow::Apply);
    w.act(Action::Undo);
    let title = w.tool_dialog().unwrap().title_text;
    assert!(title.contains("whole clip (6)"), "{title}");
}

// ------------------------------------- the status line holds (2026-10-23)
//
// Ty: *"sounds like a bug, id prefer not to leave loose ends"* — a tool's
// sentence on the status line ("Chopped 12 note(s)", "Muting sung notes is
// not built yet") was replaced at the very next refresh by the browser's own
// line ("4069 file(s)"): nobody could read what had just been done, or why
// it had not.

fn a_vocal_window() -> (fontelle_ui::Windowless, std::path::PathBuf) {
    let scratch = std::env::temp_dir().join(format!(
        "fontelle-status-hold-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&scratch).unwrap();
    let mut session = common::a_session_for(common::a_clip_project(8));
    session.set_singer_folders(vec![scratch.join("no-singers")]);
    session.set_voicebanks(scratch.join("no-voicebanks"), Vec::new());
    session
        .add_channel_of(fontelle_types::InstrumentKind::Vocal)
        .unwrap();
    (window_over(session), scratch)
}

#[test]
fn what_the_window_says_stays_on_the_status_line_long_enough_to_read() {
    let (mut w, scratch) = a_vocal_window();
    let ids = put(&mut w, vec![a_note(0, PPQN, 60)]);
    w.frame(0.0);
    w.select_notes(&ids);
    w.choose_tool_menu("Mute or unmute");
    let said = w.status().to_string();
    assert!(said.contains("Muting sung notes"), "{said}");
    for _ in 0..10 {
        w.frame(0.016);
    }
    assert_eq!(w.status(), said, "still there ten frames on");
    std::thread::sleep(fontelle_ui::STATUS_HOLD + std::time::Duration::from_millis(50));
    w.frame(0.016);
    assert_ne!(w.status(), said, "and then the browser's line again");
    std::fs::remove_dir_all(&scratch).ok();
}

#[test]
fn what_the_studio_says_stays_on_the_status_line_too() {
    let empty =
        std::env::temp_dir().join(format!("fontelle-status-noplugins-{}", std::process::id()));
    std::fs::create_dir_all(&empty).unwrap();
    let session =
        common::a_session_for(common::a_clip_project(8)).with_plugin_folders(vec![empty.clone()]);
    let mut w = window_over(session);
    w.document_mut().rescan_plugins();
    w.frame(0.016);
    let said = w.status().to_string();
    assert!(said.contains("no plugins found"), "{said}");
    for _ in 0..10 {
        w.frame(0.016);
    }
    assert_eq!(w.status(), said, "still there ten frames on");
    std::fs::remove_dir_all(&empty).ok();
}

#[test]
fn a_new_sentence_replaces_the_last_at_once() {
    let (mut w, scratch) = a_vocal_window();
    let ids = put(&mut w, vec![a_note(0, PPQN, 60)]);
    w.frame(0.0);
    w.select_notes(&ids);
    w.choose_tool_menu("Mute or unmute");
    w.frame(0.016);
    let first = w.status().to_string();
    // One note glues to nothing: a sentence of its own all the same.
    w.choose_tool_menu("Glue");
    w.frame(0.016);
    assert_ne!(w.status(), first);
    assert!(w.status().contains("Nothing to change"), "{}", w.status());
    std::fs::remove_dir_all(&scratch).ok();
}
