//! Delay compensation: lining up paths that do not cost the same (TDD §5.5).
//!
//! An insert that looks ahead delays the track it is on. Two tracks summing
//! into one bus, one of them delayed, is a mix where the drums arrive at two
//! different times — and it is the kind of error nobody hears as an error:
//! it sounds like a loose player, or a smeared transient.
//!
//! [`DelayNode`] is the aligner. It is not an effect — there is no feedback,
//! no mix, and nothing to set: it is a fixed number of samples of nothing,
//! put on the path that arrives early so both arrive together.

use fontelle_engine::{AudioNode, DelayNode, PrepareContext};

mod common;
use common::process;

const SR: f32 = 48_000.0;
const BLOCK: usize = 128;

fn node(samples: u32) -> DelayNode {
    let mut node = DelayNode::new(samples);
    node.prepare(&PrepareContext {
        sample_rate: SR,
        max_block_size: BLOCK as u32,
    });
    node
}

/// One impulse in, one impulse out, `samples` later.
#[test]
fn a_delay_node_holds_the_signal_back_by_exactly_what_it_was_told() {
    let mut node = node(10);
    let mut left = vec![0.0f32; BLOCK];
    let mut right = vec![0.0f32; BLOCK];
    left[0] = 1.0;
    right[0] = 1.0;
    process(&mut node, &mut [&mut left, &mut right]);

    let hit = left.iter().position(|s| *s > 0.5);
    assert_eq!(hit, Some(10), "{hit:?}");
    assert_eq!(right.iter().position(|s| *s > 0.5), Some(10));
    assert_eq!(
        left.iter().filter(|s| s.abs() > 1e-9).count(),
        1,
        "one impulse in, one out"
    );
}

/// Zero is a wire, and costs nothing: the common case is a track that needs
/// no compensation at all.
#[test]
fn a_delay_of_nothing_is_a_wire() {
    let mut node = node(0);
    let mut left = vec![0.0f32; BLOCK];
    left[3] = 1.0;
    let mut right = vec![0.0f32; BLOCK];
    process(&mut node, &mut [&mut left, &mut right]);
    assert_eq!(left.iter().position(|s| *s > 0.5), Some(3));
}

/// The whole point is a delay longer than a block: five milliseconds is 240
/// samples and a block is 128, so what it holds has to survive two block
/// boundaries.
#[test]
fn a_delay_longer_than_a_block_carries_across_blocks() {
    let mut node = node(200);
    let mut blocks: Vec<Vec<f32>> = (0..4).map(|_| vec![0.0f32; BLOCK]).collect();
    blocks[0][5] = 1.0;

    let mut hits = Vec::new();
    for (index, block) in blocks.iter_mut().enumerate() {
        let mut right = vec![0.0f32; BLOCK];
        process(&mut node, &mut [block, &mut right]);
        if let Some(at) = block.iter().position(|s| *s > 0.5) {
            hits.push(index * BLOCK + at);
        }
    }
    assert_eq!(hits, vec![205], "one impulse, 200 samples late");
}

/// It reports what it costs, like every other node — a compensator that lied
/// about its own latency would be compensated for in turn.
#[test]
fn a_delay_node_reports_its_own_latency() {
    assert_eq!(node(240).latency_samples(), 240);
    assert_eq!(node(0).latency_samples(), 0);
}

/// A stop empties it. What it was holding belongs to the music that was
/// playing, and playing it over whatever comes next is the same mistake a
/// reverb tail across a seek would be.
#[test]
fn a_reset_empties_the_line() {
    let mut node = node(50);
    let mut left = vec![0.0f32; BLOCK];
    let mut right = vec![0.0f32; BLOCK];
    left[0] = 1.0;
    process(&mut node, &mut [&mut left, &mut right]);
    assert!(left.iter().any(|s| *s > 0.5));

    node.reset();
    let mut left = vec![0.0f32; BLOCK];
    let mut right = vec![0.0f32; BLOCK];
    process(&mut node, &mut [&mut left, &mut right]);
    assert!(
        left.iter().all(|s| s.abs() < 1e-9),
        "the line still held the impulse"
    );
}
