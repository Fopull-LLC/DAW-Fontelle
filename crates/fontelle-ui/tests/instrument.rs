//! The instrument editor — the panel that opens Fontelle's own soundfont
//! player up (TDD §7.2's "the SF2 seeds it once, after that the user owns
//! every parameter", and §8.2's addressing).
//!
//! Reported from using the window: *"the channels are supposed to be VST
//! instruments and the soundfont player in our software is supposed to be a
//! custom VST we make, and I don't see any options for our VST right now —
//! why can't I open up the VST options?"*
//!
//! They could not be opened because there was no panel to open. Every one of
//! them was already in `fontelle_core::Patch` and reachable only by importing
//! an SF2 and taking whatever it happened to say.
//!
//! This file is the *view*: where the controls go, which one was clicked, and
//! what a drag on one asks for. What each control means to a patch is
//! `fontelle-app`'s, and is tested there.

use fontelle_types::ParamAddress;
use fontelle_ui::canvas::{
    InstrumentGroup, InstrumentParam, InstrumentView, ParamKind, instrument_hit, instrument_layout,
    knob_value,
};
use fontelle_ui::layout::Rect;
use fontelle_ui::theme::{Metrics, Theme};

fn metrics() -> Metrics {
    Theme::dark_default().metrics
}

fn knob(name: &str, value: f32) -> InstrumentParam {
    InstrumentParam {
        address: ParamAddress::new(format!("patch/{name}")),
        label: name.to_string(),
        value,
        display: format!("{value:.2}"),
        kind: ParamKind::Knob,
    }
}

fn view() -> InstrumentView {
    InstrumentView {
        title: "tri baja".to_string(),
        groups: vec![
            InstrumentGroup {
                name: "Voice".to_string(),
                params: vec![
                    knob("polyphony", 0.25),
                    InstrumentParam {
                        address: ParamAddress::new("patch/voice/legato"),
                        label: "legato".to_string(),
                        value: 0.0,
                        display: "off".to_string(),
                        kind: ParamKind::Switch,
                    },
                    InstrumentParam {
                        address: ParamAddress::new("patch/voice/quality"),
                        label: "quality".to_string(),
                        value: 1.0 / 3.0,
                        display: "Normal".to_string(),
                        kind: ParamKind::Choice(
                            ["Draft", "Normal", "High", "Ultra"]
                                .map(str::to_string)
                                .to_vec(),
                        ),
                    },
                ],
            },
            InstrumentGroup {
                name: "Amp envelope".to_string(),
                params: vec![
                    knob("attack", 0.0),
                    knob("decay", 0.3),
                    knob("sustain", 1.0),
                    knob("release", 0.2),
                ],
            },
        ],
    }
}

fn body() -> Rect {
    Rect::new(20.0, 40.0, 700.0, 400.0)
}

// ------------------------------------------------------------- geometry ---

#[test]
fn every_parameter_gets_a_cell_and_every_group_a_heading() {
    let v = view();
    let l = instrument_layout(body(), &metrics(), &v);

    assert_eq!(l.headings.len(), 2, "one heading per group");
    let cells = v.groups.iter().map(|g| g.params.len()).sum::<usize>();
    assert_eq!(l.cells.len(), cells, "one cell per parameter");

    for (_, _, rect) in &l.cells {
        assert!(!rect.is_empty(), "an empty cell is an unclickable control");
        assert!(
            body().intersects(rect),
            "a cell at {rect:?} is outside the panel {:?}",
            body()
        );
    }
    // Cells never overlap: two knobs sharing pixels is two knobs you cannot aim
    // at.
    for (i, (_, _, a)) in l.cells.iter().enumerate() {
        for (_, _, b) in l.cells.iter().skip(i + 1) {
            assert!(!a.intersects(b), "cells {a:?} and {b:?} overlap");
        }
    }
    // And the second group starts below the first group's controls.
    let first_group_bottom = l
        .cells
        .iter()
        .filter(|(g, _, _)| *g == 0)
        .map(|(_, _, r)| r.bottom())
        .fold(f32::MIN, f32::max);
    assert!(l.headings[1].1.y >= first_group_bottom - 0.001);
}

#[test]
fn a_panel_too_small_for_the_controls_produces_no_negative_rectangles() {
    let v = view();
    for (w, h) in [(0.0, 0.0), (10.0, 10.0), (60.0, 400.0), (700.0, 12.0)] {
        let l = instrument_layout(Rect::new(0.0, 0.0, w, h), &metrics(), &v);
        for (_, rect) in &l.headings {
            assert!(rect.width >= 0.0 && rect.height >= 0.0, "{w}x{h}");
        }
        for (_, _, rect) in &l.cells {
            assert!(rect.width >= 0.0 && rect.height >= 0.0, "{w}x{h}");
        }
    }
}

#[test]
fn a_narrow_panel_stacks_the_controls_instead_of_running_off_the_edge() {
    let v = view();
    let wide = instrument_layout(Rect::new(0.0, 0.0, 800.0, 600.0), &metrics(), &v);
    let narrow = instrument_layout(Rect::new(0.0, 0.0, 200.0, 600.0), &metrics(), &v);

    let rows = |l: &fontelle_ui::canvas::InstrumentLayout| {
        let mut ys: Vec<i32> = l.cells.iter().map(|(_, _, r)| r.y as i32).collect();
        ys.sort_unstable();
        ys.dedup();
        ys.len()
    };
    assert!(
        rows(&narrow) > rows(&wide),
        "a narrow panel used the same number of rows as a wide one"
    );
    for (_, _, rect) in &narrow.cells {
        assert!(rect.right() <= 200.0 + 0.001, "{rect:?} ran off the panel");
    }
}

// ---------------------------------------------------------- hit-testing ---

#[test]
fn clicking_a_cell_says_which_parameter_it_is() {
    let v = view();
    let l = instrument_layout(body(), &metrics(), &v);

    for (group, param, rect) in &l.cells {
        let hit = instrument_hit(&l, rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
        assert_eq!(hit, Some((*group, *param)), "at {rect:?}");
    }
    assert_eq!(instrument_hit(&l, -50.0, -50.0), None);
}

// ------------------------------------------------------------- the knob ---

#[test]
fn dragging_a_knob_upwards_turns_it_up() {
    // Up is more, down is less, and the whole range is a sensible throw rather
    // than the height of the window.
    let up = knob_value(0.5, -60.0, false);
    let down = knob_value(0.5, 60.0, false);
    assert!(up > 0.5, "dragging up gave {up}");
    assert!(down < 0.5, "dragging down gave {down}");
    assert!((up - 0.5) > 0.1, "the throw is too long to use: {up}");

    // Clamped at both ends, whatever the drag.
    assert_eq!(knob_value(0.5, -100_000.0, false), 1.0);
    assert_eq!(knob_value(0.5, 100_000.0, false), 0.0);

    // Shift is a fine drag: the same pixels move it less.
    let fine = knob_value(0.5, -60.0, true);
    assert!(
        fine > 0.5 && fine < up,
        "fine drag {fine} against coarse {up}"
    );
}

#[test]
fn a_switch_and_a_choice_step_rather_than_slide() {
    use fontelle_ui::canvas::next_value;

    // A switch flips.
    assert_eq!(next_value(&ParamKind::Switch, 0.0), 1.0);
    assert_eq!(next_value(&ParamKind::Switch, 1.0), 0.0);

    // A choice walks its list and wraps, landing exactly on each option.
    let kind = ParamKind::Choice(["a", "b", "c", "d"].map(str::to_string).to_vec());
    let mut value = 0.0;
    let mut seen = vec![value];
    for _ in 0..4 {
        value = next_value(&kind, value);
        seen.push(value);
    }
    assert_eq!(
        seen.first(),
        seen.last(),
        "four steps through four options did not come back round: {seen:?}"
    );
    // Every step is a distinct option.
    let options: Vec<usize> = seen
        .iter()
        .map(|v| fontelle_ui::canvas::choice_index(&kind, *v))
        .collect();
    assert_eq!(options, vec![0, 1, 2, 3, 0], "got {options:?}");

    // A knob does not step.
    assert_eq!(next_value(&ParamKind::Knob, 0.4), 0.4);
}

#[test]
fn a_choice_with_no_options_is_not_a_division_by_zero() {
    use fontelle_ui::canvas::{choice_index, next_value};
    let empty = ParamKind::Choice(Vec::new());
    assert_eq!(next_value(&empty, 0.5), 0.5);
    assert_eq!(choice_index(&empty, 0.5), 0);
}
