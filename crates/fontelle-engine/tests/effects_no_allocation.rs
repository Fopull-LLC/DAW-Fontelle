//! INVARIANT 1 for the insert chain: **no effect allocates in `process`**.
//!
//! `no_allocation_during_render.rs` holds the instrument's path. This holds
//! the mixer's, and it is driven from `EffectKind::ALL` so an effect added
//! later is covered the day it appears — which is how the pitch corrector,
//! the first insert with rings the size of a second, came to be covered
//! (`docs/tune-plan.md` §9.5).
//!
//! Its own binary, because installing [`RtGuardAllocator`] is a process-wide
//! decision and every `tests/*.rs` file is a process of its own.

use fontelle_engine::{AudioNode, EffectNode, PrepareContext, RtGuardAllocator};
use fontelle_types::{EffectConfig, EffectKind};

mod common;
use common::process;

#[global_allocator]
static ALLOCATOR: RtGuardAllocator = RtGuardAllocator;

const SR: f32 = 48_000.0;
const BLOCK: usize = 128;

#[test]
fn no_insert_allocates_while_it_is_running() {
    for kind in EffectKind::ALL {
        let mut node = EffectNode::new(EffectConfig::new(kind));
        // Off-RT: this is where a line two seconds long is allowed to be
        // reached for, and it is the whole reason `prepare` exists.
        node.prepare(&PrepareContext {
            sample_rate: SR,
            max_block_size: BLOCK as u32,
        });

        let mut left = vec![0.0f32; BLOCK];
        let mut right = vec![0.0f32; BLOCK];

        fontelle_engine::mark_current_thread_rt();
        // Enough blocks that every ring wraps at least once and every
        // control-rate decision has been taken more than once — a tuner that
        // reached for memory only on its first voiced hop would pass one
        // block and fail a song.
        for block in 0..64 {
            for (index, sample) in left.iter_mut().enumerate() {
                let at = (block * BLOCK + index) as f32;
                *sample = (std::f32::consts::TAU * 220.0 * at / SR).sin() * 0.5;
            }
            right.copy_from_slice(&left);
            process(&mut node, &mut [&mut left, &mut right]);
        }
        node.reset();
        fontelle_engine::unmark_current_thread_rt();
    }
}
