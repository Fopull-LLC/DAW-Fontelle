use fontelle_dsp::Interpolation;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum LoopMode {
    Off,
    Forward,
    PingPong,
    Sustain,
    Release,
}

/// Every field here is seeded from the SF2 zone at import and every field is
/// user-editable afterward (TDD §7.3) — this is where Fontelle inverts the usual
/// SF2-player relationship: the file supplies defaults, not the final word.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PlaybackConfig {
    pub start_offset: f64,
    pub end_offset: f64,
    pub loop_mode: LoopMode,
    pub loop_start: f64,
    pub loop_end: f64,
    /// Not an SF2 concept. Equal-power crossfade over the loop boundary — the single
    /// highest-value fix for "soundfonts sound clicky" (TDD §7.3, §7.8.1).
    pub loop_crossfade_ms: f32,
    pub reverse: bool,
    /// `None` — the default, and what SF2 import produces, since the format has
    /// no interpolation generator — means this layer follows the session's
    /// playback or render quality (TDD §7.6). `Some` pins the layer to one
    /// kernel, honoured in playback and export alike: `Draft`'s aliasing is a
    /// legitimate character choice in a sampler, so a pinned layer is never
    /// silently upgraded for a bounce.
    pub interpolation: Option<Interpolation>,
    /// How many velocities inside each end of the layer's window the layer
    /// fades over (`docs/flopsynth-next.md` §4.2): at the edge it is
    /// silent, this far in it is whole. Nought — no fade, the switch every
    /// zone had — is left out of the file.
    #[serde(default, skip_serializing_if = "is_zero_u8")]
    pub vel_fade: u8,
}

fn is_zero_u8(value: &u8) -> bool {
    *value == 0
}

impl Default for PlaybackConfig {
    fn default() -> Self {
        Self {
            start_offset: 0.0,
            end_offset: 0.0,
            loop_mode: LoopMode::Off,
            loop_start: 0.0,
            loop_end: 0.0,
            loop_crossfade_ms: 5.0,
            reverse: false,
            interpolation: None,
            vel_fade: 0,
        }
    }
}
