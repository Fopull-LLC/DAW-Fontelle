//! The distortion's DSP, measured as harmonics (TDD §13.4,
//! `docs/effects-catalogue.md` §3.1).
//!
//! A distortion is the one effect whose whole purpose is to put energy at
//! frequencies that were not in the input, so every test here measures a
//! **harmonic** rather than a level: a drive knob wired to the output gain
//! also makes things louder, and only a spectrum tells the two apart.
//!
//! # Why the oversampling test is the important one
//!
//! Clipping a 7 kHz tone produces a 49 kHz seventh harmonic. At a 48 kHz
//! sample rate that frequency does not exist, so it comes back as 1 kHz —
//! an *inharmonic* tone, unrelated to anything played, which is what makes a
//! cheap distortion sound like a broken radio rather than an amplifier.
//! Running the waveshaper at a multiple of the rate and filtering before
//! coming back down is the fix, and `oversampling_keeps_the_aliases_out` is
//! the test that says it works. It is also the only test here that would pass
//! just as happily against a waveshaper with the oversampling stripped out,
//! if it were written as a level check instead of a spectrum one.
//!
//! # What the second half of this file is for
//!
//! *"I'm finding it hard to get more than a basic distortion sound."* Five
//! curves, a drive and a tone is a pedal. The stages a pedal's circuit puts
//! around its clipper — the voicing before it, the sag under it, the bias
//! across it — are what make one box a family of sounds, and each of them
//! is measured here by the thing it should change **and** by what it should
//! leave alone. A stage that only made things louder or darker would pass
//! half of those.

use fontelle_fx::Distortion;
use fontelle_types::{DistortionConfig, DistortionCurve, DistortionPreset, Oversampling};

const SR: f32 = 48_000.0;

/// Half a second, and an exact number of cycles of every frequency used here,
/// so a rectangular-windowed bin does not leak into its neighbours.
const FRAMES: usize = 33_600;

/// Where the analysis starts: past the filters' transient, and a whole number
/// of cycles of everything used here, so the window still starts on a zero
/// phase.
const SETTLE: usize = 4_800;
const WINDOW: usize = 24_000;

fn sine(amplitude: f32, freq: f32, frames: usize) -> Vec<f32> {
    (0..frames)
        .map(|i| amplitude * (std::f32::consts::TAU * freq * i as f32 / SR).sin())
        .collect()
}

fn peak(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0f32, |m, s| m.max(s.abs()))
}

fn rms(samples: &[f32]) -> f32 {
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
}

/// The amplitude of the component at `freq`, by direct evaluation of the one
/// DFT bin that matters.
///
/// One bin rather than a whole FFT because these tests each ask about two or
/// three known frequencies, and a bin is six lines that cannot be wrong about
/// which frequency it measured.
fn amplitude_at(samples: &[f32], freq: f32) -> f32 {
    let n = samples.len();
    let (mut re, mut im) = (0.0f64, 0.0f64);
    for (i, sample) in samples.iter().enumerate() {
        let phase = std::f64::consts::TAU * freq as f64 * i as f64 / SR as f64;
        re += *sample as f64 * phase.cos();
        im += *sample as f64 * phase.sin();
    }
    (2.0 * (re * re + im * im).sqrt() / n as f64) as f32
}

fn through(config: &DistortionConfig, amplitude: f32, freq: f32) -> Vec<f32> {
    let mut distortion = Distortion::new();
    distortion.prepare(SR);
    let mut left = sine(amplitude, freq, FRAMES);
    let mut right = left.clone();
    distortion.process(&mut [&mut left, &mut right], config);
    left
}

/// The analysed part of a run: past the transient, a whole number of cycles.
fn analysed(config: &DistortionConfig, amplitude: f32, freq: f32) -> Vec<f32> {
    let out = through(config, amplitude, freq);
    out[SETTLE..SETTLE + WINDOW].to_vec()
}

/// Soft clipping, no drive, tone wide open, unity out, and every new stage at
/// rest. The baseline every test moves one knob away from.
///
/// Auto-gain is **off** here, because half of what follows measures a level
/// and wants the curve's own.
fn clean() -> DistortionConfig {
    let mut config = DistortionConfig::new();
    config.auto_gain = false;
    config
}

// --------------------------------------------------------- the old claims

#[test]
fn a_quiet_signal_at_no_drive_comes_through_almost_untouched() {
    // Not "is a wire" — a soft clipper is curved everywhere, so it never is.
    // But the bottom of its curve is straight to within a fraction of a
    // percent, which is what makes a drive knob at zero a sensible place to
    // start rather than a tone already committed to.
    let out = analysed(&clean(), 0.1, 1_000.0);
    let level = amplitude_at(&out, 1_000.0);
    assert!(
        (level - 0.1).abs() < 0.005,
        "a quiet sine should pass at its own level; got {level}"
    );
    let third = amplitude_at(&out, 3_000.0);
    assert!(third < 0.002, "and with nothing much added; got {third}");
}

#[test]
fn a_fresh_distortion_with_auto_gain_is_still_a_wire() {
    // The default has auto-gain on. At no drive there is nothing to
    // compensate, and the compensation must know that.
    let out = analysed(&DistortionConfig::new(), 0.1, 1_000.0);
    let level = amplitude_at(&out, 1_000.0);
    assert!(
        (level - 0.1).abs() < 0.005,
        "auto-gain moved a signal that had no drive on it; got {level}"
    );
}

#[test]
fn drive_adds_harmonics_that_were_not_there() {
    // The knob's actual job. A drive that only raised the level would pass a
    // peak test and fail this one, which is why this is the test.
    let mut driven = clean();
    driven.drive_db = 30.0;
    let third = amplitude_at(&analysed(&driven, 0.1, 1_000.0), 3_000.0);
    assert!(
        third > 0.02,
        "30 dB of drive should build a third harmonic; got {third}"
    );
}

#[test]
fn hard_clipping_flattens_the_top() {
    let mut config = clean();
    config.curve = DistortionCurve::HardClip;
    config.drive_db = 24.0;
    let out = through(&config, 1.0, 220.0);
    // Its ceiling, plus what an eighth-order filter rings by on a corner: a
    // band-limited square overshoots, and clamping that back at the base
    // rate would be the aliasing the oversampling is there to prevent.
    assert!(
        peak(&out) < 1.2,
        "a hard clipper should stop at its ceiling; got {}",
        peak(&out)
    );
    let flat = out[SETTLE..SETTLE + WINDOW]
        .iter()
        .filter(|s| s.abs() > 0.95)
        .count();
    assert!(
        flat > WINDOW / 3,
        "most of a hard-clipped sine should be sitting on the rail; {flat} samples were"
    );
}

#[test]
fn every_curve_does_something_and_they_are_not_all_the_same_thing() {
    // Ten curves in the chooser and one waveshaper behind them would be ten
    // names for one sound — which is what a `match` with a copy-pasted arm
    // produces, and what nothing else here would catch.
    let mut shapes = Vec::new();
    for curve in DistortionCurve::ALL {
        let mut config = clean();
        config.curve = curve;
        config.drive_db = 24.0;
        let out = analysed(&config, 0.5, 1_000.0);
        // A second or a third: the rectifier's harmonics are even.
        let added = amplitude_at(&out, 3_000.0).max(amplitude_at(&out, 2_000.0));
        assert!(added > 0.005, "{curve:?} produced no harmonics at all");
        shapes.push((curve, out));
    }
    for (i, (curve, a)) in shapes.iter().enumerate() {
        for (other, b) in &shapes[i + 1..] {
            let difference = a
                .iter()
                .zip(b.iter())
                .fold(0.0f32, |m, (x, y)| m.max((x - y).abs()));
            assert!(
                difference > 0.01,
                "{curve:?} and {other:?} are the same curve"
            );
        }
    }
}

#[test]
fn folding_turns_back_on_itself() {
    // What distinguishes a wavefolder from every other curve here: past the
    // fold point, *more* input gives *less* output. Clipping saturates; this
    // reverses, which is why it is the one curve that sounds like a synth
    // rather than an amplifier.
    let mut config = clean();
    config.curve = DistortionCurve::Fold;
    config.drive_db = 12.0;
    // Measured at the fundamental, not as a peak: a folded waveform still
    // touches full scale somewhere in every cycle, because the fold sweeps
    // through the whole curve on its way up. What collapses is the tone —
    // which is what a fold *is*.
    let quiet = amplitude_at(&analysed(&config, 0.25, 220.0), 220.0);
    let loud = amplitude_at(&analysed(&config, 1.0, 220.0), 220.0);
    assert!(
        loud < quiet * 0.5,
        "a fourfold louder input should come back down: {quiet} then {loud}"
    );
}

#[test]
fn the_tone_control_rolls_off_the_top() {
    let mut open = clean();
    open.drive_db = 18.0;
    let mut dark = open;
    dark.tone_hz = 500.0;
    let bright = amplitude_at(&analysed(&open, 0.5, 8_000.0), 8_000.0);
    let dulled = amplitude_at(&analysed(&dark, 0.5, 8_000.0), 8_000.0);
    assert!(
        dulled < bright * 0.2,
        "an 8 kHz tone should not survive a 500 Hz tone control: {bright} then {dulled}"
    );
}

#[test]
fn the_output_gain_moves_the_level_and_not_the_shape() {
    // The knob that makes a distortion usable: drive is a tone control and
    // this is the volume, so turning one must not be the other.
    let mut unity = clean();
    unity.drive_db = 24.0;
    let mut quieter = unity;
    quieter.output_db = -12.0;

    let loud = analysed(&unity, 0.5, 1_000.0);
    let soft = analysed(&quieter, 0.5, 1_000.0);
    let ratio = amplitude_at(&soft, 1_000.0) / amplitude_at(&loud, 1_000.0);
    assert!(
        (ratio - 0.251).abs() < 0.01,
        "-12 dB should be a quarter of the amplitude; got {ratio}"
    );
    // And the harmonic content, relative to the fundamental, is unchanged.
    let before = amplitude_at(&loud, 3_000.0) / amplitude_at(&loud, 1_000.0);
    let after = amplitude_at(&soft, 3_000.0) / amplitude_at(&soft, 1_000.0);
    assert!(
        (before - after).abs() < 0.01,
        "the output gain changed the tone: {before} then {after}"
    );
}

#[test]
fn oversampling_keeps_the_aliases_out() {
    // See the module comment. Clipping 7 kHz puts a seventh harmonic at
    // 49 kHz, which at this sample rate is a 1 kHz tone that nobody played.
    let mut config = clean();
    config.curve = DistortionCurve::HardClip;
    config.drive_db = 30.0;

    config.oversample = Oversampling::Off;
    let raw = amplitude_at(&analysed(&config, 0.5, 7_000.0), 1_000.0);
    config.oversample = Oversampling::Two;
    let filtered = amplitude_at(&analysed(&config, 0.5, 7_000.0), 1_000.0);

    assert!(
        raw > 1e-3,
        "the un-oversampled path should alias — if it does not, this test is \
         not measuring what it thinks it is; got {raw}"
    );
    assert!(
        filtered < raw * 0.3,
        "oversampling should take most of the alias out: {raw} then {filtered}"
    );
}

/// Everything in the output of a clipped 7 kHz tone that is not 7 kHz or its
/// one in-band harmonic at 21 kHz got there by folding — the alias, as one
/// number.
fn alias_energy(config: &DistortionConfig) -> f32 {
    let out = analysed(config, 0.5, 7_000.0);
    let total = rms(&out);
    let wanted =
        amplitude_at(&out, 7_000.0).powi(2) / 2.0 + amplitude_at(&out, 21_000.0).powi(2) / 2.0;
    (total * total - wanted).max(0.0).sqrt()
}

#[test]
fn more_oversampling_is_less_alias() {
    // The chooser's positions have to be a ladder, or the expensive ones are
    // a cost with no sound. Two times catches the seventh harmonic; the
    // ninth and eleventh, at 63 and 77 kHz, fold back inside a 96 kHz rate
    // and need the next rung.
    let mut config = clean();
    config.curve = DistortionCurve::HardClip;
    config.drive_db = 30.0;
    let at = |oversample: Oversampling| {
        let mut config = config;
        config.oversample = oversample;
        alias_energy(&config)
    };
    let (off, two, four, eight) = (
        at(Oversampling::Off),
        at(Oversampling::Two),
        at(Oversampling::Four),
        at(Oversampling::Eight),
    );
    assert!(
        two < off * 0.5,
        "2x should halve the alias at least: {off} then {two}"
    );
    assert!(
        four < two * 0.5,
        "4x should halve it again: {two} then {four}"
    );
    assert!(
        eight <= four * 1.05,
        "8x should not be worse than 4x: {four} then {eight}"
    );
}

#[test]
fn oversampling_keeps_the_harmonics_that_belong() {
    // The other half: a decimation filter set too low would pass the test
    // above by removing the wanted third harmonic along with the alias.
    for oversample in [Oversampling::Two, Oversampling::Four, Oversampling::Eight] {
        let mut config = clean();
        config.drive_db = 30.0;
        config.oversample = oversample;
        let out = analysed(&config, 0.2, 1_000.0);
        assert!(
            amplitude_at(&out, 3_000.0) > 0.02,
            "{oversample:?}: the third harmonic of a 1 kHz tone is well inside the band"
        );
        assert!(
            amplitude_at(&out, 5_000.0) > 0.005,
            "{oversample:?}: and so is the fifth"
        );
    }
}

#[test]
fn it_never_produces_a_sample_that_is_not_a_number_or_past_its_output_gain() {
    // Every curve, at every corner of its shape, at the top of the drive
    // range, on a signal already past full scale — which is what an insert
    // after a lifted EQ sees. Bounded means bounded: nothing leaves the
    // shaper above 1.0, and what leaves the effect is that, its output gain,
    // and the ringing of the band-limiting — which on a folder driven this
    // hard is wideband, and is what the wide net after the DC blocker holds.
    let ceiling = 10f32.powf(12.0 / 20.0) * 1.5 * 1.01;
    for curve in DistortionCurve::ALL {
        for shape in [0.0, 0.5, 1.0] {
            let mut config = clean();
            config.curve = curve;
            config.shape = shape;
            config.drive_db = 48.0;
            config.bias = 0.6;
            config.output_db = 12.0;
            let out = through(&config, 4.0, 120.0);
            assert!(
                out.iter().all(|s| s.is_finite()),
                "{curve:?} at shape {shape} produced a non-finite sample"
            );
            assert!(
                peak(&out) <= ceiling,
                "{curve:?} at shape {shape} left the shaper above full scale: {}",
                peak(&out)
            );
        }
    }
}

#[test]
fn reset_forgets_the_filters() {
    let mut config = clean();
    config.tone_hz = 500.0;
    config.drive_db = 24.0;
    let mut distortion = Distortion::new();
    distortion.prepare(SR);
    let mut left = sine(1.0, 220.0, 4_800);
    let mut right = left.clone();
    distortion.process(&mut [&mut left, &mut right], &config);

    distortion.reset();
    let mut left = vec![0.0; 4_800];
    let mut right = vec![0.0; 4_800];
    distortion.process(&mut [&mut left, &mut right], &config);
    assert!(
        peak(&left) < 1e-6,
        "a reset distortion still had signal in its filters"
    );
}

// ------------------------------------------------- the family behind each

#[test]
fn shape_moves_every_curve() {
    // The knob that turns ten curves into ten families. A curve on which
    // `shape` does nothing is a chooser position with a dead knob under it.
    for curve in DistortionCurve::ALL {
        let mut low = clean();
        low.curve = curve;
        low.drive_db = 24.0;
        low.shape = 0.0;
        let mut high = low;
        high.shape = 1.0;
        let a = analysed(&low, 0.5, 1_000.0);
        let b = analysed(&high, 0.5, 1_000.0);
        let difference = a
            .iter()
            .zip(b.iter())
            .fold(0.0f32, |m, (x, y)| m.max((x - y).abs()));
        assert!(
            difference > 0.01,
            "{curve:?}: shape 0 and shape 100 are the same sound"
        );
    }
}

#[test]
fn soft_clips_shape_is_how_hard_it_clips() {
    // At the soft end the curve rounds over a wide region and the harmonic
    // series stops early; at the hard end it corners and the series goes on.
    // So hardness is measured where the series *ends* — the eleventh to the
    // nineteenth, summed, because a clipped sine's series has nulls that
    // move with the clip level and one bin can land in one — at a drive
    // that puts the peaks twice over the ceiling. Much harder and every
    // soft clip is a square wave, which is true and not the claim.
    let mut soft = clean();
    soft.drive_db = 12.0;
    soft.shape = 0.0;
    let mut hard = soft;
    hard.shape = 1.0;
    let high = |config: &DistortionConfig| {
        let out = analysed(config, 0.5, 1_000.0);
        [11_000.0, 13_000.0, 15_000.0, 17_000.0, 19_000.0]
            .iter()
            .map(|f| amplitude_at(&out, *f))
            .sum::<f32>()
    };
    let (soft_high, hard_high) = (high(&soft), high(&hard));
    assert!(
        hard_high > soft_high * 2.0,
        "a harder clip should keep more of the high harmonics: {soft_high} then {hard_high}"
    );
}

#[test]
fn bias_puts_even_harmonics_in_and_leaves_no_dc_behind() {
    // A symmetric curve makes only odd harmonics. Shifting the operating
    // point breaks the symmetry, which is even harmonics on any curve — and
    // the shift has to come back out, or every distorted track carries a DC
    // offset into the mixer.
    let mut centred = clean();
    centred.drive_db = 24.0;
    let mut biased = centred;
    biased.bias = 0.5;

    let second_centred = amplitude_at(&analysed(&centred, 0.5, 1_000.0), 2_000.0);
    let out = analysed(&biased, 0.5, 1_000.0);
    let second_biased = amplitude_at(&out, 2_000.0);
    assert!(
        second_centred < 0.005,
        "a symmetric soft clip should make no second harmonic; got {second_centred}"
    );
    assert!(
        second_biased > 0.03,
        "bias should put one in; got {second_biased}"
    );
    let dc = out.iter().sum::<f32>() / out.len() as f32;
    assert!(dc.abs() < 0.01, "bias left {dc} of DC on the output");
}

#[test]
fn tubes_shape_is_how_asymmetric_it_is() {
    let mut even = clean();
    even.curve = DistortionCurve::Tube;
    even.drive_db = 24.0;
    even.shape = 0.0;
    let mut more = even;
    more.shape = 1.0;
    let a = amplitude_at(&analysed(&even, 0.5, 1_000.0), 2_000.0);
    let b = amplitude_at(&analysed(&more, 0.5, 1_000.0), 2_000.0);
    assert!(
        b > a * 1.5,
        "more shape on the tube should be more second harmonic: {a} then {b}"
    );
}

#[test]
fn sag_pulls_the_drive_down_when_the_signal_is_loud() {
    // An amplifier's power supply cannot keep up with a loud passage, so the
    // clipping softens under it. Measured as the harmonic ratio — a sag that
    // only turned the output down would leave the tone alone.
    let mut stiff = clean();
    stiff.drive_db = 30.0;
    let mut sagging = stiff;
    sagging.sag = 1.0;
    let ratio = |config: &DistortionConfig| {
        let out = analysed(config, 1.0, 1_000.0);
        amplitude_at(&out, 3_000.0) / amplitude_at(&out, 1_000.0)
    };
    let (before, after) = (ratio(&stiff), ratio(&sagging));
    assert!(
        after < before * 0.8,
        "sag should soften a loud signal's clipping: {before} then {after}"
    );
    // And a quiet one is left as it was: the supply is not being asked for
    // anything.
    let quiet = |config: &DistortionConfig| {
        let out = analysed(config, 0.02, 1_000.0);
        amplitude_at(&out, 3_000.0) / amplitude_at(&out, 1_000.0)
    };
    let (before, after) = (quiet(&stiff), quiet(&sagging));
    assert!(
        (before - after).abs() < before * 0.15 + 1e-4,
        "sag should leave a quiet signal's clipping alone: {before} then {after}"
    );
}

#[test]
fn the_pre_high_pass_is_before_the_curve() {
    // Tight versus flabby. The proof that it sits *before* the shaper rather
    // than after it: a low tone taken out first never gets distorted, so its
    // harmonics — which a post filter would let through — are not there.
    let mut loose = clean();
    loose.drive_db = 30.0;
    loose.pre_hp_hz = 20.0;
    let mut tight = loose;
    tight.pre_hp_hz = 1_000.0;
    let third_loose = amplitude_at(&analysed(&loose, 1.0, 50.0), 150.0);
    let third_tight = amplitude_at(&analysed(&tight, 1.0, 50.0), 150.0);
    assert!(
        third_loose > 0.1,
        "a loud 50 Hz tone into 30 dB of drive should clip; got {third_loose}"
    );
    assert!(
        third_tight < third_loose * 0.05,
        "with the low end filtered first there is nothing to clip: {third_loose} then {third_tight}"
    );
}

#[test]
fn the_pre_mid_boost_is_before_the_curve() {
    // The Tube Screamer's hump: a bell before the clipper drives the mids
    // harder than the rest. Measured as harmonics, because a boost after the
    // curve would be an EQ and would not change how hard anything clips.
    let mut flat = clean();
    flat.drive_db = 12.0;
    let mut humped = flat;
    humped.pre_mid_hz = 1_000.0;
    humped.pre_mid_db = 18.0;
    let third_flat = amplitude_at(&analysed(&flat, 0.05, 1_000.0), 3_000.0);
    let third_humped = amplitude_at(&analysed(&humped, 0.05, 1_000.0), 3_000.0);
    assert!(
        third_humped > third_flat * 5.0,
        "18 dB more into the curve at 1 kHz should clip far harder: {third_flat} then {third_humped}"
    );
    // And it is a bell, not a shelf: two octaves up it has done very little.
    let far_flat = amplitude_at(&analysed(&flat, 0.05, 4_000.0), 12_000.0);
    let far_humped = amplitude_at(&analysed(&humped, 0.05, 4_000.0), 12_000.0);
    assert!(
        far_humped < far_flat * 3.0 + 1e-4,
        "the boost should be a bell around 1 kHz, not everything: {far_flat} then {far_humped}"
    );
}

#[test]
fn the_clean_low_band_goes_around_the_curve() {
    // Bass distortion that keeps its bottom. Below the split the signal is
    // not distorted at all; above it, it is.
    let mut config = clean();
    config.drive_db = 40.0;
    config.clean_low_hz = 300.0;
    let low = analysed(&config, 0.5, 30.0);
    assert!(
        amplitude_at(&low, 90.0) < 0.01,
        "a 30 Hz tone under a 300 Hz split should not clip; third harmonic was {}",
        amplitude_at(&low, 90.0)
    );
    assert!(
        (amplitude_at(&low, 30.0) - 0.5).abs() < 0.08,
        "and it should come out at the level it went in; got {}",
        amplitude_at(&low, 30.0)
    );
    let high = analysed(&config, 0.5, 2_000.0);
    assert!(
        amplitude_at(&high, 6_000.0) > 0.05,
        "a 2 kHz tone over the split should clip as hard as ever; got {}",
        amplitude_at(&high, 6_000.0)
    );
}

#[test]
fn the_clean_low_split_sums_flat() {
    // The two paths have to add back to the whole signal, or the split is an
    // EQ notch at its own frequency. At no drive, the effect with a split is
    // still a wire, right at the crossover.
    let mut config = clean();
    config.clean_low_hz = 300.0;
    for freq in [100.0, 300.0, 1_000.0] {
        let level = amplitude_at(&analysed(&config, 0.3, freq), freq);
        assert!(
            (level - 0.3).abs() < 0.03,
            "a split at 300 Hz changed the level at {freq} Hz: {level}"
        );
    }
}

#[test]
fn auto_gain_makes_drive_a_tone_control() {
    // Without it, 36 dB of drive into a soft clipper is a square wave at full
    // scale; with it, the output sits near the level that went in, and only
    // the tone has changed.
    let mut raw = clean();
    raw.drive_db = 36.0;
    let mut matched = raw;
    matched.auto_gain = true;
    let loud = amplitude_at(&analysed(&raw, 0.3, 1_000.0), 1_000.0);
    let level = amplitude_at(&analysed(&matched, 0.3, 1_000.0), 1_000.0);
    assert!(
        loud > 0.9,
        "without compensation the drive is a volume: {loud}"
    );
    assert!(
        level > 0.15 && level < 0.6,
        "auto-gain should put the level back near where it started: {level}"
    );
    // The harmonics are still there — it is a gain, not a bypass.
    let third = amplitude_at(&analysed(&matched, 0.3, 1_000.0), 3_000.0);
    assert!(third > 0.03, "auto-gain took the distortion away: {third}");
}

#[test]
fn rectify_is_an_octave_up() {
    // A full-wave rectified sine has no fundamental and a strong second
    // harmonic — that is the octave fuzz, and the reason the curve exists.
    let mut config = clean();
    config.curve = DistortionCurve::Rectify;
    config.shape = 1.0;
    let out = analysed(&config, 0.8, 220.0);
    let fundamental = amplitude_at(&out, 220.0);
    let octave = amplitude_at(&out, 440.0);
    assert!(
        octave > fundamental * 3.0,
        "full-wave rectification should be mostly octave: {fundamental} at the note, {octave} above it"
    );
    // Half-wave keeps the note and adds the octave under it.
    config.shape = 0.0;
    let out = analysed(&config, 0.8, 220.0);
    assert!(
        amplitude_at(&out, 220.0) > 0.2,
        "half-wave should keep the fundamental"
    );
    assert!(amplitude_at(&out, 440.0) > 0.05, "and add the octave");
    let dc = out.iter().sum::<f32>() / out.len() as f32;
    assert!(dc.abs() < 0.02, "a rectifier left {dc} of DC on the output");
}

#[test]
fn crossover_swallows_a_quiet_signal_and_passes_a_loud_one() {
    // The dead zone at zero: the spitting, gated fuzz. A signal that never
    // leaves the zone never comes out.
    let mut config = clean();
    config.curve = DistortionCurve::Crossover;
    config.shape = 0.5;
    let quiet = through(&config, 0.02, 220.0);
    assert!(
        peak(&quiet[SETTLE..]) < 1e-4,
        "a signal inside the dead zone should be silent; got {}",
        peak(&quiet[SETTLE..])
    );
    let loud = through(&config, 1.0, 220.0);
    assert!(peak(&loud[SETTLE..]) > 0.5, "a loud one should get through");
}

#[test]
fn wrap_is_discontinuous() {
    // The digital one: past the threshold the value comes back in from the
    // other side, as an integer overflow does. The tell is a jump between
    // two adjacent samples that no smooth curve could make.
    let mut config = clean();
    config.curve = DistortionCurve::Wrap;
    config.shape = 1.0;
    config.drive_db = 12.0;
    // Un-oversampled, as the Digital preset runs it: the claim is about the
    // curve, and a two-unit step through the band-limiting filters rings
    // past the rails by a sixth of its height, which is the filters' doing
    // and not the curve's.
    config.oversample = Oversampling::Off;
    let out = through(&config, 0.8, 220.0);
    let jump = out[SETTLE..]
        .windows(2)
        .fold(0.0f32, |m, pair| m.max((pair[1] - pair[0]).abs()));
    assert!(
        jump > 1.0,
        "a wrap should jump; the biggest step was {jump}"
    );
    assert!(peak(&out) <= 1.01, "and stay inside the rails");
}

#[test]
fn triangle_fold_and_sine_fold_are_different_folds() {
    // Both turn back past the threshold. The sine fold rounds its corners
    // and the triangle fold does not, which is the difference between a
    // Buchla and a Serge, and it shows up as high harmonics.
    let mut sine_fold = clean();
    sine_fold.curve = DistortionCurve::Fold;
    sine_fold.drive_db = 12.0;
    let mut triangle = sine_fold;
    triangle.curve = DistortionCurve::TriangleFold;
    let round = amplitude_at(&analysed(&sine_fold, 0.8, 1_000.0), 9_000.0);
    let sharp = amplitude_at(&analysed(&triangle, 0.8, 1_000.0), 9_000.0);
    assert!(
        sharp > round * 1.5,
        "a triangle fold should have more of the ninth harmonic: {round} then {sharp}"
    );
}

#[test]
fn every_preset_is_audibly_a_distortion() {
    // A preset is a constructor. Each one has to land somewhere that sounds
    // like something, or it is a name on a menu that does nothing.
    let input = sine(0.3, 1_000.0, FRAMES);
    for preset in DistortionPreset::ALL {
        let config = DistortionConfig::from_preset(preset);
        let out = through(&config, 0.3, 1_000.0);
        let difference = out[SETTLE..]
            .iter()
            .zip(input[SETTLE..].iter())
            .fold(0.0f32, |m, (a, b)| m.max((a - b).abs()));
        assert!(difference > 0.02, "{preset:?} left a 1 kHz tone as it was");
        assert!(
            out.iter().all(|s| s.is_finite()),
            "{preset:?} is not finite"
        );
    }
}
