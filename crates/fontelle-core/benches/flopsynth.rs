//! What a Flopsynth voice costs (`docs/flopsynth-plan.md` §10).
//!
//! The first bench in this tree. TDD §20.5 asked for benches in CI from day
//! one and the `benches/` folder has been empty since; a synthesiser is the
//! right thing to open it with, because it is the first instrument here whose
//! per-voice cost is a *design* question rather than a memcpy.
//!
//! # What the numbers mean
//!
//! Everything below renders one second of audio at 48 kHz in blocks of 512 and
//! is reported as **a share of one core**: a case that takes 10 ms of CPU to
//! make 1 s of sound is at 1 %. That is the unit §10's targets are written in,
//! and the unit that answers the only question worth asking — how many of
//! these can play at once before the callback misses.
//!
//! Criterion measures wall time per iteration; one iteration here is one
//! second of audio, so the percentage is `time / 1 s` and the printout reads
//! directly against the budget:
//!
//! | case | budget (§10) |
//! |---|---|
//! | one voice of Init | ≤ 0.3 % |
//! | one voice of Supersaw | ≤ 1.2 % |
//! | sixteen voices of Choir Ahh | (with its chain: see `fontelle-engine`) |
//! | the wavetable bank's build | once, at first use |
//!
//! The chain — a preset's own chorus and reverb — runs in `SamplerNode`, which
//! is `fontelle-engine`'s and may not be seen from here (INVARIANT 4). Its
//! half of §10's sixteen-voice case is `fontelle-engine/benches/flopsynth_chain.rs`.

use criterion::{Criterion, criterion_group, criterion_main};
use fontelle_core::{Patch, PrepareContext, SampleStore, Sampler};
use std::hint::black_box;

const SR: f32 = 48_000.0;
const BLOCK: usize = 512;
/// One second of audio, which is what makes a result a percentage.
const BLOCKS: usize = (SR as usize) / BLOCK;

/// A sampler on `patch`, prepared and holding `voices` notes.
fn sounding(patch: Patch, voices: usize) -> Sampler {
    let mut sampler = Sampler::new(patch);
    sampler.prepare(&PrepareContext {
        sample_rate: SR,
        max_block_size: BLOCK as u32,
    });
    // Spread across the keyboard rather than sixteen of one note: the table
    // read picks a mip level from the pitch, so sixteen unisons of C1 and
    // sixteen of C6 are not the same amount of work.
    for index in 0..voices {
        sampler.note_on(48 + (index as u8 * 3) % 36, 100, 0);
    }
    sampler
}

/// One second of audio, block by block, as the callback would.
fn render(sampler: &mut Sampler, store: &SampleStore, left: &mut [f32], right: &mut [f32]) {
    for _ in 0..BLOCKS {
        let mut out: [&mut [f32]; 2] = [left, right];
        sampler.render(store, &mut out);
        black_box(&out);
    }
}

fn named(name: &str) -> Patch {
    let row = fontelle_core::flopsynth::presets::FACTORY
        .iter()
        .find(|preset| preset.name == name)
        .unwrap_or_else(|| panic!("no preset called {name}"));
    (row.build)()
}

fn voices(c: &mut Criterion) {
    let store = SampleStore::new();
    let mut left = vec![0.0f32; BLOCK];
    let mut right = vec![0.0f32; BLOCK];
    let mut group = c.benchmark_group("flopsynth/second-of-audio");
    // One iteration is one second of sound, so the wall time *is* the share
    // of a core. Few samples, because each one is a second of work.
    group.sample_size(10);

    // The same voice at 4× (`docs/flopsynth-next.md` §6: ≤3.5 % against
    // ≤1.4 % at Off): every oscillator four times over, and the ladder if it
    // had one.
    let mut supersaw_2x = named("Supersaw");
    supersaw_2x.oversampling = fontelle_dsp::Oversampling::X2;
    let mut supersaw_4x = named("Supersaw");
    supersaw_4x.oversampling = fontelle_dsp::Oversampling::X4;
    let cases: [(&str, Patch, usize); 7] = [
        (
            "init/1 voice",
            fontelle_core::flopsynth::flopsynth_init(),
            1,
        ),
        // Three oscillators of seven-voice unison through two filters: the
        // heaviest single voice the bank has, and the one §10 budgets at 1.2 %.
        ("supersaw/1 voice", named("Supersaw"), 1),
        ("supersaw/2x", supersaw_2x, 1),
        ("supersaw/4x", supersaw_4x, 1),
        // The formant filter, and sixteen of them — the chord case.
        ("choir ahh/16 voices", named("Choir Ahh"), 16),
        // Two strings of sixty-four partials, three unison voices each: the
        // heaviest *source* the bank has, and a ten-finger chord of it.
        ("grand piano/1 voice", named("Grand Piano"), 1),
        ("grand piano/10 voices", named("Grand Piano"), 10),
    ];
    for (name, patch, count) in cases {
        group.bench_function(name, |b| {
            b.iter_batched_ref(
                || sounding(patch.clone(), count),
                |sampler| render(sampler, &store, &mut left, &mut right),
                criterion::BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

/// Where the cost goes (§10's last paragraph), as differences rather than as
/// a guess: the same voice with one thing taken away at a time.
fn attribution(c: &mut Criterion) {
    let store = SampleStore::new();
    let mut left = vec![0.0f32; BLOCK];
    let mut right = vec![0.0f32; BLOCK];
    let mut group = c.benchmark_group("flopsynth/where-it-goes");
    group.sample_size(10);

    let mut no_filters = fontelle_core::flopsynth::flopsynth_init();
    for slot in &mut no_filters.filters {
        slot.enabled = false;
    }
    let mut no_env_routes = fontelle_core::flopsynth::flopsynth_init();
    no_env_routes.mod_matrix.routes.clear();

    for (name, patch) in [
        ("init, filters off", no_filters),
        ("init, matrix empty", no_env_routes),
    ] {
        group.bench_function(name, |b| {
            b.iter_batched_ref(
                || sounding(patch.clone(), 1),
                |sampler| render(sampler, &store, &mut left, &mut right),
                criterion::BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

fn bank(c: &mut Criterion) {
    // Every table, every mip level, from its recipe. Once, at first use, off
    // the audio thread — but a person waits for it, so it is worth knowing.
    //
    // `WavetableBank::new` on its own measures nothing: the bank is **lazy**,
    // and an empty one is a `Mutex` and a `HashMap`. Asking for all
    // thirty-nine is what builds them.
    c.bench_function("flopsynth/wavetable bank", |b| {
        b.iter(|| {
            let bank = fontelle_dsp::WavetableBank::new();
            for id in fontelle_dsp::WavetableId::ALL {
                black_box(bank.get(id));
            }
        });
    });
}

criterion_group!(benches, voices, attribution, bank);
criterion_main!(benches);
