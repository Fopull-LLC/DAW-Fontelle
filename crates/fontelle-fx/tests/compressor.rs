//! The compressor, measured as a transfer curve (TDD §13.4).
//!
//! The second effect, and the interesting one: it is the first link in a chain
//! that is **not linear**, which is what makes the order of a chain audible at
//! all. Two EQs commute and provably always will; an EQ before a compressor is
//! a different sound from an EQ after one, and
//! `fontelle-app/tests/insert_chains.rs` says so now that there is something
//! to say it with.
//!
//! Every test here measures a **level** — what went in against what came out —
//! because that is the only question a compressor answers. A ratio wired to
//! the wrong side of a division still produces plausible audio that is quieter
//! than it started.

use fontelle_fx::Compressor;
use fontelle_types::{CompressorConfig, DetectionMode};

const SR: f32 = 48_000.0;
const FRAMES: usize = 24_000;

/// Past any envelope's attack at the times used here, with plenty of buffer
/// left to measure over.
const SETTLE: usize = 12_000;

fn sine(amplitude: f32, freq: f32, frames: usize) -> Vec<f32> {
    (0..frames)
        .map(|i| amplitude * (std::f32::consts::TAU * freq * i as f32 / SR).sin())
        .collect()
}

fn peak(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0f32, |m, s| m.max(s.abs()))
}

fn db(level: f32) -> f32 {
    20.0 * level.max(1e-9).log10()
}

/// A settled compressor's output level, in dBFS peak.
fn through(config: &CompressorConfig, amplitude: f32) -> f32 {
    let mut compressor = Compressor::new();
    compressor.prepare(SR);
    let mut left = sine(amplitude, 220.0, FRAMES);
    let mut right = left.clone();
    compressor.process(&mut [&mut left, &mut right], None, config);
    db(peak(&left[SETTLE..]))
}

/// Threshold -20, 4:1, hard knee.
///
/// **A long release on purpose.** A peak detector watching a sine sees the
/// instantaneous level, which spends most of each cycle below the peak, so a
/// short release lets the gain creep back up between peaks and the output
/// peaks come out about half a decibel louder than the transfer curve says.
/// That ripple is real compressor behaviour — it is why a fast release on a
/// bass line distorts — but it is the *envelope* talking, and these tests are
/// about the curve. The release tests below set their own.
fn plain() -> CompressorConfig {
    CompressorConfig {
        threshold_db: -20.0,
        ratio: 4.0,
        attack_ms: 1.0,
        release_ms: 1_000.0,
        knee_db: 0.0,
        makeup_db: 0.0,
        auto_makeup: false,
        detection: DetectionMode::Peak,
        // Fully wet: the blend is the insert's, not the DSP's — see
        // `fontelle-engine/tests/inserts.rs`.
        mix: 1.0,
    }
}

#[track_caller]
fn near(measured: f32, expected: f32, tolerance: f32, what: &str) {
    assert!(
        (measured - expected).abs() <= tolerance,
        "{what}: expected {expected:+.1} dB, measured {measured:+.2} dB"
    );
}

// ------------------------------------------------------------ the curve

#[test]
fn below_the_threshold_nothing_happens() {
    // A compressor that colours quiet material is one you cannot leave on.
    let quiet = 10f32.powf(-30.0 / 20.0);
    near(
        through(&plain(), quiet),
        -30.0,
        0.2,
        "a -30 dB signal at -20 threshold",
    );
}

#[test]
fn above_the_threshold_the_excess_is_divided_by_the_ratio() {
    // The definition. 14 dB over a -20 threshold at 4:1 comes out 3.5 dB over.
    let loud = 10f32.powf(-6.0 / 20.0);
    near(through(&plain(), loud), -16.5, 0.4, "a -6 dB signal at 4:1");
}

#[test]
fn a_higher_ratio_squeezes_harder() {
    let loud = 10f32.powf(-6.0 / 20.0);
    let gentle = CompressorConfig {
        ratio: 2.0,
        ..plain()
    };
    let hard = CompressorConfig {
        ratio: 20.0,
        ..plain()
    };
    near(through(&gentle, loud), -13.0, 0.4, "2:1");
    near(through(&hard, loud), -19.3, 0.5, "20:1, which is limiting");
}

#[test]
fn a_ratio_of_one_is_a_wire() {
    // The bottom of the range has to be an identity, or a compressor sitting
    // at its default colours everything it is put on.
    let loud = 10f32.powf(-6.0 / 20.0);
    let off = CompressorConfig {
        ratio: 1.0,
        ..plain()
    };
    near(through(&off, loud), -6.0, 0.2, "1:1");
}

#[test]
fn a_soft_knee_starts_working_before_the_threshold() {
    // What a knee is *for*: the transition into compression is a curve rather
    // than a corner, so material sitting around the threshold does not switch
    // in and out audibly.
    let at_threshold = 10f32.powf(-22.0 / 20.0);
    let hard = through(&plain(), at_threshold);
    let soft = through(
        &CompressorConfig {
            knee_db: 12.0,
            ..plain()
        },
        at_threshold,
    );
    near(hard, -22.0, 0.2, "2 dB under a hard knee is untouched");
    assert!(
        soft < hard - 0.3,
        "and a soft knee has already begun: {hard:+.2} against {soft:+.2}"
    );
}

#[test]
fn a_knee_still_meets_the_hard_curve_well_above_it() {
    // A knee that changed the answer far above the threshold would not be a
    // knee, it would be a different ratio.
    let loud = 10f32.powf(-6.0 / 20.0);
    let hard = through(&plain(), loud);
    let soft = through(
        &CompressorConfig {
            knee_db: 6.0,
            ..plain()
        },
        loud,
    );
    near(soft, hard, 0.3, "11 dB above a 6 dB knee");
}

#[test]
fn makeup_gain_puts_back_what_was_taken() {
    let loud = 10f32.powf(-6.0 / 20.0);
    let with = CompressorConfig {
        makeup_db: 6.0,
        ..plain()
    };
    near(
        through(&with, loud),
        -10.5,
        0.4,
        "-16.5 plus 6 dB of makeup",
    );
}

#[test]
fn auto_makeup_gets_the_level_back_near_where_it_started() {
    // Not exactly, and it should not claim to: it compensates the *threshold*,
    // which is what the knob would otherwise be doing by hand.
    let loud = 10f32.powf(-6.0 / 20.0);
    let auto = CompressorConfig {
        auto_makeup: true,
        ..plain()
    };
    let level = through(&auto, loud);
    assert!(
        level > -12.0 && level <= 0.0,
        "auto makeup should recover most of the loss without clipping, got {level:+.2}"
    );
}

// ------------------------------------------------------------ in time

#[test]
fn the_attack_decides_how_fast_the_gain_arrives() {
    // A slow attack lets the front of a note through, which is the whole
    // reason the control exists.
    let level = 10f32.powf(-6.0 / 20.0);
    let measure = |attack_ms: f32| {
        let config = CompressorConfig {
            attack_ms,
            ..plain()
        };
        let mut compressor = Compressor::new();
        compressor.prepare(SR);
        let mut left = sine(level, 2_000.0, FRAMES);
        let mut right = left.clone();
        compressor.process(&mut [&mut left, &mut right], None, &config);
        // The first five milliseconds, which is where an attack lives.
        db(peak(&left[..(SR * 0.005) as usize]))
    };
    let fast = measure(0.1);
    let slow = measure(100.0);
    assert!(
        slow > fast + 2.0,
        "a slow attack should still be letting the transient through: \
         fast {fast:+.2}, slow {slow:+.2}"
    );
}

#[test]
fn the_release_decides_how_long_the_gain_stays_down() {
    // A burst, then quiet. A long release is still holding the level down
    // after the burst has gone; a short one has let go.
    let measure = |release_ms: f32| {
        let config = CompressorConfig {
            release_ms,
            ..plain()
        };
        let mut compressor = Compressor::new();
        compressor.prepare(SR);
        let burst = (SR * 0.05) as usize;
        let quiet = 10f32.powf(-24.0 / 20.0);
        let mut left: Vec<f32> = sine(1.0, 220.0, burst)
            .into_iter()
            .chain(sine(quiet, 220.0, FRAMES - burst))
            .collect();
        let mut right = left.clone();
        compressor.process(&mut [&mut left, &mut right], None, &config);
        // Ten milliseconds after the burst ends.
        let from = burst + (SR * 0.005) as usize;
        db(peak(&left[from..from + (SR * 0.005) as usize]))
    };
    let short = measure(5.0);
    let long = measure(2_000.0);
    assert!(
        short > long + 3.0,
        "a long release should still be ducking the quiet part: \
         short {short:+.2}, long {long:+.2}"
    );
}

// ------------------------------------------------------------ detection

#[test]
fn rms_detection_reads_a_sine_lower_than_peak_detection_does() {
    // A sine's RMS is 3 dB under its peak, so the same signal is 3 dB less
    // over the threshold and gets less reduction. That difference is the whole
    // reason both modes exist.
    let loud = 10f32.powf(-6.0 / 20.0);
    let peak_mode = through(&plain(), loud);
    let rms_mode = through(
        &CompressorConfig {
            detection: DetectionMode::Rms,
            ..plain()
        },
        loud,
    );
    assert!(
        rms_mode > peak_mode + 0.4,
        "RMS should compress a sine less than peak does: \
         peak {peak_mode:+.2}, rms {rms_mode:+.2}"
    );
}

// ------------------------------------------------------------ the plumbing

#[test]
fn one_gain_serves_both_channels_so_the_image_does_not_move() {
    // Independent per-channel gains pull a mix toward the quieter side every
    // time the other one peaks. The same reason the limiter is stereo-linked.
    let mut compressor = Compressor::new();
    compressor.prepare(SR);
    let mut left = sine(1.0, 220.0, FRAMES);
    let mut right = sine(0.25, 220.0, FRAMES);
    let before = peak(&left[SETTLE..]) / peak(&right[SETTLE..]);
    compressor.process(&mut [&mut left, &mut right], None, &plain());
    let after = peak(&left[SETTLE..]) / peak(&right[SETTLE..]);
    assert!(
        (after / before - 1.0).abs() < 0.02,
        "the balance between the channels should survive: {before} became {after}"
    );
}

#[test]
fn the_gain_reduction_is_reported_for_a_meter() {
    let loud = 10f32.powf(-6.0 / 20.0);
    let mut compressor = Compressor::new();
    compressor.prepare(SR);
    let mut left = sine(loud, 220.0, FRAMES);
    let mut right = left.clone();
    compressor.process(&mut [&mut left, &mut right], None, &plain());
    near(
        -compressor.gain_reduction_db(),
        10.5,
        1.0,
        "14 dB over at 4:1 is 10.5 dB of reduction",
    );
}

#[test]
fn a_quiet_signal_reports_no_reduction() {
    let quiet = 10f32.powf(-40.0 / 20.0);
    let mut compressor = Compressor::new();
    compressor.prepare(SR);
    let mut left = sine(quiet, 220.0, FRAMES);
    let mut right = left.clone();
    compressor.process(&mut [&mut left, &mut right], None, &plain());
    near(
        compressor.gain_reduction_db(),
        0.0,
        0.2,
        "nothing to reduce",
    );
}

#[test]
fn a_reset_lets_go_of_the_gain_it_was_holding() {
    let mut compressor = Compressor::new();
    compressor.prepare(SR);
    let mut loud = sine(1.0, 220.0, 4_096);
    let mut other = loud.clone();
    compressor.process(&mut [&mut loud, &mut other], None, &plain());
    compressor.reset();
    assert!(compressor.gain_reduction_db().abs() < 0.01);
}

#[test]
fn a_mono_bus_is_compressed_rather_than_skipped() {
    let loud = 10f32.powf(-6.0 / 20.0);
    let mut compressor = Compressor::new();
    compressor.prepare(SR);
    let mut mono = sine(loud, 220.0, FRAMES);
    compressor.process(&mut [&mut mono], None, &plain());
    near(
        db(peak(&mono[SETTLE..])),
        -16.5,
        0.4,
        "one channel is a bus",
    );
}

// ------------------------------------------------------------ the sidechain

#[test]
fn a_loud_sidechain_ducks_a_quiet_main() {
    // §13.4: "sidechain input from any mixer track". The DSP takes it here;
    // wiring one track's audio to another's detector is the graph's job and is
    // not built (see PROGRESS.md), but the effect must be ready for it rather
    // than needing rewriting when it is.
    let quiet = 10f32.powf(-24.0 / 20.0);
    let mut compressor = Compressor::new();
    compressor.prepare(SR);
    let mut left = sine(quiet, 220.0, FRAMES);
    let mut right = left.clone();
    let key = sine(1.0, 60.0, FRAMES);
    compressor.process(&mut [&mut left, &mut right], Some(&key), &plain());

    assert!(
        db(peak(&left[SETTLE..])) < -30.0,
        "a signal under the threshold should still duck when the key is loud"
    );
}

#[test]
fn without_a_sidechain_the_detector_listens_to_the_signal_itself() {
    let quiet = 10f32.powf(-24.0 / 20.0);
    near(through(&plain(), quiet), -24.0, 0.2, "and leaves it alone");
}
