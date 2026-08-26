use std::io::Cursor;
use std::path::Path;

use fontelle_core::{
    Curve, FilterSlot, Layer, LoopMode, ModDest, ModMatrix, ModRoute, ModSource, Patch,
    PlaybackConfig, SampleBuffer, SampleStore, Source, VoiceConfig,
};
use fontelle_dsp::{EnvelopeConfig, EnvelopeCurve, SvfMode};
use soundfont::raw::{Generator, GeneratorType};
use soundfont::{SoundFont2, Zone};

#[derive(Debug)]
pub struct ImportError(pub String);

impl std::fmt::Display for ImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ImportError {}

fn gen_i16(zone: &Zone, ty: GeneratorType) -> Option<i16> {
    zone.gen_list
        .iter()
        .find(|g: &&Generator| g.ty == ty)
        .and_then(|g| g.amount.as_i16().copied())
}

fn gen_range(zone: &Zone, ty: GeneratorType) -> Option<(u8, u8)> {
    zone.gen_list
        .iter()
        .find(|g: &&Generator| g.ty == ty)
        .and_then(|g| g.amount.as_range().map(|r| (r.low, r.high)))
}

/// `fine + coarse * 32768`, the standard SF2 fine/coarse offset composition
/// (used identically for sample start/end and loop start/end offsets).
fn offset_samples(zone: &Zone, fine: GeneratorType, coarse: GeneratorType) -> f64 {
    gen_i16(zone, fine).unwrap_or(0) as f64 + gen_i16(zone, coarse).unwrap_or(0) as f64 * 32768.0
}

/// `2^(timecents / 1200)` seconds. SF2's absent-generator default for every
/// time-based volume-envelope generator is -12000 timecents (~1ms, i.e.
/// effectively instant) per the spec's generator default table.
/// SF2's wide-open cutoff: 13500 absolute cents is ~19.9 kHz, above anything a
/// filter would usefully shape. A zone at or past it gets no filter at all,
/// rather than one that costs every voice work to do nothing.
const FILTER_BYPASS_CENTS: i16 = 13_500;

/// The amount of SF2's default velocity-to-filter-cutoff modulator, in cents.
const DEFAULT_VEL_TO_FILTER_CENTS: f32 = -2400.0;

/// Absolute cents to Hz. SF2 anchors the scale at 8.176 Hz (MIDI note 0), so
/// `initialFilterFc` of 13500 is ~19912 Hz and 7200 is ~523 Hz.
fn absolute_cents_to_hz(cents: i16) -> f32 {
    8.176 * 2f32.powf(cents as f32 / 1200.0)
}

/// `initialFilterQ` to a filter Q.
///
/// The generator is "the height above DC gain in centibels which the filter
/// resonance exhibits at the cutoff frequency", and SF2 2.01 defines 0 as *no*
/// resonance. No resonance is Butterworth, not unity Q, so the 3.01 dB has to
/// come off before converting — otherwise every unresonant zone in every file
/// gets a 3 dB bump at its corner.
fn filter_q_centibels_to_q(centibels: i16) -> f32 {
    10f32.powf((centibels as f32 / 10.0 - 3.01) / 20.0)
}

fn timecents_to_seconds(tc: Option<i16>) -> f32 {
    2f32.powf(tc.unwrap_or(-12_000) as f32 / 1200.0)
}

/// SF2's `sustainModEnv`: the *decrease* from full scale in 0.1% units, not an
/// attenuation in centibels the way `sustainVolEnv` is. Reading it as
/// centibels makes a half-sustained filter envelope collapse to nothing —
/// the same class of mistake as reading the volume envelope's stage times as
/// stage durations.
fn permille_decrease_to_level(permille: Option<i16>) -> f32 {
    1.0 - (permille.unwrap_or(0) as f32 / 1000.0).clamp(0.0, 1.0)
}

/// Both of SF2's LFO frequency generators are in absolute cents, with an
/// absent generator meaning 0 — which is 8.176 Hz, not silence.
fn lfo_rate_hz(zone: &Zone, generator: GeneratorType) -> f32 {
    absolute_cents_to_hz(gen_i16(zone, generator).unwrap_or(0))
}

/// Centibels of attenuation (0 = full volume, 1000 = silence) to a linear
/// `0..1` level.
fn centibels_attenuation_to_linear(cb: Option<i16>) -> f32 {
    10f32.powf(-(cb.unwrap_or(0) as f32) / 200.0)
}

fn build_layer(
    zone: &Zone,
    header: &soundfont::raw::SampleHeader,
    store: &mut SampleStore,
    pcm_bytes: &[u8],
    smpl_offset: u64,
) -> Result<Layer, ImportError> {
    let start_delta = offset_samples(
        zone,
        GeneratorType::StartAddrsOffset,
        GeneratorType::StartAddrsCoarseOffset,
    );
    let end_delta = offset_samples(
        zone,
        GeneratorType::EndAddrsOffset,
        GeneratorType::EndAddrsCoarseOffset,
    );
    let loop_start_delta = offset_samples(
        zone,
        GeneratorType::StartloopAddrsOffset,
        GeneratorType::StartloopAddrsCoarseOffset,
    );
    let loop_end_delta = offset_samples(
        zone,
        GeneratorType::EndloopAddrsOffset,
        GeneratorType::EndloopAddrsCoarseOffset,
    );

    // A corrupt or truncated file can carry a sample header whose range runs
    // past the real `smpl` chunk — nothing in the parser cross-checks the two.
    // Indexing on trust would panic out of bounds and take the process with
    // it; TDD §20.3 requires a clear failure instead. Validated up front so
    // the decode loop below can stay a plain indexed read.
    let buffer_len = (header.end.saturating_sub(header.start)) as usize;
    let byte_start = smpl_offset as usize + header.start as usize * 2;
    let byte_end = byte_start
        .checked_add(buffer_len * 2)
        .ok_or_else(|| ImportError("sample header range overflows".into()))?;
    if byte_end > pcm_bytes.len() {
        return Err(ImportError(format!(
            "sample header range [{}, {}) runs past the end of the file's sample data \
             ({} bytes) — the file is truncated or corrupt",
            header.start,
            header.end,
            pcm_bytes.len()
        )));
    }

    let mut data = Vec::with_capacity(buffer_len);
    for i in 0..buffer_len {
        let b0 = pcm_bytes[byte_start + i * 2];
        let b1 = pcm_bytes[byte_start + i * 2 + 1];
        let sample = i16::from_le_bytes([b0, b1]);
        data.push(sample as f32 / 32768.0);
    }
    let asset = store.insert(SampleBuffer {
        data: std::sync::Arc::from(data),
        sample_rate: header.sample_rate,
    });

    let root_key = match gen_i16(zone, GeneratorType::OverridingRootKey) {
        Some(v) if v >= 0 => v as u8,
        _ => header.origpitch,
    };
    let fine_tune_cents = gen_i16(zone, GeneratorType::CoarseTune).unwrap_or(0) as f32 * 100.0
        + gen_i16(zone, GeneratorType::FineTune).unwrap_or(0) as f32
        + header.pitchadj as f32;

    let loop_mode = match gen_i16(zone, GeneratorType::SampleModes).unwrap_or(0) {
        1 => LoopMode::Forward,
        3 => LoopMode::Sustain,
        _ => LoopMode::Off,
    };

    Ok(Layer {
        source: Source::Sample { file: asset },
        key_range: gen_range(zone, GeneratorType::KeyRange).unwrap_or((0, 127)),
        vel_range: gen_range(zone, GeneratorType::VelRange).unwrap_or((0, 127)),
        root_key,
        fine_tune_cents,
        playback: PlaybackConfig {
            start_offset: start_delta,
            end_offset: buffer_len as f64 + end_delta,
            loop_mode,
            loop_start: (header.loop_start as f64 - header.start as f64) + loop_start_delta,
            loop_end: (header.loop_end as f64 - header.start as f64) + loop_end_delta,
            // SF2 has no interpolation generator, so the layer names no
            // kernel and follows the session quality (TDD §7.6).
            interpolation: None,
            ..PlaybackConfig::default()
        },
        gain_db: -(gen_i16(zone, GeneratorType::InitialAttenuation).unwrap_or(0) as f32) / 10.0,
        pan: gen_i16(zone, GeneratorType::Pan).unwrap_or(0) as f32 / 500.0,
    })
}

/// Imports the first preset of the SF2 file at `path` into a `Patch`, decoding
/// its sample data into `store`.
///
/// **M0 scope** (see `docs/scaffolding-notes.md` and `PROGRESS.md` for the full
/// list): only the first preset is imported; preset-level zone generators are
/// not layered over instrument-level ones (only the instrument zones' own
/// generators are read); modulators are ignored entirely; only the amp envelope
/// (`envelopes[0]`, from the *first* sample-bearing zone found) is populated —
/// per-layer envelopes aren't representable in Fontelle's fixed voice topology
/// (TDD §7.4) since the amp envelope is shared patch-wide, not per-layer;
/// filter/LFO/mod-matrix generators are not read, so filters stay disabled and
/// the mod matrix stays empty. Every one of these is a deliberate, documented
/// cut, not an oversight — read `soundfont`'s `Generator`/`GeneratorType` and
/// extend `build_layer` to close any of them.
pub fn import_sf2(path: &Path, store: &mut SampleStore) -> Result<Patch, ImportError> {
    import_sf2_preset(path, 0, store)
}

/// One preset's identity within an SF2 file, as reported by [`list_presets`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresetInfo {
    /// Position in the file — what [`import_sf2_preset`] takes.
    pub index: usize,
    pub name: String,
    /// General MIDI program number.
    pub program: u16,
    pub bank: u16,
}

/// Lists the presets in `path`, in file order, without decoding any audio.
///
/// Worth having as its own operation because **SF2 files store presets in
/// arbitrary order** — a bank's "main" instrument is very often not first.
/// (A real example that cost real confusion: `Secret_of_Mana.sf2` opens with
/// `SOM Orca`, a whale sound effect at 1824 Hz, and keeps `SOM Piano` at
/// index 27.) Anything that imports "the" preset without letting the user see
/// this list is guessing.
pub fn list_presets(path: &Path) -> Result<Vec<PresetInfo>, ImportError> {
    let bytes = std::fs::read(path).map_err(|e| ImportError(e.to_string()))?;
    let sf2 = load_sf2(&bytes)?;
    Ok(sf2
        .presets
        .iter()
        .enumerate()
        .map(|(index, preset)| PresetInfo {
            index,
            name: preset.header.name.clone(),
            program: preset.header.preset,
            bank: preset.header.bank,
        })
        .collect())
}

/// Shared by [`list_presets`] and [`import_sf2_preset`].
///
/// `soundfont::SoundFont2::load` uses a bare `assert_eq!` on the RIFF/sfbk
/// header instead of returning `Err` for malformed input — a real defect in
/// that crate (verified by reading its source), and one that would let an
/// untrusted file crash the process. TDD §20.3 requires malformed SF2 files
/// to fail with a clear message, never crash, so this boundary must not let
/// a panic from a dependency escape it.
fn load_sf2(bytes: &[u8]) -> Result<SoundFont2, ImportError> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut cursor = Cursor::new(bytes);
        SoundFont2::load(&mut cursor)
    }))
    .map_err(|_| ImportError("not a valid SF2 file (parser panicked)".into()))?
    .map_err(|e| ImportError(format!("{e:?}")))
}

/// Imports the preset at `preset_index` (see [`list_presets`]) into a `Patch`,
/// decoding its sample data into `store`. [`import_sf2`] is this with index 0.
pub fn import_sf2_preset(
    path: &Path,
    preset_index: usize,
    store: &mut SampleStore,
) -> Result<Patch, ImportError> {
    let bytes = std::fs::read(path).map_err(|e| ImportError(e.to_string()))?;

    let sf2 = load_sf2(&bytes)?;

    let smpl = sf2
        .sample_data
        .smpl
        .ok_or_else(|| ImportError("SF2 file has no sample data (smpl chunk)".into()))?;

    let preset = sf2.presets.get(preset_index).ok_or_else(|| {
        ImportError(format!(
            "preset index {preset_index} out of range — this file has {} preset(s)",
            sf2.presets.len()
        ))
    })?;

    let instrument_id = preset
        .zones
        .iter()
        .find_map(|z| z.instrument())
        .ok_or_else(|| ImportError("preset has no zone referencing an instrument".into()))?;
    let instrument = sf2
        .instruments
        .get(*instrument_id as usize)
        .ok_or_else(|| {
            ImportError(format!(
                "preset references missing instrument {instrument_id}"
            ))
        })?;

    let mut layers = Vec::new();
    let mut amp_envelope = None;
    let mut filter = None;
    let mut modulation: Option<ZoneModulation> = None;

    for zone in &instrument.zones {
        let Some(sample_id) = zone.sample() else {
            continue; // the instrument's global zone, if any -- not merged yet (see doc comment)
        };
        let header = sf2
            .sample_headers
            .get(*sample_id as usize)
            .ok_or_else(|| ImportError(format!("zone references missing sample {sample_id}")))?;

        if amp_envelope.is_none() {
            amp_envelope = Some(EnvelopeConfig {
                delay_s: timecents_to_seconds(gen_i16(zone, GeneratorType::DelayVolEnv)),
                attack_s: timecents_to_seconds(gen_i16(zone, GeneratorType::AttackVolEnv)),
                hold_s: timecents_to_seconds(gen_i16(zone, GeneratorType::HoldVolEnv)),
                decay_s: timecents_to_seconds(gen_i16(zone, GeneratorType::DecayVolEnv)),
                sustain_level: centibels_attenuation_to_linear(gen_i16(
                    zone,
                    GeneratorType::SustainVolEnv,
                )),
                release_s: timecents_to_seconds(gen_i16(zone, GeneratorType::ReleaseVolEnv)),
                // SF2 2.04 defines the volume envelope's decay and release as a
                // constant dB rate, not a constant amplitude rate, and defines
                // the times above against a 100 dB span rather than as stage
                // durations. `EnvelopeCurve::Decibel` is what makes the numbers
                // mean what the file's author intended.
                curve: EnvelopeCurve::Decibel,
            });
        }

        // SF2 puts the filter on every zone; TDD §7.4's fixed voice topology
        // puts it after the layer mix, so a multi-zone preset with differing
        // filters can't be represented exactly. Take the first zone's, which is
        // what the amp envelope above already does, and record the limit rather
        // than averaging into something no zone asked for.
        if filter.is_none() {
            let cutoff_cents =
                gen_i16(zone, GeneratorType::InitialFilterFc).unwrap_or(FILTER_BYPASS_CENTS);
            filter = Some(FilterSlot {
                mode: SvfMode::Lowpass,
                cutoff_hz: absolute_cents_to_hz(cutoff_cents),
                resonance: filter_q_centibels_to_q(
                    gen_i16(zone, GeneratorType::InitialFilterQ).unwrap_or(0),
                ),
                enabled: cutoff_cents < FILTER_BYPASS_CENTS,
            });
        }

        if modulation.is_none() {
            modulation = Some(ZoneModulation::read(zone));
        }

        layers.push(build_layer(zone, header, store, &bytes, smpl.offset)?);
    }

    if layers.is_empty() {
        return Err(ImportError("instrument has no zones with a sample".into()));
    }

    // Filter2 stays free for the user: SF2 describes a single lowpass, so
    // importing into both slots would spend the second one reproducing the
    // first rather than leaving it available.
    let disabled_filter = FilterSlot {
        mode: SvfMode::Lowpass,
        cutoff_hz: 20_000.0,
        resonance: std::f32::consts::FRAC_1_SQRT_2,
        enabled: false,
    };
    let filter = filter.unwrap_or(disabled_filter);

    // SF2 2.04 §8.4.2's second always-present default modulator: velocity to
    // initial filter cutoff, linear, negative direction, -2400 cents. It is
    // what makes a soundfont darken as you play softer rather than merely get
    // quieter. Seeded only when there is a filter for it to move; a route
    // aimed at a disabled slot is dead weight in the matrix.
    //
    // Its sibling, velocity to initial attenuation, is applied directly in
    // `fontelle_core::velocity_to_gain` rather than routed here — see the note
    // there. Both become ordinary editable routes once the UI can show them.
    let mut mod_matrix = ModMatrix::default();
    if filter.enabled {
        let destination = ModDest::FilterCutoff(0);
        mod_matrix.routes.push(ModRoute {
            source: ModSource::Velocity,
            destination,
            depth: DEFAULT_VEL_TO_FILTER_CENTS / destination.full_scale(),
            curve: Curve::Linear,
            via: None,
            invert: true,
        });
    }
    let amp = amp_envelope.unwrap();
    let modulation = modulation.unwrap_or_default();
    modulation.seed_routes(&mut mod_matrix, layers.len(), filter.enabled);

    Ok(Patch {
        layers,
        filters: [filter, disabled_filter],
        envelopes: vec![amp, modulation.envelope],
        lfos: modulation.lfos.to_vec(),
        mod_matrix,
        voice_config: VoiceConfig::default(),
    })
}

/// The modulation half of an instrument zone: SF2's two LFOs, its modulation
/// envelope, and the six generators that connect them to something.
///
/// Read from the first sample-bearing zone, which is what the amp envelope and
/// the filter already do — Fontelle's fixed voice topology (TDD §7.4) has one
/// of each per voice, not one per layer, so a multi-zone instrument whose
/// zones disagree cannot be represented exactly and averaging would produce a
/// setting no zone asked for.
struct ZoneModulation {
    /// `patch.envelopes[1]`.
    envelope: EnvelopeConfig,
    /// Index 0 is SF2's modulation LFO, index 1 its vibrato LFO. Both always
    /// present, because SF2 gives every zone both; a patch that routes neither
    /// never advances them (see `Voice::render`).
    lfos: [fontelle_core::Lfo; 2],
    mod_lfo_to_pitch_cents: f32,
    vib_lfo_to_pitch_cents: f32,
    mod_env_to_pitch_cents: f32,
    mod_lfo_to_filter_cents: f32,
    mod_env_to_filter_cents: f32,
    mod_lfo_to_volume_db: f32,
}

/// SF2's modulation LFO, then its vibrato LFO.
const MOD_LFO: u8 = 0;
const VIB_LFO: u8 = 1;

impl Default for ZoneModulation {
    fn default() -> Self {
        let idle_lfo = fontelle_core::Lfo {
            rate_hz: 8.176,
            depth: 1.0,
            shape: fontelle_dsp::OscKind::Sine,
            delay_s: 0.0,
        };
        Self {
            envelope: EnvelopeConfig {
                delay_s: 0.0,
                attack_s: 0.0,
                hold_s: 0.0,
                decay_s: 0.0,
                sustain_level: 1.0,
                release_s: 0.0,
                curve: EnvelopeCurve::Linear,
            },
            lfos: [idle_lfo; 2],
            mod_lfo_to_pitch_cents: 0.0,
            vib_lfo_to_pitch_cents: 0.0,
            mod_env_to_pitch_cents: 0.0,
            mod_lfo_to_filter_cents: 0.0,
            mod_env_to_filter_cents: 0.0,
            mod_lfo_to_volume_db: 0.0,
        }
    }
}

impl ZoneModulation {
    fn read(zone: &Zone) -> Self {
        // Both SF2 LFOs are plain sines. The format has no shape generator.
        let lfo = |delay: GeneratorType, freq: GeneratorType| fontelle_core::Lfo {
            rate_hz: lfo_rate_hz(zone, freq),
            depth: 1.0,
            shape: fontelle_dsp::OscKind::Sine,
            delay_s: timecents_to_seconds(gen_i16(zone, delay)),
        };
        let cents = |ty| gen_i16(zone, ty).unwrap_or(0) as f32;

        let sustain_level = permille_decrease_to_level(gen_i16(zone, GeneratorType::SustainModEnv));
        Self {
            envelope: EnvelopeConfig {
                delay_s: timecents_to_seconds(gen_i16(zone, GeneratorType::DelayModEnv)),
                attack_s: timecents_to_seconds(gen_i16(zone, GeneratorType::AttackModEnv)),
                hold_s: timecents_to_seconds(gen_i16(zone, GeneratorType::HoldModEnv)),
                // SF2 defines `decayModEnv` as the time for a 100% change, and
                // a decay that only has to reach a sustain of 0.5 covers half
                // of one. `EnvelopeCurve::Decibel` bakes that rule in for the
                // volume envelope; the modulation envelope is linear, so the
                // scaling is applied here instead. Reading the generator as a
                // stage duration stretches every filter envelope's decay.
                decay_s: timecents_to_seconds(gen_i16(zone, GeneratorType::DecayModEnv))
                    * (1.0 - sustain_level),
                sustain_level,
                // Release is the same 100%-change rule, but a note released
                // from somewhere other than its sustain level covers a
                // different share of it, and that is only known at note-off.
                // Taken as written; the error is bounded by the sustain level
                // and only shows on an early release.
                release_s: timecents_to_seconds(gen_i16(zone, GeneratorType::ReleaseModEnv)),
                // SF2's modulation envelope is linear in its own units, unlike
                // the volume envelope, which is a straight line in decibels.
                curve: EnvelopeCurve::Linear,
            },
            lfos: [
                lfo(GeneratorType::DelayModLFO, GeneratorType::FreqModLFO),
                lfo(GeneratorType::DelayVibLFO, GeneratorType::FreqVibLFO),
            ],
            mod_lfo_to_pitch_cents: cents(GeneratorType::ModLfoToPitch),
            vib_lfo_to_pitch_cents: cents(GeneratorType::VibLfoToPitch),
            mod_env_to_pitch_cents: cents(GeneratorType::ModEnvToPitch),
            mod_lfo_to_filter_cents: cents(GeneratorType::ModLfoToFilterFc),
            mod_env_to_filter_cents: cents(GeneratorType::ModEnvToFilterFc),
            // The only one of the six in centibels rather than cents.
            mod_lfo_to_volume_db: cents(GeneratorType::ModLfoToVolume) / 10.0,
        }
    }

    /// Turns the amounts into matrix routes.
    ///
    /// **One route per layer** for the per-layer destinations. An SF2
    /// modulation generator applies to the whole voice, while TDD §7.5's
    /// pitch, gain and pan destinations are addressed by layer index, so a
    /// key-split instrument needs the route repeated or every layer but the
    /// first plays unmodulated. A route with zero depth is never emitted: the
    /// voice scans the matrix once per destination per block, and dead weight
    /// there is paid for on every one.
    fn seed_routes(&self, matrix: &mut ModMatrix, layers: usize, filter_enabled: bool) {
        let mut push = |source: ModSource, destination: ModDest, amount: f32| {
            if amount == 0.0 {
                return;
            }
            matrix.routes.push(ModRoute {
                source,
                destination,
                depth: amount / destination.full_scale(),
                curve: Curve::Linear,
                via: None,
                // An LFO and a modulation envelope are already signed sources;
                // the direction bit belongs to SF2's *modulators*, not to its
                // generators.
                invert: false,
            });
        };

        for layer in 0..layers.min(u8::MAX as usize) as u8 {
            push(
                ModSource::Lfo(MOD_LFO),
                ModDest::LayerPitch(layer),
                self.mod_lfo_to_pitch_cents,
            );
            push(
                ModSource::Lfo(VIB_LFO),
                ModDest::LayerPitch(layer),
                self.vib_lfo_to_pitch_cents,
            );
            push(
                ModSource::Envelope(1),
                ModDest::LayerPitch(layer),
                self.mod_env_to_pitch_cents,
            );
            push(
                ModSource::Lfo(MOD_LFO),
                ModDest::LayerGain(layer),
                self.mod_lfo_to_volume_db,
            );
        }

        // A route aimed at a switched-off filter slot is dead weight in the
        // matrix and misleading to anyone reading the patch.
        if filter_enabled {
            push(
                ModSource::Lfo(MOD_LFO),
                ModDest::FilterCutoff(0),
                self.mod_lfo_to_filter_cents,
            );
            push(
                ModSource::Envelope(1),
                ModDest::FilterCutoff(0),
                self.mod_env_to_filter_cents,
            );
        }
    }
}

// Kept as unused-but-real types matching the TDD's §7.3 diagram
// (`SF2 file --parse--> ImportedZones --seed--> Patch`), even though M0's
// `import_sf2` collapses both steps into one function. Splitting them apart —
// so a caller can inspect raw zones before seeding, or seed the same
// `ImportedZones` into more than one `Patch` — is future work, not a boundary
// this crate needs yet.
#[allow(dead_code)]
pub struct ImportedZone {
    pub name: String,
}

#[allow(dead_code)]
pub struct ImportedZones {
    pub zones: Vec<ImportedZone>,
}
