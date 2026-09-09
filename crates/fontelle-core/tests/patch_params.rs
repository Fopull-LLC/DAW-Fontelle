//! Writing one addressed parameter into a patch, from the audio thread.
//!
//! *"there's no way to actually turn a knob into an automation clip. i want to
//! be able to right click on a knob and select create automation clip."* The
//! channel's own volume and pan could already be automated; the knobs **inside**
//! the instrument — a cutoff, an envelope stage, an oscillator's level — could
//! not, because nothing below the app layer knew how to turn one of §8.2's
//! addresses into a change to a `Patch`.
//!
//! That mapping used to live in `fontelle-app`, which the engine cannot see. It
//! lives here now, beside the `Patch` it writes to, and the instrument panel
//! calls the same function — so what you can right-click and what automation
//! can move are the same list by construction rather than by agreement.
//!
//! **It runs on the audio thread**, so it allocates nothing: the address is
//! parsed by splitting a `&str`, and everything it does is a field write.

use fontelle_core::{
    NoteTrigger, Patch, PrepareContext, SampleStore, Sampler, Source, patch_params,
};
use fontelle_dsp::{OscKind, SvfMode};

const SR: f32 = 48_000.0;

fn synth() -> Patch {
    Patch::basic_synth()
}

// ------------------------------------------------------------- the write ---

#[test]
fn a_filters_cutoff_is_addressable() {
    let mut patch = synth();
    assert!(patch_params::set(&mut patch, "patch/filter[0]/cutoff", 0.0));
    let low = patch.filters[0].cutoff_hz;
    assert!(patch_params::set(&mut patch, "patch/filter[0]/cutoff", 1.0));
    assert!(
        patch.filters[0].cutoff_hz > low * 100.0,
        "the whole dial is decades, not hertz: {low} to {}",
        patch.filters[0].cutoff_hz
    );
}

#[test]
fn an_envelope_stage_and_a_switch_are_addressable() {
    let mut patch = synth();
    assert!(patch_params::set(&mut patch, "patch/env[0]/attack", 1.0));
    assert!(
        patch.envelopes[0].attack_s > 1.0,
        "the top of the dial is seconds"
    );
    assert!(patch_params::set(
        &mut patch,
        "patch/filter[1]/enabled",
        1.0
    ));
    assert!(patch.filters[1].enabled);
}

#[test]
fn a_choice_steps_through_its_options() {
    let mut patch = synth();
    assert!(patch_params::set(&mut patch, "patch/filter[0]/mode", 0.0));
    assert_eq!(patch.filters[0].mode, SvfMode::Lowpass);
    assert!(patch_params::set(&mut patch, "patch/filter[0]/mode", 1.0));
    assert_eq!(
        patch.filters[0].mode,
        SvfMode::HighShelf,
        "the top of the chooser is its last option"
    );
}

/// An oscillator's own three, which is what the built-in synth is made of.
#[test]
fn an_oscillators_shape_octave_and_tuning_are_addressable() {
    let mut patch = synth();
    assert!(patch_params::set(&mut patch, "patch/layer[0]/shape", 1.0));
    assert_eq!(patch.layers[0].source, Source::Oscillator(OscKind::Noise));
    assert!(patch_params::set(&mut patch, "patch/layer[0]/octave", 0.0));
    assert_eq!(patch.layers[0].root_key, 84, "two octaves down");
    assert!(patch_params::set(&mut patch, "patch/layer[0]/tune", 1.0));
    assert!((patch.layers[0].fine_tune_cents - 100.0).abs() < 0.01);
}

/// INVARIANT 7: an address this build does not recognise changes nothing, and
/// is not an error. A project naming a parameter a later build dropped has to
/// open rather than refuse.
#[test]
fn an_address_this_build_does_not_know_changes_nothing() {
    let mut patch = synth();
    let before = patch.clone();
    for address in [
        "",
        "patch/",
        "patch/filter[9]/cutoff",
        "patch/filter[x]/cutoff",
        "patch/env[0]/wobble",
        "patch/layer[0]/shape/extra",
        "mixer:1/gain",
        "patch/something-from-2030",
    ] {
        assert!(
            !patch_params::set(&mut patch, address, 0.5),
            "{address} should not have been recognised"
        );
    }
    assert_eq!(patch, before, "and nothing moved");
}

/// A sampled layer refuses an oscillator's controls rather than silently
/// transposing somebody's piano.
#[test]
fn a_sampled_layer_refuses_an_oscillators_controls() {
    let mut patch = synth();
    let asset = SampleStore::new();
    let _ = asset;
    patch.layers[0].source = Source::Sample {
        file: fontelle_types::AssetId::default(),
    };
    let root = patch.layers[0].root_key;
    assert!(!patch_params::set(&mut patch, "patch/layer[0]/shape", 1.0));
    assert!(!patch_params::set(&mut patch, "patch/layer[0]/octave", 0.0));
    assert_eq!(patch.layers[0].root_key, root);
    // Its level and placement are still its own.
    assert!(patch_params::set(&mut patch, "patch/layer[0]/gain", 1.0));
}

// -------------------------------------------------------------- the read ---

/// Every write reads back as what was written, which is what makes an
/// automation lane's first point sit where the knob is rather than jumping it.
#[test]
fn what_was_written_reads_back() {
    let mut patch = synth();
    for address in [
        "patch/filter[0]/cutoff",
        "patch/filter[0]/resonance",
        "patch/env[0]/attack",
        "patch/env[0]/sustain",
        "patch/layer[0]/gain",
        "patch/layer[0]/pan",
        "patch/voice/glide",
    ] {
        for wanted in [0.0f32, 0.25, 0.5, 1.0] {
            assert!(patch_params::set(&mut patch, address, wanted), "{address}");
            let read = patch_params::value(&patch, address)
                .unwrap_or_else(|| panic!("{address} has no value"));
            assert!(
                (read - wanted).abs() < 0.01,
                "{address} was set to {wanted} and reads {read}"
            );
        }
    }
}

#[test]
fn a_parameter_that_is_not_there_has_no_value() {
    let patch = synth();
    assert_eq!(patch_params::value(&patch, "patch/filter[9]/cutoff"), None);
    assert_eq!(patch_params::value(&patch, "nonsense"), None);
}

// ------------------------------------------------------ through the sampler ---

/// The seam automation actually crosses: a value arrives at the sampler by
/// address and changes what it sounds like.
#[test]
fn a_sampler_takes_an_addressed_parameter_and_it_is_audible() {
    let store = SampleStore::new();
    let mut sampler = Sampler::new(synth());
    sampler.prepare(&PrepareContext {
        sample_rate: SR,
        max_block_size: 4_800,
    });

    let render = |sampler: &mut Sampler| {
        let mut buffer = vec![0.0f32; 4_800];
        {
            let mut out: Vec<&mut [f32]> = vec![&mut buffer];
            sampler.render(&store, &mut out);
        }
        (buffer.iter().map(|s| s * s).sum::<f32>() / buffer.len() as f32).sqrt()
    };

    sampler.trigger(NoteTrigger::new(60, 127));
    let open = render(&mut sampler);

    // Shut the filter right down, the way a swept automation lane would.
    assert!(sampler.set_patch_param("patch/filter[0]/cutoff", 0.0));
    let closed = render(&mut sampler);
    assert!(
        closed < open * 0.5,
        "a cutoff swept to the bottom has to be heard: {open} against {closed}"
    );

    // And an address it does not know does nothing rather than panicking on
    // the audio thread.
    assert!(!sampler.set_patch_param("patch/nothing-here", 1.0));
}

// -------------------------------------------------- and the one that was inert

/// **A polyphony lane actually limits the voices.**
///
/// `patch/voice/polyphony` was addressable, drawable and automatable, and
/// moving it through a lane did nothing at all: the pool was sized once in
/// `Sampler::new` and never read again, so the *knob* worked — turning it
/// rebuilds the graph — and the lane did not. That is the worst shape a defect
/// can have here, because the lane draws, saves and plays, and the only thing
/// missing is the sound.
///
/// The fix is not to resize the pool: that is a `Vec` allocation, and this runs
/// on the audio thread (INVARIANT 1). The pool keeps the size it was built at
/// and polyphony becomes a **limit within it**, which is what the word means
/// anyway — a note that finds no voice under the limit steals one, exactly as
/// it does when the pool is full.
#[test]
fn lowering_polyphony_through_a_lane_takes_voices_away() {
    let mut store = SampleStore::new();
    let mut patch = synth();
    patch.voice_config.polyphony = 8;
    let mut sampler = Sampler::new(patch);
    sampler.prepare(&PrepareContext {
        sample_rate: SR,
        max_block_size: 512,
    });

    // Four notes, four voices.
    for (n, key) in [60, 64, 67, 71].into_iter().enumerate() {
        sampler.note_on(key, 100, n as u32);
    }
    assert_eq!(sampler.active_voices(), 4, "eight voices, four notes");

    // A lane pulls polyphony down to one. The next note has to take the only
    // slot there now is, rather than becoming a fifth voice.
    sampler.set_patch_param("patch/voice/polyphony", 0.0);
    sampler.note_on(72, 100, 4);
    assert!(
        sampler.active_voices() <= 4,
        "a polyphony of one should not have added a fifth voice; {} are sounding",
        sampler.active_voices()
    );

    let mut left = vec![0.0; 256];
    let mut right = vec![0.0; 256];
    sampler.render(&store, &mut [&mut left[..], &mut right[..]]);
    assert!(
        left.iter().all(|s| s.is_finite()),
        "and the limit did not break the render"
    );
    let _ = &mut store;
}

#[test]
fn raising_it_again_gives_the_voices_back() {
    // The other direction, up to the size the pool was built at: a lane that
    // could only ever take voices away would be a lane you could not undo.
    let mut patch = synth();
    patch.voice_config.polyphony = 6;
    let mut sampler = Sampler::new(patch);
    sampler.prepare(&PrepareContext {
        sample_rate: SR,
        max_block_size: 512,
    });

    sampler.set_patch_param("patch/voice/polyphony", 0.0);
    for (n, key) in [60, 64, 67].into_iter().enumerate() {
        sampler.note_on(key, 100, n as u32);
    }
    let choked = sampler.active_voices();
    assert!(choked <= 2, "one voice at a time; {choked} were sounding");

    // Back up. `set` clamps into 1..=MAX_POLYPHONY, and the pool caps it at the
    // size it was built with — see `the_pool_is_the_ceiling`.
    sampler.set_patch_param("patch/voice/polyphony", 1.0);
    for (n, key) in [60, 64, 67, 71, 74, 77].into_iter().enumerate() {
        sampler.note_on(key, 100, 100 + n as u32);
    }
    assert!(
        sampler.active_voices() > choked,
        "raising the limit should let more notes sound; still {}",
        sampler.active_voices()
    );
}

/// **The pool is the ceiling, and that is a real limit worth stating.**
///
/// A lane can lower polyphony to one and raise it back to whatever the pool
/// was built at. It cannot raise it *past* that, because growing the pool means
/// allocating and this runs on the audio thread. The pool is built from the
/// patch, so the knob's own value is the ceiling — turning the knob up rebuilds
/// the graph and raises it.
#[test]
fn the_pool_is_the_ceiling() {
    let mut patch = synth();
    patch.voice_config.polyphony = 2;
    let mut sampler = Sampler::new(patch);
    sampler.prepare(&PrepareContext {
        sample_rate: SR,
        max_block_size: 512,
    });

    // A lane asking for the top of the range on a pool of two.
    sampler.set_patch_param("patch/voice/polyphony", 1.0);
    for (n, key) in [60, 62, 64, 65, 67, 69].into_iter().enumerate() {
        sampler.note_on(key, 100, n as u32);
    }
    assert!(
        sampler.active_voices() <= 2,
        "a pool of two cannot sound {} notes",
        sampler.active_voices()
    );
}
