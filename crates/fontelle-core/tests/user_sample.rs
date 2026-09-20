//! A recording dropped into Flopsynth, played as an oscillator.
//!
//! > *"we could actually sample a real piano sound and then do effects and
//! > modulating and layering with other oscilators and stuff."*
//!
//! `tests/user_wavetable.rs` is a sound cut into cycles; this is a sound
//! kept whole. The same decision carries: the patch **owns the samples**, so
//! a preset made from a recording opens on a machine that has never seen the
//! file and never needs relinking. What is new is that a recording has a
//! pitch and a length — and may be several recordings, one per stretch of the
//! keyboard, which is what a sampled piano is.

use std::sync::Arc;

use fontelle_core::{
    NoteTrigger, Patch, PrepareContext, SampleStore, SampleZone, Sampler, Source, UserSample,
    WavetableSet, flopsynth,
};
use fontelle_dsp::{SampleLoop, SynthOsc, SynthSource};

const SR: f32 = 48_000.0;

/// A recording: `seconds` of a sine at `hz`.
fn tone(hz: f32, seconds: f32) -> Arc<[f32]> {
    (0..(SR * seconds) as usize)
        .map(|i| (std::f32::consts::TAU * hz * i as f32 / SR).sin() * 0.8)
        .collect()
}

/// One zone over the whole keyboard, recorded at A4.
fn a_recording() -> UserSample {
    UserSample {
        name: "Piano A4".to_string(),
        factory: None,
        zones: vec![SampleZone {
            name: String::new(),
            root_key: 69,
            fine_cents: 0.0,
            key_range: (0, 127),
            sample_rate: SR as u32,
            samples: tone(440.0, 1.0),
            vel_range: (0, 127),
            gain_db: 0.0,
            loop_frames: None,
        }],
    }
}

/// A Flopsynth patch whose first oscillator plays the patch's own recording.
fn a_patch_playing_its_own_recording() -> Patch {
    let mut patch = flopsynth::flopsynth_init();
    patch.samples.push(a_recording());
    if let Source::Synth(osc) = &mut patch.layers[0].source {
        osc.source = SynthSource::Sample(0);
    }
    patch
}

fn render(patch: Patch, key: u8, frames: usize) -> Vec<f32> {
    let mut sampler = Sampler::new(patch);
    sampler.prepare(&PrepareContext {
        sample_rate: SR,
        max_block_size: 512,
    });
    sampler.trigger(NoteTrigger::new(key, 100));
    let store = SampleStore::new();
    let mut out = Vec::with_capacity(frames);
    while out.len() < frames {
        let n = 512.min(frames - out.len());
        let mut left = vec![0.0f32; n];
        let mut right = vec![0.0f32; n];
        sampler.render(&store, &mut [&mut left[..], &mut right[..]]);
        out.extend_from_slice(&left);
    }
    out
}

fn zero_crossings_per_second(samples: &[f32]) -> f32 {
    let crossings = samples
        .windows(2)
        .filter(|w| w[0] <= 0.0 && w[1] > 0.0)
        .count();
    crossings as f32 / (samples.len() as f32 / SR)
}

#[test]
fn a_patch_can_carry_a_recording_of_its_own() {
    let patch = a_patch_playing_its_own_recording();
    assert_eq!(patch.samples.len(), 1);
    assert_eq!(patch.samples[0].name, "Piano A4");
    assert_eq!(patch.samples[0].zones[0].root_key, 69);
    // And the blank patch carries none, so every factory preset stays two
    // hundred bytes.
    assert!(flopsynth::flopsynth_init().samples.is_empty());
}

/// A sampled instrument is several recordings, each over a stretch of keys.
/// A key inside a zone's range plays that zone; a key outside every range
/// plays the zone whose root is nearest, so a two-sample instrument still
/// plays the whole keyboard.
#[test]
fn a_key_plays_the_zone_that_covers_it_or_the_nearest_root() {
    let sample = UserSample {
        name: "Two".to_string(),
        factory: None,
        zones: vec![
            SampleZone {
                name: "Low".to_string(),
                root_key: 48,
                fine_cents: 0.0,
                key_range: (40, 54),
                sample_rate: SR as u32,
                samples: tone(130.8, 0.1),
                vel_range: (0, 127),
                gain_db: 0.0,
                loop_frames: None,
            },
            SampleZone {
                name: "High".to_string(),
                root_key: 72,
                fine_cents: 0.0,
                key_range: (66, 78),
                sample_rate: SR as u32,
                samples: tone(523.3, 0.1),
                vel_range: (0, 127),
                gain_db: 0.0,
                loop_frames: None,
            },
        ],
    };
    assert_eq!(sample.zone_for(50).map(|z| z.root_key), Some(48));
    assert_eq!(sample.zone_for(70).map(|z| z.root_key), Some(72));
    // Between the two ranges: the nearer root.
    assert_eq!(sample.zone_for(58).map(|z| z.root_key), Some(48));
    assert_eq!(sample.zone_for(62).map(|z| z.root_key), Some(72));
    // Past both ends.
    assert_eq!(sample.zone_for(20).map(|z| z.root_key), Some(48));
    assert_eq!(sample.zone_for(100).map(|z| z.root_key), Some(72));
    let empty = UserSample {
        name: "None".to_string(),
        factory: None,
        zones: Vec::new(),
    };
    assert!(empty.zone_for(60).is_none());
}

#[test]
fn the_set_resolves_the_patchs_own_recordings() {
    let patch = a_patch_playing_its_own_recording();
    let mut set = WavetableSet::new();
    set.resolve(&patch);
    let sample = set.get_sample(0).expect("the recording is resolved");
    assert_eq!(sample.name, "Piano A4");
    assert!(set.get_sample(1).is_none());
    // And nothing is resolved for a patch that names none.
    let mut set = WavetableSet::new();
    set.resolve(&flopsynth::flopsynth_init());
    assert!(set.get_sample(0).is_none());
}

#[test]
fn an_oscillator_playing_a_recording_plays_it_at_the_note() {
    // The whole point: the recording comes out of the voice, and at the pitch
    // asked for — its own at its root, an octave up an octave up.
    let out = render(a_patch_playing_its_own_recording(), 69, 24_000);
    let peak = out.iter().fold(0.0f32, |a, s| a.max(s.abs()));
    assert!(peak > 0.01, "a recording must be audible, not {peak}");
    let measured = zero_crossings_per_second(&out[2_400..]);
    assert!(
        (measured - 440.0).abs() < 5.0,
        "at its root key the recording plays as recorded: {measured} Hz"
    );
    let out = render(a_patch_playing_its_own_recording(), 81, 24_000);
    let measured = zero_crossings_per_second(&out[2_400..]);
    assert!(
        (measured - 880.0).abs() < 10.0,
        "an octave up it plays an octave up: {measured} Hz"
    );
}

/// The zone's own tuning: a recording that was a quarter-tone flat of its
/// key is played a quarter-tone sharper, so the note lands on pitch.
#[test]
fn a_zones_fine_tune_corrects_the_recording() {
    let mut patch = a_patch_playing_its_own_recording();
    // The tone *is* 440, but the zone claims it is 50 cents flat of A4.
    patch.samples[0].zones[0].fine_cents = -50.0;
    let out = render(patch, 69, 24_000);
    let measured = zero_crossings_per_second(&out[2_400..]);
    let expected = 440.0 * 2f32.powf(50.0 / 1200.0);
    assert!(
        (measured - expected).abs() < 5.0,
        "a zone marked 50 cents flat should be played 50 cents sharp: {measured} \
         against {expected}"
    );
}

#[test]
fn a_recording_the_patch_does_not_have_is_silence_rather_than_a_panic() {
    let mut patch = flopsynth::flopsynth_init();
    if let Source::Synth(osc) = &mut patch.layers[0].source {
        osc.source = SynthSource::Sample(4);
    }
    let out = render(patch, 60, 512);
    assert!(out.iter().all(|s| s.abs() < 1e-6));
}

#[test]
fn a_patch_with_a_recording_survives_being_saved_and_opened() {
    let mut patch = a_patch_playing_its_own_recording();
    patch.samples[0].zones[0].fine_cents = 12.5;
    patch.samples[0].zones[0].key_range = (40, 80);
    if let Source::Synth(osc) = &mut patch.layers[0].source {
        osc.sample.loop_mode = SampleLoop::Forward;
        osc.sample.loop_start = 0.25;
    }
    patch.samples[0].zones[0].name = "A4 soft".to_string();
    let data = patch.to_data(&Default::default()).expect("writes");
    let back = Patch::from_data(&data, |_| None).expect("reads").patch;
    assert_eq!(back.samples.len(), 1);
    assert_eq!(back.samples[0].name, "Piano A4");
    let zone = &back.samples[0].zones[0];
    assert_eq!(zone.name, "A4 soft", "a zone's name is kept");
    assert_eq!(zone.root_key, 69);
    assert_eq!(zone.fine_cents, 12.5);
    assert_eq!(zone.key_range, (40, 80));
    assert_eq!(zone.sample_rate, SR as u32);
    assert_eq!(zone.samples.len(), patch.samples[0].zones[0].samples.len());
    // Sixteen-bit, so close rather than exact — see `StoredWavetable`.
    for (a, b) in patch.samples[0].zones[0]
        .samples
        .iter()
        .zip(zone.samples.iter())
    {
        assert!((a - b).abs() < 1e-3, "{a} came back as {b}");
    }
    let Source::Synth(osc) = &back.layers[0].source else {
        panic!("the layer stopped being a synth layer");
    };
    assert_eq!(osc.source, SynthSource::Sample(0));
    assert_eq!(osc.sample.loop_mode, SampleLoop::Forward);
    assert_eq!(osc.sample.loop_start, 0.25);
}

#[test]
fn a_preset_written_before_recordings_existed_opens_with_none() {
    let patch = flopsynth::flopsynth_init();
    let data = patch.to_data(&Default::default()).expect("writes");
    let mut body = data.body.clone();
    body.as_object_mut().expect("an object").remove("samples");
    let older = fontelle_types::PatchData {
        format_version: data.format_version,
        body,
    };
    let back = Patch::from_data(&older, |_| None).expect("reads").patch;
    assert!(back.samples.is_empty());
    // And an oscillator written before the sample and string settings
    // existed reads with their defaults.
    let osc: SynthOsc = serde_json::from_str(
        r#"{"source":{"Table":"Saw"},"position":0.0,"warp":"Off","warp_amount":0.0,
            "modulator":null,"unison":{"voices":1,"detune_cents":15.0,"blend":1.0,"width":0.5},
            "phase":0.0,"random_phase":false,"semitones":0,"key_track":true,
            "filter_route":"F1","noise_colour":0.0}"#,
    )
    .expect("an older oscillator reads");
    assert_eq!(osc.sample.loop_mode, SampleLoop::Off);
    assert_eq!(osc.string, fontelle_dsp::StringModel::default());
}

/// The three kinds of source have three sets of controls, and the address
/// list says which — so an automation lane can reach a loop point on a
/// sample and a stiffness on a string, and neither on a table.
#[test]
fn each_kind_of_source_lists_its_own_addresses() {
    let mut patch = flopsynth::flopsynth_init();
    let table = flopsynth::addresses(&patch);
    assert!(table.contains(&"patch/layer[0]/synth/table".to_string()));
    assert!(table.contains(&"patch/layer[0]/synth/kind".to_string()));
    assert!(!table.contains(&"patch/layer[0]/synth/sample/loop".to_string()));
    assert!(!table.contains(&"patch/layer[0]/synth/string/stiffness".to_string()));

    if let Source::Synth(osc) = &mut patch.layers[0].source {
        osc.source = SynthSource::Sample(0);
    }
    let sample = flopsynth::addresses(&patch);
    for field in ["loop", "loop_start", "loop_end", "grain", "spray", "zone"] {
        let address = format!("patch/layer[0]/synth/sample/{field}");
        assert!(sample.contains(&address), "a sample lists {address}");
    }
    assert!(sample.contains(&"patch/layer[0]/synth/position".to_string()));
    assert!(!sample.contains(&"patch/layer[0]/synth/table".to_string()));

    if let Source::Synth(osc) = &mut patch.layers[0].source {
        osc.source = SynthSource::String;
    }
    let string = flopsynth::addresses(&patch);
    for field in ["stiffness", "damping", "strike", "decay"] {
        let address = format!("patch/layer[0]/synth/string/{field}");
        assert!(string.contains(&address), "a string lists {address}");
    }
    assert!(!string.contains(&"patch/layer[0]/synth/sample/loop".to_string()));
    // The noise layer is none of the three.
    assert!(!string.contains(&"patch/layer[4]/synth/kind".to_string()));
}

/// Every new address writes and reads back — `patch_params`' own contract,
/// held for the new fields.
#[test]
fn the_new_addresses_write_and_read_back() {
    use fontelle_core::patch_params;
    let mut patch = flopsynth::flopsynth_init();
    // The kind chooser: three positions, table, sample, string.
    assert!(patch_params::set(
        &mut patch,
        "patch/layer[0]/synth/kind",
        0.5
    ));
    assert!(matches!(
        patch.layers[0].source,
        Source::Synth(SynthOsc {
            source: SynthSource::Sample(_),
            ..
        })
    ));
    // The loop chooser: five ways of reading, Loop the second of them.
    assert!(patch_params::set(
        &mut patch,
        "patch/layer[0]/synth/sample/loop",
        0.25
    ));
    assert!(patch_params::set(
        &mut patch,
        "patch/layer[0]/synth/sample/loop_start",
        0.3
    ));
    assert!(patch_params::set(
        &mut patch,
        "patch/layer[0]/synth/sample/loop_end",
        0.9
    ));
    // The grain knob is logarithmic over 5..500 ms, so half way is fifty;
    // the spray is plain; the zone chooser's first position is "any".
    assert!(patch_params::set(
        &mut patch,
        "patch/layer[0]/synth/sample/grain",
        0.5
    ));
    assert!(patch_params::set(
        &mut patch,
        "patch/layer[0]/synth/sample/spray",
        0.4
    ));
    let Source::Synth(osc) = &patch.layers[0].source else {
        panic!()
    };
    assert_eq!(osc.sample.loop_mode, SampleLoop::Forward);
    assert!((osc.sample.loop_start - 0.3).abs() < 1e-6);
    assert!((osc.sample.loop_end - 0.9).abs() < 1e-6);
    assert!(
        (osc.sample.grain_ms - 50.0).abs() < 0.5,
        "half way up the grain knob is fifty milliseconds: {}",
        osc.sample.grain_ms
    );
    assert!((osc.sample.spray - 0.4).abs() < 1e-6);
    for (address, value) in [
        ("patch/layer[0]/synth/sample/loop", 0.25),
        ("patch/layer[0]/synth/sample/loop_start", 0.3),
        ("patch/layer[0]/synth/sample/loop_end", 0.9),
        ("patch/layer[0]/synth/sample/grain", 0.5),
        ("patch/layer[0]/synth/sample/spray", 0.4),
        ("patch/layer[0]/synth/sample/zone", 0.0),
        ("patch/layer[0]/synth/kind", 0.5),
    ] {
        let read = patch_params::value(&patch, address).expect(address);
        assert!(
            (read - value).abs() < 1e-3,
            "{address}: wrote {value}, read {read}"
        );
    }

    assert!(patch_params::set(
        &mut patch,
        "patch/layer[0]/synth/kind",
        1.0
    ));
    assert!(matches!(
        patch.layers[0].source,
        Source::Synth(SynthOsc {
            source: SynthSource::String,
            ..
        })
    ));
    for field in ["stiffness", "damping", "strike", "decay"] {
        let address = format!("patch/layer[0]/synth/string/{field}");
        assert!(
            patch_params::set(&mut patch, &address, 0.7),
            "{address} sets"
        );
        let read = patch_params::value(&patch, &address).expect(&address);
        assert!((read - 0.7).abs() < 1e-3, "{address}: read {read}");
    }
    let Source::Synth(osc) = &patch.layers[0].source else {
        panic!()
    };
    assert!((osc.string.stiffness - 0.7).abs() < 1e-6);
    assert!((osc.string.strike - patch_params::lerp(0.7, 0.02, 0.5)).abs() < 1e-6);

    // Back to a table, and the table chooser works again.
    assert!(patch_params::set(
        &mut patch,
        "patch/layer[0]/synth/kind",
        0.0
    ));
    assert!(patch_params::set(
        &mut patch,
        "patch/layer[0]/synth/table",
        0.0
    ));
    assert!(matches!(
        patch.layers[0].source,
        Source::Synth(SynthOsc {
            source: SynthSource::Table(_),
            ..
        })
    ));
    // The noise layer takes none of them.
    assert!(!patch_params::set(
        &mut patch,
        "patch/layer[4]/synth/kind",
        1.0
    ));
}

/// A string through the voice: the same patch, the same filters and
/// envelopes, with the first oscillator a string rather than a table — and
/// the stretch is in what comes out.
#[test]
fn a_string_oscillator_rings_through_the_voice_with_its_stretch() {
    let mut patch = flopsynth::flopsynth_init();
    if let Source::Synth(osc) = &mut patch.layers[0].source {
        osc.source = SynthSource::String;
        osc.string.stiffness = 0.5;
        osc.position = 1.0;
    }
    // Open the filter right up so the partials are what the string made.
    patch.filters[0].cutoff_hz = 20_000.0;
    let out = render(patch, 60, 65_536);
    let peak = out.iter().fold(0.0f32, |a, s| a.max(s.abs()));
    assert!(peak > 0.01, "a string must be audible, not {peak}");
    let f0 = 261.625_56;
    let energy_at = |hz: f32| {
        let n = out.len() as f32;
        let (mut re, mut im) = (0.0f32, 0.0f32);
        for (i, s) in out.iter().enumerate() {
            let w = 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / n).cos();
            let p = std::f32::consts::TAU * hz * i as f32 / SR;
            re += s * w * p.cos();
            im -= s * w * p.sin();
        }
        (re * re + im * im).sqrt()
    };
    let mut best = (0.0f32, 0.0f32);
    let mut cents = -20.0f32;
    while cents <= 150.0 {
        let e = energy_at(f0 * 8.0 * 2f32.powf(cents / 1200.0));
        if e > best.0 {
            best = (e, cents);
        }
        cents += 2.0;
    }
    assert!(
        best.1 > 20.0,
        "the 8th partial should come out of the voice stretched: {} cents",
        best.1
    );
}

/// The modulation destinations name what the position knob *is* on each
/// kind of source, so a route reads "OSC A bright" on a string rather than
/// "OSC A position".
#[test]
fn the_position_destination_is_named_for_what_it_does() {
    use fontelle_core::ModDest;
    let label = |patch: &Patch| {
        flopsynth::destinations(patch)
            .into_iter()
            .find(|(dest, _)| *dest == ModDest::OscPosition(0))
            .map(|(_, label)| label)
            .expect("osc A has a position destination")
    };
    let mut patch = flopsynth::flopsynth_init();
    assert_eq!(label(&patch), "OSC A position");
    if let Source::Synth(osc) = &mut patch.layers[0].source {
        osc.source = SynthSource::Sample(0);
    }
    assert_eq!(label(&patch), "OSC A start");
    if let Source::Synth(osc) = &mut patch.layers[0].source {
        osc.source = SynthSource::String;
    }
    assert_eq!(label(&patch), "OSC A bright");
}

/// A locked zone plays for every key, transposed from its own root, where
/// the unlocked read picks the zone whose range holds the key. What a
/// kit's snare across the keyboard is.
#[test]
fn a_locked_zone_plays_for_every_key() {
    let mut patch = flopsynth::flopsynth_init();
    patch.samples.push(UserSample {
        name: "Two".to_string(),
        factory: None,
        zones: vec![
            SampleZone {
                name: "Low".to_string(),
                root_key: 48,
                fine_cents: 0.0,
                key_range: (0, 60),
                sample_rate: SR as u32,
                samples: tone(130.8, 1.0),
                vel_range: (0, 127),
                gain_db: 0.0,
                loop_frames: None,
            },
            // Recorded an octave *above* its root on purpose, so that what
            // it plays for a key is unmistakably this zone and not the
            // other one transposed to the same pitch.
            SampleZone {
                name: "High".to_string(),
                root_key: 72,
                fine_cents: 0.0,
                key_range: (61, 127),
                sample_rate: SR as u32,
                samples: tone(1_046.5, 1.0),
                vel_range: (0, 127),
                gain_db: 0.0,
                loop_frames: None,
            },
        ],
    });
    if let Source::Synth(osc) = &mut patch.layers[0].source {
        osc.source = SynthSource::Sample(0);
    }
    let unlocked = render(patch.clone(), 48, 24_000);
    let measured = zero_crossings_per_second(&unlocked[4_000..]);
    assert!(
        (measured - 130.8).abs() < 3.0,
        "unlocked, key 48 plays the low zone: {measured} Hz"
    );

    if let Source::Synth(osc) = &mut patch.layers[0].source {
        osc.sample.zone = Some(1);
    }
    let locked = render(patch.clone(), 48, 24_000);
    let measured = zero_crossings_per_second(&locked[4_000..]);
    assert!(
        (measured - 261.6).abs() < 4.0,
        "locked to the high zone, key 48 plays it two octaves down: {measured} Hz"
    );

    // A lock on a zone the recording has not got is silence, like every
    // other thing a patch names and does not have.
    if let Source::Synth(osc) = &mut patch.layers[0].source {
        osc.sample.zone = Some(9);
    }
    let missing = render(patch, 48, 24_000);
    assert!(
        missing.iter().all(|s| s.abs() < 1e-6),
        "a zone that is not there is silence"
    );
}

/// The zone chooser: its first position is *any* and the rest are the
/// recording's zones in order, so an automation lane can pick a hit.
#[test]
fn the_zone_address_is_a_chooser_over_the_recordings_zones() {
    use fontelle_core::patch_params;
    let mut patch = a_patch_playing_its_own_recording();
    patch.samples[0].zones.push(SampleZone {
        name: "Second".to_string(),
        root_key: 60,
        fine_cents: 0.0,
        key_range: (0, 127),
        sample_rate: SR as u32,
        samples: tone(261.6, 0.1),
        vel_range: (0, 127),
        gain_db: 0.0,
        loop_frames: None,
    });
    let address = "patch/layer[0]/synth/sample/zone";
    // Three positions: any, the first zone, the second.
    assert!(patch_params::set(&mut patch, address, 1.0));
    let Source::Synth(osc) = &patch.layers[0].source else {
        panic!()
    };
    assert_eq!(osc.sample.zone, Some(1));
    assert!((patch_params::value(&patch, address).unwrap() - 1.0).abs() < 1e-6);
    assert!(patch_params::set(&mut patch, address, 0.5));
    let Source::Synth(osc) = &patch.layers[0].source else {
        panic!()
    };
    assert_eq!(osc.sample.zone, Some(0));
    assert!(patch_params::set(&mut patch, address, 0.0));
    let Source::Synth(osc) = &patch.layers[0].source else {
        panic!()
    };
    assert_eq!(osc.sample.zone, None);
}

// --- The spectral read (`docs/flopsynth-next.md` §4.3, phase 4) --------

/// An oscillator reading a recording **spectrally** plays it at the note
/// — the same recording, analysed once (`spectral_analysis` is a cache)
/// and resynthesised through the string's bank — and the kind chooser
/// turns a plain read into a spectral one of the same recording, keeping
/// the zone.
#[test]
fn an_oscillator_reading_a_recording_spectrally_plays_it_at_the_note() {
    use fontelle_core::patch_params::{SOURCE_KINDS, set, value};
    let mut patch = a_patch_playing_its_own_recording();
    // The chooser's last position is Spectral; from a plain read it keeps
    // the recording.
    let spectral = SOURCE_KINDS.iter().position(|k| *k == "Spectral").unwrap() as f32
        / (SOURCE_KINDS.len() - 1) as f32;
    assert!(set(&mut patch, "patch/layer[0]/synth/kind", spectral));
    let Source::Synth(osc) = &patch.layers[0].source else {
        unreachable!()
    };
    assert_eq!(osc.source, SynthSource::Spectral(0));
    assert_eq!(value(&patch, "patch/layer[0]/synth/kind"), Some(spectral));
    let addresses = flopsynth::addresses(&patch);
    assert!(
        addresses
            .iter()
            .any(|a| a == "patch/layer[0]/synth/position")
    );
    assert!(
        addresses
            .iter()
            .any(|a| a == "patch/layer[0]/synth/warp_mode")
    );

    let out = render(patch.clone(), 69, 24_000);
    let peak = out.iter().fold(0.0f32, |a, s| a.max(s.abs()));
    assert!(peak > 0.01, "a spectral read is audible, not {peak}");
    let measured = zero_crossings_per_second(&out[4_800..]);
    assert!(
        (measured - 440.0).abs() < 3.0,
        "at its root key it plays as recorded: {measured} Hz"
    );
    let out = render(patch.clone(), 81, 24_000);
    let measured = zero_crossings_per_second(&out[4_800..]);
    assert!(
        (measured - 880.0).abs() < 6.0,
        "an octave up it plays an octave up: {measured} Hz"
    );
    // The set carries the analysis, once: the same `Arc` twice.
    let mut set_a = WavetableSet::new();
    set_a.resolve(&patch);
    let first = set_a.get_spectral(0, 0).expect("the analysis") as *const _;
    let mut set_b = WavetableSet::new();
    set_b.resolve(&patch);
    let second = set_b.get_spectral(0, 0).expect("the analysis") as *const _;
    assert!(std::ptr::eq(first, second), "analysed once, shared after");
    // And back to a plain read of the same recording.
    let sample = SOURCE_KINDS.iter().position(|k| *k == "Sample").unwrap() as f32
        / (SOURCE_KINDS.len() - 1) as f32;
    assert!(set(&mut patch, "patch/layer[0]/synth/kind", sample));
    let Source::Synth(osc) = &patch.layers[0].source else {
        unreachable!()
    };
    assert_eq!(osc.source, SynthSource::Sample(0));
}
