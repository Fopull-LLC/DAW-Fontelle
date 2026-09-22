//! Cuts the factory kits' recordings out of the drum machine.
//!
//! > *"use flopsynths new sampling features to make a variety of new complex
//! > presets that can be experimental, synthy, modulating, instruments,
//! > percussion kits, growls, dubstep sounds"* — Ty, 2026-09-16
//!
//! A Flopsynth preset that is a *kit* needs one recording per key, and a
//! preset that turns one hit into an instrument needs that hit as a
//! recording it can lock to. The drum machine already has every General
//! MIDI hit in twenty-two styles, played by voices this program owns — so
//! the bank's kits are cut from it rather than found: every slot of the
//! Studio and 808 kits, played once through the kit's own bus (the EQ and
//! compressor a kit has, `drum_kit`'s chain — rendered through
//! `SamplerNode`, which is what runs a patch's chain), trimmed to the hit,
//! faded at its end, and written at 32 kHz mono:
//!
//! ```text
//! cargo run -p fontelle-app --example kit_samples --release -- \
//!     assets/flopsynth/samples/kit
//! ```
//!
//! One file per hit, named as the roll names the row (`Kick.wav`,
//! `Tom-Mid-2.wav`), under `studio/` and `808/`. Compiled in by
//! `fontelle_core::factory_samples`, whose list of files this tool's output
//! must match.

use std::path::Path;
use std::sync::Arc;

use fontelle_core::{DrumKitStyle, GM_DRUM_MAP, drum_kit};
use fontelle_core::{SampleStore, Sampler};
use fontelle_engine::{
    AudioNode, PrepareContext, ProcessContext, SamplerNode, TransportSnapshot, TransportState,
};
use fontelle_types::{EventPayload, NodeId, TimedEvent};

const SR: f32 = 48_000.0;
const BLOCK: usize = 512;
/// Every hit is kept at one rate: a hat's top is under this Nyquist and a
/// kick has nothing to lose.
const RATE: u32 = 32_000;
/// The most a hit is kept for. A crash rings longer; what is kept is where
/// it has fallen far enough that the preset's own release takes the rest.
const LONGEST_S: f32 = 2.5;

/// The file name a hit gets: its roll name with the spaces made safe for
/// an `include_bytes!` path.
pub fn file_name(name: &str) -> String {
    format!("{}.wav", name.replace(' ', "-"))
}

/// One hit through the kit's bus: `key` at full velocity, rendered for
/// `seconds`, as a mono sum.
fn render(patch: &fontelle_core::Patch, key: u8, seconds: f32) -> Vec<f32> {
    let store = Arc::new(SampleStore::new());
    let mut node = SamplerNode::new(Sampler::new(patch.clone()), store);
    node.prepare(&PrepareContext {
        sample_rate: SR,
        max_block_size: BLOCK as u32,
    });
    let blocks = (SR * seconds) as usize / BLOCK;
    let mut out = Vec::with_capacity(blocks * BLOCK);
    for block in 0..blocks {
        let at = (block * BLOCK) as i64;
        let mut events: Vec<TimedEvent> = Vec::new();
        if block == 0 {
            events.push(TimedEvent {
                sample: 0,
                target: NodeId::default(),
                payload: EventPayload::NoteOn {
                    key,
                    velocity: 127,
                    pan: 0,
                    fine_pitch: 0,
                    release: 0,
                    mod_x: 0,
                    mod_y: 0,
                    voice_context: 0,
                },
            });
        }
        // A drum machine is played with the shortest of notes: the hit's
        // own decay is what is heard, and the patch's release is long.
        if block == 1 {
            events.push(TimedEvent {
                sample: at,
                target: NodeId::default(),
                payload: EventPayload::NoteOff {
                    key,
                    voice_context: 0,
                },
            });
        }
        let mut left = vec![0.0f32; BLOCK];
        let mut right = vec![0.0f32; BLOCK];
        {
            let (l, r) = (&mut left[..], &mut right[..]);
            let mut outputs: [&mut [f32]; 2] = [l, r];
            let mut ctx = ProcessContext {
                inputs: &[],
                outputs: &mut outputs,
                all_events: &events,
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
        out.extend(left.iter().zip(&right).map(|(a, b)| (a + b) * 0.5));
    }
    out
}

/// Windowed-sinc resampling from `SR` to `rate`, the low-pass a little
/// under the new Nyquist so nothing folds. `grand_samples`' resampler.
fn resample(input: &[f32], rate: u32) -> Vec<f32> {
    let ratio = SR / rate as f32;
    let cutoff = 0.45 / ratio;
    let half = 48i64;
    let out_len = (input.len() as f32 / ratio) as usize;
    (0..out_len)
        .map(|i| {
            let centre = i as f32 * ratio;
            let base = centre.floor() as i64;
            let mut sum = 0.0f32;
            for k in (base - half)..=(base + half) {
                if k < 0 || k >= input.len() as i64 {
                    continue;
                }
                let x = k as f32 - centre;
                let sinc = if x.abs() < 1e-6 {
                    2.0 * cutoff
                } else {
                    (std::f32::consts::TAU * cutoff * x).sin() / (std::f32::consts::PI * x)
                };
                let w = x / half as f32;
                let window = 0.42
                    + 0.5 * (std::f32::consts::PI * w).cos()
                    + 0.08 * (std::f32::consts::TAU * w).cos();
                sum += input[k as usize] * sinc * window;
            }
            sum
        })
        .collect()
}

/// The hit alone: from just before its onset to where it has fallen 60 dB
/// under its own peak (or `LONGEST_S`), with a short fade in and a
/// raised-cosine fade over the last 30 ms so the cut is never a click.
fn cut(raw: &[f32]) -> Vec<f32> {
    let peak = raw.iter().fold(0f32, |m, s| m.max(s.abs()));
    let onset = raw
        .iter()
        .position(|s| s.abs() > peak * 1e-3)
        .unwrap_or(0)
        .saturating_sub(48);
    // The end: the last 10 ms window whose RMS is within 60 dB of the peak.
    let window = 480;
    let floor = peak * 1e-3;
    let mut end = raw.len();
    for (index, chunk) in raw[onset..].chunks(window).enumerate().rev() {
        let rms = (chunk.iter().map(|s| s * s).sum::<f32>() / chunk.len() as f32).sqrt();
        if rms > floor {
            end = (onset + (index + 2) * window).min(raw.len());
            break;
        }
    }
    let end = end.min(onset + (SR * LONGEST_S) as usize);
    let mut note: Vec<f32> = raw[onset..end].to_vec();
    for (i, s) in note.iter_mut().take(24).enumerate() {
        *s *= i as f32 / 24.0;
    }
    let fade = ((SR * 0.03) as usize).min(note.len() / 2);
    let len = note.len();
    for (i, s) in note[len - fade..].iter_mut().enumerate() {
        let t = i as f32 / fade as f32;
        *s *= 0.5 + 0.5 * (std::f32::consts::PI * t).cos();
    }
    note
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let Some(out) = args.get(1) else {
        eprintln!("usage: kit_samples <out-dir>");
        std::process::exit(2);
    };
    let out = Path::new(out);
    for (folder, style) in [
        ("studio", DrumKitStyle::Studio),
        ("808", DrumKitStyle::EightOhEight),
    ] {
        let dir = out.join(folder);
        std::fs::create_dir_all(&dir).unwrap();
        let patch = drum_kit(style);
        // Every hit first, then one gain for the whole kit: the balance
        // between the hits is the kit's own.
        let mut cuts: Vec<(&str, Vec<f32>)> = Vec::new();
        for slot in GM_DRUM_MAP.iter() {
            let raw = render(&patch, slot.key, LONGEST_S + 0.5);
            let note = resample(&cut(&raw), RATE);
            println!(
                "  {folder} {:<12} {:.2}s peak {:.1} dB",
                slot.name,
                note.len() as f32 / RATE as f32,
                20.0 * note
                    .iter()
                    .fold(0f32, |m, s| m.max(s.abs()))
                    .max(1e-9)
                    .log10()
            );
            cuts.push((slot.name, note));
        }
        let loudest = cuts
            .iter()
            .flat_map(|(_, note)| note.iter().map(|s| s.abs()))
            .fold(0f32, f32::max);
        let gain = 0.95 / loudest.max(1e-9);
        for (name, note) in cuts {
            let path = dir.join(file_name(name));
            let scaled: Vec<f32> = note.iter().map(|s| s * gain).collect();
            let mut w = fontelle_assets::WavWriter::create(&path, RATE, 1).unwrap();
            w.write(&scaled).unwrap();
            w.finish().unwrap();
        }
        println!("{folder}: gain {:+.1} dB", 20.0 * gain.log10());
    }
}
