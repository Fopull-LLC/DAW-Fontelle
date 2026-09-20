//! The serialised form of a [`Patch`] (TDD §8.3, §17.2).
//!
//! §8.3 requires that "a patch built inside the DAW loads in the standalone
//! plugin, and vice versa, byte-identically", which is only true if one crate
//! owns the definition — so the format lives here, in `fontelle-core`, next to
//! the type it describes, rather than in the document model that stores it.

use std::collections::HashMap;

use fontelle_types::{AssetId, PatchData, SampleRef};
use slotmap::Key;

use crate::mod_matrix::ModMatrix;
use crate::patch::{FilterSlot, Layer, Lfo, MACRO_COUNT, Macro, Patch, PatchFx, Source, ZoneId};
use crate::playback::PlaybackConfig;
use crate::voice::VoiceConfig;
use fontelle_dsp::{EnvelopeConfig, OscKind, Oversampling};

/// The revision of the patch format this build writes.
///
/// Bumped whenever a change cannot be read by [`serde`]'s own defaulting —
/// a renamed field, a changed unit, a restructured enum. Adding a field with a
/// `#[serde(default)]` does not need a bump; changing what an existing field
/// *means* always does, because the old value will parse and be wrong.
pub const PATCH_FORMAT_VERSION: u32 = 1;

/// Why a stored patch could not be read, or written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatchFormatError {
    /// Written by a build newer than this one. Named separately from
    /// [`Self::Malformed`] because it is not a broken file — it is a file this
    /// build is too old for, and telling the two apart is the difference
    /// between "upgrade Fontelle" and "your project is damaged".
    FromTheFuture { found: u32, newest: u32 },
    /// The body does not match the shape its `format_version` claims.
    Malformed(String),
}

impl std::fmt::Display for PatchFormatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::FromTheFuture { found, newest } => write!(
                f,
                "this patch is in format version {found}, and this build of Fontelle \
                 understands up to version {newest} — upgrade Fontelle to open it"
            ),
            Self::Malformed(why) => write!(f, "this patch could not be read: {why}"),
        }
    }
}

impl std::error::Error for PatchFormatError {}

/// A layer whose audio could not be found on load.
///
/// TDD §17.4 is explicit that broken links are a normal condition: the project
/// "loads and plays with placeholders for anything still missing. It does not
/// refuse to open, and it does not lose the references on the next save." So an
/// unresolved sample is reported, not raised — the layer is silent and the
/// reference is handed back for the relink dialog to work on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnresolvedSample {
    /// Which layer of the loaded patch is silent as a result.
    pub layer: usize,
    /// What it pointed at. `None` when the patch was written without any
    /// provenance for that layer at all — a synthesised or recorded sample
    /// that never came from a file.
    pub sample: Option<SampleRef>,
}

/// The result of reading a stored patch: the patch, plus whatever could not be
/// relinked.
#[derive(Debug, Clone)]
pub struct LoadedPatch {
    pub patch: Patch,
    pub unresolved: Vec<UnresolvedSample>,
}

// --- The stored shape ------------------------------------------------------
//
// Everything except `Source` is the live type with `serde` derived on it, so
// there is exactly one definition of each field and no conversion to keep in
// step. `Source` is the one type that cannot be: it addresses audio by
// `AssetId`, a `slotmap` key, which INVARIANT 8 forbids serialising.

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
enum StoredSource {
    Sf2Zone {
        file: Option<SampleRef>,
        zone: u32,
    },
    Sample {
        file: Option<SampleRef>,
    },
    Oscillator(OscKind),
    /// The built-in drum machine's hits. The one source that stores **whole**
    /// rather than by reference: it names no file, so there is nothing to
    /// relink and nothing that can go missing.
    Drum(fontelle_dsp::DrumVoice),
    /// A Flopsynth oscillator. Stored **whole**, like `Drum` and for exactly
    /// the same reason: it names no file, so there is nothing to relink.
    ///
    /// This variant is why the format version moved rather than the field
    /// defaults absorbing it: an older build reading a patch with one in it
    /// would report `Malformed` — "your project is damaged" — where the true
    /// answer is "upgrade Fontelle".
    Synth(fontelle_dsp::SynthOsc),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct StoredLayer {
    source: StoredSource,
    key_range: (u8, u8),
    vel_range: (u8, u8),
    root_key: u8,
    fine_tune_cents: f32,
    playback: PlaybackConfig,
    gain_db: f32,
    pan: f32,
}

/// One [`UserWavetable`] as a patch file holds it.
///
/// The samples are **sixteen-bit PCM, base64** — the encoding
/// `Channel::patch_data` already uses for a plugin's blob, and for the same
/// reason: `serde_json` writes a `Vec<f32>` as decimal numbers, four to
/// twelve characters each, which on a table of a hundred thousand samples is
/// a megabyte of text. Sixteen bits rather than the full float because the
/// table is normalised into its pyramid on the way in and a preset is not an
/// archive of the file it came from.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct StoredWavetable {
    name: String,
    frames: usize,
    /// Little-endian `i16`, base64.
    samples: String,
}

impl StoredWavetable {
    fn of(table: &crate::patch::UserWavetable) -> Self {
        Self {
            name: table.name.clone(),
            frames: table.frames,
            samples: encode_pcm16(&table.samples),
        }
    }

    /// Back to samples. A blob that is not readable comes back as **no
    /// samples** rather than as an error: a preset with a damaged table
    /// should open with that oscillator silent, the way one naming a missing
    /// table does, rather than refusing to open at all.
    fn into_table(self) -> crate::patch::UserWavetable {
        crate::patch::UserWavetable {
            name: self.name,
            frames: self.frames,
            samples: decode_pcm16(&self.samples),
        }
    }
}

/// One [`crate::UserSample`] as a patch file holds it: the zones, each with
/// its samples as sixteen-bit PCM in base64 — [`StoredWavetable`]'s encoding,
/// for its reasons. Unless it is one of the bank's own sets, in which case
/// `factory` names it and there are no zones: the audio is in the binary
/// (`crate::factory_samples`), and a project is not asked to carry it.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct StoredSample {
    name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    factory: Option<String>,
    #[serde(default)]
    zones: Vec<StoredZone>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct StoredZone {
    /// Left out when empty, so a file written before zones had names is
    /// byte for byte the file written after.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    name: String,
    root_key: u8,
    fine_cents: f32,
    key_range: (u8, u8),
    sample_rate: u32,
    /// Little-endian `i16`, base64.
    samples: String,
}

impl StoredSample {
    fn of(sample: &crate::UserSample) -> Self {
        if let Some(set) = sample.factory {
            return Self {
                name: sample.name.clone(),
                factory: Some(set.id().to_string()),
                zones: Vec::new(),
            };
        }
        Self {
            name: sample.name.clone(),
            factory: None,
            zones: sample
                .zones
                .iter()
                .map(|zone| StoredZone {
                    name: zone.name.clone(),
                    root_key: zone.root_key,
                    fine_cents: zone.fine_cents,
                    key_range: zone.key_range,
                    sample_rate: zone.sample_rate,
                    samples: encode_pcm16(&zone.samples),
                })
                .collect(),
        }
    }

    /// Back to a recording. A damaged blob is a silent zone, for
    /// `StoredWavetable::into_table`'s reason; a factory set this build has
    /// not got — a file from a later one — is a silent recording under the
    /// stored name, for the same reason.
    fn into_sample(self) -> crate::UserSample {
        if let Some(id) = self.factory {
            return match crate::factory_samples::FactorySampleSet::from_id(&id) {
                Some(set) => set.sample(),
                None => crate::UserSample {
                    name: self.name,
                    factory: None,
                    zones: Vec::new(),
                },
            };
        }
        crate::UserSample {
            name: self.name,
            factory: None,
            zones: self
                .zones
                .into_iter()
                .map(|zone| crate::SampleZone {
                    name: zone.name,
                    root_key: zone.root_key,
                    fine_cents: zone.fine_cents,
                    key_range: zone.key_range,
                    sample_rate: zone.sample_rate,
                    samples: decode_pcm16(&zone.samples).into(),
                })
                .collect(),
        }
    }
}

fn encode_pcm16(samples: &[f32]) -> String {
    let mut bytes = Vec::with_capacity(samples.len() * 2);
    for sample in samples {
        let value = (sample.clamp(-1.0, 1.0) * 32_767.0).round() as i16;
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    fontelle_types::encode_base64(&bytes)
}

fn decode_pcm16(text: &str) -> Vec<f32> {
    let bytes = fontelle_types::decode_base64(text).unwrap_or_default();
    bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| i16::from_le_bytes(*pair) as f32 / 32_767.0)
        .collect()
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct StoredPatch {
    layers: Vec<StoredLayer>,
    filters: [FilterSlot; 2],
    envelopes: Vec<EnvelopeConfig>,
    lfos: Vec<Lfo>,
    mod_matrix: ModMatrix,
    voice_config: VoiceConfig,
    /// The three below are `#[serde(default)]`, so a v1 body written without
    /// them — which is every patch this build writes that has no effects, no
    /// named macros and no trim — reads back identically.
    #[serde(default)]
    fx: Vec<PatchFx>,
    /// A list rather than the patch's array: four macros were written from
    /// the day macros existed until 2026-09-20 and eight since, and a list
    /// reads either. Written **four long unless a later one is set** —
    /// `stored_macros` — so no row of the bank is rewritten for slots it
    /// does not use (`docs/flopsynth-next.md` §0, rule 7); read padded to
    /// [`MACRO_COUNT`] and cut there (`macros_from_stored`).
    #[serde(default)]
    macros: Vec<Macro>,
    #[serde(default)]
    output_db: f32,
    /// The patch's own tables. `#[serde(default)]`, so every preset written
    /// before dropped sounds existed — which is the whole factory bank —
    /// reads back as carrying none.
    #[serde(default)]
    wavetables: Vec<StoredWavetable>,
    /// The patch's own recordings, `#[serde(default)]` for the same reason
    /// — and left out when there are none, so the files written before
    /// recordings existed are not all rewritten to say so.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    samples: Vec<StoredSample>,
    /// `#[serde(default)]` and left out at `Off`, for the same reason again
    /// (`docs/flopsynth-next.md` §4.1, ground rule 7).
    #[serde(default, skip_serializing_if = "Oversampling::is_off")]
    oversampling: Oversampling,
}

/// How many macros, LFOs and envelopes the file carried before there were
/// eight, eight and six — what a patch with nothing past them still writes.
const SLOTS_ALWAYS_WRITTEN: usize = 4;

/// `all` without its trailing at-rest entries past the first
/// [`SLOTS_ALWAYS_WRITTEN`]: how the LFOs, the envelopes and the macros go
/// into the file, so a row written with four stays at four until a fifth is
/// touched (`docs/flopsynth-next.md` §0, rule 7), and reads back full
/// through `Patch::fill_modulator_slots`.
fn trimmed<T: Clone + PartialEq>(all: &[T], at_rest: &T) -> Vec<T> {
    let last_set = all.iter().rposition(|x| x != at_rest).map_or(0, |i| i + 1);
    all[..last_set.max(SLOTS_ALWAYS_WRITTEN).min(all.len())].to_vec()
}

/// The macros as the file holds them: the first four always, and past
/// those only up to the last one that is not at rest.
fn stored_macros(macros: &[Macro; MACRO_COUNT]) -> Vec<Macro> {
    trimmed(macros, &Macro::default())
}

/// The file's list as the patch's array: padded with macros at rest, and
/// cut at [`MACRO_COUNT`] — a file from a build with more has no ninth
/// knob here to land on.
fn macros_from_stored(mut stored: Vec<Macro>) -> [Macro; MACRO_COUNT] {
    stored.resize_with(MACRO_COUNT, Macro::default);
    stored
        .try_into()
        .unwrap_or_else(|_| unreachable!("resized to MACRO_COUNT"))
}

impl Patch {
    /// Serialises this patch into the form a project or preset file stores.
    ///
    /// `provenance` says where each layer's audio came from. A layer whose
    /// `AssetId` is not in the map is written with no provenance: honest, and
    /// the only option for a sample that never came from a file.
    pub fn to_data(
        &self,
        provenance: &HashMap<AssetId, SampleRef>,
    ) -> Result<PatchData, PatchFormatError> {
        let stored = StoredPatch {
            layers: self
                .layers
                .iter()
                .map(|layer| StoredLayer {
                    source: match &layer.source {
                        Source::Sf2Zone { file, zone } => StoredSource::Sf2Zone {
                            file: provenance.get(file).cloned(),
                            zone: zone.0,
                        },
                        Source::Sample { file } => StoredSource::Sample {
                            file: provenance.get(file).cloned(),
                        },
                        Source::Oscillator(kind) => StoredSource::Oscillator(*kind),
                        Source::Drum(voice) => StoredSource::Drum(*voice),
                        Source::Synth(osc) => StoredSource::Synth(*osc),
                    },
                    key_range: layer.key_range,
                    vel_range: layer.vel_range,
                    root_key: layer.root_key,
                    fine_tune_cents: layer.fine_tune_cents,
                    playback: layer.playback,
                    gain_db: layer.gain_db,
                    pan: layer.pan,
                })
                .collect(),
            filters: self.filters,
            envelopes: trimmed(&self.envelopes, &crate::patch::envelope_at_rest()),
            lfos: trimmed(&self.lfos, &Lfo::default()),
            mod_matrix: self.mod_matrix.clone(),
            voice_config: self.voice_config,
            fx: self.fx.clone(),
            macros: stored_macros(&self.macros),
            output_db: self.output_db,
            wavetables: self.wavetables.iter().map(StoredWavetable::of).collect(),
            samples: self.samples.iter().map(StoredSample::of).collect(),
            oversampling: self.oversampling,
        };

        Ok(PatchData {
            format_version: PATCH_FORMAT_VERSION,
            // Only a non-finite float can fail here — `serde_json` has no
            // number for one. Reported rather than written as `null`, which
            // would come back as a parse error on a machine that no longer
            // has the patch that produced it.
            body: serde_json::to_value(&stored)
                .map_err(|e| PatchFormatError::Malformed(e.to_string()))?,
        })
    }

    /// Reads a stored patch back, resolving each stored sample reference to a
    /// live `AssetId` through `resolve`.
    ///
    /// Never fails on a missing sample — see [`UnresolvedSample`].
    pub fn from_data(
        data: &PatchData,
        mut resolve: impl FnMut(&SampleRef) -> Option<AssetId>,
    ) -> Result<LoadedPatch, PatchFormatError> {
        if data.format_version > PATCH_FORMAT_VERSION {
            return Err(PatchFormatError::FromTheFuture {
                found: data.format_version,
                newest: PATCH_FORMAT_VERSION,
            });
        }
        let body = migrate(data.body.clone(), data.format_version)?;
        let stored: StoredPatch =
            serde_json::from_value(body).map_err(|e| PatchFormatError::Malformed(e.to_string()))?;

        let mut unresolved = Vec::new();
        let mut layers = Vec::with_capacity(stored.layers.len());
        for (index, layer) in stored.layers.into_iter().enumerate() {
            // One closure over both sampled variants: a missing file means the
            // same thing whichever names it, and a layer left pointing at a
            // null `AssetId` renders silence because `SampleStore::get` has
            // nothing under it.
            let mut asset_for = |file: Option<SampleRef>| match file {
                Some(sample) => match resolve(&sample) {
                    Some(asset) => asset,
                    None => {
                        unresolved.push(UnresolvedSample {
                            layer: index,
                            sample: Some(sample),
                        });
                        AssetId::null()
                    }
                },
                None => {
                    unresolved.push(UnresolvedSample {
                        layer: index,
                        sample: None,
                    });
                    AssetId::null()
                }
            };
            layers.push(Layer {
                source: match layer.source {
                    StoredSource::Sf2Zone { file, zone } => Source::Sf2Zone {
                        file: asset_for(file),
                        zone: ZoneId(zone),
                    },
                    StoredSource::Sample { file } => Source::Sample {
                        file: asset_for(file),
                    },
                    StoredSource::Oscillator(kind) => Source::Oscillator(kind),
                    StoredSource::Drum(voice) => Source::Drum(voice),
                    StoredSource::Synth(osc) => Source::Synth(osc),
                },
                key_range: layer.key_range,
                vel_range: layer.vel_range,
                root_key: layer.root_key,
                fine_tune_cents: layer.fine_tune_cents,
                playback: layer.playback,
                gain_db: layer.gain_db,
                pan: layer.pan,
            });
        }

        let mut patch = Patch {
            layers,
            filters: stored.filters,
            envelopes: stored.envelopes,
            lfos: stored.lfos,
            mod_matrix: stored.mod_matrix,
            voice_config: stored.voice_config,
            fx: stored.fx,
            macros: macros_from_stored(stored.macros),
            output_db: stored.output_db,
            wavetables: stored
                .wavetables
                .into_iter()
                .map(StoredWavetable::into_table)
                .collect(),
            samples: stored
                .samples
                .into_iter()
                .map(StoredSample::into_sample)
                .collect(),
            oversampling: stored.oversampling,
        };
        // A Flopsynth row gets every slot the strip shows, whatever the
        // file carried — see `fill_modulator_slots`; the writer above trims
        // them back, so the file does not move.
        patch.fill_modulator_slots();
        Ok(LoadedPatch { patch, unresolved })
    }
}

/// Every sample a stored patch points at, without deserialising it into a
/// live [`Patch`].
///
/// What reopening a project needs before it can resolve anything: the loader
/// has to know which files to read, and it cannot ask a `Patch` because there
/// is no `Patch` until the files are read. Also what an "export bundle"
/// (§17.1) has to walk.
pub fn referenced_samples(data: &PatchData) -> Result<Vec<SampleRef>, PatchFormatError> {
    if data.format_version > PATCH_FORMAT_VERSION {
        return Err(PatchFormatError::FromTheFuture {
            found: data.format_version,
            newest: PATCH_FORMAT_VERSION,
        });
    }
    let body = migrate(data.body.clone(), data.format_version)?;
    let stored: StoredPatch =
        serde_json::from_value(body).map_err(|e| PatchFormatError::Malformed(e.to_string()))?;
    Ok(stored
        .layers
        .into_iter()
        .filter_map(|layer| match layer.source {
            StoredSource::Sf2Zone { file, .. } | StoredSource::Sample { file } => file,
            StoredSource::Oscillator(_) | StoredSource::Drum(_) | StoredSource::Synth(_) => None,
        })
        .collect())
}

/// Brings a body written by an older build up to [`PATCH_FORMAT_VERSION`].
///
/// One step per revision, in order, each rewriting the body from `N` to
/// `N + 1`:
///
/// ```text
/// if version == 0 { body = v0_to_v1(body)?; version = 1; }
/// if version == 1 { body = v1_to_v2(body)?; version = 2; }
/// ```
///
/// There is nothing before v0, so the chain is empty today and any version
/// that is not the current one has no route forward. The shape is written down
/// because the first migration is the one most likely to be added under time
/// pressure, and it should be a fill-in-the-blank rather than a design
/// decision made in a hurry.
///
/// Only ever called with `from <= PATCH_FORMAT_VERSION`; a newer version is
/// refused by [`Patch::from_data`] before it gets here, so that a file this
/// build is too old for reads as "upgrade Fontelle" rather than as damage.
fn migrate(mut body: serde_json::Value, from: u32) -> Result<serde_json::Value, PatchFormatError> {
    let mut version = from;
    if version == 0 {
        body = v0_to_v1(body)?;
        version = 1;
    }
    if version != PATCH_FORMAT_VERSION {
        return Err(PatchFormatError::Malformed(format!(
            "no migration from patch format version {from} to {PATCH_FORMAT_VERSION}"
        )));
    }
    Ok(body)
}

/// v0 → v1: `Lfo::shape: OscKind` became `Lfo::wave: LfoWave`.
///
/// The only change that needs one. Everything else this version added is
/// `#[serde(default)]`, and the v1 reader with its defaults **is** the v0
/// reader — which is the property `tests/patch_format.rs` measures by reading
/// a hand-written v0 body and a v1 body and asserting they come back equal.
///
/// The five shapes map to the five waves that mean the same thing, except
/// `Noise`, which has no counterpart: an LFO running a noise oscillator was
/// producing a new random value every block, and the wave that does that is
/// `SampleHold`. Naming it anything else would silently change what an
/// existing patch sounds like.
fn v0_to_v1(mut body: serde_json::Value) -> Result<serde_json::Value, PatchFormatError> {
    let Some(lfos) = body.get_mut("lfos").and_then(|v| v.as_array_mut()) else {
        // A body with no LFO array at all is either an empty patch or one this
        // reader will refuse below for its own reasons; either way there is
        // nothing here to rename.
        return Ok(body);
    };
    for lfo in lfos {
        let Some(object) = lfo.as_object_mut() else {
            continue;
        };
        let Some(shape) = object.remove("shape") else {
            continue;
        };
        let wave = match shape.as_str() {
            Some("Sine") => "Sine",
            Some("Triangle") => "Triangle",
            Some("Saw") => "SawUp",
            Some("Square") => "Square",
            Some("Noise") => "SampleHold",
            _ => {
                return Err(PatchFormatError::Malformed(format!(
                    "an LFO in a version 0 patch has an unreadable shape: {shape}"
                )));
            }
        };
        object.insert("wave".to_string(), serde_json::Value::from(wave));
    }
    Ok(body)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use fontelle_types::{AssetKind, AssetRef};

    use super::*;
    use crate::mod_matrix::{Curve, ModDest, ModRoute, ModSource};
    use crate::playback::LoopMode;
    use crate::sampler::{PrepareContext, Sampler};
    use crate::streaming::{SampleBuffer, SampleStore};
    use crate::voice::{RetriggerMode, StealPolicy, UnisonConfig};
    use fontelle_dsp::{EnvelopeCurve, Interpolation, SvfMode};

    const SR: f32 = 48_000.0;

    fn a_file() -> AssetRef {
        AssetRef::unregistered(
            std::path::PathBuf::from("/soundfonts/Example.sf2"),
            0x0123_4567_89ab_cdef,
            325_000_000,
            AssetKind::Sf2,
        )
    }

    /// A ramp rather than a constant: a position error inside the round trip
    /// (a lost start offset, a lost loop point) is silent on a flat buffer and
    /// obvious on a slope.
    fn ramp(len: usize) -> SampleBuffer {
        SampleBuffer {
            data: Arc::from(
                (0..len)
                    .map(|i| i as f32 / len as f32)
                    .collect::<Vec<f32>>(),
            ),
            sample_rate: SR as u32,
        }
    }

    /// A patch with **no field left at its default**, so a field the format
    /// drops shows up as a difference rather than as a coincidence.
    fn a_thoroughly_non_default_patch(asset: AssetId) -> Patch {
        Patch {
            layers: vec![
                Layer {
                    source: Source::Sample { file: asset },
                    key_range: (24, 96),
                    vel_range: (7, 120),
                    root_key: 57,
                    fine_tune_cents: -13.5,
                    playback: PlaybackConfig {
                        start_offset: 3.0,
                        end_offset: 400.0,
                        loop_mode: LoopMode::Forward,
                        loop_start: 100.0,
                        loop_end: 380.0,
                        loop_crossfade_ms: 12.5,
                        reverse: true,
                        interpolation: Some(Interpolation::High),
                    },
                    gain_db: -4.25,
                    pan: -0.75,
                },
                Layer {
                    source: Source::Oscillator(OscKind::Square),
                    key_range: (0, 23),
                    vel_range: (1, 127),
                    root_key: 36,
                    fine_tune_cents: 7.0,
                    playback: PlaybackConfig::default(),
                    gain_db: 1.5,
                    pan: 0.25,
                },
            ],
            filters: [
                FilterSlot {
                    mode: SvfMode::Bell,
                    cutoff_hz: 812.5,
                    resonance: 2.75,
                    enabled: true,
                    ..Default::default()
                },
                FilterSlot {
                    mode: SvfMode::HighShelf,
                    cutoff_hz: 6_400.0,
                    resonance: 0.9,
                    enabled: false,
                    ..Default::default()
                },
            ],
            envelopes: vec![EnvelopeConfig {
                delay_s: 0.01,
                attack_s: 0.02,
                hold_s: 0.03,
                decay_s: 1.5,
                sustain_level: 0.4,
                release_s: 0.6,
                curve: EnvelopeCurve::Decibel,
                ..Default::default()
            }],
            lfos: vec![Lfo {
                rate_hz: 5.5,
                depth: 0.3,
                wave: fontelle_types::LfoWave::Triangle,
                delay_s: 0.25,
                ..Default::default()
            }],
            mod_matrix: ModMatrix {
                routes: vec![ModRoute {
                    source: ModSource::Lfo(0),
                    destination: ModDest::LayerPitch(1),
                    depth: -0.125,
                    curve: Curve::Quantised { steps: 12 },
                    via: Some(ModSource::ModWheel),
                    invert: true,
                    bypass: false,
                }],
            },
            voice_config: VoiceConfig {
                polyphony: 33,
                steal_policy: StealPolicy::Quietest,
                glide_time_s: 0.05,
                glide_legato_only: true,
                unison: UnisonConfig {
                    voices: 3,
                    detune_cents: 9.0,
                    spread: 0.6,
                    randomise_phase: true,
                },
                retrigger: RetriggerMode::Legato,
                // Not the default, so the round trip proves the field
                // travels rather than that both ends guessed the same.
                bend_range_semitones: 7.0,
            },
            ..Default::default()
        }
    }

    /// Renders one note through a patch, so two patches can be compared by
    /// what they sound like rather than by what they claim.
    fn render_a_note(patch: Patch, store: &SampleStore) -> Vec<f32> {
        let mut sampler = Sampler::new(patch);
        sampler.prepare(&PrepareContext {
            sample_rate: SR,
            max_block_size: 256,
        });
        sampler.note_on(60, 100, 0);
        let mut left = vec![0.0; 256];
        let mut right = vec![0.0; 256];
        {
            let mut out: Vec<&mut [f32]> = vec![&mut left, &mut right];
            sampler.render(store, &mut out);
        }
        left.extend_from_slice(&right);
        left
    }

    #[test]
    fn a_round_tripped_patch_renders_sample_for_sample_identically() {
        let mut store = SampleStore::new();
        let asset = store.insert(ramp(400));
        let patch = a_thoroughly_non_default_patch(asset);
        let provenance = HashMap::from([(
            asset,
            SampleRef {
                file: a_file(),
                sample: 4,
            },
        )]);

        let data = patch.to_data(&provenance).expect("a patch must serialise");
        let loaded = Patch::from_data(&data, |_| Some(asset)).expect("and read back");
        assert!(loaded.unresolved.is_empty());

        let before = render_a_note(patch, &store);
        let after = render_a_note(loaded.patch, &store);
        assert_eq!(before, after, "the round trip changed how the patch sounds");
    }

    #[test]
    fn every_field_survives_the_round_trip() {
        // Rendering one note cannot see `polyphony`, a steal policy, or a
        // route aimed at a layer the note does not reach. A field silently
        // dropped by the format is a preset that loads subtly wrong months
        // later, so the whole structure is compared, not just its sound.
        let mut store = SampleStore::new();
        let asset = store.insert(ramp(400));
        let patch = a_thoroughly_non_default_patch(asset);
        let provenance = HashMap::from([(
            asset,
            SampleRef {
                file: a_file(),
                sample: 4,
            },
        )]);

        let data = patch.to_data(&provenance).unwrap();
        let loaded = Patch::from_data(&data, |_| Some(asset)).unwrap();

        assert_eq!(patch, loaded.patch);
    }

    #[test]
    fn a_layers_sample_is_named_by_its_file_not_by_the_key_the_store_minted() {
        // INVARIANT 8, and the practical half of it: the same soundfont
        // decoded into a different `SampleStore` gets different `AssetId`s, so
        // a patch that stored the key would load pointing at nothing — or,
        // worse, at whatever else happened to land on that key.
        let mut writing_store = SampleStore::new();
        let asset = writing_store.insert(ramp(400));
        let patch = a_thoroughly_non_default_patch(asset);
        let sample_ref = SampleRef {
            file: a_file(),
            sample: 4,
        };
        let data = patch
            .to_data(&HashMap::from([(asset, sample_ref.clone())]))
            .unwrap();

        // A different store, with the same audio at a different key.
        let mut reading_store = SampleStore::new();
        let _decoy = reading_store.insert(SampleBuffer {
            data: Arc::from(vec![0.0; 8]),
            sample_rate: SR as u32,
        });
        let elsewhere = reading_store.insert(ramp(400));
        assert_ne!(asset, elsewhere, "the two stores must disagree on the key");

        let mut asked = Vec::new();
        let loaded = Patch::from_data(&data, |r| {
            asked.push(r.clone());
            Some(elsewhere)
        })
        .unwrap();

        assert_eq!(asked, vec![sample_ref], "the file is what was looked up");
        assert_eq!(
            render_a_note(patch, &writing_store),
            render_a_note(loaded.patch, &reading_store),
            "the same audio through a different store must sound the same"
        );
    }

    #[test]
    fn an_unresolvable_sample_loads_as_a_silent_layer_and_is_reported() {
        // TDD §17.4: the project loads and plays with placeholders. It does
        // not refuse to open, and it does not lose the reference.
        let mut store = SampleStore::new();
        let asset = store.insert(ramp(400));
        let patch = a_thoroughly_non_default_patch(asset);
        let sample_ref = SampleRef {
            file: a_file(),
            sample: 4,
        };
        let data = patch
            .to_data(&HashMap::from([(asset, sample_ref.clone())]))
            .unwrap();

        let loaded = Patch::from_data(&data, |_| None).expect("a broken link is not a load error");

        assert_eq!(
            loaded.unresolved,
            vec![UnresolvedSample {
                layer: 0,
                sample: Some(sample_ref),
            }]
        );
        let empty = SampleStore::new();
        assert!(
            render_a_note(loaded.patch, &empty)
                .iter()
                .all(|s| *s == 0.0),
            "a layer with no audio must be silent, not a panic"
        );
    }

    #[test]
    fn a_sample_with_no_provenance_is_written_and_reported_as_such() {
        // A layer whose audio never came from a file — a future recording, a
        // generated buffer. Writing it as if it had a file would be a lie the
        // relink dialog then chases.
        let mut store = SampleStore::new();
        let asset = store.insert(ramp(400));
        let patch = a_thoroughly_non_default_patch(asset);

        let data = patch.to_data(&HashMap::new()).unwrap();
        let loaded = Patch::from_data(&data, |_| unreachable!("nothing to resolve")).unwrap();

        assert_eq!(
            loaded.unresolved,
            vec![UnresolvedSample {
                layer: 0,
                sample: None,
            }]
        );
    }

    #[test]
    fn the_format_version_is_stamped_on_every_write() {
        let patch = Patch {
            layers: Vec::new(),
            filters: [
                FilterSlot {
                    mode: SvfMode::Lowpass,
                    cutoff_hz: 20_000.0,
                    resonance: 0.7,
                    enabled: false,
                    ..Default::default()
                },
                FilterSlot {
                    mode: SvfMode::Lowpass,
                    cutoff_hz: 20_000.0,
                    resonance: 0.7,
                    enabled: false,
                    ..Default::default()
                },
            ],
            envelopes: Vec::new(),
            lfos: Vec::new(),
            mod_matrix: ModMatrix::default(),
            voice_config: VoiceConfig::default(),
            ..Default::default()
        };
        let data = patch.to_data(&HashMap::new()).unwrap();
        assert_eq!(data.format_version, PATCH_FORMAT_VERSION);
    }

    #[test]
    fn a_patch_from_a_newer_build_is_refused_by_version_naming_both() {
        let mut store = SampleStore::new();
        let asset = store.insert(ramp(400));
        let mut data = a_thoroughly_non_default_patch(asset)
            .to_data(&HashMap::new())
            .unwrap();
        data.format_version = PATCH_FORMAT_VERSION + 7;

        let err = Patch::from_data(&data, |_| None).expect_err("a future version must be refused");
        assert_eq!(
            err,
            PatchFormatError::FromTheFuture {
                found: PATCH_FORMAT_VERSION + 7,
                newest: PATCH_FORMAT_VERSION,
            }
        );
        // The message has to say what to do about it, not just that it failed.
        let message = err.to_string();
        assert!(message.contains(&(PATCH_FORMAT_VERSION + 7).to_string()));
        assert!(message.contains("upgrade"));
    }

    #[test]
    fn a_malformed_body_fails_with_a_message_rather_than_panicking() {
        // TDD §20.3's standard, applied to our own format.
        let data = PatchData {
            format_version: PATCH_FORMAT_VERSION,
            body: serde_json::json!({ "layers": "not a list of layers" }),
        };
        let err = Patch::from_data(&data, |_| None).expect_err("nonsense must not parse");
        assert!(matches!(err, PatchFormatError::Malformed(_)));
        assert!(!err.to_string().is_empty());
    }

    #[test]
    fn a_version_with_no_migration_chain_says_so_rather_than_parsing_anyway() {
        // The entry point itself, since no released version is old enough to
        // reach it through `from_data` yet. A body from a version this build
        // has no route from must not be handed to `serde` on the assumption
        // that the shape happens to still fit.
        let err = migrate(serde_json::json!({}), PATCH_FORMAT_VERSION + 1)
            .expect_err("an unknown version has no chain");
        assert!(matches!(err, PatchFormatError::Malformed(_)));
        assert!(err.to_string().contains("migration"));
    }

    #[test]
    fn the_body_is_readable_json_rather_than_a_byte_array() {
        // TDD §17.2 chose JSON so a project is "diffable, inspectable,
        // greppable, and recoverable by hand". A patch serialised as bytes
        // inside it would be none of those, which is what the previous
        // `Vec<u8>` gave.
        let mut store = SampleStore::new();
        let asset = store.insert(ramp(400));
        let data = a_thoroughly_non_default_patch(asset)
            .to_data(&HashMap::from([(
                asset,
                SampleRef {
                    file: a_file(),
                    sample: 4,
                },
            )]))
            .unwrap();

        let text = serde_json::to_string_pretty(&data).unwrap();
        assert!(text.contains("Example.sf2"), "the file should be greppable");
        assert!(text.contains("root_key"), "fields should be named");
    }
}
