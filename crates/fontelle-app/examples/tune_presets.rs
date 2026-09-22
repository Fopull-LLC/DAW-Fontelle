//! Phase 4's ear test (`docs/tune-plan.md` §12): a vocal through **every one
//! of the sixteen presets**, written out as WAVs so somebody can listen to
//! them before a pixel of the console is drawn.
//!
//! `fontelle-fx/tests/tune.rs` can measure that the three archetypes come out
//! different — `the_three_archetypes_measure_differently` is what the recipes
//! were tuned *to* — but no test can hear whether Trap Robot sounds like trap
//! or whether Pop Polish is doing anything at all. That is what this is for,
//! and it is the gate the plan puts between phase 4 and the window.
//!
//! ```text
//! cargo run --release -p fontelle-app --example tune_presets -- /tmp/tune-presets
//! cargo run --release -p fontelle-app --example tune_presets -- /tmp/tune-presets vocal.wav
//! ```
//!
//! **Reading the `pulls` column.** It is how much of the way from what was
//! sung to the scale note the output actually went, averaged over the voiced
//! hops of the second half, with `transpose`/`detune` taken off first. What
//! it is for is the *ordering* — Transparent loosest, the hard ones at a full
//! hundred — rather than the absolute figure: the source has a natural
//! vibrato and the target is a fixed note, so a hop-by-hop ratio swings past
//! 100 % whenever the vibrato is on the far side of the target. The number
//! that gates anything is `fontelle-fx`'s
//! `the_three_archetypes_measure_differently`, not this one.
//!
//! With no input file it synthesises a vowel that glides *off* the note — a
//! singer sliding between two scale degrees and sitting flat on the second —
//! which is the signal a corrector has something to do to. `source.wav` is
//! written beside the rest so the two can be compared.

use fontelle_fx::{NoteInput, Tune};
use fontelle_types::{TuneConfig, TunePreset};

const RATE: f32 = 48_000.0;
const BLOCK: usize = 128;

fn main() -> std::io::Result<()> {
    let mut args = std::env::args().skip(1);
    let out = args
        .next()
        .unwrap_or_else(|| "/tmp/tune-presets".to_string());
    let out = std::path::PathBuf::from(out);
    std::fs::create_dir_all(&out)?;

    let source = match args.next() {
        Some(path) => read_wav16(std::path::Path::new(&path))?,
        None => sliding_vowel(),
    };
    fontelle_app::write_wav16(&out.join("source.wav"), &source, 1, RATE as u32)?;
    println!("source: {:.1} s", source.len() as f32 / RATE);

    for preset in TunePreset::ALL {
        let config = TuneConfig::from_preset(preset);
        // The two MIDI presets are the ones with nothing to listen to unless
        // a key is held: their `control` is a MIDI mode, so with no source
        // they fall back to the scale. A held A gives them the melody they
        // were made for and makes the file worth opening.
        let notes = NoteInput {
            last: Some(69),
            mask: 1 << 9,
            bend_cents: 0.0,
            ons: 0,
        };
        let (corrected, trace) = run(&source, &config, notes);
        let name = preset.label().to_lowercase().replace(' ', "-");
        let clipped = fontelle_app::write_wav16(
            &out.join(format!("{name}.wav")),
            &corrected,
            1,
            RATE as u32,
        )?;
        // **How much of the way to the target each preset actually pulls**,
        // over the second half of the take — `fontelle-fx`'s own
        // `the_three_archetypes_measure_differently` metric, and the only one
        // of these numbers worth printing.
        //
        // The obvious measure — mean absolute difference from the source —
        // reads the same 0.03 for all sixteen, because PSOLA resynthesises
        // the waveform at *every* ratio including 1 and the phase difference
        // swamps the pitch difference. A number that cannot tell Transparent
        // from Hard Tune is worse than no number: it says the presets are
        // identical when what is identical is the measure.
        let shift = f32::from(config.transpose) * 100.0 + config.detune_cents;
        let closed: Vec<f32> = trace
            .iter()
            .skip(trace.len() / 2)
            .filter(|f| f.flags & fontelle_types::TUNE_VOICED != 0)
            .map(|f| {
                // The **shift comes off first**. `transpose` and `detune` are
                // added after the correction (§4.5), so a preset an octave
                // down puts `out` twelve semitones from `sung` while the
                // correction it is being asked about is worth a few cents:
                // left in, Octave Under reads −4965 % and Chipmunk +5263 %,
                // which is the measure describing the transposer and not the
                // corrector.
                let corrected = f.out_cents - shift;
                let distance = f.target_cents - f.sung_cents;
                if distance.abs() < 1.0 {
                    1.0
                } else {
                    (corrected - f.sung_cents) / distance
                }
            })
            .collect();
        let pull = if closed.is_empty() {
            f32::NAN
        } else {
            closed.iter().sum::<f32>() / closed.len() as f32
        };
        // **Level and peak**, which are what decide whether a bank is usable.
        // Switching presets should change the *sound* and not the volume, so
        // the level column has to stay near zero; the peak column is the one
        // that says whether it will clip on the way out.
        let rms = |s: &[f32]| (s.iter().map(|x| x * x).sum::<f32>() / s.len().max(1) as f32).sqrt();
        let db = |x: f32| 20.0 * x.max(1e-12).log10();
        let level = db(rms(&corrected)) - db(rms(&source));
        let peak = corrected.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        println!(
            "{:<16} {:<7} retune {:>6.1} ms  shift {:>+5.0} \u{a2}  pulls {:>4.0} %  level {:>+5.1} dB  peak {peak:>4.2}  {clipped} clipped",
            preset.label(),
            config.engine.label(),
            config.retune_ms,
            shift,
            pull * 100.0,
            level,
        );
    }
    println!("written under {}", out.display());
    Ok(())
}

/// The whole signal through one corrector, a block at a time, the way the
/// engine feeds it.
fn run(
    source: &[f32],
    config: &TuneConfig,
    notes: NoteInput,
) -> (Vec<f32>, Vec<fontelle_types::TuneFrame>) {
    let mut tune = Tune::new();
    tune.prepare(RATE, config);
    let mut out = Vec::with_capacity(source.len());
    let mut trace = Vec::new();
    let mut scratch = vec![0.0f32; BLOCK];
    for chunk in source.chunks(BLOCK) {
        let frames = chunk.len();
        scratch[..frames].copy_from_slice(chunk);
        {
            let (head, _) = scratch.split_at_mut(frames);
            let mut channels: [&mut [f32]; 1] = [head];
            tune.process(&mut channels, notes, config, 120.0);
        }
        out.extend_from_slice(&scratch[..frames]);
        trace.extend(tune.trace().iter().copied());
    }
    (out, trace)
}

/// Four seconds of a vowel that slides between two notes and sits flat on the
/// second — the thing a corrector exists to catch.
///
/// Deliberately *not* [`shift_a_fifth`](../shift_a_fifth.rs)'s gliding vowel,
/// which wanders continuously: a corrector given a continuous glide has no
/// note to snap to and every preset sounds like the same smear. This one holds
/// A3 (220 Hz), slides up over a beat, holds 35 cents flat of C#4, and takes a
/// breath in the middle so the unvoiced path is exercised too.
fn sliding_vowel() -> Vec<f32> {
    let n = (RATE * 4.0) as usize;
    let formants = [700.0f32, 1_200.0, 2_600.0];
    let mut phase = 0.0f32;
    let mut state = 0x1234_5678u32;
    let a3 = 220.0f32;
    // C#4, thirty-five cents flat: near enough that a slow retune leaves it
    // there and a fast one snaps it, which is the difference the presets are
    // made of.
    let cs4 = 277.18 * 2.0f32.powf(-35.0 / 1200.0);
    (0..n)
        .map(|i| {
            let t = i as f32 / RATE;
            if (2.4..2.6).contains(&t) {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                return ((state >> 8) as f32 / 8_388_608.0 - 1.0) * 0.08;
            }
            // Hold, slide, hold — with a slow natural vibrato over all of it,
            // because a corrector's `natural_vibrato` knob has nothing to keep
            // if the singer has none.
            let base = if t < 1.4 {
                a3
            } else if t < 1.9 {
                let k = (t - 1.4) / 0.5;
                a3 * (cs4 / a3).powf(k)
            } else {
                cs4
            };
            let f0 = base * (1.0 + 0.012 * (std::f32::consts::TAU * 5.2 * t).sin());
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

/// A 16-bit WAV, mixed to mono. The same reader
/// [`shift_a_fifth`](../shift_a_fifth.rs) uses, and for the same reason: an
/// example that needed a decoder crate would be an example nobody runs.
fn read_wav16(path: &std::path::Path) -> std::io::Result<Vec<f32>> {
    let bytes = std::fs::read(path)?;
    let bad = |what: &str| std::io::Error::new(std::io::ErrorKind::InvalidData, what.to_string());
    if bytes.len() < 44 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(bad("not a RIFF/WAVE file"));
    }
    let mut channels = 1usize;
    let mut at = 12usize;
    while at + 8 <= bytes.len() {
        let id = &bytes[at..at + 4];
        let size = u32::from_le_bytes([bytes[at + 4], bytes[at + 5], bytes[at + 6], bytes[at + 7]])
            as usize;
        if id == b"fmt " && at + 12 <= bytes.len() {
            channels = u16::from_le_bytes([bytes[at + 10], bytes[at + 11]]).max(1) as usize;
        }
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
    Err(bad("no data chunk"))
}
