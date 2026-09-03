//! The piano roll's Tools panel: what is on it, what a click does, and what
//! comes out the other end as an edit.
//!
//! Asked for as *"a tools tab which has various tools such as a randomizer, a
//! transposer — and it should also let you transpose all of your selections
//! velocity or pan or whatever all at once adding or subtracting a value"*.
//!
//! The shape it takes is the settings tab's, because that shape is already in
//! this window and already understood: **rows of a name and a value, where a
//! click steps the value forward and a Ctrl+click steps it back.** No text
//! field is involved — there is not one in this window — and a value you can
//! reach in a handful of clicks is quicker than one you have to type anyway.
//!
//! Everything here is pure. The panel is geometry, the settings are numbers,
//! and what a tool *does* is a list of [`RollEdit`]s — so all of it is
//! checkable without a window, a document, or a mouse.

use fontelle_model::{Note, NoteProperty, RandomMode};
use fontelle_types::{NoteId, PPQN};
use fontelle_ui::canvas::{
    LaneProperty, RollEdit, TOOL_ROWS, ToolAction, ToolRow, Tools,
};
use fontelle_ui::layout::Rect;
use fontelle_ui::theme::Theme;

fn metrics() -> fontelle_ui::theme::Metrics {
    Theme::dark_default().metrics
}

/// A chip on a toolbar at the top of a tall panel — the ordinary case.
fn chip() -> Rect {
    Rect::new(120.0, 4.0, 40.0, 18.0)
}

fn bounds() -> Rect {
    Rect::new(0.0, 0.0, 800.0, 600.0)
}

fn a_note(key: u8, velocity: u8) -> Note {
    Note {
        start: 0,
        length: PPQN,
        key,
        velocity,
        pan: 0,
        fine_pitch: 0,
        release: 0,
        mod_x: 0,
        mod_y: 0,
        slide: false,
    }
}

/// Three selected notes, and an arena holding them.
fn selection() -> (fontelle_model::Arena<NoteId, Note>, Vec<NoteId>) {
    let mut notes = fontelle_model::Arena::default();
    let ids = vec![
        notes.insert(a_note(60, 40)),
        notes.insert(a_note(64, 100)),
        notes.insert(a_note(67, 120)),
    ];
    (notes, ids)
}

// -------------------------------------------------------------- the rows ---

#[test]
fn every_row_says_what_it_is_and_what_it_is_at() {
    // The same rule the settings tab follows: a row with no name cannot be
    // identified, and a row with no value cannot be read. An *action* row is
    // the exception — it is a button, and its name is the whole of it.
    let tools = Tools::default();
    for row in TOOL_ROWS {
        assert!(!tools.label(row).is_empty(), "{row:?} has no name");
        if tools.action(row).is_some() {
            continue;
        }
        assert!(
            !tools.value(row).is_empty(),
            "{row:?} does not say what it is at"
        );
    }
}

#[test]
fn the_tools_that_were_asked_for_are_all_reachable_from_the_chip() {
    let mut actions: Vec<ToolAction> = TOOL_ROWS
        .iter()
        .filter_map(|row| Tools::default().action(*row))
        .collect();
    // The two importers are entries on the menu rather than rows in a dialog:
    // a file browser is where they take their settings.
    actions.extend(
        fontelle_ui::canvas::TOOL_MENU
            .iter()
            .filter_map(|item| match item {
                fontelle_ui::canvas::ToolMenuItem::Run(action) => Some(*action),
                fontelle_ui::canvas::ToolMenuItem::Open(_) => None,
            }),
    );
    for wanted in [
        ToolAction::Transpose,
        ToolAction::Add,
        ToolAction::Subtract,
        ToolAction::Randomize,
        ToolAction::ImportMidi,
        ToolAction::ImportScore,
    ] {
        assert!(actions.contains(&wanted), "no row does {wanted:?}");
    }
}

#[test]
fn an_action_row_is_not_a_value_to_step() {
    // Clicking "Randomize" must randomize, not quietly change the amount
    // above it — which is what a row that was both would do.
    let mut tools = Tools::default();
    let before = tools;
    for row in TOOL_ROWS {
        if tools.action(row).is_some() {
            tools.nudge(row, 1);
        }
    }
    assert_eq!(tools, before);
}

// --------------------------------------------------------------- values ---

#[test]
fn transpose_says_its_direction_as_well_as_its_size() {
    // "+0" against "0" is the difference between a number that can go either
    // way and one that might not — the same rule the MIDI transpose row
    // follows.
    let up = Tools {
        semitones: 0,
        ..Tools::default()
    };
    assert!(up.value(ToolRow::Transpose).starts_with('+'));
    let down = Tools {
        semitones: -5,
        ..Tools::default()
    };
    assert!(down.value(ToolRow::Transpose).starts_with('-'));
}

#[test]
fn transpose_stops_at_two_octaves_rather_than_wrapping() {
    // A number's ends are ends: running off one and arriving at the other is
    // a control that cannot be trusted to a held press.
    let mut tools = Tools::default();
    for _ in 0..200 {
        tools.nudge(ToolRow::Transpose, 1);
    }
    assert_eq!(tools.semitones, 24);
    for _ in 0..400 {
        tools.nudge(ToolRow::Transpose, -1);
    }
    assert_eq!(tools.semitones, -24);
}

#[test]
fn the_property_row_cycles_through_all_six() {
    // A set with no ends wraps, which is the other half of the same rule.
    let mut tools = Tools::default();
    let mut seen = vec![tools.property];
    for _ in 0..5 {
        tools.nudge(ToolRow::Property, 1);
        seen.push(tools.property);
    }
    seen.sort_by_key(|p| format!("{p:?}"));
    seen.dedup();
    assert_eq!(seen.len(), 6, "all six properties are reachable");

    // And it comes back round rather than stopping.
    let mut tools = Tools::default();
    let first = tools.property;
    for _ in 0..6 {
        tools.nudge(ToolRow::Property, 1);
    }
    assert_eq!(tools.property, first);
}

#[test]
fn the_amount_walks_a_ladder_so_a_useful_value_is_a_few_clicks_away() {
    // Stepping by one would be twenty clicks to reach twenty, which is the
    // difference between a tool and a chore.
    let mut tools = Tools::default();
    let mut reached = vec![tools.amount];
    for _ in 0..20 {
        tools.nudge(ToolRow::Amount, 1);
        reached.push(tools.amount);
    }
    assert!(reached.contains(&10), "10 should be on the ladder");
    assert!(reached.contains(&20), "20 should be on the ladder");
    assert!(
        reached.iter().position(|v| *v == 20).unwrap() <= 10,
        "20 is more than ten clicks away: {reached:?}"
    );
    // The ladder only goes up, and stops.
    assert!(reached.windows(2).all(|w| w[1] >= w[0]), "{reached:?}");
}

#[test]
fn the_amount_never_reaches_zero_because_an_offset_of_nothing_is_not_a_tool() {
    let mut tools = Tools::default();
    for _ in 0..50 {
        tools.nudge(ToolRow::Amount, -1);
    }
    assert!(tools.amount >= 1, "got {}", tools.amount);
}

#[test]
fn the_randomizers_amount_reaches_both_ends_of_its_dial() {
    let mut tools = Tools::default();
    for _ in 0..50 {
        tools.nudge(ToolRow::RandomAmount, -1);
    }
    assert_eq!(tools.random_amount, 0, "nothing at all is a legal setting");
    for _ in 0..50 {
        tools.nudge(ToolRow::RandomAmount, 1);
    }
    assert_eq!(tools.random_amount, 100);
}

#[test]
fn the_randomizers_mode_toggles() {
    let mut tools = Tools::default();
    assert_eq!(tools.random_mode, RandomMode::Around);
    tools.nudge(ToolRow::RandomMode, 1);
    assert_eq!(tools.random_mode, RandomMode::Anywhere);
    tools.nudge(ToolRow::RandomMode, 1);
    assert_eq!(tools.random_mode, RandomMode::Around);
}

// -------------------------------------------------------------- the edits ---

#[test]
fn transposing_moves_every_selected_note_and_nothing_else() {
    let (notes, ids) = selection();
    let mut tools = Tools {
        semitones: 12,
        ..Tools::default()
    };
    let edits = tools.run(ToolAction::Transpose, &ids, &notes);
    assert_eq!(edits.len(), 1);
    match &edits[0] {
        RollEdit::Move {
            ids: moved,
            tick_delta,
            key_delta,
        } => {
            assert_eq!(moved, &ids);
            assert_eq!(*tick_delta, 0, "a transpose does not move a note in time");
            assert_eq!(*key_delta, 12);
        }
        other => panic!("expected a move, got {other:?}"),
    }
}

#[test]
fn transposing_by_nothing_does_nothing() {
    let (notes, ids) = selection();
    let mut tools = Tools {
        semitones: 0,
        ..Tools::default()
    };
    assert!(tools.run(ToolAction::Transpose, &ids, &notes).is_empty());
}

#[test]
fn adding_and_subtracting_are_the_same_amount_the_two_ways() {
    let (notes, ids) = selection();
    let mut tools = Tools {
        property: LaneProperty::Velocity,
        amount: 10,
        ..Tools::default()
    };

    let up = tools.run(ToolAction::Add, &ids, &notes);
    let down = tools.run(ToolAction::Subtract, &ids, &notes);
    match (&up[0], &down[0]) {
        (
            RollEdit::NudgeProperty { delta: plus, property: a, ids: up_ids },
            RollEdit::NudgeProperty { delta: minus, property: b, .. },
        ) => {
            assert_eq!(*plus, 10);
            assert_eq!(*minus, -10);
            assert_eq!(a, b);
            assert_eq!(*a, NoteProperty::Velocity);
            assert_eq!(up_ids, &ids);
        }
        other => panic!("expected two nudges, got {other:?}"),
    }
}

#[test]
fn the_offset_acts_on_whichever_property_the_row_says() {
    // *"transpose all of your selections velocity or pan or whatever"* — the
    // property row is what "or whatever" is.
    let (notes, ids) = selection();
    let mut tools = Tools {
        property: LaneProperty::Pan,
        ..Tools::default()
    };
    let edits = tools.run(ToolAction::Add, &ids, &notes);
    match &edits[0] {
        RollEdit::NudgeProperty { property, .. } => assert_eq!(*property, NoteProperty::Pan),
        other => panic!("expected a nudge, got {other:?}"),
    }
}

#[test]
fn randomizing_produces_one_value_for_every_selected_note() {
    let (notes, ids) = selection();
    let mut tools = Tools {
        random_amount: 50,
        ..Tools::default()
    };
    let edits = tools.run(ToolAction::Randomize, &ids, &notes);
    match &edits[0] {
        RollEdit::SetPropertyEach {
            ids: named,
            values,
            property,
        } => {
            assert_eq!(named, &ids);
            assert_eq!(values.len(), ids.len());
            assert_eq!(*property, NoteProperty::Velocity);
        }
        other => panic!("expected a value each, got {other:?}"),
    }
}

#[test]
fn randomizing_reads_the_notes_that_are_there_rather_than_starting_from_zero() {
    // `Around` is a wobble around what is already written, so the notes have
    // to be read. A randomizer that ignored them would flatten the phrase and
    // then jitter the flat version.
    let (notes, ids) = selection();
    let mut tools = Tools {
        random_amount: 5,
        random_mode: RandomMode::Around,
        ..Tools::default()
    };
    let edits = tools.run(ToolAction::Randomize, &ids, &notes);
    let RollEdit::SetPropertyEach { values, .. } = &edits[0] else {
        panic!("expected a value each");
    };
    // The notes were at 40, 100 and 120, and a 5% wobble cannot cross them.
    assert!(values[0] < values[1], "{values:?}");
    assert!(values[1] < values[2], "{values:?}");
}

#[test]
fn rolling_again_gives_a_different_answer() {
    // The seed steps on every roll, which is what "Randomize" pressed twice
    // has to mean.
    let (notes, ids) = selection();
    let mut tools = Tools {
        random_amount: 100,
        ..Tools::default()
    };
    let first = tools.run(ToolAction::Randomize, &ids, &notes);
    let second = tools.run(ToolAction::Randomize, &ids, &notes);
    assert_ne!(first, second, "pressing it twice must roll twice");
}

#[test]
fn a_tool_with_nothing_selected_does_nothing_at_all() {
    // And says so by producing no edit, rather than by producing a command
    // over an empty list that lands in the history as an entry that did
    // nothing.
    let (notes, _) = selection();
    let mut tools = Tools {
        semitones: 12,
        ..Tools::default()
    };
    for action in [
        ToolAction::Transpose,
        ToolAction::Add,
        ToolAction::Subtract,
        ToolAction::Randomize,
    ] {
        assert!(
            tools.run(action, &[], &notes).is_empty(),
            "{action:?} did something to an empty selection"
        );
    }
}

#[test]
fn the_two_import_rows_produce_no_edit_because_they_are_the_windows_to_make() {
    // Reading a file is not something this crate may do (INVARIANT 2), so the
    // panel names the action and the window carries it out.
    let (notes, ids) = selection();
    let mut tools = Tools::default();
    for action in [ToolAction::ImportMidi, ToolAction::ImportScore] {
        assert!(tools.run(action, &ids, &notes).is_empty());
    }
}

// ------------------------------------------------------------- geometry ---

#[test]
fn the_panel_hangs_off_its_chip_and_stays_inside_the_window() {
    let panel = tools_dialog_layout(ToolKind::Adjust, chip(), bounds(), &metrics());
    assert!(!panel.frame.is_empty());
    assert!(panel.frame.y >= chip().bottom() - 0.01, "it opens downwards");
    assert!(panel.frame.right() <= bounds().right() + 0.01);
    assert!(panel.frame.bottom() <= bounds().bottom() + 0.01);
    assert_eq!(panel.rows.len(), ToolKind::Adjust.rows().len());
}

#[test]
fn a_chip_near_the_bottom_opens_the_panel_upwards() {
    // A panel that hung off the bottom would be a list of tools half of which
    // nobody can reach.
    let low = Rect::new(120.0, 560.0, 40.0, 18.0);
    let panel = tools_dialog_layout(ToolKind::Adjust, low, bounds(), &metrics());
    assert!(panel.frame.bottom() <= bounds().bottom() + 0.01);
    assert!(panel.frame.y < low.y, "it opened upwards");
}

#[test]
fn a_chip_near_the_right_edge_slides_the_panel_back_inside() {
    let right = Rect::new(780.0, 4.0, 40.0, 18.0);
    let panel = tools_dialog_layout(ToolKind::Adjust, right, bounds(), &metrics());
    assert!(panel.frame.right() <= bounds().right() + 0.01);
    assert!(panel.frame.x >= bounds().x - 0.01);
}

#[test]
fn a_click_on_a_row_finds_that_row() {
    let panel = tools_dialog_layout(ToolKind::Adjust, chip(), bounds(), &metrics());
    for (row, rect) in &panel.rows {
        if rect.is_empty() {
            continue;
        }
        let hit = tools_dialog_hit(&panel, rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
        assert_eq!(hit, Some(*row), "clicking {row:?} found {hit:?}");
    }
}

#[test]
fn a_click_that_missed_the_panel_hits_nothing() {
    let panel = tools_dialog_layout(ToolKind::Adjust, chip(), bounds(), &metrics());
    assert_eq!(tools_dialog_hit(&panel, 5.0, 590.0), None);
}

#[test]
fn a_window_too_short_for_the_whole_panel_still_shows_what_it_can() {
    // Better than the alternative every other menu in this window takes,
    // which is to draw nothing: a short window must still be one you can
    // transpose in.
    let short = Rect::new(0.0, 0.0, 800.0, 90.0);
    let panel = tools_dialog_layout(ToolKind::Adjust, Rect::new(10.0, 2.0, 40.0, 16.0), short, &metrics());
    assert!(!panel.frame.is_empty());
    assert!(panel.frame.bottom() <= short.bottom() + 0.01);
    let reachable = panel.rows.iter().filter(|(_, r)| !r.is_empty()).count();
    assert!(reachable > 0, "nothing on the panel can be clicked");
    // And a row that fell off the end hit-tests as absent rather than as the
    // row that happens to be drawn where it would have been.
    for (row, rect) in &panel.rows {
        if rect.is_empty() {
            continue;
        }
        assert_eq!(
            tools_dialog_hit(&panel, rect.x + 1.0, rect.y + rect.height / 2.0),
            Some(*row)
        );
    }
}

#[test]
fn nothing_on_the_panel_is_drawn_outside_it() {
    let panel = tools_dialog_layout(ToolKind::Adjust, chip(), bounds(), &metrics());
    for (row, rect) in &panel.rows {
        if rect.is_empty() {
            continue;
        }
        assert!(
            panel.frame.intersection(rect) == *rect,
            "{row:?} at {rect:?} is outside {:?}",
            panel.frame
        );
    }
}

// ------------------------------------------------- one tool at a time ---
//
// Reported from using the window:
//
// > *"right now you split the functionality between the settings and tools
// > button in the piano roll so i currently have to chose how much i want it
// > transposed in the settings and then click transpose in the tools to
// > actually do it. this is really annoying. please make it so like fl studio
// > these menus pop up as their own menu i can then make tweaks in to chose
// > how i want the tool to apply then i click apply."*
//
// The first sentence is a misreading of the window and the misreading is the
// window's fault: the settings tab's *"Transpose"* is the MIDI keyboard's and
// has nothing to do with the roll, but it is one word in a list of settings
// and the roll's transposer is one word in a list of tools, so of course they
// read as two halves of one thing. Two fixes, and this file is the second:
// the settings row now says whose transpose it is, and the Tools chip now
// opens a **menu of tools** where each one is its own small dialog with its
// own settings and its own Apply — rather than one bench with every tool's
// settings on it at once, where the amount you were about to apply sat three
// rows above a button belonging to a different tool.

use fontelle_ui::canvas::{TOOL_MENU, ToolKind, ToolMenuItem, tools_dialog_hit, tools_dialog_layout};

#[test]
fn the_chip_opens_a_menu_of_tools_rather_than_every_tools_settings_at_once() {
    let labels: Vec<String> = TOOL_MENU.iter().map(|item| item.label()).collect();
    assert_eq!(
        labels.len(),
        5,
        "three tools and the two importers: {labels:?}"
    );
    for item in TOOL_MENU {
        assert!(!item.label().is_empty(), "{item:?} has no name");
    }
    // The three that take settings say so with an ellipsis, the FL habit and
    // everybody else's: a menu entry that opens something is spelled
    // differently from one that does something.
    for kind in ToolKind::ALL {
        let entry = TOOL_MENU
            .iter()
            .find(|item| matches!(item, ToolMenuItem::Open(k) if *k == kind))
            .unwrap_or_else(|| panic!("{kind:?} is not on the menu"));
        assert!(
            entry.label().ends_with('\u{2026}'),
            "{kind:?} opens a dialog and its entry does not say so: {}",
            entry.label()
        );
    }
}

#[test]
fn every_tool_dialog_holds_that_tools_settings_and_no_other_tools() {
    // The whole complaint. A dialog that also shows the randomizer's strength
    // is the bench again, and the reason a transpose amount three rows away
    // from an Apply button read as living somewhere else entirely.
    for kind in ToolKind::ALL {
        let mine = kind.rows();
        assert!(!mine.is_empty(), "{kind:?} has no settings at all");
        for other in ToolKind::ALL {
            if other == kind {
                continue;
            }
            for row in other.rows() {
                assert!(
                    !mine.contains(row),
                    "{kind:?}'s dialog shows {row:?}, which belongs to {other:?}"
                );
            }
        }
    }
}

#[test]
fn every_tool_dialog_ends_in_the_button_that_applies_it() {
    // *"i can then make tweaks in to chose how i want the tool to apply then i
    // click apply."* The apply is the last thing in the dialog, under the
    // settings it applies, because that is the order you do them in.
    let tools = Tools::default();
    for kind in ToolKind::ALL {
        let rows = kind.rows();
        let (last, rest) = rows.split_last().expect("a dialog has rows");
        assert!(
            tools.action(*last).is_some(),
            "{kind:?}'s dialog does not end in a button"
        );
        // Every row above it is a setting, with one exception that is not one:
        // Adjust applies two ways — add and take off — and the two buttons sit
        // together at the bottom.
        for row in rest {
            if tools.action(*row).is_some() {
                assert!(
                    tools.action(rows[rows.len() - 2]).is_some(),
                    "{kind:?} has a button above a setting"
                );
            }
        }
    }
}

#[test]
fn a_dialog_is_titled_with_the_tool_it_is() {
    for kind in ToolKind::ALL {
        assert!(!kind.title().is_empty(), "{kind:?} has no title");
        let l = tools_dialog_layout(kind, chip(), bounds(), &metrics());
        assert_eq!(l.kind, kind);
        assert!(!l.title.is_empty(), "{kind:?}'s dialog has no title bar");
        assert!(
            l.title.intersection(&l.frame) == l.title,
            "the title escapes the dialog"
        );
        for (row, rect) in &l.rows {
            assert!(
                rect.is_empty() || !rect.intersects(&l.title),
                "{row:?} is drawn over the title"
            );
        }
    }
}

#[test]
fn a_click_on_a_dialog_row_finds_that_row() {
    let l = tools_dialog_layout(ToolKind::Randomize, chip(), bounds(), &metrics());
    for (row, rect) in &l.rows {
        let (x, y) = (rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
        assert_eq!(tools_dialog_hit(&l, x, y), Some(*row));
    }
    assert_eq!(tools_dialog_hit(&l, l.frame.right() + 20.0, l.frame.y), None);
    assert_eq!(
        tools_dialog_hit(&l, l.title.x + 2.0, l.title.y + 2.0),
        None,
        "the title is not a row to click"
    );
}

#[test]
fn a_tool_dialog_hangs_off_its_chip_and_stays_inside_the_window() {
    for kind in ToolKind::ALL {
        let l = tools_dialog_layout(kind, chip(), bounds(), &metrics());
        assert_eq!(
            l.frame.intersection(&bounds()),
            l.frame,
            "{kind:?}'s dialog escapes the window"
        );
        assert!(l.frame.y >= chip().bottom() - 0.001 || l.frame.bottom() <= chip().y + 0.001);
        for (row, rect) in &l.rows {
            assert_eq!(
                rect.intersection(&l.frame),
                *rect,
                "{row:?} is drawn outside {kind:?}'s dialog"
            );
        }
    }
}
