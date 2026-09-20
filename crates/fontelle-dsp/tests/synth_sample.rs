//! Flopsynth's **sample** source: a recording played as it is, pitched across
//! the keyboard, through the same oscillator that reads a table.
//!
//! > *"i think those have features for dragging an audio file in and using
//! > the waveform of that as your oscilator ... we could actually sample a
//! > real piano sound and then do effects and modulating and layering with
//! > other oscilators and stuff."* — Ty, 2026-09-15
//!
//! A table is one cycle; a sample is the whole sound. What these tests hold
//! is that the oscillator treats it as a *sound* — it plays at the pitch it
//! was recorded at when asked for that pitch, twice as fast an octave up, it
//! ends when the recording ends unless it is told to loop, and the start knob
//! starts it later — and that the stack's unison, which is the reason it is
//! an oscillator rather than a sampler, still works on it.

use fontelle_dsp::{
    Interpolation, SampleData, SampleLoop, SynthInput, SynthOsc, SynthSource, SynthState, Unison,
    UnisonMode, UnisonSpread,
};

const SR: f32 = 48_000.0;

/// A recording: a sine at `hz`, `seconds` long, at `rate`.
fn recording(hz: f32, seconds: f32, rate: f32) -> Vec<f32> {
    let frames = (seconds * rate) as usize;
    (0..frames)
        .map(|i| (std::f32::consts::TAU * hz * i as f32 / rate).sin() * 0.8)
        .collect()
}

fn osc() -> SynthOsc {
    SynthOsc {
        source: SynthSource::Sample(0),
        ..SynthOsc::default()
    }
}

/// `frames` of the left channel, the oscillator reading `data` for a note at
/// `note_hz`.
fn render(config: &SynthOsc, data: &SampleData<'_>, note_hz: f32, frames: usize) -> Vec<f32> {
    let mut state = SynthState::new();
    state.reset(config, 1);
    (0..frames)
        .map(|_| {
            state
                .next_sample_from(config, SynthInput::Sample(*data), note_hz, SR, 0.0)
                .0
        })
        .collect()
}

fn zero_crossings_per_second(samples: &[f32]) -> f32 {
    let crossings = samples
        .windows(2)
        .filter(|w| w[0] <= 0.0 && w[1] > 0.0)
        .count();
    crossings as f32 / (samples.len() as f32 / SR)
}

fn rms(samples: &[f32]) -> f32 {
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len().max(1) as f32).sqrt()
}

#[test]
fn a_sample_asked_for_its_own_pitch_plays_as_recorded() {
    let sound = recording(440.0, 1.0, SR);
    let data = SampleData {
        samples: &sound,
        sample_rate: SR,
        root_hz: 440.0,
        interpolation: Interpolation::Normal,
        gain: 1.0,
        loop_frames: None,
    };
    let out = render(&osc(), &data, 440.0, 24_000);
    let measured = zero_crossings_per_second(&out);
    assert!(
        (measured - 440.0).abs() < 4.0,
        "recorded at 440, asked for 440, measured {measured} Hz"
    );
    // And sample for sample it *is* the recording — no filter, no resampling
    // at a ratio of one. Under the stack's own centre pan, which is −3 dB a
    // side for every source this oscillator has.
    let centre = std::f32::consts::FRAC_1_SQRT_2;
    for (i, (a, b)) in out.iter().zip(&sound).enumerate().take(4_000) {
        assert!(
            (a - b * centre).abs() < 1e-3,
            "at {i} the oscillator played {a} where the recording has {b}"
        );
    }
}

#[test]
fn an_octave_up_plays_twice_as_fast() {
    let sound = recording(440.0, 1.0, SR);
    let data = SampleData {
        samples: &sound,
        sample_rate: SR,
        root_hz: 440.0,
        interpolation: Interpolation::Normal,
        gain: 1.0,
        loop_frames: None,
    };
    let out = render(&osc(), &data, 880.0, 12_000);
    let measured = zero_crossings_per_second(&out);
    assert!(
        (measured - 880.0).abs() < 8.0,
        "an octave over the root should be 880 Hz, measured {measured}"
    );
}

/// A recording at a different rate from the session's plays at the pitch it
/// was recorded at, not at the pitch its sample count suggests.
#[test]
fn a_recording_at_another_rate_is_pitched_by_its_own_rate() {
    let sound = recording(220.0, 1.0, 24_000.0);
    let data = SampleData {
        samples: &sound,
        sample_rate: 24_000.0,
        root_hz: 220.0,
        interpolation: Interpolation::Normal,
        gain: 1.0,
        loop_frames: None,
    };
    let out = render(&osc(), &data, 220.0, 24_000);
    let measured = zero_crossings_per_second(&out);
    assert!(
        (measured - 220.0).abs() < 3.0,
        "a 24 kHz recording of 220 Hz played at 48 kHz should still be 220 Hz, \
         measured {measured}"
    );
}

#[test]
fn a_one_shot_ends_when_the_recording_does() {
    let sound = recording(440.0, 0.25, SR);
    let data = SampleData {
        samples: &sound,
        sample_rate: SR,
        root_hz: 440.0,
        interpolation: Interpolation::Normal,
        gain: 1.0,
        loop_frames: None,
    };
    let out = render(&osc(), &data, 440.0, 24_000);
    let during = rms(&out[..10_000]);
    let after = rms(&out[13_000..]);
    assert!(during > 0.3, "the recording plays while it lasts: {during}");
    assert!(
        after < 1e-6,
        "past the end of a one-shot there is nothing: {after}"
    );
}

#[test]
fn a_looped_sample_keeps_going() {
    let sound = recording(440.0, 0.25, SR);
    let data = SampleData {
        samples: &sound,
        sample_rate: SR,
        root_hz: 440.0,
        interpolation: Interpolation::Normal,
        gain: 1.0,
        loop_frames: None,
    };
    let mut looped = osc();
    looped.sample.loop_mode = SampleLoop::Forward;
    looped.sample.loop_start = 0.2;
    looped.sample.loop_end = 0.8;
    let out = render(&looped, &data, 440.0, 48_000);
    let late = rms(&out[40_000..]);
    assert!(
        late > 0.3,
        "a looped recording is still sounding a second in: {late}"
    );
    // And it is still the note: a loop that lands on the wrong sample would
    // put a click in every cycle, which reads as energy off the pitch.
    let measured = zero_crossings_per_second(&out[24_000..]);
    assert!(
        (measured - 440.0).abs() < 6.0,
        "the loop should still be 440 Hz, measured {measured}"
    );
}

/// The start knob: the oscillator's `position`, which on a table is the frame
/// and on a sample is **where in the recording** the note starts.
#[test]
fn the_position_knob_is_where_the_note_starts() {
    // Half a second of silence, then the tone.
    let mut sound = vec![0.0f32; 24_000];
    sound.extend(recording(440.0, 0.5, SR));
    let data = SampleData {
        samples: &sound,
        sample_rate: SR,
        root_hz: 440.0,
        interpolation: Interpolation::Normal,
        gain: 1.0,
        loop_frames: None,
    };
    let from_the_top = render(&osc(), &data, 440.0, 4_000);
    assert!(
        rms(&from_the_top) < 1e-6,
        "from the top the first eighty milliseconds are the silence"
    );
    let mut halfway = osc();
    halfway.position = 0.5;
    let out = render(&halfway, &data, 440.0, 4_000);
    assert!(
        rms(&out) > 0.3,
        "started halfway, the note is the tone at once: {}",
        rms(&out)
    );
}

#[test]
fn one_unison_voice_is_the_plain_read() {
    let sound = recording(330.0, 0.5, SR);
    let data = SampleData {
        samples: &sound,
        sample_rate: SR,
        root_hz: 330.0,
        interpolation: Interpolation::Normal,
        gain: 1.0,
        loop_frames: None,
    };
    let plain = osc();
    let mut one = plain;
    one.unison = Unison {
        voices: 1,
        detune_cents: 50.0,
        blend: 0.0,
        width: 1.0,
        mode: UnisonMode::Classic,
        spread: UnisonSpread::Power,
    };
    let a = render(&plain, &data, 330.0, 8_000);
    let b = render(&one, &data, 330.0, 8_000);
    for (i, (x, y)) in a.iter().zip(&b).enumerate() {
        assert!((x - y).abs() < 1e-6, "differ at {i}: {x} vs {y}");
    }
}

/// Three voices a few cents apart **beat**: the envelope of the sum rises and
/// falls where one voice's would be flat. This is the whole reason a sample
/// is an oscillator rather than a sampler — a piano's three strings do
/// exactly this.
#[test]
fn a_detuned_stack_beats() {
    let sound = recording(220.0, 2.0, SR);
    let data = SampleData {
        samples: &sound,
        sample_rate: SR,
        root_hz: 220.0,
        interpolation: Interpolation::Normal,
        gain: 1.0,
        loop_frames: None,
    };
    let mut stack = osc();
    stack.unison = Unison {
        voices: 3,
        detune_cents: 8.0,
        blend: 1.0,
        width: 0.0,
        mode: UnisonMode::Classic,
        spread: UnisonSpread::Power,
    };
    let out = render(&stack, &data, 220.0, 96_000);
    // RMS over 50 ms windows: a beat at ~1 Hz swings it.
    let windows: Vec<f32> = out.chunks(2_400).map(rms).collect();
    let loudest = windows.iter().cloned().fold(0.0f32, f32::max);
    let quietest = windows.iter().cloned().fold(f32::MAX, f32::min);
    assert!(
        quietest < loudest * 0.6,
        "three detuned copies should beat: loudest {loudest}, quietest {quietest}"
    );
}

/// A sample past its own end reads nothing, and a sample with no frames is
/// silence rather than a panic.
#[test]
fn an_empty_recording_is_silence() {
    let data = SampleData {
        samples: &[],
        sample_rate: SR,
        root_hz: 440.0,
        interpolation: Interpolation::Normal,
        gain: 1.0,
        loop_frames: None,
    };
    let out = render(&osc(), &data, 440.0, 1_000);
    assert!(out.iter().all(|s| *s == 0.0));
}

/// Off-grid energy over on-grid energy, in dB, between 30 Hz and 20 kHz —
/// the measure `tests/synth_alias.rs` uses.
fn alias_db(samples: &[f32], f0: f32) -> f32 {
    let n = samples.len();
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
    fontelle_dsp::fft_in_place(&mut re, &mut im);
    let bin_hz = SR / n as f32;
    let f0_bins = f0 / bin_hz;
    let top = (20_000.0 / bin_hz) as usize;
    let (mut signal, mut alias) = (0.0f64, 0.0f64);
    for k in 8..top {
        let power = f64::from(re[k] * re[k] + im[k] * im[k]);
        let nearest = (k as f32 / f0_bins).round() * f0_bins;
        if (k as f32 - nearest).abs() <= 4.0 {
            signal += power;
        } else {
            alias += power;
        }
    }
    10.0 * (alias / signal.max(1e-30)).log10() as f32
}

/// The recording read `interpolation`-wise, +19 semitones up from a sine at
/// its root: every read between two frames is an estimate, and the better
/// kernel's estimate is the cleaner one (`docs/flopsynth-next.md` §4.1,
/// "sample interpolation follows quality").
fn transposed_alias(interpolation: Interpolation) -> f32 {
    const FRAMES: usize = 16_384;
    // A high root — ten frames to the cycle — so the read between frames
    // is an estimate worth the name: at 440 Hz every kernel reads a sine
    // within 78 dB and there is nothing to tell them apart by. Nineteen
    // semitones over it puts the note at 15 kHz, on the transform's grid.
    let ratio = 2f32.powf(19.0 / 12.0);
    let note = ((5_000.0 * ratio) * FRAMES as f32 / SR).round() * SR / FRAMES as f32;
    let root = note / ratio;
    let sound = recording(root, 2.0, SR);
    let data = SampleData {
        samples: &sound,
        sample_rate: SR,
        root_hz: root,
        interpolation,
        gain: 1.0,
        loop_frames: None,
    };
    alias_db(&render(&osc(), &data, note, FRAMES), note)
}

#[test]
fn the_read_follows_the_interpolation_it_is_handed() {
    let draft = transposed_alias(Interpolation::Draft);
    let normal = transposed_alias(Interpolation::Normal);
    let high = transposed_alias(Interpolation::High);
    assert!(
        high <= normal - 12.0,
        "High reads {high:.1} dB of alias against Normal's {normal:.1}: the kernel is not followed"
    );
    assert!(
        normal <= draft - 6.0,
        "Normal {normal:.1} against Draft {draft:.1}"
    );
}

/// `Ultra` is the top of the same chooser (`patch/quality`), and until now a
/// read at it was a `todo!()` — a panic on the audio thread, reachable from
/// a menu. It reads at least as well as `High`.
#[test]
fn ultra_reads_at_least_as_cleanly_as_high_and_does_not_panic() {
    let high = transposed_alias(Interpolation::High);
    let ultra = transposed_alias(Interpolation::Ultra);
    assert!(
        ultra <= high + 0.5,
        "Ultra {ultra:.1} against High {high:.1}"
    );
}
