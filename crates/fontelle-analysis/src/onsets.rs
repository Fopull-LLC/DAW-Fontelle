//! Where things start (plan §2.7): super-flux onset detection with an
//! adaptive threshold. Feeds "Slice at transients" and segmentation.

/// One onset.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Onset {
    /// Where it starts, in samples from the start of the buffer.
    pub sample: usize,
    /// How strong the change was, 0..1 against the strongest in the buffer.
    pub strength: f32,
}

/// The slice page's transient knobs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OnsetSettings {
    /// 0..1: higher finds quieter onsets.
    pub sensitivity: f32,
    /// Two onsets closer than this are one.
    pub min_gap_ms: f32,
}

impl Default for OnsetSettings {
    fn default() -> Self {
        Self {
            sensitivity: 0.5,
            min_gap_ms: 30.0,
        }
    }
}

/// Frames a second the detection function runs at: 5 ms apart, so a hit is
/// placed to within half that before the time-domain refinement.
const FRAMES_PER_SECOND: f64 = 200.0;
/// Log-spaced bands a octave, as super-flux's filterbank.
const BANDS_PER_OCTAVE: f32 = 24.0;
/// The band the filterbank starts at, in hertz.
const LOWEST_HZ: f32 = 30.0;
/// How many frames back the flux compares against (super-flux's μ).
const LAG: usize = 2;
/// The neighbourhood a peak must be the top of: 10 ms before, 50 ms after
/// (super-flux's pre/post max).
const PRE_MAX_S: f64 = 0.01;
const POST_MAX_S: f64 = 0.05;
/// The span the moving average looks back over (pre-avg).
const PRE_AVG_S: f64 = 0.15;

/// Every onset in `samples` (mono), in order.
///
/// Super-flux (Böck and Widmer, 2013): a log-magnitude spectrogram on
/// log-spaced bands, each frame compared with a maximum-filtered frame two
/// back — the filter is what keeps vibrato from reading as a stream of
/// onsets — and the positive differences summed. A peak is an onset when it
/// tops its neighbourhood and stands above the moving average by a margin
/// set from the function's own mean and the sensitivity (the adaptive
/// threshold). Each is then placed in the waveform to the millisecond: the
/// block where the high-passed energy rises most, near the peak's frame.
pub fn detect_onsets(samples: &[f32], sample_rate: u32, settings: &OnsetSettings) -> Vec<Onset> {
    if samples.is_empty() || sample_rate == 0 {
        return Vec::new();
    }
    let sr = f64::from(sample_rate);
    // About 21 ms of window whatever the rate.
    let size = ((0.021 * sr) as usize).next_power_of_two().max(64);
    let hop = ((sr / FRAMES_PER_SECOND).round() as usize).max(1);
    let bands = filterbank(size, sample_rate);

    let mut planner = realfft::RealFftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(size);
    let window: Vec<f32> = (0..size)
        .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / size as f32).cos())
        .collect();
    let mut time = fft.make_input_vec();
    let mut spectrum = fft.make_output_vec();

    // Frame `n` is centred on sample `n * hop`.
    let frames = samples.len() / hop + 1;
    let mut log_bands: Vec<Vec<f32>> = Vec::with_capacity(frames);
    for n in 0..frames {
        let centre = (n * hop) as isize;
        for (i, t) in time.iter_mut().enumerate() {
            let at = centre - (size / 2) as isize + i as isize;
            *t = if at >= 0 && (at as usize) < samples.len() {
                samples[at as usize] * window[i]
            } else {
                0.0
            };
        }
        let _ = fft.process(&mut time, &mut spectrum);
        log_bands.push(
            bands
                .iter()
                .map(|band| {
                    let energy: f32 = band
                        .iter()
                        .map(|(bin, weight)| spectrum[*bin].norm() * weight)
                        .sum();
                    (1.0 + energy).log10()
                })
                .collect(),
        );
    }

    // The flux against a maximum-filtered past frame. Before the start is
    // silence, so a sound that begins on the first sample is an onset.
    let width = bands.len();
    let silence = vec![0.0f32; width];
    let mut odf = vec![0.0f32; frames];
    for n in 0..frames {
        let past = if n >= LAG {
            &log_bands[n - LAG]
        } else {
            &silence
        };
        odf[n] = (0..width)
            .map(|b| {
                let lo = b.saturating_sub(1);
                let hi = (b + 1).min(width - 1);
                let reference = past[lo..=hi].iter().copied().fold(f32::MIN, f32::max);
                (log_bands[n][b] - reference).max(0.0)
            })
            .sum();
    }

    let peak = odf.iter().copied().fold(0.0f32, f32::max);
    if peak <= 1e-6 {
        return Vec::new();
    }
    let mean = odf.iter().sum::<f32>() / frames as f32;
    let sensitivity = settings.sensitivity.clamp(0.0, 1.0);
    let delta = mean * (0.25 + 1.5 * (1.0 - sensitivity)) + peak * 0.02 * (1.0 - sensitivity);
    let pre_max = (PRE_MAX_S * FRAMES_PER_SECOND).round() as usize;
    let post_max = (POST_MAX_S * FRAMES_PER_SECOND).round() as usize;
    let pre_avg = (PRE_AVG_S * FRAMES_PER_SECOND).round() as usize;

    let min_gap = ((f64::from(settings.min_gap_ms.max(0.0)) / 1000.0) * sr) as usize;
    let mut onsets: Vec<Onset> = Vec::new();
    for n in 0..frames {
        let value = odf[n];
        if value <= 0.0 {
            continue;
        }
        let lo = n.saturating_sub(pre_max);
        let hi = (n + post_max).min(frames - 1);
        if odf[lo..=hi].iter().any(|v| *v > value) {
            continue;
        }
        // A plateau's first frame only.
        if n > 0 && odf[lo..n].contains(&value) {
            continue;
        }
        // The average of what came before: a frame is not compared with
        // itself, or the first sound in a file could never stand above it.
        let avg_lo = n.saturating_sub(pre_avg);
        let average = if n > avg_lo {
            odf[avg_lo..n].iter().sum::<f32>() / (n - avg_lo) as f32
        } else {
            0.0
        };
        if value < average + delta {
            continue;
        }
        let sample = refine(samples, n * hop, size, sample_rate);
        if let Some(last) = onsets.last()
            && sample < last.sample + min_gap
        {
            continue;
        }
        onsets.push(Onset {
            sample,
            strength: value / peak,
        });
    }
    onsets
}

/// The triangular log-spaced filterbank, as (bin, weight) lists. Bands
/// narrower than a bin at the bottom collapse onto the bins they share, so
/// no band is empty and none is counted twice.
fn filterbank(size: usize, sample_rate: u32) -> Vec<Vec<(usize, f32)>> {
    let bins = size / 2 + 1;
    let hz_per_bin = sample_rate as f32 / size as f32;
    let top = (sample_rate as f32 / 2.0).min(16_000.0);
    let mut centres: Vec<usize> = Vec::new();
    let mut hz = LOWEST_HZ;
    while hz < top {
        let bin = (hz / hz_per_bin).round() as usize;
        if bin < bins && centres.last() != Some(&bin) {
            centres.push(bin);
        }
        hz *= 2f32.powf(1.0 / BANDS_PER_OCTAVE);
    }
    centres
        .windows(3)
        .map(|w| {
            let (lo, mid, hi) = (w[0], w[1], w[2]);
            let mut band = Vec::new();
            for bin in lo..=hi {
                let weight = if bin <= mid {
                    if mid == lo {
                        1.0
                    } else {
                        (bin - lo) as f32 / (mid - lo) as f32
                    }
                } else {
                    (hi - bin) as f32 / (hi - mid) as f32
                };
                if weight > 0.0 {
                    band.push((bin, weight));
                }
            }
            band
        })
        .collect()
}

/// Where in the waveform an onset detected in the frame centred on `centre`
/// starts: the millisecond block, from a window before the frame to half a
/// window after, where the high-passed energy rises the most.
fn refine(samples: &[f32], centre: usize, size: usize, sample_rate: u32) -> usize {
    let block = ((sample_rate / 1000) as usize).max(1);
    let lo = centre.saturating_sub(size).max(1);
    let hi = (centre + size / 2).min(samples.len());
    if hi <= lo + 2 * block {
        return centre.min(samples.len().saturating_sub(1));
    }
    let energies: Vec<f32> = (lo..hi)
        .step_by(block)
        .map(|start| {
            let end = (start + block).min(hi);
            (start..end)
                .map(|i| {
                    let d = samples[i] - samples[i - 1];
                    d * d
                })
                .sum()
        })
        .collect();
    let most = energies.iter().copied().fold(0.0f32, f32::max);
    let floor = most * 1e-3 + 1e-12;
    let mut best = (0usize, f32::MIN);
    for b in 1..energies.len() {
        let rise = (energies[b] + floor).ln() - (energies[b - 1] + floor).ln();
        if rise > best.1 {
            best = (b, rise);
        }
    }
    lo + best.0 * block
}
