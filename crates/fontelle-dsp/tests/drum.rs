//! The drum voice: one hit, synthesised.
//!
//! > *"i want you to create a new instrument, a built in general purpose drum
//! > machine that can just make a variety of drum styles and sounds and you can
//! > play them all in the piano roll all labeled and stuff should have lots of
//! > presets for different styles and genres of kits."*
//!
//! This is the bottom of it: the arithmetic that turns a handful of numbers
//! into a kick or a hat. It is here rather than in `fontelle-core` for the
//! reason every other primitive is — the oscillator, the envelope, the filter
//! are all shapes with no opinion about patches — and it is checkable without a
//! patch, a voice or a device, which is the whole point of the split.
//!
//! What a test can honestly say about a drum sound is not "it sounds like a
//! kick". It is: it makes signal, it stops, it stops **when it was told to**,
//! it stays inside full scale, its pitch goes where it was aimed, and two
//! different settings are two different sounds. Those are the failures that
//! actually happen — a voice that never frees itself, one that clips the bus,
//! one whose knob does nothing.

use fontelle_dsp::{DrumBody, DrumModel, DrumSynth, DrumVoice};

const SR: f32 = 48_000.0;

/// One hit, rendered to `seconds` of samples.
fn render(voice: &DrumVoice, seconds: f32) -> Vec<f32> {
    let mut synth = DrumSynth::new();
    synth.trigger(voice, SR);
    (0..(seconds * SR) as usize)
        .map(|_| synth.next_sample(voice, SR))
        .collect()
}

fn peak(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0f32, |a, s| a.max(s.abs()))
}

/// Root-mean-square over a window, which is what "is there still sound here"
/// means for noise — a peak can be one stray sample.
fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
}

/// How much energy sits between two frequencies, by a windowed DFT.
///
/// A **windowed** one, and swept at fine steps, because what is being asked
/// about here is a line spectrum: a mode is a spike a few hertz wide, and a
/// sparse probe walks straight past one. The same lesson `drum_kit.rs`'s
/// pairwise test records.
fn energy_between(samples: &[f32], from: f32, to: f32) -> f32 {
    let n = samples.len();
    if n < 2 {
        return 0.0;
    }
    let windowed: Vec<f32> = samples
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let w = 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / (n - 1) as f32).cos();
            s * w
        })
        .collect();
    let mut total = 0.0f32;
    let mut hz = from;
    while hz <= to {
        let (mut re, mut im) = (0.0f32, 0.0f32);
        let w = std::f32::consts::TAU * hz / SR;
        for (i, s) in windowed.iter().enumerate() {
            let a = w * i as f32;
            re += s * a.cos();
            im -= s * a.sin();
        }
        total += (re * re + im * im).sqrt();
        hz += 1.0;
    }
    total / n as f32
}

/// The window from `from` to `to` seconds.
fn window(samples: &[f32], from: f32, to: f32) -> &[f32] {
    let a = ((from * SR) as usize).min(samples.len());
    let b = ((to * SR) as usize).min(samples.len());
    &samples[a..b.max(a)]
}

fn kick() -> DrumVoice {
    DrumVoice {
        model: DrumModel::Kick,
        body: DrumBody::Sine,
        tune_hz: 50.0,
        bend_semitones: 24.0,
        bend_s: 0.04,
        decay_s: 0.35,
        tone_hz: 400.0,
        noise: 0.05,
        snap: 0.4,
        drive: 0.2,
        gain_db: 0.0,
        metal: 0.0,
        crush: 0.0,
        modes: 0.0,
        tail: 0.0,
        rattle: 0.0,
    }
}

// ------------------------------------------------------------ it sounds ---

#[test]
fn every_model_makes_a_sound_and_none_of_them_leaves_full_scale() {
    // A drum that clips the bus on its own is a drum nobody can use in a mix,
    // and the master limiter catching every hit is not a defence — it is a
    // kit that sounds squashed with nothing to point at.
    for model in DrumModel::ALL {
        let voice = DrumVoice { model, ..kick() };
        let out = render(&voice, 2.0);
        let peak = peak(&out);
        assert!(peak > 0.05, "{model:?} is silent");
        assert!(peak <= 1.0, "{model:?} peaks at {peak}");
    }
}

#[test]
fn a_hit_decays_to_nothing_and_says_when_it_has() {
    // The voice has to free itself. A drum that never reports done is a voice
    // slot held for the rest of the song, and with a pool of them that is a
    // kit that stops sounding after a few bars.
    for model in DrumModel::ALL {
        let voice = DrumVoice {
            model,
            decay_s: 0.2,
            ..kick()
        };
        let mut synth = DrumSynth::new();
        synth.trigger(&voice, SR);
        assert!(!synth.is_done(), "{model:?} was done before it started");
        let mut tail = Vec::new();
        for _ in 0..(SR * 4.0) as usize {
            tail.push(synth.next_sample(&voice, SR));
        }
        assert!(synth.is_done(), "{model:?} never finished");
        assert!(
            rms(window(&tail, 3.0, 4.0)) < 1e-4,
            "{model:?} is still ringing four seconds later"
        );
    }
}

#[test]
fn a_fresh_synth_is_silent_until_it_is_triggered() {
    // Voices come out of a pool. One that made a sound before anybody hit it
    // would be a drum machine that plays itself.
    let voice = kick();
    let mut synth = DrumSynth::new();
    assert!(synth.is_done(), "an untriggered voice is not sounding");
    let quiet: Vec<f32> = (0..1000).map(|_| synth.next_sample(&voice, SR)).collect();
    assert_eq!(peak(&quiet), 0.0);
}

#[test]
fn triggering_again_starts_the_hit_over_rather_than_layering_it() {
    // A closed hat played sixteen times a bar is one voice retriggered, and a
    // retrigger that added to what was there would grow without bound.
    let voice = kick();
    let mut synth = DrumSynth::new();
    synth.trigger(&voice, SR);
    for _ in 0..(SR * 0.2) as usize {
        synth.next_sample(&voice, SR);
    }
    synth.trigger(&voice, SR);
    let again: Vec<f32> = (0..(SR * 0.01) as usize)
        .map(|_| synth.next_sample(&voice, SR))
        .collect();
    let fresh = render(&voice, 0.01);
    assert!(
        (peak(&again) - peak(&fresh)).abs() < 0.05,
        "a retrigger is a fresh hit: {} against {}",
        peak(&again),
        peak(&fresh)
    );
}

// -------------------------------------------------------- the knobs work ---

#[test]
fn decay_is_how_long_the_hit_lasts() {
    let short = render(
        &DrumVoice {
            decay_s: 0.08,
            ..kick()
        },
        2.0,
    );
    let long = render(
        &DrumVoice {
            decay_s: 0.8,
            ..kick()
        },
        2.0,
    );
    let at = |out: &[f32]| rms(window(out, 0.25, 0.35));
    assert!(
        at(&long) > at(&short) * 4.0,
        "a long decay is still sounding where a short one has gone: {} against {}",
        at(&long),
        at(&short)
    );
}

#[test]
fn tune_moves_the_pitch_of_the_body() {
    // Measured as zero crossings rather than by ear: a hit tuned an octave up
    // crosses zero about twice as often.
    let crossings = |out: &[f32]| {
        window(out, 0.05, 0.25)
            .windows(2)
            .filter(|w| (w[0] < 0.0) != (w[1] < 0.0))
            .count()
    };
    let low = render(
        &DrumVoice {
            tune_hz: 50.0,
            bend_semitones: 0.0,
            noise: 0.0,
            snap: 0.0,
            ..kick()
        },
        1.0,
    );
    let high = render(
        &DrumVoice {
            tune_hz: 100.0,
            bend_semitones: 0.0,
            noise: 0.0,
            snap: 0.0,
            ..kick()
        },
        1.0,
    );
    assert!(
        crossings(&high) > crossings(&low) * 3 / 2,
        "{} crossings against {}",
        crossings(&high),
        crossings(&low)
    );
}

#[test]
fn the_bend_starts_the_hit_above_where_it_settles() {
    // The pitch drop is what makes a kick a kick rather than a low sine.
    let crossings = |out: &[f32], from: f32, to: f32| {
        window(out, from, to)
            .windows(2)
            .filter(|w| (w[0] < 0.0) != (w[1] < 0.0))
            .count() as f32
            / (to - from)
    };
    let voice = DrumVoice {
        bend_semitones: 36.0,
        bend_s: 0.08,
        decay_s: 0.6,
        noise: 0.0,
        snap: 0.0,
        ..kick()
    };
    let out = render(&voice, 1.0);
    assert!(
        crossings(&out, 0.0, 0.03) > crossings(&out, 0.3, 0.5) * 1.5,
        "the hit does not start above where it ends: {} then {}",
        crossings(&out, 0.0, 0.03),
        crossings(&out, 0.3, 0.5)
    );
}

#[test]
fn noise_is_the_balance_between_the_body_and_the_hiss() {
    // Zero noise on a tuned body is a periodic signal; full noise is not.
    // Measured as how well the signal predicts itself one period later.
    let periodicity = |out: &[f32]| {
        let w = window(out, 0.02, 0.12);
        let lag = (SR / 200.0) as usize;
        if w.len() <= lag {
            return 0.0;
        }
        let pairs: f32 = w[lag..].iter().zip(w).map(|(a, b)| a * b).sum();
        let power: f32 = w.iter().map(|s| s * s).sum();
        if power <= 0.0 {
            0.0
        } else {
            (pairs / power).abs()
        }
    };
    let tone = DrumVoice {
        model: DrumModel::Tom,
        tune_hz: 200.0,
        bend_semitones: 0.0,
        noise: 0.0,
        snap: 0.0,
        decay_s: 0.5,
        ..kick()
    };
    let hiss = DrumVoice { noise: 1.0, ..tone };
    assert!(
        periodicity(&render(&tone, 0.5)) > periodicity(&render(&hiss, 0.5)),
        "noise did not make the hit less periodic"
    );
}

#[test]
fn tone_opens_and_closes_the_noise() {
    // The brightness control. A hat with its tone right down is a thud.
    let dark = DrumVoice {
        model: DrumModel::ClosedHat,
        tone_hz: 800.0,
        ..kick()
    };
    let bright = DrumVoice {
        tone_hz: 12_000.0,
        ..dark
    };
    // Energy above the corner: how often it crosses zero stands in for it.
    let rate = |out: &[f32]| {
        window(out, 0.0, 0.05)
            .windows(2)
            .filter(|w| (w[0] < 0.0) != (w[1] < 0.0))
            .count()
    };
    assert!(
        rate(&render(&bright, 0.5)) > rate(&render(&dark, 0.5)),
        "the bright hat is not brighter"
    );
}

#[test]
fn gain_is_in_decibels_and_zero_changes_nothing() {
    let plain = render(&kick(), 1.0);
    let same = render(
        &DrumVoice {
            gain_db: 0.0,
            ..kick()
        },
        1.0,
    );
    assert_eq!(peak(&plain), peak(&same), "0 dB is the identity");
    let quiet = render(
        &DrumVoice {
            gain_db: -12.0,
            ..kick()
        },
        1.0,
    );
    let ratio = peak(&quiet) / peak(&plain);
    assert!(
        (ratio - 0.25).abs() < 0.05,
        "-12 dB should be a quarter of the amplitude, got {ratio}"
    );
}

#[test]
fn drive_makes_it_louder_and_flatter_without_leaving_full_scale() {
    let clean = DrumVoice {
        drive: 0.0,
        ..kick()
    };
    let dirty = DrumVoice {
        drive: 1.0,
        ..kick()
    };
    let a = render(&clean, 1.0);
    let b = render(&dirty, 1.0);
    // A driven hit holds more energy for the same peak — that is what
    // saturation is.
    assert!(peak(&b) <= 1.0, "driven to {}", peak(&b));
    assert!(
        rms(window(&b, 0.0, 0.2)) > rms(window(&a, 0.0, 0.2)),
        "drive did not make it denser"
    );
}

#[test]
fn every_body_shape_is_a_different_sound() {
    // The chiptune kit is a square kit, and this is what makes that possible
    // without a second synth.
    let of = |body| {
        render(
            &DrumVoice {
                model: DrumModel::Tom,
                body,
                bend_semitones: 0.0,
                noise: 0.0,
                snap: 0.0,
                ..kick()
            },
            0.3,
        )
    };
    let sine = of(DrumBody::Sine);
    let square = of(DrumBody::Square);
    let different = sine
        .iter()
        .zip(&square)
        .filter(|(a, b)| (*a - *b).abs() > 0.01)
        .count();
    assert!(
        different > sine.len() / 10,
        "a square body renders the same as a sine one"
    );
}

// ------------------------------------------------------------ robustness ---

#[test]
fn nonsense_settings_produce_silence_rather_than_a_crash_or_a_scream() {
    // A patch out of a hand-edited project file, or one written by a build
    // that had wider ranges. Nothing here may produce a NaN: one NaN on the
    // bus poisons every sample after it, right through the master.
    let nonsense = [
        DrumVoice {
            tune_hz: 0.0,
            ..kick()
        },
        DrumVoice {
            tune_hz: -100.0,
            ..kick()
        },
        DrumVoice {
            tune_hz: 1.0e9,
            ..kick()
        },
        DrumVoice {
            decay_s: 0.0,
            ..kick()
        },
        DrumVoice {
            decay_s: -1.0,
            ..kick()
        },
        DrumVoice {
            bend_s: 0.0,
            ..kick()
        },
        DrumVoice {
            tone_hz: 0.0,
            ..kick()
        },
        DrumVoice {
            tone_hz: 1.0e9,
            ..kick()
        },
        DrumVoice {
            noise: 9.0,
            ..kick()
        },
        DrumVoice {
            noise: -9.0,
            ..kick()
        },
        DrumVoice {
            drive: 100.0,
            ..kick()
        },
        DrumVoice {
            gain_db: 200.0,
            ..kick()
        },
    ];
    for voice in nonsense {
        let out = render(&voice, 0.5);
        assert!(
            out.iter().all(|s| s.is_finite()),
            "{voice:?} produced a NaN or an infinity"
        );
        assert!(peak(&out) <= 1.0, "{voice:?} peaked at {}", peak(&out));
    }
}

#[test]
fn a_sample_rate_of_nothing_is_survived() {
    // Belt and braces: the device's rate reaches here from outside, and a
    // divide by it is a divide by whatever it says.
    let voice = kick();
    let mut synth = DrumSynth::new();
    synth.trigger(&voice, 0.0);
    for _ in 0..100 {
        assert!(synth.next_sample(&voice, 0.0).is_finite());
    }
}

// ------------------------------------------------- metal and crush ---
//
// > *"the drumkits in the drum machine kind of all sound very similar"*
//
// Measured, and the report was right: with white noise through one filter as
// the only noise source, every kit's hat came out with its centre between 10
// and 13 kHz and the same 25 ms length, and two kicks were the same sine with
// two decays. What tells an 808 from a 909 from a LinnDrum is the *source* —
// six square oscillators, white noise, or eight-bit samples — and those are
// the two knobs below.

/// The Hann-windowed spectrum of the first 4096 samples, one magnitude per
/// DFT bin from `lo_hz` to `hi_hz`.
///
/// A direct evaluation rather than an FFT crate, because a test that pulls in
/// a dependency to ask "is this metallic" is a test that has stopped being
/// about the drum. Windowed, so a line falls into its bin rather than leaking
/// into the gaps this is about to measure.
fn spectrum(out: &[f32], lo_hz: f32, hi_hz: f32) -> Vec<f32> {
    let n = out.len().min(4096);
    let windowed: Vec<f32> = out[..n]
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let w = 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / n as f32).cos();
            s * w
        })
        .collect();
    let bin_hz = SR / n as f32;
    let first = (lo_hz / bin_hz).ceil() as usize;
    let last = (hi_hz / bin_hz).floor() as usize;
    (first..=last)
        .map(|k| {
            let (mut re, mut im) = (0.0f32, 0.0f32);
            for (i, s) in windowed.iter().enumerate() {
                let phase = 2.0 * std::f32::consts::PI * k as f32 * i as f32 / n as f32;
                re += s * phase.cos();
                im += s * phase.sin();
            }
            (re * re + im * im).sqrt()
        })
        .collect()
}

/// Spectral flatness: the geometric mean of the power over the arithmetic
/// mean. White noise is near one; a handful of lines with gaps between them
/// is near zero. The standard reading of "is this a hiss or a tone".
fn flatness(spectrum: &[f32]) -> f32 {
    let power: Vec<f32> = spectrum.iter().map(|m| m * m + 1e-12).collect();
    let geometric = (power.iter().map(|p| p.ln()).sum::<f32>() / power.len() as f32).exp();
    let arithmetic = power.iter().sum::<f32>() / power.len() as f32;
    geometric / arithmetic
}

fn closed_hat() -> DrumVoice {
    DrumVoice {
        model: DrumModel::ClosedHat,
        tune_hz: 320.0,
        decay_s: 0.08,
        tone_hz: 7_000.0,
        noise: 1.0,
        snap: 0.1,
        drive: 0.0,
        ..kick()
    }
}

#[test]
fn a_fresh_voice_has_no_metal_and_no_crush() {
    // Both knobs are additions. A voice read out of a project written before
    // they existed has to sound the way it did, so their resting value is the
    // sound they were added to.
    let voice = DrumVoice::default();
    assert_eq!(voice.metal, 0.0);
    assert_eq!(voice.crush, 0.0);
}

#[test]
fn metal_puts_pitches_in_the_noise() {
    // White noise is flat; six square waves are lines with gaps between them.
    // Spectral flatness tells the two apart without anybody listening, and it
    // is the difference between a hiss and an 808 hat.
    let hiss = render(
        &DrumVoice {
            metal: 0.0,
            ..closed_hat()
        },
        0.3,
    );
    let metal = render(
        &DrumVoice {
            metal: 1.0,
            ..closed_hat()
        },
        0.3,
    );
    assert!(peak(&metal) > 0.05, "the metal hat is silent");
    let (flat_hiss, flat_metal) = (
        flatness(&spectrum(&hiss, 6_000.0, 20_000.0)),
        flatness(&spectrum(&metal, 6_000.0, 20_000.0)),
    );
    assert!(
        flat_metal < flat_hiss * 0.5,
        "metal {flat_metal:.3} against noise {flat_hiss:.3}: the bank is not showing"
    );
    // And half way is half way: a knob, not a switch. Flatness cannot see
    // the middle — noise fills the gaps between the lines at any blend — but
    // the *floor* of those gaps falls as the noise is turned down, and that
    // is what a half-metal hat sounds like: the same lines over less hiss.
    let floor = |out: &[f32]| {
        let mut power: Vec<f32> = spectrum(out, 6_000.0, 20_000.0)
            .iter()
            .map(|m| m * m)
            .collect();
        let mean = power.iter().sum::<f32>() / power.len() as f32;
        power.sort_by(|a, b| a.partial_cmp(b).unwrap());
        power[power.len() / 10] / mean.max(1e-12)
    };
    let half = render(
        &DrumVoice {
            metal: 0.5,
            ..closed_hat()
        },
        0.3,
    );
    let (floor_hiss, floor_half, floor_metal) = (floor(&hiss), floor(&half), floor(&metal));
    assert!(
        floor_metal < floor_half && floor_half < floor_hiss,
        "metal is not a sweep: {floor_metal:.4} {floor_half:.4} {floor_hiss:.4}"
    );
}

#[test]
fn a_metal_hat_follows_the_tune() {
    // The bank is tuned from `tune_hz`, which is what makes a 606's hats sit
    // higher than an 808's rather than being the same hat twice. Checked at
    // the bottom of the bank, with the filter opened so the fundamentals get
    // through: a hat tuned to 200 Hz has a line there and one tuned to 400
    // does not.
    let open = DrumVoice {
        metal: 1.0,
        tone_hz: 100.0,
        ..closed_hat()
    };
    let low = render(
        &DrumVoice {
            tune_hz: 200.0,
            ..open
        },
        0.3,
    );
    let high = render(
        &DrumVoice {
            tune_hz: 400.0,
            ..open
        },
        0.3,
    );
    let at_200 = |out: &[f32]| {
        spectrum(out, 180.0, 220.0)
            .into_iter()
            .fold(0.0f32, f32::max)
    };
    assert!(
        at_200(&low) > at_200(&high) * 4.0,
        "200 Hz reads {:.2} on the low hat and {:.2} on the high one",
        at_200(&low),
        at_200(&high)
    );
}

#[test]
fn crush_holds_samples_and_coarsens_them() {
    // Sample-rate reduction is a held value: at full crush most samples equal
    // the one before them, which a clean render essentially never does.
    let held = |out: &[f32]| {
        let pairs = window(out, 0.0, 0.1).windows(2);
        let total = pairs.len().max(1);
        let same = pairs.filter(|w| w[0] == w[1]).count();
        same as f32 / total as f32
    };
    let clean = render(
        &DrumVoice {
            crush: 0.0,
            ..kick()
        },
        0.3,
    );
    let crushed = render(
        &DrumVoice {
            crush: 1.0,
            ..kick()
        },
        0.3,
    );
    assert!(
        held(&clean) < 0.05,
        "a clean kick holds {:.0}% of its samples",
        held(&clean) * 100.0
    );
    assert!(
        held(&crushed) > 0.5,
        "a crushed kick holds only {:.0}% of its samples",
        held(&crushed) * 100.0
    );
    // And it still ends, inside full scale.
    assert!(peak(&crushed) <= 1.0);
    assert!(peak(&crushed) > 0.1, "crushing silenced the kick");
    let mut synth = DrumSynth::new();
    let voice = DrumVoice {
        crush: 1.0,
        ..kick()
    };
    synth.trigger(&voice, SR);
    for _ in 0..(2.0 * SR) as usize {
        synth.next_sample(&voice, SR);
    }
    assert!(synth.is_done(), "a crushed kick never finishes");
}

#[test]
fn half_crush_is_between_none_and_all() {
    // A knob, not a switch: the reduction has to move with it or the panel
    // has a control with two positions and a lot of travel between them.
    let held = |out: &[f32]| {
        let pairs = window(out, 0.0, 0.1).windows(2);
        let total = pairs.len().max(1);
        pairs.filter(|w| w[0] == w[1]).count() as f32 / total as f32
    };
    let a = held(&render(
        &DrumVoice {
            crush: 0.3,
            ..kick()
        },
        0.3,
    ));
    let b = held(&render(
        &DrumVoice {
            crush: 0.7,
            ..kick()
        },
        0.3,
    ));
    let c = held(&render(
        &DrumVoice {
            crush: 1.0,
            ..kick()
        },
        0.3,
    ));
    assert!(
        a > 0.0 && a < b && b < c,
        "crush is not monotonic: {a:.2} {b:.2} {c:.2}"
    );
}

#[test]
fn metal_and_crush_are_clamped_like_everything_else() {
    // A hand-edited project with `metal: 40` is a loud hat, not a NaN.
    for voice in [
        DrumVoice {
            metal: 40.0,
            ..closed_hat()
        },
        DrumVoice {
            metal: -3.0,
            ..closed_hat()
        },
        DrumVoice {
            crush: 9.0,
            ..kick()
        },
        DrumVoice {
            crush: f32::NAN,
            ..kick()
        },
        DrumVoice {
            metal: f32::INFINITY,
            ..closed_hat()
        },
    ] {
        let out = render(&voice, 0.3);
        assert!(
            out.iter().all(|s| s.is_finite()),
            "{voice:?} produced a NaN"
        );
        assert!(peak(&out) <= 1.0, "{voice:?} left full scale");
    }
}

// ------------------------------------------------ realism: the modes ------
//
// > *"the sounds in it still sound way too synthesized and not realistic
// > enough and not diverse enough ... ultimately still just sounding like
// > tweaked versions of the same synthesized sounding sounds."*
//
// The cause, measured before it was fixed: every pitched hit was **one**
// oscillator. A kick, a tom and a conga were the same sine with three
// envelopes on it, and no setting of tune, bend or decay could make one of
// them ring like a struck head, because what a struck head does — ring at a
// set of inharmonic modes that die at different rates — was not in the model
// at all. `modes`, `tail` and `rattle` are the three that put it there, and
// these are the tests that say they are doing something.

/// A tom with its modes up has energy **off** the fundamental's harmonics.
///
/// That is the whole claim. A sine, however enveloped, puts its energy at one
/// frequency; a membrane puts it at 1.00, 1.59, 2.14 … of it, and the ear
/// reads the difference as a drum rather than as a tone.
#[test]
fn modes_put_energy_where_a_sine_has_none() {
    let plain = DrumVoice {
        model: DrumModel::Tom,
        tune_hz: 120.0,
        noise: 0.0,
        snap: 0.0,
        drive: 0.0,
        decay_s: 0.6,
        modes: 0.0,
        ..DrumVoice::default()
    };
    let modal = DrumVoice {
        modes: 1.0,
        ..plain
    };
    // 1.50 × 120 = 180 Hz — the tom's second mode, and a frequency no
    // harmonic of 120 lands on (120, 240, 360 …).
    let band = |voice: &DrumVoice| energy_between(&render(voice, 0.6), 172.0, 188.0);
    assert!(
        band(&modal) > band(&plain) * 8.0,
        "the second mode is not there: {:.6} against {:.6}",
        band(&modal),
        band(&plain)
    );
}

/// A **membrane's** modes are inharmonic, and a conga is the model that has
/// them: 1.000, 1.593, 2.135, 2.917 — Bessel zeros, which land on no harmonic
/// of the fundamental. That is what stops a struck head fusing into a note.
///
/// A tom is deliberately **not** tested here, and the reason is worth writing
/// down rather than discovering twice: a real tom is a membrane loaded by the
/// air in its shell, and that loading pulls the low modes towards 1.50, 1.75
/// and 2.00 — nearly harmonic, which is exactly *why* a tuned tom sounds like
/// a note where a conga does not. Asserting inharmonicity there would be
/// asserting that the drum is built wrong.
#[test]
fn a_membranes_modes_are_not_harmonics_of_its_fundamental() {
    let voice = DrumVoice {
        model: DrumModel::Perc,
        tune_hz: 100.0,
        noise: 0.0,
        snap: 0.0,
        drive: 0.0,
        decay_s: 0.6,
        modes: 1.0,
        ..DrumVoice::default()
    };
    let rendered = render(&voice, 0.6);
    // The second harmonic of 100 Hz is 200; the second *mode* is at 159.3.
    // If the bank were harmonic the first would be the louder of the two.
    let harmonic = energy_between(&rendered, 194.0, 206.0);
    let mode = energy_between(&rendered, 153.0, 165.0);
    assert!(
        mode > harmonic * 2.0,
        "the bank is harmonic: mode {mode:.6}, harmonic {harmonic:.6}"
    );
}

/// `tail` puts a slower decay under the fast one — which is what a real drum
/// does and what one exponential cannot.
#[test]
fn a_tail_keeps_ringing_after_the_hit_has_gone() {
    let dry = DrumVoice {
        model: DrumModel::Tom,
        tune_hz: 100.0,
        decay_s: 0.25,
        modes: 1.0,
        tail: 0.0,
        noise: 0.0,
        ..DrumVoice::default()
    };
    let rung = DrumVoice { tail: 1.0, ..dry };
    let late = |voice: &DrumVoice| {
        let out = render(voice, 1.2);
        rms(&out[out.len() * 2 / 3..])
    };
    assert!(
        late(&rung) > late(&dry) * 3.0,
        "the tail is not there: {:.6} against {:.6}",
        late(&rung),
        late(&dry)
    );
}

/// `rattle` rings the noise through a resonance — a snare's wires against its
/// shell, which flat filtered noise cannot be.
#[test]
fn rattle_gives_the_noise_a_resonance_of_its_own() {
    let flat = DrumVoice {
        model: DrumModel::Snare,
        tune_hz: 190.0,
        noise: 0.9,
        decay_s: 0.25,
        rattle: 0.0,
        ..DrumVoice::default()
    };
    let wires = DrumVoice {
        rattle: 1.0,
        ..flat
    };
    // A resonance is a peak, and a peak is crest factor: the same energy
    // through a narrower band arrives less flat than it left.
    let peaky = |voice: &DrumVoice| {
        let out = render(voice, 0.4);
        let peak = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        peak / rms(&out).max(1e-9)
    };
    assert!(
        (peaky(&wires) - peaky(&flat)).abs() > 0.15,
        "rattle changed nothing: {:.3} against {:.3}",
        peaky(&wires),
        peaky(&flat)
    );
}

/// **Old kits sound exactly as they did.** The three knobs read with serde
/// defaults, and at their defaults the voice is the one that was there before
/// — otherwise every project made until today would open sounding different.
#[test]
fn a_voice_from_before_the_modes_existed_is_unchanged() {
    let old: DrumVoice = serde_json::from_str(
        r#"{"model":"Tom","body":"Sine","tune_hz":120.0,
        "bend_semitones":6.0,"bend_s":0.05,"decay_s":0.4,"tone_hz":900.0,
        "noise":0.2,"snap":0.2,"drive":0.1,"gain_db":0.0}"#,
    )
    .expect("a kit written before the knobs existed");
    assert_eq!(old.modes, 0.0);
    assert_eq!(old.tail, 0.0);
    assert_eq!(old.rattle, 0.0);
}
