//! Measures the whole factory bank in one pass (`docs/flopsynth-plan.md` §7.4).
//!
//! # Why a tool and not just the test
//!
//! `tests/flopsynth_presets.rs` is a *gate*: it says which presets are wrong.
//! Tuning the loudness column from a gate means one preset per run, which is
//! §13's third risk written out — thirty rounds of `cargo test` to write thirty
//! numbers. This renders the bank once and prints the whole table: what each
//! preset measured, how far it is from the median, **and the number to add to
//! its `.out(…)`**, so a sound-design pass is one run and one edit.
//!
//! It measures exactly what the tests measure, on the same signals, so a row
//! that reads clean here passes there:
//!
//! ```text
//! cargo run -p fontelle-core --example preset_probe --release
//! cargo run -p fontelle-core --example preset_probe --release -- Keys
//! ```
//!
//! Three columns, and the fourth is Ty: `--play-flopsynth` is what says
//! whether the sound is any good, and none of this can.

use std::collections::HashMap;
use std::env;

use fontelle_core::flopsynth::presets::FACTORY;
use fontelle_core::{NoteTrigger, PrepareContext, SampleStore, Sampler};

const SR: f32 = 48_000.0;

/// The five axes of one preset: the four scalars and the ten-band shape.
type Axes = ([f32; 4], [f32; 10]);

/// What one row of the table holds: name, category, matched level, the peak it
/// reaches on a loud chord, and its axes.
type Row<'a> = (&'a str, &'a str, f32, f32, Axes);

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
    let mut out = Vec::with_capacity(total);
    let mut done = 0usize;
    let mut released = false;
    while done < total {
        if !released && done >= release_at {
            sampler.release_all();
            released = true;
        }
        let frames = 512.min(total - done);
        // The transport moves, as it does under the engine and in the gate:
        // a free-running LFO reads its phase off the clock.
        sampler.set_clock(fontelle_core::RenderClock {
            bpm: 120.0,
            position_sample: done as u64,
        });
        let mut left = vec![0.0f32; frames];
        let mut right = vec![0.0f32; frames];
        sampler.render(&store, &mut [&mut left[..], &mut right[..]]);
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

/// The loudest 50 ms, which is the window the test's match is made on.
fn loudness(samples: &[f32]) -> f32 {
    let window = (SR * 0.05) as usize;
    samples.chunks(window).map(rms).fold(0.0f32, f32::max)
}

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

/// The four scalar axes, exactly as `every_pair_in_a_category_is_audibly_apart`
/// describes them.
fn describe(samples: &[f32]) -> [f32; 4] {
    let peak_level = peak(samples).max(1e-9);
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

/// The ten-band shape, the fifth axis.
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

fn main() {
    let only: Option<String> = env::args().nth(1);
    let rows: Vec<_> = FACTORY
        .iter()
        .filter(|row| {
            only.as_ref()
                .is_none_or(|want| row.category.label().eq_ignore_ascii_case(want))
        })
        .collect();
    if rows.is_empty() {
        eprintln!("no presets in {only:?}; the categories are:");
        for category in fontelle_core::flopsynth::presets::FlopsynthCategory::ALL {
            eprintln!("  {}", category.label());
        }
        std::process::exit(1);
    }

    // The loudness match is against the **whole** bank's median, not the
    // filtered set's — filtering to one category and re-centring on it would
    // walk that category away from everybody else.
    let mut all_levels: Vec<f32> = Vec::with_capacity(FACTORY.len());
    let mut measured: Vec<Row> = Vec::new();
    for row in FACTORY {
        let matched = render_chord((row.build)(), &[60, 64, 67, 72], 100, 1.0, 1.2);
        let level = db(loudness(&matched));
        all_levels.push(level);
        if !rows.iter().any(|r| r.name == row.name) {
            continue;
        }
        let loud = render_chord((row.build)(), &[60, 64, 67, 72], 127, 1.5, 2.5);
        let one = render_chord((row.build)(), &[60], 100, 0.6, 2.5);
        measured.push((
            row.name,
            row.category.label(),
            level,
            peak(&loud),
            (describe(&one), profile(&one)),
        ));
    }
    all_levels.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median = all_levels[all_levels.len() / 2];

    println!(
        "{} presets; the bank's median is {median:.1} dBFS\n",
        FACTORY.len()
    );
    println!(
        "{:<20} {:<16} {:>8} {:>8} {:>7}",
        "preset", "category", "level", "add", "peak"
    );
    let mut sorted = measured.clone();
    sorted.sort_by(|a, b| a.2.partial_cmp(&b.2).unwrap());
    for (name, category, level, peak_level, _) in &sorted {
        let correction = median - level;
        let flag = if correction.abs() > 3.0 {
            "  <- outside the 3 dB gate"
        } else if *peak_level > 0.98 {
            "  <- clips a four-note chord"
        } else {
            ""
        };
        println!(
            "{name:<20} {category:<16} {level:>7.1}  {correction:>+7.1} {peak_level:>7.3}{flag}"
        );
    }

    // The nearest neighbour inside each category, because "apart" is the axis
    // that a new row actually fails on and knowing *which* preset it collides
    // with is the whole of what a fix needs.
    let mut by_category: HashMap<&str, Vec<(&str, Axes)>> = HashMap::new();
    for (name, category, _, _, axes) in &measured {
        by_category.entry(category).or_default().push((name, *axes));
    }
    // The axes themselves, when one category is asked for: a pair that reads
    // "too close" is close on *some* axis, and which one is the whole of what
    // a fix has to move.
    if only.is_some() {
        println!("\naxes: t30(ln)  centroid  crest  flux(ln) | ten bands, 60 Hz .. 14 kHz");
        let mut rows = measured.clone();
        rows.sort_by(|a, b| a.0.cmp(b.0));
        for (name, _, _, _, (scalar, shape)) in &rows {
            let bands: String = shape
                .iter()
                .map(|b| format!("{:>4.0}", b * 100.0))
                .collect::<Vec<_>>()
                .join("");
            println!(
                "  {name:<20} {:>6.2} {:>8.2} {:>6.2} {:>8.2} |{bands}",
                scalar[0], scalar[1], scalar[2], scalar[3]
            );
        }
    }

    println!("\nnearest neighbour in category (the gate is 0.35):");
    let mut categories: Vec<_> = by_category.keys().copied().collect();
    categories.sort_unstable();
    for category in categories {
        let presets = &by_category[category];
        for (i, (name, a)) in presets.iter().enumerate() {
            let mut nearest: Option<(&str, f32)> = None;
            for (j, (other, b)) in presets.iter().enumerate() {
                if i == j {
                    continue;
                }
                let scalar =
                    a.0.iter()
                        .zip(&b.0)
                        .map(|(x, y)| (x - y).abs())
                        .fold(0.0f32, f32::max);
                let shape: f32 = a.1.iter().zip(&b.1).map(|(x, y)| (x - y).abs()).sum();
                let apart = scalar.max(shape);
                if nearest.is_none_or(|(_, best)| apart < best) {
                    nearest = Some((other, apart));
                }
            }
            if let Some((other, apart)) = nearest {
                let flag = if apart <= 0.35 { "  <- too close" } else { "" };
                println!("  {category:<16} {name:<20} {apart:>5.2} from {other}{flag}");
            }
        }
    }
}
