//! Automation on the wire (TDD §12, §11.1).
//!
//! `fontelle-model/tests/automation.rs` is the half that reads a value off a
//! curve. This is the half that turns a curve into events the audio thread can
//! act on — and the decision it encodes is the **control rate**.
//!
//! A parameter could be emitted per sample, which is a hundred thousand events
//! a second per lane for a smoothness nobody can hear, or per block, which the
//! sequencer cannot do because it does not know the block size. So it is a
//! fixed interval in samples, chosen to be shorter than any block the engine
//! runs — see `AUTOMATION_INTERVAL`. The node applies what arrived; the
//! voice's mod matrix is block-rate for exactly the same reason.

use std::collections::HashMap;

use fontelle_model::{
    Arena, AutomationData, AutomationPoint, Clip, ClipSource, CurveShape, Lane, Project, TempoMap,
};
use fontelle_types::{EventPayload, NodeId, PPQN, ParamAddress, Tick};
use slotmap::KeyData;

const SR: f64 = 48_000.0;
/// 120 BPM at 48 kHz is exactly 25 samples per tick.
const SAMPLES_PER_TICK: i64 = 25;

fn node(n: u64) -> NodeId {
    NodeId::from(KeyData::from_ffi(n))
}

fn target() -> ParamAddress {
    ParamAddress::new("mixer:5/insert[0]/param/band1.gain")
}

fn point(tick: Tick, value: f64) -> AutomationPoint {
    AutomationPoint {
        tick,
        value,
        curve: CurveShape::Linear,
        tension: 0.0,
    }
}

/// A project with one automation clip from `start`, `length` long.
fn compile(
    start: Tick,
    length: Tick,
    points: Vec<AutomationPoint>,
) -> fontelle_types::CompiledTimeline {
    let mut project = Project::new("automation");
    project.tempo_map = TempoMap::new(120.0, SR);
    let lane = project.lanes.insert(Lane {
        name: "auto".into(),
        height: 32.0,
        color: [0; 4],
        muted: false,
        locked: false,
    });
    let mut arena = Arena::default();
    for p in points {
        arena.insert(p);
    }
    project.clips.insert(Clip {
        lane,
        start,
        length,
        source: ClipSource::Automation(AutomationData {
            target: target(),
            points: arena,
        }),
        prefab_link: None,
        color: None,
        muted: false,
        loop_length: None,
    });

    let param_nodes: HashMap<ParamAddress, NodeId> = [(target(), node(7))].into_iter().collect();
    fontelle_sequencer::compile(&project, &HashMap::new(), &param_nodes)
}

/// Every parameter event in the timeline, as `(sample, target node, value)`.
fn params(timeline: &fontelle_types::CompiledTimeline) -> Vec<(i64, NodeId, f64)> {
    timeline
        .events
        .iter()
        .filter_map(|event| match &event.payload {
            EventPayload::ParamValue { value, .. } => Some((event.sample, event.target, *value)),
            _ => None,
        })
        .collect()
}

#[test]
fn a_project_with_no_automation_emits_no_parameter_events() {
    let timeline = compile(0, PPQN * 4, Vec::new());
    assert!(params(&timeline).is_empty());
}

#[test]
fn an_automation_clip_emits_events_at_the_node_that_owns_its_target() {
    // The address is resolved *here*, off the graph, rather than on the audio
    // thread: parsing a string per event per block is work the RT side should
    // never be doing, and the map is something only the realisation step knows.
    let timeline = compile(0, PPQN * 4, vec![point(0, 0.0), point(PPQN * 4, 1.0)]);
    let events = params(&timeline);
    assert!(!events.is_empty(), "a sweep should emit something");
    assert!(
        events.iter().all(|(_, target, _)| *target == node(7)),
        "every one addressed to the node holding that parameter"
    );
}

#[test]
fn a_target_no_node_owns_emits_nothing() {
    // A project whose automation names a track that has been deleted, or a
    // parameter this build does not have. Silence, not a panic and not an
    // event nobody reads.
    let mut project = Project::new("orphan");
    project.tempo_map = TempoMap::new(120.0, SR);
    let lane = project.lanes.insert(Lane {
        name: "auto".into(),
        height: 32.0,
        color: [0; 4],
        muted: false,
        locked: false,
    });
    let mut arena = Arena::default();
    arena.insert(point(0, 0.5));
    project.clips.insert(Clip {
        lane,
        start: 0,
        length: PPQN * 4,
        source: ClipSource::Automation(AutomationData {
            target: ParamAddress::new("mixer:999/gain"),
            points: arena,
        }),
        prefab_link: None,
        color: None,
        muted: false,
        loop_length: None,
    });
    let timeline = fontelle_sequencer::compile(&project, &HashMap::new(), &HashMap::new());
    assert!(params(&timeline).is_empty());
}

#[test]
fn a_sweep_is_emitted_often_enough_to_sound_like_one() {
    // One bar at 120 BPM is two seconds. At the control rate that is hundreds
    // of steps, which is what makes a filter sweep a sweep rather than a
    // staircase.
    let timeline = compile(0, PPQN * 4, vec![point(0, 0.0), point(PPQN * 4, 1.0)]);
    let events = params(&timeline);
    assert!(
        events.len() > 100,
        "a two-second sweep produced only {} steps",
        events.len()
    );
}

#[test]
fn the_values_follow_the_curve_that_was_drawn() {
    let timeline = compile(0, PPQN * 4, vec![point(0, 0.0), point(PPQN * 4, 1.0)]);
    let events = params(&timeline);
    let (first, last) = (events[0], events[events.len() - 1]);
    assert!(first.2 < 0.05, "it starts at the bottom: {}", first.2);
    assert!(last.2 > 0.95, "and ends at the top: {}", last.2);
    // And rises the whole way, which a curve read backwards would not.
    for pair in events.windows(2) {
        assert!(
            pair[1].2 >= pair[0].2 - 1e-9,
            "the sweep went backwards at sample {}",
            pair[1].0
        );
    }
}

#[test]
fn a_clip_that_starts_late_emits_nothing_before_it() {
    // §12.2: a clip cannot reach back in time. Before it, the parameter
    // belongs to the knob.
    let timeline = compile(PPQN * 8, PPQN * 4, vec![point(0, 0.5)]);
    let events = params(&timeline);
    assert!(
        events
            .iter()
            .all(|(sample, _, _)| *sample >= PPQN * 8 * SAMPLES_PER_TICK),
        "an event landed before the clip started"
    );
}

#[test]
fn a_flat_curve_costs_one_event_rather_than_hundreds() {
    // Nothing is changing, so nothing needs saying twice. A lane that emitted
    // at the control rate regardless would put a hundred thousand identical
    // events in a five-minute song.
    let timeline = compile(0, PPQN * 16, vec![point(0, 0.4)]);
    let events = params(&timeline);
    assert_eq!(events.len(), 1, "one event sets it and it stays set");
    assert!((events[0].2 - 0.4).abs() < 1e-9);
}

#[test]
fn the_value_a_clip_leaves_behind_is_emitted_once_at_its_end() {
    // §12.2's second rule needs one event to latch it: after the clip, the
    // parameter holds what it left, and the audio thread learns that by being
    // told rather than by knowing where clips are.
    let timeline = compile(0, PPQN * 4, vec![point(0, 0.0), point(PPQN * 4, 1.0)]);
    let events = params(&timeline);
    let end = PPQN * 4 * SAMPLES_PER_TICK;
    let last = events[events.len() - 1];
    assert!(
        last.0 <= end && last.2 > 0.95,
        "the last event should land at the clip's end carrying its final value: {last:?}"
    );
}

#[test]
fn a_muted_automation_clip_emits_nothing() {
    let mut project = Project::new("muted");
    project.tempo_map = TempoMap::new(120.0, SR);
    let lane = project.lanes.insert(Lane {
        name: "auto".into(),
        height: 32.0,
        color: [0; 4],
        muted: false,
        locked: false,
    });
    let mut arena = Arena::default();
    arena.insert(point(0, 0.0));
    arena.insert(point(PPQN * 4, 1.0));
    project.clips.insert(Clip {
        lane,
        start: 0,
        length: PPQN * 4,
        source: ClipSource::Automation(AutomationData {
            target: target(),
            points: arena,
        }),
        prefab_link: None,
        color: None,
        muted: true,
        loop_length: None,
    });
    let param_nodes: HashMap<ParamAddress, NodeId> = [(target(), node(7))].into_iter().collect();
    let timeline = fontelle_sequencer::compile(&project, &HashMap::new(), &param_nodes);
    assert!(params(&timeline).is_empty());
}

#[test]
fn the_timeline_stays_sorted_by_time_with_automation_in_it() {
    // The RT side walks the event list forwards and never sorts. A timeline
    // that interleaved automation out of order would apply a sweep backwards.
    let timeline = compile(0, PPQN * 4, vec![point(0, 0.0), point(PPQN * 4, 1.0)]);
    for pair in timeline.events.windows(2) {
        assert!(
            pair[0].sample <= pair[1].sample,
            "out of order at {}",
            pair[0].sample
        );
    }
}

#[test]
fn where_two_clips_overlap_the_later_one_is_what_is_emitted() {
    // §12.2's first rule, on the wire this time: the compiler must not emit
    // both and leave the audio thread to sort out which wins.
    let mut project = Project::new("overlap");
    project.tempo_map = TempoMap::new(120.0, SR);
    let lane = project.lanes.insert(Lane {
        name: "auto".into(),
        height: 32.0,
        color: [0; 4],
        muted: false,
        locked: false,
    });
    for (start, value) in [(0, 0.2), (PPQN * 4, 0.9)] {
        let mut arena = Arena::default();
        arena.insert(point(0, value));
        project.clips.insert(Clip {
            lane,
            start,
            length: PPQN * 8,
            source: ClipSource::Automation(AutomationData {
                target: target(),
                points: arena,
            }),
            prefab_link: None,
            color: None,
            muted: false,
            loop_length: None,
        });
    }
    let param_nodes: HashMap<ParamAddress, NodeId> = [(target(), node(7))].into_iter().collect();
    let timeline = fontelle_sequencer::compile(&project, &HashMap::new(), &param_nodes);

    // From where the second clip starts onward. Only one event lands there —
    // the value stops changing, and a curve that is not going anywhere costs
    // one event, not one per interval.
    let events = params(&timeline);
    let in_overlap: Vec<f64> = events
        .iter()
        .filter(|(sample, _, _)| *sample >= PPQN * 4 * SAMPLES_PER_TICK)
        .map(|(_, _, value)| *value)
        .collect();
    assert!(!in_overlap.is_empty(), "the switch has to be stated");
    assert!(
        in_overlap.iter().all(|value| (*value - 0.9).abs() < 1e-6),
        "the later clip's value alone, got {in_overlap:?}"
    );
    assert!(
        events
            .iter()
            .any(|(sample, _, value)| *sample < PPQN * 4 * SAMPLES_PER_TICK
                && (*value - 0.2).abs() < 1e-6),
        "and the earlier one's before that"
    );
}
