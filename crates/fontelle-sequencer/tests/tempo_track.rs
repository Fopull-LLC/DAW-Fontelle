//! The tempo, compiled onto the timeline the audio thread reads (TDD §6.2,
//! §11.1).
//!
//! `fontelle-engine` cannot see a `TempoMap` — that is `fontelle-model`'s, and
//! INVARIANT 4 runs the other way — so a node that wants to know the tempo
//! (a delay set in note values is the first, and an LFO will be the next)
//! cannot ask the project. It gets one number a block, on the transport
//! snapshot, and this is where that number comes from.
//!
//! Compiling it here rather than inventing a second path is the same argument
//! `param_nodes` settles: the sequencer already owns every tick-to-sample
//! conversion in the project, because that is what compiling a timeline *is*.
//! A tempo table built anywhere else would be a second answer to a question
//! that already has one, with somewhere for the two to drift.

use std::collections::HashMap;

use fontelle_model::{Project, TempoMap, TempoSegment};
use fontelle_types::PPQN;

const SR: f64 = 48_000.0;

fn compile(project: &Project) -> fontelle_types::CompiledTimeline {
    fontelle_sequencer::compile(project, &HashMap::new(), &HashMap::new())
}

#[test]
fn a_project_at_one_tempo_compiles_to_one_segment() {
    let mut project = Project::new("Steady");
    project.tempo_map = TempoMap::new(96.0, SR);

    let timeline = compile(&project);
    assert_eq!(timeline.tempo.len(), 1, "one tempo, one segment");
    assert_eq!(timeline.tempo[0].0, 0, "starting at the beginning");
    assert!((timeline.tempo[0].1 - 96.0).abs() < 1e-3);
    assert!((timeline.bpm_at(0) - 96.0).abs() < 1e-3);
    assert!(
        (timeline.bpm_at(SR as i64 * 600) - 96.0).abs() < 1e-3,
        "and running on for as long as the song does"
    );
}

#[test]
fn a_tempo_change_lands_on_the_sample_it_happens_at() {
    // The join this file exists for: the map is in *ticks* and the block
    // contract is in *samples*, and converting between them is exactly the
    // thing the sequencer is for. A table left in ticks would make every node
    // that reads it do the conversion again, with the tempo map it cannot see.
    let mut project = Project::new("Slowing");
    project.tempo_map = TempoMap::from_segments(
        vec![
            TempoSegment {
                start_tick: 0,
                bpm: 120.0,
            },
            // Four bars in, at 4/4.
            TempoSegment {
                start_tick: PPQN * 16,
                bpm: 60.0,
            },
        ],
        SR,
    );

    let timeline = compile(&project);
    assert_eq!(timeline.tempo.len(), 2);

    // Sixteen beats at 120 bpm is eight seconds.
    let change_at = timeline.tempo[1].0;
    let expected = (8.0 * SR) as i64;
    assert!(
        (change_at - expected).abs() <= 1,
        "the change should land at {expected}, landed at {change_at}"
    );
    assert!((timeline.bpm_at(change_at - 1) - 120.0).abs() < 1e-3);
    assert!((timeline.bpm_at(change_at) - 60.0).abs() < 1e-3);
}

#[test]
fn the_tempo_table_is_sorted_by_sample() {
    // `bpm_at` binary-searches it, and a table out of order is a lookup that
    // returns whichever entry the search happened to land on.
    let mut project = Project::new("Several");
    project.tempo_map = TempoMap::from_segments(
        vec![
            TempoSegment {
                start_tick: 0,
                bpm: 100.0,
            },
            TempoSegment {
                start_tick: PPQN * 4,
                bpm: 150.0,
            },
            TempoSegment {
                start_tick: PPQN * 8,
                bpm: 75.0,
            },
        ],
        SR,
    );

    let timeline = compile(&project);
    assert_eq!(timeline.tempo.len(), 3);
    assert!(
        timeline.tempo.windows(2).all(|w| w[0].0 < w[1].0),
        "the tempo table is not sorted: {:?}",
        timeline.tempo
    );
    // And every segment is findable at its own start.
    for (sample, bpm) in &timeline.tempo {
        assert!((timeline.bpm_at(*sample) - bpm).abs() < 1e-3);
    }
}

#[test]
fn compiling_a_project_twice_gives_the_same_tempo_table() {
    // The compile is a pure function of the document, and the tempo table is
    // not an exception — it is what a node reads to decide how long a delay
    // is, and a table that differed between two compiles of the same project
    // would be a delay that changed length when something unrelated moved.
    let mut project = Project::new("Twice");
    project.tempo_map = TempoMap::from_segments(
        vec![
            TempoSegment {
                start_tick: 0,
                bpm: 128.0,
            },
            TempoSegment {
                start_tick: PPQN * 32,
                bpm: 90.0,
            },
        ],
        SR,
    );
    assert_eq!(compile(&project).tempo, compile(&project).tempo);
}
