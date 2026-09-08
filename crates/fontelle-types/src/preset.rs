//! A preset, for every device (`docs/flopsynth-plan.md` §P).
//!
//! > *"a preset system kind of like FL Studio's baked into the DAW itself that
//! > works for every instrument and effect so we don't have to hardcode presets
//! > in every plugin ... when you have a `*` for unsaved edits you're able to
//! > save it either to the same preset or save as to a new preset in your
//! > bank."* — Ty, 2026-09-06
//!
//! # The whole idea, in one paragraph
//!
//! A **preset is a file**: a name, a category, the device it is for, and that
//! device's own saved state. *Factory* presets are such files embedded in the
//! binary at build time; *user* presets are such files in a bank folder the
//! user owns. Every device — a built-in instrument, a built-in effect, a
//! hosted plugin — gets the same preset bar in its window, the same rows in
//! the browser's Presets tab, the same favourites and the same undo. What a
//! device contributes is nothing but *what its state is*; the system does the
//! rest.
//!
//! # Why it costs nothing per device
//!
//! [`PresetPayload`] invents no new shape. `PatchData` is what a channel
//! already stores, `EffectConfig` what an insert stores, `PluginState` what
//! either stores for a plugin. A device's state is already a serialisable
//! value the document holds, so saving one is writing that value to a file
//! with a name on it — which is why a preset system for *every* device is a
//! type and a folder walk rather than a feature per plugin.
//!
//! # Why it is in `fontelle-types`
//!
//! For the reason [`crate::Favorite`] is: the app writes the files, the window
//! draws the bar, the model stores which preset a channel came from, and none
//! of those three may depend on the others (§4.1).

use crate::{EffectConfig, EffectKind, InstrumentKind, PatchData, PluginKey, PluginState};

/// The revision of the preset file format this build writes.
///
/// Bumped when a change cannot be read by serde's own defaulting, on exactly
/// the reasoning [`fontelle_core::patch_format::PATCH_FORMAT_VERSION`] gives.
/// A file claiming a higher version is refused as *from the future* rather
/// than parsed into nonsense — see [`Preset::is_from_the_future`].
pub const PRESET_FORMAT_VERSION: u32 = 1;

/// Which device a preset is for.
///
/// Its [`slug`](DeviceKind::slug) is a **folder name**, and therefore
/// INVARIANT 7's: renaming one moves everybody's presets, so
/// `fontelle-types/tests/preset.rs` freezes the table with the literal strings
/// in it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DeviceKind {
    Instrument(InstrumentKind),
    Effect(EffectKind),
    /// A hosted plugin, by the id it declares rather than the path it was
    /// found at — INVARIANT 8's reasoning, so a preset for a plugin survives
    /// that plugin being reinstalled somewhere else.
    Plugin(PluginKey),
}

impl DeviceKind {
    /// The folder this device's presets live in, under both the factory tree
    /// and the user's bank.
    ///
    /// Effects are prefixed `fx-` so that an instrument and an effect that
    /// share a name — Filter is both a `SvfMode` and an insert — can never
    /// share a folder.
    pub fn slug(&self) -> String {
        match self {
            Self::Instrument(kind) => match kind {
                InstrumentKind::SoundFont => "soundfont".to_string(),
                InstrumentKind::Osc3 => "3osc".to_string(),
                InstrumentKind::Sampler => "sampler".to_string(),
                InstrumentKind::DrumMachine => "drum-machine".to_string(),
                InstrumentKind::Flopsynth => "flopsynth".to_string(),
                // The odd one out, and it has to be: a channel of this kind
                // says its instrument is named somewhere else. A preset for
                // an actual plugin is `Self::Plugin`, which names which.
                InstrumentKind::Plugin => "plugin".to_string(),
            },
            Self::Effect(kind) => format!(
                "fx-{}",
                match kind {
                    EffectKind::Utility => "utility",
                    EffectKind::Eq => "eq",
                    EffectKind::Filter => "filter",
                    EffectKind::Compressor => "compressor",
                    EffectKind::Gate => "gate",
                    EffectKind::Distortion => "distortion",
                    EffectKind::Bitcrush => "bitcrush",
                    EffectKind::Soften => "soften",
                    EffectKind::Chorus => "chorus",
                    EffectKind::Delay => "delay",
                    EffectKind::Reverb => "reverb",
                }
            ),
            // A plugin id is a reverse-domain name or a URI and may carry
            // anything the vendor liked, including slashes and colons. Every
            // character that is not portable in a folder name becomes `-`, so
            // the slug is one folder whatever the id was.
            Self::Plugin(key) => {
                let mut slug = format!("plugin-{}-", key.format.extension());
                for ch in key.id.chars() {
                    if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '.' | '_') {
                        slug.push(ch);
                    } else {
                        slug.push('-');
                    }
                }
                slug
            }
        }
    }

    /// What the browser's row and the bar's tooltip call this device.
    pub fn label(&self) -> String {
        match self {
            Self::Instrument(kind) => kind.label().to_string(),
            Self::Effect(kind) => kind.label().to_string(),
            Self::Plugin(key) => key.id.clone(),
        }
    }
}

/// The device's own saved state — the three shapes the document already
/// stores, and deliberately no fourth.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PresetPayload {
    Patch(PatchData),
    Effect(EffectConfig),
    Plugin(PluginState),
}

/// One preset file.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Preset {
    pub format_version: u32,
    pub device: DeviceKind,
    pub name: String,
    pub category: String,
    pub payload: PresetPayload,
}

impl Preset {
    /// A preset of `device`, at the current format version.
    pub fn new(
        device: DeviceKind,
        name: impl Into<String>,
        category: impl Into<String>,
        payload: PresetPayload,
    ) -> Self {
        Self {
            format_version: PRESET_FORMAT_VERSION,
            device,
            name: name.into(),
            category: category.into(),
            payload,
        }
    }

    /// Whether the payload is the shape this device stores.
    ///
    /// The bank lists a file that fails this as *unreadable* rather than
    /// loading it: an effect's settings applied to an instrument is not a
    /// degraded preset, it is a preset for something else.
    pub fn is_consistent(&self) -> bool {
        match (&self.device, &self.payload) {
            // A plugin *instrument* stores a `PluginState`, not a patch —
            // which is the one place the two enums do not line up by name.
            (DeviceKind::Instrument(InstrumentKind::Plugin), PresetPayload::Plugin(_)) => true,
            (DeviceKind::Instrument(_), PresetPayload::Patch(_)) => true,
            (DeviceKind::Effect(kind), PresetPayload::Effect(config)) => config.kind() == *kind,
            (DeviceKind::Plugin(key), PresetPayload::Plugin(state)) => state.key == *key,
            _ => false,
        }
    }

    /// Whether this file was written by a build newer than this one.
    pub fn is_from_the_future(&self) -> bool {
        self.format_version > PRESET_FORMAT_VERSION
    }
}

/// Whether a preset came with the program or from the user's own bank.
///
/// Two origins rather than a flag, because they behave differently in exactly
/// one way that matters everywhere: a factory preset is read-only, so "Save"
/// is disabled on one and "Save as…" is not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PresetOrigin {
    Factory,
    User,
}

/// What a device remembers about the preset it was loaded from.
///
/// **The name is remembered; the cleanliness is recognised.** A device carries
/// this through every edit and never stores whether it is dirty — that is
/// computed by comparing the device's current payload against the bank's for
/// this ref, so an undo makes the `*` go out with nothing to remember. See
/// §P.6, which is Ty's replacement for the second half of the effects
/// catalogue's rule 10.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct PresetRef {
    pub name: String,
    pub category: String,
    pub origin: PresetOrigin,
}

impl PresetRef {
    pub fn new(name: impl Into<String>, category: impl Into<String>, origin: PresetOrigin) -> Self {
        Self {
            name: name.into(),
            category: category.into(),
            origin,
        }
    }
}
