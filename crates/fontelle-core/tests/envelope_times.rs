//! Envelope stage times as modulation destinations (TDD §7.5: *"every
//! envelope stage time/level"*; `ModDest::EnvelopeStageTime`).
//!
//! The destination has been declared since the matrix existed and is offered
//! in Flopsynth's own address table as "Env N decay time" — and the voice
//! never read it, so a route to it was a knob that moved nothing. Found
//! voicing the Grand Piano, whose one non-negotiable is that a bass string
//! rings for twenty seconds and a treble one for one: a decay that follows
//! the key is the difference between a piano and an organ with a slow fade.
//!
//! The unit is **octaves of time**: a route at full depth spans eight, the
//! same convention as cutoff and pitch, so "twice as long" means the same
//! thing on a ten-millisecond click as on a ten-second pad.

use fontelle_core::flopsynth::flopsynth_init;
use fontelle_core::{Curve, ModDest, ModRoute, ModSource};
use fontelle_core::{NoteTrigger, Patch, PrepareContext, SampleStore, Sampler};

const SR: f32 = 48_000.0;

/// The Init patch with a plain percussive amp envelope and one extra route.
fn patch_with(route: Option<ModRoute>) -> Patch {
    let mut patch = flopsynth_init();
    let amp = &mut patch.envelopes[0];
    amp.attack_s = 0.0;
    amp.decay_s = 0.5;
    amp.sustain_level = 0.0;
    amp.release_s = 0.05;
    patch.mod_matrix.routes.extend(route);
    patch
}

fn route(source: ModSource, destination: ModDest, depth: f32) -> ModRoute {
    ModRoute {
        source,
        destination,
        depth,
        curve: Curve::Linear,
        via: None,
        invert: false,
        bypass: false,
    }
}

/// Renders one note, released after `hold` seconds, for `seconds` in all.
fn render(patch: Patch, key: u8, velocity: u8, hold: f32, seconds: f32) -> Vec<f32> {
    let store = SampleStore::new();
    let mut sampler = Sampler::new(patch);
    sampler.prepare(&PrepareContext {
        sample_rate: SR,
        max_block_size: 512,
    });
    sampler.trigger(NoteTrigger::new(key, velocity));
    let total = (SR * seconds) as usize;
    let release_at = (SR * hold) as usize;
    let mut out = Vec::with_capacity(total);
    let mut done = 0usize;
    let mut released = false;
    while done < total {
        if !released && done >= release_at {
            sampler.release_all();
            released = true;
        }
        let frames = 512.min(total - done);
        let mut left = vec![0.0f32; frames];
        let mut right = vec![0.0f32; frames];
        sampler.render(&store, &mut [&mut left[..], &mut right[..]]);
        out.extend_from_slice(&left);
        done += frames;
    }
    out
}

fn rms(samples: &[f32]) -> f32 {
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len().max(1) as f32).sqrt()
}

/// Seconds from the loudest ten milliseconds to the first one 30 dB under it.
fn t30(samples: &[f32]) -> f32 {
    let envelope: Vec<f32> = samples.chunks(480).map(rms).collect();
    let (loudest_at, loudest) = envelope
        .iter()
        .copied()
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .expect("a rendered note has samples");
    envelope[loudest_at..]
        .iter()
        .position(|level| *level < loudest * 0.0316)
        .unwrap_or(envelope.len() - loudest_at) as f32
        * 0.01
}

#[test]
fn a_key_tracked_decay_makes_a_high_note_shorter_than_a_low_one() {
    // Without the route the two notes decay alike: the control, so that the
    // assertion below is about the route and not about the oscillator.
    let low = t30(&render(patch_with(None), 36, 100, 3.0, 3.0));
    let high = t30(&render(patch_with(None), 96, 100, 3.0, 3.0));
    assert!(
        (low - high).abs() < low * 0.25,
        "unrouted, the decays should match: low {low:.2} s, high {high:.2} s"
    );

    // Key → decay, downwards: five octaves of keyboard at full depth is 4.5
    // octaves of time, so the high note should ring for a small fraction of
    // the low one's length.
    let tracked = || {
        patch_with(Some(route(
            ModSource::Key,
            ModDest::EnvelopeStageTime(0, 3),
            -1.0,
        )))
    };
    let low = t30(&render(tracked(), 36, 100, 3.0, 3.0));
    let high = t30(&render(tracked(), 96, 100, 3.0, 3.0));
    assert!(
        high < low / 4.0,
        "a key-tracked decay should shorten the high note: low {low:.2} s, high {high:.2} s"
    );
}

#[test]
fn an_inverted_key_route_lengthens_the_bass_instead() {
    // The form a piano actually wants: the *short* time is the one stored,
    // because the knob tops out at ten seconds and a bass string rings past
    // that, so the route is what stretches the bottom of the keyboard.
    let tracked = || {
        let mut patch = patch_with(Some(ModRoute {
            invert: true,
            bypass: false,
            ..route(ModSource::Key, ModDest::EnvelopeStageTime(0, 3), 0.5)
        }));
        patch.envelopes[0].decay_s = 0.1;
        patch
    };
    let low = t30(&render(tracked(), 24, 100, 3.0, 3.0));
    let high = t30(&render(tracked(), 108, 100, 3.0, 3.0));
    assert!(
        low > high * 4.0,
        "inverted, the bass should be the long one: low {low:.2} s, high {high:.2} s"
    );
    assert!(
        high < 0.25,
        "the top of the keyboard keeps the stored time, give or take: {high:.2} s"
    );
}

#[test]
fn a_velocity_route_to_the_release_is_heard_after_the_note_off() {
    // Stage 5 is the release — the stage a note-off starts, which is the one
    // an SF2 `velocity → release` modulator wants and the one a piano's
    // damper time would go to.
    let tracked = || {
        let mut patch = patch_with(Some(route(
            ModSource::Velocity,
            ModDest::EnvelopeStageTime(0, 5),
            0.5,
        )));
        // A sustained note, so that everything after the note-off is release.
        patch.envelopes[0].sustain_level = 1.0;
        patch.envelopes[0].release_s = 0.05;
        patch
    };
    let after_off = |velocity: u8| {
        let out = render(tracked(), 60, velocity, 0.5, 3.0);
        t30(&out[(SR * 0.5) as usize..])
    };
    let soft = after_off(16);
    let hard = after_off(127);
    assert!(
        hard > soft * 3.0,
        "a hard note should release slower here: soft {soft:.3} s, hard {hard:.3} s"
    );
}
