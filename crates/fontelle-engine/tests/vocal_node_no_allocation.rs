//! INVARIANT 1 for the vocal track's node: playing phrases, seeking,
//! crossing a crossfade and taking a newly published table **never**
//! allocate or free on the audio thread. A new table comes through a slot,
//! and the one it replaces goes back for the app's thread to free.
//!
//! Its own binary: the guard allocator is a process-wide decision.

use std::sync::Arc;

use fontelle_engine::{
    AudioNode, PrepareContext, ProcessContext, RtGuardAllocator, SungPhrase, TransportSnapshot,
    TransportState, VocalNode, VocalPlayer, VocalTable,
};

#[global_allocator]
static ALLOCATOR: RtGuardAllocator = RtGuardAllocator;

fn phrases(scale: f32) -> VocalTable {
    let phrase = |start: f64| SungPhrase {
        start,
        sample_rate: 24_000,
        samples: (0..36_000)
            .map(|i| (i as f32 * 0.01).sin() * scale)
            .collect::<Vec<_>>()
            .into(),
    };
    VocalTable::new((0..8).map(|i| phrase(i as f64 * 1.4)).collect())
}

#[test]
fn the_vocal_node_never_allocates_on_the_audio_thread() {
    let player = VocalPlayer::new();
    player.publish(phrases(0.5));
    let mut node = VocalNode::new(Arc::clone(&player)).on_channel(-3.0, 0.2);
    node.prepare(&PrepareContext {
        sample_rate: 48_000.0,
        max_block_size: 256,
    });
    let mut left = vec![0.0f32; 256];
    let mut right = vec![0.0f32; 256];
    let mut at: i64 = 0;
    let mut fresh = Some(phrases(0.25));
    for step in 0..2_000 {
        // The app's thread, between callbacks.
        match step {
            300 => player.publish(fresh.take().expect("once")),
            400 => player.reclaim(),
            500 => at = 200_000, // a seek
            900 => at = 0,       // a loop come round
            _ => {}
        }
        let state = if step % 700 < 20 {
            TransportState::Stopped
        } else {
            TransportState::Playing
        };
        fontelle_engine::mark_current_thread_rt();
        {
            let mut channels: [&mut [f32]; 2] = [&mut left, &mut right];
            let mut ctx = ProcessContext {
                inputs: &[],
                outputs: &mut channels,
                all_events: &[],
                live_events: &[],
                audio: &[],
                node: fontelle_types::NodeId::default(),
                transport: TransportSnapshot {
                    state,
                    position_sample: at,
                    ..Default::default()
                },
                sample_range: at..at + 256,
            };
            node.process(&mut ctx);
        }
        fontelle_engine::unmark_current_thread_rt();
        at += 256;
    }
    player.reclaim();
}
