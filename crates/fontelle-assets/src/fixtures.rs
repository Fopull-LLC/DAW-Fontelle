//! Hand-built, spec-valid SF2 byte streams for tests.
//!
//! Deterministic, licence-free, and fully under the test's control, so a test
//! can assert *exact* expected values — including by round-tripping the
//! timecent and centibel maths through its own inverse, which is what catches
//! a sign error rather than merely a missing field. This is the fixture for
//! TDD §23's own named risk, "SF2 import defaults subtly wrong".
//!
//! It lives in the library rather than in `tests/` because two crates' test
//! suites need it — `fontelle-assets` for what import produces, and
//! `fontelle-app` for what a saved project reopens to — and an integration
//! test cannot enable a feature on the crate it is testing. It is small,
//! contains no data, and is never called by anything that ships.

use std::io::Write;
use std::path::PathBuf;

/// SF2 generator amounts are either a plain `i16`, or (for KeyRange/VelRange only)
/// a `(low, high)` byte pair — see `soundfont::raw::GeneratorAmount`.
pub enum GenAmount {
    Value(i16),
    Range(u8, u8),
}

pub struct Gen {
    pub id: u16,
    pub amount: GenAmount,
}

pub fn gen_val(id: u16, amount: i16) -> Gen {
    Gen {
        id,
        amount: GenAmount::Value(amount),
    }
}

pub fn gen_range(id: u16, low: u8, high: u8) -> Gen {
    Gen {
        id,
        amount: GenAmount::Range(low, high),
    }
}

// GeneratorType ids we need (soundfont::raw::GeneratorType as u16 — mirrored here
// so this fixture builder has no dependency on the parser crate's internals).
pub const GEN_START_ADDRS_OFFSET: u16 = 0;
pub const GEN_END_ADDRS_OFFSET: u16 = 1;
pub const GEN_STARTLOOP_ADDRS_OFFSET: u16 = 2;
pub const GEN_ENDLOOP_ADDRS_OFFSET: u16 = 3;
pub const GEN_MOD_LFO_TO_PITCH: u16 = 5;
pub const GEN_VIB_LFO_TO_PITCH: u16 = 6;
pub const GEN_MOD_ENV_TO_PITCH: u16 = 7;
pub const GEN_INITIAL_FILTER_FC: u16 = 8;
pub const GEN_INITIAL_FILTER_Q: u16 = 9;
pub const GEN_MOD_LFO_TO_FILTER_FC: u16 = 10;
pub const GEN_MOD_ENV_TO_FILTER_FC: u16 = 11;
pub const GEN_MOD_LFO_TO_VOLUME: u16 = 13;
pub const GEN_PAN: u16 = 17;
pub const GEN_DELAY_MOD_LFO: u16 = 21;
pub const GEN_FREQ_MOD_LFO: u16 = 22;
pub const GEN_DELAY_VIB_LFO: u16 = 23;
pub const GEN_FREQ_VIB_LFO: u16 = 24;
pub const GEN_ATTACK_MOD_ENV: u16 = 26;
pub const GEN_DECAY_MOD_ENV: u16 = 28;
pub const GEN_SUSTAIN_MOD_ENV: u16 = 29;
pub const GEN_RELEASE_MOD_ENV: u16 = 30;
pub const GEN_DELAY_VOL_ENV: u16 = 33;
pub const GEN_ATTACK_VOL_ENV: u16 = 34;
pub const GEN_HOLD_VOL_ENV: u16 = 35;
pub const GEN_DECAY_VOL_ENV: u16 = 36;
pub const GEN_SUSTAIN_VOL_ENV: u16 = 37;
pub const GEN_RELEASE_VOL_ENV: u16 = 38;
pub const GEN_KEY_RANGE: u16 = 43;
pub const GEN_VEL_RANGE: u16 = 44;
pub const GEN_INITIAL_ATTENUATION: u16 = 48;
pub const GEN_COARSE_TUNE: u16 = 51;
pub const GEN_FINE_TUNE: u16 = 52;
pub const GEN_SAMPLE_MODES: u16 = 54;
pub const GEN_OVERRIDING_ROOT_KEY: u16 = 58;

pub fn write_chunk(buf: &mut Vec<u8>, id: &[u8; 4], content: &[u8]) {
    buf.extend_from_slice(id);
    buf.extend_from_slice(&(content.len() as u32).to_le_bytes());
    buf.extend_from_slice(content);
    if content.len() % 2 == 1 {
        buf.push(0);
    }
}

pub fn write_list(buf: &mut Vec<u8>, list_type: &[u8; 4], build_inner: impl FnOnce(&mut Vec<u8>)) {
    let mut inner = Vec::new();
    inner.extend_from_slice(list_type);
    build_inner(&mut inner);
    write_chunk(buf, b"LIST", &inner);
}

pub fn zstr(name: &str, field_len: usize) -> Vec<u8> {
    let mut bytes = name.as_bytes().to_vec();
    bytes.resize(field_len, 0);
    bytes
}

pub fn write_gen(buf: &mut Vec<u8>, g: &Gen) {
    buf.extend_from_slice(&g.id.to_le_bytes());
    match g.amount {
        GenAmount::Value(v) => buf.extend_from_slice(&v.to_le_bytes()),
        GenAmount::Range(low, high) => buf.extend_from_slice(&[low, high]),
    }
}

/// One instrument zone's worth of generators, plus the sample id it must end
/// with (`SampleID` is what makes a zone a "local" zone with a sound, rather
/// than the global/default zone).
pub struct ZoneSpec {
    pub generators: Vec<Gen>,
}

pub struct Sf2Fixture {
    pub samples: Vec<i16>,
    pub sample_rate: u32,
    /// Indices into `samples` (matches `SampleHeader::start/end`).
    pub header_start: u32,
    pub header_end: u32,
    pub header_loop_start: u32,
    pub header_loop_end: u32,
    pub origpitch: u8,
    pub pitchadj: i8,
    pub zone: ZoneSpec,
    /// Instrument zones after the first. A real multi-zone instrument is how
    /// a key split or a stereo sample pair is written, and the per-layer
    /// modulation destinations only mean anything against more than one.
    pub extra_zones: Vec<ZoneSpec>,
}

/// Assembles a complete, spec-valid single-preset/single-instrument/single-sample
/// SF2 file. Every chunk size is computed from real content — nothing here is a
/// guessed byte count.
pub fn build_sf2(fixture: &Sf2Fixture) -> Vec<u8> {
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
        inst.extend_from_slice(&(fixture.extra_zones.len() as u16 + 1).to_le_bytes());
        write_chunk(buf, b"inst", &inst);

        // ibag: one entry per instrument zone, each naming where its
        // generators start, plus the terminator entry that names the end of
        // the last zone's.
        let zones: Vec<&ZoneSpec> = std::iter::once(&fixture.zone)
            .chain(fixture.extra_zones.iter())
            .collect();
        let mut ibag = Vec::new();
        let mut generator_index = 0u16;
        for zone in &zones {
            ibag.extend_from_slice(&generator_index.to_le_bytes());
            ibag.extend_from_slice(&0u16.to_le_bytes());
            // +1 for the SampleID generator every zone ends with.
            generator_index += zone.generators.len() as u16 + 1;
        }
        ibag.extend_from_slice(&generator_index.to_le_bytes());
        ibag.extend_from_slice(&0u16.to_le_bytes());
        write_chunk(buf, b"ibag", &ibag);

        write_chunk(buf, b"imod", &[0u8; 10]);

        // igen: every zone's generators, each ending with SampleID=0, plus the
        // terminator.
        let mut igen = Vec::new();
        for zone in &zones {
            for g in &zone.generators {
                write_gen(&mut igen, g);
            }
            write_gen(&mut igen, &gen_val(53, 0)); // GeneratorType::SampleID -> sample 0
        }
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

pub fn write_fixture_to_temp_file(name: &str, bytes: &[u8]) -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!("fontelle-test-{name}-{}.sf2", std::process::id()));
    let mut f = std::fs::File::create(&path).expect("write test fixture");
    f.write_all(bytes).expect("write test fixture bytes");
    path
}

/// Builds an SF2 with several presets, each with its own instrument and its
/// own single-zone sample, so preset *selection* can be tested rather than
/// just preset *parsing*. Mirrors `build_sf2`'s structure; kept separate so
/// the single-preset fixture above stays easy to read.
pub fn build_multi_preset_sf2(presets: &[(&str, u16, u16, i16)]) -> Vec<u8> {
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
pub const MULTI: &[(&str, u16, u16, i16)] = &[
    ("Orca", 123, 0, 55),
    ("Bells", 14, 0, 60),
    ("Piano", 0, 0, 69),
];
