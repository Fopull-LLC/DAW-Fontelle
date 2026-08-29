//! Play, stop, seek and loop, end to end: a real document, a real graph, and
//! real audio out the other side.
//!
//! These drive `TransportReader` in the same loop `AudioDevice`'s callback
//! does, because the callback itself can only be run by a sound card. That is
//! the reason the per-block decision lives in a type of its own — every
//! behaviour below would otherwise be checkable only by ear.

mod common;

use std::sync::Arc;

use fontelle_app::{SampleLibrary, render_offline_with_transport};
use fontelle_core::{
    FilterSlot, Layer, LoopMode, ModMatrix, Patch, PlaybackConfig, SampleBuffer, Source,
    VoiceConfig,
};
use fontelle_dsp::{EnvelopeConfig, EnvelopeCurve, Interpolation, SvfMode};
use fontelle_engine::{BLOCK_SIZE, CompiledGraph, Transport, TransportReader, TransportState};
use fontelle_model::Project;
use fontelle_types::CompiledTimeline;

use common::SR;

/// A looped sine with a long release, so a note is still sounding — and would
/// keep sounding — when the transport is stopped underneath it. A short
/// release would make "stop silences it" true by accident.
fn sustained_patch(library: &mut SampleLibrary) -> Patch {
    let cycle = 100;
    let data: Vec<f32> = (0..cycle)
        .map(|i| (i as f32 / cycle as f32 * std::f32::consts::TAU).sin())
        .collect();
    let asset = library.insert_synthetic(
        "sustained",
        SampleBuffer {
            data: Arc::from(data),
            sample_rate: SR,
        },
    );
    let disabled = FilterSlot {
        mode: SvfMode::Lowpass,
        cutoff_hz: 20_000.0,
        resonance: 0.0,
        enabled: false,
    };
    let env = EnvelopeConfig {
        delay_s: 0.0,
        attack_s: 0.0,
        hold_s: 0.0,
        decay_s: 0.0,
        sustain_level: 1.0,
        release_s: 4.0,
        curve: EnvelopeCurve::Linear,
    };
    Patch {
        layers: vec![Layer {
            source: Source::Sample { file: asset },
            key_range: (0, 127),
            vel_range: (0, 127),
            root_key: 60,
            fine_tune_cents: 0.0,
            playback: PlaybackConfig {
                loop_mode: LoopMode::Forward,
                loop_start: 0.0,
                loop_end: cycle as f64,
                end_offset: cycle as f64,
                interpolation: Some(Interpolation::Normal),
                ..PlaybackConfig::default()
            },
            gain_db: 0.0,
            pan: 0.0,
        }],
        filters: [disabled, disabled],
        envelopes: vec![env, env],
        lfos: Vec::new(),
        mod_matrix: ModMatrix::default(),
        voice_config: VoiceConfig::default(),
    }
}

/// The demo document, its graph and its timeline — through the same
/// realisation step the CLI uses, so what these tests drive is what a run of
/// `--play-sf2` drives.
fn song_and_graph() -> (Project, CompiledGraph, CompiledTimeline) {
    let mut library = SampleLibrary::new();
    let patch = sustained_patch(&mut library);
    let (project, realised, timeline) =
        common::demo_rig(&patch, &library, fontelle_app::PLAYBACK_QUALITY);
    (project, realised.graph, timeline)
}

/// The device callback's loop, minus the interleave and the sound card.
/// Renders one block and appends it, so a test can change the transport
/// between blocks the way a user changes it between callbacks.
fn render_one_block(
    reader: &mut TransportReader,
    transport: &Transport,
    timeline: &fontelle_types::CompiledTimeline,
    graph: &mut CompiledGraph,
    out: &mut Vec<f32>,
) {
    let step = reader.next_step(transport, timeline, BLOCK_SIZE, BLOCK_SIZE, false);
    if step.reset {
        graph.reset_sequenced();
    }
    if step.process {
        graph.process_block(step.events, step.snapshot, step.range.clone());
        for i in 0..step.frames {
            out.push(graph.buffer_pool.buffer_mut(0)[i]);
            out.push(graph.buffer_pool.buffer_mut(1)[i]);
        }
    } else {
        out.extend(std::iter::repeat_n(0.0, step.frames * 2));
    }
}

fn peak(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0f32, |m, s| m.max(s.abs()))
}

#[test]
fn a_stopped_transport_renders_silence_and_a_playing_one_does_not() {
    let (_project, mut graph, timeline) = song_and_graph();
    let transport = Transport::new();
    let mut reader = TransportReader::new();

    let mut stopped = Vec::new();
    for _ in 0..8 {
        render_one_block(&mut reader, &transport, &timeline, &mut graph, &mut stopped);
    }
    assert_eq!(peak(&stopped), 0.0, "nothing plays until play is pressed");

    transport.play();
    let mut playing = Vec::new();
    for _ in 0..8 {
        render_one_block(&mut reader, &transport, &timeline, &mut graph, &mut playing);
    }
    assert!(
        peak(&playing) > 0.01,
        "the first note of the phrase should be sounding, peak was {}",
        peak(&playing)
    );
}

#[test]
fn stopping_silences_the_output_for_as_long_as_it_is_stopped() {
    // The §6.3 idle claim, and only that. It is deliberately *not* evidence
    // that the stop cut the voices: a stopped transport runs no nodes at all,
    // so this passes whether or not anything was silenced. What the reset
    // actually buys is the test below.
    let (_project, mut graph, timeline) = song_and_graph();
    let transport = Transport::new();
    transport.play();
    let mut reader = TransportReader::new();

    let mut before = Vec::new();
    for _ in 0..16 {
        render_one_block(&mut reader, &transport, &timeline, &mut graph, &mut before);
    }
    assert!(
        peak(&before) > 0.01,
        "a note must actually be sounding first"
    );

    transport.stop();
    let mut after = Vec::new();
    for _ in 0..16 {
        render_one_block(&mut reader, &transport, &timeline, &mut graph, &mut after);
    }
    assert_eq!(
        peak(&after),
        0.0,
        "every sample after the stop must be exactly zero"
    );
}

#[test]
fn a_note_sounding_at_the_stop_does_not_come_back_when_play_is_pressed_again() {
    // This is what resetting the graph on stop is *for*, and the only place
    // the difference is observable. The patch sustains indefinitely with a
    // four-second release, and there is no note-on at the resume position, so
    // a voice that survived the stop is the only thing that could make sound
    // here. Written after the obvious version of this test — "stop produces
    // silence" — turned out to pass with the reset removed entirely.
    let (_project, mut graph, timeline) = song_and_graph();
    let transport = Transport::new();
    transport.play();
    let mut reader = TransportReader::new();

    let mut sounding = Vec::new();
    for _ in 0..16 {
        render_one_block(
            &mut reader,
            &transport,
            &timeline,
            &mut graph,
            &mut sounding,
        );
    }
    assert!(
        peak(&sounding) > 0.01,
        "a note must actually be sounding first"
    );

    transport.stop();
    let mut ignored = Vec::new();
    render_one_block(&mut reader, &transport, &timeline, &mut graph, &mut ignored);

    transport.play();
    let mut resumed = Vec::new();
    for _ in 0..4 {
        render_one_block(&mut reader, &transport, &timeline, &mut graph, &mut resumed);
    }
    assert_eq!(
        peak(&resumed),
        0.0,
        "the voice that was sounding at the stop was cut, so resuming plays nothing \
         until the next note-on"
    );
}

#[test]
fn the_playhead_stops_where_playback_stopped_and_play_resumes_from_there() {
    let (_project, mut graph, timeline) = song_and_graph();
    let transport = Transport::new();
    transport.play();
    let mut reader = TransportReader::new();

    let mut out = Vec::new();
    for _ in 0..10 {
        render_one_block(&mut reader, &transport, &timeline, &mut graph, &mut out);
    }
    let stopped_at = transport.position_sample();
    assert_eq!(stopped_at, 10 * BLOCK_SIZE as i64);

    transport.stop();
    for _ in 0..5 {
        render_one_block(&mut reader, &transport, &timeline, &mut graph, &mut out);
    }
    assert_eq!(
        transport.position_sample(),
        stopped_at,
        "a stopped playhead does not creep"
    );

    transport.play();
    render_one_block(&mut reader, &transport, &timeline, &mut graph, &mut out);
    assert_eq!(
        transport.position_sample(),
        stopped_at + BLOCK_SIZE as i64,
        "play picks up where stop left off"
    );
}

#[test]
fn seeking_back_to_the_start_replays_the_piece_identically() {
    // Sample-for-sample identical, not merely "makes sound": a seek resets the
    // graph and rewinds the event cursor, so the second pass is the same
    // render as the first from the same starting state. Anything weaker would
    // pass with a cursor that had been left partway through the timeline.
    let (_project, mut graph, timeline) = song_and_graph();
    let transport = Transport::new();
    transport.play();
    let mut reader = TransportReader::new();

    let mut first = Vec::new();
    for _ in 0..24 {
        render_one_block(&mut reader, &transport, &timeline, &mut graph, &mut first);
    }

    transport.seek(0);
    let mut second = Vec::new();
    for _ in 0..24 {
        render_one_block(&mut reader, &transport, &timeline, &mut graph, &mut second);
    }

    assert!(
        peak(&first) > 0.01,
        "the phrase has to be audible to compare"
    );
    assert_eq!(
        first, second,
        "playing the same passage twice sounds the same"
    );
}

#[test]
fn seeking_forwards_lands_in_the_middle_of_the_phrase() {
    // The demo phrase holds a triad from beat 1.5 onward. Seeking straight to
    // it plays it — a note whose note-on is behind the playhead is not
    // retriggered, which is exactly what makes this the interesting case: what
    // sounds is whatever the *next* events say, and there is one right after.
    let (project, mut graph, timeline) = song_and_graph();
    let transport = Transport::new();
    transport.play();
    let mut reader = TransportReader::new();

    let chord_start = project
        .tempo_map
        .tick_to_sample(3 * (fontelle_types::PPQN / 2));
    transport.seek(chord_start);

    let mut out = Vec::new();
    for _ in 0..16 {
        render_one_block(&mut reader, &transport, &timeline, &mut graph, &mut out);
    }
    assert!(
        peak(&out) > 0.01,
        "the held chord starts at the seek target, peak was {}",
        peak(&out)
    );
}

#[test]
fn a_loop_renders_its_second_pass_exactly_like_its_first() {
    let (project, mut graph, timeline) = song_and_graph();
    let transport = Transport::new();
    transport.set_state(TransportState::Rendering);

    // One bar of the demo phrase, looped: long enough to contain the run and
    // to cross several blocks, and not a whole multiple of the block size, so
    // the seam genuinely falls inside a block rather than tidily between two.
    let loop_end_tick = fontelle_types::PPQN * 2 + 37;
    let loop_end = project.tempo_map.tick_to_sample(loop_end_tick);
    transport.set_loop_range((0, loop_end_tick), (0, loop_end));
    transport.set_looping(true);

    let audio = render_offline_with_transport(&timeline, &mut graph, loop_end * 2, &transport);
    let (first, second) = audio.split_at(loop_end as usize * 2);

    assert!(
        peak(first) > 0.01,
        "the loop has to contain audible material"
    );
    assert_eq!(
        first, second,
        "a loop that plays differently the second time round is not a loop"
    );
}

#[test]
fn a_loop_never_plays_material_from_past_its_end() {
    // The strong version of the same claim: render past the loop and compare
    // against a straight render, which diverges the moment the loop wraps.
    let (project, mut graph, timeline) = song_and_graph();
    let looped = Transport::new();
    looped.set_state(TransportState::Rendering);
    let loop_end_tick = fontelle_types::PPQN;
    let loop_end = project.tempo_map.tick_to_sample(loop_end_tick);
    looped.set_loop_range((0, loop_end_tick), (0, loop_end));
    looped.set_looping(true);
    let with_loop = render_offline_with_transport(&timeline, &mut graph, loop_end * 2, &looped);

    let (_project, mut graph, timeline) = song_and_graph();
    let straight = Transport::new();
    straight.set_state(TransportState::Rendering);
    let without_loop =
        render_offline_with_transport(&timeline, &mut graph, loop_end * 2, &straight);

    assert_eq!(
        with_loop[..loop_end as usize * 2],
        without_loop[..loop_end as usize * 2],
        "up to the loop end the two renders are the same playback"
    );
    assert_ne!(
        with_loop[loop_end as usize * 2..],
        without_loop[loop_end as usize * 2..],
        "past it, the looped render has gone back to the top"
    );
}
