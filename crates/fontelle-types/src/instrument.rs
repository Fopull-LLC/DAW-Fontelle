//! Which of the three instruments a channel plays.
//!
//! Reported from using the window:
//!
//! > *"we should make sure there is an actual instrument selection menu and
//! > when you select new instrument it lets you select one of those and then
//! > you actually edit it from there how you want instead of how it is right
//! > now where is basically makes everything an oscillator and then i click a
//! > soundfont in the soundfonts menu to change it which is just weird."*
//!
//! **Why this is stored rather than read off the patch.** A
//! `fontelle_core::Patch` is a list of layers, and each layer's `Source` says
//! whether it is a soundfont zone, a sample or an oscillator — so for a patch
//! with something in it the kind can be derived. It cannot be derived for an
//! *empty* one, and empty is a real state: a sampler with no sample and a
//! soundfont player with no soundfont are the same patch, and both are where
//! you sit while deciding what to load. So the **choice** is what is kept, and
//! the patch follows from it.
//!
//! In `fontelle-types` because the model stores it, the window draws a menu of
//! it and the app builds a starter patch from it — the same reason every other
//! shared id type is here.

/// One of the three instruments Fontelle can put on a channel.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub enum InstrumentKind {
    /// Plays presets out of an SF2 file — what this program is for (TDD §7).
    #[default]
    SoundFont,
    /// The built-in three-oscillator synth, `fontelle_core::Patch::basic_synth`.
    Osc3,
    /// Plays one audio file across the keyboard.
    Sampler,
    /// The built-in drum machine — `fontelle_core::drum_kit`.
    ///
    /// > *"a built in general purpose drum machine that can just make a
    /// > variety of drum styles and sounds ... should be encorperated like any
    /// > other vst would be."*
    ///
    /// It is the **second** instrument that arrives able to play, and the only
    /// one that never wants a file at all: a kit is thirty-six synthesised
    /// hits on the General MIDI drum map, so it works on a fresh install with
    /// no bank configured.
    DrumMachine,
    /// The built-in wavetable synthesiser — `fontelle_core::flopsynth`.
    ///
    /// > *"a new built in synthesizer plugin. this will be our main synth for
    /// > the daw kind of like how fl studio has flex ... should be an advanced
    /// > synthesizer inspired by the likes of omnisphere and Serum."*
    ///
    /// The **third** instrument that arrives able to play, and the second that
    /// never wants a file: every wavetable it reads is generated from a
    /// spectrum recipe at first use, so a Flopsynth preset works on a fresh
    /// install and can never break a link.
    ///
    /// It is an ordinary [`fontelle_core::Patch`] whose layers carry
    /// `Source::Synth`, which is why save, load, automation, the key map, the
    /// mixer and undo never had to be told it exists — the drum machine's
    /// lesson, taken again.
    Flopsynth,
    /// A plugin somebody else wrote (TDD §8.4).
    ///
    /// The odd one out, and it has to be: the other four *are* the
    /// instrument, and this one only says that the channel's instrument is
    /// named somewhere else — in `Channel::plugin`, by
    /// [`crate::PluginKey`]. Which is the same shape `SoundFont` has, one
    /// level up: choosing the kind is not choosing the sound.
    Plugin,
}

impl InstrumentKind {
    /// In the order a menu offers them: the soundfont player first, because it
    /// is what somebody opened this program to use.
    pub const ALL: [Self; 6] = [
        Self::SoundFont,
        Self::Flopsynth,
        Self::DrumMachine,
        Self::Osc3,
        Self::Sampler,
        Self::Plugin,
    ];

    /// What the menu row says.
    pub fn label(self) -> &'static str {
        match self {
            Self::SoundFont => "SoundFont player",
            Self::Osc3 => "3OSC",
            Self::Sampler => "Sampler",
            Self::DrumMachine => "Drum machine",
            Self::Flopsynth => "Flopsynth",
            Self::Plugin => "Plugin",
        }
    }

    /// What a channel of this kind is called before it is given a name.
    pub fn default_name(self) -> &'static str {
        match self {
            Self::SoundFont => "SoundFont",
            Self::Osc3 => "3OSC",
            Self::Sampler => "Sampler",
            Self::DrumMachine => "Drums",
            Self::Flopsynth => "Flopsynth",
            Self::Plugin => "Plugin",
        }
    }

    /// Whether an instrument of this kind arrives able to make a sound.
    ///
    /// The synth and the drum machine do. The other two are waiting for a
    /// file, and one that arrived playing a saw would be lying about what it
    /// is.
    pub fn plays_on_arrival(self) -> bool {
        matches!(self, Self::Osc3 | Self::DrumMachine | Self::Flopsynth)
    }

    /// What a row of this kind is waiting for, when it is waiting.
    pub fn wants(self) -> Option<&'static str> {
        match self {
            Self::SoundFont => Some("Choose a soundfont"),
            Self::Sampler => Some("Drop a sound on it"),
            Self::Osc3 | Self::DrumMachine | Self::Flopsynth => None,
            Self::Plugin => Some("Choose a plugin"),
        }
    }
}
