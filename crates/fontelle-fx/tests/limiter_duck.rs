//! The limiter's sidechain, which is what makes it a ducker: when a key is
//! given, the gain is computed from *that* signal, so a loud key pushes the
//! main signal down under the ceiling. Feed the kick in and the bass gets out
//! of its way.

use fontelle_fx::{Limiter, LimiterConfig};

const SR: f32 = 48_000.0;

/// Runs `main` through the limiter keyed from `key`, and hands the output back.
fn duck(main: &[f32], key: &[f32], config: &LimiterConfig) -> Vec<f32> {
    let mut limiter = Limiter::new();
    limiter.prepare(SR, config);
    let mut buffer = main.to_vec();
    let mut slice = [buffer.as_mut_slice()];
    limiter.process(&mut slice, Some(key), config);
    buffer
}

#[test]
fn a_loud_key_ducks_the_main_and_silence_lets_it_through() {
    // A ceiling of 0.5: a key at 1.0 wants the main at half, a key at 0.0
    // leaves it alone.
    let config = LimiterConfig {
        ceiling: 0.5,
        lookahead_ms: 1.0,
        release_ms: 20.0,
    };
    let frames = SR as usize / 2; // half a second, plenty for the release
    let main = vec![0.8f32; frames];
    // Silent for the first quarter-second, then a full-scale key.
    let mut key = vec![0.0f32; frames];
    for k in key.iter_mut().skip(frames / 2) {
        *k = 1.0;
    }

    let out = duck(&main, &key, &config);

    // Late in the quiet stretch: nothing over the ceiling, so the main is
    // essentially untouched.
    let quiet = out[frames / 2 - 100];
    assert!(
        quiet > 0.75,
        "with a silent key the main should pass through: {quiet}"
    );

    // Late in the loud stretch: the key is at 1.0 and the ceiling is 0.5, so
    // the main is pulled to about half.
    let ducked = out[frames - 100];
    assert!(
        (ducked - 0.4).abs() < 0.05,
        "a full-scale key over a 0.5 ceiling should halve a 0.8 main: {ducked}"
    );
}

#[test]
fn with_no_key_the_limiter_still_limits_its_own_signal() {
    // The master-bus behaviour is unchanged: given no key, it limits what runs
    // through it, exactly as before the sidechain existed.
    let config = LimiterConfig {
        ceiling: 0.5,
        lookahead_ms: 1.0,
        release_ms: 20.0,
    };
    let frames = SR as usize / 4;
    let mut buffer = vec![0.9f32; frames];
    let mut limiter = Limiter::new();
    limiter.prepare(SR, &config);
    {
        let mut slice = [buffer.as_mut_slice()];
        limiter.process(&mut slice, None, &config);
    }
    // Nothing over the ceiling once it has settled.
    let settled = buffer[frames - 100];
    assert!(
        settled <= 0.5 + 1e-3,
        "an unkeyed limiter still holds its own signal to the ceiling: {settled}"
    );
}
