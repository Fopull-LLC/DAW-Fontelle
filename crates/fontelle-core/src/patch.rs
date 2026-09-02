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

/// The **blank instrument**: three oscillators, an amplitude envelope with a
/// little shape on it, and a filter you can open.
///
/// This is what a channel plays when nobody has chosen a soundfont for it, and
/// it exists because of what adding a channel used to feel like — *"when
/// clicking new instrument, right now it doesnt do anything until i select an
/// instrument in the soundfonts tab... then it actually happening later when
/// you werent intending"*. A channel that plays nothing is a channel whose
/// every control does nothing, so the button that makes one looked broken and
/// then looked haunted.
///
/// The choices in it are the ones a default patch has to get right:
///
/// - **A saw, a square and a sine**, in that order, because those are the three
///   a person reaches for and having all three already there is what makes the
///   panel worth opening. The square is an octave down and the sine two, so the
///   three of them are a shape rather than a chorus — and the saw is detuned a
///   few cents against nothing at all, which does nothing on its own and is the
///   knob somebody will find.
/// - **Only the saw is up.** Three oscillators at full level is a default that
///   clips the moment anybody plays a chord; the other two are down at the
///   bottom of their travel, ready to be brought in.
/// - **Headroom.** The saw sits well down, so that a four-note chord at full
///   velocity is still inside full scale with room for the other two
///   oscillators to be brought up. The master's brickwall limiter would catch
///   the overshoot, but a default patch that spends its life in the limiter is
///   a default patch that sounds squashed and nobody can tell why.
/// - **A short attack and release**, not zero: a hard-gated square is a click
///   at each end, and "why does it tick" is not a first impression worth
///   giving.
impl Patch {
    pub fn basic_synth() -> Self {
        let osc = |kind: fontelle_dsp::OscKind, root_key: u8, gain_db: f32, cents: f32| Layer {
            source: Source::Oscillator(kind),
            // Every key, always: an oscillator has no recorded range to run
            // out of, and a blank instrument with holes in the keyboard would
            // be a strange thing to hand somebody.
            key_range: (0, 127),
            vel_range: (0, 127),
            // Middle C, so a note plays its own pitch — see
            // `crate::voice::OSC_ROOT_HZ`. A root an octave *above* this plays
            // an octave down, which is how the sub oscillators are tuned.
            root_key,
            fine_tune_cents: cents,
            playback: PlaybackConfig::default(),
            gain_db,
            pan: 0.0,
        };
        Self {
            layers: vec![
                osc(fontelle_dsp::OscKind::Saw, 60, -14.0, 0.0),
                osc(fontelle_dsp::OscKind::Square, 72, SILENT_DB, 0.0),
                osc(fontelle_dsp::OscKind::Sine, 84, SILENT_DB, 0.0),
            ],
            filters: [
                FilterSlot {
                    mode: fontelle_dsp::SvfMode::Lowpass,
                    // Wide open. A default patch that is already filtered is
                    // one whose cutoff knob only ever brightens by undoing a
                    // choice nobody made.
                    cutoff_hz: 20_000.0,
                    resonance: 0.7,
                    enabled: true,
                },
                FilterSlot {
                    mode: fontelle_dsp::SvfMode::Highpass,
                    cutoff_hz: 20.0,
                    resonance: 0.7,
                    enabled: false,
                },
            ],
            // The amp envelope, then one spare for the matrix to route — the
            // same two every imported patch has.
            envelopes: vec![
                fontelle_dsp::EnvelopeConfig {
                    delay_s: 0.0,
                    attack_s: 0.005,
                    hold_s: 0.0,
                    decay_s: 0.0,
                    sustain_level: 1.0,
                    release_s: 0.12,
                    curve: fontelle_dsp::EnvelopeCurve::Decibel,
                },
                fontelle_dsp::EnvelopeConfig {
                    delay_s: 0.0,
                    attack_s: 0.0,
                    hold_s: 0.0,
                    decay_s: 0.3,
                    sustain_level: 0.0,
                    release_s: 0.1,
                    curve: fontelle_dsp::EnvelopeCurve::Decibel,
                },
            ],
            lfos: Vec::new(),
            mod_matrix: crate::mod_matrix::ModMatrix::default(),
            voice_config: crate::voice::VoiceConfig::default(),
        }
    }
}

/// The level a layer is **off** at.
///
/// A number rather than an `Option`, because a switched-off oscillator is one
/// whose level knob is at the bottom of its travel and not one that has been
/// removed from the patch — turn it up and it comes back. Sixty decibels down
/// is a thousandth of the amplitude: inaudible under anything, and a real
/// value that serialises, which `-inf` is not (JSON has no infinity, and a
/// patch that saved as `null` would not open).
pub const SILENT_DB: f32 = -60.0;
