//! "How well could notes be pulled out of this at all" (plan §2.6): a
//! number from 0 to 1 and a word, separate from the key's confidence.

use crate::transcribe::{NoteEvent, Posteriorgrams};

/// The measurements the confidence is built from, each 0..1 except the
/// signal-to-noise ratio.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExtractionEvidence {
    /// The mean, over notes, of each note's peak activation.
    pub posterior: f32,
    /// Of the frames with sound in them, the share a confident note
    /// explains.
    pub coverage: f32,
    /// Loud against quiet frames (95th to 10th percentile level), in dB.
    pub snr_db: f32,
    /// The median spectral flatness of the frames with sound in them: near
    /// zero for tones, high for noise and drums.
    pub flatness: f32,
}

/// Where the words change.
const CLEAR: f32 = 0.75;
const USABLE: f32 = 0.4;
/// A note this sure explains the frames it covers.
const CONFIDENT_NOTE: f32 = 0.35;
/// The flatness transform's size.
const FLATNESS_FFT: usize = 1024;

/// The three words the badge says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Clarity {
    Clear,
    Usable,
    RoughGuess,
}

impl Clarity {
    pub fn of(confidence: f32) -> Self {
        if confidence >= CLEAR {
            Self::Clear
        } else if confidence >= USABLE {
            Self::Usable
        } else {
            Self::RoughGuess
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Clear => "Clear",
            Self::Usable => "Usable",
            Self::RoughGuess => "Rough guess",
        }
    }
}

/// Measures `audio` (mono, any rate) against what was transcribed from it.
pub fn extraction_evidence(
    audio: &[f32],
    sample_rate: u32,
    post: &Posteriorgrams,
    notes: &[NoteEvent],
) -> ExtractionEvidence {
    use crate::transcribe::notes::{MIDI_OFFSET, N_KEYS};
    let frames = post.frames;
    let sr = f64::from(sample_rate.max(1));

    // Each model frame's level, over the 512 samples (at 22 050 Hz) round it.
    let half = (256.0 / 22_050.0 * sr) as isize;
    let rms: Vec<f32> = (0..frames)
        .map(|f| {
            let centre = (Posteriorgrams::frame_time(f) * sr) as isize;
            let (a, b) = (
                (centre - half).max(0) as usize,
                ((centre + half).max(0) as usize).min(audio.len()),
            );
            if b <= a {
                return 0.0;
            }
            (audio[a..b].iter().map(|x| x * x).sum::<f32>() / (b - a) as f32).sqrt()
        })
        .collect();
    let mut sorted = rms.clone();
    sorted.sort_by(f32::total_cmp);
    let pct = |p: f32| {
        sorted
            .get(((sorted.len() as f32 - 1.0) * p) as usize)
            .copied()
            .unwrap_or(0.0)
    };
    let (p10, p95, peak) = (pct(0.10), pct(0.95), pct(1.0));
    // Sound: within 40 dB of the loudest frame. (Not "over the quiet ones":
    // in steady noise nothing is, and a melody buried in hiss still has
    // notes for coverage to find.)
    let floor = (0.01 * peak).max(1e-4);
    let sounding: Vec<usize> = (0..frames).filter(|&f| rms[f] >= floor).collect();

    let posterior = if notes.is_empty() {
        0.0
    } else {
        notes
            .iter()
            .map(|n| {
                let key = usize::from(n.midi - MIDI_OFFSET);
                (n.start_frame..n.end_frame.min(frames))
                    .map(|t| post.note[t * N_KEYS + key])
                    .fold(0.0f32, f32::max)
            })
            .sum::<f32>()
            / notes.len() as f32
    };

    let mut explained = vec![false; frames];
    for n in notes.iter().filter(|n| n.amplitude >= CONFIDENT_NOTE) {
        for slot in explained
            .iter_mut()
            .take(n.end_frame.min(frames))
            .skip(n.start_frame)
        {
            *slot = true;
        }
    }
    let coverage = if sounding.is_empty() {
        0.0
    } else {
        sounding.iter().filter(|&&f| explained[f]).count() as f32 / sounding.len() as f32
    };

    let snr_db = if p95 <= 0.0 {
        0.0
    } else {
        (20.0 * (p95 / p10.max(1e-5)).log10()).clamp(0.0, 80.0)
    };

    // Flatness on every fourth sounding frame is plenty for a median.
    let window: Vec<f32> = (0..FLATNESS_FFT)
        .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / FLATNESS_FFT as f32).cos())
        .collect();
    let (mut re, mut im) = (vec![0.0f32; FLATNESS_FFT], vec![0.0f32; FLATNESS_FFT]);
    let mut flatnesses: Vec<f32> = sounding
        .iter()
        .step_by(4)
        .map(|&f| {
            let centre = (Posteriorgrams::frame_time(f) * sr) as isize;
            for i in 0..FLATNESS_FFT {
                let at = centre - (FLATNESS_FFT / 2) as isize + i as isize;
                re[i] = if at >= 0 && (at as usize) < audio.len() {
                    audio[at as usize] * window[i]
                } else {
                    0.0
                };
                im[i] = 0.0;
            }
            fontelle_dsp::fft_in_place(&mut re, &mut im);
            let bins = 1..FLATNESS_FFT / 2;
            let n = bins.len() as f64;
            let (mut log_sum, mut sum) = (0.0f64, 0.0f64);
            for k in bins {
                let p = f64::from(re[k] * re[k] + im[k] * im[k]) + 1e-12;
                log_sum += p.ln();
                sum += p;
            }
            ((log_sum / n).exp() / (sum / n)) as f32
        })
        .collect();
    flatnesses.sort_by(f32::total_cmp);
    let flatness = flatnesses.get(flatnesses.len() / 2).copied().unwrap_or(1.0);

    ExtractionEvidence {
        posterior,
        coverage,
        snr_db,
        flatness,
    }
}

/// The weights of [`extraction_confidence`]: hand-set on the synthetic
/// fixtures (a sine melody, a sung line, chords, a mix, a drum loop and
/// noise), to be refitted on labelled real material (plan §2.6).
const W_POSTERIOR: f32 = 4.0;
const W_COVERAGE: f32 = 4.0;
const W_SNR: f32 = 0.04;
const W_FLATNESS: f32 = -12.0;
const BIAS: f32 = -4.0;

/// The evidence as one number, 0..1.
pub fn extraction_confidence(evidence: &ExtractionEvidence) -> f32 {
    let x = W_POSTERIOR * evidence.posterior
        + W_COVERAGE * evidence.coverage
        + W_SNR * evidence.snr_db.min(60.0)
        + W_FLATNESS * evidence.flatness
        + BIAS;
    1.0 / (1.0 + (-x).exp())
}
