//! Where the song *is*, compiled onto the timeline the audio thread reads
//! (`docs/disgusting-beat-plan.md` §3.4).
//!
//! `tempo_track.rs` beside this one compiled the tempo — how long a beat is.
//! This compiles the **position** — which beat it is. The difference is
//! invisible on a delay, whose repeats are relative to whatever went into it,
//! and is the whole thing for an effect whose pattern has to put the same
//! curve under beat 4 of every bar.
//!
//! Deriving it in the node from `position_sample × bpm` is the obvious wrong
//! answer: it integrates the *current* tempo over the *whole* song, so a
//! project with one tempo change plays the pattern in the wrong place from
//! then on — and quietly. `a_tick_is_right_across_a_tempo_change` is that
//! case.

use std::collections::HashMap;

use fontelle_model::{Project, TempoMap, TempoSegment};
use fontelle_types::{CompiledTimeline, PPQN};

const SR: f64 = 48_000.0;

fn compile(project: &Project) -> CompiledTimeline {
    fontelle_sequencer::compile(project, &HashMap::new(), &HashMap::new())
}

/// Every conversion in this file is checked against the map the project holds,
/// which is the one authority on tick-to-sample in this program.
fn assert_tick_matches_the_map(timeline: &CompiledTimeline, map: &TempoMap, sample: i64) {
    let expected = map.sample_to_tick(sample);
    let got = timeline.tick_at(sample);
    assert!(
        (got - expected).abs() <= 1,
        "at sample {sample} the timeline says tick {got}, the map says {expected}"
    );
}

#[test]
fn the_tick_at_a_sample_is_the_tick_the_tempo_map_says() {
    for bpm in [60.0, 96.0, 120.0, 174.0] {
        let mut project = Project::new("Steady");
        project.tempo_map = TempoMap::new(bpm, SR);
        let timeline = compile(&project);

        for beat in [0, 1, 4, 17, 64, 255] {
            let sample = project.tempo_map.tick_to_sample(PPQN * beat);
            assert_tick_matches_the_map(&timeline, &project.tempo_map, sample);
        }
    }
}

#[test]
fn a_tick_is_right_across_a_tempo_change() {
    // The case a `position_sample × bpm` derivation gets wrong. Four bars of
    // 4/4 at 120, then half speed: the sample at bar 9 is twice as far in as
    // the naive answer, and a pattern locked with that arithmetic would be
    // half a bar out for the rest of the song.
    let mut project = Project::new("Slowing");
    project.tempo_map = TempoMap::from_segments(
        vec![
            TempoSegment {
                start_tick: 0,
                bpm: 120.0,
            },
            TempoSegment {
                start_tick: PPQN * 16,
                bpm: 60.0,
            },
        ],
        SR,
    );
    let timeline = compile(&project);

    for beat in [0, 8, 15, 16, 17, 32, 64] {
        let sample = project.tempo_map.tick_to_sample(PPQN * beat);
        assert_tick_matches_the_map(&timeline, &project.tempo_map, sample);
    }

    // And spelt out, so the failure names the thing rather than a number.
    // The naive `position_sample × bpm` reading of this sample is bar 9 plus
    // *two* beats, because it measures the whole song at 60.
    let change = project.tempo_map.tick_to_sample(PPQN * 16);
    let one_beat_later = timeline.tick_at(change + SR as i64);
    assert_eq!(
        one_beat_later,
        PPQN * 17,
        "a beat at 60 bpm is one second long"
    );
}

#[test]
fn a_sample_before_the_song_reads_back_before_the_song() {
    // Reachable: the count-in runs at negative samples, and a node that asked
    // for its phase there must not get a positive tick.
    let mut project = Project::new("Count in");
    project.tempo_map = TempoMap::new(120.0, SR);
    let timeline = compile(&project);

    assert!(
        timeline.tick_at(-(SR as i64)) < 0,
        "a second before the start is a negative tick, not a wrapped one"
    );
}

#[test]
fn ticks_per_sample_is_the_rate_inside_the_segment() {
    // What a node advances its phase by, per sample, without asking again.
    let mut project = Project::new("Rate");
    project.tempo_map = TempoMap::new(120.0, SR);
    let timeline = compile(&project);

    let rate = timeline.ticks_per_sample_at(0);
    let expected = 120.0 / 60.0 * PPQN as f64 / SR;
    assert!(
        (rate - expected).abs() < 1e-9,
        "120 bpm is {expected} ticks a sample, not {rate}"
    );

    // Advancing by it across a block lands where the table says the block
    // ends — this is the property the whole per-sample phase walk rests on.
    let block = 512;
    let walked = timeline.tick_at(0) as f64 + rate * block as f64;
    let asked = timeline.tick_at(block) as f64;
    assert!(
        (walked - asked).abs() <= 1.0,
        "walking a block by the rate ({walked}) and asking for its end ({asked}) disagree"
    );
}

#[test]
fn the_beats_in_a_bar_come_from_the_project() {
    let mut project = Project::new("Waltz");
    project.beats_per_bar = 3;
    let timeline = compile(&project);
    assert_eq!(timeline.beats_per_bar, 3);

    let mut project = Project::new("Common");
    project.beats_per_bar = 4;
    let timeline = compile(&project);
    assert_eq!(timeline.beats_per_bar, 4);
}

#[test]
fn an_uncompiled_timeline_answers_tick_zero_and_four_beats_a_bar() {
    // `CompiledTimeline::default()` is reachable in several places that never
    // compile a project, and every one of its answers has to be musical
    // rather than zero — `beats_per_bar` of nothing is a division by zero on
    // the audio thread.
    let timeline = CompiledTimeline::empty();
    assert_eq!(timeline.tick_at(0), 0);
    assert_eq!(timeline.tick_at(48_000), PPQN * 2, "at the default tempo");
    assert_eq!(timeline.beats_per_bar, 4);
    assert!(timeline.ticks_per_sample_at(0) > 0.0);
}

#[test]
fn a_tick_keeps_its_fraction_for_the_audio_thread() {
    // **A whole tick is not good enough for a DSP.** At 120 bpm and 48 kHz a
    // tick lasts 25 samples, so a rounded answer is a staircase with
    // 25-sample treads — and a node that turns the song's position into a
    // *read position* hears every one of them as a jump. Twenty-five samples
    // of song time is twelve samples of read position on a half-speed slope,
    // once per block: a buzz at the block rate, which is exactly what
    // DisgustingBeat's first release did on every diagonal anybody drew.
    //
    // So there are two answers and they are different questions. `tick_at`
    // is *which tick this is* — what a grid, a marker or a quantiser wants.
    // `tick_at_exact` is *where in it*, and it is what anything that derives
    // a continuous quantity from the song's position must use.
    let project = Project::new("Fractional");
    let timeline = compile(&project);
    let per_sample = timeline.ticks_per_sample_at(0);
    let samples_per_tick = 1.0 / per_sample;
    assert!(
        (samples_per_tick - 25.0).abs() < 1e-9,
        "the arithmetic below assumes 25 samples a tick, got {samples_per_tick}"
    );

    // Twelve samples in, which is just under half a tick: the rounded answer
    // is still zero and the exact one is not.
    assert_eq!(timeline.tick_at(12), 0, "a rounded tick rounds");
    let exact = timeline.tick_at_exact(12);
    assert!(
        (exact - 12.0 / 25.0).abs() < 1e-9,
        "the exact tick lost its fraction: {exact}"
    );

    // And it is a straight line in the samples, which is the property the
    // audio thread actually depends on: no tread anywhere, at any block
    // boundary, for a whole bar.
    for sample in 0..(SR as i64 * 2) {
        let want = sample as f64 * per_sample;
        let got = timeline.tick_at_exact(sample);
        assert!(
            (got - want).abs() < 1e-9,
            "the exact tick stepped at sample {sample}: {got} against {want}"
        );
    }
}
