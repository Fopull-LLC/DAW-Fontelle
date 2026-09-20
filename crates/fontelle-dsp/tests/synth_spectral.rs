//! The spectral source (`docs/flopsynth-next.md` §4.3): a recording
//! analysed into frames of partials — sixty-four ratios and amplitudes
//! every ten milliseconds — and played back through the string's bank of
//! phasors. Position scans the frames, Freeze holds one, Stretch spreads
//! the partials, Shift moves the formant.

use fontelle_dsp::{
    SpectralFrames, SynthInput, SynthOsc, SynthSource, SynthState, WarpMode, analyse_spectral,
    fft_in_place,
};

const SR: f32 = 48_000.0;

/// `seconds` of partials at `hz × ratio` with `amps`, each amplitude
/// scaled by `fade(t)`'s value for that partial.
fn recording(
    hz: f32,
    partials: &[(f32, f32)],
    seconds: f32,
    fade: impl Fn(usize, f32) -> f32,
) -> Vec<f32> {
    let n = (seconds * SR) as usize;
    (0..n)
        .map(|i| {
            let t = i as f32 / SR;
            partials
                .iter()
                .enumerate()
                .map(|(p, (ratio, amp))| {
                    amp * fade(p, t) * (std::f32::consts::TAU * hz * ratio * t).sin()
                })
                .sum()
        })
        .collect()
}

/// The frequency of the strongest line in `samples`, in hertz, by a
/// parabolic peak on a Blackman-Harris window.
fn peak_hz(samples: &[f32]) -> f32 {
    let n = samples.len().next_power_of_two() / 2;
    let samples = &samples[..n];
    let mut re: Vec<f32> = samples
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let t = std::f32::consts::TAU * i as f32 / n as f32;
            let w =
                0.35875 - 0.48829 * t.cos() + 0.14128 * (2.0 * t).cos() - 0.01168 * (3.0 * t).cos();
            s * w
        })
        .collect();
    let mut im = vec![0.0f32; n];
    fft_in_place(&mut re, &mut im);
    let mag: Vec<f32> = re[..n / 2]
        .iter()
        .zip(&im[..n / 2])
        .map(|(r, i)| (r * r + i * i).sqrt())
        .collect();
    let k = mag
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
        .map(|(i, _)| i)
        .unwrap()
        .clamp(1, n / 2 - 2);
    let (a, b, c) = (mag[k - 1].ln(), mag[k].ln(), mag[k + 1].ln());
    let offset = 0.5 * (a - c) / (a - 2.0 * b + c);
    (k as f32 + offset) * SR / n as f32
}

/// Energy at `hz`, a Hann-windowed DFT.
fn energy_at(samples: &[f32], hz: f32) -> f32 {
    let n = samples.len() as f32;
    let (mut re, mut im) = (0.0f32, 0.0f32);
    for (i, sample) in samples.iter().enumerate() {
        let window = 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / n).cos();
        let phase = std::f32::consts::TAU * hz * i as f32 / SR;
        re += sample * window * phase.cos();
        im -= sample * window * phase.sin();
    }
    (re * re + im * im).sqrt() / n
}

/// `seconds` of a spectral oscillator on `frames` at `note_hz`.
fn play(frames: &SpectralFrames, osc: &SynthOsc, note_hz: f32, seconds: f32) -> Vec<f32> {
    let mut state = SynthState::new();
    state.reset(osc, 1);
    (0..(seconds * SR) as usize)
        .map(|_| {
            state
                .next_sample_from(osc, SynthInput::Spectral(frames), note_hz, SR, 0.0)
                .0
        })
        .collect()
}

fn spectral_osc() -> SynthOsc {
    SynthOsc {
        source: SynthSource::Spectral(0),
        unison: fontelle_dsp::Unison {
            voices: 1,
            ..Default::default()
        },
        random_phase: false,
        ..SynthOsc::default()
    }
}

#[test]
fn a_recorded_sine_comes_back_as_the_same_sine() {
    let sine = recording(440.0, &[(1.0, 0.5)], 1.0, |_, _| 1.0);
    let frames = analyse_spectral(&sine, SR, 440.0);
    assert!(
        frames.frames.len() >= 90,
        "ten-millisecond hops over a second: {}",
        frames.frames.len()
    );
    assert!((frames.hop_s - 0.01).abs() < 0.002);
    // The first partial is the note, at about the recording's level; the
    // rest are nothing.
    let frame = &frames.frames[frames.frames.len() / 2];
    assert!(
        (frame.ratio[0] - 1.0).abs() < 0.002,
        "ratio {}",
        frame.ratio[0]
    );
    assert!((frame.amp[0] - 0.5).abs() < 0.1, "amp {}", frame.amp[0]);
    assert!(
        frame.amp[1..].iter().all(|a| *a < 0.02),
        "{:?}",
        &frame.amp[..8]
    );
    // Played back at 440, it is a sine at 440 within a cent.
    let out = play(&frames, &spectral_osc(), 440.0, 1.0);
    let hz = peak_hz(&out[SR as usize / 4..]);
    let cents = 1_200.0 * (hz / 440.0).log2();
    assert!(cents.abs() < 1.0, "{hz} Hz is {cents:+.2} cents off");
    // And at 330 it is a sine at 330: the partials are ratios of the note.
    let out = play(&frames, &spectral_osc(), 330.0, 1.0);
    let hz = peak_hz(&out[SR as usize / 4..]);
    assert!((1_200.0 * (hz / 330.0).log2()).abs() < 1.0, "{hz} Hz");
}

#[test]
fn a_chords_partials_are_all_present() {
    // Three partials at 1, 2 and 3.02 — a touch stretched, like a string —
    // at 0.5, 0.25 and 0.125.
    let chord = recording(
        220.0,
        &[(1.0, 0.5), (2.0, 0.25), (3.02, 0.125)],
        1.0,
        |_, _| 1.0,
    );
    let frames = analyse_spectral(&chord, SR, 220.0);
    let frame = &frames.frames[frames.frames.len() / 2];
    assert!(
        (frame.amp[0] / frame.amp[1] - 2.0).abs() < 0.3,
        "{:?}",
        &frame.amp[..4]
    );
    assert!(
        (frame.amp[1] / frame.amp[2] - 2.0).abs() < 0.3,
        "{:?}",
        &frame.amp[..4]
    );
    assert!(
        (frame.ratio[2] - 3.02).abs() < 0.01,
        "the stretch is kept: {}",
        frame.ratio[2]
    );
    assert!(frame.amp[3] < frame.amp[2] * 0.1, "nothing at four");
    let out = play(&frames, &spectral_osc(), 220.0, 1.0);
    let tail = &out[SR as usize / 2..];
    let (one, two, three) = (
        energy_at(tail, 220.0),
        energy_at(tail, 440.0),
        energy_at(tail, 220.0 * 3.02),
    );
    assert!((one / two - 2.0).abs() < 0.4, "{one} {two} {three}");
    assert!((two / three - 2.0).abs() < 0.4, "{one} {two} {three}");
    assert!(energy_at(tail, 880.0) < three * 0.1, "nothing at four");
}

#[test]
fn position_scans_the_recordings_own_brightness() {
    // The second partial fades out over the second; the first holds.
    let fading = recording(220.0, &[(1.0, 0.4), (2.0, 0.4)], 1.0, |p, t| {
        if p == 1 { (1.0 - t).max(0.0) } else { 1.0 }
    });
    let frames = analyse_spectral(&fading, SR, 220.0);
    let mut osc = spectral_osc();
    // Frozen, so the frame under the position is what plays.
    osc.warp = WarpMode::Freeze;
    osc.warp_amount = 1.0;
    let brightness = |position: f32| {
        let mut osc = osc;
        osc.position = position;
        let out = play(&frames, &osc, 220.0, 0.5);
        let tail = &out[SR as usize / 4..];
        energy_at(tail, 440.0) / energy_at(tail, 220.0).max(1e-9)
    };
    let start = brightness(0.05);
    let late = brightness(0.9);
    assert!(
        start > 0.7,
        "at the start the second partial is there: {start:.2}"
    );
    assert!(
        late < start * 0.25,
        "late in the recording it has gone: {late:.2} against {start:.2}"
    );
    // Not frozen, from the start, half a second of it: the second partial
    // is going as the recording's does.
    osc.warp_amount = 0.0;
    osc.position = 0.0;
    let out = play(&frames, &osc, 220.0, 0.9);
    let early = &out[..(0.2 * SR) as usize];
    let later = &out[(0.7 * SR) as usize..];
    let ratio = |s: &[f32]| energy_at(s, 440.0) / energy_at(s, 220.0).max(1e-9);
    assert!(
        ratio(later) < ratio(early) * 0.5,
        "{:.2} then {:.2}",
        ratio(early),
        ratio(later)
    );
}

#[test]
fn stretch_spreads_the_partials_and_shift_moves_the_formant() {
    let chord = recording(
        220.0,
        &[(1.0, 0.4), (2.0, 0.3), (3.0, 0.2), (4.0, 0.1)],
        1.0,
        |_, _| 1.0,
    );
    let frames = analyse_spectral(&chord, SR, 220.0);
    let mut osc = spectral_osc();
    osc.warp = WarpMode::Stretch;
    osc.warp_amount = 0.5;
    let out = play(&frames, &osc, 220.0, 0.6);
    let tail = &out[SR as usize / 4..];
    // At half, the spread is one and a half: the second partial sits at
    // 1 + 1.5 = 2.5 times the note, the third at 4.
    assert!(
        energy_at(tail, 220.0 * 2.5) > energy_at(tail, 440.0) * 4.0,
        "the second partial moved up"
    );
    assert!(
        energy_at(tail, 220.0 * 4.0) > energy_at(tail, 220.0 * 3.0) * 4.0,
        "and the third"
    );
    // Shift at full moves the spectral envelope up an octave: each
    // partial wears the level of the one at half its number, so the
    // fourth is as loud as the second was and the second as the first.
    osc.warp = WarpMode::Shift;
    osc.warp_amount = 1.0;
    let out = play(&frames, &osc, 220.0, 0.6);
    let tail = &out[SR as usize / 4..];
    let plain = play(&frames, &spectral_osc(), 220.0, 0.6);
    let plain = &plain[SR as usize / 4..];
    let fourth = energy_at(tail, 880.0) / energy_at(plain, 440.0).max(1e-9);
    assert!(
        (0.6..=1.6).contains(&fourth),
        "the fourth partial wears the second's level: {fourth:.2}"
    );
    let second = energy_at(tail, 440.0) / energy_at(plain, 220.0).max(1e-9);
    assert!(
        (0.6..=1.6).contains(&second),
        "the second wears the first's: {second:.2}"
    );
}
