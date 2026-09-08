//! The factory bank (`docs/flopsynth-plan.md` §7).
//!
//! > *"should have lots of built in presets in a bank for tons of instruments
//! > organized by type. make sure to include a wide variety between synthesis
//! > and synths that sound like other instruments like choir ahhs or strings
//! > etc."* — Ty, 2026-09-06
//!
//! # Why these tests and not "they sound nice"
//!
//! The drum kits taught that a bank of "different" presets can be one preset
//! at forty brightnesses (`drum-kit-axes`). So: every preset sounds, every one
//! stays inside full scale, every one sits within a few dB of every other, and
//! **every pair inside a category is measurably apart on at least one axis** —
//! with the loudness match in place precisely so that "apart" cannot be bought
//! by being louder.
//!
//! Three numbers are necessary and not sufficient. Ty listening to a walk
//! through the bank is the other half, and `--play-flopsynth all` is what
//! makes that possible.

use std::collections::HashMap;

use fontelle_core::flopsynth::presets::{FACTORY, FlopsynthCategory};
use fontelle_core::{NoteTrigger, PrepareContext, SampleStore, Sampler, flopsynth};

const SR: f32 = 48_000.0;

/// One note through a preset, for `seconds`, as a mono sum.
fn render_note(patch: fontelle_core::Patch, key: u8, velocity: u8, seconds: f32) -> Vec<f32> {
    render_chord(patch, &[key], velocity, seconds, seconds)
}

/// `keys` all at once, held for `hold` seconds and then let go, rendered for
/// `seconds` in total.
fn render_chord(
    patch: fontelle_core::Patch,
    keys: &[u8],
    velocity: u8,
    hold: f32,
    seconds: f32,
) -> Vec<f32> {
    let store = SampleStore::new();
    let mut sampler = Sampler::new(patch);
    sampler.prepare(&PrepareContext {
        sample_rate: SR,
        max_block_size: 512,
    });
    for key in keys {
        sampler.trigger(NoteTrigger::new(*key, velocity));
    }
    let total = (SR * seconds) as usize;
    let release_at = (SR * hold) as usize;
    let block = 512usize;
    let mut out = Vec::with_capacity(total);
    let mut done = 0usize;
    let mut released = false;
    while done < total {
        if !released && done >= release_at {
            sampler.release_all();
            released = true;
        }
        let frames = block.min(total - done);
        let mut left = vec![0.0f32; frames];
        let mut right = vec![0.0f32; frames];
        {
            let (l, r) = (&mut left[..], &mut right[..]);
            sampler.render(&store, &mut [l, r]);
        }
        for (a, b) in left.iter().zip(&right) {
            out.push((a + b) * 0.5);
        }
        done += frames;
    }
    out
}

fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
}

fn peak(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0f32, |a, s| a.max(s.abs()))
}

fn db(x: f32) -> f32 {
    20.0 * x.max(1e-9).log10()
}

/// The loudest **50 ms** of a sound.
///
/// The window a short-term loudness meter uses, and the only one that reads a
/// hi-hat and a pad comparably: a quarter-second window on a sixty-millisecond
/// hat is three quarters silence, and reports it twelve decibels quieter than
/// it sounds — which would then have the loudness match turn it up until it
/// clipped. The mean over the whole note is worse still, for the same reason
/// one step further.
fn loudness(samples: &[f32]) -> f32 {
    let window = (SR * 0.05) as usize;
    samples.chunks(window).map(rms).fold(0.0f32, f32::max)
}

/// A Hann-windowed DFT at `hz`.
fn energy_at(samples: &[f32], hz: f32) -> f32 {
    let n = samples.len() as f32;
    let (mut re, mut im) = (0.0f32, 0.0f32);
    for (i, sample) in samples.iter().enumerate() {
        let window = 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / n).cos();
        let phase = std::f32::consts::TAU * hz * i as f32 / SR;
        re += sample * window * phase.cos();
        im -= sample * window * phase.sin();
    }
    (re * re + im * im).sqrt() / n
}

/// The size of the bank, as a floor rather than a count.
///
/// Raised from 120/6 when Ty asked for the roster to be expanded again: the
/// floor is what a *browser* needs to be worth browsing, and a category with
/// six rows in it is a shelf you read in one glance. Ten is the point at which
/// a category stops being a list and starts being somewhere to look.
#[test]
fn there_are_at_least_two_hundred_and_every_category_has_at_least_ten() {
    assert!(
        FACTORY.len() >= 200,
        "the gate is 200 presets; the bank has {}",
        FACTORY.len()
    );
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for row in FACTORY {
        *counts.entry(row.category.label()).or_default() += 1;
    }
    assert_eq!(
        counts.len(),
        FlopsynthCategory::ALL.len(),
        "every category has to have presets in it, or it is a heading over \
         nothing: {counts:?}"
    );
    for category in FlopsynthCategory::ALL {
        let count = counts.get(category.label()).copied().unwrap_or(0);
        assert!(count >= 10, "{} has only {count} presets", category.label());
    }
}

#[test]
fn every_name_is_unique_and_every_preset_builds_a_flopsynth_patch() {
    let mut names: Vec<&str> = FACTORY.iter().map(|row| row.name).collect();
    let total = names.len();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), total, "two presets share a name");

    for row in FACTORY {
        let patch = (row.build)();
        assert!(
            flopsynth::is_flopsynth(&patch),
            "{} is not a Flopsynth patch",
            row.name
        );
        assert!(
            patch.layers.len() >= 5,
            "{} has {} layers; the five roles are A, B, C, Sub, Noise",
            row.name,
            patch.layers.len()
        );
        for index in 0..5 {
            assert!(
                matches!(patch.layers[index].source, fontelle_core::Source::Synth(_)),
                "{}: layer {index} is not a synth oscillator",
                row.name
            );
        }
        assert!(!row.name.is_empty());
        // A name is a file's stem (§P.3), so it has to be one.
        assert!(
            !row.name.contains(['/', '\\', ':']),
            "{} is not a legal file name",
            row.name
        );
    }
}

#[test]
fn every_preset_sounds() {
    for row in FACTORY {
        // A whole second, and the loudest part of it: a riser sounds late, and
        // measuring only the attack would call it silent.
        let out = render_note((row.build)(), 60, 100, 1.5);
        let loudest = out.chunks(4_800).map(rms).fold(0.0f32, f32::max);
        assert!(
            db(loudest) > -40.0,
            "{} is silent: loudest tenth of a second is {:.1} dBFS",
            row.name,
            db(loudest)
        );
    }
}

/// Every preset stays inside full scale on a four-note chord at full velocity.
///
/// The patch's own layers and its trim, without its effects — the chain lives
/// in the node, so `fontelle-engine/tests/flopsynth_fx.rs` is where that half
/// is measured.
#[test]
fn every_preset_stays_inside_full_scale() {
    for row in FACTORY {
        let out = render_chord((row.build)(), &[60, 64, 67, 72], 127, 1.5, 2.5);
        assert!(
            peak(&out) <= 0.98,
            "{} peaked at {:.3} on a four-note chord at velocity 127",
            row.name,
            peak(&out)
        );
    }
}

/// **The loudness match.** This is what `Patch::output_db` is for, and it is
/// what stops the "audibly apart" test below being gamed by making a preset
/// louder rather than different.
#[test]
fn every_preset_sits_within_three_db_of_the_banks_median() {
    let mut measured: Vec<(&str, f32)> = FACTORY
        .iter()
        .map(|row| {
            let out = render_chord((row.build)(), &[60, 64, 67, 72], 100, 1.0, 1.2);
            (row.name, db(loudness(&out)))
        })
        .collect();

    let mut levels: Vec<f32> = measured.iter().map(|(_, db)| *db).collect();
    levels.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median = levels[levels.len() / 2];

    measured.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
    let outliers: Vec<String> = measured
        .iter()
        .filter(|(_, level)| (level - median).abs() > 3.0)
        .map(|(name, level)| format!("  {name}: {level:+.1} dB ({:+.1} off)", level - median))
        .collect();
    assert!(
        outliers.is_empty(),
        // The whole table, so the agent tuning `output_db` can do it in one
        // pass rather than one preset per run (§13's third risk).
        "the median is {median:.1} dBFS; these are more than 3 dB from it:\n{}",
        outliers.join("\n")
    );
}

/// The drum kit's test, one level up: **every pair inside a category has to be
/// apart on at least one of five axes.** Two presets that differ only in how
/// bright they are is the failure this exists to catch.
///
/// Note what this can and cannot see: it renders through `Sampler`, and a
/// patch's effects chain runs in `SamplerNode` (§2.2). So two presets that
/// differ *only* by their effects are identical here — which is the right
/// answer, because §7.3 says two presets on the same source and filter model
/// must differ on at least two things and an effect is only one of them.
#[test]
fn every_pair_in_a_category_is_audibly_apart() {
    /// The four axes: how long it rings, where its energy sits, how peaky it
    /// is, and what its attack does. All in log units so that "apart by 0.35"
    /// means the same thing on each.
    fn describe(samples: &[f32]) -> [f32; 4] {
        let peak_level = peak(samples).max(1e-9);
        // t30: how long after its **loudest moment** the envelope takes to
        // fall 30 dB.
        //
        // From the loudest moment and not from the start, because half this
        // bank has a slow attack: a choir's first ten milliseconds are already
        // 30 dB below its peak, so searching from zero reports every pad as
        // decaying instantly and throws the axis away.
        let envelope: Vec<f32> = samples.chunks(480).map(rms).collect();
        let loudest_at = envelope
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .map(|(i, _)| i)
            .unwrap_or(0);
        let floor = peak_level * 0.0316;
        let t30 = envelope[loudest_at..]
            .iter()
            .position(|level| *level < floor)
            .unwrap_or(envelope.len() - loudest_at) as f32
            * 0.01;

        let mut weighted = 0.0f32;
        let mut total = 0.0f32;
        let mut hz = 60.0f32;
        while hz < 14_000.0 {
            let e = energy_at(samples, hz);
            weighted += hz.ln() * e;
            total += e;
            hz *= 1.2;
        }
        let centroid = if total > 1e-9 { weighted / total } else { 0.0 };

        let level = rms(samples).max(1e-9);
        let crest = (peak_level / level).ln();

        // Spectral flux over the first 300 ms — the attack's character, which
        // is the axis that tells a pluck from a pad when the two happen to
        // have the same brightness.
        let attack = &samples[..samples.len().min((SR * 0.3) as usize)];
        let flux = attack
            .chunks(480)
            .map(rms)
            .collect::<Vec<_>>()
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .sum::<f32>()
            / level;

        [t30.max(1e-3).ln(), centroid, crest, (flux + 1.0).ln()]
    }

    /// Ten log-spaced bands, normalised to sum to one — a sound's **shape**
    /// rather than its mean.
    ///
    /// The fifth axis, and the one that does most of the work. A spectral
    /// centroid is a *mean*, and two vowels are unmistakably different to an
    /// ear while having almost the same one: /a/ and /u/ move energy between
    /// bands without moving where its middle sits. Measured, before this
    /// existed: "Choir Ahh" and "Choir Ooh" read as 0.26 apart on the four
    /// scalar axes and 1.4 apart on this one.
    ///
    /// The distance is the total variation between two profiles — nought for
    /// two identical spectra, two for two that share no band at all.
    fn profile(samples: &[f32]) -> [f32; 10] {
        let mut bands = [0.0f32; 10];
        for (index, band) in bands.iter_mut().enumerate() {
            let lo = 60.0 * (14_000.0f32 / 60.0).powf(index as f32 / 10.0);
            let hi = 60.0 * (14_000.0f32 / 60.0).powf((index + 1) as f32 / 10.0);
            let mut hz = lo;
            while hz < hi {
                *band += energy_at(samples, hz);
                hz *= 1.08;
            }
        }
        let total: f32 = bands.iter().sum::<f32>().max(1e-9);
        for band in &mut bands {
            *band /= total;
        }
        bands
    }

    const APART: f32 = 0.35;
    #[allow(clippy::type_complexity)]
    let mut by_category: HashMap<&str, Vec<(&str, ([f32; 4], [f32; 10]))>> = HashMap::new();
    for row in FACTORY {
        // Held for six tenths and then let go, because **how long it rings**
        // is one of the four axes and a note that is never released has no
        // decay to measure. Three of the four would then be doing all the
        // work, and a pluck and a pad would read as the same preset.
        let out = render_chord((row.build)(), &[60], 100, 0.6, 2.5);
        by_category
            .entry(row.category.label())
            .or_default()
            .push((row.name, (describe(&out), profile(&out))));
    }

    let mut same: Vec<String> = Vec::new();
    for (category, presets) in &by_category {
        for (i, (a_name, a)) in presets.iter().enumerate() {
            for (b_name, b) in &presets[i + 1..] {
                let scalar =
                    a.0.iter()
                        .zip(&b.0)
                        .map(|(x, y)| (x - y).abs())
                        .fold(0.0f32, f32::max);
                let shape: f32 = a.1.iter().zip(&b.1).map(|(x, y)| (x - y).abs()).sum();
                let apart = scalar.max(shape);
                if apart <= APART {
                    same.push(format!(
                        "  {category}: {a_name} and {b_name} are {apart:.2} apart \
                         (scalar {scalar:.2}, shape {shape:.2})\n    {:.2?}\n    {:.2?}",
                        a.0, b.0
                    ));
                }
            }
        }
    }
    assert!(
        same.is_empty(),
        "these pairs are the same preset with two names:\n{}",
        same.join("\n")
    );
}

/// The brief names this one by hand — *"synths that sound like other
/// instruments like choir ahhs"* — so the test is spectral rather than
/// aesthetic: the sustain's loudest peaks are where a vowel's formants are.
#[test]
fn choir_ahh_has_a_voice_in_it() {
    let row = FACTORY
        .iter()
        .find(|row| row.name == "Choir Ahh")
        .expect("the bank has a Choir Ahh");
    let out = render_note((row.build)(), 48, 100, 2.0);
    // The sustain, past the slow attack.
    let sustain = &out[(SR * 1.0) as usize..(SR * 1.8) as usize];

    let near = |hz: f32| {
        let mut best = 0.0f32;
        let mut probe = hz * 0.85;
        while probe < hz * 1.18 {
            best = best.max(energy_at(sustain, probe));
            probe *= 1.02;
        }
        best
    };
    // /a/: F1 730 Hz, F2 1090 Hz.
    let formants = near(730.0).min(near(1090.0));
    // Between and above them, where a vowel has a trough.
    let elsewhere = near(1_700.0).max(near(4_500.0));
    assert!(
        formants > elsewhere * 2.0,
        "Choir Ahh has to have a throat in it: {formants} at the formants \
         against {elsewhere} away from them"
    );
}

/// §7.3's two rules that every preset has to follow, because they are what
/// make a bank playable rather than a bank of sequencer patches.
#[test]
fn every_preset_names_two_macros_and_routes_velocity() {
    use fontelle_core::ModSource;
    let mut missing: Vec<String> = Vec::new();
    for row in FACTORY {
        let patch = (row.build)();
        let named = patch.macros.iter().filter(|m| !m.name.is_empty()).count();
        if named < 2 {
            missing.push(format!("  {}: only {named} named macros", row.name));
        }
        // A macro nothing reads is a knob that does nothing.
        for (index, macro_knob) in patch.macros.iter().enumerate() {
            if macro_knob.name.is_empty() {
                continue;
            }
            let read = patch.mod_matrix.routes.iter().any(|route| {
                route.source == ModSource::Macro(index as u8)
                    || route.via == Some(ModSource::Macro(index as u8))
            });
            if !read {
                missing.push(format!(
                    "  {}: macro \"{}\" is named and nothing reads it",
                    row.name, macro_knob.name
                ));
            }
        }
        let velocity = patch.mod_matrix.routes.iter().any(|route| {
            route.source == ModSource::Velocity || route.via == Some(ModSource::Velocity)
        });
        if !velocity {
            missing.push(format!(
                "  {}: velocity goes nowhere — this program's user plays",
                row.name
            ));
        }
    }
    assert!(missing.is_empty(), "{}", missing.join("\n"));
}

/// Every preset survives being saved and reopened, which is the only reason
/// any of the rest of this matters.
#[test]
fn every_preset_round_trips_the_format() {
    for row in FACTORY {
        let patch = (row.build)();
        let data = patch
            .to_data(&Default::default())
            .unwrap_or_else(|e| panic!("{} would not serialise: {e}", row.name));
        let back = fontelle_core::Patch::from_data(&data, |_| None)
            .unwrap_or_else(|e| panic!("{} would not read back: {e}", row.name))
            .patch;
        assert_eq!(back, patch, "{} did not survive the round trip", row.name);
    }
}

/// A preset's every control is reachable by address, so automation, MIDI learn
/// and undo reach all of it — the same claim `flopsynth::addresses` makes for
/// the Init patch, made again for the shapes the bank actually contains.
#[test]
fn every_preset_offers_only_addresses_that_work() {
    for row in FACTORY {
        let patch = (row.build)();
        for address in flopsynth::addresses(&patch) {
            assert!(
                fontelle_core::patch_params::value(&patch, &address).is_some(),
                "{}: {address} is offered and cannot be read",
                row.name
            );
        }
    }
}
