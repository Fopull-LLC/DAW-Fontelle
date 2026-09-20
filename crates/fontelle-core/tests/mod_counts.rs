//! The counts (`docs/flopsynth-next.md` §4.2, Ty's §9.6): eight LFOs, six
//! envelopes with the amp, eight macros. Every one is a fixed array in the
//! voice, and the voice has to stay small enough that a pool of sixty-four
//! is not a megabyte and a half of cache misses.
//!
//! The file keeps its old shape: a patch written with four macros, four
//! LFOs and four envelopes reads with eight, eight and six, and a patch
//! whose later slots are at rest writes four again — so none of the bank's
//! rows moves for a count nobody has used yet, and every Flopsynth patch
//! has every slot the strip shows (the slots are the instrument's, not the
//! patch's, which is how Serum has it).

use fontelle_core::flopsynth::{flopsynth_init, sources};
use fontelle_core::{
    Curve, MACRO_COUNT, MAX_LFOS, ModDest, ModRoute, ModSource, NoteTrigger, Patch, PrepareContext,
    SampleStore, Sampler, patch_params,
};

const SR: f32 = 48_000.0;

#[test]
fn the_counts_are_the_plans() {
    assert_eq!(MAX_LFOS, 8);
    assert_eq!(fontelle_core::MAX_MOD_ENVELOPES, 5, "five beside the amp");
    assert_eq!(MACRO_COUNT, 8);
}

#[test]
fn the_init_patch_carries_every_slot() {
    let patch = flopsynth_init();
    assert_eq!(patch.lfos.len(), MAX_LFOS);
    assert_eq!(
        patch.envelopes.len(),
        fontelle_core::MAX_MOD_ENVELOPES + 1,
        "the amp and five more"
    );
    assert_eq!(patch.macros.len(), MACRO_COUNT);
    // And the strip lists all of them, in order.
    let names: Vec<String> = sources(&patch).into_iter().map(|(_, n)| n).collect();
    for i in 1..=6 {
        assert!(
            names.contains(&format!("ENV {i}")),
            "ENV {i} missing: {names:?}"
        );
    }
    for i in 1..=8 {
        assert!(
            names.contains(&format!("LFO {i}")),
            "LFO {i} missing: {names:?}"
        );
    }
    for i in 1..=8 {
        assert!(names.contains(&format!("M{i}")), "M{i} missing: {names:?}");
    }
}

/// The plan's line was 24 KB, written before Phase 2 put a decimator's
/// state in every oscillator slot and before the comb's delay line was in
/// the filter: a voice is 110 KB, and sixteen `SynthState`s and six
/// `SynthFilter`s are 105 of them. What the counts add is the part this
/// phase owns, and it is under a kilobyte; the whole is held at what it
/// measures so the next thing that doubles it is noticed.
#[test]
fn the_counts_add_under_a_kilobyte_to_the_voice() {
    let modulators = MAX_LFOS * std::mem::size_of::<fontelle_core::LfoState>()
        + fontelle_core::MAX_MOD_ENVELOPES * std::mem::size_of::<fontelle_dsp::EnvelopeGenerator>();
    assert!(modulators < 1024, "the modulators are {modulators} bytes");
    let size = std::mem::size_of::<fontelle_core::Voice>();
    assert!(size < 128 * 1024, "a voice is {size} bytes");
}

#[test]
fn four_macros_in_the_file_read_as_eight_and_write_as_four() {
    let patch = flopsynth_init();
    let data = patch.to_data(&Default::default()).unwrap();
    let body = data.body.to_string();
    let written = data.body["macros"].as_array().expect("a list").len();
    assert_eq!(
        written, 4,
        "untouched macros past the fourth stay out: {body}"
    );
    let back = Patch::from_data(&data, |_| None).unwrap().patch;
    assert_eq!(back.macros.len(), MACRO_COUNT);
    assert_eq!(back, patch);

    // A patch that names the eighth writes all eight and reads them back.
    let mut named = patch.clone();
    named.macros[7].name = "Air".to_string();
    named.macros[7].value = 0.25;
    let data = named.to_data(&Default::default()).unwrap();
    assert_eq!(data.body["macros"].as_array().unwrap().len(), 8);
    let back = Patch::from_data(&data, |_| None).unwrap().patch;
    assert_eq!(back.macros[7].name, "Air");
    assert_eq!(back, named);

    // Six of them: between four and eight, only as many as are set.
    let mut six = patch.clone();
    six.macros[5].value = 0.5;
    let data = six.to_data(&Default::default()).unwrap();
    assert_eq!(data.body["macros"].as_array().unwrap().len(), 6);
    assert_eq!(Patch::from_data(&data, |_| None).unwrap().patch, six);
}

/// A row of the bank as it is on disk — four LFOs, four envelopes, four
/// macros — reads with every slot, and writes back what it was.
#[test]
fn a_four_slot_file_reads_full_and_writes_as_it_was() {
    let mut four = flopsynth_init();
    four.lfos.truncate(4);
    four.envelopes.truncate(4);
    let data = four.to_data(&Default::default()).unwrap();
    assert_eq!(data.body["lfos"].as_array().unwrap().len(), 4);
    assert_eq!(data.body["envelopes"].as_array().unwrap().len(), 4);
    let back = Patch::from_data(&data, |_| None).unwrap().patch;
    assert_eq!(back.lfos.len(), MAX_LFOS, "padded with LFOs at rest");
    assert_eq!(back.envelopes.len(), fontelle_core::MAX_MOD_ENVELOPES + 1);
    assert_eq!(back, flopsynth_init(), "the Init is the padded row");
    let again = back.to_data(&Default::default()).unwrap();
    assert_eq!(again.body, data.body, "the file does not move");

    // Touch the seventh LFO and it is written, with the at-rest ones
    // before it; the eighth stays out.
    let mut seven = back.clone();
    seven.lfos[6].rate_hz = 0.3;
    let data = seven.to_data(&Default::default()).unwrap();
    assert_eq!(data.body["lfos"].as_array().unwrap().len(), 7);
    assert_eq!(Patch::from_data(&data, |_| None).unwrap().patch, seven);
    // The same for the sixth envelope.
    let mut six = back.clone();
    six.envelopes[5].decay_s = 1.5;
    let data = six.to_data(&Default::default()).unwrap();
    assert_eq!(data.body["envelopes"].as_array().unwrap().len(), 6);
    assert_eq!(Patch::from_data(&data, |_| None).unwrap().patch, six);

    // A patch that is not Flopsynth's — an import with two LFOs — is left
    // with its two: the slots are the synth's.
    let mut import = Patch::basic_synth();
    import.lfos = vec![fontelle_core::Lfo::default(); 2];
    let data = import.to_data(&Default::default()).unwrap();
    assert_eq!(
        Patch::from_data(&data, |_| None).unwrap().patch.lfos.len(),
        2
    );
}

#[test]
fn the_eighth_macro_has_an_address() {
    let mut patch = flopsynth_init();
    let addresses = fontelle_core::flopsynth::addresses(&patch);
    for i in 0..MACRO_COUNT {
        let address = format!("patch/macro[{i}]");
        assert!(addresses.contains(&address), "no {address}");
        assert!(patch_params::set(&mut patch, &address, 0.75));
        assert_eq!(patch_params::value(&patch, &address), Some(0.75));
    }
    assert!(!patch_params::set(&mut patch, "patch/macro[8]", 0.5));
}

/// A route from the eighth LFO and from the sixth envelope is heard: the
/// voice advances what the patch has, not the first four.
#[test]
fn the_voice_reads_the_last_lfo_and_the_last_envelope() {
    let rms = |patch: Patch| {
        let store = SampleStore::new();
        let mut sampler = Sampler::new(patch);
        sampler.prepare(&PrepareContext {
            sample_rate: SR,
            max_block_size: 128,
        });
        sampler.trigger(NoteTrigger::new(60, 100));
        let mut sum = 0.0f64;
        let mut count = 0usize;
        // A second of it, in the engine's blocks, skipping the first tenth.
        for block in 0..(SR as usize / 128) {
            let mut l = [0.0f32; 128];
            let mut r = [0.0f32; 128];
            sampler.render(&store, &mut [&mut l, &mut r]);
            if block > 40 {
                sum += l.iter().map(|s| f64::from(s * s)).sum::<f64>();
                count += 128;
            }
        }
        (sum / count as f64).sqrt()
    };
    let route = |source: ModSource, destination: ModDest, depth: f32| ModRoute {
        source,
        destination,
        depth,
        curve: Curve::Linear,
        via: None,
        invert: false,
        bypass: false,
    };
    let base = flopsynth_init();
    let plain = rms(base.clone());
    assert!(plain > 1e-3);

    // LFO 8 at 0 Hz — a sine held at its phase — a quarter of the way in
    // reads 1.0, and on Amp at full depth is +24 dB.
    let mut lfo = base.clone();
    lfo.lfos[7].rate_hz = 0.0;
    lfo.lfos[7].phase = 0.25;
    lfo.lfos[7].sync = false;
    lfo.mod_matrix
        .routes
        .push(route(ModSource::Lfo(7), ModDest::Amp, 1.0));
    let louder = rms(lfo);
    assert!(
        louder > plain * 8.0,
        "LFO 8 on Amp: {plain} -> {louder} (expected ×15.8)"
    );

    // Envelope 6 held at full sustain, on Amp at −1: −24 dB.
    let mut env = base.clone();
    env.envelopes[5].attack_s = 0.0;
    env.envelopes[5].sustain_level = 1.0;
    env.mod_matrix
        .routes
        .push(route(ModSource::Envelope(5), ModDest::Amp, -1.0));
    let quieter = rms(env);
    assert!(
        quieter < plain / 8.0,
        "ENV 6 on Amp: {plain} -> {quieter} (expected ÷15.8)"
    );
}
