//! An envelope's loop (`docs/flopsynth-next.md` §3.4): two stages, and the
//! envelope runs from the end of the second back to the start of the first
//! while the note is held. The field lands with the editor; the voice reads
//! it in Phase 3. Absent from the file unless set, so every envelope ever
//! written reads as it did.

use fontelle_dsp::{EnvStage, EnvelopeConfig};

#[test]
fn a_loop_names_two_stages_and_is_absent_from_the_file_unless_set() {
    let plain = EnvelopeConfig::default();
    assert_eq!(plain.loop_stages, None);
    let text = serde_json::to_string(&plain).unwrap();
    assert!(!text.contains("loop"), "{text}");
    let looped = EnvelopeConfig {
        loop_stages: Some((EnvStage::Attack, EnvStage::Decay)),
        ..EnvelopeConfig::default()
    };
    let text = serde_json::to_string(&looped).unwrap();
    assert!(text.contains("loop_stages"), "{text}");
    let back: EnvelopeConfig = serde_json::from_str(&text).unwrap();
    assert_eq!(back.loop_stages, Some((EnvStage::Attack, EnvStage::Decay)));
    // The stages in their order, so a loop's two ends can be compared.
    assert!(EnvStage::Attack < EnvStage::Decay && EnvStage::Decay < EnvStage::Release);
    assert_eq!(EnvStage::ALL.len(), 6);
}

use fontelle_dsp::EnvelopeGenerator;

const SR: f32 = 48_000.0;

/// `seconds` of the generator's level, `note_off` after `held` seconds
/// (or never), one value per sample.
fn run(config: &EnvelopeConfig, held: Option<f32>, seconds: f32) -> Vec<f32> {
    let mut env = EnvelopeGenerator::new();
    env.note_on();
    let total = (seconds * SR) as usize;
    let off_at = held.map(|s| (s * SR) as usize).unwrap_or(usize::MAX);
    (0..total)
        .map(|i| {
            if i == off_at {
                env.note_off();
            }
            env.advance(config, SR)
        })
        .collect()
}

/// Where the level peaks (local maxima above 0.9), in seconds.
fn peaks(levels: &[f32]) -> Vec<f32> {
    levels
        .windows(3)
        .enumerate()
        .filter(|(_, w)| w[1] > 0.9 && w[1] >= w[0] && w[1] > w[2])
        .map(|(i, _)| (i + 1) as f32 / SR)
        .collect()
}

/// A looping attack–decay while the key is held is a periodic gain: 50 ms
/// up and 50 ms down to nothing, over and over, at 10 Hz — and when the
/// key is let go the loop ends and the release runs.
#[test]
fn a_looping_decay_reads_as_a_periodic_gain() {
    let config = EnvelopeConfig {
        attack_s: 0.05,
        decay_s: 0.05,
        sustain_level: 0.0,
        release_s: 0.02,
        loop_stages: Some((EnvStage::Attack, EnvStage::Decay)),
        ..EnvelopeConfig::default()
    };
    let levels = run(&config, Some(0.55), 0.8);
    let tops = peaks(&levels[..(0.55 * SR) as usize]);
    assert!(
        tops.len() >= 5,
        "five peaks in half a second at 10 Hz, got {tops:?}"
    );
    for pair in tops.windows(2) {
        let period = pair[1] - pair[0];
        assert!(
            (period - 0.1).abs() < 0.004,
            "a 100 ms period, got {period:.4} s: {tops:?}"
        );
    }
    // Without the loop the same envelope is one swell and then nothing.
    let once = run(
        &EnvelopeConfig {
            loop_stages: None,
            ..config
        },
        Some(0.55),
        0.8,
    );
    assert_eq!(peaks(&once[..(0.55 * SR) as usize]).len(), 1);
    // Let go: the release runs from wherever the loop was, and it ends.
    let after = &levels[(0.6 * SR) as usize..];
    assert!(
        after.iter().all(|l| *l == 0.0),
        "silent 50 ms after the release started"
    );
    let mut env = EnvelopeGenerator::new();
    env.note_on();
    for _ in 0..(0.55 * SR) as usize {
        env.advance(&config, SR);
    }
    env.note_off();
    for _ in 0..(0.1 * SR) as usize {
        env.advance(&config, SR);
    }
    assert!(!env.is_active(), "the loop does not hold the note open");
}

/// A loop that returns from the end of the decay restarts the attack from
/// the level it is at, not from zero: the amp envelope loops too, and a
/// drop to nothing at every turn would be a click at 10 Hz.
#[test]
fn a_loop_returns_without_a_jump() {
    let config = EnvelopeConfig {
        attack_s: 0.05,
        decay_s: 0.05,
        sustain_level: 0.5,
        release_s: 0.02,
        loop_stages: Some((EnvStage::Attack, EnvStage::Decay)),
        ..EnvelopeConfig::default()
    };
    let levels = run(&config, None, 0.4);
    let biggest_step = levels
        .windows(2)
        .map(|w| (w[1] - w[0]).abs())
        .fold(0.0f32, f32::max);
    // The steepest slope is the attack's, one fiftieth of a millisecond.
    assert!(
        biggest_step < 1.0 / (0.05 * SR) * 1.5,
        "a step of {biggest_step} between two samples"
    );
    // And it still gets to the top every time.
    assert!(peaks(&levels).len() >= 3, "{:?}", peaks(&levels));
}

/// A loop whose end is the sustain: the plateau lasts as long as the decay
/// took to reach it, then the loop returns — a breathing swell rather than
/// a hold.
#[test]
fn a_loop_to_the_sustain_holds_the_plateau_for_the_decays_length() {
    let config = EnvelopeConfig {
        attack_s: 0.02,
        decay_s: 0.04,
        sustain_level: 0.3,
        release_s: 0.02,
        loop_stages: Some((EnvStage::Attack, EnvStage::Sustain)),
        ..EnvelopeConfig::default()
    };
    let levels = run(&config, None, 0.5);
    let tops = peaks(&levels);
    assert!(tops.len() >= 3, "{tops:?}");
    for pair in tops.windows(2) {
        let period = pair[1] - pair[0];
        // Attack 20 + decay 40 + plateau 40 = 100 ms.
        assert!(
            (period - 0.1).abs() < 0.004,
            "a 100 ms period, got {period:.4} s: {tops:?}"
        );
    }
    // The plateau is really held at the sustain level for a while.
    let held = levels
        .windows(2)
        .filter(|w| (w[0] - 0.3).abs() < 1e-3 && (w[1] - 0.3).abs() < 1e-3)
        .count();
    assert!(
        held as f32 / SR > 0.1,
        "at least a tenth of a second on the plateau in half a second, got {}",
        held as f32 / SR
    );
}

/// The loop is a fact about a held key: a released note runs out through
/// its release whether or not it was looping, and a loop set on an
/// envelope that has already been released changes nothing.
#[test]
fn a_released_note_does_not_loop() {
    let config = EnvelopeConfig {
        attack_s: 0.01,
        decay_s: 0.01,
        sustain_level: 0.0,
        release_s: 0.05,
        loop_stages: Some((EnvStage::Attack, EnvStage::Decay)),
        ..EnvelopeConfig::default()
    };
    let levels = run(&config, Some(0.005), 0.2);
    assert_eq!(
        peaks(&levels).len(),
        0,
        "released mid-attack: no peak, no loop, one release"
    );
    assert!(levels[(0.1 * SR) as usize..].iter().all(|l| *l == 0.0));
}
