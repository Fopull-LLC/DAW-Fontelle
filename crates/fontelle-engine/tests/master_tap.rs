//! What a Master insert's window reads (Ty, 2026-11-12: *"loudness readout
//! on master would be good"*): the node writes its reading to a tap the
//! window holds, with relaxed atomics only (INVARIANT 1), and the tap
//! outlives a graph rebuild because the session keeps it.

use std::sync::Arc;

use fontelle_engine::{AudioNode, EffectNode, MasterTap, PrepareContext};
use fontelle_types::{EffectConfig, MasterConfig};

mod common;
use common::process;

const SR: f32 = 48_000.0;
const BLOCK: usize = 512;

#[test]
fn a_master_insert_writes_its_reading_to_the_tap() {
    let tap = Arc::new(MasterTap::new());
    let fresh = tap.read();
    assert!(
        fresh.loudness_lufs <= -60.0,
        "an unplayed tap reads silence"
    );

    let mut node = EffectNode::new(EffectConfig::Master(MasterConfig::new()))
        .with_master_tap(Arc::clone(&tap));
    node.prepare(&PrepareContext {
        sample_rate: SR,
        max_block_size: BLOCK as u32,
    });
    // Four seconds of a quiet tone: the leveler lifts it, and says so.
    for block in 0..(4 * 48_000 / BLOCK) {
        let mut left: Vec<f32> = (0..BLOCK)
            .map(|i| {
                let n = (block * BLOCK + i) as f32;
                0.03 * (std::f32::consts::TAU * 220.0 * n / SR).sin()
            })
            .collect();
        let mut right = left.clone();
        process(&mut node, &mut [&mut left, &mut right]);
    }
    let reading = tap.read();
    assert!(reading.loudness_lufs > -40.0, "{}", reading.loudness_lufs);
    assert!(reading.level_db > 3.0, "{}", reading.level_db);
    assert!(reading.reduction_db >= 0.0);
}

#[test]
fn an_insert_that_is_not_a_master_leaves_the_tap_alone() {
    let tap = Arc::new(MasterTap::new());
    let mut node = EffectNode::new(EffectConfig::Width(fontelle_types::WidthConfig::new()))
        .with_master_tap(Arc::clone(&tap));
    node.prepare(&PrepareContext {
        sample_rate: SR,
        max_block_size: BLOCK as u32,
    });
    let mut left = vec![0.5f32; BLOCK];
    let mut right = left.clone();
    process(&mut node, &mut [&mut left, &mut right]);
    assert!(tap.read().loudness_lufs <= -60.0);
}
