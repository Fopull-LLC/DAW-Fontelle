#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StealPolicy {
    Oldest,
    Quietest,
    LowestPriority,
}

#[derive(Debug, Clone, Copy)]
pub struct UnisonConfig {
    pub voices: u8,
    pub detune_cents: f32,
    pub spread: f32,
    pub randomise_phase: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetriggerMode {
    Poly,
    Mono,
    Legato,
}

#[derive(Debug, Clone, Copy)]
pub struct VoiceConfig {
    /// 1..=256.
    pub polyphony: u16,
    pub steal_policy: StealPolicy,
    pub glide_time_s: f32,
    pub glide_legato_only: bool,
    pub unison: UnisonConfig,
    pub retrigger: RetriggerMode,
}

impl Default for VoiceConfig {
    fn default() -> Self {
        Self {
            polyphony: 64,
            steal_policy: StealPolicy::Oldest,
            glide_time_s: 0.0,
            glide_legato_only: false,
            unison: UnisonConfig {
                voices: 1,
                detune_cents: 0.0,
                spread: 0.0,
                randomise_phase: false,
            },
            retrigger: RetriggerMode::Poly,
        }
    }
}

/// One playing note. Fixed-topology (INVARIANT 6): Layers → mix → Filter 1 → Filter 2
/// → Amp → Pan → out, with the mod matrix feeding every stage. Predictable per-voice
/// cost, zero allocation on note-on, no graph compilation on the audio thread.
pub struct Voice {
    active: bool,
    key: u8,
    voice_context: u32,
}

impl Voice {
    pub fn new() -> Self {
        Self {
            active: false,
            key: 0,
            voice_context: 0,
        }
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn key(&self) -> u8 {
        self.key
    }

    pub fn voice_context(&self) -> u32 {
        self.voice_context
    }

    pub fn trigger(&mut self, _patch: &crate::Patch, key: u8, _velocity: u8, voice_context: u32) {
        self.active = true;
        self.key = key;
        self.voice_context = voice_context;
    }

    /// Voice stealing always ramps out over a short release rather than cutting
    /// hard (TDD §7.4) — never a click.
    pub fn release(&mut self) {
        todo!("enter release stage; deactivate once the release ramp completes")
    }

    pub fn render(&mut self, _patch: &crate::Patch, _out: &mut [f32]) {
        todo!("layers -> mix -> filter chain -> amp -> pan, per INVARIANT 6 topology")
    }
}

impl Default for Voice {
    fn default() -> Self {
        Self::new()
    }
}

/// A pre-allocated pool sized to `VoiceConfig::polyphony` at `prepare()` time
/// (TDD §7.4) — no allocation on note-on, ever.
pub struct VoicePool {
    voices: Vec<Voice>,
}

impl VoicePool {
    pub fn with_capacity(capacity: u16) -> Self {
        Self {
            voices: (0..capacity).map(|_| Voice::new()).collect(),
        }
    }

    pub fn active_count(&self) -> usize {
        self.voices.iter().filter(|v| v.is_active()).count()
    }

    /// Finds a free voice, or steals one per the patch's `StealPolicy`.
    pub fn allocate(&mut self, _policy: StealPolicy) -> Option<&mut Voice> {
        todo!("free-voice search, then steal-policy fallback with release ramp")
    }

    pub fn iter_active_mut(&mut self) -> impl Iterator<Item = &mut Voice> {
        self.voices.iter_mut().filter(|v| v.is_active())
    }
}
