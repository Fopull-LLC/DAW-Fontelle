//! Hand-builds real, spec-valid SF2 byte streams (not a downloaded fixture — the
//! whole point is a deterministic, license-free file this test fully controls and
//! can assert exact expected values against) and checks that `import_sf2` decodes
//! them into a correct `fontelle_core::Patch`. This is the test for the TDD's own
//! named risk (§23): "SF2 import defaults subtly wrong."

use std::io::Write;
use std::path::PathBuf;

use fontelle_assets::{import_sf2, import_sf2_preset};
use fontelle_core::{LoopMode, SampleStore, Source};
use fontelle_dsp::EnvelopeCurve;

/// SF2 generator amounts are either a plain `i16`, or (for KeyRange/VelRange only)
/// a `(low, high)` byte pair — see `soundfont::raw::GeneratorAmount`.
enum GenAmount {
    Value(i16),
    Range(u8, u8),
}

struct Gen {
    id: u16,
    amount: GenAmount,
}

fn gen_val(id: u16, amount: i16) -> Gen {
    Gen {
        id,
        amount: GenAmount::Value(amount),
    }
}

fn gen_range(id: u16, low: u8, high: u8) -> Gen {
    Gen {
        id,
        amount: GenAmount::Range(low, high),
    }
}

// GeneratorType ids we need (soundfont::raw::GeneratorType as u16 — mirrored here
// so this fixture builder has no dependency on the parser crate's internals).
const GEN_START_ADDRS_OFFSET: u16 = 0;
const GEN_END_ADDRS_OFFSET: u16 = 1;
const GEN_STARTLOOP_ADDRS_OFFSET: u16 = 2;
const GEN_ENDLOOP_ADDRS_OFFSET: u16 = 3;
const GEN_PAN: u16 = 17;
const GEN_DELAY_VOL_ENV: u16 = 33;
const GEN_ATTACK_VOL_ENV: u16 = 34;
const GEN_HOLD_VOL_ENV: u16 = 35;
const GEN_DECAY_VOL_ENV: u16 = 36;
const GEN_SUSTAIN_VOL_ENV: u16 = 37;
const GEN_RELEASE_VOL_ENV: u16 = 38;
const GEN_KEY_RANGE: u16 = 43;
const GEN_VEL_RANGE: u16 = 44;
const GEN_INITIAL_ATTENUATION: u16 = 48;
const GEN_COARSE_TUNE: u16 = 51;
const GEN_FINE_TUNE: u16 = 52;
const GEN_SAMPLE_MODES: u16 = 54;
const GEN_OVERRIDING_ROOT_KEY: u16 = 58;

fn write_chunk(buf: &mut Vec<u8>, id: &[u8; 4], content: &[u8]) {
    buf.extend_from_slice(id);
    buf.extend_from_slice(&(content.len() as u32).to_le_bytes());
    buf.extend_from_slice(content);
    if content.len() % 2 == 1 {
        buf.push(0);
    }
}

fn write_list(buf: &mut Vec<u8>, list_type: &[u8; 4], build_inner: impl FnOnce(&mut Vec<u8>)) {
    let mut inner = Vec::new();
    inner.extend_from_slice(list_type);
    build_inner(&mut inner);
    write_chunk(buf, b"LIST", &inner);
}

fn zstr(name: &str, field_len: usize) -> Vec<u8> {
    let mut bytes = name.as_bytes().to_vec();
    bytes.resize(field_len, 0);
    bytes
}

fn write_gen(buf: &mut Vec<u8>, g: &Gen) {
    buf.extend_from_slice(&g.id.to_le_bytes());
    match g.amount {
        GenAmount::Value(v) => buf.extend_from_slice(&v.to_le_bytes()),
        GenAmount::Range(low, high) => buf.extend_from_slice(&[low, high]),
    }
}

/// One instrument zone's worth of generators, plus the sample id it must end
/// with (`SampleID` is what makes a zone a "local" zone with a sound, rather
/// than the global/default zone).
struct ZoneSpec {
    generators: Vec<Gen>,
}

struct Sf2Fixture {
    samples: Vec<i16>,
    sample_rate: u32,
    /// Indices into `samples` (matches `SampleHeader::start/end`).
    header_start: u32,
    header_end: u32,
    header_loop_start: u32,
    header_loop_end: u32,
    origpitch: u8,
    pitchadj: i8,
    zone: ZoneSpec,
}

/// Assembles a complete, spec-valid single-preset/single-instrument/single-sample
/// SF2 file. Every chunk size is computed from real content — nothing here is a
/// guessed byte count.
fn build_sf2(fixture: &Sf2Fixture) -> Vec<u8> {
    let mut sfbk = Vec::new();
    sfbk.extend_from_slice(b"sfbk");

    // --- INFO ---
    write_list(&mut sfbk, b"INFO", |buf| {
        write_chunk(buf, b"ifil", &[2, 0, 1, 0]); // version 2.1
        write_chunk(buf, b"isng", b"EMU8000\0");
        write_chunk(buf, b"INAM", b"Fontelle Test Bank\0");
    });

    // --- sdta ---
    write_list(&mut sfbk, b"sdta", |buf| {
        let mut pcm = Vec::with_capacity(fixture.samples.len() * 2);
        for s in &fixture.samples {
            pcm.extend_from_slice(&s.to_le_bytes());
        }
        write_chunk(buf, b"smpl", &pcm);
    });

    // --- pdta ---
    write_list(&mut sfbk, b"pdta", |buf| {
        // phdr: one real preset (bag 0) + the mandatory EOP terminator (bag 1).
        let mut phdr = Vec::new();
        phdr.extend_from_slice(&zstr("TestPreset", 20));
        phdr.extend_from_slice(&0u16.to_le_bytes()); // preset #
        phdr.extend_from_slice(&0u16.to_le_bytes()); // bank
        phdr.extend_from_slice(&0u16.to_le_bytes()); // bag_id
        phdr.extend_from_slice(&0u32.to_le_bytes());
        phdr.extend_from_slice(&0u32.to_le_bytes());
        phdr.extend_from_slice(&0u32.to_le_bytes());
        phdr.extend_from_slice(&zstr("EOP", 20));
        phdr.extend_from_slice(&0u16.to_le_bytes()); // preset #
        phdr.extend_from_slice(&0u16.to_le_bytes()); // bank
        phdr.extend_from_slice(&1u16.to_le_bytes()); // bag_id = 1 (terminator)
        phdr.extend_from_slice(&0u32.to_le_bytes());
        phdr.extend_from_slice(&0u32.to_le_bytes());
        phdr.extend_from_slice(&0u32.to_le_bytes());
        write_chunk(buf, b"phdr", &phdr);

        // pbag: preset zone 0 points at generator 0 (Instrument generator only),
        // plus the terminator entry.
        let mut pbag = Vec::new();
        pbag.extend_from_slice(&0u16.to_le_bytes()); // gen_id
        pbag.extend_from_slice(&0u16.to_le_bytes()); // mod_id
        pbag.extend_from_slice(&1u16.to_le_bytes()); // terminator gen_id
        pbag.extend_from_slice(&0u16.to_le_bytes());
        write_chunk(buf, b"pbag", &pbag);

        // pmod: no modulators, just the terminator record.
        write_chunk(buf, b"pmod", &[0u8; 10]);

        // pgen: the preset zone's only generator is Instrument=0 (point at our
        // one instrument), plus the terminator.
        let mut pgen = Vec::new();
        pgen.extend_from_slice(&41u16.to_le_bytes()); // GeneratorType::Instrument
        pgen.extend_from_slice(&0u16.to_le_bytes()); // instrument index 0
        pgen.extend_from_slice(&[0u8; 4]); // terminator
        write_chunk(buf, b"pgen", &pgen);

        // inst: one real instrument (bag 0) + EOS terminator (bag 1).
        let mut inst = Vec::new();
        inst.extend_from_slice(&zstr("TestInst", 20));
        inst.extend_from_slice(&0u16.to_le_bytes());
        inst.extend_from_slice(&zstr("EOS", 20));
        inst.extend_from_slice(&1u16.to_le_bytes());
        write_chunk(buf, b"inst", &inst);

        // ibag: instrument zone 0 points at generator 0, plus the terminator.
        let mut ibag = Vec::new();
        ibag.extend_from_slice(&0u16.to_le_bytes());
        ibag.extend_from_slice(&0u16.to_le_bytes());
        ibag.extend_from_slice(&(fixture.zone.generators.len() as u16 + 1).to_le_bytes());
        ibag.extend_from_slice(&0u16.to_le_bytes());
        write_chunk(buf, b"ibag", &ibag);

        write_chunk(buf, b"imod", &[0u8; 10]);

        // igen: the zone's generators, ending with SampleID=0, plus the terminator.
        let mut igen = Vec::new();
        for g in &fixture.zone.generators {
            write_gen(&mut igen, g);
        }
        write_gen(&mut igen, &gen_val(53, 0)); // GeneratorType::SampleID -> sample 0
        igen.extend_from_slice(&[0u8; 4]); // terminator
        write_chunk(buf, b"igen", &igen);

        // shdr: one real sample + EOS terminator.
        let mut shdr = Vec::new();
        shdr.extend_from_slice(&zstr("TestSample", 20));
        shdr.extend_from_slice(&fixture.header_start.to_le_bytes());
        shdr.extend_from_slice(&fixture.header_end.to_le_bytes());
        shdr.extend_from_slice(&fixture.header_loop_start.to_le_bytes());
        shdr.extend_from_slice(&fixture.header_loop_end.to_le_bytes());
        shdr.extend_from_slice(&fixture.sample_rate.to_le_bytes());
        shdr.push(fixture.origpitch);
        shdr.push(fixture.pitchadj as u8);
        shdr.extend_from_slice(&0u16.to_le_bytes()); // sample_link
        shdr.extend_from_slice(&1u16.to_le_bytes()); // MonoSample
        shdr.extend_from_slice(&zstr("EOS", 20));
        shdr.extend_from_slice(&[0u8; 26]);
        write_chunk(buf, b"shdr", &shdr);
    });

    let mut riff = Vec::new();
    write_chunk(&mut riff, b"RIFF", &sfbk);
    riff
}

fn write_fixture_to_temp_file(name: &str, bytes: &[u8]) -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!("fontelle-test-{name}-{}.sf2", std::process::id()));
    let mut f = std::fs::File::create(&path).expect("write test fixture");
    f.write_all(bytes).expect("write test fixture bytes");
    path
}

#[test]
fn imports_key_vel_range_root_key_and_tuning() {
    let fixture = Sf2Fixture {
        samples: vec![0, 4096, 8192, 12288, 16384, 20480, 24576, 28672],
        sample_rate: 44_100,
        header_start: 0,
        header_end: 8,
        header_loop_start: 2,
        header_loop_end: 6,
        origpitch: 69, // A4 -- must be overridden by the generator below
        pitchadj: 0,
        zone: ZoneSpec {
            generators: vec![
                gen_range(GEN_KEY_RANGE, 48, 72),
                gen_range(GEN_VEL_RANGE, 1, 127),
                gen_val(GEN_OVERRIDING_ROOT_KEY, 60),
                gen_val(GEN_COARSE_TUNE, 1),  // +100 cents
                gen_val(GEN_FINE_TUNE, 25),   // +25 cents -> 125 total
                gen_val(GEN_SAMPLE_MODES, 0), // no loop
            ],
        },
    };
    let path = write_fixture_to_temp_file("basic", &build_sf2(&fixture));

    let mut store = SampleStore::new();
    let patch = import_sf2(&path, &mut store).expect("valid fixture must import");
    std::fs::remove_file(&path).ok();

    assert_eq!(patch.layers.len(), 1);
    let layer = &patch.layers[0];
    assert_eq!(layer.key_range, (48, 72));
    assert_eq!(layer.vel_range, (1, 127));
    assert_eq!(
        layer.root_key, 60,
        "OverridingRootKey must win over the sample header's origpitch (69)"
    );
    assert_eq!(
        layer.fine_tune_cents, 125.0,
        "CoarseTune(1)*100 + FineTune(25) = 125 cents"
    );
    assert_eq!(layer.playback.loop_mode, LoopMode::Off);

    let Source::Sample { file } = layer.source else {
        panic!("expected a Sample source");
    };
    let buffer = store
        .get(file)
        .expect("decoded sample must be in the store");
    assert_eq!(buffer.sample_rate, 44_100);
    assert_eq!(
        buffer.data.len(),
        8,
        "buffer must span exactly [header.start, header.end)"
    );
    let expected: Vec<f32> = fixture
        .samples
        .iter()
        .map(|s| *s as f32 / 32768.0)
        .collect();
    for (got, want) in buffer.data.iter().zip(expected.iter()) {
        assert!((got - want).abs() < 1e-6, "got {got}, want {want}");
    }
}

#[test]
fn note_outside_the_fixtures_key_range_would_be_silent() {
    // Regression guard for the key-range plumbing specifically, independent of
    // the render path (which is tested in fontelle-core): a zone scoped to
    // 48..72 must not claim to cover note 90.
    let fixture = Sf2Fixture {
        samples: vec![0, 1, 2, 3],
        sample_rate: 8_000,
        header_start: 0,
        header_end: 4,
        header_loop_start: 0,
        header_loop_end: 4,
        origpitch: 60,
        pitchadj: 0,
        zone: ZoneSpec {
            generators: vec![
                gen_range(GEN_KEY_RANGE, 48, 72),
                gen_range(GEN_VEL_RANGE, 0, 127),
            ],
        },
    };
    let path = write_fixture_to_temp_file("range", &build_sf2(&fixture));

    let mut store = SampleStore::new();
    let patch = import_sf2(&path, &mut store).unwrap();
    std::fs::remove_file(&path).ok();

    let layer = &patch.layers[0];
    assert!(!(layer.key_range.0..=layer.key_range.1).contains(&90));
}

#[test]
fn imports_loop_points_gain_pan_and_volume_envelope() {
    // Round-trip every conversion with the *inverse* of the formula the
    // importer must use, rather than a hand-picked magic number — this fails
    // if the importer's math is wrong in either direction, not just if it
    // forgot the field entirely.
    let attack_s = 0.01_f64;
    let decay_s = 0.02_f64;
    let release_s = 0.03_f64;
    let to_timecents = |s: f64| (1200.0 * s.log2()).round() as i16;

    let attenuation_centibels: i16 = 100; // 10dB attenuation -> gain_db = -10.0
    let pan_amount: i16 = 250; // spec range -500..=500 -> our -1.0..=1.0
    let sustain_centibels: i16 = 200; // -> sustain_level = 10^(-200/200) = 0.1

    let fixture = Sf2Fixture {
        samples: vec![100, 200, 300, 400, 500, 600, 700, 800, 900, 1000],
        sample_rate: 22_050,
        header_start: 0,
        header_end: 10,
        header_loop_start: 3,
        header_loop_end: 9,
        origpitch: 60,
        pitchadj: 0,
        zone: ZoneSpec {
            generators: vec![
                gen_range(GEN_KEY_RANGE, 0, 127),
                gen_range(GEN_VEL_RANGE, 0, 127),
                gen_val(GEN_SAMPLE_MODES, 1), // continuous loop
                gen_val(GEN_INITIAL_ATTENUATION, attenuation_centibels),
                gen_val(GEN_PAN, pan_amount),
                gen_val(GEN_DELAY_VOL_ENV, -32768), // spec sentinel for "none"
                gen_val(GEN_ATTACK_VOL_ENV, to_timecents(attack_s)),
                gen_val(GEN_HOLD_VOL_ENV, -32768),
                gen_val(GEN_DECAY_VOL_ENV, to_timecents(decay_s)),
                gen_val(GEN_SUSTAIN_VOL_ENV, sustain_centibels),
                gen_val(GEN_RELEASE_VOL_ENV, to_timecents(release_s)),
                gen_val(GEN_STARTLOOP_ADDRS_OFFSET, 1),
                gen_val(GEN_ENDLOOP_ADDRS_OFFSET, -1),
                gen_val(GEN_START_ADDRS_OFFSET, 2),
                gen_val(GEN_END_ADDRS_OFFSET, 0),
            ],
        },
    };
    let path = write_fixture_to_temp_file("env", &build_sf2(&fixture));

    let mut store = SampleStore::new();
    let patch = import_sf2(&path, &mut store).unwrap();
    std::fs::remove_file(&path).ok();

    let layer = &patch.layers[0];
    assert_eq!(layer.playback.loop_mode, LoopMode::Forward);
    assert_eq!(
        layer.playback.loop_start,
        3.0 + 1.0,
        "header loop_start (3) + StartloopAddrsOffset (1)"
    );
    assert_eq!(
        layer.playback.loop_end,
        9.0 - 1.0,
        "header loop_end (9) + EndloopAddrsOffset (-1)"
    );
    assert_eq!(layer.playback.start_offset, 2.0);

    assert!(
        (layer.gain_db - (-10.0)).abs() < 1e-4,
        "100 centibels attenuation -> -10dB, got {}",
        layer.gain_db
    );
    assert!(
        (layer.pan - 0.5).abs() < 1e-4,
        "pan amount 250/500 -> 0.5, got {}",
        layer.pan
    );

    let amp = &patch.envelopes[0];
    assert!(
        (amp.attack_s - attack_s as f32).abs() < 1e-3,
        "attack: got {}, want {attack_s}",
        amp.attack_s
    );
    assert!(
        (amp.decay_s - decay_s as f32).abs() < 1e-3,
        "decay: got {}, want {decay_s}",
        amp.decay_s
    );
    assert!(
        (amp.release_s - release_s as f32).abs() < 1e-3,
        "release: got {}, want {release_s}",
        amp.release_s
    );
    let expected_sustain = 10f32.powf(-(sustain_centibels as f32) / 200.0);
    assert!(
        (amp.sustain_level - expected_sustain).abs() < 1e-4,
        "sustain: got {}, want {expected_sustain}",
        amp.sustain_level
    );
    // The stage times above are only meaningful alongside the curve they are
    // defined against: SF2 writes volume-envelope decay and release as a
    // constant dB rate over a 100 dB span, so importing the numbers but
    // playing them back as linear amplitude ramps stretches every decay by
    // more than an order of magnitude. The modulation envelope keeps the
    // linear curve, which is what SF2 defines for it.
    assert_eq!(
        amp.curve,
        EnvelopeCurve::Decibel,
        "the volume envelope must import on the SF2 decibel curve"
    );
    assert_eq!(
        patch.envelopes[1].curve,
        EnvelopeCurve::Linear,
        "the modulation envelope is linear in SF2, not in dB"
    );
}

/// TDD §20.3: a malformed SF2 must "either import correctly or fail with a
/// clear message. Neither crashing nor silent misbehaviour is acceptable."
/// A sample header whose `end` runs past the actual `smpl` chunk is a
/// realistic form of corruption (truncated download, bad authoring tool), and
/// it must not take the process down.
#[test]
fn a_sample_header_pointing_past_the_end_of_the_pcm_data_fails_cleanly() {
    let fixture = Sf2Fixture {
        samples: vec![100, 200, 300, 400],
        sample_rate: 44_100,
        header_start: 0,
        // The file only holds 4 samples; claim 5000. Nothing in the parser
        // cross-checks this against the smpl chunk's real length.
        header_end: 5_000,
        header_loop_start: 0,
        header_loop_end: 4,
        origpitch: 60,
        pitchadj: 0,
        zone: ZoneSpec {
            generators: vec![
                gen_range(GEN_KEY_RANGE, 0, 127),
                gen_range(GEN_VEL_RANGE, 0, 127),
            ],
        },
    };
    let path = write_fixture_to_temp_file("truncated-pcm", &build_sf2(&fixture));

    let mut store = SampleStore::new();
    let result = import_sf2(&path, &mut store);
    std::fs::remove_file(&path).ok();

    assert!(
        result.is_err(),
        "a sample header running past the smpl chunk must produce an ImportError, \
         not an out-of-bounds panic"
    );
}

/// Builds an SF2 with several presets, each with its own instrument and its
/// own single-zone sample, so preset *selection* can be tested rather than
/// just preset *parsing*. Mirrors `build_sf2`'s structure; kept separate so
/// the single-preset fixture above stays easy to read.
fn build_multi_preset_sf2(presets: &[(&str, u16, u16, i16)]) -> Vec<u8> {
    let n = presets.len();
    let mut sfbk = Vec::new();
    sfbk.extend_from_slice(b"sfbk");

    write_list(&mut sfbk, b"INFO", |buf| {
        write_chunk(buf, b"ifil", &[2, 0, 1, 0]);
        write_chunk(buf, b"isng", b"EMU8000\0");
        write_chunk(buf, b"INAM", b"Fontelle Multi Bank\0");
    });

    // Each sample is 4 frames long, distinguishable by its constant value.
    write_list(&mut sfbk, b"sdta", |buf| {
        let mut pcm = Vec::new();
        for (i, _) in presets.iter().enumerate() {
            for _ in 0..4 {
                pcm.extend_from_slice(&(((i as i16) + 1) * 1000).to_le_bytes());
            }
        }
        write_chunk(buf, b"smpl", &pcm);
    });

    write_list(&mut sfbk, b"pdta", |buf| {
        let mut phdr = Vec::new();
        for (i, (name, program, bank, _)) in presets.iter().enumerate() {
            phdr.extend_from_slice(&zstr(name, 20));
            phdr.extend_from_slice(&program.to_le_bytes());
            phdr.extend_from_slice(&bank.to_le_bytes());
            phdr.extend_from_slice(&(i as u16).to_le_bytes()); // bag_id
            phdr.extend_from_slice(&[0u8; 12]);
        }
        phdr.extend_from_slice(&zstr("EOP", 20));
        phdr.extend_from_slice(&0u16.to_le_bytes());
        phdr.extend_from_slice(&0u16.to_le_bytes());
        phdr.extend_from_slice(&(n as u16).to_le_bytes());
        phdr.extend_from_slice(&[0u8; 12]);
        write_chunk(buf, b"phdr", &phdr);

        // One preset zone each, whose single generator is Instrument=i.
        let mut pbag = Vec::new();
        for i in 0..=n {
            pbag.extend_from_slice(&(i as u16).to_le_bytes()); // gen_id
            pbag.extend_from_slice(&0u16.to_le_bytes()); // mod_id
        }
        write_chunk(buf, b"pbag", &pbag);
        write_chunk(buf, b"pmod", &[0u8; 10]);

        let mut pgen = Vec::new();
        for i in 0..n {
            write_gen(&mut pgen, &gen_val(41, i as i16)); // Instrument -> i
        }
        pgen.extend_from_slice(&[0u8; 4]);
        write_chunk(buf, b"pgen", &pgen);

        let mut inst = Vec::new();
        for (i, (name, ..)) in presets.iter().enumerate() {
            inst.extend_from_slice(&zstr(&format!("{name} inst"), 20));
            inst.extend_from_slice(&(i as u16).to_le_bytes());
        }
        inst.extend_from_slice(&zstr("EOS", 20));
        inst.extend_from_slice(&(n as u16).to_le_bytes());
        write_chunk(buf, b"inst", &inst);

        // Each instrument zone carries two generators: root-key override then
        // SampleID, so `ibag` advances by 2 per instrument.
        let mut ibag = Vec::new();
        for i in 0..=n {
            ibag.extend_from_slice(&((i * 2) as u16).to_le_bytes());
            ibag.extend_from_slice(&0u16.to_le_bytes());
        }
        write_chunk(buf, b"ibag", &ibag);
        write_chunk(buf, b"imod", &[0u8; 10]);

        let mut igen = Vec::new();
        for (i, (_, _, _, root)) in presets.iter().enumerate() {
            write_gen(&mut igen, &gen_val(GEN_OVERRIDING_ROOT_KEY, *root));
            write_gen(&mut igen, &gen_val(53, i as i16)); // SampleID -> i
        }
        igen.extend_from_slice(&[0u8; 4]);
        write_chunk(buf, b"igen", &igen);

        let mut shdr = Vec::new();
        for (i, (name, ..)) in presets.iter().enumerate() {
            let start = (i * 4) as u32;
            shdr.extend_from_slice(&zstr(&format!("{name} smp"), 20));
            shdr.extend_from_slice(&start.to_le_bytes());
            shdr.extend_from_slice(&(start + 4).to_le_bytes());
            shdr.extend_from_slice(&start.to_le_bytes());
            shdr.extend_from_slice(&(start + 4).to_le_bytes());
            shdr.extend_from_slice(&(8_000u32 + i as u32).to_le_bytes());
            shdr.push(60);
            shdr.push(0);
            shdr.extend_from_slice(&0u16.to_le_bytes());
            shdr.extend_from_slice(&1u16.to_le_bytes());
        }
        shdr.extend_from_slice(&zstr("EOS", 20));
        shdr.extend_from_slice(&[0u8; 26]);
        write_chunk(buf, b"shdr", &shdr);
    });

    let mut riff = Vec::new();
    write_chunk(&mut riff, b"RIFF", &sfbk);
    riff
}

/// The presets in the fixture below are deliberately *not* in program order,
/// the way real banks often aren't — `Secret_of_Mana.sf2` opens with a whale
/// sound effect and keeps its piano at index 27.
const MULTI: &[(&str, u16, u16, i16)] = &[
    ("Orca", 123, 0, 55),
    ("Bells", 14, 0, 60),
    ("Piano", 0, 0, 69),
];

#[test]
fn list_presets_reports_every_preset_in_file_order_without_the_terminator() {
    let path = write_fixture_to_temp_file("multi-list", &build_multi_preset_sf2(MULTI));
    let listed = fontelle_assets::list_presets(&path).expect("fixture must parse");
    std::fs::remove_file(&path).ok();

    assert_eq!(
        listed.len(),
        3,
        "the mandatory EOP terminator record must not be reported as a preset"
    );
    let names: Vec<&str> = listed.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, vec!["Orca", "Bells", "Piano"]);
    assert_eq!(listed[0].index, 0);
    assert_eq!(listed[2].index, 2);
    assert_eq!(listed[0].program, 123, "program order != file order");
    assert_eq!(listed[2].program, 0);
}

#[test]
fn import_sf2_preset_selects_the_requested_preset_not_the_first() {
    let path = write_fixture_to_temp_file("multi-pick", &build_multi_preset_sf2(MULTI));

    let mut store = SampleStore::new();
    let piano = import_sf2_preset(&path, 2, &mut store).expect("preset 2 must import");
    let mut store0 = SampleStore::new();
    let orca = import_sf2_preset(&path, 0, &mut store0).expect("preset 0 must import");
    std::fs::remove_file(&path).ok();

    // Each fixture preset has a distinct root key, so this proves selection
    // reached a different instrument rather than re-importing preset 0.
    assert_eq!(piano.layers[0].root_key, 69, "preset 2 is the 'Piano' zone");
    assert_eq!(orca.layers[0].root_key, 55, "preset 0 is the 'Orca' zone");

    let Source::Sample { file } = piano.layers[0].source else {
        panic!("expected a Sample source");
    };
    let buffer = store.get(file).unwrap();
    assert_eq!(
        buffer.sample_rate, 8_002,
        "preset 2 must decode its own sample, not preset 0's"
    );
}

#[test]
fn import_sf2_defaults_to_preset_zero() {
    let path = write_fixture_to_temp_file("multi-default", &build_multi_preset_sf2(MULTI));
    let mut a = SampleStore::new();
    let mut b = SampleStore::new();
    let default = import_sf2(&path, &mut a).unwrap();
    let explicit = import_sf2_preset(&path, 0, &mut b).unwrap();
    std::fs::remove_file(&path).ok();

    assert_eq!(default.layers[0].root_key, explicit.layers[0].root_key);
}

#[test]
fn an_out_of_range_preset_index_fails_with_a_message_naming_the_real_count() {
    let path = write_fixture_to_temp_file("multi-oob", &build_multi_preset_sf2(MULTI));
    let mut store = SampleStore::new();
    let err = import_sf2_preset(&path, 99, &mut store).expect_err("99 is out of range");
    std::fs::remove_file(&path).ok();

    let text = err.to_string();
    assert!(
        text.contains("99"),
        "message should name the bad index: {text}"
    );
    assert!(
        text.contains('3'),
        "message should name the real preset count: {text}"
    );
}

#[test]
fn rejects_a_file_that_is_not_a_valid_sf2() {
    let path = write_fixture_to_temp_file("garbage", b"not a soundfont");
    let mut store = SampleStore::new();
    let result = import_sf2(&path, &mut store);
    std::fs::remove_file(&path).ok();
    assert!(
        result.is_err(),
        "a non-SF2 file must fail to import, not panic or silently succeed"
    );
}
