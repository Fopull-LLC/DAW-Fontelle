//! The piano roll's tools, without a window: the menu, each dialog's rows,
//! what a press on them does, and what a tool answers.
//!
//! `docs/roll-tools-plan.md` (Ty, 2026-10-20: *"spend some time putting
//! together a plan to make the tools section start feeling feature
//! complete"*). Everything here is pure: the dialog is geometry, the settings
//! are numbers, and a tool's answer is a `ToolResult` — the same answer the
//! roll ghosts, *Hear it* plays and Apply writes.

use fontelle_model::{
    Arena, ChopSpec, FlamSpec, LevelsMode, Note, StrumDirection, ToolResult, chopped, glued,
    quantized, strummed,
};
use fontelle_types::{NoteId, PPQN};
use fontelle_ui::canvas::{
    Keymap, TOOL_MENU, ToolAction, ToolControl, ToolKind, ToolMenuItem, ToolPress, ToolRow, Tools,
    pattern_cells, step_arrows, tools_dialog_hit, tools_dialog_layout, tools_dialog_place,
    tools_dialog_press, tools_dialog_size,
};
use fontelle_ui::layout::Rect;
use fontelle_ui::theme::Theme;

fn metrics() -> fontelle_ui::theme::Metrics {
    Theme::dark_default().metrics
}

fn bounds() -> Rect {
    Rect::new(0.0, 0.0, 1000.0, 700.0)
}

fn note(start: i64, length: i64, key: u8) -> Note {
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

fn scope(notes: Vec<Note>) -> Vec<(NoteId, Note)> {
    let mut arena: Arena<NoteId, Note> = Arena::new();
    notes
        .into_iter()
        .map(|n| (arena.insert(n.clone()), n))
        .collect()
}

/// Two bars of triads, a bar each: something every tool has an answer for.
fn chords() -> Vec<(NoteId, Note)> {
    let bar = PPQN * 4;
    scope(vec![
        note(0, bar, 60),
        note(0, bar, 64),
        note(0, bar, 67),
        note(bar, bar, 62),
        note(bar, bar, 65),
        note(bar, bar, 69),
    ])
}

fn dialog(kind: ToolKind, tools: &Tools) -> fontelle_ui::canvas::ToolsDialog {
    let rows = tools.rows(kind);
    tools_dialog_layout(
        kind,
        &rows,
        kind.title().to_string(),
        (300.0, 40.0),
        bounds(),
        &metrics(),
    )
}

fn centre(r: Rect) -> (f32, f32) {
    (r.x + r.width / 2.0, r.y + r.height / 2.0)
}

// ---------------------------------------------------------------- the menu ---

#[test]
fn every_tool_is_on_the_menu_under_its_heading() {
    let headings: Vec<&str> = TOOL_MENU
        .iter()
        .filter_map(|item| match item {
            ToolMenuItem::Heading(h) => Some(*h),
            _ => None,
        })
        .collect();
    assert_eq!(
        headings,
        vec![
            "Timing", "Pitch", "Levels", "Length", "Shape", "Generate", "Select", "Files"
        ]
    );
    for kind in ToolKind::ALL {
        assert!(
            TOOL_MENU.contains(&ToolMenuItem::Open(kind)),
            "{kind:?} cannot be reached"
        );
    }
    assert!(matches!(TOOL_MENU[0], ToolMenuItem::Heading(_)));
}

#[test]
fn the_files_are_under_their_own_heading_at_the_end() {
    let files = TOOL_MENU
        .iter()
        .position(|i| *i == ToolMenuItem::Heading("Files"))
        .unwrap();
    let after: Vec<ToolMenuItem> = TOOL_MENU[files + 1..].to_vec();
    assert_eq!(
        after,
        vec![
            ToolMenuItem::Run(ToolAction::ImportMidi),
            ToolMenuItem::Run(ToolAction::ImportScore),
            ToolMenuItem::Run(ToolAction::ExportMidi),
        ]
    );
}

#[test]
fn a_menu_row_carries_its_shortcut() {
    let keymap = Keymap::default();
    let label = |item: ToolMenuItem| item.label_in(&keymap);
    assert_eq!(
        label(ToolMenuItem::Run(ToolAction::Glue)),
        "Glue \u{2014} Ctrl+G"
    );
    assert_eq!(
        label(ToolMenuItem::Run(ToolAction::QuickChop)),
        "Quick chop \u{2014} Ctrl+U"
    );
    assert_eq!(
        label(ToolMenuItem::Run(ToolAction::QuantizeStarts)),
        "Quantize starts \u{2014} Shift+Q"
    );
    assert_eq!(
        label(ToolMenuItem::Open(ToolKind::Arpeggiate)),
        "Arpeggiate\u{2026} \u{2014} Alt+A"
    );
    assert_eq!(
        label(ToolMenuItem::Open(ToolKind::Chop)),
        "Chop\u{2026} \u{2014} Alt+U"
    );
    assert_eq!(
        label(ToolMenuItem::Open(ToolKind::Quantize)),
        "Quantize\u{2026} \u{2014} Ctrl+Q"
    );
    assert_eq!(
        label(ToolMenuItem::Run(ToolAction::Legato)),
        "Legato \u{2014} Ctrl+L"
    );
    assert_eq!(label(ToolMenuItem::Heading("Shape")), "Shape");
}

// ----------------------------------------------------------------- the rows ---

#[test]
fn every_row_of_every_dialog_has_a_name_a_value_and_a_tip() {
    let mut tools = Tools {
        song_key: Some(0b1010_1011_0101),
        ..Tools::default()
    };
    tools.strum.direction = StrumDirection::Random;
    for kind in ToolKind::ALL {
        for row in tools.rows(kind) {
            assert!(!tools.label(row).is_empty(), "{kind:?} {row:?} has no name");
            if tools.control(row) == ToolControl::Steps {
                assert!(
                    !tools.value(row).is_empty(),
                    "{kind:?} {row:?} has no value"
                );
            }
            assert!(!tools.tip(row).is_empty(), "{kind:?} {row:?} has no tip");
        }
    }
    for row in [ToolRow::Reset, ToolRow::Hear, ToolRow::Apply] {
        assert!(!tools.tip(row).is_empty());
        assert_eq!(tools.control(row), ToolControl::Button);
    }
}

#[test]
fn a_dialog_shows_only_the_rows_its_mode_uses() {
    let mut tools = Tools::default();
    tools.levels.mode = LevelsMode::Add;
    let add = tools.rows(ToolKind::Levels);
    assert!(add.contains(&ToolRow::LevelsAmount));
    assert!(!add.contains(&ToolRow::LevelsLow));
    tools.levels.mode = LevelsMode::Limit;
    let limit = tools.rows(ToolKind::Levels);
    assert!(limit.contains(&ToolRow::LevelsLow) && limit.contains(&ToolRow::LevelsHigh));
    assert!(!limit.contains(&ToolRow::LevelsAmount));
    tools.levels.mode = LevelsMode::Ramp;
    let ramp = tools.rows(ToolKind::Levels);
    assert!(ramp.contains(&ToolRow::LevelsFrom) && ramp.contains(&ToolRow::LevelsTo));
}

#[test]
fn scale_steps_are_offered_only_when_the_song_has_a_key() {
    let mut tools = Tools::default();
    assert!(!tools.rows(ToolKind::Transpose).contains(&ToolRow::InKey));
    tools.song_key = Some(0b1010_1011_0101);
    assert!(tools.rows(ToolKind::Transpose).contains(&ToolRow::InKey));
}

#[test]
fn a_seed_is_shown_where_there_is_something_random() {
    let mut tools = Tools::default();
    assert!(tools.rows(ToolKind::Humanize).contains(&ToolRow::Seed));
    assert!(tools.rows(ToolKind::Shuffle).contains(&ToolRow::Seed));
    assert!(!tools.rows(ToolKind::Strum).contains(&ToolRow::Seed));
    tools.strum.direction = StrumDirection::Random;
    assert!(tools.rows(ToolKind::Strum).contains(&ToolRow::Seed));
}

#[test]
fn values_are_said_in_words_people_use() {
    let tools = Tools {
        tempo: 120.0,
        ..Tools::default()
    };
    assert_eq!(tools.value(ToolRow::Property), "Velocity");
    assert_eq!(tools.value(ToolRow::RandomMode), "Wobble");
    assert!(
        tools.value(ToolRow::StrumTime).ends_with(" ms"),
        "{}",
        tools.value(ToolRow::StrumTime)
    );
    assert_eq!(tools.label(ToolRow::Semitones), "Semitones");
    assert_eq!(tools.value(ToolRow::Semitones), "+12");
    let steps = Tools {
        in_key: true,
        song_key: Some(0b1010_1011_0101),
        ..Tools::default()
    };
    assert_eq!(steps.label(ToolRow::Semitones), "Scale steps");
}

#[test]
fn a_number_stops_at_its_ends_and_a_choice_goes_round() {
    let mut tools = Tools::default();
    for _ in 0..60 {
        tools.nudge(ToolRow::Semitones, 1);
    }
    assert_eq!(tools.semitones, 24);
    let first = tools.levels.mode;
    let mut seen = 0;
    loop {
        tools.nudge(ToolRow::LevelsMode, 1);
        seen += 1;
        if tools.levels.mode == first {
            break;
        }
        assert!(seen < 20, "the modes never came round");
    }
    assert_eq!(seen, 6, "six modes, then the first again");
    tools.nudge(ToolRow::Seed, -1);
    tools.nudge(ToolRow::Seed, -1);
    assert!(tools.humanize.seed >= 1);
}

#[test]
fn the_pattern_row_is_cells_that_toggle() {
    let mut tools = Tools::default();
    assert_eq!(tools.control(ToolRow::ChopPattern), ToolControl::Pattern);
    let before = tools.chop.pattern;
    tools.toggle_cell(ToolRow::ChopPattern, 1);
    assert_eq!(tools.chop.pattern, before ^ 0b10);
    tools.toggle_cell(ToolRow::ChopPattern, 1);
    assert_eq!(tools.chop.pattern, before);
}

#[test]
fn reset_puts_one_tool_back_and_leaves_the_others() {
    let mut tools = Tools::default();
    tools.nudge(ToolRow::StrumTime, 1);
    tools.nudge(ToolRow::ChopGate, -1);
    tools.reset(ToolKind::Strum);
    assert_eq!(tools.strum, Tools::default().strum);
    assert_ne!(tools.chop, Tools::default().chop);
}

// ------------------------------------------------------------ the answers ---

#[test]
fn every_dialog_has_an_answer_at_its_defaults() {
    let tools = Tools {
        clip_end: Some(PPQN * 8),
        ..Tools::default()
    };
    // A little off the grid, so Quantize has something to do too.
    let gappy = scope(
        (0..8)
            .map(|i| note(i * PPQN / 4 + 7, 60, 60 + (i % 4) as u8))
            .collect(),
    );
    for kind in ToolKind::ALL {
        let a = tools.result(kind, &chords());
        let b = tools.result(kind, &gappy);
        assert!(
            !(a.is_empty() && b.is_empty()),
            "{kind:?} did nothing at its defaults"
        );
    }
}

#[test]
fn a_dialog_answers_with_the_models_own_function() {
    let tools = Tools::default();
    let s = chords();
    assert_eq!(tools.result(ToolKind::Strum, &s), strummed(&s, tools.strum));
    assert_eq!(tools.result(ToolKind::Chop, &s), chopped(&s, tools.chop));
}

#[test]
fn applying_a_random_tool_rolls_the_next_one() {
    let mut tools = Tools::default();
    let s = chords();
    let first = tools.result(ToolKind::Humanize, &s);
    tools.applied(ToolKind::Humanize);
    assert_ne!(first, tools.result(ToolKind::Humanize, &s));
}

#[test]
fn the_quick_tools_use_the_snap() {
    let tools = Tools {
        roll_grid: PPQN / 4,
        quantize: fontelle_model::QuantizeSpec {
            strength: 0.5,
            ..Default::default()
        },
        ..Tools::default()
    };
    let s = scope(vec![note(0, PPQN, 60), note(PPQN, PPQN, 60)]);
    assert_eq!(
        tools.instant(ToolAction::QuickChop, &s),
        Some(chopped(
            &s,
            ChopSpec {
                step: PPQN / 4,
                ..ChopSpec::default()
            }
        ))
    );
    assert_eq!(tools.instant(ToolAction::Glue, &s), Some(glued(&s)));
    let late = scope(vec![note(PPQN / 4 + 30, 100, 60)]);
    let starts = tools.instant(ToolAction::QuantizeStarts, &late).unwrap();
    let full = quantized(
        &[(PPQN / 4 + 30, 100)],
        fontelle_model::QuantizeSpec {
            grid: PPQN / 4,
            ..Default::default()
        },
    );
    assert_eq!(
        starts.after(&late)[0].start,
        full[0].0,
        "full strength, whatever the dialog says"
    );
    let _ = FlamSpec::default();
    let _ = ToolResult::default();
}

#[test]
fn settings_survive_a_round_trip_and_garbage_is_refused() {
    let mut tools = Tools::default();
    tools.nudge(ToolRow::ChopGate, -1);
    tools.nudge(ToolRow::StrumDirection, 1);
    tools.roll_grid = 7;
    let back = Tools::from_json(&tools.to_json()).expect("it reads back");
    assert_eq!(back.chop, tools.chop);
    assert_eq!(back.strum, tools.strum);
    assert_eq!(
        back.roll_grid,
        Tools::default().roll_grid,
        "what the window sets is not kept"
    );
    assert!(Tools::from_json("not json").is_none());
    let partial = Tools::from_json("{\"semitones\": 5}").expect("missing is default");
    assert_eq!(partial.semitones, 5);
    assert_eq!(partial.chop, Tools::default().chop);
}

// ------------------------------------------------------------- geometry ---

#[test]
fn the_dialog_is_where_it_was_put_and_inside_the_window() {
    let tools = Tools::default();
    let d = dialog(ToolKind::Strum, &tools);
    assert_eq!((d.frame.x, d.frame.y), (300.0, 40.0));
    assert!(d.frame.right() <= bounds().right() && d.frame.bottom() <= bounds().bottom());
    let far = tools_dialog_layout(
        ToolKind::Strum,
        &tools.rows(ToolKind::Strum),
        String::new(),
        (990.0, 690.0),
        bounds(),
        &metrics(),
    );
    assert!(far.frame.right() <= bounds().right() + 0.01);
    assert!(far.frame.bottom() <= bounds().bottom() + 0.01);
}

#[test]
fn the_buttons_share_the_bottom_row() {
    let tools = Tools::default();
    let d = dialog(ToolKind::Chop, &tools);
    let at = |row| d.rows.iter().find(|(r, _)| *r == row).unwrap().1;
    let (reset, hear, apply) = (at(ToolRow::Reset), at(ToolRow::Hear), at(ToolRow::Apply));
    assert_eq!(reset.y, hear.y);
    assert_eq!(hear.y, apply.y);
    assert!(reset.right() <= hear.x && hear.right() <= apply.x);
    let settings_bottom = d
        .rows
        .iter()
        .filter(|(r, _)| !matches!(r, ToolRow::Reset | ToolRow::Hear | ToolRow::Apply))
        .map(|(_, rect)| rect.bottom())
        .fold(0.0, f32::max);
    assert!(apply.y >= settings_bottom - 0.01, "under the settings");
}

#[test]
fn a_value_row_has_arrows_that_step_it_and_a_middle_that_is_the_row() {
    let tools = Tools::default();
    let d = dialog(ToolKind::Strum, &tools);
    let rect = d
        .rows
        .iter()
        .find(|(r, _)| *r == ToolRow::StrumTime)
        .unwrap()
        .1;
    let (back, value, forward) = step_arrows(rect);
    assert!(back.right() <= value.x && value.right() <= forward.x);
    let press = |(x, y)| tools_dialog_press(&d, &tools, x, y);
    assert_eq!(
        press(centre(back)),
        Some(ToolPress::Step(ToolRow::StrumTime, -1))
    );
    assert_eq!(
        press(centre(forward)),
        Some(ToolPress::Step(ToolRow::StrumTime, 1))
    );
    assert_eq!(
        press(centre(value)),
        Some(ToolPress::Row(ToolRow::StrumTime))
    );
    assert_eq!(press(centre(d.title)), Some(ToolPress::Title));
    assert_eq!(press((5.0, 690.0)), None);
    assert_eq!(
        tools_dialog_hit(&d, centre(value).0, centre(value).1),
        Some(ToolRow::StrumTime)
    );
}

#[test]
fn a_press_on_a_pattern_cell_is_that_cell() {
    let tools = Tools::default();
    let d = dialog(ToolKind::Chop, &tools);
    let rect = d
        .rows
        .iter()
        .find(|(r, _)| *r == ToolRow::ChopPattern)
        .unwrap()
        .1;
    let cells = pattern_cells(rect, tools.chop.pattern_len);
    assert_eq!(cells.len(), usize::from(tools.chop.pattern_len));
    for (i, cell) in cells.iter().enumerate() {
        assert!(rect.contains(cell.x, cell.y));
        let (x, y) = centre(*cell);
        assert_eq!(
            tools_dialog_press(&d, &tools, x, y),
            Some(ToolPress::Cell(ToolRow::ChopPattern, i))
        );
    }
}

#[test]
fn the_dialog_opens_on_the_side_away_from_the_selection() {
    let grid = Rect::new(100.0, 100.0, 800.0, 400.0);
    let size = tools_dialog_size(&Tools::default().rows(ToolKind::Strum), &metrics());
    let right = tools_dialog_place(size, grid, Some((600.0, 850.0)), None, bounds());
    assert!(
        right.0 + size.0 <= 600.0,
        "selection on the right, dialog on the left"
    );
    let left = tools_dialog_place(size, grid, Some((150.0, 300.0)), None, bounds());
    assert!(
        left.0 >= 300.0,
        "selection on the left, dialog on the right"
    );
    let chosen = tools_dialog_place(
        size,
        grid,
        Some((150.0, 300.0)),
        Some((20.0, 30.0)),
        bounds(),
    );
    assert_eq!(chosen, (20.0, 30.0), "where it was dragged to wins");
    let off = tools_dialog_place(size, grid, None, Some((5000.0, -40.0)), bounds());
    assert!(
        off.0 + size.0 <= bounds().right() + 0.01 && off.1 >= 0.0,
        "but never off the window"
    );
}

#[test]
fn a_window_too_short_for_the_dialog_still_shows_what_it_can() {
    let tools = Tools::default();
    let short = Rect::new(0.0, 0.0, 800.0, 90.0);
    let d = tools_dialog_layout(
        ToolKind::Arpeggiate,
        &tools.rows(ToolKind::Arpeggiate),
        String::new(),
        (10.0, 0.0),
        short,
        &metrics(),
    );
    assert!(d.frame.bottom() <= short.bottom() + 0.01);
    assert!(d.rows.iter().any(|(_, r)| !r.is_empty()));
    for (_, rect) in &d.rows {
        assert!(
            rect.is_empty()
                || (d.frame.contains(rect.x, rect.y) && rect.bottom() <= d.frame.bottom() + 0.01)
        );
    }
}

#[test]
fn mute_mutes_what_is_playing_and_unmutes_what_is_all_muted() {
    let tools = Tools::default();
    let s = scope(vec![
        note(0, 240, 60),
        Note {
            muted: true,
            ..note(240, 240, 62)
        },
    ]);
    let muted = tools.instant(ToolAction::ToggleMute, &s).unwrap();
    assert!(
        muted.after(&s).iter().all(|n| n.muted),
        "any playing: mute them all"
    );
    let all = scope(muted.after(&s));
    let back = tools.instant(ToolAction::ToggleMute, &all).unwrap();
    assert!(
        back.after(&all).iter().all(|n| !n.muted),
        "all muted: let them play"
    );
    assert_eq!(
        ToolMenuItem::Run(ToolAction::ToggleMute).label_in(&Keymap::default()),
        "Mute or unmute \u{2014} Ctrl+Shift+M"
    );
}
