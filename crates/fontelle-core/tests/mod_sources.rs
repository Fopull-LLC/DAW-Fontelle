//! The four sources §4.2 of `docs/flopsynth-next.md` adds: `Chaos` (a Lorenz
//! attractor per voice), `RandomWalk` (smoothed noise per voice),
//! `EnvelopeFollower` (the voice's own level) and `StepSeq` (two sixteen-step
//! sequencers — the *modulation* half of what an arp does, and no time
//! addressing outside the patch: the value at a step is state, not events).
//! Each is a `ModSource` appended at the end, so no saved patch changes
//! meaning, and each is absent from the file until its knobs move.

use fontelle_core::flopsynth::{LayerRole, flopsynth_init, sources};
use fontelle_core::{
    Curve, ModDest, ModRoute, ModSource, NoteTrigger, Patch, PrepareContext, RenderClock,
    SampleStore, Sampler, Source, patch_params,
};
use fontelle_dsp::{SynthSource, WavetableId};

const SR: f32 = 48_000.0;
const BLOCK: usize = 128;

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

/// The Init with only A up, a sine, no filter, an instant amp envelope.
fn a_sine() -> Patch {
    let mut patch = flopsynth_init();
    for (index, layer) in patch.layers.iter_mut().enumerate() {
        let Source::Synth(osc) = &mut layer.source else {
            continue;
        };
        if index == LayerRole::OscA as usize {
            osc.source = SynthSource::Table(WavetableId::SubSine);
            osc.unison.voices = 1;
            layer.gain_db = -6.0;
        } else {
            layer.gain_db = -120.0;
        }
    }
    for slot in &mut patch.filters {
        slot.enabled = false;
    }
    patch.envelopes[0].attack_s = 0.0;
    patch.envelopes[0].decay_s = 0.0;
    patch.envelopes[0].sustain_level = 1.0;
    patch.mod_matrix.routes.clear();
    patch
}

/// `key` at velocity 100 for `seconds`, in the engine's blocks with the
/// clock moving at 120, the left channel.
fn render(patch: Patch, key: u8, seconds: f32) -> Vec<f32> {
    let store = SampleStore::new();
    let mut sampler = Sampler::new(patch);
    sampler.prepare(&PrepareContext {
        sample_rate: SR,
        max_block_size: BLOCK as u32,
    });
    sampler.trigger(NoteTrigger::new(key, 100));
    let frames = (seconds * SR) as usize;
    let mut out = Vec::with_capacity(frames);
    while out.len() < frames {
        sampler.set_clock(RenderClock {
            bpm: 120.0,
            position_sample: out.len() as u64,
        });
        let mut l = [0.0f32; BLOCK];
        let mut r = [0.0f32; BLOCK];
        sampler.render(&store, &mut [&mut l, &mut r]);
        out.extend_from_slice(&l);
    }
    out.truncate(frames);
    out
}

/// As [`render`], for the **second** voice a sampler starts: a first note
/// is played and cut, then `key`.
fn render_second(patch: Patch, key: u8, seconds: f32) -> Vec<f32> {
    let store = SampleStore::new();
    let mut sampler = Sampler::new(patch);
    sampler.prepare(&PrepareContext {
        sample_rate: SR,
        max_block_size: BLOCK as u32,
    });
    sampler.trigger(NoteTrigger::new(key, 100));
    sampler.reset();
    sampler.trigger(NoteTrigger::new(key, 100));
    let frames = (seconds * SR) as usize;
    let mut out = Vec::with_capacity(frames);
    while out.len() < frames {
        sampler.set_clock(RenderClock {
            bpm: 120.0,
            position_sample: out.len() as u64,
        });
        let mut l = [0.0f32; BLOCK];
        let mut r = [0.0f32; BLOCK];
        sampler.render(&store, &mut [&mut l, &mut r]);
        out.extend_from_slice(&l);
    }
    out.truncate(frames);
    out
}

/// The amplitude envelope, in decibels, one value per `ms` milliseconds.
fn envelope_db(out: &[f32], ms: f32) -> Vec<f32> {
    out.chunks((SR * ms * 0.001) as usize)
        .map(|c| {
            20.0 * c
                .iter()
                .fold(0.0f32, |m, s| m.max(s.abs()))
                .max(1e-6)
                .log10()
        })
        .collect()
}

/// How many times the envelope turns round (a local maximum or minimum).
fn turns(env: &[f32]) -> usize {
    env.windows(3)
        .filter(|w| (w[1] > w[0] && w[1] > w[2]) || (w[1] < w[0] && w[1] < w[2]))
        .count()
}

#[test]
fn the_four_are_sources_with_names_and_moving_within_a_block() {
    let patch = flopsynth_init();
    let names: Vec<(ModSource, String)> = sources(&patch);
    for (source, label) in [
        (ModSource::Chaos, "Chaos"),
        (ModSource::RandomWalk, "Walk"),
        (ModSource::EnvelopeFollower, "Follow"),
        (ModSource::StepSeq(0), "SEQ 1"),
        (ModSource::StepSeq(1), "SEQ 2"),
    ] {
        assert!(
            names.iter().any(|(s, l)| *s == source && l == label),
            "{label} is missing from {names:?}"
        );
        assert!(source.moves_within_a_block(), "{label} turns every step");
    }
    // Every one is bipolar but the follower, which is a level.
    assert!(!ModSource::EnvelopeFollower.is_bipolar());
    assert!(ModSource::Chaos.is_bipolar() && ModSource::RandomWalk.is_bipolar());
    assert!(ModSource::StepSeq(0).is_bipolar());
}

#[test]
fn their_knobs_have_addresses_and_stay_out_of_the_file_at_rest() {
    let mut patch = flopsynth_init();
    let addresses = fontelle_core::flopsynth::addresses(&patch);
    for address in [
        "patch/chaos/rate",
        "patch/walk/rate",
        "patch/walk/smooth",
        "patch/seq[0]/length",
        "patch/seq[0]/rate",
        "patch/seq[0]/sync",
        "patch/seq[0]/division",
        "patch/seq[0]/smooth",
        "patch/seq[0]/step[0]",
        "patch/seq[0]/step[15]",
        "patch/seq[1]/step[7]",
    ] {
        assert!(addresses.iter().any(|a| a == address), "no {address}");
        assert!(
            patch_params::value(&patch, address).is_some(),
            "{address} reads nothing"
        );
    }
    assert!(!addresses.iter().any(|a| a == "patch/seq[0]/step[16]"));
    assert!(!addresses.iter().any(|a| a == "patch/seq[2]/rate"));

    // At rest nothing is written.
    let data = patch.to_data(&Default::default()).unwrap();
    let text = data.body.to_string();
    for key in ["chaos", "walk", "sequencers"] {
        assert!(
            !text.contains(&format!("\"{key}\"")),
            "{key} written at rest"
        );
    }
    // A step moved is written and read back; the rest stay at rest.
    assert!(patch_params::set(&mut patch, "patch/seq[1]/step[3]", 0.9));
    assert!(patch_params::set(&mut patch, "patch/chaos/rate", 0.7));
    let data = patch.to_data(&Default::default()).unwrap();
    let text = data.body.to_string();
    assert!(text.contains("\"sequencers\"") && text.contains("\"chaos\""));
    assert!(!text.contains("\"walk\""));
    let back = Patch::from_data(&data, |_| None).unwrap().patch;
    assert_eq!(back, patch);
    // A step is bipolar, and the length is one to sixteen.
    assert!((back.sequencers[1].steps[3] - 0.8).abs() < 1e-6);
    assert!(patch_params::set(&mut patch, "patch/seq[0]/length", 0.0));
    assert_eq!(patch.sequencers[0].length, 1);
    assert!(patch_params::set(&mut patch, "patch/seq[0]/length", 1.0));
    assert_eq!(patch.sequencers[0].length, 16);
}

/// Chaos on the amp: the level wanders, never past the depth, turning
/// more often at a faster rate — and two notes wander differently.
#[test]
fn chaos_wanders_within_its_depth_and_faster_at_a_higher_rate() {
    let mut patch = a_sine();
    patch.chaos.rate_hz = 1.0;
    // ±12 dB.
    patch
        .mod_matrix
        .routes
        .push(route(ModSource::Chaos, ModDest::Amp, 0.5));
    let slow = envelope_db(&render(patch.clone(), 60, 4.0), 10.0);
    let base = envelope_db(&render(a_sine(), 60, 0.1), 10.0)[5];
    let (low, high) = slow
        .iter()
        .skip(5)
        .fold((f32::MAX, f32::MIN), |(l, h), v| (l.min(*v), h.max(*v)));
    assert!(
        high - low > 6.0,
        "it wanders: {low:.1}..{high:.1} dB over four seconds"
    );
    assert!(
        low > base - 12.5 && high < base + 12.5,
        "and stays inside ±12 dB of {base:.1}: {low:.1}..{high:.1}"
    );
    patch.chaos.rate_hz = 8.0;
    let fast = envelope_db(&render(patch.clone(), 60, 4.0), 10.0);
    assert!(
        turns(&fast) > turns(&slow) * 3,
        "eight times the rate turns more than three times as often: {} against {}",
        turns(&fast),
        turns(&slow)
    );
    // Another note is another orbit: the second voice a sampler starts
    // is seeded differently from its first (the same note played again
    // after a reset is the same orbit, which is what makes this testable).
    let other = envelope_db(&render_second(patch, 60, 4.0), 10.0);
    let same = fast
        .iter()
        .zip(&other)
        .filter(|(a, b)| (*a - *b).abs() < 0.5)
        .count();
    assert!(
        same < fast.len() / 2,
        "two notes follow the same path for {same} of {} frames",
        fast.len()
    );
}

/// The walk on the amp: it moves, it stays inside its depth, and smoothing
/// takes the corners off — the biggest change per ten milliseconds is
/// smaller smoothed than stepped.
#[test]
fn the_random_walk_moves_smoothly_within_its_depth() {
    let mut patch = a_sine();
    patch.walk.rate_hz = 6.0;
    patch.walk.smooth = 0.0;
    patch
        .mod_matrix
        .routes
        .push(route(ModSource::RandomWalk, ModDest::Amp, 0.5));
    let stepped = envelope_db(&render(patch.clone(), 60, 3.0), 10.0);
    let base = envelope_db(&render(a_sine(), 60, 0.1), 10.0)[5];
    let (low, high) = stepped
        .iter()
        .skip(5)
        .fold((f32::MAX, f32::MIN), |(l, h), v| (l.min(*v), h.max(*v)));
    assert!(high - low > 6.0, "it moves: {low:.1}..{high:.1} dB");
    assert!(low > base - 12.5 && high < base + 12.5, "inside ±12 dB");
    let biggest = |env: &[f32]| {
        env.windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0f32, f32::max)
    };
    patch.walk.smooth = 1.0;
    let smoothed = envelope_db(&render(patch, 60, 3.0), 10.0);
    assert!(
        biggest(&smoothed) < biggest(&stepped) * 0.5,
        "smoothed steps of {:.1} dB against stepped {:.1} dB",
        biggest(&smoothed),
        biggest(&stepped)
    );
}

/// The follower is the voice's own level: on a note that swells in over
/// half a second, a route from it to pitch bends the note up as it grows.
#[test]
fn the_envelope_follower_is_the_voices_own_level() {
    let mut patch = a_sine();
    patch.envelopes[0].attack_s = 0.5;
    // Up to 960 cents at a level of 1.0; the note peaks around 0.3.
    patch.mod_matrix.routes.push(route(
        ModSource::EnvelopeFollower,
        ModDest::LayerPitch(LayerRole::OscA as u8),
        0.1,
    ));
    let out = render(patch, 60, 0.6);
    // The pitch, from zero crossings, over 200 ms windows (five hertz
    // apart, a third of a semitone at C4).
    let pitch = |from_s: f32| {
        let from = (from_s * SR) as usize;
        let window = &out[from..from + (0.2 * SR) as usize];
        let crossings = window
            .windows(2)
            .filter(|w| w[0] <= 0.0 && w[1] > 0.0)
            .count();
        crossings as f32 / 0.2
    };
    let early = pitch(0.0);
    let late = pitch(0.4);
    let cents = 1200.0 * (late / early).log2();
    assert!(
        cents > 60.0,
        "the pitch rises with the level: {early:.0} Hz -> {late:.0} Hz ({cents:+.0} cents)"
    );
}

/// A sequencer on the amp: four steps at eight a second, loud on the odd
/// ones — four loud pulses a second; the same synced to sixteenths at 120.
#[test]
fn a_step_sequencer_steps_at_its_rate_and_at_its_division() {
    let mut patch = a_sine();
    let seq = &mut patch.sequencers[0];
    seq.length = 4;
    seq.steps[0] = 1.0;
    seq.steps[1] = -1.0;
    seq.steps[2] = 1.0;
    seq.steps[3] = -1.0;
    seq.steps[4] = 1.0; // past the length: never reached
    seq.rate_hz = 8.0;
    seq.sync = false;
    patch
        .mod_matrix
        .routes
        .push(route(ModSource::StepSeq(0), ModDest::Amp, 1.0));
    let pulses = |patch: Patch| {
        let env = envelope_db(&render(patch, 60, 2.0), 1.0);
        let top = env.iter().fold(f32::MIN, |m, v| m.max(*v));
        let loud: Vec<bool> = env.iter().map(|v| *v > top - 6.0).collect();
        let share = loud.iter().filter(|l| **l).count() as f32 / loud.len() as f32;
        (loud.windows(2).filter(|w| w[0] && !w[1]).count(), share)
    };
    let (edges, share) = pulses(patch.clone());
    assert_eq!(edges, 8, "eight loud steps in two seconds at 8 Hz");
    assert!(
        (0.4..=0.6).contains(&share),
        "loud half the time: {share:.2}"
    );

    // Synced: a sixteenth at 120 is an eighth of a second — the same.
    patch.sequencers[0].sync = true;
    patch.sequencers[0].division = fontelle_types::NoteDivision::Sixteenth;
    patch.sequencers[0].rate_hz = 1.0;
    let (edges, _) = pulses(patch.clone());
    assert_eq!(edges, 8, "eight loud steps in two seconds at sixteenths");

    // The second sequencer is its own: eight steps at 4 Hz, one loud.
    let mut patch = a_sine();
    let seq = &mut patch.sequencers[1];
    seq.length = 8;
    seq.steps[0] = 1.0;
    for step in &mut seq.steps[1..8] {
        *step = -1.0;
    }
    seq.rate_hz = 4.0;
    seq.sync = false;
    patch
        .mod_matrix
        .routes
        .push(route(ModSource::StepSeq(1), ModDest::Amp, 1.0));
    let (edges, share) = pulses(patch);
    assert_eq!(edges, 1, "one loud step per two-second cycle");
    assert!(share < 0.2, "loud an eighth of the time: {share:.2}");
}
