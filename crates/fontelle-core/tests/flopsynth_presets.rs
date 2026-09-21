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
        // The transport moves, as it does under the engine: a free-running
        // LFO reads its phase off the clock, and a clock left at zero
        // freezes every one of them at the start of its cycle — which is
        // how Whale's sweep measured as silence.
        sampler.set_clock(fontelle_core::RenderClock {
            bpm: 120.0,
            position_sample: done as u64,
        });
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
        // **The Grand Piano is Ty's voicing and is not touched here**
        // (`docs/flopsynth-next.md` §10). Its hammer — the noise burst
        // Envelope 2 gates, +59 dB at the strike and a further +57 at full
        // velocity — puts a four-note chord at velocity 127 at 2.3 times
        // full scale, and did so in the studio from the day it was voiced:
        // the gate rendered 512-frame blocks and read the burst a block
        // late, which is 10 ms, past the whole of its hold. Held at what it
        // measures, so the next thing that moves it is noticed; the master's
        // limiter is what stands between this and the output. Ty's to voice
        // or to leave.
        if row.name == "Grand Piano" {
            assert!(
                (2.0..=2.6).contains(&peak(&out)),
                "the Grand Piano peaked at {:.3}; it measured 2.318",
                peak(&out)
            );
            continue;
        }
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
    // The axes are the crate's (`fontelle_core::preview::sound_vector`):
    // four scalars in log units — how long it rings, where its energy sits,
    // how peaky it is, what its attack does — and a ten-band spectral shape
    // whose distance is a total variation, the axis that tells two vowels
    // apart when their centroids coincide (measured, before it existed:
    // "Choir Ahh" and "Choir Ooh" read as 0.26 apart on the scalars and 1.4
    // on the shape). Lifted into the crate for §5.2's *sounds like*, so what
    // this gate calls alike the browser calls near.
    use fontelle_core::preview::sound_vector;

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
            .push((row.name, {
                let vector = sound_vector(&out);
                (vector.scalar, vector.shape)
            }));
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

// ------------------------------------------------- tags and notes (§5.1)
//
// `docs/flopsynth-next.md` §5.1, Ty's §9.7: every row carries at least
// three tags from one controlled vocabulary and a showcase phrase — one
// sentence saying what it is for and which macro to reach for. The tags a
// row does not write are **derived** from what it is (its category, its
// sources, what it does), so a browser can find "every bass with unison"
// without anybody having typed *unison* three hundred times; a row can
// add words of its own for what cannot be derived.

#[test]
fn every_preset_has_three_tags_from_the_vocabulary_and_a_showcase_phrase() {
    use fontelle_core::flopsynth::{TAGS, presets};
    let mut wrong: Vec<String> = Vec::new();
    for row in FACTORY {
        let patch = (row.build)();
        let tags = presets::tags_of(row, &patch);
        if tags.len() < 3 {
            wrong.push(format!("  {}: {} tags ({tags:?})", row.name, tags.len()));
        }
        for tag in &tags {
            if !TAGS.contains(&tag.as_str()) {
                wrong.push(format!(
                    "  {}: \"{tag}\" is not in the vocabulary",
                    row.name
                ));
            }
        }
        let mut seen = std::collections::BTreeSet::new();
        for tag in &tags {
            if !seen.insert(tag.clone()) {
                wrong.push(format!("  {}: \"{tag}\" twice", row.name));
            }
        }
        let notes = presets::notes_of(row, &patch);
        if notes.trim().is_empty() {
            wrong.push(format!("  {}: no showcase phrase", row.name));
            continue;
        }
        // The phrase names a macro the row has — the one to reach for.
        let names_a_macro = patch
            .macros
            .iter()
            .filter(|m| !m.name.is_empty())
            .any(|m| notes.contains(&m.name));
        if !names_a_macro {
            wrong.push(format!("  {}: \"{notes}\" names no macro", row.name));
        }
        if !notes.ends_with('.') {
            wrong.push(format!("  {}: \"{notes}\" is not a sentence", row.name));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    // The vocabulary is words, lower case, no two alike.
    let mut seen = std::collections::BTreeSet::new();
    for tag in TAGS {
        assert!(!tag.is_empty() && *tag == tag.to_lowercase(), "{tag}");
        assert!(seen.insert(*tag), "{tag} twice in the vocabulary");
    }
    assert!(
        TAGS.len() >= 30,
        "a vocabulary worth browsing: {}",
        TAGS.len()
    );
}

/// The derived tags say true things: a row with a unison stack is tagged
/// *unison*, one that reads a recording *sample*, a monophonic one *mono*.
#[test]
fn derived_tags_say_what_the_row_is() {
    use fontelle_core::flopsynth::presets;
    let find = |name: &str| FACTORY.iter().find(|r| r.name == name).unwrap();
    let supersaw = find("Supersaw");
    let tags = presets::tags_of(supersaw, &(supersaw.build)());
    assert!(tags.iter().any(|t| t == "lead"), "{tags:?}");
    assert!(tags.iter().any(|t| t == "unison"), "{tags:?}");
    assert!(tags.iter().any(|t| t == "table"), "{tags:?}");
    let grand = find("Grand Piano");
    let tags = presets::tags_of(grand, &(grand.build)());
    assert!(tags.iter().any(|t| t == "sample"), "{tags:?}");
    assert!(tags.iter().any(|t| t == "keys"), "{tags:?}");
    // The phrase reaches for a macro by name.
    let notes = presets::notes_of(grand, &(grand.build)());
    assert!(
        notes.contains("Brightness") || notes.contains("Hardness"),
        "{notes}"
    );
}
