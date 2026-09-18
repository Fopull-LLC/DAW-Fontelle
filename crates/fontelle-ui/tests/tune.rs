//! The pitch corrector's window (`docs/tune-plan.md` §7.7).
//!
//! Pure geometry and pure answers. What is measured here is what a person can
//! reach: that the whole console fits the window it opens at, that every
//! control is where the layout says, that the keyboard says what the scale is,
//! and that the trace draws the note rather than a line along the floor.
//!
//! The first of these is the one that would have caught Flopsynth's first
//! build, which passed every layout test and opened with a card wrapped off
//! the bottom.

use fontelle_ui::canvas::{
    FlopsynthCard, FlopsynthPicture, InstrumentGroup, InstrumentParam, ParamKind, TuneHit,
    TuneView, key_click_mask, key_solo_mask, trace_at, tune_hit, tune_keyboard_layout, tune_layout,
    viewport_points, viewport_rails,
};
use fontelle_ui::layout::Rect;

use fontelle_types::{
    ParamAddress, TUNE_LOCKED, TUNE_VOICED, TuneConfig, TuneFrame, TuneScale, cents_of_hz,
};

/// The size the window opens at, and the smallest it may be (§7.2).
///
/// Read off the constants the window is **actually created with** rather than
/// spelled again here: a layout measured at 960×600 while the window opened
/// at the EQ's 720×420 would pass every test in this file and open with the
/// cards wrapped off the bottom, which is the fault §7.2 was written to
/// prevent.
const OPENS_AT: (f32, f32) = (
    fontelle_ui::layout::TUNE_SIZE.0 as f32,
    fontelle_ui::layout::TUNE_SIZE.1 as f32,
);
const SMALLEST: (f32, f32) = (
    fontelle_ui::layout::TUNE_MINIMUM.0 as f32,
    fontelle_ui::layout::TUNE_MINIMUM.1 as f32,
);

fn a_param(id: &str, kind: ParamKind) -> InstrumentParam {
    InstrumentParam {
        address: ParamAddress::new(format!("mixer/track[0]/insert[0]/{id}")),
        label: id.to_string(),
        value: 0.5,
        display: "0.5".to_string(),
        kind,
        automated: false,
    }
}

fn a_card(name: &str, count: usize, row: usize, columns: usize) -> FlopsynthCard {
    FlopsynthCard {
        group: InstrumentGroup {
            name: name.to_string(),
            params: (0..count)
                .map(|index| a_param(&format!("{name}{index}"), ParamKind::Knob))
                .collect(),
        },
        picture: FlopsynthPicture::None,
        oscillator: None,
        row,
        aside: false,
        columns,
        removable: false,
        sizes: Vec::new(),
    }
}

/// The MIDI card: the source drop-down, which carries
/// [`fontelle_ui::canvas::TUNE_SOURCE`] rather than a parameter address
/// because a source is a routing edge, and the bend switch beside it.
fn a_midi_card() -> FlopsynthCard {
    let mut card = a_card("MIDI", 0, 1, 2);
    card.group.params.push(InstrumentParam {
        address: ParamAddress::new(fontelle_ui::canvas::TUNE_SOURCE),
        label: "Source".to_string(),
        value: 0.0,
        display: "no MIDI".to_string(),
        kind: ParamKind::Choice(vec!["no MIDI".to_string(), "Melody".to_string()]),
        automated: false,
    });
    card.group
        .params
        .push(a_param("midi_bend", ParamKind::Switch));
    card
}

/// The real seven-card view, at a fresh config — the shape
/// `fontelle-app/src/tune.rs` builds, spelled here so this crate can be tested
/// without the one above it.
fn a_view() -> TuneView {
    let fresh = TuneConfig::new();
    TuneView {
        title: "Vocal \u{2014} TUNE \u{b7} pitch correction".to_string(),
        cards: vec![
            // The keys are not cells: the Scale card is its three choosers
            // and the twelve switches are the keyboard.
            a_card("Input", 4, 0, 5),
            a_card("Correction", 5, 0, 5),
            a_card("Voice", 7, 0, 7),
            a_card("Scale", 3, 1, 4),
            a_midi_card(),
            a_card("Vibrato", 6, 1, 6),
            a_card("Character", 4, 1, 4),
            a_card("Output", 2, 1, 2),
        ],
        mask: fresh.active_mask(),
        root: 0,
        held: 0,
        keyboard_from: 48,
        trace: Vec::new(),
        floor_cents: cents_of_hz(100.0),
        ceiling_cents: cents_of_hz(1_000.0),
        latency_ms: 21.3,
        engine: "smooth".to_string(),
        mode: "studio".to_string(),
        sources: vec!["no MIDI".to_string(), "Melody".to_string()],
        source: 0,
    }
}

fn body(size: (f32, f32)) -> Rect {
    Rect::new(0.0, 0.0, size.0, size.1)
}

#[test]
fn the_whole_tune_window_fits_the_window_it_opens_at() {
    for size in [OPENS_AT, SMALLEST] {
        let body = body(size);
        let view = a_view();
        let layout = tune_layout(body, &view);

        assert!(!layout.viewport.is_empty(), "at {size:?} there is no trace");
        assert!(
            !layout.keyboard.is_empty(),
            "at {size:?} there is no keyboard"
        );
        for (index, card) in layout.cards.iter().enumerate() {
            assert!(
                !card.frame.is_empty(),
                "at {size:?} card {index} ({}) was not placed",
                view.cards[index].group.name
            );
            assert!(
                card.frame.bottom() <= body.bottom() + 0.5,
                "at {size:?} card {index} ({}) runs {} past the bottom",
                view.cards[index].group.name,
                card.frame.bottom() - body.bottom()
            );
            assert!(
                card.frame.right() <= body.right() + 0.5,
                "at {size:?} card {index} ({}) runs off the right",
                view.cards[index].group.name
            );
            for (param, cell) in &card.cells {
                assert!(
                    cell.bottom() <= card.frame.bottom() + 0.5,
                    "at {size:?} card {index}'s control {param} hangs off it"
                );
            }
        }
        // And no two cards on top of each other, which is the defect a card
        // count and a body height can both look right for.
        for (a, first) in layout.cards.iter().enumerate() {
            for second in layout.cards.iter().skip(a + 1) {
                assert!(
                    !first.frame.inset(0.5).intersects(&second.frame.inset(0.5)),
                    "at {size:?} two cards overlap: {:?} and {:?}",
                    first.frame,
                    second.frame
                );
            }
        }
        // The bands do not run into each other either.
        assert!(layout.viewport.bottom() <= layout.keyboard.y + 0.5);
        assert!(
            layout.keyboard.bottom()
                <= layout
                    .cards
                    .iter()
                    .map(|card| card.frame.y)
                    .fold(f32::MAX, f32::min)
                    + 0.5
        );
    }
}

#[test]
fn every_control_is_where_the_layout_says() {
    let view = a_view();
    let layout = tune_layout(body(OPENS_AT), &view);
    for (card, placed) in layout.cards.iter().enumerate() {
        for (param, cell) in &placed.cells {
            let x = cell.x + cell.width / 2.0;
            let y = cell.y + cell.height / 2.0;
            assert_eq!(
                tune_hit(&layout, &view, x, y),
                Some(TuneHit::Control {
                    card,
                    param: *param
                }),
                "the middle of card {card}'s control {param} is not that control"
            );
        }
    }
    // And every key, by the class it stands for.
    for (index, key) in layout.keys.iter().enumerate() {
        assert!(!key.is_empty(), "key {index} was not placed");
        // The middle of a natural is under its accidental neighbours' edges,
        // so a natural is tested near its bottom where nothing overlaps it.
        let y = key.bottom() - 2.0;
        let x = key.x + key.width / 2.0;
        assert_eq!(
            tune_hit(&layout, &view, x, y),
            Some(TuneHit::Key {
                class: (index % 12) as u8
            }),
            "key {index} does not hit-test as its own class"
        );
    }
}

#[test]
fn a_wide_chooser_takes_two_cells() {
    // "Baritone/Bass" is thirteen characters, wider than a cell, so the
    // range chooser is two cells wide — which is why §7.2's Input card is
    // five cells and not four.
    let wide = a_param(
        "range",
        ParamKind::Choice(vec!["soprano".to_string(), "baritone/bass".to_string()]),
    );
    let narrow = a_param("mode", ParamKind::Choice(vec!["live".to_string()]));
    assert_eq!(fontelle_ui::canvas::cell_span(&wide), 2);
    assert_eq!(fontelle_ui::canvas::cell_span(&narrow), 1);

    let mut view = a_view();
    view.cards[0].group.params = vec![wide, narrow];
    let layout = tune_layout(body(OPENS_AT), &view);
    let cells = &layout.cards[0].cells;
    assert!(
        cells[0].1.width > cells[1].1.width * 1.5,
        "the wide chooser is no wider than the narrow one"
    );
}

#[test]
fn the_keyboard_marks_the_mask_the_root_the_held_keys_and_the_target() {
    let mut view = a_view();
    // D major: F# is in and F is out.
    view.mask = TuneScale::Major.mask(2);
    view.root = 2;
    view.held = 1 << 7;
    let layout = tune_layout(body(OPENS_AT), &view);
    assert_eq!(layout.keys.len(), 24);

    // The mask is what the window draws lit, and it is octave-periodic: both
    // octaves show the same thing, which is what a scale is.
    for octave in 0..2 {
        for class in 0..12u8 {
            let index = octave * 12 + class as usize;
            let lit = view.mask & (1 << class) != 0;
            assert_eq!(
                lit,
                TuneScale::Major.mask(2) & (1 << class) != 0,
                "key {index} disagrees with the scale"
            );
        }
    }
    assert_ne!(view.mask & (1 << 6), 0, "F# is in D major");
    assert_eq!(view.mask & (1 << 5), 0, "F is not");
    assert_eq!(view.root, 2);
    assert_ne!(view.held & (1 << 7), 0, "the held G is marked");
}

#[test]
fn clicking_a_key_asks_for_a_custom_scale_with_that_class_flipped() {
    let mut view = a_view();
    view.mask = TuneScale::Major.mask(0);
    // C major has no F#; clicking it puts it in and leaves everything else.
    let with_f_sharp = key_click_mask(&view, 6);
    assert_ne!(with_f_sharp & (1 << 6), 0);
    assert_eq!(
        with_f_sharp & !(1 << 6),
        TuneScale::Major.mask(0),
        "clicking one key moved another"
    );
    // And clicking it again takes it back out.
    view.mask = with_f_sharp;
    assert_eq!(key_click_mask(&view, 6), TuneScale::Major.mask(0));
    // Shift-click is a solo: that class alone.
    assert_eq!(key_solo_mask(6), 1 << 6);
    assert_eq!(key_solo_mask(6).count_ones(), 1);
}

#[test]
fn viewport_points_put_a_pitch_on_its_rail_and_leave_gaps_where_unvoiced() {
    let mut view = a_view();
    let a3 = cents_of_hz(220.0);
    view.trace = vec![
        TuneFrame {
            sung_cents: a3,
            out_cents: a3,
            target_cents: a3,
            flags: TUNE_VOICED | TUNE_LOCKED,
        },
        TuneFrame {
            sung_cents: a3,
            out_cents: a3,
            target_cents: a3,
            flags: 0,
        },
        TuneFrame {
            sung_cents: a3 + 1200.0,
            out_cents: a3 + 1200.0,
            target_cents: a3 + 1200.0,
            flags: TUNE_VOICED,
        },
    ];
    let area = Rect::new(0.0, 0.0, 300.0, 100.0);
    let points = viewport_points(area, &view);
    assert_eq!(points.len(), 3);
    assert!(points[0].sung.is_some());
    assert!(
        points[1].sung.is_none() && points[1].corrected.is_none(),
        "an unvoiced hop is a gap, not a line along the floor"
    );
    assert!(points[2].sung.is_some());
    // Higher is higher on the picture, which is the one thing a pitch trace
    // must never get backwards.
    assert!(
        points[2].sung.unwrap() < points[0].sung.unwrap(),
        "an octave up drew lower"
    );
    // Newest at the right edge.
    assert!(points[0].x < points[2].x);
    assert!((points[2].x - area.right()).abs() < 0.5);
    assert!(points[0].locked, "the lock flag reaches the picture");

    // And the rails are where the scale's notes are, on the same axis.
    let rails = viewport_rails(area, &view);
    assert!(!rails.is_empty(), "a chromatic scale has rails");
    assert!(
        rails.iter().any(|(_, root)| *root),
        "the root's rail is marked"
    );
    for (y, _) in &rails {
        assert!(*y >= area.y - 0.5 && *y <= area.bottom() + 0.5);
    }
}

#[test]
fn an_empty_trace_draws_nothing() {
    let view = a_view();
    let area = Rect::new(0.0, 0.0, 300.0, 100.0);
    assert!(
        viewport_points(area, &view).is_empty(),
        "an offline session draws nothing rather than a flat line"
    );
    assert!(trace_at(area, &view, 10.0).is_none());
    // And an empty area draws nothing whatever the trace holds.
    let mut with_trace = a_view();
    with_trace.trace = vec![TuneFrame::default()];
    assert!(viewport_points(Rect::ZERO, &with_trace).is_empty());
}

#[test]
fn hovering_the_viewport_reads_the_frame_under_the_pointer() {
    let mut view = a_view();
    view.trace = (0..100)
        .map(|index| TuneFrame {
            sung_cents: 6_000.0 + index as f32,
            out_cents: 6_000.0,
            target_cents: 6_000.0,
            flags: TUNE_VOICED,
        })
        .collect();
    let area = Rect::new(0.0, 0.0, 300.0, 100.0);
    let left = trace_at(area, &view, area.x).expect("a frame at the left edge");
    let right = trace_at(area, &view, area.right()).expect("a frame at the right edge");
    assert!(
        left.sung_cents < right.sung_cents,
        "the oldest hop is on the left"
    );
    assert_eq!(right.sung_cents, 6_099.0);
}

#[test]
fn the_keyboard_is_two_octaves_with_the_accidentals_over_the_seams() {
    let band = Rect::new(0.0, 0.0, 700.0, 64.0);
    let keys = tune_keyboard_layout(band);
    assert_eq!(keys.len(), 24);
    // Fourteen naturals, full height; ten accidentals, shorter.
    let naturals = keys
        .iter()
        .filter(|key| (key.height - band.height).abs() < 0.5)
        .count();
    assert_eq!(naturals, 14, "two octaves have fourteen white keys");
    let accidentals = keys.iter().filter(|key| key.height < band.height).count();
    assert_eq!(accidentals, 10);
    // The naturals tile the band without gaps or overlaps.
    let mut whites: Vec<&Rect> = keys
        .iter()
        .filter(|key| (key.height - band.height).abs() < 0.5)
        .collect();
    whites.sort_by(|a, b| a.x.total_cmp(&b.x));
    assert!((whites[0].x - band.x).abs() < 0.5);
    assert!((whites[13].right() - band.right()).abs() < 0.5);
    for pair in whites.windows(2) {
        assert!(
            (pair[1].x - pair[0].right()).abs() < 0.5,
            "the white keys have a gap in them"
        );
    }
    // **And they are the right fourteen.** Counting them was not enough: the
    // mask that says which classes are black was written with its bits in the
    // wrong places, so C# and D# were laid out as white keys and D and E as
    // black ones. Every count above still passed — there were still fourteen
    // of one and ten of the other, still tiling the band — and the picture
    // was of an instrument that does not exist.
    for octave in 0..2 {
        for class in 0..12u8 {
            let key = &keys[octave * 12 + class as usize];
            let black = matches!(class, 1 | 3 | 6 | 8 | 10);
            let drawn_black = key.height < band.height - 0.5;
            assert_eq!(
                drawn_black,
                black,
                "{} is drawn as a {} key",
                [
                    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"
                ][class as usize],
                if drawn_black { "black" } else { "white" }
            );
        }
    }
    // An empty band gives empty keys rather than panicking.
    assert_eq!(tune_keyboard_layout(Rect::ZERO).len(), 24);
}

/// The MIDI source drop-down is a control like any other (§7.5).
///
/// It is drawn, hit-tested and opened by the same code as every chooser on
/// the console; the one place it differs is the write, which
/// `press_tune_editor` sends to `set_insert_notes` because the source is a
/// routing edge rather than a value in the config. So what this measures is
/// that the shared half really is shared: a cell in the layout, and a hit
/// test that finds it.
#[test]
fn the_midi_source_is_a_chooser_the_layout_can_reach() {
    let view = a_view();
    let layout = tune_layout(body(OPENS_AT), &view);
    let card = view
        .cards
        .iter()
        .position(|card| card.group.name == "MIDI")
        .expect("a MIDI card");
    let source = &view.cards[card].group.params[0];
    assert_eq!(source.address.as_str(), fontelle_ui::canvas::TUNE_SOURCE);
    assert!(
        matches!(source.kind, ParamKind::Choice(_)),
        "the source is a drop-down, not a knob"
    );

    let cell = layout.cards[card]
        .cells
        .iter()
        .find(|(index, _)| *index == 0)
        .map(|(_, cell)| *cell)
        .expect("the source has a cell");
    assert!(!cell.is_empty(), "and the cell is on screen");
    assert_eq!(
        tune_hit(
            &layout,
            &view,
            cell.x + cell.width / 2.0,
            cell.y + cell.height / 2.0
        ),
        Some(TuneHit::Control { card, param: 0 }),
        "clicking it reaches the source and nothing else"
    );
}
