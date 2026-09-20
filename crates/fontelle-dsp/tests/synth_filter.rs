//! The six filter models Phase 4 adds (`docs/flopsynth-next.md` §4.4),
//! each measured against what it claims to be — and the three claims every
//! model makes: it is a different filter from the other nine, it
//! self-oscillates (where it can) at its cutoff within two cents, and it
//! makes no NaN at full drive.
//!
//! `tests/filters.rs` holds the first four models and the drawn response;
//! this file holds the new six.

use fontelle_dsp::{
    FilterModel, FilterSlope, SvfMode, SynthFilter, SynthFilterSettings, fft_in_place,
};

const SR: f32 = 48_000.0;

fn settings(model: FilterModel, cutoff: f32) -> SynthFilterSettings {
    SynthFilterSettings {
        model,
        mode: SvfMode::Lowpass,
        slope: FilterSlope::Db12,
        cutoff_hz: cutoff,
        resonance: 0.0,
        drive: 0.0,
        character: 0.0,
        oversampling: fontelle_dsp::Oversampling::Off,
    }
}

/// The settled gain at `hz`, in dB: a sine of `level` in, the peak of the
/// last fifth out.
fn response_at(config: &SynthFilterSettings, hz: f32, level: f32) -> f32 {
    let mut filter = SynthFilter::new();
    let frames = ((SR / hz) * 400.0) as usize;
    let settle = frames * 4 / 5;
    let mut peak = 0.0f32;
    for i in 0..frames {
        let x = (std::f32::consts::TAU * hz * i as f32 / SR).sin() * level;
        let y = filter.process(x, config, SR);
        if i >= settle {
            peak = peak.max(y.abs());
        }
    }
    20.0 * (peak / level).max(1e-9).log10()
}

fn response_db(config: &SynthFilterSettings, hz: f32) -> f32 {
    response_at(config, hz, 0.5)
}

/// The best response within a quarter-tone either side of `hz`, so a peak
/// that pre-warping moved a few cents is still found.
fn peak_near(config: &SynthFilterSettings, hz: f32) -> f32 {
    (-3..=3)
        .map(|step| response_db(config, hz * 2f32.powf(step as f32 / 24.0)))
        .fold(f32::NEG_INFINITY, f32::max)
}

/// Energy at `hz` in `samples`, a Hann-windowed DFT.
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

/// A second of the filter ringing on its own after one sample of impulse:
/// the last half, and its strongest line in hertz (a parabolic peak on a
/// Blackman-Harris window).
fn self_oscillation_hz(config: &SynthFilterSettings) -> (f32, f32) {
    let mut filter = SynthFilter::new();
    let frames = SR as usize;
    let mut out = Vec::with_capacity(frames);
    let mut peak = 0.0f32;
    for i in 0..frames {
        let x = if i == 0 { 0.5 } else { 0.0 };
        let y = filter.process(x, config, SR);
        peak = peak.max(y.abs());
        out.push(y);
    }
    let tail = &out[frames / 2..];
    let n = 16_384;
    let mut re: Vec<f32> = tail[..n]
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
    let rms = (tail.iter().map(|s| s * s).sum::<f32>() / tail.len() as f32).sqrt();
    assert!(
        rms > 0.02,
        "{:?} at full resonance has to still be ringing after half a second: RMS {rms}",
        config.model
    );
    assert!(peak < 4.0, "{:?} ran away: peak {peak}", config.model);
    ((k as f32 + offset) * SR / n as f32, peak)
}

const NEW_MODELS: [FilterModel; 6] = [
    FilterModel::Diode,
    FilterModel::Sallen,
    FilterModel::Phaser,
    FilterModel::Vowel,
    FilterModel::Ring,
    FilterModel::Dual,
];

#[test]
fn the_six_new_models_are_appended_and_each_names_its_character() {
    assert_eq!(FilterModel::ALL.len(), 10);
    assert_eq!(&FilterModel::ALL[4..], &NEW_MODELS);
    for model in NEW_MODELS {
        assert!(
            model.character_label().is_some(),
            "{model:?} has a character knob"
        );
    }
}

/// Every one of the ten is a different filter from every other, at the
/// same four probes `filters.rs` uses — quietly, because the Sallen-Key's
/// whole character is that it squashes its resonance at level, and at
/// half scale it reads as a clean two-pole.
#[test]
fn every_model_is_a_different_filter() {
    let describe = |model: FilterModel| {
        let mut config = settings(model, 800.0);
        config.resonance = 0.8;
        config.character = 0.5;
        [220.0f32, 800.0, 3_000.0, 9_000.0].map(|hz| response_at(&config, hz, 0.02))
    };
    let described: Vec<_> = FilterModel::ALL
        .iter()
        .map(|m| (*m, describe(*m)))
        .collect();
    for (i, (a_model, a)) in described.iter().enumerate() {
        for (b_model, b) in &described[i + 1..] {
            let apart = a
                .iter()
                .zip(b)
                .map(|(x, y)| (x - y).abs())
                .fold(0.0f32, f32::max);
            assert!(
                apart > 3.0,
                "{a_model:?} and {b_model:?} are the same filter ({apart} dB apart \
                 at the furthest of four probes): {a:?} {b:?}"
            );
        }
    }
}

/// The models with a loop that can ring — the two ladders and the Sallen-Key
/// — ring at their cutoff, within two cents, at the top of the knob. Not
/// "somewhere near it": a self-oscillating filter is played as an
/// oscillator, with key tracking at 100 %, and two cents is where a
/// listener stops hearing a beat against the real one.
#[test]
fn a_self_oscillating_model_rings_at_its_cutoff_within_two_cents() {
    for model in [FilterModel::Ladder, FilterModel::Diode, FilterModel::Sallen] {
        for cutoff in [220.0f32, 880.0, 2_640.0] {
            let mut config = settings(model, cutoff);
            config.resonance = 1.0;
            config.character = 0.5;
            let (hz, _) = self_oscillation_hz(&config);
            let cents = 1_200.0 * (hz / cutoff).log2();
            assert!(
                cents.abs() < 2.0,
                "{model:?} at {cutoff} Hz rings at {hz:.2} Hz, {cents:+.2} cents off"
            );
        }
    }
}

/// Nothing produces a NaN, or runs away, with every knob at the top and a
/// loud saw going in.
#[test]
fn no_model_makes_a_nan_at_full_drive() {
    for model in FilterModel::ALL {
        for mode in [SvfMode::Lowpass, SvfMode::Bandpass, SvfMode::Highpass] {
            let mut config = settings(model, 600.0);
            config.mode = mode;
            config.resonance = 1.0;
            config.drive = 1.0;
            config.character = 1.0;
            let mut filter = SynthFilter::new();
            let mut peak = 0.0f32;
            for i in 0..SR as usize {
                let phase = (i as f32 * 110.0 / SR).fract();
                let x = (2.0 * phase - 1.0) * 1.5;
                let y = filter.process(x, &config, SR);
                assert!(y.is_finite(), "{model:?} {mode:?} made {y} at sample {i}");
                peak = peak.max(y.abs());
            }
            assert!(peak < 12.0, "{model:?} {mode:?} ran away: peak {peak}");
        }
    }
}

// ------------------------------------------------------------- Diode ---

/// The 303's ladder: four poles like the transistor one, but its bass goes
/// as the resonance comes up — the trait every acid line is built on — and
/// its loop clips one way harder than the other.
#[test]
fn the_diode_ladder_thins_its_bass_as_resonance_rises() {
    let mut quiet = settings(FilterModel::Diode, 800.0);
    quiet.resonance = 0.0;
    let mut resonant = quiet;
    // Short of self-oscillation (the loop rings on its own from 0.89), so
    // the measurement is of the filter and not of its limit cycle.
    resonant.resonance = 0.7;
    // At rest, a ladder: 24 dB an octave in the stop band.
    let slope = response_db(&quiet, 3_200.0) - response_db(&quiet, 6_400.0);
    assert!(
        (slope - 24.0).abs() < 4.0,
        "four poles fall 24 dB an octave: {slope:.1}"
    );
    // The bass, two octaves below the corner, is lower with resonance up.
    let bass_drop = response_db(&quiet, 200.0) - response_db(&resonant, 200.0);
    assert!(
        bass_drop > 3.0,
        "the diode ladder loses its bass with resonance: only {bass_drop:.1} dB"
    );
    // Which the transistor ladder does not: the difference is the model.
    let mut ladder_quiet = quiet;
    ladder_quiet.model = FilterModel::Ladder;
    let mut ladder_resonant = resonant;
    ladder_resonant.model = FilterModel::Ladder;
    let ladder_drop = response_db(&ladder_quiet, 200.0) - response_db(&ladder_resonant, 200.0);
    assert!(
        ladder_drop < bass_drop - 2.0,
        "the transistor ladder keeps its bass: {ladder_drop:.1} against {bass_drop:.1}"
    );
}

/// The diode's clip is asymmetric, so a loud tone through a resonant diode
/// ladder gains a second harmonic the transistor ladder's symmetric `tanh`
/// cannot make.
#[test]
fn the_diode_ladders_clip_makes_even_harmonics() {
    let even_ratio = |model: FilterModel| {
        let mut config = settings(model, 1_000.0);
        config.resonance = 0.85;
        config.character = 1.0;
        let mut filter = SynthFilter::new();
        let out: Vec<f32> = (0..SR as usize)
            .map(|i| {
                let x = (std::f32::consts::TAU * 250.0 * i as f32 / SR).sin() * 1.0;
                filter.process(x, &config, SR)
            })
            .collect();
        let tail = &out[SR as usize / 2..];
        energy_at(tail, 500.0) / energy_at(tail, 250.0).max(1e-9)
    };
    let diode = even_ratio(FilterModel::Diode);
    let ladder = even_ratio(FilterModel::Ladder);
    assert!(
        diode > 0.03,
        "a diode ladder driven hard has a second harmonic: {diode:.4}"
    );
    assert!(
        diode > ladder * 4.0,
        "and the transistor ladder has far less of one: {ladder:.4} against {diode:.4}"
    );
}

// ------------------------------------------------------------ Sallen ---

/// The MS-20's filter: two poles with the clipper in the loop, so a loud
/// signal at the corner is squashed where a quiet one is boosted — the
/// resonance itself distorts, which is the whole character.
#[test]
fn the_sallen_key_squashes_its_resonance_when_driven() {
    let mut config = settings(FilterModel::Sallen, 1_000.0);
    config.resonance = 0.9;
    config.character = 1.0;
    let quiet = response_at(&config, 1_000.0, 0.02);
    let loud = response_at(&config, 1_000.0, 1.0);
    assert!(
        quiet > 10.0,
        "a quiet tone at the corner is boosted by the resonance: {quiet:.1} dB"
    );
    assert!(
        loud < quiet - 6.0,
        "a loud one has its peak clipped away: {loud:.1} dB against {quiet:.1}"
    );
    // The character knob is how hard the clipper is: softer, the squash is
    // less.
    config.character = 0.0;
    let loud_soft = response_at(&config, 1_000.0, 1.0);
    assert!(
        loud_soft > loud + 2.0,
        "a softer clipper squashes less: {loud_soft:.1} against {loud:.1}"
    );
    // And it is two poles: 12 dB an octave.
    config.resonance = 0.0;
    let slope = response_db(&config, 4_000.0) - response_db(&config, 8_000.0);
    assert!(
        (slope - 12.0).abs() < 3.0,
        "two poles fall 12 dB an octave: {slope:.1}"
    );
}

// ------------------------------------------------------------ Phaser ---

/// N first-order all-passes mixed with the dry signal: a notch wherever the
/// chain's phase reaches an odd half-turn. Four stages at 1 kHz put the
/// first at `tan(π/8)` of the corner and the second at `tan(3π/8)`; twelve
/// stages put the first at `tan(π/24)`.
#[test]
fn the_phaser_filter_notches_where_its_stages_reach_half_a_turn() {
    let mut four = settings(FilterModel::Phaser, 1_000.0);
    four.character = 0.0;
    let tan = |turns: f32| (turns * std::f32::consts::PI).tan();
    let first = 1_000.0 * tan(1.0 / 8.0);
    let second = 1_000.0 * tan(3.0 / 8.0);
    let at_first = (-2..=2)
        .map(|s| response_db(&four, first * 2f32.powf(s as f32 / 48.0)))
        .fold(f32::INFINITY, f32::min);
    let at_second = (-2..=2)
        .map(|s| response_db(&four, second * 2f32.powf(s as f32 / 48.0)))
        .fold(f32::INFINITY, f32::min);
    let at_corner = response_db(&four, 1_000.0);
    assert!(
        at_first < -20.0 && at_second < -20.0,
        "notches at {first:.0} and {second:.0} Hz: {at_first:.1} and {at_second:.1} dB"
    );
    assert!(
        at_corner.abs() < 1.0,
        "the corner is where the chain is back in phase: {at_corner:.1} dB"
    );
    // Twelve stages: the first notch comes down to tan(π/24) of the corner,
    // where four stages passed everything.
    let mut twelve = four;
    twelve.character = 1.0;
    let low = 1_000.0 * tan(1.0 / 24.0);
    let twelve_at_low = (-2..=2)
        .map(|s| response_db(&twelve, low * 2f32.powf(s as f32 / 48.0)))
        .fold(f32::INFINITY, f32::min);
    assert!(
        twelve_at_low < -20.0 && response_db(&four, low) > -3.0,
        "twelve stages notch at {low:.0} Hz ({twelve_at_low:.1} dB) where four do not \
         ({:.1} dB)",
        response_db(&four, low)
    );
    // Resonance is feedback round the chain: the peaks between the notches
    // rise.
    let mut fed = four;
    fed.resonance = 0.8;
    assert!(
        response_db(&fed, 1_000.0) > at_corner + 3.0,
        "feedback lifts the peak: {:.1} against {at_corner:.1}",
        response_db(&fed, 1_000.0)
    );
}

// ------------------------------------------------------------- Vowel ---

/// Five formants from the singing-voice table rather than three from the
/// spoken one, with each formant's own bandwidth; the cutoff is the throat
/// — an octave up is a smaller one, which is the gender.
#[test]
fn the_vowel_filter_puts_five_formants_where_the_table_does() {
    // /a/ at character 0: F1 650, F2 1080, F3 2650 in the tenor table.
    let mut config = settings(FilterModel::Vowel, 1_000.0);
    config.character = 0.0;
    config.resonance = 0.5;
    let (f1, f2, f3) = (650.0f32, 1_080.0f32, 2_650.0f32);
    let at = [f1, f2, f3].map(|hz| peak_near(&config, hz));
    // Between F2 and F3 there is nothing.
    let trough = response_db(&config, 1_700.0);
    for (hz, level) in [f1, f2, f3].iter().zip(at) {
        assert!(
            level > trough + 6.0,
            "a formant at {hz:.0} Hz: {level:.1} dB against {trough:.1} between"
        );
    }
    // The throat: at 1.5 kHz the first formant sits at 975, not 650.
    let mut smaller = config;
    smaller.cutoff_hz = 1_500.0;
    assert!(
        peak_near(&smaller, 975.0) > peak_near(&smaller, 650.0) + 3.0,
        "a smaller throat moves F1 up: {:.1} at 975 against {:.1} at 650",
        peak_near(&smaller, 975.0),
        peak_near(&smaller, 650.0)
    );
    // The morph: /i/ at 0.5 has F1 at 290 and F2 at 1870.
    let mut i = config;
    i.character = 0.5;
    let between = response_db(&i, 800.0);
    assert!(
        peak_near(&i, 290.0) > between + 6.0 && peak_near(&i, 1_870.0) > between + 6.0,
        "/i/: {:.1} at 290, {:.1} at 1870, {between:.1} between",
        peak_near(&i, 290.0),
        peak_near(&i, 1_870.0)
    );
}

// -------------------------------------------------------------- Ring ---

/// A ring modulator as a filter: the input times a sine at the cutoff, so
/// a note comes out as its sum and difference with the corner. Resonance
/// is the mix, character hardens the sine towards a square.
#[test]
fn the_ring_filter_makes_sidebands_at_the_cutoff_either_side_of_the_note() {
    let render = |config: &SynthFilterSettings| -> Vec<f32> {
        let mut filter = SynthFilter::new();
        (0..SR as usize)
            .map(|i| {
                let x = (std::f32::consts::TAU * 1_000.0 * i as f32 / SR).sin() * 0.5;
                filter.process(x, config, SR)
            })
            .collect()
    };
    let mut config = settings(FilterModel::Ring, 300.0);
    config.resonance = 1.0;
    let out = render(&config);
    let tail = &out[SR as usize / 2..];
    let (note, below, above) = (
        energy_at(tail, 1_000.0),
        energy_at(tail, 700.0),
        energy_at(tail, 1_300.0),
    );
    assert!(
        below > note * 10.0 && above > note * 10.0,
        "fully wet, the note is gone and its sidebands are there: {note:.4} / {below:.4} / \
         {above:.4}"
    );
    // Dry at the bottom of the mix.
    config.resonance = 0.0;
    let out = render(&config);
    let tail = &out[SR as usize / 2..];
    assert!(
        energy_at(tail, 1_000.0) > energy_at(tail, 700.0) * 10.0,
        "at mix 0 the note passes"
    );
    // A harder modulator has a third harmonic, and so a second pair of
    // sidebands at 1000 ± 900.
    config.resonance = 1.0;
    config.character = 1.0;
    let out = render(&config);
    let tail = &out[SR as usize / 2..];
    let soft = render(&{
        let mut c = config;
        c.character = 0.0;
        c
    });
    let soft = &soft[SR as usize / 2..];
    assert!(
        energy_at(tail, 1_900.0) > energy_at(soft, 1_900.0) * 4.0,
        "a squarer modulator adds the third harmonic's sidebands: {:.4} against {:.4}",
        energy_at(tail, 1_900.0),
        energy_at(soft, 1_900.0)
    );
}

// -------------------------------------------------------------- Dual ---

/// Two SVFs at cutoff ± spread, summed. The spread is the character; the
/// shape and the Q are the clean filter's.
#[test]
fn the_dual_filter_is_two_corners_a_spread_apart() {
    let mut config = settings(FilterModel::Dual, 1_000.0);
    config.mode = SvfMode::Bandpass;
    config.resonance = 8.0;
    // Half the knob is an octave each side.
    config.character = 0.5;
    let (low, high, middle) = (
        peak_near(&config, 500.0),
        peak_near(&config, 2_000.0),
        response_db(&config, 1_000.0),
    );
    assert!(
        low > middle + 3.0 && high > middle + 3.0,
        "peaks at 500 and 2000 Hz with a trough between: {low:.1} / {middle:.1} / {high:.1}"
    );
    // No spread: one band at the cutoff.
    config.character = 0.0;
    assert!(
        response_db(&config, 1_000.0) > response_db(&config, 500.0) + 6.0,
        "with no spread the two coincide at the cutoff"
    );
    // The shape is the SVF's: a low-pass pair falls off past the higher
    // corner.
    config.mode = SvfMode::Lowpass;
    config.resonance = 0.707;
    config.character = 0.5;
    // An octave above the higher corner, and short of where the warp
    // towards Nyquist steepens everything.
    let slope = response_db(&config, 4_000.0) - response_db(&config, 8_000.0);
    assert!(
        (slope - 12.0).abs() < 4.0,
        "past both corners the pair falls 12 dB an octave: {slope:.1}"
    );
}
