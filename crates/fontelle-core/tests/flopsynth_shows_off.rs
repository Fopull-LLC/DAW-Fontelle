//! **The bank has to show off what the synth can do.**
//!
//! > *"right now it's very general but i want more presets that utilize its
//! > advanced synth capabilities to make some really cool unique electronic
//! > sounds ... so it has much more cool stuff to show off just immediately
//! > out of the box."*
//!
//! Measured before anything was added, and the report was exact: 211 presets
//! used maybe three fifths of the engine. Never used **at all** were
//! `WarpMode::Quantise`, `ModSource::Random`, `NoteOnCounter`, `Aftertouch`,
//! `NoteModX`, `NoteModY`, `PitchBend`, `ModDest::OscUnisonBlend`,
//! `FilterDrive`, `LfoPhase`, `UnisonDetune`, `EnvelopeStageLevel`, and three
//! of the forty wavetables — and a dozen more were used exactly once.
//!
//! A capability nobody has heard is a capability nobody knows is there. This
//! is the claim that a person opening the browser can *find* the synth's range
//! without building a patch themselves, and it is written as a test because a
//! bank drifts: the cheapest preset to write is another saw pad, and thirty of
//! those would pass every other gate in the suite.

use fontelle_core::flopsynth::presets::FACTORY;
use fontelle_core::{Patch, Source};
use fontelle_dsp::{WarpMode, WavetableId};

/// Every patch in the bank, built once.
fn bank() -> Vec<(&'static str, Patch)> {
    FACTORY
        .iter()
        .map(|row| (row.name, (row.build)()))
        .collect()
}

fn oscs(patch: &Patch) -> impl Iterator<Item = &fontelle_dsp::SynthOsc> {
    patch.layers.iter().filter_map(|layer| match &layer.source {
        Source::Synth(osc) => Some(osc),
        _ => None,
    })
}

#[test]
fn every_wavetable_is_played_by_something() {
    let bank = bank();
    let unused: Vec<&str> = WavetableId::ALL
        .iter()
        .filter(|table| {
            !bank.iter().any(|(_, patch)| {
                oscs(patch).any(
                    |osc| matches!(osc.source, fontelle_dsp::SynthSource::Table(t) if t == **table),
                )
            })
        })
        .map(|t| t.label())
        .collect();
    assert!(
        unused.is_empty(),
        "forty tables ship and these are in no preset at all: {unused:?}"
    );
}

#[test]
fn every_warp_mode_is_shown_off() {
    // Seven modes, and `Quantise` — the one warp that is a *reduction*, and
    // the whole of the synth's digital grit — was in nothing.
    let bank = bank();
    let unused: Vec<&str> = WarpMode::ALL
        .iter()
        .filter(|mode| **mode != WarpMode::Off)
        .filter(|mode| {
            !bank.iter().any(|(_, patch)| {
                oscs(patch).any(|osc| osc.warp == **mode && osc.warp_amount > 0.0)
            })
        })
        .map(|m| m.label())
        .collect();
    assert!(unused.is_empty(), "warp modes no preset uses: {unused:?}");
}

#[test]
fn the_expressive_sources_are_wired_to_something() {
    // A synth whose presets ignore the wheel, aftertouch and the roll's own
    // per-note properties is one that plays the same however it is played.
    use fontelle_core::ModSource::*;
    let bank = bank();
    let wanted = [
        ("Random", Random),
        ("NoteOnCounter", NoteOnCounter),
        ("Aftertouch", Aftertouch),
        ("ModWheel", ModWheel),
        ("PitchBend", PitchBend),
        ("NoteModX", NoteModX),
        ("NoteModY", NoteModY),
    ];
    let unused: Vec<&str> = wanted
        .iter()
        .filter(|(_, source)| {
            !bank.iter().any(|(_, patch)| {
                patch
                    .mod_matrix
                    .routes
                    .iter()
                    .any(|r| r.source == *source || r.via == Some(*source))
            })
        })
        .map(|(name, _)| *name)
        .collect();
    assert!(unused.is_empty(), "sources no preset reads: {unused:?}");
}

#[test]
fn the_advanced_destinations_are_driven_by_something() {
    use fontelle_core::ModDest::*;
    let bank = bank();
    // Named individually rather than over a `ALL`, because these are the ones
    // that make a preset *move* — the difference between a patch and a sound.
    type Wanted = (&'static str, fn(&fontelle_core::ModDest) -> bool);
    let wanted: [Wanted; 6] = [
        ("OscUnisonBlend", |d| matches!(d, OscUnisonBlend(_))),
        ("OscUnisonDetune", |d| matches!(d, OscUnisonDetune(_))),
        ("FilterDrive", |d| matches!(d, FilterDrive(_))),
        ("LfoPhase", |d| matches!(d, LfoPhase(_))),
        ("UnisonDetune", |d| matches!(d, UnisonDetune)),
        ("EnvelopeStageLevel", |d| {
            matches!(d, EnvelopeStageLevel(_, _))
        }),
    ];
    let unused: Vec<&str> = wanted
        .iter()
        .filter(|(_, is)| {
            !bank
                .iter()
                .any(|(_, patch)| patch.mod_matrix.routes.iter().any(|r| is(&r.destination)))
        })
        .map(|(name, _)| *name)
        .collect();
    assert!(unused.is_empty(), "destinations nothing drives: {unused:?}");
}

/// The point of the whole exercise: enough of it, and enough of it electronic.
#[test]
fn the_bank_is_big_enough_to_browse() {
    assert!(
        FACTORY.len() >= 260,
        "the bank is {} presets; the expansion was meant to take it past 260",
        FACTORY.len()
    );
}
