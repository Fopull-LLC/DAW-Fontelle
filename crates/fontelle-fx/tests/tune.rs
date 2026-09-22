//! What the pitch corrector promises (`docs/tune-plan.md` §9.3).
//!
//! The claims here are about **what comes out**, measured with a pitch tracker
//! over many blocks rather than by counting zero crossings — the trap the
//! `performance-events` memory records. Each one is a step of §3.3, in the
//! order §3.3 lists them.

use fontelle_dsp::{PitchTracker, hz_to_cents};
use fontelle_fx::{NoteInput, Tune};
use fontelle_types::{
    TuneConfig, TuneControl, TuneEngine, TunePreset, TuneRange, TuneScale, VibratoShape,
};

const RATE: f32 = 48_000.0;
const BLOCK: usize = 128;
const BPM: f32 = 120.0;

/// A buzz at `f0` through three resonances — the signal a tuner meets.
fn vowel_at(cents_of: &dyn Fn(f32) -> f32, seconds: f32) -> Vec<f32> {
    let n = (seconds * RATE) as usize;
    let formants = [700.0f32, 1_200.0, 2_600.0];
    let mut phase = 0.0f32;
    (0..n)
        .map(|i| {
            let t = i as f32 / RATE;
            let f0 = 440.0 * ((cents_of(t) - 6900.0) / 1200.0).exp2();
            phase += std::f32::consts::TAU * f0 / RATE;
            let partials = ((RATE / 2.0 / f0) as usize).clamp(1, 40);
            let mut sum = 0.0;
            for k in 1..=partials {
                let hz = f0 * k as f32;
                let mut gain = 0.0;
                for f in formants {
                    let bw = f * 0.10;
                    gain += 1.0 / (1.0 + ((hz - f) / bw).powi(2));
                }
                sum += gain / k as f32 * (phase * k as f32).sin();
            }
            sum * 0.12
        })
        .collect()
}

/// A steady note at `hz`.
fn steady(hz: f32, seconds: f32) -> Vec<f32> {
    let cents = hz_to_cents(hz);
    vowel_at(&|_| cents, seconds)
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

/// One signal through one corrector, mono, in place.
fn run(config: &TuneConfig, notes: NoteInput, signal: &[f32]) -> Vec<f32> {
    let mut tune = Tune::new();
    tune.prepare(RATE, config);
    let mut out = Vec::with_capacity(signal.len());
    let mut scratch = vec![0.0f32; BLOCK];
    for block in signal.chunks(BLOCK) {
        let frames = block.len();
        scratch[..frames].copy_from_slice(block);
        let (head, _) = scratch.split_at_mut(frames);
        let mut channels: [&mut [f32]; 1] = [head];
        tune.process(&mut channels, notes, config, BPM);
        out.extend_from_slice(&scratch[..frames]);
    }
    out
}

/// The same, keeping the corrector's own trace, hop by hop.
fn run_traced(
    config: &TuneConfig,
    notes: &dyn Fn(f32) -> NoteInput,
    signal: &[f32],
) -> (Vec<f32>, Vec<fontelle_types::TuneFrame>) {
    let mut tune = Tune::new();
    tune.prepare(RATE, config);
    let mut out = Vec::with_capacity(signal.len());
    let mut trace = Vec::new();
    let mut scratch = vec![0.0f32; BLOCK];
    for (index, block) in signal.chunks(BLOCK).enumerate() {
        let frames = block.len();
        scratch[..frames].copy_from_slice(block);
        let seconds = (index * BLOCK) as f32 / RATE;
        {
            let (head, _) = scratch.split_at_mut(frames);
            let mut channels: [&mut [f32]; 1] = [head];
            tune.process(&mut channels, notes(seconds), config, BPM);
        }
        out.extend_from_slice(&scratch[..frames]);
        trace.extend(tune.trace().iter().copied());
    }
    (out, trace)
}

fn no_notes() -> NoteInput {
    NoteInput::default()
}

/// The pitch of the output, hop by hop, in MIDI cents — `None` where unvoiced.
fn out_cents(signal: &[f32]) -> Vec<Option<f32>> {
    let mut tracker = PitchTracker::new(80.0, 1_200.0, 64);
    tracker.prepare(RATE);
    let mut found = Vec::new();
    for block in signal.chunks(BLOCK) {
        tracker.push(block, &mut |frame| found.push(frame.map(|f| f.cents)));
    }
    found
}

/// What the output settled at, in cents: the mean of the last quarter.
fn settled_cents(signal: &[f32]) -> f32 {
    let track = out_cents(signal);
    let tail: Vec<f32> = track[track.len() * 3 / 4..]
        .iter()
        .flatten()
        .copied()
        .collect();
    assert!(!tail.is_empty(), "nothing voiced came out");
    tail.iter().sum::<f32>() / tail.len() as f32
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

// ------------------------------------------------------------------ §4.7

#[test]
fn a_fresh_tune_is_a_tuner_and_not_a_wire() {
    // Thirty cents flat of A3 (220 Hz), on a fresh insert.
    let flat = hz_to_cents(220.0) - 30.0;
    let signal = vowel_at(&|_| flat, 1.5);
    let out = run(&TuneConfig::new(), no_notes(), &signal);
    let landed = settled_cents(&out);
    let want = hz_to_cents(220.0);
    assert!(
        (landed - want).abs() < 3.0,
        "a fresh tuner left it {:.1} cents off",
        landed - want
    );
}

#[test]
fn speech_is_left_alone() {
    // An unvoiced-heavy signal: the tracker never calls it a note, so the
    // corrector is the delayed input and nothing else.
    let config = TuneConfig::new();
    let signal = noise(48_000, 0x51ee_d101, 0.25);
    let out = run(&config, no_notes(), &signal);
    let latency = config.latency_samples(RATE) as usize;
    let start = latency + 4_000;
    let error: Vec<f32> = out[start..]
        .iter()
        .zip(&signal[start - latency..signal.len() - latency])
        .map(|(a, b)| a - b)
        .collect();
    let level = db(rms(&error) / rms(&signal[start..]).max(1e-9));
    assert!(level < -40.0, "the consonants were tuned: {level:.1} dB");
}

// ------------------------------------------------------- §3.3 steps 4–6

#[test]
fn retune_zero_snaps_within_two_hops() {
    let flat = hz_to_cents(220.0) - 40.0;
    let config = TuneConfig {
        retune_ms: fontelle_types::MIN_TUNE_RETUNE_MS,
        ..TuneConfig::new()
    };
    let (_, trace) = run_traced(&config, &|_| no_notes(), &vowel_at(&|_| flat, 1.0));
    let voiced: Vec<_> = trace
        .iter()
        .filter(|f| f.flags & fontelle_types::TUNE_VOICED != 0)
        .collect();
    assert!(voiced.len() > 20);
    // Two hops after the first voiced one, the correction is already there.
    let landed = voiced[2];
    let error = landed.out_cents - landed.target_cents;
    assert!(
        error.abs() < 6.0,
        "an instant retune was still {error:.1} cents out two hops in"
    );
}

#[test]
fn retune_two_hundred_takes_two_hundred_milliseconds_to_get_most_of_the_way() {
    let flat = hz_to_cents(220.0) - 50.0;
    let config = TuneConfig {
        retune_ms: 200.0,
        ..TuneConfig::new()
    };
    let (_, trace) = run_traced(&config, &|_| no_notes(), &vowel_at(&|_| flat, 2.0));
    let voiced: Vec<_> = trace
        .iter()
        .filter(|f| f.flags & fontelle_types::TUNE_VOICED != 0)
        .collect();
    let hops_per_200ms = (0.2 * RATE / 64.0) as usize;
    assert!(voiced.len() > hops_per_200ms + 4);
    let at = voiced[hops_per_200ms];
    let distance = at.target_cents - at.sung_cents;
    let travelled = at.out_cents - at.sung_cents;
    let fraction = travelled / distance;
    // One time constant is 63 % of the way, ± a tenth.
    assert!(
        (fraction - 0.63).abs() < 0.10,
        "a 200 ms retune was {:.0} % of the way after 200 ms",
        fraction * 100.0
    );
}

#[test]
fn amount_scales_the_correction() {
    let flat = hz_to_cents(220.0) - 40.0;
    let signal = vowel_at(&|_| flat, 1.5);
    let full = settled_cents(&run(&TuneConfig::new(), no_notes(), &signal));
    let half = settled_cents(&run(
        &TuneConfig {
            amount: 0.5,
            ..TuneConfig::new()
        },
        no_notes(),
        &signal,
    ));
    let moved_full = full - flat;
    let moved_half = half - flat;
    assert!(
        (moved_half / moved_full - 0.5).abs() < 0.12,
        "half the amount moved {:.0} % of the way",
        moved_half / moved_full * 100.0
    );
}

// -------------------------------------------------------------- §4.2 scale

/// 470 Hz sits between A#4 (466.16) and B4 (493.88), nearer the A#.
fn four_seventy() -> Vec<f32> {
    vowel_at(&|_| hz_to_cents(470.0), 1.5)
}

#[test]
fn in_c_major_four_seventy_hertz_goes_to_b() {
    let config = TuneConfig {
        scale: TuneScale::Major,
        root: 0,
        range: TuneRange::Soprano,
        ..TuneConfig::new()
    };
    let landed = settled_cents(&run(&config, no_notes(), &four_seventy()));
    let b4 = hz_to_cents(493.883);
    assert!(
        (landed - b4).abs() < 8.0,
        "C major sent 470 Hz to {:.0} cents, B is {b4:.0}",
        landed
    );
}

/// §1.5 says "with only C and G enabled it goes to G", about the same 470 Hz
/// tone — and 470 Hz is 185 cents under C5 and 315 cents over G4, so it goes
/// to the C. The claim is right and the note in the plan was picked wrong;
/// 420 Hz is the one that lands on the G.
#[test]
fn with_only_c_and_g_it_goes_to_g() {
    let config = TuneConfig {
        scale: TuneScale::Custom,
        notes: (1 << 0) | (1 << 7),
        range: TuneRange::Soprano,
        ..TuneConfig::new()
    };
    let landed = settled_cents(&run(
        &config,
        no_notes(),
        &vowel_at(&|_| hz_to_cents(420.0), 1.5),
    ));
    let g4 = hz_to_cents(391.995);
    let c5 = hz_to_cents(523.251);
    assert!(
        (landed - g4).abs() < 15.0,
        "two notes in the scale sent 420 Hz to {landed:.0} cents; G is {g4:.0} and C is {c5:.0}"
    );
}

#[test]
fn with_d_as_the_root_f_sharp_is_in_and_f_is_out() {
    let config = TuneConfig {
        scale: TuneScale::Major,
        root: 2,
        range: TuneRange::Soprano,
        ..TuneConfig::new()
    };
    // F#5 is 739.99; F5 is 698.46. A note between them must go to the F#.
    let between = vowel_at(
        &|_| (hz_to_cents(698.456) + hz_to_cents(739.989)) / 2.0,
        1.5,
    );
    let landed = settled_cents(&run(&config, no_notes(), &between));
    let f_sharp = hz_to_cents(739.989);
    assert!(
        (landed - f_sharp).abs() < 10.0,
        "D major sent it to {landed:.0} cents rather than to F# at {f_sharp:.0}"
    );
}

#[test]
fn the_chromatic_scale_sends_it_to_b_flat() {
    let config = TuneConfig {
        range: TuneRange::Soprano,
        ..TuneConfig::new()
    };
    let landed = settled_cents(&run(&config, no_notes(), &four_seventy()));
    let b_flat = hz_to_cents(466.164);
    assert!(
        (landed - b_flat).abs() < 6.0,
        "chromatic sent 470 Hz to {landed:.0} cents, A# is {b_flat:.0}"
    );
}

#[test]
fn a_custom_mask_is_read_only_when_the_scale_says_custom() {
    // The same mask, once under a named scale and once under Custom.
    let mask = (1 << 0) | (1 << 7);
    let named = TuneConfig {
        scale: TuneScale::Major,
        notes: mask,
        ..TuneConfig::new()
    };
    assert_eq!(named.active_mask(), TuneScale::Major.mask(0));
    let custom = TuneConfig {
        scale: TuneScale::Custom,
        notes: mask,
        ..named
    };
    assert_eq!(custom.active_mask(), mask);
}

#[test]
fn a_note_on_the_boundary_does_not_flip_between_two_targets() {
    // Exactly between A#4 and B4, wobbling by ten cents: a tuner with no
    // hysteresis picks a different note every hop.
    let centre = (hz_to_cents(466.164) + hz_to_cents(493.883)) / 2.0;
    let config = TuneConfig {
        scale: TuneScale::Major,
        range: TuneRange::Soprano,
        retune_ms: fontelle_types::MIN_TUNE_RETUNE_MS,
        ..TuneConfig::new()
    };
    let signal = vowel_at(
        &|t| centre + 10.0 * (std::f32::consts::TAU * 3.0 * t).sin(),
        2.0,
    );
    let (_, trace) = run_traced(&config, &|_| no_notes(), &signal);
    let targets: Vec<f32> = trace
        .iter()
        .filter(|f| f.flags & fontelle_types::TUNE_VOICED != 0)
        .map(|f| f.target_cents)
        .collect();
    let flips = targets
        .windows(2)
        .filter(|w| (w[1] - w[0]).abs() > 50.0)
        .count();
    assert!(
        flips <= 2,
        "the target flipped {flips} times on a note that never moved a semitone"
    );
}

#[test]
fn flex_leaves_a_far_off_note_mostly_alone() {
    let off = hz_to_cents(220.0) - 45.0;
    let signal = vowel_at(&|_| off, 1.5);
    let strict = settled_cents(&run(&TuneConfig::new(), no_notes(), &signal));
    let loose = settled_cents(&run(
        &TuneConfig {
            flex: 1.0,
            ..TuneConfig::new()
        },
        no_notes(),
        &signal,
    ));
    let want = hz_to_cents(220.0);
    let strict_fraction = (strict - off) / (want - off);
    let loose_fraction = (loose - off) / (want - off);
    assert!(
        strict_fraction > 0.95,
        "flex at zero corrected only {:.0} %",
        strict_fraction * 100.0
    );
    assert!(
        loose_fraction < 0.25,
        "flex at a hundred still corrected {:.0} %",
        loose_fraction * 100.0
    );
}

#[test]
fn humanize_lets_a_held_note_drift_back_and_corrects_a_moving_one_in_full() {
    let off = hz_to_cents(220.0) - 40.0;
    let held = vowel_at(&|_| off, 2.0);
    let want = hz_to_cents(220.0);
    let config = TuneConfig {
        humanize: 1.0,
        ..TuneConfig::new()
    };
    let drifted = settled_cents(&run(&config, no_notes(), &held));
    assert!(
        (drifted - off).abs() < 15.0,
        "a fully humanized held note settled at {:.0} cents rather than back at {off:.0}",
        drifted
    );
    // And a line that keeps moving is corrected in full: a slow chromatic
    // walk, where `settled` never gets to run, corrected exactly as hard with
    // the knob at the top as with it at the bottom. Measured against the
    // corrector with humanize off rather than against an absolute number,
    // because a rising line's distance to its nearest scale degree sweeps the
    // whole semitone by construction — the claim is that humanize does not
    // *weaken* it, not that a moving line is ever in tune.
    let moving = vowel_at(&|t| off + 900.0 * t, 2.0);
    let corrected = |humanize: f32| -> f32 {
        let config = TuneConfig {
            humanize,
            ..TuneConfig::new()
        };
        let (_, trace) = run_traced(&config, &|_| no_notes(), &moving);
        let late: Vec<_> = trace
            .iter()
            .filter(|f| f.flags & fontelle_types::TUNE_VOICED != 0)
            .skip(200)
            .collect();
        assert!(!late.is_empty());
        late.iter()
            .map(|f| {
                let distance = f.target_cents - f.sung_cents;
                if distance.abs() < 1.0 {
                    1.0
                } else {
                    (f.out_cents - f.sung_cents) / distance
                }
            })
            .sum::<f32>()
            / late.len() as f32
    };
    let with_humanize = corrected(1.0);
    let without = corrected(0.0);
    assert!(
        with_humanize > without - 0.1,
        "humanize weakened a moving line: {with_humanize:.2} against {without:.2}"
    );
    assert!(want > 0.0);
}

// ------------------------------------------------------------ §3.5 vibrato

/// The strength of a line at `hz` in a pitch track sampled once per hop.
fn line_in_track(track: &[Option<f32>], hz: f32, hop: f32) -> f32 {
    let values: Vec<f32> = track.iter().flatten().copied().collect();
    if values.len() < 32 {
        return 0.0;
    }
    let mean = values.iter().sum::<f32>() / values.len() as f32;
    let rate = RATE / hop;
    let mut re = 0.0f32;
    let mut im = 0.0f32;
    for (i, value) in values.iter().enumerate() {
        let phase = std::f32::consts::TAU * hz * i as f32 / rate;
        re += (value - mean) * phase.cos();
        im += (value - mean) * phase.sin();
    }
    2.0 * (re * re + im * im).sqrt() / values.len() as f32
}

#[test]
fn the_singers_vibrato_survives_at_one_hundred_and_is_flat_at_zero() {
    let centre = hz_to_cents(220.0);
    let signal = vowel_at(
        &|t| centre + 40.0 * (std::f32::consts::TAU * 6.0 * t).sin(),
        2.0,
    );
    let kept = out_cents(&run(&TuneConfig::new(), no_notes(), &signal));
    let flattened = out_cents(&run(
        &TuneConfig {
            natural_vibrato: 0.0,
            retune_ms: fontelle_types::MIN_TUNE_RETUNE_MS,
            ..TuneConfig::new()
        },
        no_notes(),
        &signal,
    ));
    let kept_line = line_in_track(&kept, 6.0, 64.0);
    let flat_line = line_in_track(&flattened, 6.0, 64.0);
    assert!(
        kept_line > 20.0,
        "the singer's vibrato came out only {kept_line:.0} cents deep"
    );
    assert!(
        flat_line < kept_line * 0.4,
        "flattening left {flat_line:.0} cents of a {kept_line:.0} cent wobble"
    );
}

#[test]
fn added_vibrato_has_its_depth_its_rate_and_its_onset() {
    let config = TuneConfig {
        vibrato_depth: 60.0,
        vibrato_rate_hz: 5.0,
        vibrato_onset_ms: 0.0,
        natural_vibrato: 0.0,
        retune_ms: fontelle_types::MIN_TUNE_RETUNE_MS,
        ..TuneConfig::new()
    };
    let signal = steady(220.0, 2.0);
    let track = out_cents(&run(&config, no_notes(), &signal));
    let at_rate = line_in_track(&track, 5.0, 64.0);
    let elsewhere = line_in_track(&track, 11.0, 64.0);
    assert!(
        at_rate > 35.0,
        "the added vibrato is only {at_rate:.0} cents deep"
    );
    assert!(
        at_rate > elsewhere * 3.0,
        "the wobble is not at the rate it was asked for"
    );

    // And the onset: with a second of delay, the first half second is flat.
    let late = TuneConfig {
        vibrato_onset_ms: 1_000.0,
        ..config
    };
    let (_, trace) = run_traced(&late, &|_| no_notes(), &signal);
    let hops = (0.4 * RATE / 64.0) as usize;
    let early: Vec<Option<f32>> = trace
        .iter()
        .take(hops)
        .map(|f| (f.flags & fontelle_types::TUNE_VOICED != 0).then_some(f.out_cents))
        .collect();
    assert!(
        line_in_track(&early, 5.0, 64.0) < 12.0,
        "the vibrato started before its onset"
    );
}

#[test]
fn a_synced_vibrato_takes_its_rate_from_the_tempo() {
    // An eighth at 120 bpm is a quarter of a second, so four hertz.
    let config = TuneConfig {
        vibrato_depth: 60.0,
        vibrato_sync: true,
        vibrato_division: fontelle_types::NoteDivision::Eighth,
        vibrato_onset_ms: 0.0,
        natural_vibrato: 0.0,
        vibrato_shape: VibratoShape::Sine,
        retune_ms: fontelle_types::MIN_TUNE_RETUNE_MS,
        ..TuneConfig::new()
    };
    assert!((config.vibrato_hz(BPM) - 4.0).abs() < 1e-3);
    let track = out_cents(&run(&config, no_notes(), &steady(220.0, 2.5)));
    let at_four = line_in_track(&track, 4.0, 64.0);
    let at_five_five = line_in_track(&track, 5.5, 64.0);
    assert!(
        at_four > at_five_five * 2.0,
        "the synced vibrato ran at the knob's rate, not the tempo's"
    );
}

// ---------------------------------------------------------------- §5 MIDI

fn held(key: u8) -> NoteInput {
    NoteInput {
        last: Some(key),
        mask: 1 << (key % 12),
        bend_cents: 0.0,
        ons: 0,
    }
}

#[test]
fn midi_melody_forces_the_held_key() {
    let config = TuneConfig {
        control: TuneControl::MidiMelody,
        retune_ms: 5.0,
        ..TuneConfig::new()
    };
    // A sung C4 (60) with E4 (64) held.
    let signal = vowel_at(&|_| 6_000.0, 1.5);
    let landed = settled_cents(&run(&config, held(64), &signal));
    assert!(
        (landed - 6_400.0).abs() < 15.0,
        "the held E4 did not win: {landed:.0} cents"
    );
}

#[test]
fn letting_go_returns_to_the_scale() {
    let config = TuneConfig {
        control: TuneControl::MidiMelody,
        retune_ms: 5.0,
        scale: TuneScale::Chromatic,
        ..TuneConfig::new()
    };
    // C4 sung thirty cents sharp; E4 held for the first second only.
    let signal = vowel_at(&|_| 6_030.0, 2.0);
    let (out, _) = run_traced(
        &config,
        &|seconds| {
            if seconds < 1.0 { held(64) } else { no_notes() }
        },
        &signal,
    );
    let track = out_cents(&out);
    let hops_per_second = (RATE / 64.0) as usize;
    let during: Vec<f32> = track[hops_per_second / 2..hops_per_second]
        .iter()
        .flatten()
        .copied()
        .collect();
    let after: Vec<f32> = track[track.len() * 3 / 4..]
        .iter()
        .flatten()
        .copied()
        .collect();
    let mean = |v: &[f32]| v.iter().sum::<f32>() / v.len() as f32;
    assert!(
        (mean(&during) - 6_400.0).abs() < 20.0,
        "the held key was not forced: {:.0}",
        mean(&during)
    );
    assert!(
        (mean(&after) - 6_000.0).abs() < 15.0,
        "letting go did not return to the scale: {:.0}",
        mean(&after)
    );
}

#[test]
fn the_last_key_held_wins() {
    let config = TuneConfig {
        control: TuneControl::MidiMelody,
        retune_ms: 5.0,
        ..TuneConfig::new()
    };
    let notes = NoteInput {
        last: Some(67),
        mask: (1 << 4) | (1 << 7),
        bend_cents: 0.0,
        ons: 0,
    };
    let landed = settled_cents(&run(&config, notes, &vowel_at(&|_| 6_000.0, 1.5)));
    assert!(
        (landed - 6_700.0).abs() < 15.0,
        "the first key held won: {landed:.0}"
    );
}

#[test]
fn midi_scale_uses_the_held_classes() {
    // Only F and A held: a 470 Hz note has to go to A rather than to the A#
    // the chromatic scale would send it to.
    let config = TuneConfig {
        control: TuneControl::MidiScale,
        range: TuneRange::Soprano,
        ..TuneConfig::new()
    };
    let notes = NoteInput {
        last: Some(69),
        mask: (1 << 5) | (1 << 9),
        bend_cents: 0.0,
        ons: 0,
    };
    let landed = settled_cents(&run(&config, notes, &four_seventy()));
    let a4 = hz_to_cents(440.0);
    assert!(
        (landed - a4).abs() < 15.0,
        "the held classes were ignored: {landed:.0} cents, A is {a4:.0}"
    );
}

#[test]
fn the_bend_moves_the_target_when_asked_and_not_otherwise() {
    let base = TuneConfig {
        control: TuneControl::MidiMelody,
        retune_ms: 5.0,
        ..TuneConfig::new()
    };
    let notes = NoteInput {
        last: Some(64),
        mask: 1 << 4,
        bend_cents: -80.0,
        ons: 0,
    };
    let signal = vowel_at(&|_| 6_000.0, 1.5);
    let bent = settled_cents(&run(&base, notes, &signal));
    assert!(
        (bent - 6_320.0).abs() < 20.0,
        "the bend did not reach the target: {bent:.0}"
    );
    let straight = settled_cents(&run(
        &TuneConfig {
            midi_bend: false,
            ..base
        },
        notes,
        &signal,
    ));
    assert!(
        (straight - 6_400.0).abs() < 15.0,
        "the bend moved the target with the switch off: {straight:.0}"
    );
}

// --------------------------------------------------------- §3.3 the yodel

#[test]
fn an_onset_restarts_the_glide_from_the_sung_pitch() {
    // A jump of a fourth, with a slow retune: the hop after the jump must be
    // at the *new sung* pitch, not at the old note and not at the new target.
    let config = TuneConfig {
        retune_ms: 100.0,
        ..TuneConfig::new()
    };
    let signal = vowel_at(
        &|t| {
            if t < 1.0 {
                hz_to_cents(220.0) - 30.0
            } else {
                hz_to_cents(293.66) - 30.0
            }
        },
        2.0,
    );
    let (_, trace) = run_traced(&config, &|_| no_notes(), &signal);
    let hop = 64.0;
    let jump_hop = (1.0 * RATE / hop) as usize;
    // Two hops after the jump has been noticed, the correction is still small.
    let after = trace[jump_hop + 6..jump_hop + 10]
        .iter()
        .filter(|f| f.flags & fontelle_types::TUNE_VOICED != 0)
        .map(|f| (f.out_cents - f.sung_cents).abs())
        .fold(0.0f32, f32::max);
    assert!(
        after < 18.0,
        "the glide did not restart at the new note: {after:.0} cents of correction already in"
    );
}

#[test]
fn transpose_and_detune_are_added_after_the_correction() {
    let signal = vowel_at(&|_| hz_to_cents(220.0) - 20.0, 1.5);
    let plain = settled_cents(&run(&TuneConfig::new(), no_notes(), &signal));
    let moved = settled_cents(&run(
        &TuneConfig {
            transpose: 5,
            detune_cents: -20.0,
            ..TuneConfig::new()
        },
        no_notes(),
        &signal,
    ));
    let want = plain + 500.0 - 20.0;
    assert!(
        (moved - want).abs() < 12.0,
        "transpose and detune landed at {moved:.0} rather than {want:.0}"
    );
}

// ----------------------------------------------------------- §3.6 the gate

#[test]
fn below_the_gate_nothing_is_corrected() {
    let config = TuneConfig {
        gate_db: -30.0,
        ..TuneConfig::new()
    };
    // Forty cents flat, but sixty decibels down.
    let signal: Vec<f32> = vowel_at(&|_| hz_to_cents(220.0) - 40.0, 1.0)
        .iter()
        .map(|s| s * 0.001)
        .collect();
    let out = run(&config, no_notes(), &signal);
    let latency = config.latency_samples(RATE) as usize;
    let start = latency + 4_000;
    let error: Vec<f32> = out[start..]
        .iter()
        .zip(&signal[start - latency..signal.len() - latency])
        .map(|(a, b)| a - b)
        .collect();
    assert!(
        db(rms(&error) / rms(&signal[start..]).max(1e-12)) < -40.0,
        "room noise got tuned"
    );
}

#[test]
fn strict_tracking_leaves_a_noisy_tone_alone_and_relaxed_corrects_it() {
    // A note forty cents flat with as much noise as tone.
    let clean = vowel_at(&|_| hz_to_cents(220.0) - 40.0, 2.0);
    let dirt = noise(clean.len(), 0x2468_ace0, 0.03);
    let signal: Vec<f32> = clean.iter().zip(&dirt).map(|(a, b)| a + b).collect();
    let voiced_hops = |tracking: f32| {
        let config = TuneConfig {
            tracking,
            ..TuneConfig::new()
        };
        let (_, trace) = run_traced(&config, &|_| no_notes(), &signal);
        trace
            .iter()
            .filter(|f| f.flags & fontelle_types::TUNE_VOICED != 0)
            .count()
    };
    let relaxed = voiced_hops(0.0);
    let strict = voiced_hops(1.0);
    assert!(
        relaxed > 500,
        "relaxed tracking found the note in only {relaxed} hops"
    );
    assert!(
        strict * 20 < relaxed,
        "strict tracking was no stricter: {strict} hops against {relaxed}"
    );
}

// ------------------------------------------------------------- §6 the bank

#[test]
fn the_three_archetypes_measure_differently() {
    // One glided vowel through three presets, on three axes: how fast the
    // correction settles, whether the formants moved, and whether the grains
    // modulate. A table of numbers is not a family until the numbers differ.
    let signal = vowel_at(
        &|t| hz_to_cents(220.0) - 45.0 + 30.0 * (std::f32::consts::TAU * 1.5 * t).sin(),
        2.0,
    );
    let mut settle = Vec::new();
    for preset in [
        TunePreset::Transparent,
        TunePreset::HardTune,
        TunePreset::CheapPlastic,
    ] {
        let config = TuneConfig::from_preset(preset);
        let (out, trace) = run_traced(&config, &|_| no_notes(), &signal);
        assert!(rms(&out) > 1e-4, "{} made no sound", preset.label());
        // How much of the distance to the target the correction has closed,
        // averaged over the second half.
        let closed: Vec<f32> = trace
            .iter()
            .skip(trace.len() / 2)
            .filter(|f| f.flags & fontelle_types::TUNE_VOICED != 0)
            .map(|f| {
                let distance = f.target_cents - f.sung_cents;
                if distance.abs() < 1.0 {
                    1.0
                } else {
                    (f.out_cents - f.sung_cents) / distance
                }
            })
            .collect();
        assert!(!closed.is_empty(), "{} never tracked", preset.label());
        settle.push((preset, closed.iter().sum::<f32>() / closed.len() as f32));
    }
    let transparent = settle[0].1;
    let hard = settle[1].1;
    assert!(
        hard > transparent + 0.25,
        "the hard tune corrects {hard:.2} and the transparent one {transparent:.2} — \
         they are the same effect with two names"
    );
    // And the engines differ, which is the axis the settling time cannot see.
    assert_eq!(
        TuneConfig::from_preset(TunePreset::CheapPlastic).engine,
        TuneEngine::Grain
    );
    assert_eq!(
        TuneConfig::from_preset(TunePreset::HardTune).engine,
        TuneEngine::Smooth
    );
}

/// Every preset, as an **insert** rather than as a bare corrector: the dry the
/// mix control blends back in is the delayed input, exactly as `EffectNode`
/// blends it. Two of these presets are half wet on purpose — a doubler under
/// the dry and a harmony beside it — and measuring only their wet half would
/// say they were three decibels quiet when what they are is half dry.
#[test]
fn every_tune_preset_makes_a_sound_within_three_decibels_of_the_wire() {
    let signal = vowel_at(&|_| hz_to_cents(220.0) - 20.0, 1.2);
    let reference = db(rms(&signal[signal.len() / 3..]));
    for preset in TunePreset::ALL {
        let config = TuneConfig::from_preset(preset);
        let latency = config.latency_samples(RATE) as usize;
        let wet = run(&config, held(64), &signal);
        let mix = config.mix;
        let insert: Vec<f32> = wet
            .iter()
            .enumerate()
            .map(|(i, sample)| {
                let dry = i.checked_sub(latency).map_or(0.0, |at| signal[at]);
                sample * mix + dry * (1.0 - mix)
            })
            .collect();
        let level = db(rms(&insert[insert.len() / 3..]));
        // A preset that sits part dry pays the mix law: `EffectNode` blends
        // with a gain each rather than an equal-power curve — which is what
        // parallel processing *means* — so half of a signal plus half of an
        // unrelated one is three decibels under either. That is the two
        // harmony presets' whole design, not a level fault, so their
        // allowance carries it.
        let allowed = if mix < 0.99 {
            3.0 - 20.0 * mix.log10()
        } else {
            3.0
        };
        assert!(
            (level - reference).abs() < allowed,
            "{} came out {:+.1} dB from the wire",
            preset.label(),
            level - reference
        );
    }
}

// ------------------------------------------------------------ character ---
//
// §4.8's four knobs: the colour a corrector puts on the voice after it has
// finished correcting it. They are what separates an expensive-sounding
// autotune from a cheap-sounding one, and the plan's original sixteen presets
// could only reach that axis through `engine` and `texture` — which change
// *how the grains are laid*, not what the voice is made of.
//
// Every one of them is measured on the **corrected** signal, so a knob that
// quietly changed the pitch would fail its neighbour's test too.

/// What **share** of the signal's energy sits above the voice's body.
///
/// A ratio rather than a level, because both of the things measured with it
/// change the level too: drive has an auto-gain and air is a shelf. Asking
/// "how much is up there" would then be answered by "the whole signal got
/// louder", which is not the question. A one-pole high pass at roughly 3 kHz
/// over the RMS of the whole — crude on purpose, since what is being asked is
/// "is there proportionally more up there".
fn high_share(signal: &[f32]) -> f32 {
    let a = 0.65f32;
    let mut prev_in = 0.0;
    let mut prev_out = 0.0;
    let mut high = 0.0;
    let mut all = 0.0;
    for &x in signal {
        let y = a * (prev_out + x - prev_in);
        prev_in = x;
        prev_out = y;
        high += y * y;
        all += x * x;
    }
    if all <= 1e-12 {
        return 0.0;
    }
    (high / all).sqrt()
}

#[test]
fn drive_adds_harmonics_without_moving_the_note() {
    // **A sine, at a level somebody records at.**
    //
    // Not `steady`'s vowel: forty harmonics through three formants already
    // put a quarter of their energy above 3 kHz, and a saturator's own
    // harmonics land in the same place and barely move the share. A sine has
    // none of its own, so everything the measure finds up there was made
    // here. And at 0.7 rather than the vowel's −20 dBFS, because the top of
    // the curve is the part being asked about.
    let signal: Vec<f32> = (0..(RATE as usize))
        .map(|i| (std::f32::consts::TAU * 220.0 * i as f32 / RATE).sin() * 0.7)
        .collect();
    let clean = TuneConfig {
        drive: 0.0,
        ..TuneConfig::new()
    };
    let driven = TuneConfig {
        drive: 0.9,
        ..TuneConfig::new()
    };
    let a = run(&clean, no_notes(), &signal);
    let b = run(&driven, no_notes(), &signal);
    assert!(
        high_share(&b) > high_share(&a) * 1.3,
        "drive put nothing on top: {:.5} against {:.5}",
        high_share(&b),
        high_share(&a)
    );
    // And it is still the same note: a saturator that shifted the pitch would
    // be a bug the ear finds before any of this does.
    let (before, after) = (settled_cents(&a), settled_cents(&b));
    assert!(
        (after - before).abs() < 25.0,
        "drive moved the note by {:.0} cents",
        after - before
    );
}

/// Drive is **off** in a fresh corrector, so rule 2 still holds: a fresh Tune
/// corrects and colours nothing.
#[test]
fn a_fresh_corrector_has_no_character_on_it() {
    let fresh = TuneConfig::new();
    assert_eq!(fresh.drive, 0.0);
    assert_eq!(fresh.crush, 0.0);
    assert_eq!(fresh.air, 0.0);
    assert_eq!(fresh.width, 1.0, "unity, not zero: a width of 0 is mono");
}

#[test]
fn crush_quantises_the_output_in_time() {
    let signal = steady(220.0, 1.0);
    let crushed = run(
        &TuneConfig {
            crush: 0.9,
            ..TuneConfig::new()
        },
        no_notes(),
        &signal,
    );
    // A sample-and-hold leaves runs of identical samples. A signal that has
    // been through one has far more of them than a smooth one does.
    let repeats = |s: &[f32]| s.windows(2).filter(|w| w[0] == w[1]).count();
    let plain = run(&TuneConfig::new(), no_notes(), &signal);
    assert!(
        repeats(&crushed) > repeats(&plain) * 4,
        "crush held no samples: {} against {}",
        repeats(&crushed),
        repeats(&plain)
    );
}

#[test]
fn air_tilts_the_top_both_ways() {
    let signal = steady(220.0, 1.0);
    let dull = run(
        &TuneConfig {
            air: -1.0,
            ..TuneConfig::new()
        },
        no_notes(),
        &signal,
    );
    let flat = run(&TuneConfig::new(), no_notes(), &signal);
    let bright = run(
        &TuneConfig {
            air: 1.0,
            ..TuneConfig::new()
        },
        no_notes(),
        &signal,
    );
    let (d, f, b) = (high_share(&dull), high_share(&flat), high_share(&bright));
    assert!(
        d < f && f < b,
        "air did not tilt: dull {d:.5}, flat {f:.5}, bright {b:.5}"
    );
}

#[test]
fn width_spreads_a_stereo_wet_and_leaves_a_mono_source_mono() {
    let signal = steady(220.0, 1.0);
    // Two channels that differ, so there is a side to widen.
    let other = steady(220.5, 1.0);
    let run_stereo = |config: &TuneConfig| -> (Vec<f32>, Vec<f32>) {
        let mut tune = Tune::new();
        tune.prepare(RATE, config);
        let (mut left, mut right) = (Vec::new(), Vec::new());
        let (mut a, mut b) = (vec![0.0f32; BLOCK], vec![0.0f32; BLOCK]);
        for (l, r) in signal.chunks(BLOCK).zip(other.chunks(BLOCK)) {
            let frames = l.len().min(r.len());
            a[..frames].copy_from_slice(&l[..frames]);
            b[..frames].copy_from_slice(&r[..frames]);
            {
                let (ha, _) = a.split_at_mut(frames);
                let (hb, _) = b.split_at_mut(frames);
                let mut channels: [&mut [f32]; 2] = [ha, hb];
                tune.process(&mut channels, no_notes(), config, BPM);
            }
            left.extend_from_slice(&a[..frames]);
            right.extend_from_slice(&b[..frames]);
        }
        (left, right)
    };
    let side = |(l, r): &(Vec<f32>, Vec<f32>)| {
        let s: Vec<f32> = l.iter().zip(r).map(|(a, b)| (a - b) * 0.5).collect();
        rms(&s)
    };
    let narrow = run_stereo(&TuneConfig {
        width: 0.0,
        ..TuneConfig::new()
    });
    let unity = run_stereo(&TuneConfig::new());
    let wide = run_stereo(&TuneConfig {
        width: 2.0,
        ..TuneConfig::new()
    });
    assert!(
        side(&narrow) < side(&unity) * 0.2,
        "width 0 left a side signal: {:.6}",
        side(&narrow)
    );
    assert!(
        side(&wide) > side(&unity) * 1.5,
        "width 2 did not widen: {:.6} against {:.6}",
        side(&wide),
        side(&unity)
    );
}

/// **No preset in the bank clips**, on a vocal at a level people actually
/// record at.
///
/// `Gender Down` already carried an `output_db` trim because moving the whole
/// envelope down a third costs three or four decibels; §4.8's knobs make the
/// opposite mistake possible, and did — `drive` makes its own gain back,
/// `air` at the top of its travel is a 9 dB shelf and `width` above unity
/// adds side energy, so a preset that reached for all three arrived over
/// full scale. A factory preset that distorts on a normal take is one people
/// blame the plug-in for, so this is a gate on the bank rather than advice in
/// a comment.
#[test]
fn no_factory_preset_clips_a_normal_vocal() {
    // −6 dBFS peak: hotter than most vocal stems and quiet enough that a
    // preset failing here would fail on a real one too.
    let raw = vowel_at(
        &|t| hz_to_cents(220.0) + 40.0 * (std::f32::consts::TAU * 1.2 * t).sin(),
        1.5,
    );
    let peak = raw.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    let signal: Vec<f32> = raw.iter().map(|s| s / peak * 0.5).collect();

    let mut over = Vec::new();
    for preset in TunePreset::ALL {
        let config = TuneConfig::from_preset(preset);
        let out = run(&config, no_notes(), &signal);
        let loudest = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        if loudest > 1.0 {
            over.push((preset.label(), loudest));
        }
    }
    assert!(
        over.is_empty(),
        "these presets clip a −6 dBFS vocal: {}",
        over.iter()
            .map(|(name, peak)| format!("{name} at {peak:.2}"))
            .collect::<Vec<_>>()
            .join(", ")
    );
}
