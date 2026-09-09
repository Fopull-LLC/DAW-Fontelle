//! What the pitch corrector costs (`docs/tune-plan.md` §10).
//!
//! A stereo block of 128 at 48 kHz, each engine, each mode, alto/tenor — and
//! the worst case the plan names: the low range in live mode, where the coarse
//! search runs over the longest lags at twice the rate.
//!
//! The number that matters is the fraction of one core: a block of 128 at
//! 48 kHz is 2.667 ms of audio, so a hundred microseconds is 3.75 %.

use criterion::{Criterion, criterion_group, criterion_main};
use fontelle_fx::{NoteInput, Tune};
use fontelle_types::{TuneConfig, TuneEngine, TuneMode, TuneRange};

const RATE: f32 = 48_000.0;
const BLOCK: usize = 128;

/// A vowel, which is what this actually runs on — a sine would let the
/// tracker's continuity guard off the hook it exists for.
fn vowel(f0: f32, seconds: f32) -> Vec<f32> {
    let n = (seconds * RATE) as usize;
    let formants = [700.0f32, 1_200.0, 2_600.0];
    let partials = ((RATE / 2.0 / f0) as usize).clamp(1, 40);
    (0..n)
        .map(|i| {
            let t = i as f32 / RATE;
            let mut sum = 0.0;
            for k in 1..=partials {
                let hz = f0 * k as f32;
                let mut gain = 0.0;
                for f in formants {
                    let bw = f * 0.10;
                    gain += 1.0 / (1.0 + ((hz - f) / bw).powi(2));
                }
                sum += gain / k as f32 * (std::f32::consts::TAU * hz * t).sin();
            }
            sum * 0.12
        })
        .collect()
}

fn bench_one(criterion: &mut Criterion, name: &str, config: TuneConfig) {
    let source = vowel(180.0, 1.0);
    let mut tune = Tune::new();
    tune.prepare(RATE, &config);
    let mut left = vec![0.0f32; BLOCK];
    let mut right = vec![0.0f32; BLOCK];
    let mut at = 0usize;
    criterion.bench_function(name, |b| {
        b.iter(|| {
            let block = &source[at..at + BLOCK];
            left.copy_from_slice(block);
            right.copy_from_slice(block);
            {
                let mut channels: [&mut [f32]; 2] = [&mut left, &mut right];
                tune.process(&mut channels, NoteInput::default(), &config, 120.0);
            }
            at = (at + BLOCK) % (source.len() - BLOCK);
        });
    });
}

fn benches(criterion: &mut Criterion) {
    for (name, engine) in [
        ("smooth", TuneEngine::Smooth),
        ("hard", TuneEngine::Hard),
        ("grain", TuneEngine::Grain),
    ] {
        bench_one(
            criterion,
            &format!("tune/{name}/studio/alto"),
            TuneConfig {
                engine,
                ..TuneConfig::new()
            },
        );
    }
    bench_one(
        criterion,
        "tune/smooth/live/alto",
        TuneConfig {
            mode: TuneMode::Live,
            ..TuneConfig::new()
        },
    );
    // The worst case §10 names: the longest lags at the shortest hop.
    bench_one(
        criterion,
        "tune/smooth/live/low",
        TuneConfig {
            mode: TuneMode::Live,
            range: TuneRange::Low,
            ..TuneConfig::new()
        },
    );
}

criterion_group!(tune, benches);
criterion_main!(tune);
