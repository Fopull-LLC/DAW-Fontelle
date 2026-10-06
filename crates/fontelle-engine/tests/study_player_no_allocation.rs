//! INVARIANT 1 for Analyze Musically's preview player: playing, seeking,
//! looping, switching A/B and taking new audio in **never** allocate or free
//! on the audio thread. New audio is handed over through a slot and what it
//! replaces is handed back for the window's thread to free.
//!
//! Its own binary: the guard allocator is a process-wide decision.

use std::sync::Arc;

use fontelle_engine::{
    AudioNode, PrepareContext, RtGuardAllocator, StudyAudio, StudyPlayer, StudyPlayerNode,
};

mod common;
use common::process;

#[global_allocator]
static ALLOCATOR: RtGuardAllocator = RtGuardAllocator;

fn audio(frames: usize, scale: f32) -> StudyAudio {
    let ramp: Arc<[f32]> = (0..frames * 2)
        .map(|i| i as f32 * scale)
        .collect::<Vec<_>>()
        .into();
    StudyAudio {
        channels: 2,
        sample_rate: 44_100,
        original: Arc::clone(&ramp),
        edited: ramp,
    }
}

#[test]
fn the_player_never_allocates_on_the_audio_thread() {
    let player = StudyPlayer::new();
    player.submit(audio(88_200, 1e-6));
    let mut node = StudyPlayerNode::new(Arc::clone(&player));
    node.prepare(&PrepareContext {
        sample_rate: 48_000.0,
        max_block_size: 128,
    });
    let mut left = vec![0.0f32; 128];
    let mut right = vec![0.0f32; 128];
    let mut fresh = Some(audio(88_200, 2e-6));
    for step in 0..400 {
        // The window's thread, between callbacks.
        match step {
            10 => player.play(1_000, None, false),
            50 => player.set_original(true),
            80 => player.set_original(false),
            100 => player.play(40_000, Some(41_000), true),
            150 => player.submit(fresh.take().expect("once")),
            200 => player.reclaim(),
            250 => player.play(80_000, None, false),
            300 => player.stop(),
            _ => {}
        }
        fontelle_engine::mark_current_thread_rt();
        process(&mut node, &mut [&mut left, &mut right]);
        let _ = player.position();
        fontelle_engine::unmark_current_thread_rt();
    }
    player.reclaim();
}
