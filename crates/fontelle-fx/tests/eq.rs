//! The parametric EQ, measured as a frequency response (TDD §13.4).
//!
//! Every test here plays a sine through the EQ and asks how loud it came out —
//! which is the only question an EQ answers, and the only one that catches the
//! ways this kind of DSP goes wrong. A bell wired to the wrong coefficient
//! still produces plausible-looking audio; a shelf whose gain is applied twice
//! sounds like an EQ until you measure it; a cascade whose sections share one
//! filter's state produces a slope that is nearly right. None of those are
//! visible in a waveform and all of them are visible in a number.
//!
//! Steady state, not the whole buffer: a filter's first few milliseconds are
//! its transient response, and measuring the ring-in as though it were the
//! passband is how a test ends up asserting the wrong number to three decimal
//! places.

use fontelle_fx::ParametricEq;
use fontelle_types::{BandChannel, BandType, EqBand, EqConfig};

const SR: f32 = 48_000.0;

/// Long enough that the tail below is many cycles of the lowest frequency
/// tested (40 Hz is 1200 samples a cycle).
const FRAMES: usize = 24_000;

/// Where the measurement starts: past any filter's ring-in at these
/// frequencies, and still leaving two thirds of the buffer to average over.
const SETTLE: usize = 8_000;

fn sine(freq: f32, frames: usize) -> Vec<f32> {
    (0..frames)
        .map(|i| (std::f32::consts::TAU * freq * i as f32 / SR).sin())
        .collect()
}

fn rms(samples: &[f32]) -> f32 {
    let sum: f64 = samples.iter().map(|s| (*s as f64) * (*s as f64)).sum();
    (sum / samples.len() as f64).sqrt() as f32
}

/// What the EQ does to a sine at `freq`, in dB. Zero is "passed through".
fn response(config: &EqConfig, freq: f32) -> f32 {
    let mut eq = ParametricEq::new();
    eq.prepare(SR);
    let mut left = sine(freq, FRAMES);
    let mut right = left.clone();
    eq.process(&mut [&mut left, &mut right], config);
    20.0 * (rms(&left[SETTLE..]) / rms(&sine(freq, FRAMES)[SETTLE..])).log10()
}

/// A config with one band on it and the other seven off.
fn one(band: EqBand) -> EqConfig {
    let mut config = EqConfig::default();
    config.bands[0] = band;
    config
}

fn bell(freq_hz: f32, gain_db: f32, q: f32) -> EqBand {
    EqBand {
        band_type: BandType::Bell,
        freq_hz,
        gain_db,
        q,
        enabled: true,
        solo: false,
        channel: BandChannel::Stereo,
    }
}

fn band(band_type: BandType, freq_hz: f32) -> EqBand {
    EqBand {
        band_type,
        freq_hz,
        gain_db: 0.0,
        q: std::f32::consts::FRAC_1_SQRT_2,
        enabled: true,
        solo: false,
        channel: BandChannel::Stereo,
    }
}

/// Asserts a measured response is within `tolerance` dB of `expected`.
#[track_caller]
fn near(measured: f32, expected: f32, tolerance: f32, what: &str) {
    assert!(
        (measured - expected).abs() <= tolerance,
        "{what}: expected {expected:+.1} dB, measured {measured:+.2} dB"
    );
}

// ------------------------------------------------------------- pass-through

#[test]
fn an_eq_with_nothing_switched_on_is_a_wire() {
    // The default, and the state every EQ spends most of its life in. Sample
    // for sample rather than by response, because "inaudible" is not the claim
    // — an insert nobody has touched must not colour anything at all.
    let config = EqConfig::default();
    let mut eq = ParametricEq::new();
    eq.prepare(SR);

    let original = sine(440.0, 2_048);
    let mut left = original.clone();
    let mut right = original.clone();
    eq.process(&mut [&mut left, &mut right], &config);

    assert_eq!(left, original, "the left channel is untouched");
    assert_eq!(right, original, "and so is the right");
}

#[test]
fn a_band_that_is_switched_off_does_nothing() {
    let mut off = bell(1_000.0, 12.0, 1.0);
    off.enabled = false;
    near(response(&one(off), 1_000.0), 0.0, 0.05, "a disabled bell");
}

#[test]
fn a_bell_at_zero_gain_is_also_a_wire() {
    // The case a bell implemented as "always filter, then blend" gets wrong:
    // at 0 dB the filter must be an identity, not a subtle colouration.
    near(
        response(&one(bell(1_000.0, 0.0, 2.0)), 1_000.0),
        0.0,
        0.05,
        "a flat bell at its own frequency",
    );
}

// -------------------------------------------------------------------- bells

#[test]
fn a_bell_lifts_its_own_frequency_by_its_own_gain() {
    near(
        response(&one(bell(1_000.0, 6.0, 1.0)), 1_000.0),
        6.0,
        0.5,
        "a +6 dB bell at 1 kHz",
    );
}

#[test]
fn a_bell_cuts_as_well_as_it_lifts() {
    // Symmetry, which a gain folded into the wrong coefficient breaks in one
    // direction and not the other.
    near(
        response(&one(bell(1_000.0, -9.0, 1.0)), 1_000.0),
        -9.0,
        0.5,
        "a -9 dB bell at 1 kHz",
    );
}

#[test]
fn a_bell_leaves_frequencies_well_away_from_it_alone() {
    let config = one(bell(1_000.0, 12.0, 2.0));
    near(
        response(&config, 60.0),
        0.0,
        0.6,
        "two decades below a bell",
    );
    near(
        response(&config, 15_000.0),
        0.0,
        0.6,
        "well above the same bell",
    );
}

#[test]
fn a_narrow_bell_reaches_less_far_than_a_wide_one() {
    // What Q means, stated as the thing a user actually notices.
    let wide = response(&one(bell(1_000.0, 12.0, 0.5)), 2_000.0);
    let narrow = response(&one(bell(1_000.0, 12.0, 8.0)), 2_000.0);
    assert!(
        wide > narrow + 2.0,
        "an octave up: wide Q gave {wide:+.1} dB, narrow gave {narrow:+.1} dB"
    );
}

// ------------------------------------------------------------------- shelves

#[test]
fn a_low_shelf_lifts_what_is_below_it_and_not_what_is_above() {
    let config = one(EqBand {
        band_type: BandType::LowShelf,
        freq_hz: 300.0,
        gain_db: 8.0,
        q: std::f32::consts::FRAC_1_SQRT_2,
        enabled: true,
        solo: false,
        channel: BandChannel::Stereo,
    });
    near(
        response(&config, 40.0),
        8.0,
        0.6,
        "well below a +8 dB shelf",
    );
    near(response(&config, 8_000.0), 0.0, 0.6, "well above it");
}

#[test]
fn a_high_shelf_lifts_what_is_above_it_and_not_what_is_below() {
    let config = one(EqBand {
        band_type: BandType::HighShelf,
        freq_hz: 4_000.0,
        gain_db: -8.0,
        q: std::f32::consts::FRAC_1_SQRT_2,
        enabled: true,
        solo: false,
        channel: BandChannel::Stereo,
    });
    near(
        response(&config, 15_000.0),
        -8.0,
        0.6,
        "well above a -8 dB shelf",
    );
    near(response(&config, 100.0), 0.0, 0.6, "well below it");
}

// ------------------------------------------------------- pass filters, and slope

#[test]
fn a_low_pass_keeps_what_is_below_and_removes_what_is_above() {
    let config = one(band(BandType::LowPass12, 1_000.0));
    near(
        response(&config, 100.0),
        0.0,
        0.6,
        "a decade below a low-pass",
    );
    assert!(
        response(&config, 8_000.0) < -30.0,
        "three octaves above it should be gone"
    );
}

#[test]
fn a_high_pass_keeps_what_is_above_and_removes_what_is_below() {
    let config = one(band(BandType::HighPass12, 1_000.0));
    near(
        response(&config, 10_000.0),
        0.0,
        0.6,
        "a decade above a high-pass",
    );
    assert!(
        response(&config, 100.0) < -30.0,
        "three octaves below it should be gone"
    );
}

#[test]
fn the_corner_of_a_pass_filter_is_three_db_down() {
    // The definition of a corner frequency, and the thing that tells you the
    // filter is at the frequency asked for rather than near it.
    near(
        response(&one(band(BandType::LowPass12, 1_000.0)), 1_000.0),
        -3.0,
        0.6,
        "at a 12 dB/oct low-pass's own corner",
    );
}

#[test]
fn a_steeper_slope_removes_more_of_the_same_octave() {
    // The whole reason there are three of each: one octave above the corner, a
    // 12 falls about 6 dB, a 24 about 12, a 48 about 24. Measured against each
    // other rather than against absolute numbers, because what a person picks
    // a slope *for* is the comparison.
    let at = |kind| response(&one(band(kind, 1_000.0)), 2_000.0);
    let (twelve, twentyfour, fortyeight) = (
        at(BandType::LowPass12),
        at(BandType::LowPass24),
        at(BandType::LowPass48),
    );
    assert!(
        twelve > twentyfour + 3.0,
        "24 should be steeper than 12: {twelve:+.1} against {twentyfour:+.1}"
    );
    assert!(
        twentyfour > fortyeight + 6.0,
        "48 steeper than 24: {twentyfour:+.1} against {fortyeight:+.1}"
    );
    // And each is its own order. Not 6/12/24: "12 dB/oct" is the *asymptotic*
    // slope, and one octave above the corner is not yet the asymptote. A
    // Butterworth of order n measures 1/sqrt(1 + (f/fc)^2n) there, which for
    // f/fc = 2 is -12.3, -24.1 and -48.2 dB. The first draft of this test
    // asserted the one-pole numbers and the filter was right.
    near(twelve, -12.3, 1.0, "an octave above a 12 dB/oct low-pass");
    near(twentyfour, -24.1, 1.5, "the same octave on a 24");
    near(fortyeight, -48.2, 3.0, "and on a 48");
}

#[test]
fn a_steep_high_pass_is_steep_in_the_other_direction() {
    // Cascaded sections are easy to get right one way round and wrong the
    // other, because the Q table is shared and the mode is not.
    let at = |kind| response(&one(band(kind, 1_000.0)), 500.0);
    let (twelve, fortyeight) = (at(BandType::HighPass12), at(BandType::HighPass48));
    assert!(
        twelve > fortyeight + 9.0,
        "an octave below: 12 gave {twelve:+.1} dB, 48 gave {fortyeight:+.1} dB"
    );
}

// ---------------------------------------------------------- notch and bandpass

#[test]
fn a_notch_removes_its_own_frequency_and_keeps_its_neighbours() {
    let config = one(EqBand {
        band_type: BandType::Notch,
        freq_hz: 1_000.0,
        gain_db: 0.0,
        q: 4.0,
        enabled: true,
        solo: false,
        channel: BandChannel::Stereo,
    });
    assert!(
        response(&config, 1_000.0) < -20.0,
        "the notch's own frequency should be gone"
    );
    near(response(&config, 250.0), 0.0, 1.0, "two octaves below it");
}

#[test]
fn a_band_pass_keeps_only_its_own_region() {
    let config = one(EqBand {
        band_type: BandType::BandPass,
        freq_hz: 1_000.0,
        gain_db: 0.0,
        q: 2.0,
        enabled: true,
        solo: false,
        channel: BandChannel::Stereo,
    });
    near(response(&config, 1_000.0), 0.0, 1.0, "at its own centre");
    assert!(
        response(&config, 100.0) < -15.0,
        "and well below it, nothing"
    );
}

// --------------------------------------------------------------- eight of them

#[test]
fn all_eight_bands_work_at_once_and_each_is_its_own() {
    // Eight bands, eight frequencies, one lift each — which fails if any band
    // shares a filter with any other, and passes if they are independent.
    let mut config = EqConfig::default();
    let frequencies = [
        80.0, 160.0, 320.0, 640.0, 1_280.0, 2_560.0, 5_120.0, 10_240.0,
    ];
    for (slot, freq) in frequencies.iter().enumerate() {
        config.bands[slot] = bell(*freq, 6.0, 6.0);
    }
    for freq in frequencies {
        near(
            response(&config, freq),
            6.0,
            1.5,
            &format!("band at {freq} Hz with all eight running"),
        );
    }
}

// ---------------------------------------------------------------------- solo

#[test]
fn soloing_a_band_leaves_only_the_region_that_band_works_on() {
    // Per-band listen (TDD §13.4): what the band is *working on*, so a person
    // can hear the thing they are about to cut. A bandpass at the band's own
    // frequency and Q, which is the same audition whatever kind of band it is.
    let mut config = one(bell(1_000.0, 6.0, 4.0));
    config.bands[0].solo = true;
    near(response(&config, 1_000.0), 0.0, 1.5, "the soloed region");
    assert!(
        response(&config, 100.0) < -15.0,
        "and nothing from well outside it"
    );
}

#[test]
fn a_solo_anywhere_silences_the_bands_that_are_not_soloed() {
    // Soloing band 2 must not leave band 1's lift audible underneath it —
    // which is what "solo" means everywhere else in the mixer.
    let mut config = EqConfig::default();
    config.bands[0] = bell(100.0, 12.0, 4.0);
    config.bands[1] = bell(5_000.0, 0.0, 4.0);
    config.bands[1].solo = true;
    assert!(
        response(&config, 100.0) < -15.0,
        "band 1's frequency should be outside the soloed region entirely"
    );
}

// ------------------------------------------------------------------ mid/side

#[test]
fn a_side_band_leaves_anything_down_the_middle_alone() {
    // The point of the mode: a lift that does not touch what is panned centre.
    // A mono signal is all mid and no side, so it must come out unchanged
    // however hard a side band is driven.
    //
    // **Per band, not per EQ.** The first draft of this had one `mid_side`
    // switch on the whole EQ, which is provably a no-op: a filter is linear,
    // so F(M) + F(S) = F(L), and filtering mid and side alike is exactly
    // filtering left and right alike. The test failed and the design was what
    // was wrong.
    let mut band = bell(1_000.0, 12.0, 1.0);
    band.channel = BandChannel::Side;
    let config = one(band);

    let mut eq = ParametricEq::new();
    eq.prepare(SR);
    let original = sine(1_000.0, FRAMES);
    let (mut left, mut right) = (original.clone(), original.clone());
    eq.process(&mut [&mut left, &mut right], &config);

    let level = 20.0 * (rms(&left[SETTLE..]) / rms(&original[SETTLE..])).log10();
    near(level, 0.0, 0.5, "a centred sine through a side-only lift");
}

#[test]
fn a_side_band_reaches_what_is_actually_in_the_sides() {
    // The other half: something that *is* in the sides does change. An
    // out-of-phase pair is all side and no mid.
    let mut band = bell(1_000.0, 12.0, 1.0);
    band.channel = BandChannel::Side;
    let config = one(band);

    let mut eq = ParametricEq::new();
    eq.prepare(SR);
    let original = sine(1_000.0, FRAMES);
    let mut left = original.clone();
    let mut right: Vec<f32> = original.iter().map(|s| -s).collect();
    eq.process(&mut [&mut left, &mut right], &config);

    let level = 20.0 * (rms(&left[SETTLE..]) / rms(&original[SETTLE..])).log10();
    assert!(
        level > 9.0,
        "an out-of-phase pair is all sides and should get the whole lift; got {level:+.1} dB"
    );
}

#[test]
fn a_mid_band_is_the_other_way_round() {
    let mut band = bell(1_000.0, 12.0, 1.0);
    band.channel = BandChannel::Mid;
    let config = one(band);

    let mut eq = ParametricEq::new();
    eq.prepare(SR);
    let original = sine(1_000.0, FRAMES);

    let (mut left, mut right) = (original.clone(), original.clone());
    eq.process(&mut [&mut left, &mut right], &config);
    let centred = 20.0 * (rms(&left[SETTLE..]) / rms(&original[SETTLE..])).log10();
    assert!(
        centred > 9.0,
        "a centred sine is all mid; got {centred:+.1} dB"
    );

    eq.reset();
    let mut left = original.clone();
    let mut right: Vec<f32> = original.iter().map(|s| -s).collect();
    eq.process(&mut [&mut left, &mut right], &config);
    let sides = 20.0 * (rms(&left[SETTLE..]) / rms(&original[SETTLE..])).log10();
    near(
        sides,
        0.0,
        0.5,
        "and an out-of-phase pair has no mid to lift",
    );
}

#[test]
fn a_stereo_band_beside_a_side_band_still_acts_on_everything() {
    // The case the rotation makes easy to get wrong: once *any* band asks for
    // mid/side the whole EQ runs in it, and a plain stereo band sitting next
    // to it has to keep meaning "both sides" rather than quietly becoming a
    // mid band.
    let mut config = EqConfig::default();
    config.bands[0] = bell(1_000.0, 6.0, 1.0);
    config.bands[1] = bell(5_000.0, 6.0, 1.0);
    config.bands[1].channel = BandChannel::Side;

    let mut eq = ParametricEq::new();
    eq.prepare(SR);
    let original = sine(1_000.0, FRAMES);
    let (mut left, mut right) = (original.clone(), original.clone());
    eq.process(&mut [&mut left, &mut right], &config);

    let level = 20.0 * (rms(&left[SETTLE..]) / rms(&original[SETTLE..])).log10();
    near(level, 6.0, 0.5, "the stereo band on a centred sine");
}

#[test]
fn a_side_band_on_a_mono_bus_has_nothing_to_do() {
    // One channel is all mid by definition. A side band that ran anyway would
    // be filtering the mid and calling it the side.
    let mut band = bell(1_000.0, 12.0, 1.0);
    band.channel = BandChannel::Side;
    let config = one(band);

    let mut eq = ParametricEq::new();
    eq.prepare(SR);
    let original = sine(1_000.0, FRAMES);
    let mut mono = original.clone();
    eq.process(&mut [&mut mono], &config);

    assert_eq!(mono, original, "a mono bus has no sides to lift");
}

// ------------------------------------------------------------------ plumbing

#[test]
fn the_two_channels_do_not_bleed_into_each_other() {
    // One filter's state shared between L and R is the classic stereo EQ bug:
    // it sounds almost right and collapses the image.
    let config = one(band(BandType::LowPass12, 500.0));
    let mut eq = ParametricEq::new();
    eq.prepare(SR);

    let mut left = sine(200.0, FRAMES);
    let mut right = vec![0.0; FRAMES];
    eq.process(&mut [&mut left, &mut right], &config);

    assert!(
        rms(&right[SETTLE..]) < 1e-6,
        "silence on the right must stay silence, got {}",
        rms(&right[SETTLE..])
    );
}

#[test]
fn a_reset_clears_the_tail_so_the_next_thing_starts_clean() {
    let config = one(band(BandType::LowPass12, 500.0));
    let mut eq = ParametricEq::new();
    eq.prepare(SR);

    let mut loud = vec![1.0f32; 512];
    let mut other = vec![1.0f32; 512];
    eq.process(&mut [&mut loud, &mut other], &config);
    eq.reset();

    let mut quiet = vec![0.0f32; 512];
    let mut other = vec![0.0f32; 512];
    eq.process(&mut [&mut quiet, &mut other], &config);
    assert!(
        quiet.iter().all(|s| s.abs() < 1e-9),
        "a reset filter fed silence must produce silence"
    );
}

#[test]
fn a_mono_render_is_processed_rather_than_skipped() {
    // The EQ is handed whatever the bus has. One channel is a legitimate bus,
    // and an effect that quietly does nothing on it is worse than one that
    // refuses.
    let config = one(bell(1_000.0, 6.0, 1.0));
    let mut eq = ParametricEq::new();
    eq.prepare(SR);

    let original = sine(1_000.0, FRAMES);
    let mut mono = original.clone();
    eq.process(&mut [&mut mono], &config);

    let level = 20.0 * (rms(&mono[SETTLE..]) / rms(&original[SETTLE..])).log10();
    near(level, 6.0, 0.5, "a +6 dB bell on a mono bus");
}

// --------------------------------------------------------- the drawn curve

// `EqBand::response_db` is what the mixer panel draws. It lives beside the
// parameters rather than beside the DSP, so this is the only place the two can
// be held against each other — and they have to be, because a curve that lies
// about the sound is worse than no curve at all: it is believed.

/// Asserts the drawn curve agrees with the measured filter at `freq`.
#[track_caller]
fn curve_matches(config: &EqConfig, freq: f32, tolerance: f32) {
    let drawn = config.curve_db(freq);
    let measured = response(config, freq);
    assert!(
        (drawn - measured).abs() <= tolerance,
        "at {freq} Hz the panel would draw {drawn:+.2} dB and the filter does \
         {measured:+.2} dB"
    );
}

#[test]
fn the_curve_a_bell_draws_is_the_bell_it_makes() {
    let config = one(bell(1_000.0, 9.0, 2.0));
    for freq in [100.0, 500.0, 800.0, 1_000.0, 1_250.0, 2_000.0, 6_000.0] {
        curve_matches(&config, freq, 0.6);
    }
}

#[test]
fn the_curve_a_cut_draws_is_the_cut_it_makes() {
    let config = one(bell(2_000.0, -12.0, 4.0));
    for freq in [500.0, 1_500.0, 2_000.0, 3_000.0, 8_000.0] {
        curve_matches(&config, freq, 0.6);
    }
}

#[test]
fn the_curve_a_shelf_draws_is_the_shelf_it_makes() {
    for (band_type, corner) in [(BandType::LowShelf, 300.0), (BandType::HighShelf, 4_000.0)] {
        let config = one(EqBand {
            band_type,
            freq_hz: corner,
            gain_db: 8.0,
            q: std::f32::consts::FRAC_1_SQRT_2,
            enabled: true,
            solo: false,
            channel: BandChannel::Stereo,
        });
        for freq in [60.0, 300.0, 1_000.0, 4_000.0, 12_000.0] {
            curve_matches(&config, freq, 0.8);
        }
    }
}

#[test]
fn the_curve_a_pass_filter_draws_is_the_slope_it_makes() {
    for band_type in [
        BandType::LowPass12,
        BandType::LowPass24,
        BandType::LowPass48,
        BandType::HighPass12,
        BandType::HighPass24,
    ] {
        let config = one(band(band_type, 1_000.0));
        // Not past 6 kHz: a digital filter's response departs from the
        // analogue prototype as it approaches Nyquist, and the display does
        // not model the warping. It is a display.
        for freq in [200.0, 700.0, 1_000.0, 1_400.0, 3_000.0] {
            curve_matches(&config, freq, 1.5);
        }
    }
}

#[test]
fn the_curve_of_several_bands_is_the_sum_of_them() {
    // Filters multiply and decibels add, which is the whole reason the panel
    // can draw one line for eight bands.
    let mut config = EqConfig::default();
    config.bands[0] = bell(200.0, 6.0, 1.0);
    config.bands[1] = bell(1_000.0, -4.0, 2.0);
    config.bands[2] = bell(5_000.0, 3.0, 1.5);
    for freq in [100.0, 200.0, 600.0, 1_000.0, 2_500.0, 5_000.0, 10_000.0] {
        curve_matches(&config, freq, 0.8);
    }
}

#[test]
fn a_band_that_is_off_draws_nothing() {
    let mut off = bell(1_000.0, 12.0, 1.0);
    off.enabled = false;
    assert_eq!(one(off).curve_db(1_000.0), 0.0);
}

#[test]
fn a_side_band_is_not_in_the_mid_curve() {
    // Two curves, because there are two paths. One line claiming to be both
    // would have a shape neither of them has.
    let mut side = bell(1_000.0, 12.0, 1.0);
    side.channel = BandChannel::Side;
    let config = one(side);

    assert_eq!(config.response_db(1_000.0, BandChannel::Mid), 0.0);
    assert!(config.response_db(1_000.0, BandChannel::Side) > 9.0);
}

#[test]
fn a_stereo_band_is_in_both_curves() {
    let config = one(bell(1_000.0, 6.0, 1.0));
    assert!(config.response_db(1_000.0, BandChannel::Mid) > 5.0);
    assert!(config.response_db(1_000.0, BandChannel::Side) > 5.0);
}
