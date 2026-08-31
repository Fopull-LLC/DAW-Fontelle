//! Which keys the instrument on a channel can actually play, and what each one
//! is called.
//!
//! Reported from using the window: *"often I will use drum soundfonts however
//! the piano roll shows these the same as normal ones, making it so that you
//! have no idea which notes actually play anything ... it's so much easier
//! instead of trial and error trying to figure out which notes actually play
//! something for these drum soundfonts that only have a few sounds every few
//! notes."*
//!
//! Both halves of that are already in the patch and neither was ever shown.
//! `Layer::key_range` says exactly which keys sound — `Voice::trigger_note`
//! has always tested against it, so a note outside every layer's range starts
//! a voice with no layers active and is silent. And since the importer began
//! keeping sample names, the layer on key 38 knows it is the snare.
//!
//! This is the step that turns those two into something a canvas can draw. It
//! lives in `fontelle-app` for the reason `instrument::describe` does: the UI
//! may not see a `Patch` (INVARIANT 2), and this is the one layer that sees
//! both it and the sample library.
//!
//! **The rule worth arguing about is the naming one**, and it is the reason a
//! melodic soundfont is not covered in labels: a name is shown only where the
//! zone carrying it is narrow enough that it describes *that key*. A kit
//! writes one zone per hit; a sampled piano writes zones spanning registers,
//! and "Piano C3" stamped down twelve rows is noise, not information.

use std::sync::Arc;

use fontelle_app::{SampleLibrary, key_map};
use fontelle_core::{
    FilterSlot, Layer, LoopMode, ModMatrix, Patch, PlaybackConfig, SampleBuffer, Source,
    VoiceConfig,
};
use fontelle_dsp::{EnvelopeConfig, EnvelopeCurve, Interpolation, SvfMode};
use fontelle_ui::document::KeyMap;

fn disabled() -> FilterSlot {
    FilterSlot {
        mode: SvfMode::Lowpass,
        cutoff_hz: 20_000.0,
        resonance: 0.0,
        enabled: false,
    }
}

fn instant() -> EnvelopeConfig {
    EnvelopeConfig {
        delay_s: 0.0,
        attack_s: 0.0,
        hold_s: 0.0,
        decay_s: 0.0,
        sustain_level: 1.0,
        release_s: 0.001,
        curve: EnvelopeCurve::Linear,
    }
}

/// A patch whose layers are `(sample name, key range)` — everything else is
/// the same for all of them, because nothing else is what this reads.
fn patch_of(library: &mut SampleLibrary, layers: &[(&str, (u8, u8))]) -> Patch {
    Patch {
        layers: layers
            .iter()
            .map(|(name, key_range)| {
                let asset = library.insert_synthetic(
                    name,
                    SampleBuffer {
                        data: Arc::from(vec![1.0; 16]),
                        sample_rate: 44_100,
                    },
                );
                Layer {
                    source: Source::Sample { file: asset },
                    key_range: *key_range,
                    vel_range: (0, 127),
                    root_key: key_range.0,
                    fine_tune_cents: 0.0,
                    playback: PlaybackConfig {
                        loop_mode: LoopMode::Off,
                        interpolation: Some(Interpolation::Draft),
                        end_offset: 16.0,
                        ..PlaybackConfig::default()
                    },
                    gain_db: 0.0,
                    pan: 0.0,
                }
            })
            .collect(),
        filters: [disabled(), disabled()],
        envelopes: vec![instant(), instant()],
        lfos: Vec::new(),
        mod_matrix: ModMatrix::default(),
        voice_config: VoiceConfig::default(),
    }
}

/// A kit with the gaps a real one has: nothing on 37, 39, 40, 41.
const KIT: &[(&str, (u8, u8))] = &[
    ("Kick", (36, 36)),
    ("Snare", (38, 38)),
    ("Closed Hat", (42, 42)),
    ("Open Hat", (46, 46)),
];

#[test]
fn a_key_no_layer_covers_cannot_be_played() {
    let mut library = SampleLibrary::new();
    let patch = patch_of(&mut library, &[("Strings", (48, 72))]);
    let map = key_map(&patch, &library);

    assert!(
        map.is_known(),
        "there is an instrument, so the answer is known"
    );
    for key in 48..=72 {
        assert!(map.plays(key), "key {key} is inside the layer's range");
    }
    for key in [0, 47, 73, 127] {
        assert!(
            !map.plays(key),
            "key {key} is outside every layer — the roll must grey it"
        );
    }
}

#[test]
fn a_kits_hits_are_named_by_the_samples_on_them() {
    let mut library = SampleLibrary::new();
    let patch = patch_of(&mut library, KIT);
    let map = key_map(&patch, &library);

    for (name, (key, _)) in KIT {
        assert!(map.plays(*key), "{name} is on key {key}");
        assert_eq!(
            map.name(*key),
            Some(*name),
            "key {key} has to say it is the {name}"
        );
    }
    for key in [37, 39, 40, 41] {
        assert!(!map.plays(key), "key {key} is one of the kit's gaps");
        assert_eq!(map.name(key), None, "and a gap is not called anything");
    }
    assert!(
        map.is_named(),
        "a kit is a named map — the roll widens its keyboard for one"
    );
}

/// The other half of the rule, and the one that keeps a normal session
/// looking normal. A sampled instrument's zones span registers, and its
/// sample names describe those registers rather than single keys. Stamping
/// one down every row of a two-octave zone is noise.
#[test]
fn a_pitched_multisample_is_playable_everywhere_and_named_nowhere() {
    let mut library = SampleLibrary::new();
    let patch = patch_of(
        &mut library,
        &[
            ("Piano Lo", (21, 47)),
            ("Piano Mid", (48, 71)),
            ("Piano Hi", (72, 108)),
        ],
    );
    let map = key_map(&patch, &library);

    assert!(map.plays(60));
    assert_eq!(map.name(60), None, "a register's name is not a key's name");
    assert_eq!(map.name(21), None, "not even at the edge of a zone");
    assert!(
        !map.is_named(),
        "so the roll leaves its keyboard the width it always was"
    );
    assert!(!map.plays(20), "the range still ends where the samples do");
}

/// A channel with nothing on it yet. Greying all 128 rows because no
/// instrument has been chosen would say "this font plays nothing", which is a
/// different and wrong statement.
#[test]
fn a_channel_with_no_instrument_greys_nothing() {
    let map = KeyMap::unknown();

    assert!(!map.is_known());
    assert!(!map.is_named());
    for key in [0, 60, 127] {
        assert!(map.plays(key), "nothing is known, so nothing is greyed out");
        assert_eq!(map.name(key), None);
    }
}

/// **One hit is enough to read a patch as a key map**, and that number is
/// measured rather than chosen. Across the 606 melodic presets in the
/// soundfont bank on the machine this was written on, exactly two contain a
/// one-key zone at all — and both of those contain five, so they are caught
/// either way. Of the 11 percussion presets, all 11 contain at least one and
/// only 10 contain two. Requiring two would therefore have cost a real kit
/// (`Z3 Percussion`: one hit and one eleven-key band) and bought nothing.
///
/// A hit two keys wide is still a hit — some kits give a sample a key of room
/// either side — and once the patch is a key map its wider zones are named
/// too, because in a kit a stretched zone is a percussion sound and not a
/// register.
#[test]
fn one_hit_is_enough_to_read_a_patch_as_a_key_map() {
    let mut library = SampleLibrary::new();
    let patch = patch_of(
        &mut library,
        &[("Wide Hit", (60, 61)), ("Tom Band", (70, 72))],
    );
    let map = key_map(&patch, &library);

    assert_eq!(map.name(60), Some("Wide Hit"));
    assert_eq!(map.name(61), Some("Wide Hit"));
    assert_eq!(
        map.name(70),
        Some("Tom Band"),
        "one hit makes this a kit, and a kit's bands are named"
    );
    assert!(map.plays(70));
}

/// Measured against the soundfonts on the machine this was written on, which
/// is the only way this rule could have been got right.
///
/// A real kit is **not** all one-key zones. `SOM Percussion` is (eleven zones,
/// all one key). But `FZ Percussion` is five one-key hits plus a single
/// thirteen-key zone, `SMW Percussion` four plus an eight and a twenty-eight,
/// and `MMX Percussion` three plus a three, a thirteen, a fourteen and two
/// seventeens. Those wide zones are one percussion sample stretched across a
/// band of keys, and under a zone-by-zone rule they came out unnamed — so
/// F-Zero labelled 5 of its 18 playable keys and Mega Man X 3 of 67, which is
/// most of the trial and error the whole feature exists to remove.
///
/// So the question "is this name a key's name?" is asked of the **patch**, not
/// of the zone alone: a patch with two or more one-key zones is a key map, and
/// in a key map every covered key takes the name of the zone covering it. The
/// melodic case is untouched because it never qualifies — `STR_Ensemble`'s
/// string multisample has zones of 3, 4, 5, 6, 7, 31 and 37 keys and not one
/// narrow enough to count.
#[test]
fn a_kit_names_its_stretched_zones_too_not_only_its_single_key_hits() {
    let mut library = SampleLibrary::new();
    // F-Zero's percussion, in shape: five hits and one stretched band.
    let patch = patch_of(
        &mut library,
        &[
            ("kick", (36, 36)),
            ("sticks", (37, 37)),
            ("snare", (38, 38)),
            ("hihat", (42, 42)),
            ("hihat2", (44, 44)),
            ("toms", (48, 60)),
        ],
    );
    let map = key_map(&patch, &library);

    assert_eq!(map.name(36), Some("kick"), "the hits are still named");
    for key in 48..=60 {
        assert_eq!(
            map.name(key),
            Some("toms"),
            "key {key} is in the kit's stretched band and has to say what it is"
        );
    }
    assert!(!map.plays(61), "and the band ends where the zone does");
}

/// The counterweight, and the reason the test above is about the patch rather
/// than about being generous with widths. A melodic multisample's zones are
/// three to seven keys wide — exactly the range a looser per-zone rule would
/// have swept up — and none of them is narrow enough to make the patch a key
/// map, so it stays unlabelled.
#[test]
fn a_multisample_with_real_world_zone_widths_is_still_named_nowhere() {
    let mut library = SampleLibrary::new();
    // `STR_Ensemble.sf2`'s string ensemble, in shape.
    let patch = patch_of(
        &mut library,
        &[
            ("str lo", (24, 54)),
            ("str a", (55, 59)),
            ("str b", (60, 63)),
            ("str c", (64, 70)),
            ("str d", (71, 76)),
            ("str hi", (77, 113)),
        ],
    );
    let map = key_map(&patch, &library);

    assert!(!map.is_named(), "a string ensemble is not a drum kit");
    for key in [24, 55, 60, 64, 71, 77] {
        assert_eq!(map.name(key), None, "key {key} is one step of a range");
    }
}

/// Even inside a key map, a zone spanning half the keyboard is an instrument
/// stretched across it, not a percussion band — and its name on every row is
/// forty rows saying the same uninformative thing.
///
/// Measured, like the rest of this rule. Across the bank this was developed
/// against, the widest band in any real percussion preset is 28 keys
/// (`SMW Percussion`); the two melodic presets that contain a one-key zone at
/// all (`MP_NES_Composer.sf2`'s "dj TW" and "tuhh SM NES") carry zones of 48,
/// 56 and 59. The cut falls cleanly between them.
#[test]
fn a_zone_spanning_half_the_keyboard_is_not_a_hit_even_in_a_kit() {
    let mut library = SampleLibrary::new();
    // "dj TW"'s shape: a few short zones and two enormous ones.
    let patch = patch_of(
        &mut library,
        &[
            ("blip", (60, 60)),
            ("blop", (61, 62)),
            ("base UF", (0, 47)),
            ("pad", (63, 118)),
        ],
    );
    let map = key_map(&patch, &library);

    assert_eq!(
        map.name(60),
        Some("blip"),
        "the short zones are still named"
    );
    assert_eq!(map.name(61), Some("blop"));
    assert_eq!(
        map.name(20),
        None,
        "a 48-key zone's name describes the instrument, not key 20"
    );
    assert_eq!(map.name(100), None, "nor key 100");
    assert!(
        map.plays(20) && map.plays(100),
        "and both are still perfectly playable — this is about the label only"
    );
}

/// The other side of the same cut: a kit's stretched bands are well inside it
/// and keep their names.
#[test]
fn a_kits_widest_real_band_is_still_named() {
    let mut library = SampleLibrary::new();
    // `SMW Percussion`'s shape: four hits, an eight and a twenty-eight.
    let patch = patch_of(
        &mut library,
        &[
            ("kick", (36, 36)),
            ("snare2", (37, 37)),
            ("hisnare", (38, 38)),
            ("snare", (40, 40)),
            ("hihat", (41, 48)),
            ("toms", (49, 76)),
        ],
    );
    let map = key_map(&patch, &library);

    assert_eq!(map.name(48), Some("hihat"));
    assert_eq!(map.name(76), Some("toms"), "28 keys is a band, not a range");
}
