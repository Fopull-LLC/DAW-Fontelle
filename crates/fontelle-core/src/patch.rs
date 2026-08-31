use fontelle_types::AssetId;

use crate::mod_matrix::ModMatrix;
use crate::playback::PlaybackConfig;
use crate::voice::VoiceConfig;
use fontelle_dsp::{EnvelopeConfig, OscKind};

/// A zone/preset identifier inside an SF2 file, assigned by `fontelle-assets` at
/// import time. Opaque here — `fontelle-core` never parses SF2 metadata itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ZoneId(pub u32);

#[derive(Debug, Clone, PartialEq)]
pub enum Source {
    Sf2Zone { file: AssetId, zone: ZoneId },
    Sample { file: AssetId },
    Oscillator(OscKind),
}

#[derive(Debug, Clone, PartialEq)]
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
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Lfo {
    pub rate_hz: f32,
    pub depth: f32,
    pub shape: fontelle_dsp::OscKind,
    /// How long after note-on before the LFO starts, in seconds.
    ///
    /// Not cosmetic: vibrato that begins on the note's first sample is the
    /// single most recognisable way a sampled string section sounds synthetic.
    /// Every real player leans into it, and SF2 has a generator
    /// (`delayVibLFO`) for exactly this.
    pub delay_s: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FilterSlot {
    pub mode: fontelle_dsp::SvfMode,
    pub cutoff_hz: f32,
    pub resonance: f32,
    pub enabled: bool,
}

/// The user's fully-owned instrument definition (TDD §7.2). An SF2 file seeds this
/// once at import; after that it has no live link back to the file's metadata.
#[derive(Debug, Clone, PartialEq)]
pub struct Patch {
    /// The zones, stacked or split by key/velocity.
    ///
    /// **There is no limit here, and there deliberately is not.** A drum kit
    /// is one zone per hit and real ones run to forty-odd — `Nokia_30.sf2`'s
    /// kit has 47. `voice::MAX_LAYERS` bounds how many may sound *at once*
    /// (TDD §7.4), which is a different quantity: a key split is not a stack,
    /// and only the zones covering the note being played take a slot.
    ///
    /// This once said "up to 16", and the voice enforced it against the wrong
    /// thing — it zipped its sixteen slots against this list, so every zone
    /// past index 15 was silently unreachable. That is not a limit anybody
    /// chose; it is `zip` stopping at the shorter side, and it cost three of
    /// the kits in the development bank 30, 37 and 68 keys apiece.
    pub layers: Vec<Layer>,
    pub filters: [FilterSlot; 2],
    /// At least amp + mod envelopes; the user may add more.
    pub envelopes: Vec<EnvelopeConfig>,
    pub lfos: Vec<Lfo>,
    pub mod_matrix: ModMatrix,
    pub voice_config: VoiceConfig,
}
