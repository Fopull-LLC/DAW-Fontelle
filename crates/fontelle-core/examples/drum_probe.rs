//! Renders every kit's kick, snare, tom and hat to WAVs so somebody can
//! **listen** to them, and prints the three numbers the pairwise test reads.
//!
//! ```text
//! cargo run --release -p fontelle-core --example drum_probe -- /tmp/kits
//! ```
use fontelle_dsp::{DrumSynth, DrumVoice};

const SR: f32 = 48_000.0;

fn render(voice: &DrumVoice, seconds: f32) -> Vec<f32> {
    let mut synth = DrumSynth::new();
    synth.trigger(voice, SR);
    (0..(seconds * SR) as usize)
        .map(|_| synth.next_sample(voice, SR))
        .collect()
}

fn centroid(s: &[f32]) -> f32 {
    let mut num = 0.0f64;
    let mut den = 0.0f64;
    let mut hz = 40.0f32;
    while hz < 16_000.0 {
        let (mut re, mut im) = (0.0f32, 0.0f32);
        let w = std::f32::consts::TAU * hz / SR;
        for (i, x) in s.iter().enumerate() {
            let a = w * i as f32;
            re += x * a.cos();
            im -= x * a.sin();
        }
        let mag = ((re * re + im * im).sqrt()) as f64;
        num += mag * hz as f64;
        den += mag;
        hz *= 1.06;
    }
    if den <= 0.0 { 0.0 } else { (num / den) as f32 }
}

fn main() -> std::io::Result<()> {
    let out = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "/tmp/kits".into());
    let out = std::path::PathBuf::from(out);
    std::fs::create_dir_all(&out)?;
    for style in fontelle_core::DrumKitStyle::ALL {
        let patch = fontelle_core::drum_kit(style);
        let mut whole: Vec<f32> = Vec::new();
        for layer in &patch.layers {
            let fontelle_core::Source::Drum(voice) = &layer.source else {
                continue;
            };
            let hit = render(voice, 1.2);
            let peak = hit.iter().fold(0.0f32, |m, s| m.max(s.abs()));
            println!(
                "{:<16} {:<10} peak {peak:.3} centroid {:>6.0} Hz  modes {:.2} tail {:.2}",
                style.label(),
                voice.model.label(),
                centroid(&hit[..(0.35 * SR) as usize]),
                voice.modes,
                voice.tail,
            );
            whole.extend_from_slice(&hit);
            whole.extend(std::iter::repeat_n(0.0, (0.15 * SR) as usize));
        }
        let name = style.label().to_lowercase().replace([' ', '/'], "-");
        write_wav16(&out.join(format!("{name}.wav")), &whole)?;
    }
    println!("written under {}", out.display());
    Ok(())
}

fn write_wav16(path: &std::path::Path, samples: &[f32]) -> std::io::Result<()> {
    let mut bytes = Vec::with_capacity(44 + samples.len() * 2);
    let data = samples.len() as u32 * 2;
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&(SR as u32).to_le_bytes());
    bytes.extend_from_slice(&(SR as u32 * 2).to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data.to_le_bytes());
    for s in samples {
        let v = (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    std::fs::write(path, bytes)
}
