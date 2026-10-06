//! What a 3-minute song costs to analyse on one core (plan §3.10: under 6 s
//! with basic-pitch). tract runs on the calling thread only.
//!
//! `cargo bench -p fontelle-analysis --bench analysis`

use criterion::{Criterion, criterion_group, criterion_main};
use fontelle_analysis::mono::{PyinParams, pyin, segment};
use fontelle_analysis::testsignals::{melody_and_chords, vibrato_melody};
use fontelle_analysis::transcribe::NoteParams;
use fontelle_analysis::transcribe::basic_pitch::BasicPitch;
use std::hint::black_box;

/// Three minutes of the melody-and-chords fixture, end to end.
fn three_minutes(sample_rate: u32) -> Vec<f32> {
    let piece = melody_and_chords(sample_rate).samples;
    let want = 180 * sample_rate as usize;
    piece.iter().copied().cycle().take(want).collect()
}

fn bench(c: &mut Criterion) {
    let model = BasicPitch::load().expect("the model loads");
    let params = NoteParams::default();
    let mut group = c.benchmark_group("basic_pitch_3_minutes");
    group.sample_size(10);
    let at_22k = three_minutes(22_050);
    group.bench_function("22050", |b| {
        b.iter(|| black_box(model.transcribe(&at_22k, 22_050, &params).unwrap()))
    });
    let at_44k = three_minutes(44_100);
    group.bench_function("44100_with_resampling", |b| {
        b.iter(|| black_box(model.transcribe(&at_44k, 44_100, &params).unwrap()))
    });
    group.finish();

    // The monophonic path (plan §3.10: a 3-minute vocal under 3 s).
    let mut group = c.benchmark_group("pyin_3_minutes");
    group.sample_size(10);
    let piece = vibrato_melody(44_100).samples;
    let vocal: Vec<f32> = piece.iter().copied().cycle().take(180 * 44_100).collect();
    group.bench_function("44100_with_segmentation", |b| {
        b.iter(|| black_box(segment(&pyin(&vocal, 44_100, &PyinParams::default()))))
    });
    group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
