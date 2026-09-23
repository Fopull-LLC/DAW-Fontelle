//! The song's position as the **audio thread** is handed it
//! (`docs/disgusting-beat-plan.md` §3.4).
//!
//! `fontelle-sequencer/tests/tempo_table.rs` proves the timeline can answer
//! the question exactly. This proves the answer survives the trip: what a
//! node reads off `TransportSnapshot` has to be a straight line in the
//! samples, with no tread at any block boundary, because a node that turns
//! the song's position into a *read position* hears every tread as a jump.
//!
//! This is the regression test for DisgustingBeat's first release, which
//! buzzed at the block rate on every sloped curve anybody drew. The DSP was
//! right and the number it was handed was rounded: at 120 bpm a tick is
//! twenty-five samples, so the phase snapped back by up to a tick every
//! block, which on a half-speed slope is a twelve-sample jump in the read
//! position three hundred and seventy-five times a second.

use fontelle_engine::{BLOCK_SIZE, Transport, TransportReader, TransportState, timeline_channel};
use fontelle_types::CompiledTimeline;

fn timeline() -> CompiledTimeline {
    CompiledTimeline {
        events: Vec::new(),
        index: Vec::new(),
        tempo: Vec::new(),
        audio: Vec::new(),
        ..Default::default()
    }
}

#[test]
fn the_tick_the_audio_thread_reads_is_a_straight_line() {
    let (_publisher, mut source) = timeline_channel(timeline());
    let transport = Transport::new();
    transport.set_state(TransportState::Playing);
    let mut reader = TransportReader::new();

    let mut expected: Option<f64> = None;
    // Two seconds of blocks — plenty of boundaries for a tread to show at.
    for block in 0..(48_000 * 2 / BLOCK_SIZE) {
        let step = reader.next_step(&transport, source.current(), BLOCK_SIZE, BLOCK_SIZE, false);
        let snapshot = step.snapshot;
        let tick = snapshot.position_tick;
        if let Some(want) = expected {
            assert!(
                (tick - want).abs() < 1e-6,
                "block {block}: the tick stepped by {} at the boundary \u{2014} \
                 the audio thread was handed {tick} where the song is at {want}",
                tick - want
            );
        }
        // Where the song will be at the first sample of the next block.
        expected = Some(tick + snapshot.ticks_per_sample * step.frames as f64);
    }
}

#[test]
fn the_tick_is_where_the_song_is_and_not_merely_which_tick() {
    // A block is 128 samples and a tick is 25 of them, so a block boundary
    // lands inside a tick five times out of six. The one that matters is the
    // fraction: rounding it away is the whole bug.
    let (_publisher, mut source) = timeline_channel(timeline());
    let transport = Transport::new();
    transport.set_state(TransportState::Playing);
    let mut reader = TransportReader::new();

    let mut fractional = 0;
    for _ in 0..24 {
        let step = reader.next_step(&transport, source.current(), BLOCK_SIZE, BLOCK_SIZE, false);
        let tick = step.snapshot.position_tick;
        if (tick - tick.round()).abs() > 1e-9 {
            fractional += 1;
        }
    }
    assert!(
        fractional >= 16,
        "only {fractional} of twenty-four blocks began inside a tick; \
         the snapshot is rounding the song's position"
    );
}
