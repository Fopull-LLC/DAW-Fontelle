//! The roll over a vocal part, as a power user drives it: the lanes under
//! the notes (a tab each, greyed and saying why when the voice ignores
//! it; pencil, line and eraser strokes), vibrato and glide handles on the
//! notes, the note menu, and the phoneme row — all tested without a window.

use fontelle_model::{Arena, LaneCurve, LaneKey, Note, NoteVoice};
use fontelle_sing::{
    Capabilities, GlideShape, Lane, LaneInfo, NoteOptionInfo, PhonemeEdit, Portamento, Vibrato,
};
use fontelle_types::{NoteId, PPQN};
use fontelle_ui::canvas::{
    LaneStroke, NoteHandle, PHONEME_ROW_HEIGHT, PhonemeHit, RollView, StrokeMode, VOCAL_TAB_HEIGHT,
    VocalCurve, VocalNoteRow, apply_note_row, carve_vocal_strips, curve_scale, curve_value_y,
    curve_y_value, drag_handle, note_handles, parse_phonemes, phoneme_cells, phoneme_hit,
    phoneme_suggestions, retime_phoneme, roll_layout, vibrato_wave, vocal_note_menu_rows,
    vocal_tab_at, vocal_tabs,
};
use fontelle_ui::layout::Rect;
use fontelle_ui::theme::Theme;

fn caps() -> Capabilities {
    Capabilities {
        lanes: vec![Lane::PitchBend, Lane::Dynamics],
        vibrato: true,
        portamento: true,
        velocity: true,
        phoneme_timing: true,
        phonemes: ["a", "i", "k", "ka", "s", "sh"]
            .iter()
            .map(|s| s.to_string())
            .collect(),
        extra_lanes: vec![LaneInfo {
            id: "growl".into(),
            name: "Growl".into(),
            min: 0.0,
            max: 1.0,
            default: 0.0,
            unit: "%".into(),
        }],
        settings: Vec::new(),
        note_options: vec![NoteOptionInfo {
            id: "colour".into(),
            name: "Voice colour".into(),
            choices: vec![
                ("plain".into(), "Plain".into()),
                ("soft".into(), "Soft".into()),
            ],
        }],
    }
}

fn note(start: i64, key: u8) -> Note {
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
        lyric: "ka".into(),
        voice: None,
        muted: false,
    }
}

fn centre(r: Rect) -> (f32, f32) {
    (r.x + r.width / 2.0, r.y + r.height / 2.0)
}

// ------------------------------------------------------------- the strips

#[test]
fn a_vocal_roll_gives_the_phoneme_row_and_the_tabs_room_of_their_own() {
    let theme = Theme::dark_default();
    let frame = Rect::new(0.0, 0.0, 900.0, 600.0);
    let plain = roll_layout(frame, &theme.metrics, 140.0);
    let mut vocal = plain;
    let strips = carve_vocal_strips(&mut vocal);
    assert_eq!(strips.phonemes.height, PHONEME_ROW_HEIGHT);
    assert_eq!(strips.phonemes.y, vocal.grid.bottom());
    assert_eq!(vocal.grid.height, plain.grid.height - PHONEME_ROW_HEIGHT);
    assert_eq!(strips.tabs.height, VOCAL_TAB_HEIGHT);
    assert_eq!(strips.tabs.y, plain.velocity.y);
    assert_eq!(vocal.velocity.y, plain.velocity.y + VOCAL_TAB_HEIGHT);
    assert_eq!(vocal.velocity.bottom(), plain.velocity.bottom());
    // With the lane hidden there are no tabs, and the phoneme row stays.
    let mut hidden = roll_layout(frame, &theme.metrics, 0.0);
    let strips = carve_vocal_strips(&mut hidden);
    assert!(strips.tabs.is_empty());
    assert!(!strips.phonemes.is_empty());
}

#[test]
fn every_lane_has_a_tab_and_the_ones_the_voice_ignores_say_why() {
    let strip = Rect::new(100.0, 400.0, 800.0, VOCAL_TAB_HEIGHT);
    let tabs = vocal_tabs(strip, &caps(), &|s| s.len() as f32 * 6.0);
    // The notes' own lane first, the six standard lanes, the voice's own.
    assert_eq!(tabs.len(), 1 + 6 + 1);
    assert_eq!(tabs[0].curve, None);
    assert_eq!(tabs[1].curve, Some(VocalCurve::Standard(Lane::PitchBend)));
    assert!(tabs[1].honoured && tabs[1].why.is_none());
    let breath = tabs
        .iter()
        .find(|t| t.curve == Some(VocalCurve::Standard(Lane::Breathiness)))
        .unwrap();
    assert!(!breath.honoured);
    assert!(breath.why.as_deref().unwrap().contains("ignores"));
    let growl = tabs.last().unwrap();
    assert_eq!(growl.curve, Some(VocalCurve::Extra("growl".into())));
    assert_eq!(growl.label, "Growl");
    // Laid out left to right, without overlapping.
    for pair in tabs.windows(2) {
        assert!(pair[0].rect.right() <= pair[1].rect.x);
    }
    let middle = tabs[3].rect;
    assert_eq!(vocal_tab_at(&tabs, middle.x + 2.0, middle.y + 2.0), Some(3));
    assert_eq!(vocal_tab_at(&tabs, 0.0, 0.0), None);
    // The key a tab's lane is stored under.
    assert_eq!(
        VocalCurve::Extra("growl".into()).key("Eng"),
        LaneKey::Extra {
            backend: "Eng".into(),
            id: "growl".into()
        }
    );
}

#[test]
fn a_lanes_values_and_heights_turn_into_each_other() {
    let plot = Rect::new(0.0, 100.0, 500.0, 100.0);
    let dynamics = curve_scale(&VocalCurve::Standard(Lane::Dynamics), &caps());
    assert_eq!(dynamics.neutral, 0.0);
    assert_eq!(dynamics.unit, "dB");
    let y = curve_value_y(&dynamics, plot, 0.0);
    assert!((curve_y_value(&dynamics, plot, y) - 0.0).abs() < 1e-3);
    assert_eq!(curve_value_y(&dynamics, plot, dynamics.max), plot.y);
    assert_eq!(curve_value_y(&dynamics, plot, dynamics.min), plot.bottom());
    assert_eq!(
        curve_y_value(&dynamics, plot, -500.0),
        dynamics.max,
        "clamped"
    );
    let growl = curve_scale(&VocalCurve::Extra("growl".into()), &caps());
    assert_eq!((growl.min, growl.max, growl.unit.as_str()), (0.0, 1.0, "%"));
}

// ------------------------------------------------------------- strokes

#[test]
fn a_pencil_stroke_follows_the_hand_and_leaves_the_rest() {
    let mut s = LaneStroke::begin(
        LaneCurve::default(),
        StrokeMode::Pencil,
        (100, 1.0),
        0.0,
        10,
    );
    s.drag((200, 1.0));
    s.drag((300, 3.0));
    let c = s.curve();
    assert!((c.at(150, 0.0) - 1.0).abs() < 1e-4);
    assert!((c.at(250, 0.0) - 2.0).abs() < 0.05, "a line between moves");
    assert_eq!(c.at(50, 0.0), 0.0);
    assert_eq!(c.at(400, 0.0), 0.0);
}

#[test]
fn a_line_stroke_is_one_straight_line_from_where_it_began() {
    let mut base = LaneCurve::default();
    base.write(0, 1000, 10, |_| 0.5, 0.0);
    let mut s = LaneStroke::begin(base, StrokeMode::Line, (200, 0.0), 0.0, 10);
    s.drag((900, 9.0));
    s.drag((600, 4.0));
    let c = s.curve();
    assert!((c.at(400, 0.0) - 2.0).abs() < 0.05);
    assert!(
        (c.at(800, 0.0) - 0.5).abs() < 1e-4,
        "the earlier reach is undone"
    );
    assert!((c.at(100, 0.0) - 0.5).abs() < 1e-4);
}

#[test]
fn an_eraser_stroke_returns_what_it_crosses_to_neutral() {
    let mut base = LaneCurve::default();
    base.write(0, 1000, 10, |_| 0.5, 0.0);
    let mut s = LaneStroke::begin(base, StrokeMode::Erase, (300, 0.9), 0.0, 10);
    s.drag((500, 0.1));
    let c = s.curve();
    assert_eq!(c.at(400, 0.0), 0.0);
    assert!((c.at(100, 0.0) - 0.5).abs() < 1e-4);
    assert!((c.at(700, 0.0) - 0.5).abs() < 1e-4);
}

// ------------------------------------------------------- note handles

fn voiced() -> NoteVoice {
    NoteVoice {
        vibrato: Some(Vibrato {
            depth: 50.0,
            rate: 6.0,
            delay: 0.5,
            fade_in: 0.2,
            fade_out: 0.2,
        }),
        portamento: Some(Portamento {
            length: 0.1,
            position: 0.5,
            shape: GlideShape::S,
        }),
        ..NoteVoice::default()
    }
}

#[test]
fn a_notes_vibrato_and_glide_have_handles_where_they_are() {
    let rect = Rect::new(200.0, 300.0, 200.0, 12.0);
    let handles = note_handles(rect, Some(&voiced()), 400.0, true);
    let at = |h: NoteHandle| {
        handles
            .iter()
            .find(|(k, _)| *k == h)
            .map(|(_, r)| *r)
            .unwrap()
    };
    // The delay is half way; the fades a fifth of what follows each way.
    assert!((centre(at(NoteHandle::Delay)).0 - 300.0).abs() < 1.0);
    assert!((centre(at(NoteHandle::FadeIn)).0 - 320.0).abs() < 1.0);
    assert!((centre(at(NoteHandle::FadeOut)).0 - 380.0).abs() < 1.0);
    // Depth sits above the note by the depth (half a row for 50 cents).
    assert!(centre(at(NoteHandle::Depth)).1 < rect.y);
    // The glide's start, ahead of the note: 0.1 s at 400 px/s, centred.
    assert!((centre(at(NoteHandle::Glide)).0 - (200.0 - 20.0)).abs() < 1.0);
    // No vibrato: only the glide.
    let bare = note_handles(rect, None, 400.0, true);
    assert_eq!(bare.len(), 1);
    assert_eq!(bare[0].0, NoteHandle::Glide);
    assert!(note_handles(rect, None, 400.0, false).is_empty());
}

#[test]
fn dragging_a_handle_changes_just_that_and_stays_in_range() {
    let rect = Rect::new(200.0, 300.0, 200.0, 12.0);
    let start = voiced();
    let up = drag_handle(NoteHandle::Depth, &start, 0.0, -12.0, rect, 400.0);
    assert!(
        (up.vibrato.unwrap().depth - 150.0).abs() < 0.5,
        "a row up is 100 cents more"
    );
    assert_eq!(up.vibrato.unwrap().rate, 6.0);
    let late = drag_handle(NoteHandle::Delay, &start, 1000.0, 0.0, rect, 400.0);
    assert!(late.vibrato.unwrap().delay <= 0.95);
    let fast = drag_handle(NoteHandle::Rate, &start, 24.0, 0.0, rect, 400.0);
    assert!(fast.vibrato.unwrap().rate > 6.0);
    let longer = drag_handle(NoteHandle::Glide, &start, -20.0, 0.0, rect, 400.0);
    assert!(longer.portamento.unwrap().length > 0.1);
    let none = drag_handle(NoteHandle::Glide, &start, 5000.0, 0.0, rect, 400.0);
    assert!(none.portamento.unwrap().length >= 0.0);
}

#[test]
fn a_vibrato_is_drawn_as_a_wave_as_deep_as_it_is() {
    let rect = Rect::new(0.0, 100.0, 400.0, 10.0);
    let v = Vibrato {
        depth: 100.0,
        rate: 5.0,
        delay: 0.0,
        fade_in: 0.0,
        fade_out: 0.0,
    };
    let wave = vibrato_wave(rect, &v, 400.0);
    assert!(wave.len() > 50);
    let centre = rect.y + rect.height / 2.0;
    let peak = wave
        .iter()
        .map(|(_, y)| (y - centre).abs())
        .fold(0.0, f32::max);
    // 100 cents is a row: the wave swings a whole row either way.
    assert!((peak - 10.0).abs() < 0.5, "{peak}");
}

// ----------------------------------------------------------- note menu

#[test]
fn the_note_menu_offers_what_the_voice_answers_to() {
    let rows = vocal_note_menu_rows(None, &caps(), "Eng");
    let labels: Vec<&str> = rows.iter().map(|(e, _)| e.label.as_str()).collect();
    assert!(
        labels.iter().any(|l| l.contains("Add vibrato")),
        "{labels:?}"
    );
    assert!(
        rows.iter()
            .any(|(_, r)| *r == VocalNoteRow::Glide(Some(GlideShape::Linear)))
    );
    assert!(rows.iter().any(|(_, r)| *r == VocalNoteRow::EditPhonemes));
    assert!(
        rows.iter()
            .any(|(_, r)| matches!(r, VocalNoteRow::NoteOption { value, .. } if value == "soft"))
    );
    // With a vibrato, it can be taken off.
    let rows = vocal_note_menu_rows(Some(&voiced()), &caps(), "Eng");
    assert!(rows.iter().any(|(_, r)| *r == VocalNoteRow::Vibrato(false)));
    // A voice that ignores vibrato says so, greyed.
    let deaf = Capabilities::default();
    let rows = vocal_note_menu_rows(None, &deaf, "Eng");
    let vib = rows
        .iter()
        .find(|(e, _)| e.label.contains("vibrato"))
        .unwrap();
    assert!(!vib.0.enabled);
}

#[test]
fn a_note_menu_row_makes_the_notes_new_voice() {
    let added = apply_note_row(&VocalNoteRow::Vibrato(true), None, "Eng")
        .unwrap()
        .unwrap();
    assert_eq!(added.vibrato, Some(Vibrato::default()));
    let gone = apply_note_row(&VocalNoteRow::Vibrato(false), Some(&added), "Eng").unwrap();
    assert!(gone.is_none(), "an empty voice is none");
    let linear = apply_note_row(&VocalNoteRow::Glide(Some(GlideShape::Linear)), None, "Eng")
        .unwrap()
        .unwrap();
    assert_eq!(linear.portamento.unwrap().shape, GlideShape::Linear);
    let soft = apply_note_row(
        &VocalNoteRow::NoteOption {
            id: "colour".into(),
            value: "soft".into(),
        },
        None,
        "Eng",
    )
    .unwrap()
    .unwrap();
    assert_eq!(soft.options["Eng"]["colour"], "soft");
    assert_eq!(
        apply_note_row(&VocalNoteRow::EditPhonemes, None, "Eng"),
        None
    );
}

// ---------------------------------------------------------- phoneme row

#[test]
fn each_sung_phoneme_has_a_cell_where_it_starts() {
    let mut notes = Arena::<NoteId, Note>::default();
    let a = notes.insert(note(0, 60));
    let b = notes.insert(note(PPQN, 62));
    let view = RollView::default();
    let grid = Rect::new(50.0, 0.0, 2000.0, 400.0);
    let row = Rect::new(50.0, 400.0, 2000.0, PHONEME_ROW_HEIGHT);
    // 1920 ticks a second (120 bpm): "k" starts 0.05 s before b, "a" on it.
    let timed = vec![
        (a, vec![("k".to_string(), None), ("a".to_string(), None)]),
        (
            b,
            vec![("k".to_string(), Some(-0.05)), ("a".to_string(), Some(0.0))],
        ),
    ];
    let cells = phoneme_cells(row, &view, grid, &notes, &timed, 1920.0);
    assert_eq!(cells.len(), 4);
    // Untimed: shared out over the note.
    assert!(cells[1].rect.x > cells[0].rect.x);
    let b_x = fontelle_ui::canvas::tick_to_x(&view, grid, PPQN);
    let k = &cells[2];
    let ahead = fontelle_ui::canvas::tick_to_x(&view, grid, PPQN - 96);
    assert!((k.rect.x - ahead).abs() < 1.0, "{} vs {}", k.rect.x, ahead);
    assert!((cells[3].rect.x - b_x).abs() < 1.0);
    // A boundary is grabbed only where the voice honours timing.
    let at = cells[3].rect.x;
    assert_eq!(
        phoneme_hit(&cells, at + 1.0, row.y + 4.0, true),
        Some(PhonemeHit::Boundary { note: b, index: 1 })
    );
    assert_eq!(
        phoneme_hit(&cells, at + 1.0, row.y + 4.0, false),
        Some(PhonemeHit::Cell { note: b, index: 1 })
    );
    assert_eq!(phoneme_hit(&cells, at + 1.0, row.y - 30.0, true), None);
}

#[test]
fn typed_phonemes_are_read_and_suggested_from_the_voices_own() {
    assert_eq!(
        parse_phonemes("[k a]"),
        Some(vec![
            PhonemeEdit {
                symbol: "k".into(),
                offset: None
            },
            PhonemeEdit {
                symbol: "a".into(),
                offset: None
            },
        ])
    );
    assert_eq!(
        parse_phonemes("  "),
        None,
        "empty is the voice's own reading"
    );
    let inventory = caps().phonemes;
    assert_eq!(phoneme_suggestions(&inventory, "a s", 5), vec!["s", "sh"]);
    assert_eq!(phoneme_suggestions(&inventory, "k", 5), vec!["k", "ka"]);
    assert!(phoneme_suggestions(&inventory, "", 5).is_empty());
}

#[test]
fn dragging_a_boundary_retimes_that_phoneme_between_its_neighbours() {
    let sung = vec![
        ("k".to_string(), Some(-0.05)),
        ("a".to_string(), Some(0.0)),
        ("n".to_string(), Some(0.3)),
    ];
    let edits = retime_phoneme(&sung, None, 1, 0.1);
    assert_eq!(edits.len(), 3);
    assert_eq!(edits[1].offset, Some(0.1));
    assert_eq!(edits[0].symbol, "k");
    // Never past the next one.
    let edits = retime_phoneme(&sung, None, 1, 0.9);
    assert!(edits[1].offset.unwrap() < 0.3);
    // An edit already made is the starting point.
    let mine = vec![
        PhonemeEdit {
            symbol: "g".into(),
            offset: Some(-0.08),
        },
        PhonemeEdit {
            symbol: "a".into(),
            offset: Some(0.0),
        },
    ];
    let edits = retime_phoneme(&sung, Some(&mine), 0, -0.02);
    assert_eq!(edits[0].symbol, "g");
    assert_eq!(edits[0].offset, Some(-0.02));
}
