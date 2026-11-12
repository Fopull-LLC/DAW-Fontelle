//! The faults that make a preset sound **unfinished** rather than wrong.
//!
//! > *"please also do a full audit on all of the flopsynth presets as weve
//! > improved it a lot since then but not really gone back and made sure our
//! > plugins are up to par. the synth is capable of a lot and making really
//! > polished sounds but many still sound unpolished or janky"* — Ty,
//! > 2026-11-12
//!
//! `tests/flopsynth_presets.rs` holds that every preset sounds, fits, sits
//! at the bank's level and is apart from its neighbours. None of that hears
//! a click when a gate slams shut, a release that drops two decibels in its
//! first sample, half a volt of DC under a pulse, a C6 folding back into
//! the audio band, a "pluck" that swells, or a trumpet that never wavers.
//! These are those readings, held for the whole bank; `examples/
//! preset_polish.rs` prints every one of them per preset, which is how
//! the bank was audited and how a new row should be checked.
//!
//! What was measured on 2026-11-12, before the fixes, is in each test's
//! comment. Nobody listened: these are the faults a measurement can see.

// The example reads with it too, and uses what this file does not.
#[allow(dead_code)]
#[path = "support/polish.rs"]
mod polish;

use fontelle_core::flopsynth::presets::{FACTORY, FactoryPreset, FlopsynthCategory};
use fontelle_core::{ModDest, ModSource};
use polish::bank;

fn row(name: &str) -> &'static FactoryPreset {
    FACTORY.iter().find(|r| r.name == name).unwrap()
}

/// **No preset clicks once it has started**: not when a gate shuts, not
/// when a stepped LFO jumps, not at the release, not when a voice dies.
///
/// Measured before the fixes: every row whose amp envelope was on the
/// `Decibel` curve still carried the Init patch's release bend of −0.6,
/// which on that curve is a drop of nearly two decibels **in the first
/// sample** of the release (808 Freeze: the envelope went 1.000 → 0.808 in
/// one step) — the sampled grand's whole family clicked when let go.
/// Square and sample-and-hold LFOs gating the amp with no smoothing were
/// the rest (Random Bleeps, Clock Divide; Pulse Train and a dozen more sat
/// just under the line and were rounded with them).
#[test]
fn no_preset_clicks_once_it_has_started() {
    // Where a burst *is* the sound, each with what it is.
    const MEANS_IT: &[(&str, &str)] = &[("Crackle Snap", "a crackle is clicks")];
    let wrong: Vec<String> = bank()
        .iter()
        .filter(|r| !MEANS_IT.iter().any(|(name, _)| *name == r.name))
        .filter(|r| r.click.0 > 10.0 && r.click.1 > -40.0)
        .map(|r| {
            format!(
                "  {}: a burst {:.1} dB over its surroundings, {:.1} dB under the note, at {:.3} s",
                r.name, r.click.0, r.click.1, r.click.2
            )
        })
        .collect();
    assert!(wrong.is_empty(), "these click:\n{}", wrong.join("\n"));
}

/// A preset that asks for a soft start — an amp attack of 5 ms or more —
/// gets one: nothing in its first 5 ms stands out of what follows.
///
/// The one gate here the bank already passed on 2026-11-12: it holds the
/// line for the next row, since a soft attack on a table that does not
/// start at a zero crossing is the easiest click to write.
#[test]
fn a_soft_attack_starts_without_a_click() {
    let wrong: Vec<String> = bank()
        .iter()
        .filter(|r| (row(r.name).build)().envelopes[0].attack_s >= 0.005)
        .filter(|r| r.onset.0 > 10.0 && r.onset.1 > -40.0)
        .map(|r| {
            format!(
                "  {}: {:.1} dB over what follows, {:.1} dB under the note",
                r.name, r.onset.0, r.onset.1
            )
        })
        .collect();
    assert!(
        wrong.is_empty(),
        "these start with a click:\n{}",
        wrong.join("\n")
    );
}

/// **No preset carries DC or rumble under 30 Hz** — DC no closer than
/// 30 dB to the note's RMS, and no more than a hundredth of its energy
/// under 30 Hz. Nobody hears either,
/// and both cost: headroom the limiter spends on nothing, a thump wherever
/// the envelope moves fast, and an offset every distortion downstream
/// clips lopsided.
///
/// Measured before the fixes: a 25 % pulse is half full scale of DC — the
/// chip shelf's pulses were 1 to 7 dB of DC against their own RMS (Chip
/// Pad −1.0 dB, Bright Pulse −2.4), and warps that bend a cycle
/// lopsidedly (Flip Sync, Mirror Wash, PD Piano) were as bad, and the
/// brown-noise beds put a fifth of their energy under 30 Hz — 59 rows in
/// all. The kits are recordings and are left to be what they are.
#[test]
fn no_preset_carries_dc_or_subsonic_rumble() {
    let wrong: Vec<String> = bank()
        .iter()
        .filter(|r| {
            !matches!(
                row(r.name).category,
                FlopsynthCategory::KitsAndHits | FlopsynthCategory::SynthDrums
            )
        })
        .filter(|r| r.dc > -30.0 || r.sub > -20.0)
        .map(|r| {
            format!(
                "  {}: DC {:.1} dB of its RMS, {:.1} dB under 30 Hz",
                r.name, r.dc, r.sub
            )
        })
        .collect();
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// **A bright preset is oversampled where folding shows.** At C6, a patch
/// whose off-grid energy is within 45 dB of its tone *and* falls by more
/// than 10 dB at 4× is hearing its own aliasing — and the cure is one
/// field (`Patch::oversampling`), at a cost `docs/flopsynth-next.md` §6
/// puts at a few per cent of a core.
///
/// Measured before the fixes: 48 rows — the chip pulses, the FM keys and
/// bells, the sync leads — none of the bank asked for oversampling at all.
#[test]
fn bright_presets_are_oversampled_where_folding_shows() {
    let wrong: Vec<String> = bank()
        .iter()
        .filter(|r| r.alias.0 > -45.0 && r.alias.1 > 10.0)
        .map(|r| {
            format!(
                "  {}: off-grid {:.1} dB at C6, {:.1} dB of it gone at 4×",
                r.name, r.alias.0, r.alias.1
            )
        })
        .collect();
    assert!(wrong.is_empty(), "these fold:\n{}", wrong.join("\n"));
}

/// **A plucked or struck sound hits at once.** A preset whose amp envelope
/// has no sustain and starts within 5 ms is a pluck, a strike or a hit,
/// and its loudest moment is its first tenth of a second — not later.
///
/// Measured before the fixes: Steel peaked 0.26 s in and Mandolin 0.21 s.
/// Both ran their tone through a band-pass the pluck archetype's filter
/// envelope swept *down* into: the strike opened the band far above the
/// note, and what came out was a swell into the decay — and a `.out(32)`
/// trying to make up for it. Touch Pluck (0.19 s) and Kora (0.13 s) were
/// two equal near-sines a few cents apart, beating the strike down.
#[test]
fn plucked_and_struck_presets_hit_at_once() {
    let wrong: Vec<String> = bank()
        .iter()
        .filter(|r| {
            let amp = (row(r.name).build)().envelopes[0];
            amp.sustain_level <= 0.0 && amp.attack_s <= 0.005
        })
        .filter(|r| r.peak_at > 0.12)
        .map(|r| format!("  {}: loudest at {:.2} s", r.name, r.peak_at))
        .collect();
    assert!(wrong.is_empty(), "these swell:\n{}", wrong.join("\n"));
}

/// §7.3's own rule, held: **every sustained acoustic imitation has a
/// vibrato that arrives late, and the mod wheel deepens it.** A held brass
/// or wind note that never wavers is the tell of a synth, and the wheel is
/// the first thing a player reaches for on one.
///
/// Measured before the fixes: Brass Section, French Horn, Trombone, Tuba
/// and Muted Trumpet had no vibrato at all; the woodwinds had a late one
/// the wheel could not reach.
#[test]
fn acoustic_imitations_have_a_late_vibrato_the_wheel_deepens() {
    // Imitations that do not sing, each with why.
    const MEANS_IT: &[(&str, &str)] = &[
        ("Robot Voice", "a robot is the voice without its vibrato"),
        ("Whisper", "a whisper has no pitch to waver"),
    ];
    let mut wrong = Vec::new();
    for row in FACTORY {
        if !matches!(
            row.category,
            FlopsynthCategory::Strings
                | FlopsynthCategory::BrassAndWinds
                | FlopsynthCategory::ChoirAndVocal
        ) || MEANS_IT.iter().any(|(name, _)| *name == row.name)
        {
            continue;
        }
        let patch = (row.build)();
        if patch.envelopes[0].sustain_level <= 0.0 {
            continue;
        }
        let routes = &patch.mod_matrix.routes;
        let late = routes
            .iter()
            .any(|route| match (route.source, route.destination) {
                (ModSource::Lfo(i), ModDest::LayerPitch(_)) => patch
                    .lfos
                    .get(usize::from(i))
                    .is_some_and(|lfo| lfo.delay_s > 0.0 || lfo.fade_s > 0.0),
                _ => false,
            });
        let wheel = routes.iter().any(|route| {
            let reads_wheel =
                route.source == ModSource::ModWheel || route.via == Some(ModSource::ModWheel);
            reads_wheel
                && matches!(
                    route.destination,
                    ModDest::LayerPitch(_) | ModDest::LfoDepth(_)
                )
        });
        if !late || !wheel {
            wrong.push(format!(
                "  {}: {}{}",
                row.name,
                if late { "" } else { "no late vibrato " },
                if wheel { "" } else { "nothing on the wheel" }
            ));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// **A knob that turns an LFO up has room to turn it.** An LFO's depth is
/// clamped to 0..1, and every one starts at 1 unless the row says
/// otherwise — so a macro or the wheel routed to *add* depth to an LFO
/// left at 1 moves nothing at all.
///
/// Measured before the fixes: the `strings()` archetype's Vibrato macro,
/// and the woodwinds' (Flute, Oboe, Tenor Sax, Piccolo, Shakuhachi), were
/// dead knobs for this reason — named, routed, and inert.
#[test]
fn a_route_that_deepens_an_lfo_has_room_to() {
    let mut wrong = Vec::new();
    for row in FACTORY {
        let patch = (row.build)();
        for route in &patch.mod_matrix.routes {
            let ModDest::LfoDepth(i) = route.destination else {
                continue;
            };
            // The sources a player turns, which rest at nothing and rise.
            let turned = matches!(
                route.source,
                ModSource::Macro(_) | ModSource::ModWheel | ModSource::Aftertouch
            );
            if !turned || route.invert || route.depth <= 0.0 {
                continue;
            }
            let Some(lfo) = patch.lfos.get(usize::from(i)) else {
                continue;
            };
            let room = 1.0 - lfo.depth;
            if room < route.depth.min(1.0) * 0.5 {
                wrong.push(format!(
                    "  {}: {:?} adds {:.2} to LFO {} already at {:.2}",
                    row.name, route.source, route.depth, i, lfo.depth
                ));
            }
        }
    }
    assert!(
        wrong.is_empty(),
        "these knobs cannot move:\n{}",
        wrong.join("\n")
    );
}
