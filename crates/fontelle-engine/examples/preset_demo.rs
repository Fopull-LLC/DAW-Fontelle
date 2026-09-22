//! Renders a named preset shelf to one WAV, so somebody can hear it.
//!
//! Each preset gets a short phrase — a held chord, then a four-note run — and
//! is played **through its own effects chain**, which `fontelle-core` cannot do
//! (INVARIANT 4). `preset_probe` measures the bank; this is the other column,
//! the one only a person can fill in.
//!
//! ```text
//! cargo run --release -p fontelle-engine --example preset_demo -- "Sync & FM" /tmp/out.wav
//! ```
use fontelle_core::{SampleStore, Sampler};
use fontelle_engine::{
    AudioNode, PrepareContext, ProcessContext, SamplerNode, TransportSnapshot, TransportState,
};
use fontelle_types::{EventPayload, NodeId, TimedEvent};
use std::sync::Arc;

const SR: f32 = 48_000.0;
const BLOCK: usize = 512;

fn ev(sample: i64, key: u8, on: bool) -> TimedEvent {
    TimedEvent {
        sample,
        target: NodeId::default(),
        payload: if on {
            EventPayload::NoteOn {
                key,
                velocity: 104,
                pan: 0,
                fine_pitch: 0,
                release: 0,
                mod_x: 90,
                mod_y: 70,
                voice_context: 0,
            }
        } else {
            EventPayload::NoteOff {
                key,
                voice_context: 0,
            }
        },
    }
}

fn main() -> std::io::Result<()> {
    let shelf = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "Sync & FM".into());
    let out = std::env::args()
        .nth(2)
        .unwrap_or_else(|| "/tmp/demo.wav".into());
    let mut stereo: Vec<f32> = Vec::new();

    for row in fontelle_core::flopsynth::presets::FACTORY {
        if row.category.label() != shelf {
            continue;
        }
        let s = |t: f32| (t * SR) as i64;
        let mut events = Vec::new();
        // A chord, held, then a run — enough to hear the motion and the tail.
        for key in [48u8, 55, 60, 64] {
            events.push(ev(0, key, true));
            events.push(ev(s(1.6), key, false));
        }
        for (i, key) in [67u8, 72, 70, 75].into_iter().enumerate() {
            let at = s(1.7 + i as f32 * 0.28);
            events.push(ev(at, key, true));
            events.push(ev(at + s(0.26), key, false));
        }
        let total = s(4.2) as usize;

        let store = Arc::new(SampleStore::new());
        let mut node = SamplerNode::new(Sampler::new((row.build)()), store);
        node.prepare(&PrepareContext {
            sample_rate: SR,
            max_block_size: BLOCK as u32,
        });
        for block in 0..total.div_ceil(BLOCK) {
            let at = (block * BLOCK) as i64;
            let (mut l, mut r) = (vec![0.0f32; BLOCK], vec![0.0f32; BLOCK]);
            let here: Vec<TimedEvent> = events
                .iter()
                .filter(|e| e.sample >= at && e.sample < at + BLOCK as i64)
                .cloned()
                .collect();
            {
                let (a, b) = (&mut l[..], &mut r[..]);
                let mut outs: [&mut [f32]; 2] = [a, b];
                let mut ctx = ProcessContext {
                    inputs: &[],
                    outputs: &mut outs,
                    all_events: &here,
                    live_events: &[],
                    audio: &[],
                    node: NodeId::default(),
                    transport: TransportSnapshot {
                        state: TransportState::Playing,
                        position_sample: at,
                        bpm: 120.0,
                        ..Default::default()
                    },
                    sample_range: at..at + BLOCK as i64,
                };
                node.process(&mut ctx);
            }
            for (a, b) in l.iter().zip(&r) {
                stereo.push(*a);
                stereo.push(*b);
            }
        }
        stereo.extend(std::iter::repeat_n(0.0, (0.3 * SR) as usize * 2));
        println!("  {}", row.name);
    }

    let data = stereo.len() as u32 * 2;
    let mut b = Vec::with_capacity(44 + data as usize);
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&(SR as u32).to_le_bytes());
    b.extend_from_slice(&(SR as u32 * 4).to_le_bytes());
    b.extend_from_slice(&4u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data.to_le_bytes());
    for s in &stereo {
        b.extend_from_slice(&((s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16).to_le_bytes());
    }
    std::fs::write(&out, b)?;
    println!("written to {out}");
    Ok(())
}
