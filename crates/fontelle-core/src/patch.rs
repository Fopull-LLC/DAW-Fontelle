use fontelle_types::AssetId;

use crate::mod_matrix::ModMatrix;
use crate::playback::PlaybackConfig;
use crate::voice::VoiceConfig;
use fontelle_dsp::{EnvelopeConfig, OscKind};

/// A zone/preset identifier inside an SF2 file, assigned by `fontelle-assets` at
/// import time. Opaque here — `fontelle-core` never parses SF2 metadata itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZoneId(pub u32);

#[derive(Debug, Clone)]
pub enum Source {
    Sf2Zone { file: AssetId, zone: ZoneId },
    Sample { file: AssetId },
    Oscillator(OscKind),
}

#[derive(Debug, Clone)]
pub struct Layer {
    pub source: Source,
    pub key_range: (u8, u8),
    pub vel_range: (u8, u8),
    pub root_key: u8,
    pub fine_tune_cents: f32,
    pub playback: PlaybackConfig,
    pub gain_db: f32,
    pub pan: f32,
}

/// A named LFO instance. Depth/rate are mod-matrix destinations (TDD §7.5).
#[derive(Debug, Clone, Copy)]
pub struct Lfo {
    pub rate_hz: f32,
    pub depth: f32,
    pub shape: fontelle_dsp::OscKind,
}

#[derive(Debug, Clone, Copy)]
pub struct FilterSlot {
    pub mode: fontelle_dsp::SvfMode,
    pub cutoff_hz: f32,
    pub resonance: f32,
    pub enabled: bool,
}

/// The user's fully-owned instrument definition (TDD §7.2). An SF2 file seeds this
/// once at import; after that it has no live link back to the file's metadata.
#[derive(Debug, Clone)]
pub struct Patch {
    /// Up to 16 layers: stacked, or split by key/velocity.
    pub layers: Vec<Layer>,
    pub filters: [FilterSlot; 2],
    /// At least amp + mod envelopes; the user may add more.
    pub envelopes: Vec<EnvelopeConfig>,
    pub lfos: Vec<Lfo>,
    pub mod_matrix: ModMatrix,
    pub voice_config: VoiceConfig,
}
