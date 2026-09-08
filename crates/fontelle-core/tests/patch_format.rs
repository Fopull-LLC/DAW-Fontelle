//! The patch format's version 1 and the migration into it
//! (`docs/flopsynth-plan.md` §5).
//!
//! # Why the v0 fixture is written by hand
//!
//! Because generating one with `to_data` would generate a **v1** body, and a
//! migration test whose input came from the code it is testing measures
//! nothing. The JSON below is what a build before this work actually wrote,
//! typed out — which is the only fixture that can catch the migration being
//! wrong.

use fontelle_core::{
    LfoMode, PATCH_FORMAT_VERSION, Patch, PatchFormatError, SILENT_DB, Source, flopsynth,
};
use fontelle_types::{LfoWave, PatchData};

/// A version 0 body, as a build before Flopsynth wrote one.
///
/// Two oscillator layers, both filters, two envelopes and two LFOs — with the
/// LFOs carrying `shape`, the field that became `wave`, which is the whole
/// reason the version moved.
fn a_v0_body() -> serde_json::Value {
    serde_json::json!({
        "layers": [
            {
                "source": { "Oscillator": "Saw" },
                "key_range": [0, 127],
                "vel_range": [0, 127],
                "root_key": 60,
                "fine_tune_cents": -7.0,
                "playback": {
                    "start_offset": 0.0,
                    "end_offset": 0.0,
                    "loop_mode": "Off",
                    "loop_start": 0.0,
                    "loop_end": 0.0,
                    "loop_crossfade_ms": 0.0,
                    "reverse": false,
                    "interpolation": null
                },
                "gain_db": -14.0,
                "pan": 0.0
            },
            {
                "source": { "Oscillator": "Square" },
                "key_range": [0, 127],
                "vel_range": [0, 127],
                "root_key": 72,
                "fine_tune_cents": 0.0,
                "playback": {
                    "start_offset": 0.0,
                    "end_offset": 0.0,
                    "loop_mode": "Off",
                    "loop_start": 0.0,
                    "loop_end": 0.0,
                    "loop_crossfade_ms": 0.0,
                    "reverse": false,
                    "interpolation": null
                },
                "gain_db": -60.0,
                "pan": 0.25
            }
        ],
        "filters": [
            { "mode": "Lowpass", "cutoff_hz": 2400.0, "resonance": 0.7, "enabled": true },
            { "mode": "Highpass", "cutoff_hz": 20.0, "resonance": 0.7, "enabled": false }
        ],
        "envelopes": [
            {
                "delay_s": 0.0, "attack_s": 0.005, "hold_s": 0.0, "decay_s": 0.0,
                "sustain_level": 1.0, "release_s": 0.12, "curve": "Decibel"
            },
            {
                "delay_s": 0.0, "attack_s": 0.0, "hold_s": 0.0, "decay_s": 0.3,
                "sustain_level": 0.0, "release_s": 0.1, "curve": "Decibel"
            }
        ],
        "lfos": [
            { "rate_hz": 5.2, "depth": 0.4, "shape": "Saw", "delay_s": 0.35 },
            { "rate_hz": 1.0, "depth": 1.0, "shape": "Noise", "delay_s": 0.0 }
        ],
        "mod_matrix": {
            "routes": [
                {
                    "source": { "Lfo": 0 },
                    "destination": { "LayerPitch": 0 },
                    "depth": 0.25,
                    "curve": "Linear",
                    "via": { "ModWheel": null },
                    "invert": false
                }
            ]
        },
        "voice_config": {
            "polyphony": 64,
            "steal_policy": "Oldest",
            "glide_time_s": 0.0,
            "glide_legato_only": false,
            "unison": {
                "voices": 1, "detune_cents": 0.0, "spread": 0.0, "randomise_phase": false
            },
            "retrigger": "Poly",
            "bend_range_semitones": 2.0
        }
    })
}

fn read(data: &PatchData) -> Patch {
    Patch::from_data(data, |_| None)
        .expect("this body has to read")
        .patch
}

#[test]
fn the_format_version_is_one() {
    assert_eq!(PATCH_FORMAT_VERSION, 1);
}

/// The migration's one job: `Lfo::shape: OscKind` became `Lfo::wave: LfoWave`.
#[test]
fn a_version_zero_bodys_lfo_shapes_become_waves() {
    let patch = read(&PatchData {
        format_version: 0,
        body: a_v0_body(),
    });
    assert_eq!(patch.lfos.len(), 2);
    assert_eq!(
        patch.lfos[0].wave,
        LfoWave::SawUp,
        "a v0 `Saw` is a rising ramp, which is `SawUp`"
    );
    // `Noise` has no counterpart but sample & hold: an LFO running a noise
    // oscillator produced a new random value per block, and that is what
    // `SampleHold` is. Naming it anything else would silently change what an
    // existing patch sounds like.
    assert_eq!(patch.lfos[1].wave, LfoWave::SampleHold);
    // Everything the LFO had before is untouched.
    assert!((patch.lfos[0].rate_hz - 5.2).abs() < 1e-6);
    assert!((patch.lfos[0].depth - 0.4).abs() < 1e-6);
    assert!((patch.lfos[0].delay_s - 0.35).abs() < 1e-6);
}

/// The rest of §5's claim: everything else this version added is
/// `#[serde(default)]`, so **the v1 reader with its defaults is the v0
/// reader**. Measured by reading the same content both ways and comparing the
/// two patches, rather than by trusting the sentence.
#[test]
fn a_v1_body_with_none_of_the_new_fields_reads_as_the_v0_body_did() {
    let from_v0 = read(&PatchData {
        format_version: 0,
        body: a_v0_body(),
    });
    // The same body, already migrated, claiming to be v1.
    let mut v1_body = a_v0_body();
    for lfo in v1_body["lfos"].as_array_mut().unwrap() {
        let object = lfo.as_object_mut().unwrap();
        let shape = object.remove("shape").unwrap();
        let wave = match shape.as_str().unwrap() {
            "Saw" => "SawUp",
            "Noise" => "SampleHold",
            other => other,
        };
        object.insert("wave".to_string(), serde_json::Value::from(wave));
    }
    let from_v1 = read(&PatchData {
        format_version: 1,
        body: v1_body,
    });
    assert_eq!(from_v0, from_v1);
}

/// The gate: every patch the tree could write before this work still loads,
/// and round-trips unchanged.
#[test]
fn every_patch_this_build_can_make_round_trips_unchanged() {
    let mut cases: Vec<(String, Patch)> = vec![
        ("basic_synth".to_string(), Patch::basic_synth()),
        ("flopsynth_init".to_string(), flopsynth::flopsynth_init()),
    ];
    for style in fontelle_core::DrumKitStyle::ALL {
        cases.push((
            format!("drum kit {}", style.label()),
            fontelle_core::drum_kit(style),
        ));
    }

    for (name, patch) in cases {
        let data = patch
            .to_data(&Default::default())
            .unwrap_or_else(|e| panic!("{name} would not serialise: {e}"));
        assert_eq!(data.format_version, PATCH_FORMAT_VERSION);
        let back = read(&data);
        assert_eq!(back, patch, "{name} did not survive the round trip");
    }
}

/// A patch with every one of this version's additions in it, which is the
/// case a "defaults absorb everything" migration would silently drop.
#[test]
fn the_new_fields_survive_the_round_trip() {
    let mut patch = flopsynth::flopsynth_init();
    patch.output_db = -4.5;
    patch.macros[0].name = "Brightness".to_string();
    patch.macros[0].value = 0.62;
    patch.macros[2].name = "Space".to_string();
    patch.fx.push(fontelle_core::PatchFx {
        config: fontelle_types::EffectConfig::new(fontelle_types::EffectKind::Reverb),
        enabled: true,
    });
    patch.fx.push(fontelle_core::PatchFx {
        config: fontelle_types::EffectConfig::new(fontelle_types::EffectKind::Delay),
        enabled: false,
    });
    patch.filters[0].drive = 0.4;
    patch.filters[0].model = fontelle_dsp::FilterModel::Ladder;
    patch.filters[0].slope = fontelle_dsp::FilterSlope::Db24;
    patch.filters[0].key_track = 0.5;
    patch.filters[0].character = 0.75;
    patch.envelopes[0].attack_shape = 0.4;
    patch.envelopes[0].decay_shape = -0.6;
    patch.envelopes[0].release_shape = -0.2;
    patch.lfos[0].sync = true;
    patch.lfos[0].division = fontelle_types::NoteDivision::EighthDotted;
    patch.lfos[0].fade_s = 0.4;
    patch.lfos[0].phase = 0.3;
    patch.lfos[0].mode = LfoMode::OneShot;
    patch.lfos[0].smooth = 0.2;
    let Source::Synth(osc) = &mut patch.layers[0].source else {
        unreachable!()
    };
    osc.warp = fontelle_dsp::WarpMode::Mirror;
    osc.warp_amount = 0.55;
    osc.modulator = Some(2);
    osc.unison.voices = 7;
    osc.unison.detune_cents = 23.0;
    osc.random_phase = true;
    osc.semitones = -19;

    let data = patch.to_data(&Default::default()).expect("serialises");
    assert_eq!(read(&data), patch);
}

#[test]
fn a_body_from_the_future_is_refused_as_such_rather_than_as_damage() {
    let result = Patch::from_data(
        &PatchData {
            format_version: PATCH_FORMAT_VERSION + 1,
            body: a_v0_body(),
        },
        |_| None,
    );
    match result {
        Err(PatchFormatError::FromTheFuture { found, newest }) => {
            assert_eq!(found, PATCH_FORMAT_VERSION + 1);
            assert_eq!(newest, PATCH_FORMAT_VERSION);
        }
        other => panic!("expected FromTheFuture, got {other:?}"),
    }
}

/// A `Source::Synth` layer is stored **whole**, like a drum hit — it names no
/// file, so `referenced_samples` has nothing to report for it and a bundle
/// export has nothing to copy.
#[test]
fn a_synth_layer_references_no_files() {
    let data = flopsynth::flopsynth_init()
        .to_data(&Default::default())
        .expect("serialises");
    let referenced = fontelle_core::referenced_samples(&data).expect("reads");
    assert!(
        referenced.is_empty(),
        "a Flopsynth patch names no files and can never need relinking"
    );
}

/// The init patch's own layers, after a round trip, still say what they are —
/// which is what makes `Session::kind_of` answer "Flopsynth" for a reopened
/// project rather than "3OSC".
#[test]
fn a_reopened_flopsynth_patch_is_still_a_flopsynth_patch() {
    let data = flopsynth::flopsynth_init()
        .to_data(&Default::default())
        .expect("serialises");
    let back = read(&data);
    assert!(flopsynth::is_flopsynth(&back));
    assert_eq!(back.layers.len(), 5);
    assert_eq!(back.layers[1].gain_db, SILENT_DB);
}
