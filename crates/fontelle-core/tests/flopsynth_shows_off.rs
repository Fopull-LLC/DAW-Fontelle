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
use fontelle_dsp::{SampleLoop, SynthSource, WarpMode, WavetableId};

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

// ---------------------------------------------------------- the sampling ---
//
// > *"use flopsynths new sampling features to make a variety of new complex
// > presets that can be experimental, synthy, modulating, instruments,
// > percussion kits, growls, dubstep sounds"* — Ty, 2026-09-16
//
// The sample source can read a recording five ways, lock to one zone of it,
// and be another oscillator's FM or RM modulator; the bank ships four sets of
// recordings. Each of those is a thing a person opening the synthesiser
// should be able to *find* — so each must be in some preset.

/// A sample oscillator that is heard: on, or somebody's modulator.
fn sampled<'a>(patch: &'a Patch) -> impl Iterator<Item = &'a fontelle_dsp::SynthOsc> + 'a {
    let modulators: Vec<u8> = oscs(patch).filter_map(|osc| osc.modulator).collect();
    patch
        .layers
        .iter()
        .enumerate()
        .filter_map(move |(index, layer)| match &layer.source {
            Source::Synth(osc)
                if matches!(osc.source, SynthSource::Sample(_))
                    && (layer.gain_db > fontelle_core::SILENT_DB
                        || modulators.contains(&(index as u8))) =>
            {
                Some(osc)
            }
            _ => None,
        })
}

#[test]
fn every_way_of_reading_a_recording_is_shown_off() {
    let bank = bank();
    let unused: Vec<&str> = SampleLoop::ALL
        .iter()
        .filter(|mode| {
            !bank
                .iter()
                .any(|(_, patch)| sampled(patch).any(|osc| osc.sample.loop_mode == **mode))
        })
        .map(|m| m.label())
        .collect();
    assert!(
        unused.is_empty(),
        "ways of reading a recording no preset uses: {unused:?}"
    );
}

#[test]
fn every_factory_recording_is_played_by_something() {
    use fontelle_core::factory_samples::FactorySampleSet;
    let bank = bank();
    let unused: Vec<&str> = FactorySampleSet::ALL
        .iter()
        .filter(|set| {
            !bank.iter().any(|(_, patch)| {
                patch
                    .samples
                    .iter()
                    .position(|sample| sample.factory == Some(**set))
                    .is_some_and(|at| {
                        sampled(patch).any(|osc| osc.source == SynthSource::Sample(at as u8))
                    })
            })
        })
        .map(|s| s.label())
        .collect();
    assert!(
        unused.is_empty(),
        "recordings the bank ships and no preset plays: {unused:?}"
    );
}

/// A recording as the thing that *modulates*: FM by a piano's decay is a
/// sound that changes over the note the way no LFO does, and RM by a drum
/// is a sub-harmonic nothing else here makes.
#[test]
fn a_recording_is_somebodys_modulator() {
    let bank = bank();
    let count = bank
        .iter()
        .filter(|(_, patch)| {
            oscs(patch).any(|osc| {
                osc.warp.needs_a_modulator()
                    && osc.warp_amount > 0.0
                    && osc.modulator.is_some_and(|from| {
                        matches!(
                            patch.layers.get(usize::from(from)).map(|l| &l.source),
                            Some(Source::Synth(m)) if matches!(m.source, SynthSource::Sample(_))
                        )
                    })
            })
        })
        .count();
    assert!(
        count >= 3,
        "presets whose FM or RM modulator is a recording: {count}, and the \
         bank should show that off more than once"
    );
}

/// A zone lock: one hit of a kit across the keyboard.
#[test]
fn a_kit_hit_is_an_instrument_somewhere() {
    let bank = bank();
    let count = bank
        .iter()
        .filter(|(_, patch)| sampled(patch).any(|osc| osc.sample.zone.is_some()))
        .count();
    assert!(
        count >= 3,
        "presets locked to one zone of a recording: {count}"
    );
}
