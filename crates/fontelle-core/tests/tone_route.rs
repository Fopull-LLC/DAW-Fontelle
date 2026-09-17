//! No preset runs its **tone** through a filter set up for its **breath**.
//!
//! > *"pan flute sounds very noisy right now it just sounds like noise and
//! > air and a faint wave in the background."* — Ty, 2026-09-17
//!
//! The Init patch's oscillators are on the *serial* route, which goes
//! through Filter 1 and then Filter 2. A row that enables Filter 2 as a
//! band-pass or high-pass for its noise layer and leaves its oscillators on
//! that route has put the tone through the breath's filter as well: the Pan
//! Pipe's sine came out of an 1.8 kHz band-pass at resonance 1.2 with its
//! fundamental twenty-three decibels down at C3, and the whole `choir()`
//! family was thirty down through the breath's 3 kHz band. The organ shelf
//! had the same fault on 2026-09-13 (`tests/organ_click.rs`). This is the
//! rule those three fixes share, held for the whole bank:
//!
//! **An audible oscillator on the serial route while Filter 2 is a band-pass
//! or a high-pass at 200 Hz or over is a mistake** — unless the row says
//! otherwise, which the list below is: the rows where Filter 2 *is* the
//! instrument (a mute, a ukulele's body, a tape's band). A high-pass under
//! 200 Hz sits below every fundamental from C3 up and is a floor, not a
//! breath's filter: the Harp's and the Electric Grand's are that.
//!
//! `examples/preset_audit.rs` is the reading this came from; it also prints
//! the level across the keyboard, which is how the cliffs in `Oboe` and
//! `Dusty Rhodes` were found.

use fontelle_core::flopsynth::presets::FACTORY;
use fontelle_core::{SILENT_DB, Source};
use fontelle_dsp::{FilterRoute, SvfMode};

/// Rows whose Filter 2 is a band-pass or high-pass **on the tone**, on
/// purpose. Each names what the filter is.
const MEANS_IT: &[(&str, &str)] = &[
    (
        "Muted Trumpet",
        "the harmon mute is a resonant band on the horn",
    ),
    (
        "Ukulele",
        "a ukulele has no bass; the high-pass is its body",
    ),
    ("Pianotron", "a tape's band, on the tape"),
];

/// A band-pass, or a high-pass that reaches the fundamentals.
fn breath_filter(mode: SvfMode, cutoff_hz: f32) -> bool {
    match mode {
        SvfMode::Bandpass => true,
        SvfMode::Highpass => cutoff_hz >= 200.0,
        _ => false,
    }
}

#[test]
fn no_tone_goes_through_the_breaths_filter() {
    let mut wrong = Vec::new();
    for row in FACTORY {
        if MEANS_IT.iter().any(|(name, _)| *name == row.name) {
            continue;
        }
        let patch = (row.build)();
        let f2 = &patch.filters[1];
        if !f2.enabled || !breath_filter(f2.mode, f2.cutoff_hz) {
            continue;
        }
        let on_serial: Vec<&str> = patch
            .layers
            .iter()
            .enumerate()
            .take(4)
            .filter_map(|(index, layer)| match &layer.source {
                Source::Synth(osc)
                    if layer.gain_db > SILENT_DB && osc.filter_route == FilterRoute::Serial =>
                {
                    Some(["A", "B", "C", "SUB"][index])
                }
                _ => None,
            })
            .collect();
        if !on_serial.is_empty() {
            wrong.push(format!(
                "  {}: {} through Filter 2's {:?} at {:.0} Hz",
                row.name,
                on_serial.join(", "),
                f2.mode,
                f2.cutoff_hz
            ));
        }
    }
    assert!(
        wrong.is_empty(),
        "presets whose tone goes through the filter meant for their breath \
         (route the oscillator to F1, or add the row to MEANS_IT and say why):\n{}",
        wrong.join("\n")
    );
}

/// The allow-list is not a place to park a fault: every row on it still
/// has its Filter 2 as a band-pass or high-pass, and still sends a tone
/// through it. A row that stops doing either comes off the list.
#[test]
fn every_row_that_means_it_still_does() {
    for (name, why) in MEANS_IT {
        let row = FACTORY
            .iter()
            .find(|r| r.name == *name)
            .unwrap_or_else(|| panic!("{name} ({why}) is not in the bank"));
        let patch = (row.build)();
        let f2 = &patch.filters[1];
        assert!(
            f2.enabled && breath_filter(f2.mode, f2.cutoff_hz),
            "{name} no longer has a band-pass or high-pass on Filter 2"
        );
        assert!(
            patch.layers.iter().take(4).any(|layer| matches!(
                &layer.source,
                Source::Synth(osc)
                    if layer.gain_db > SILENT_DB && osc.filter_route == FilterRoute::Serial
            )),
            "{name} no longer sends a tone through it"
        );
    }
}
