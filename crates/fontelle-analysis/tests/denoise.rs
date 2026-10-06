//! The Clean page's denoiser (plan §2.7, P3).

use fontelle_analysis::denoise::{DenoiseOutput, DenoiseSettings, NoiseProfile, denoise};
use fontelle_analysis::testsignals::noise;

const SR: u32 = 48_000;

fn tone(seconds: f64, hz: f64, level: f32) -> Vec<f32> {
    (0..(seconds * f64::from(SR)) as usize)
        .map(|i| (std::f64::consts::TAU * hz * i as f64 / f64::from(SR)).sin() as f32 * level)
        .collect()
}

fn add(a: &[f32], b: &[f32]) -> Vec<f32> {
    a.iter().zip(b).map(|(x, y)| x + y).collect()
}

fn db(power: f64) -> f64 {
    10.0 * power.max(1e-30).log10()
}

/// Mean power per frame over 2048-point Hann frames of `x`, in the bins
/// `keep` says yes to.
fn band_power(x: &[f32], keep: impl Fn(usize) -> bool) -> f64 {
    const N: usize = 2048;
    let window: Vec<f32> = (0..N)
        .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / N as f32).cos())
        .collect();
    let mut total = 0.0f64;
    let mut frames = 0;
    let mut at = N;
    while at + 2 * N <= x.len() {
        let mut re: Vec<f32> = (0..N).map(|i| x[at + i] * window[i]).collect();
        let mut im = vec![0.0f32; N];
        fontelle_dsp::fft_in_place(&mut re, &mut im);
        for bin in 1..N / 2 {
            if keep(bin) {
                total += f64::from(re[bin] * re[bin] + im[bin] * im[bin]);
            }
        }
        frames += 1;
        at += N / 2;
    }
    total / f64::from(frames.max(1))
}

/// Mean spectral flatness (geometric over arithmetic mean of the power)
/// over 1024-point frames, 200 Hz to 16 kHz.
fn flatness(x: &[f32]) -> f64 {
    const N: usize = 1024;
    let window: Vec<f32> = (0..N)
        .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / N as f32).cos())
        .collect();
    let lo = 200 * N / SR as usize;
    let hi = 16_000 * N / SR as usize;
    let mut sum = 0.0;
    let mut frames = 0;
    let mut at = 4096;
    while at + N + 4096 <= x.len() {
        let mut re: Vec<f32> = (0..N).map(|i| x[at + i] * window[i]).collect();
        let mut im = vec![0.0f32; N];
        fontelle_dsp::fft_in_place(&mut re, &mut im);
        let powers: Vec<f64> = (lo..hi)
            .map(|b| f64::from(re[b] * re[b] + im[b] * im[b]).max(1e-30))
            .collect();
        let geo = (powers.iter().map(|p| p.ln()).sum::<f64>() / powers.len() as f64).exp();
        let arith = powers.iter().sum::<f64>() / powers.len() as f64;
        sum += geo / arith;
        frames += 1;
        at += N;
    }
    sum / f64::from(frames)
}

#[test]
fn white_noise_under_a_tone_drops_by_the_set_amount() {
    let hiss = noise(SR, 4.0, 0.05);
    let signal = add(&tone(4.0, 440.0, 0.3), &hiss);
    // The profile from noise alone, as the user's selection of a gap would be
    // (another stretch of the same hiss, not the same samples).
    let profile = NoiseProfile::capture(&noise(SR, 1.0, 0.05)[..], SR).expect("long enough");
    for reduce_db in [6.0f32, 12.0, 18.0] {
        let settings = DenoiseSettings {
            reduce_db,
            amount: 0.5,
            sensitivity: 0.5,
            output: DenoiseOutput::Cleaned,
        };
        let out = denoise(&signal, &profile, &settings);
        assert_eq!(out.len(), signal.len());
        // Away from the tone: bins more than 100 Hz either side of 440.
        let tone_bin = 440.0 * 2048.0 / f64::from(SR);
        let away = |bin: usize| (bin as f64 - tone_bin).abs() > 100.0 * 2048.0 / f64::from(SR);
        let before = db(band_power(&signal, away));
        let after = db(band_power(&out, away));
        let drop = before - after;
        assert!(
            (drop - f64::from(reduce_db)).abs() < 2.0,
            "reduce by {reduce_db} dB took the noise down {drop:.2} dB"
        );
        // And the tone stays.
        let near = |bin: usize| (bin as f64 - tone_bin).abs() <= 2.0;
        let kept = db(band_power(&out, near)) - db(band_power(&signal, near));
        assert!(kept.abs() < 1.0, "the tone moved {kept:.2} dB");
    }
}

#[test]
fn a_clean_tone_passes_within_0_5_db() {
    let clean = add(&tone(3.0, 330.0, 0.4), &tone(3.0, 1250.0, 0.1));
    // A profile of a faint hiss, far under the tone.
    let profile = NoiseProfile::capture(&noise(SR, 1.0, 0.001), SR).expect("long enough");
    let out = denoise(&clean, &profile, &DenoiseSettings::default());
    let rms = |x: &[f32]| {
        (x[4096..x.len() - 4096]
            .iter()
            .map(|v| f64::from(*v) * f64::from(*v))
            .sum::<f64>()
            / (x.len() - 8192) as f64)
            .sqrt()
    };
    let change = 20.0 * (rms(&out) / rms(&clean)).log10();
    assert!(change.abs() < 0.5, "a clean tone came out {change:.3} dB");
}

#[test]
fn no_musical_noise_above_threshold() {
    // Noise alone, taken down 20 dB: what is left must still be noise —
    // flat — rather than the warbling tones of isolated surviving bins.
    let hiss = noise(SR, 4.0, 0.1);
    let profile = NoiseProfile::capture(&noise(SR, 1.0, 0.1), SR).expect("long enough");
    let settings = DenoiseSettings {
        reduce_db: 20.0,
        amount: 0.5,
        sensitivity: 0.5,
        output: DenoiseOutput::Cleaned,
    };
    let out = denoise(&hiss, &profile, &settings);
    let before = flatness(&hiss);
    let after = flatness(&out);
    assert!(
        after > 0.75 * before,
        "the residue is tonal: flatness {after:.3} against the noise's {before:.3}"
    );
}

#[test]
fn what_is_removed_and_what_is_kept_add_back_to_the_input() {
    let signal = add(&tone(2.0, 440.0, 0.3), &noise(SR, 2.0, 0.05));
    let profile = NoiseProfile::capture(&noise(SR, 1.0, 0.05), SR).expect("long enough");
    let mut settings = DenoiseSettings::default();
    let kept = denoise(&signal, &profile, &settings);
    settings.output = DenoiseOutput::Removed;
    let removed = denoise(&signal, &profile, &settings);
    for ((k, r), x) in kept.iter().zip(&removed).zip(&signal) {
        assert!((k + r - x).abs() < 1e-4);
    }
    // And the removed part is mostly the noise: well under the signal.
    let power = |x: &[f32]| x.iter().map(|v| f64::from(*v) * f64::from(*v)).sum::<f64>();
    assert!(power(&removed) < 0.1 * power(&signal));
}

#[test]
fn a_selection_shorter_than_a_window_is_no_profile() {
    assert!(NoiseProfile::capture(&[0.0; 100], SR).is_none());
}

/// The optional voice denoiser (RNNoise via nnnoiseless): 48 kHz mono, no
/// profile. It must keep the length and take hiss down.
#[cfg(feature = "voice-denoise")]
#[test]
fn the_voice_denoiser_takes_hiss_down_and_keeps_the_length() {
    let hiss = noise(SR, 2.0, 0.05);
    let out = fontelle_analysis::denoise::denoise_voice(&hiss);
    assert_eq!(out.len(), hiss.len());
    let power = |x: &[f32]| {
        x[9600..]
            .iter()
            .map(|v| f64::from(*v) * f64::from(*v))
            .sum::<f64>()
    };
    let drop = db(power(&hiss)) - db(power(&out));
    // RNNoise is trained on voices over real noise and is gentle with pure
    // white hiss (about 4 dB here); this pins that it is wired in, scaled and
    // aligned, not how good the network is.
    assert!(drop > 2.0, "hiss down {drop:.2} dB");
}
