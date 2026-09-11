//! Phase 1's ear test (`docs/tune-plan.md` §12): a signal through
//! [`PsolaShifter`] at a fifth up, an octave down and a formant shift, written
//! out as WAVs so somebody can **listen** to them.
//!
//! Nothing in `fontelle-dsp/tests/psola.rs` can hear whether a shifted vowel
//! sounds like a voice — it can only measure that the peak moved. This is the
//! fourth column.
//!
//! ```text
//! cargo run --release -p fontelle-app --example shift_a_fifth -- /tmp/tune
//! cargo run --release -p fontelle-app --example shift_a_fifth -- /tmp/tune in.wav
//! ```
//!
//! With no input file it synthesises a vowel that glides, which is enough to
//! hear phasiness, clicks at grain boundaries and whether the formants moved.

use fontelle_dsp::{GrainEngine, PsolaShifter};

const RATE: f32 = 48_000.0;

fn main() -> std::io::Result<()> {
    let mut args = std::env::args().skip(1);
    let out = args.next().unwrap_or_else(|| "/tmp/tune".to_string());
    let out = std::path::PathBuf::from(out);
    std::fs::create_dir_all(&out)?;

    let source = match args.next() {
        Some(path) => read_wav16(std::path::Path::new(&path))?,
        None => gliding_vowel(),
    };
    fontelle_app::write_wav16(&out.join("source.wav"), &source, 1, RATE as u32)?;

    for (name, engine, ratio, formant, texture) in [
        (
            "fifth-up",
            GrainEngine::Smooth,
            2.0f32.powf(7.0 / 12.0),
            1.0,
            0.0,
        ),
        (
            "fifth-up-chipmunk",
            GrainEngine::Smooth,
            2.0f32.powf(7.0 / 12.0),
            2.0f32.powf(7.0 / 12.0),
            0.0,
        ),
        ("octave-down", GrainEngine::Smooth, 0.5, 1.0, 0.0),
        ("formant-only", GrainEngine::Smooth, 1.0, 1.35, 0.0),
        ("hard", GrainEngine::Hard, 2.0f32.powf(7.0 / 12.0), 1.0, 0.8),
        (
            "cheap",
            GrainEngine::Grain,
            2.0f32.powf(7.0 / 12.0),
            1.0,
            0.2,
        ),
        ("wire", GrainEngine::Smooth, 1.0, 1.0, 0.0),
    ] {
        let shifted = run(&source, engine, ratio, formant, texture);
        let clipped =
            fontelle_app::write_wav16(&out.join(format!("{name}.wav")), &shifted, 1, RATE as u32)?;
        println!("{name}: {} samples, {clipped} clipped", shifted.len());
    }
    println!("written under {}", out.display());
    Ok(())
}

/// The whole signal through one shifter, tracking its own pitch as the
/// corrector will.
fn run(source: &[f32], engine: GrainEngine, ratio: f32, formant: f32, texture: f32) -> Vec<f32> {
    let p_max = (RATE / 80.0).ceil() as u32;
    let block = 128usize;
    let mut tracker = fontelle_dsp::PitchTracker::new(80.0, 1_000.0, 64);
    tracker.prepare(RATE);
    let mut shifter = PsolaShifter::new(1);
    shifter.prepare(RATE, p_max, block as u32, 2 * p_max + 64);
    shifter.set_engine(engine, texture, 25.0);

    let mut period = RATE / 200.0;
    let mut out = Vec::with_capacity(source.len());
    let mut scratch = vec![0.0f32; block];
    for chunk in source.chunks(block) {
        let frames = chunk.len();
        scratch[..frames].copy_from_slice(chunk);
        let mut voiced = false;
        tracker.push(&scratch[..frames], &mut |frame| {
            if let Some(frame) = frame {
                period = RATE / frame.hz;
                voiced = true;
            }
        });
        shifter.set_period(period);
        shifter.set_ratio(if voiced { ratio } else { 1.0 });
        shifter.set_formant(formant);
        let (head, _) = scratch.split_at_mut(frames);
        let mut channels: [&mut [f32]; 1] = [head];
        shifter.process(&mut channels);
        out.extend_from_slice(&scratch[..frames]);
    }
    out
}

/// Four seconds of a vowel gliding a fourth and back, with a breath in the
/// middle — enough to hear a click, a phase smear and a formant that moved.
fn gliding_vowel() -> Vec<f32> {
    let n = (RATE * 4.0) as usize;
    let formants = [700.0f32, 1_200.0, 2_600.0];
    let mut phase = 0.0f32;
    let mut state = 0x1234_5678u32;
    (0..n)
        .map(|i| {
            let t = i as f32 / RATE;
            // A breath from 1.9 s to 2.1 s: unvoiced, and the shifter must
            // pass it through untouched.
            if (1.9..2.1).contains(&t) {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                return ((state >> 8) as f32 / 8_388_608.0 - 1.0) * 0.08;
            }
            let f0 = 160.0 * (1.0 + 0.15 * (std::f32::consts::TAU * 0.4 * t).sin());
            phase += std::f32::consts::TAU * f0 / RATE;
            let mut sum = 0.0;
            for k in 1..=40 {
                let hz = f0 * k as f32;
                if hz > RATE / 2.0 {
                    break;
                }
                let mut gain = 0.0;
                for f in formants {
                    let bw = f * 0.10;
                    gain += 1.0 / (1.0 + ((hz - f) / bw).powi(2));
                }
                sum += gain / k as f32 * (phase * k as f32).sin();
            }
            sum * 0.12
        })
        .collect()
}

/// Enough of a WAV reader to open a mono or stereo 16-bit file and sum it.
fn read_wav16(path: &std::path::Path) -> std::io::Result<Vec<f32>> {
    let bytes = std::fs::read(path)?;
    if bytes.len() < 44 || &bytes[0..4] != b"RIFF" {
        return Err(std::io::Error::other("not a RIFF file"));
    }
    let channels = u16::from_le_bytes([bytes[22], bytes[23]]).max(1) as usize;
    // The data chunk, found rather than assumed at 36: a file with a LIST
    // chunk in it is still a WAV.
    let mut at = 12;
    while at + 8 <= bytes.len() {
        let id = &bytes[at..at + 4];
        let size = u32::from_le_bytes([bytes[at + 4], bytes[at + 5], bytes[at + 6], bytes[at + 7]])
            as usize;
        if id == b"data" {
            let end = (at + 8 + size).min(bytes.len());
            let samples: Vec<f32> = bytes[at + 8..end]
                .as_chunks::<2>()
                .0
                .iter()
                .map(|b| i16::from_le_bytes(*b) as f32 / i16::MAX as f32)
                .collect();
            return Ok(samples
                .chunks(channels)
                .map(|frame| frame.iter().sum::<f32>() / channels as f32)
                .collect());
        }
        at += 8 + size + (size & 1);
    }
    Err(std::io::Error::other("no data chunk"))
}
