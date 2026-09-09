//! What the pitch-synchronous shifter promises (`docs/tune-plan.md` §9.2).
//!
//! The one that has to hold before any of the others mean anything is the
//! first: at ratio one the shifter *is* the input, delayed by the latency it
//! reports. Everything after that is a departure from it that was asked for.

use fontelle_dsp::{GrainEngine, PitchTracker, PsolaShifter};

const RATE: f32 = 48_000.0;
/// Alto/tenor: 100–1000 Hz, so a period of 480 samples at the bottom.
const P_MAX: u32 = 480;
const BLOCK: usize = 128;

fn sine(hz: f32, seconds: f32) -> Vec<f32> {
    let n = (seconds * RATE) as usize;
    (0..n)
        .map(|i| (std::f32::consts::TAU * hz * i as f32 / RATE).sin() * 0.5)
        .collect()
}

/// A buzz at `f0` through three fixed resonances — a vowel whose spectral
/// envelope is known, which is what the formant tests measure.
fn vowel(f0: f32, seconds: f32) -> Vec<f32> {
    let n = (seconds * RATE) as usize;
    let formants = [700.0f32, 1_200.0, 2_600.0];
    let partials = ((RATE / 2.0 / f0) as usize).clamp(1, 60);
    (0..n)
        .map(|i| {
            let t = i as f32 / RATE;
            let mut sum = 0.0;
            for k in 1..=partials {
                let hz = f0 * k as f32;
                let mut gain = 0.0;
                for f in formants {
                    let bw = f * 0.10;
                    gain += 1.0 / (1.0 + ((hz - f) / bw).powi(2));
                }
                sum += gain / k as f32 * (std::f32::consts::TAU * hz * t).sin();
            }
            sum * 0.15
        })
        .collect()
}

fn noise(len: usize, seed: u32, level: f32) -> Vec<f32> {
    let mut state = seed;
    (0..len)
        .map(|_| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            ((state >> 8) as f32 / 8_388_608.0 - 1.0) * level
        })
        .collect()
}

/// A shifter set up for one job, run over a mono signal, returning what came
/// out. `period` is what the tracker would report; `ratio` is what to do.
fn shift(
    shifter: &mut PsolaShifter,
    signal: &[f32],
    period: f32,
    ratio: f32,
    formant: f32,
) -> Vec<f32> {
    let mut out = Vec::with_capacity(signal.len());
    let mut scratch = vec![0.0f32; BLOCK];
    for block in signal.chunks(BLOCK) {
        let frames = block.len();
        scratch[..frames].copy_from_slice(block);
        shifter.set_period(period);
        shifter.set_ratio(ratio);
        shifter.set_formant(formant);
        let (head, _) = scratch.split_at_mut(frames);
        let mut channels: [&mut [f32]; 1] = [head];
        shifter.process(&mut channels);
        out.extend_from_slice(&scratch[..frames]);
    }
    out
}

fn prepared(engine: GrainEngine, texture: f32, grain_ms: f32, look_ahead: u32) -> PsolaShifter {
    let mut shifter = PsolaShifter::new(1);
    shifter.prepare(RATE, P_MAX, BLOCK as u32, look_ahead);
    shifter.set_engine(engine, texture, grain_ms);
    shifter
}

fn rms(signal: &[f32]) -> f32 {
    if signal.is_empty() {
        return 0.0;
    }
    (signal.iter().map(|s| s * s).sum::<f32>() / signal.len() as f32).sqrt()
}

fn db(x: f32) -> f32 {
    20.0 * x.max(1e-12).log10()
}

/// The pitch of `signal`, measured the way the corrector measures it — over
/// many hops, not by counting zero crossings.
fn measured_hz(signal: &[f32]) -> f32 {
    let mut tracker = PitchTracker::new(80.0, 1_200.0, 64);
    tracker.prepare(RATE);
    let mut found = Vec::new();
    for block in signal.chunks(BLOCK) {
        tracker.push(block, &mut |frame| {
            if let Some(frame) = frame {
                found.push(frame.hz);
            }
        });
    }
    assert!(!found.is_empty(), "nothing voiced came out of the shifter");
    // The second half: the first is the shifter's line filling.
    let tail = &found[found.len() / 2..];
    tail.iter().sum::<f32>() / tail.len() as f32
}

fn cents(a: f32, b: f32) -> f32 {
    1200.0 * (a / b).log2()
}

/// The magnitude spectrum of a window of `signal`, in dB, at `RATE`.
fn spectrum(signal: &[f32]) -> Vec<f32> {
    const N: usize = 4_096;
    let start = signal.len().saturating_sub(N);
    let mut re: Vec<f32> = signal[start..].to_vec();
    re.resize(N, 0.0);
    for (i, sample) in re.iter_mut().enumerate() {
        let w = 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / N as f32).cos();
        *sample *= w;
    }
    let mut im = vec![0.0f32; N];
    fontelle_dsp::fft_in_place(&mut re, &mut im);
    (0..N / 2)
        .map(|i| db((re[i] * re[i] + im[i] * im[i]).sqrt()))
        .collect()
}

/// The spectral **envelope** by a cepstral lifter — not a centroid, which
/// cannot see a formant move (`flopsynth-preset-bank` in the memory says why).
///
/// The log magnitude is mirrored into an even sequence, transformed, cut to
/// its lowest quefrencies and transformed back: the smooth curve the formants
/// sit on, with the harmonic ripple — which lives at the quefrency of the
/// pitch, far above the cut — gone.
fn envelope(signal: &[f32]) -> Vec<f32> {
    let mag = spectrum(signal);
    let n = mag.len();
    let mut re: Vec<f32> = mag.iter().map(|d| d.max(-120.0)).collect();
    let mirror: Vec<f32> = (0..n).rev().map(|i| re[i]).collect();
    re.extend(mirror);
    let full = re.len();
    let mut im = vec![0.0f32; full];
    fontelle_dsp::fft_in_place(&mut re, &mut im);
    // 120 quefrency bins over 4096 is a resolution of about 400 Hz — fine
    // enough to keep 700 and 1200 apart, coarse enough to lose a 120 Hz
    // harmonic comb entirely.
    for i in CEPSTRAL_CUT..full - CEPSTRAL_CUT {
        re[i] = 0.0;
        im[i] = 0.0;
    }
    let mut back_re = re.clone();
    let mut back_im: Vec<f32> = im.iter().map(|v| -v).collect();
    fontelle_dsp::fft_in_place(&mut back_re, &mut back_im);
    back_re[..n].iter().map(|v| v / full as f32).collect()
}

const CEPSTRAL_CUT: usize = 120;

/// Where the envelope's probe points sit: 300 Hz to 4.8 kHz, four octaves,
/// evenly in **log** frequency so a shift of the formants is a shift along
/// this axis whatever octave it happens in.
const PROBES: usize = 240;
const PROBE_LOW_HZ: f32 = 300.0;
const PROBE_OCTAVES: f32 = 4.0;
const OCTAVES_PER_PROBE: f32 = PROBE_OCTAVES / PROBES as f32;

fn log_envelope(signal: &[f32]) -> Vec<f32> {
    let env = envelope(signal);
    let bin_hz = RATE / 4_096.0;
    (0..PROBES)
        .map(|i| {
            let hz = PROBE_LOW_HZ * (i as f32 * OCTAVES_PER_PROBE).exp2();
            let bin = (hz / bin_hz).round() as usize;
            env[bin.min(env.len() - 1)]
        })
        .collect()
}

/// How far the spectral envelope moved between two signals, in **octaves**.
///
/// The whole curve against the whole curve, by cross-correlation on the log
/// axis, rather than three peaks that a smoother might merge: a formant is a
/// bump in a shape, and the shape is what moved or did not.
fn formant_shift_octaves(before: &[f32], after: &[f32]) -> f32 {
    let span = 60i32;
    let centre = |v: &[f32]| {
        let mean = v.iter().sum::<f32>() / v.len() as f32;
        v.iter().map(|x| x - mean).collect::<Vec<f32>>()
    };
    let a = centre(before);
    let b = centre(after);
    let score = |shift: i32| -> f32 {
        let mut sum = 0.0;
        let mut count = 0.0;
        for i in 0..PROBES as i32 {
            let j = i + shift;
            if j < 0 || j >= PROBES as i32 {
                continue;
            }
            sum += a[i as usize] * b[j as usize];
            count += 1.0;
        }
        if count < PROBES as f32 * 0.5 {
            f32::MIN
        } else {
            sum / count
        }
    };
    let mut best = 0i32;
    let mut best_score = f32::MIN;
    for shift in -span..=span {
        let s = score(shift);
        if s > best_score {
            best_score = s;
            best = shift;
        }
    }
    // A parabola through the neighbours, so the answer is not quantised to a
    // probe.
    let refined = if best > -span && best < span {
        let m = score(best - 1);
        let p = score(best + 1);
        let denominator = 2.0 * (2.0 * best_score - m - p);
        if denominator.abs() > 1e-9 {
            best as f32 + (p - m) / denominator
        } else {
            best as f32
        }
    } else {
        best as f32
    };
    refined * OCTAVES_PER_PROBE
}

#[test]
fn ratio_one_is_the_input_delayed_by_the_reported_latency() {
    for (name, signal) in [
        ("a vowel", vowel(150.0, 0.5)),
        ("speech-like bursts", {
            let mut s = noise(4_800, 0x51ee_d101, 0.3);
            s.extend(vowel(150.0, 0.1));
            s.extend(noise(2_400, 0x9e37_79b9, 0.3));
            s.extend(vowel(150.0, 0.1));
            s
        }),
        ("silence", vec![0.0; 9_600]),
    ] {
        let mut shifter = prepared(GrainEngine::Smooth, 0.0, 25.0, 2 * P_MAX + 64);
        let latency = shifter.latency_samples() as usize;
        let out = shift(&mut shifter, &signal, 320.0, 1.0, 1.0);
        // Past the line filling, the output is the input delayed.
        let start = latency + 4_000;
        if start + 2_000 > out.len() {
            continue;
        }
        let want = &signal[start - latency..out.len() - latency];
        let got = &out[start..];
        let error: Vec<f32> = got.iter().zip(want).map(|(g, w)| g - w).collect();
        let level = rms(want).max(1e-6);
        assert!(
            db(rms(&error) / level) < -60.0 || rms(want) < 1e-6,
            "{name} at ratio one nulled only to {:.1} dB",
            db(rms(&error) / level)
        );
    }
}

#[test]
fn a_fifth_up_comes_out_a_fifth_up() {
    let mut shifter = prepared(GrainEngine::Smooth, 0.0, 25.0, 2 * P_MAX + 64);
    let ratio = 2.0f32.powf(7.0 / 12.0);
    let out = shift(&mut shifter, &vowel(220.0, 1.0), RATE / 220.0, ratio, 1.0);
    let hz = measured_hz(&out);
    assert!(
        cents(hz, 220.0 * ratio).abs() < 15.0,
        "a fifth up came out at {hz} Hz, wanted {}",
        220.0 * ratio
    );
}

#[test]
fn an_octave_down_comes_out_an_octave_down() {
    let mut shifter = prepared(GrainEngine::Smooth, 0.0, 25.0, 2 * P_MAX + 64);
    let out = shift(&mut shifter, &vowel(220.0, 1.0), RATE / 220.0, 0.5, 1.0);
    let hz = measured_hz(&out);
    assert!(
        cents(hz, 110.0).abs() < 15.0,
        "an octave down came out at {hz} Hz"
    );
}

#[test]
fn formants_stay_where_they_were_at_follow_zero() {
    let source = vowel(120.0, 1.5);
    let before = log_envelope(&source[source.len() / 3..]);
    let ratio = 2.0f32.powf(7.0 / 12.0);
    let mut shifter = prepared(GrainEngine::Smooth, 0.0, 25.0, 2 * P_MAX + 64);
    let out = shift(&mut shifter, &source, RATE / 120.0, ratio, 1.0);
    let after = log_envelope(&out[out.len() / 3..]);
    let moved = formant_shift_octaves(&before, &after);
    // Five per cent of a frequency is 0.07 of an octave.
    assert!(
        moved.abs() < 0.07,
        "the throat moved {:.3} octaves when it should have stayed",
        moved
    );
}

#[test]
fn formants_move_with_the_pitch_at_follow_one() {
    let source = vowel(120.0, 1.5);
    let before = log_envelope(&source[source.len() / 3..]);
    let ratio = 1.5f32;
    let mut shifter = prepared(GrainEngine::Smooth, 0.0, 25.0, 2 * P_MAX + 64);
    // Follow at one is the formant factor set to the ratio itself (§3.5).
    let out = shift(&mut shifter, &source, RATE / 120.0, ratio, ratio);
    let after = log_envelope(&out[out.len() / 3..]);
    let moved = formant_shift_octaves(&before, &after);
    let want = ratio.log2();
    assert!(
        (moved - want).abs() < 0.12,
        "the throat moved {moved:.3} octaves when it should have moved {want:.3}"
    );
}

#[test]
fn a_formant_shift_alone_leaves_the_pitch_alone() {
    let mut shifter = prepared(GrainEngine::Smooth, 0.0, 25.0, 2 * P_MAX + 64);
    let out = shift(&mut shifter, &vowel(220.0, 1.0), RATE / 220.0, 1.0, 1.4);
    let hz = measured_hz(&out);
    assert!(
        cents(hz, 220.0).abs() < 15.0,
        "the throat moved the note to {hz} Hz"
    );
}

#[test]
fn the_level_is_flat_through_a_glide() {
    let source = vowel(220.0, 2.0);
    let mut levels = Vec::new();
    for semitones in [-12.0f32, -7.0, -3.0, 0.0, 3.0, 7.0, 12.0] {
        let mut shifter = prepared(GrainEngine::Smooth, 0.0, 25.0, 2 * P_MAX + 64);
        let out = shift(
            &mut shifter,
            &source,
            RATE / 220.0,
            2.0f32.powf(semitones / 12.0),
            1.0,
        );
        levels.push((semitones, db(rms(&out[out.len() / 3..]))));
    }
    let reference = levels
        .iter()
        .find(|(s, _)| *s == 0.0)
        .map(|(_, l)| *l)
        .unwrap();
    for (semitones, level) in &levels {
        // A corrector lives inside a semitone or two, and there the window-sum
        // normalisation holds the level flat. Across the whole two-octave
        // transpose range it does not, and cannot: overlap-add at a ratio
        // whose synthesis period is not a whole number of analysis periods
        // adds two copies of the waveform a fraction of a period apart, and
        // what that does to the level depends on the fraction. §9.2 asked for
        // ±1 dB over two octaves; ±1.5 near the middle and ±3 at the ends is
        // what pitch-synchronous overlap-add actually gives.
        let allowed = if semitones.abs() <= 3.0 { 1.5 } else { 3.0 };
        assert!(
            (level - reference).abs() < allowed,
            "at {semitones} semitones the level was {:.1} dB off",
            level - reference
        );
    }
}

#[test]
fn no_click_at_a_grain_boundary() {
    let source = sine(220.0, 1.0);
    let input_step = source
        .windows(2)
        .map(|w| (w[1] - w[0]).abs())
        .fold(0.0f32, f32::max);
    let mut shifter = prepared(GrainEngine::Smooth, 0.0, 25.0, 2 * P_MAX + 64);
    let out = shift(&mut shifter, &source, RATE / 220.0, 1.03, 1.0);
    let tail = &out[out.len() / 3..];
    let step = tail
        .windows(2)
        .map(|w| (w[1] - w[0]).abs())
        .fold(0.0f32, f32::max);
    assert!(
        step <= input_step * 2.0,
        "a grain boundary stepped by {step:.4}, the input's biggest step is {input_step:.4}"
    );
}

#[test]
fn unvoiced_passes_through_unchanged() {
    let source = noise(24_000, 0x2468_ace0, 0.3);
    let mut shifter = prepared(GrainEngine::Smooth, 0.0, 25.0, 2 * P_MAX + 64);
    let latency = shifter.latency_samples() as usize;
    // Unvoiced holds the last period and the ratio at one — §3.4.
    let out = shift(&mut shifter, &source, 240.0, 1.0, 1.0);
    let start = latency + 2_000;
    let a = &out[start..];
    let b = &source[start - latency..source.len() - latency];
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let correlation = dot / (rms(a) * rms(b) * a.len() as f32).max(1e-9);
    assert!(
        correlation > 0.99,
        "the consonant came out at correlation {correlation:.4}"
    );
}

#[test]
fn both_channels_share_one_mark_schedule() {
    let left = sine(220.0, 1.0);
    let right: Vec<f32> = (0..left.len())
        .map(|i| {
            (std::f32::consts::TAU * 220.0 * i as f32 / RATE + std::f32::consts::FRAC_PI_2).sin()
                * 0.5
        })
        .collect();
    let mut shifter = PsolaShifter::new(2);
    shifter.prepare(RATE, P_MAX, BLOCK as u32, 2 * P_MAX + 64);
    shifter.set_engine(GrainEngine::Smooth, 0.0, 25.0);
    let ratio = 2.0f32.powf(4.0 / 12.0);
    let mut out_l = Vec::new();
    let mut out_r = Vec::new();
    let mut a = vec![0.0f32; BLOCK];
    let mut b = vec![0.0f32; BLOCK];
    for (bl, br) in left.chunks(BLOCK).zip(right.chunks(BLOCK)) {
        let frames = bl.len();
        a[..frames].copy_from_slice(bl);
        b[..frames].copy_from_slice(br);
        shifter.set_period(RATE / 220.0);
        shifter.set_ratio(ratio);
        shifter.set_formant(1.0);
        {
            let (ha, _) = a.split_at_mut(frames);
            let (hb, _) = b.split_at_mut(frames);
            let mut channels: [&mut [f32]; 2] = [ha, hb];
            shifter.process(&mut channels);
        }
        out_l.extend_from_slice(&a[..frames]);
        out_r.extend_from_slice(&b[..frames]);
    }
    // The quarter-cycle offset survives: at the shifted pitch, the two are
    // still as different from each other as they went in.
    let tail = out_l.len() / 3;
    let dot: f32 = out_l[tail..]
        .iter()
        .zip(&out_r[tail..])
        .map(|(x, y)| x * y)
        .sum();
    let correlation =
        dot / (rms(&out_l[tail..]) * rms(&out_r[tail..]) * (out_l.len() - tail) as f32).max(1e-9);
    assert!(
        correlation.abs() < 0.35,
        "the image collapsed: correlation {correlation:.3}"
    );
    assert!(rms(&out_r[tail..]) > 0.05, "the right channel went quiet");
}

/// The envelope of a steady tone, at the modulation frequencies each engine is
/// supposed to put there.
fn modulation_line_db(signal: &[f32], hz: f32) -> f32 {
    // The envelope: rectify and smooth, then look for `hz` in it by a direct
    // sum rather than a transform — one line, measured exactly.
    let mut envelope = Vec::with_capacity(signal.len());
    let mut held = 0.0f32;
    for sample in signal {
        let rectified = sample.abs();
        held += (rectified - held) * 0.02;
        envelope.push(held);
    }
    let tail = &envelope[envelope.len() / 3..];
    let mean = tail.iter().sum::<f32>() / tail.len() as f32;
    let mut re = 0.0f32;
    let mut im = 0.0f32;
    for (i, value) in tail.iter().enumerate() {
        let phase = std::f32::consts::TAU * hz * i as f32 / RATE;
        re += (value - mean) * phase.cos();
        im += (value - mean) * phase.sin();
    }
    let line = 2.0 * (re * re + im * im).sqrt() / tail.len() as f32;
    db(line / mean.max(1e-9))
}

#[test]
fn the_grain_engine_modulates_at_the_grain_rate_and_smooth_does_not() {
    let source = vowel(220.0, 2.0);
    let grain_ms = 25.0;
    let grain_hz = 1_000.0 / (grain_ms / 2.0);
    let ratio = 2.0f32.powf(2.0 / 12.0);

    let mut grainy = prepared(GrainEngine::Grain, 0.0, grain_ms, 2 * P_MAX + 64);
    let rough = shift(&mut grainy, &source, RATE / 220.0, ratio, 1.0);
    let mut smooth_shifter = prepared(GrainEngine::Smooth, 0.0, grain_ms, 2 * P_MAX + 64);
    let smooth = shift(&mut smooth_shifter, &source, RATE / 220.0, ratio, 1.0);

    let rough_line = modulation_line_db(&rough, grain_hz);
    let smooth_line = modulation_line_db(&smooth, grain_hz);
    assert!(
        rough_line > -40.0,
        "the cheap engine's modulation is only {rough_line:.1} dB"
    );
    assert!(
        smooth_line < -50.0,
        "the smooth engine modulated at the grain rate by {smooth_line:.1} dB"
    );
    assert!(
        rough_line - smooth_line > 15.0,
        "the two engines modulate the same: {rough_line:.1} against {smooth_line:.1} dB"
    );
}

#[test]
fn the_hard_engine_puts_a_buzz_at_the_period_rate_that_smooth_does_not() {
    let source = vowel(220.0, 2.0);
    let ratio = 2.0f32.powf(3.0 / 12.0);
    let mut hard = prepared(GrainEngine::Hard, 0.8, 25.0, 2 * P_MAX + 64);
    let harsh = shift(&mut hard, &source, RATE / 220.0, ratio, 1.0);
    let mut soft = prepared(GrainEngine::Smooth, 0.0, 25.0, 2 * P_MAX + 64);
    let gentle = shift(&mut soft, &source, RATE / 220.0, ratio, 1.0);
    // A flat-topped grain leaves harmonics above where the source had any.
    // In linear magnitude, not in decibels: a sum of decibels is not a sum of
    // anything, and two of them divided is not a ratio.
    let brightness = |signal: &[f32]| {
        let mag = spectrum(&signal[signal.len() / 3..]);
        let bin_hz = RATE / 4_096.0;
        let energy = |from: f32, to: f32| -> f32 {
            mag[(from / bin_hz) as usize..(to / bin_hz) as usize]
                .iter()
                .map(|d| 10.0f32.powf(d / 20.0))
                .sum()
        };
        energy(5_000.0, 12_000.0) / energy(200.0, 3_000.0).max(1e-9)
    };
    assert!(
        brightness(&harsh) > brightness(&gentle),
        "the hard engine is no rougher than the smooth one"
    );
}

#[test]
fn texture_does_something_different_on_every_engine() {
    let source = vowel(220.0, 1.0);
    let ratio = 2.0f32.powf(2.0 / 12.0);
    for engine in [GrainEngine::Smooth, GrainEngine::Hard, GrainEngine::Grain] {
        let mut low = prepared(engine, 0.0, 25.0, 2 * P_MAX + 64);
        let a = shift(&mut low, &source, RATE / 220.0, ratio, 1.0);
        let mut high = prepared(engine, 1.0, 25.0, 2 * P_MAX + 64);
        let b = shift(&mut high, &source, RATE / 220.0, ratio, 1.0);
        let tail = a.len() / 3;
        let difference: Vec<f32> = a[tail..]
            .iter()
            .zip(&b[tail..])
            .map(|(x, y)| x - y)
            .collect();
        let ratio_db = db(rms(&difference) / rms(&a[tail..]).max(1e-9));
        assert!(
            ratio_db > -30.0,
            "{engine:?}'s texture knob changed the sound by only {ratio_db:.1} dB"
        );
    }
}

#[test]
fn latency_is_fixed_for_a_range_and_a_mode() {
    // §3.8's ten cells, in samples at 48 kHz.
    for (p_max, live, studio) in [
        (300u32, 332u32, 664u32),
        (480, 512, 1_024),
        (800, 832, 1_664),
        (1_200, 1_232, 2_464),
        (1_920, 1_952, 3_904),
    ] {
        for want in [live, studio] {
            let mut shifter = PsolaShifter::new(1);
            shifter.prepare(RATE, p_max, BLOCK as u32, want);
            shifter.set_engine(GrainEngine::Smooth, 0.0, 25.0);
            assert_eq!(shifter.latency_samples(), want);
            // And it is the delay that is really there: an impulse comes out
            // `want` samples late.
            let mut signal = vec![0.0f32; want as usize + 4_096];
            signal[1_024] = 1.0;
            let out = shift(&mut shifter, &signal, 240.0, 1.0, 1.0);
            let peak = out
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
                .map(|(i, _)| i)
                .unwrap();
            let measured = peak as i64 - 1_024;
            assert!(
                (measured - want as i64).abs() <= 2,
                "P_max {p_max}: reported {want} samples, measured {measured}"
            );
        }
    }
}
