//! Flopsynth's **string** source: a stiff string, as a bank of decaying
//! partials rather than as a table.
//!
//! Three reports said the Grand Piano did not sound like one, and three rounds
//! of voicing could not fix it, because a wavetable is periodic and therefore
//! **exactly harmonic by construction** — while a real string is stiff, so its
//! nth partial sits at `n·f0·sqrt(1 + B·n²)`, sharp of where a table puts it
//! (`examples/piano_partials.rs` measured the gap: +48 cents at the 12th
//! partial of middle C, against the table's 0). That stretch is why tuners
//! stretch-tune, and it is most of what separates "struck string" from "organ
//! with a decay". The other half is that a string's high partials die first,
//! which a static table under a filter can only imitate.
//!
//! What these tests hold is the physics, not a piano: the partials are where
//! a stiff string puts them, they decay the way a damped one does, the hammer
//! lands where the strike knob says, and a harder strike is a brighter one.

use fontelle_dsp::{
    SynthInput, SynthOsc, SynthSource, SynthState, Unison, UnisonMode, UnisonSpread,
};

const SR: f32 = 48_000.0;

fn string() -> SynthOsc {
    SynthOsc {
        source: SynthSource::String,
        ..SynthOsc::default()
    }
}

fn render(config: &SynthOsc, note_hz: f32, frames: usize) -> Vec<f32> {
    let mut state = SynthState::new();
    state.reset(config, 1);
    (0..frames)
        .map(|_| {
            state
                .next_sample_from(config, SynthInput::None, note_hz, SR, 0.0)
                .0
        })
        .collect()
}

/// A Hann-windowed DFT at `hz`, in linear magnitude.
fn energy_at(samples: &[f32], hz: f32) -> f32 {
    let n = samples.len() as f32;
    let (mut re, mut im) = (0.0f32, 0.0f32);
    for (i, s) in samples.iter().enumerate() {
        let t = i as f32 / n;
        let window = 0.5 - 0.5 * (std::f32::consts::TAU * t).cos();
        let phase = std::f32::consts::TAU * hz * i as f32 / SR;
        re += s * window * phase.cos();
        im -= s * window * phase.sin();
    }
    (re * re + im * im).sqrt() / n
}

/// Where the strongest line within ±`span` cents of `hz` actually sits, in
/// cents from `hz`.
fn peak_offset_cents(samples: &[f32], hz: f32, span: f32) -> f32 {
    let mut best = (f32::MIN, 0.0f32);
    let mut cents = -span;
    while cents <= span {
        let e = energy_at(samples, hz * 2f32.powf(cents / 1200.0));
        if e > best.0 {
            best = (e, cents);
        }
        cents += 2.0;
    }
    best.1
}

fn rms(samples: &[f32]) -> f32 {
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len().max(1) as f32).sqrt()
}

#[test]
fn the_fundamental_is_the_note() {
    for hz in [65.4f32, 261.6, 1046.5] {
        let out = render(&string(), hz, 32_768);
        let off = peak_offset_cents(&out, hz, 60.0);
        assert!(
            off.abs() <= 4.0,
            "the fundamental at {hz} Hz sits {off} cents off"
        );
    }
}

/// The whole point. A stiff string's 8th partial is well sharp of 8·f0; a
/// string with no stiffness is harmonic, which is what a table already is.
#[test]
fn a_stiff_string_stretches_its_partials() {
    let f0 = 261.6;
    let mut stiff = string();
    stiff.string.stiffness = 0.5;
    let out = render(&stiff, f0, 65_536);
    let eighth = peak_offset_cents(&out, f0 * 8.0, 150.0);
    let second = peak_offset_cents(&out, f0 * 2.0, 150.0);
    assert!(
        eighth > 20.0,
        "the 8th partial of a stiff string should be well sharp: {eighth} cents"
    );
    assert!(
        eighth > second + 10.0,
        "the stretch grows with the partial: 2nd at {second}, 8th at {eighth}"
    );

    let mut floppy = string();
    floppy.string.stiffness = 0.0;
    let out = render(&floppy, f0, 65_536);
    let eighth = peak_offset_cents(&out, f0 * 8.0, 150.0);
    assert!(
        eighth.abs() <= 6.0,
        "with no stiffness the partials are harmonic: 8th at {eighth} cents"
    );
}

/// The stretch follows the physics, not just the knob: the same knob is
/// stiffer on a short treble string than a long bass one.
#[test]
fn the_treble_is_stiffer_than_the_bass_at_one_setting() {
    let mut s = string();
    s.string.stiffness = 0.4;
    let bass = render(&s, 65.4, 65_536);
    let treble = render(&s, 1046.5, 65_536);
    let bass_4th = peak_offset_cents(&bass, 65.4 * 4.0, 150.0);
    let treble_4th = peak_offset_cents(&treble, 1046.5 * 4.0, 150.0);
    assert!(
        treble_4th > bass_4th + 5.0,
        "the 4th partial should stretch more up the keyboard: bass {bass_4th}, \
         treble {treble_4th}"
    );
}

/// A damped string loses its top first: the 6th partial falls faster than
/// the fundamental, so the note darkens as it rings.
#[test]
fn high_partials_decay_faster_than_low_ones() {
    let f0 = 220.0;
    let mut s = string();
    s.string.damping = 0.6;
    s.string.decay_s = 2.0;
    s.position = 1.0;
    let out = render(&s, f0, 96_000);
    let early = &out[..16_384];
    let late = &out[64_000..64_000 + 16_384];
    let f1_fall = energy_at(late, f0) / energy_at(early, f0);
    let f6_fall = energy_at(late, f0 * 6.0) / energy_at(early, f0 * 6.0);
    assert!(
        f6_fall < f1_fall * 0.5,
        "the 6th partial should fall faster than the 1st: {f6_fall} against {f1_fall}"
    );
}

/// The position knob is the string's **brightness**: a harder strike rings
/// partials a soft one never reaches.
#[test]
fn a_brighter_setting_rings_the_upper_partials_harder() {
    let f0 = 220.0;
    let mut soft = string();
    soft.position = 0.1;
    let mut hard = string();
    hard.position = 0.9;
    let a = render(&soft, f0, 16_384);
    let b = render(&hard, f0, 16_384);
    let tilt = |s: &[f32]| energy_at(s, f0 * 7.0) / energy_at(s, f0);
    assert!(
        tilt(&b) > tilt(&a) * 3.0,
        "the 7th partial against the fundamental: soft {}, hard {}",
        tilt(&a),
        tilt(&b)
    );
}

/// Where the hammer lands decides which partials it cannot excite: struck a
/// quarter of the way along, the 4th partial has a node under the hammer.
#[test]
fn the_strike_point_leaves_a_hole_at_its_own_partial() {
    let f0 = 220.0;
    let mut quarter = string();
    quarter.string.strike = 0.25;
    quarter.position = 1.0;
    let mut eighth = string();
    eighth.string.strike = 0.125;
    eighth.position = 1.0;
    let a = render(&quarter, f0, 16_384);
    let b = render(&eighth, f0, 16_384);
    let fourth = |s: &[f32]| energy_at(s, f0 * 4.0) / energy_at(s, f0 * 3.0);
    assert!(
        fourth(&a) < fourth(&b) * 0.2,
        "struck at a quarter the 4th partial should be missing: {} against {} \
         struck at an eighth",
        fourth(&a),
        fourth(&b)
    );
}

/// A struck string starts from **rest**: displacement zero, with the hammer's
/// velocity. So the first sample is nothing and the note rises from it as a
/// slope rather than landing as a step — a step is the click the amp
/// envelope would then have to hide.
#[test]
fn it_starts_from_rest_without_a_click() {
    let out = render(&string(), 220.0, 4_800);
    let peak = out.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
    assert!(peak > 0.05, "the string rings at all: peak {peak}");
    assert!(
        out[0].abs() < 1e-6,
        "the first sample is rest, not a step: {}",
        out[0]
    );
    assert!(
        out[1].abs() < peak * 0.12,
        "the second sample should still be near rest: {} against a peak of {peak}",
        out[1]
    );
}

#[test]
fn the_decay_knob_is_how_long_it_rings() {
    let mut short = string();
    short.string.decay_s = 0.3;
    let mut long = string();
    long.string.decay_s = 4.0;
    let a = render(&short, 220.0, 48_000);
    let b = render(&long, 220.0, 48_000);
    let late = |s: &[f32]| rms(&s[36_000..]);
    assert!(
        late(&b) > late(&a) * 5.0,
        "a longer decay rings on: short {}, long {}",
        late(&a),
        late(&b)
    );
}

/// Three strings a cent or two apart, which is what a piano's unison is:
/// the sum beats, so the note's envelope is not one smooth fall.
#[test]
fn a_unison_of_strings_beats_like_a_trichord() {
    let mut s = string();
    s.string.decay_s = 6.0;
    s.string.damping = 0.0;
    s.unison = Unison {
        voices: 3,
        detune_cents: 6.0,
        blend: 1.0,
        width: 0.0,
        mode: UnisonMode::Classic,
        spread: UnisonSpread::Power,
    };
    let out = render(&s, 220.0, 96_000);
    let windows: Vec<f32> = out[4_800..].chunks(2_400).map(rms).collect();
    // Monotonic decay would make every window quieter than the last; a beat
    // makes some louder.
    let rises = windows.windows(2).filter(|w| w[1] > w[0] * 1.05).count();
    assert!(
        rises >= 3,
        "a detuned trichord should beat; the envelope only ever fell ({windows:?})"
    );
}

/// Above the top of the keyboard the bank has few partials and no aliases:
/// nothing folds back under the fundamental.
#[test]
fn a_high_string_has_no_energy_below_its_fundamental() {
    let f0 = 4_186.0;
    let mut s = string();
    s.position = 1.0;
    s.string.stiffness = 1.0;
    let out = render(&s, f0, 16_384);
    let fundamental = energy_at(&out, f0);
    assert!(fundamental > 0.001, "the note itself has to be there");
    let mut worst: f32 = 0.0;
    let mut hz = 200.0;
    while hz < f0 * 0.9 {
        worst = worst.max(energy_at(&out, hz));
        hz += 200.0;
    }
    assert!(
        worst < fundamental * 0.02,
        "aliases below the fundamental: worst {worst} against {fundamental}"
    );
}
