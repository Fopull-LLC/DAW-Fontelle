//! A thing somebody starred.
//!
//! > *"make it so that i can favorite (star) plugins, instruments, effects,
//! > etc. so that the favorites are always the most visible."*
//!
//! A favourite is a fact about the **person**, not the project: the same
//! reverb is a favourite in every song. So it lives in the settings file, and
//! the type lives here for the reason [`crate::InstrumentKind`] does — the app
//! stores it, the window draws menus around it, and neither may depend on the
//! other (§4.1).
//!
//! It names things the way the document names them: a built-in effect by its
//! [`EffectKind`], a built-in instrument by its [`InstrumentKind`], and a
//! plugin by its [`PluginKey`] — the id the plugin declares, never the path it
//! was found at, so a starred plugin is still starred after it is reinstalled
//! somewhere else. That is INVARIANT 8's reasoning, one level up.

use crate::{DeviceKind, EffectKind, InstrumentKind, PluginKey, PresetOrigin};

/// One starred thing.
///
/// Written as `{"effect":"Reverb"}`, `{"instrument":"DrumMachine"}` or
/// `{"plugin":"clap:com.u-he.diva"}`: a line somebody can read out of
/// `settings.json` and know what they starred.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Favorite {
    Effect(EffectKind),
    Instrument(InstrumentKind),
    Plugin(PluginKey),
    /// One preset out of the bank, named the way the bank names it: the device
    /// it is for, its name, and which origin it came from.
    ///
    /// The origin is part of the identity because §P.3 lets a user preset
    /// share a name with a factory one — they are two different files and two
    /// different rows, so starring one may not star the other.
    ///
    /// The fourth variant, which the `favorites-and-stars` note anticipated:
    /// a preset is something you go looking for the way you go looking for a
    /// soundfont, so it is something you star.
    Preset {
        device: DeviceKind,
        name: String,
        origin: PresetOrigin,
    },
}
