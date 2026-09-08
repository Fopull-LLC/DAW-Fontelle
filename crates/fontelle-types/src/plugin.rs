//! What the document knows about a plugin somebody else wrote (TDD §8.4).
//!
//! Hosting was a v1 non-goal with a scaffold: *"the `AudioNode` trait (§5.1)
//! and the parameter contract (§8.2) are the entire boundary a future CLAP
//! host would plug into."* That turned out to be true — nothing in this file
//! is a new addressing scheme, a second parameter model, or a parallel notion
//! of what an instrument is. It is three things the document could not say
//! before: **which** plugin a slot holds, **what it was set to**, and
//! **whatever the plugin itself wants remembered**.
//!
//! In `fontelle-types` for the reason [`crate::InstrumentKind`] is: the model
//! stores it, the window draws it, the app loads it, and the host crate that
//! actually talks to the plugin must be able to name the same thing without
//! any of them depending on each other (§4.1).

use std::fmt;

/// A plugin format this build can name.
///
/// All three are named even though only two are hosted, and that is
/// deliberate: a [`PluginKey`] read out of a project written by a later build
/// has to say what it could not load, and "a VST3 named X is missing" is a
/// sentence somebody can act on where "unreadable" is not. §8.4's order of
/// intent is CLAP first, LV2 second, VST3 only through a bridge if it is ever
/// justified — and §3.4 is why: CLAP is MIT and lilv is ISC, with no
/// agreement to sign for either.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub enum PluginFormat {
    Clap,
    Vst3,
    Lv2,
}

impl PluginFormat {
    pub const ALL: [Self; 3] = [Self::Clap, Self::Vst3, Self::Lv2];

    pub fn label(self) -> &'static str {
        match self {
            Self::Clap => "CLAP",
            Self::Vst3 => "VST3",
            Self::Lv2 => "LV2",
        }
    }

    /// What a bundle of this format is called on disk, without the dot.
    pub fn extension(self) -> &'static str {
        match self {
            Self::Clap => "clap",
            Self::Vst3 => "vst3",
            Self::Lv2 => "lv2",
        }
    }

    /// Whether this build can actually load one.
    ///
    /// A format this says `false` about can still be *named* — see the type's
    /// own note. It is asked before a scan walks a folder and before a slot
    /// tries to open what it holds.
    pub fn hosted(self) -> bool {
        matches!(self, Self::Clap | Self::Lv2)
    }

    /// The prefix a [`PluginKey`] of this format is written with.
    fn tag(self) -> &'static str {
        match self {
            Self::Clap => "clap",
            Self::Vst3 => "vst3",
            Self::Lv2 => "lv2",
        }
    }

    fn from_tag(tag: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|format| format.tag() == tag)
    }
}

/// Which plugin, permanently.
///
/// The **id the plugin declares**, not the path it was found at. A plugin that
/// is moved, upgraded, or installed somewhere else on the machine the project
/// is opened on is the same plugin; a path is a fact about one computer. This
/// is the same reasoning INVARIANT 8 applies to audio — a saved project names
/// things by what they *are*, and the machine resolves that to where they are.
///
/// Written as one string, `clap:com.u-he.diva`, because it appears in
/// `project.json` and a two-field object for something read as a single name
/// is harder to read and harder to grep.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PluginKey {
    pub format: PluginFormat,
    pub id: String,
}

impl PluginKey {
    pub fn new(format: PluginFormat, id: impl Into<String>) -> Self {
        Self {
            format,
            id: id.into(),
        }
    }

    pub fn clap(id: impl Into<String>) -> Self {
        Self::new(PluginFormat::Clap, id)
    }

    /// Reads one back. `None` for a format this build cannot name at all, and
    /// for an empty id.
    ///
    /// Split at the **first** colon: a CLAP id is a reverse-domain name and
    /// nothing forbids a colon inside it, so the tag is what is before the
    /// first one and everything after it is the id.
    pub fn parse(text: &str) -> Option<Self> {
        let (tag, id) = text.split_once(':')?;
        let format = PluginFormat::from_tag(tag)?;
        (!id.is_empty()).then(|| Self::new(format, id))
    }
}

impl fmt::Display for PluginKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.format.tag(), self.id)
    }
}

impl serde::Serialize for PluginKey {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> serde::Deserialize<'de> for PluginKey {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).ok_or_else(|| serde::de::Error::custom(format!("unknown plugin {text}")))
    }
}

/// One of a plugin's parameters, at the value the document last saw.
///
/// The id is the plugin's own — CLAP calls it a stable id and says it must
/// never change, which is INVARIANT 7 written from the other side of the
/// boundary. It is what an automation lane addresses
/// (`mixer:<track>/insert[0]/param/7`), so it is stored as the plugin gives
/// it rather than as an index into a list that a plugin update could reorder.
///
/// The value is **plain**, in the plugin's own units, not normalised. CLAP's
/// own advice to hosts is to store plain values, and it is right for the same
/// reason the taper on a built-in parameter is: a plugin that widens a range
/// in an update should keep sounding the same, and only the plain number can.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PluginParamValue {
    pub id: u32,
    pub value: f64,
}

/// Everything a project has to remember about one plugin instance.
///
/// **Both halves are kept**, and they are not redundant. `blob` is the
/// plugin's own state — the authority, and the only thing that can carry what
/// a plugin knows that is not a parameter (a loaded sample, a drawn envelope,
/// a preset name). `params` is what this program can read, write and
/// *automate*: a lane needs a number, and a lane pointing into an opaque blob
/// would be a lane nobody could draw. A plugin with no state extension is
/// restored from `params` alone, which is the whole reason they are stored
/// rather than derived.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PluginState {
    pub key: PluginKey,
    /// What the plugin called itself when this was written.
    ///
    /// So a strip can say "Diva" rather than "clap:com.u-he.diva" while the
    /// plugin is missing — the one moment the name is needed most and the
    /// plugin is not there to ask.
    pub name: String,
    #[serde(default)]
    pub params: Vec<PluginParamValue>,
    /// The plugin's own state, base64 (see [`crate::encode_base64`]).
    ///
    /// `None` for a plugin that does not implement the state extension, and
    /// for a slot whose state has not been read back yet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blob: Option<String>,
}

impl PluginState {
    pub fn new(key: PluginKey, name: impl Into<String>) -> Self {
        Self {
            key,
            name: name.into(),
            params: Vec::new(),
            blob: None,
        }
    }

    /// What the document last saw this parameter set to.
    pub fn param(&self, id: u32) -> Option<f64> {
        self.params
            .iter()
            .find(|param| param.id == id)
            .map(|param| param.value)
    }

    /// Records a parameter's value, replacing what was there.
    ///
    /// A list rather than a map because it is written to JSON and read back in
    /// order, and because a plugin's parameter count is tens, not thousands.
    pub fn set_param(&mut self, id: u32, value: f64) {
        match self.params.iter_mut().find(|param| param.id == id) {
            Some(param) => param.value = value,
            None => self.params.push(PluginParamValue { id, value }),
        }
    }
}
