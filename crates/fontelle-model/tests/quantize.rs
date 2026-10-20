//! Quantize: notes pulled onto a grid, by as much as the strength says.
//!
//! Ty, 2026-10-20: *"add quantize tool and make ctrl q shortcut."* FL's
//! Ctrl+Q is a quick quantize of the starts; its Quantize dialog adds a
//! strength and the ends. The arithmetic is here, pure; the roll's dialog
//! and Ctrl+Q both hand their notes to it.

use fontelle_model::{QuantizeSpec, quantized};
use fontelle_types::PPQN;

const SIXTEENTH: i64 = PPQN / 4;

fn spec(grid: i64) -> QuantizeSpec {
    QuantizeSpec {
        grid,
        strength: 1.0,
        ends: false,
    }
}

#[test]
fn a_start_goes_to_the_nearest_line_and_the_length_rides_along() {
    let out = quantized(
        &[(SIXTEENTH + 10, 100), (2 * SIXTEENTH - 7, 50)],
        spec(SIXTEENTH),
    );
    assert_eq!(out, vec![(SIXTEENTH, 100), (2 * SIXTEENTH, 50)]);
}

#[test]
fn halfway_rounds_forward_and_nothing_goes_before_the_clip() {
    let out = quantized(&[(SIXTEENTH / 2, 30), (3, 30)], spec(SIXTEENTH));
    assert_eq!(out, vec![(SIXTEENTH, 30), (0, 30)]);
}

#[test]
fn half_strength_goes_half_way() {
    let mut half = spec(SIXTEENTH);
    half.strength = 0.5;
    let out = quantized(&[(SIXTEENTH + 20, 100)], half);
    assert_eq!(out, vec![(SIXTEENTH + 10, 100)]);
    let mut none = spec(SIXTEENTH);
    none.strength = 0.0;
    assert_eq!(
        quantized(&[(SIXTEENTH + 20, 100)], none),
        vec![(SIXTEENTH + 20, 100)]
    );
}

/// With the ends, a note's end goes to its own nearest line too — and never
/// onto its start: a note quantized to nothing is a note deleted.
#[test]
fn the_ends_go_to_a_line_and_a_note_keeps_at_least_one_step() {
    let mut ends = spec(SIXTEENTH);
    ends.ends = true;
    let out = quantized(
        &[(SIXTEENTH + 5, SIXTEENTH * 2 - 20), (4 * SIXTEENTH, 10)],
        ends,
    );
    assert_eq!(
        out,
        vec![(SIXTEENTH, SIXTEENTH * 2), (4 * SIXTEENTH, SIXTEENTH)]
    );
}

#[test]
fn notes_already_on_the_grid_are_left_as_they_are() {
    let notes = [(0, 120), (SIXTEENTH * 3, 7), (PPQN * 4, PPQN)];
    assert_eq!(quantized(&notes, spec(SIXTEENTH)), notes.to_vec());
}

#[test]
fn no_grid_changes_nothing() {
    let notes = [(13, 120), (77, 7)];
    assert_eq!(quantized(&notes, spec(0)), notes.to_vec());
}
