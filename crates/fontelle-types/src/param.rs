use std::fmt;

/// The stable string address of an automatable parameter (TDD §8.2, INVARIANT 7).
/// Serves plugin export, DAW automation targets, preset serialisation, MIDI learn,
/// and undo command targets — one addressing scheme for all five. Never changes
/// across versions once assigned.
///
/// Examples: `channel:<uuid>/patch/layer[2]/filter1.cutoff`, `transport/tempo`.
#[derive(
    Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub struct ParamAddress(String);

impl ParamAddress {
    pub fn new(address: impl Into<String>) -> Self {
        Self(address.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ParamAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for ParamAddress {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

/// How a parameter's value is distributed across its 0..1 automation range
/// (TDD §8.2's "value distribution").
///
/// It exists because a lane is a fixed number of pixels and a range is not
/// always evenly interesting: a linear 20 Hz–20 kHz frequency lane spends nine
/// tenths of its travel above 2 kHz, which makes the half of the spectrum
/// music lives in unaimable.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Taper {
    Linear,
    /// Even in *ratio* rather than in difference — the right one for anything
    /// measured in octaves or in time.
    Logarithmic,
    /// A choice with `n` positions. The value is always one of them, which is
    /// what makes automating a band type produce band types rather than
    /// numbers between two of them.
    Stepped(u32),
}

/// What a parameter's number means, so a lane can label itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Unit {
    None,
    Decibels,
    Hertz,
    Seconds,
    Ratio,
    Percent,
    /// On or off. Stepped with two positions, and worth its own unit because
    /// a lane showing it should draw a square wave rather than a ramp.
    Switch,
}

impl Unit {
    /// The suffix a read-out puts after the number.
    pub fn suffix(self) -> &'static str {
        match self {
            Self::None | Self::Switch => "",
            Self::Decibels => " dB",
            Self::Hertz => " Hz",
            Self::Seconds => " s",
            Self::Ratio => ":1",
            Self::Percent => "%",
        }
    }
}

/// Everything §8.2 says a parameter carries: stable ID, display name, range,
/// default, unit, and value distribution.
///
/// The smoother §8.2 also names belongs to whatever *applies* the value — an
/// effect's own DSP, or the node reading automation — and not to the
/// description of it, which is what this is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParamSpec {
    /// **Never changes.** It is what a saved automation clip, a preset and a
    /// MIDI-learn binding all name (INVARIANT 7).
    pub id: &'static str,
    pub name: &'static str,
    pub min: f32,
    pub max: f32,
    pub default: f32,
    pub unit: Unit,
    pub taper: Taper,
}

impl ParamSpec {
    /// Where `value` sits in this parameter's 0..1 automation range.
    pub fn normalise(&self, value: f32) -> f32 {
        let value = value.clamp(self.min, self.max);
        match self.taper {
            Taper::Linear => (value - self.min) / (self.max - self.min),
            Taper::Logarithmic => {
                let (min, max) = (self.min.max(1e-6), self.max.max(1e-6));
                (value.max(1e-6) / min).ln() / (max / min).ln()
            }
            Taper::Stepped(steps) => {
                let steps = steps.max(2) as f32;
                let index = ((value - self.min) / (self.max - self.min) * (steps - 1.0)).round();
                index / (steps - 1.0)
            }
        }
        .clamp(0.0, 1.0)
    }

    /// And back: what `t` in 0..1 means in this parameter's own units.
    pub fn denormalise(&self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self.taper {
            Taper::Linear => self.min + t * (self.max - self.min),
            Taper::Logarithmic => {
                let (min, max) = (self.min.max(1e-6), self.max.max(1e-6));
                min * (max / min).powf(t)
            }
            Taper::Stepped(steps) => {
                let steps = steps.max(2) as f32;
                let index = (t * (steps - 1.0)).round();
                self.min + index / (steps - 1.0) * (self.max - self.min)
            }
        }
    }

    /// `value`, forced into what this parameter can actually hold.
    pub fn clamp(&self, value: f32) -> f32 {
        match self.taper {
            Taper::Stepped(_) => self.denormalise(self.normalise(value)),
            _ => value.clamp(self.min, self.max),
        }
    }
}

/// One automatable parameter, named the way §8.2 names it.
///
/// A *parsed view* of a [`ParamAddress`], not a second addressing scheme —
/// §8.2 is explicit that a parallel scheme is a design regression. The string
/// is what is stored, in automation clips, presets and MIDI-learn bindings;
/// this is what code matches on after reading one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParamTarget {
    /// `transport/tempo`. The tempo map is generated by evaluating this
    /// (§12.3), so tempo automation and the tempo map are one system.
    Tempo,
    /// `mixer:<track>/gain`
    TrackGain(crate::MixerTrackId),
    /// `mixer:<track>/pan`
    TrackPan(crate::MixerTrackId),
    /// `mixer:<track>/insert[<slot>]/param/<param>`
    Insert {
        track: crate::MixerTrackId,
        slot: usize,
        /// The effect's own stable id for it — [`ParamSpec::id`].
        param: String,
    },
}

impl ParamTarget {
    pub fn address(&self) -> ParamAddress {
        use slotmap::Key;
        ParamAddress::new(match self {
            Self::Tempo => "transport/tempo".to_string(),
            Self::TrackGain(track) => format!("mixer:{}/gain", track.data().as_ffi()),
            Self::TrackPan(track) => format!("mixer:{}/pan", track.data().as_ffi()),
            Self::Insert { track, slot, param } => format!(
                "mixer:{}/insert[{slot}]/param/{param}",
                track.data().as_ffi()
            ),
        })
    }

    /// Reads one back. `None` for an address this build does not recognise —
    /// a project from a newer version, or an address belonging to a part of
    /// §8.2's scheme that is not built yet (a channel's patch, say).
    ///
    /// `None` rather than a guess: an automation clip pointed at something
    /// unrecognised must do nothing, not move whichever parameter parsed
    /// closest.
    pub fn parse(address: &ParamAddress) -> Option<Self> {
        let text = address.as_str();
        if text == "transport/tempo" {
            return Some(Self::Tempo);
        }
        let rest = text.strip_prefix("mixer:")?;
        let (id, rest) = rest.split_once('/')?;
        let track = crate::MixerTrackId::from(slotmap::KeyData::from_ffi(id.parse().ok()?));
        match rest {
            "gain" => Some(Self::TrackGain(track)),
            "pan" => Some(Self::TrackPan(track)),
            _ => {
                let rest = rest.strip_prefix("insert[")?;
                let (slot, rest) = rest.split_once(']')?;
                let param = rest.strip_prefix("/param/")?;
                if param.is_empty() {
                    return None;
                }
                Some(Self::Insert {
                    track,
                    slot: slot.parse().ok()?,
                    param: param.to_string(),
                })
            }
        }
    }

    /// The mixer track this target belongs to, when it belongs to one.
    pub fn track(&self) -> Option<crate::MixerTrackId> {
        match self {
            Self::Tempo => None,
            Self::TrackGain(track) | Self::TrackPan(track) => Some(*track),
            Self::Insert { track, .. } => Some(*track),
        }
    }
}
