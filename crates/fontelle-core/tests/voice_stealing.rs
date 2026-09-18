//! Which voice a new note takes when the pool is full (`StealPolicy`).
//!
//! `docs/flopsynth-next.md` §1.4(9): `Quietest` and `LowestPriority` were
//! one policy wearing three names — both fell through to `Oldest` — so a
//! patch that said "steal the quietest" stole the oldest and nothing told
//! anybody. Each policy is held here to the voice it says it takes: with
//! room for two and a third note arriving, the one that goes is the one
//! the policy names.

use fontelle_core::{Patch, PrepareContext, SampleStore, Sampler, StealPolicy};

const SR: f32 = 48_000.0;

/// Two voices, the amp envelope at full sustain with a long release so a
/// released voice is still loud when the third note lands.
fn a_two_voice_sampler(policy: StealPolicy) -> Sampler {
    let mut patch = Patch::basic_synth();
    patch.voice_config.polyphony = 2;
    patch.voice_config.steal_policy = policy;
    patch.envelopes[0].attack_s = 0.0;
    patch.envelopes[0].decay_s = 0.0;
    patch.envelopes[0].sustain_level = 1.0;
    patch.envelopes[0].release_s = 2.0;
    let mut sampler = Sampler::new(patch);
    sampler.prepare(&PrepareContext {
        sample_rate: SR,
        max_block_size: 512,
    });
    sampler
}

/// One block, so the envelopes have a level to compare.
fn run(sampler: &mut Sampler) {
    let store = SampleStore::new();
    let mut left = vec![0.0; 256];
    let mut right = vec![0.0; 256];
    sampler.render(&store, &mut [&mut left[..], &mut right[..]]);
}

fn keys(sampler: &Sampler) -> Vec<u8> {
    let mut keys: Vec<u8> = sampler.sounding_keys().collect();
    keys.sort_unstable();
    keys
}

#[test]
fn oldest_takes_the_voice_that_has_sounded_longest() {
    let mut sampler = a_two_voice_sampler(StealPolicy::Oldest);
    sampler.note_on(60, 20, 0);
    run(&mut sampler);
    sampler.note_on(64, 127, 1);
    run(&mut sampler);
    sampler.note_on(67, 100, 2);
    assert_eq!(keys(&sampler), [64, 67], "60 was the oldest, quiet or not");
}

#[test]
fn quietest_takes_the_voice_with_the_least_level() {
    let mut sampler = a_two_voice_sampler(StealPolicy::Quietest);
    // The old note is loud, the newer one is soft: quietest is not oldest.
    sampler.note_on(60, 127, 0);
    run(&mut sampler);
    sampler.note_on(64, 20, 1);
    run(&mut sampler);
    sampler.note_on(67, 100, 2);
    assert_eq!(
        keys(&sampler),
        [60, 67],
        "the soft 64 goes, though 60 is older"
    );
}

#[test]
fn lowest_priority_takes_a_released_voice_before_a_held_one() {
    let mut sampler = a_two_voice_sampler(StealPolicy::LowestPriority);
    // A soft note held, and a loud note let go: its release is two seconds,
    // so one block on it is still far louder than the soft note — the
    // quietest policy would take the soft one. Lowest priority takes the one
    // nobody is holding.
    sampler.note_on(60, 20, 0);
    sampler.note_on(64, 127, 1);
    run(&mut sampler);
    sampler.note_off(64, 1);
    run(&mut sampler);
    assert_eq!(
        keys(&sampler),
        [60, 64],
        "the released voice is still ringing"
    );
    sampler.note_on(67, 100, 2);
    assert_eq!(
        keys(&sampler),
        [60, 67],
        "the released 64 goes, though it is louder than the held 60"
    );

    // And with every voice held, it is the quietest that goes.
    let mut sampler = a_two_voice_sampler(StealPolicy::LowestPriority);
    sampler.note_on(60, 127, 0);
    run(&mut sampler);
    sampler.note_on(64, 20, 1);
    run(&mut sampler);
    sampler.note_on(67, 100, 2);
    assert_eq!(keys(&sampler), [60, 67]);
}

#[test]
fn quietest_with_every_voice_at_the_same_level_is_oldest() {
    // Nothing to choose between them, so the policy falls back to age
    // rather than to pool order — a tie broken by pool order would steal
    // slot 0 every time, which on a repeated chord is the same note.
    let mut sampler = a_two_voice_sampler(StealPolicy::Quietest);
    sampler.note_on(60, 100, 0);
    run(&mut sampler);
    sampler.note_on(64, 100, 1);
    run(&mut sampler);
    sampler.note_on(67, 100, 2);
    assert_eq!(keys(&sampler), [64, 67]);
}
