//! A sound dropped into Flopsynth, used as an oscillator's waveform.
//!
//! > *"i want to like with omnisphere or serum ... be able to drag audio files
//! > into it to use those waveforms in the synthesis as im pretty sure thats
//! > somethign you could do in them which would be a cool feature."*
//!
//! The bank's own tables are recipes and name no file (§3.2), which is what
//! makes a factory preset two hundred bytes and unbreakable. A dropped sound
//! cannot be a recipe, so the patch **carries the samples**: a preset made
//! from a file is self-contained, opens on a machine that has never seen that
//! file, and still needs no relinking. That is the same decision
//! `Source::Drum` made, taken to the one case where the numbers come from
//! outside.

use fontelle_core::{
    NoteTrigger, Patch, PrepareContext, SampleStore, Sampler, Source, UserWavetable, WavetableSet,
};
use fontelle_dsp::{SynthOsc, SynthSource, WavetableId};

const SR: f32 = 48_000.0;

/// Three cycles of a sine, as a file would arrive.
fn a_sound() -> Vec<f32> {
    (0..6_144)
        .map(|i| (std::f32::consts::TAU * i as f32 / 2_048.0).sin())
        .collect()
}

/// A Flopsynth patch whose first oscillator reads the patch's own table.
fn a_patch_reading_its_own_table() -> Patch {
    let mut patch = fontelle_core::flopsynth::flopsynth_init();
    patch.wavetables.push(UserWavetable {
        name: "Dropped".to_string(),
        frames: 3,
        samples: a_sound(),
    });
    if let Source::Synth(osc) = &mut patch.layers[0].source {
        osc.source = SynthSource::User(0);
    }
    patch
}

#[test]
fn a_patch_can_carry_a_table_of_its_own() {
    let patch = a_patch_reading_its_own_table();
    assert_eq!(patch.wavetables.len(), 1);
    assert_eq!(patch.wavetables[0].name, "Dropped");
    assert_eq!(patch.wavetables[0].frames, 3);
}

#[test]
fn the_set_resolves_the_patchs_own_tables_beside_the_banks() {
    let patch = a_patch_reading_its_own_table();
    let mut set = WavetableSet::new();
    set.resolve(&patch);
    let table = set.get_user(0).expect("the patch's own table is resolved");
    assert_eq!(table.frame_count(), 3);
    // And a table nothing names is not built: the same rule the bank's have.
    assert!(set.get_user(1).is_none());
}

#[test]
fn a_patch_that_names_no_table_of_its_own_carries_none() {
    let patch = fontelle_core::flopsynth::flopsynth_init();
    let mut set = WavetableSet::new();
    set.resolve(&patch);
    assert!(set.get_user(0).is_none());
}

#[test]
fn an_oscillator_reading_a_dropped_sound_makes_that_sound() {
    // The whole point, and the trap the audio-clip work names: a document
    // test that only asks what the patch *says* passes while the studio is
    // silent. So this renders.
    let mut sampler = Sampler::new(a_patch_reading_its_own_table());
    sampler.prepare(&PrepareContext {
        sample_rate: SR,
        max_block_size: 512,
    });
    sampler.trigger(NoteTrigger::new(60, 100));
    let store = SampleStore::new();
    let mut left = vec![0.0f32; 4_800];
    let mut right = vec![0.0f32; 4_800];
    sampler.render(&store, &mut [&mut left[..], &mut right[..]]);
    let peak = left.iter().fold(0.0f32, |a, s| a.max(s.abs()));
    assert!(peak > 0.01, "a dropped sound must be audible, not {peak}");
}

#[test]
fn a_table_the_patch_does_not_have_is_silence_rather_than_a_panic() {
    // A patch from another build, or one whose table was removed: the layer
    // says nothing rather than taking the process with it, which is what a
    // missing bank table already does.
    let mut patch = fontelle_core::flopsynth::flopsynth_init();
    if let Source::Synth(osc) = &mut patch.layers[0].source {
        osc.source = SynthSource::User(3);
    }
    let mut sampler = Sampler::new(patch);
    sampler.prepare(&PrepareContext {
        sample_rate: SR,
        max_block_size: 512,
    });
    sampler.trigger(NoteTrigger::new(60, 100));
    let store = SampleStore::new();
    let mut left = vec![0.0f32; 512];
    let mut right = vec![0.0f32; 512];
    sampler.render(&store, &mut [&mut left[..], &mut right[..]]);
    assert!(left.iter().all(|s| s.abs() < 1e-6));
}

#[test]
fn a_patch_with_a_dropped_sound_survives_being_saved_and_opened() {
    // INVARIANT 7, and the reason the samples are *in* the patch: a preset
    // made from a file has to open on a machine that has never seen it.
    let patch = a_patch_reading_its_own_table();
    let data = patch.to_data(&Default::default()).expect("writes");
    let back = Patch::from_data(&data, |_| None).expect("reads").patch;
    assert_eq!(back.wavetables.len(), 1);
    assert_eq!(back.wavetables[0].name, "Dropped");
    assert_eq!(back.wavetables[0].frames, 3);
    assert_eq!(
        back.wavetables[0].samples.len(),
        patch.wavetables[0].samples.len()
    );
    // Sixteen-bit, so the samples come back close rather than exact — the
    // table is normalised on the way into the pyramid anyway, and a preset
    // is not an archive of the file.
    for (a, b) in patch.wavetables[0]
        .samples
        .iter()
        .zip(&back.wavetables[0].samples)
    {
        assert!((a - b).abs() < 1e-3, "{a} came back as {b}");
    }
    if let Source::Synth(osc) = &back.layers[0].source {
        assert_eq!(osc.source, SynthSource::User(0));
    } else {
        panic!("the layer stopped being a synth layer");
    }
}

#[test]
fn a_preset_written_before_dropped_sounds_existed_opens_with_none() {
    // Every factory preset is one of these, so this is the one that says the
    // whole bank still opens.
    let patch = fontelle_core::flopsynth::flopsynth_init();
    let data = patch.to_data(&Default::default()).expect("writes");
    let mut body = data.body.clone();
    body.as_object_mut()
        .expect("an object")
        .remove("wavetables");
    let older = fontelle_types::PatchData {
        format_version: data.format_version,
        body,
    };
    let back = Patch::from_data(&older, |_| None).expect("reads").patch;
    assert!(back.wavetables.is_empty());
}

#[test]
fn the_bank_and_a_dropped_sound_are_two_different_sources() {
    // Naming one must not be read as the other: a patch reading its own
    // table zero and one reading the bank's first table are different
    // instruments, and serde writes both by name.
    let bank = SynthSource::Table(WavetableId::Sine);
    let dropped = SynthSource::User(0);
    assert_ne!(bank, dropped);
    let osc = SynthOsc {
        source: dropped,
        ..SynthOsc::default()
    };
    let text = serde_json::to_string(&osc).expect("writes");
    let back: SynthOsc = serde_json::from_str(&text).expect("reads");
    assert_eq!(back.source, dropped);
}
