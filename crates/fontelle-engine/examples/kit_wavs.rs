//! Renders every kit **through its own bus** to a WAV, so somebody can listen.
//!
//! `fontelle-core`'s `drum_probe` renders the voices, which is what a kit was
//! before it had a chain; this renders what actually comes out of the
//! instrument. Two bars of a plain pattern per kit, then the six audition hits
//! on their own with room to hear each tail.
//!
//! ```text
//! cargo run --release -p fontelle-engine --example kit_wavs -- /tmp/kits
//! ```
use fontelle_core::{SampleStore, Sampler};
use fontelle_engine::{
    AudioNode, PrepareContext, ProcessContext, SamplerNode, TransportSnapshot, TransportState,
};
use fontelle_types::{EventPayload, NodeId, TimedEvent};
use std::sync::Arc;

const SR: f32 = 48_000.0;
const BLOCK: usize = 512;
/// A sixteenth at 120 bpm.
const STEP: usize = (SR as usize * 60) / (120 * 4);

fn note(sample: i64, key: u8, velocity: u8) -> TimedEvent {
    TimedEvent {
        sample,
        target: NodeId::default(),
        payload: EventPayload::NoteOn {
            key,
            velocity,
            pan: 0,
            fine_pitch: 0,
            release: 0,
            mod_x: 0,
            mod_y: 0,
            voice_context: 0,
        },
    }
}

fn main() -> std::io::Result<()> {
    let out = std::path::PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "/tmp/kits".into()),
    );
    std::fs::create_dir_all(&out)?;
    for style in fontelle_core::DrumKitStyle::ALL {
        let patch = fontelle_core::drum_kit(style);
        let slots = patch.fx.len();
        let mut events: Vec<TimedEvent> = Vec::new();
        for bar in 0..2 {
            for step in 0..16 {
                let at = ((bar * 16 + step) * STEP) as i64;
                if step % 4 == 0 {
                    events.push(note(at, 36, 118));
                }
                if step % 8 == 4 {
                    events.push(note(at, 38, 110));
                }
                events.push(note(at, if step % 4 == 2 { 46 } else { 42 }, 84));
            }
        }
        let after = (32 * STEP) as i64;
        for (i, key) in [36u8, 38, 42, 46, 43, 51].into_iter().enumerate() {
            events.push(note(after + (i as f32 * 0.75 * SR) as i64, key, 118));
        }
        let total = after as usize + (6.0 * SR) as usize;

        let store = Arc::new(SampleStore::new());
        let mut node = SamplerNode::new(Sampler::new(patch), store);
        node.prepare(&PrepareContext {
            sample_rate: SR,
            max_block_size: BLOCK as u32,
        });
        let mut stereo: Vec<f32> = Vec::with_capacity(total * 2);
        let mut peak = 0.0f32;
        for block in 0..total.div_ceil(BLOCK) {
            let at = (block * BLOCK) as i64;
            let mut left = vec![0.0f32; BLOCK];
            let mut right = vec![0.0f32; BLOCK];
            let here: Vec<TimedEvent> = events
                .iter()
                .filter(|e| e.sample >= at && e.sample < at + BLOCK as i64)
                .cloned()
                .collect();
            {
                let (l, r) = (&mut left[..], &mut right[..]);
                let mut outs: [&mut [f32]; 2] = [l, r];
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
            for (l, r) in left.iter().zip(&right) {
                peak = peak.max(l.abs()).max(r.abs());
                stereo.push(*l);
                stereo.push(*r);
            }
        }
        let name = style.label().to_lowercase().replace([' ', '/', '&'], "-");
        write_wav16(&out.join(format!("{name}.wav")), &stereo)?;
        println!("{:<14} peak {peak:.3}  {slots} fx", style.label());
    }
    println!("written under {}", out.display());
    Ok(())
}

fn write_wav16(path: &std::path::Path, samples: &[f32]) -> std::io::Result<()> {
    let data = samples.len() as u32 * 2;
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
    for s in samples {
        b.extend_from_slice(&((s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16).to_le_bytes());
    }
    std::fs::write(path, b)
}
