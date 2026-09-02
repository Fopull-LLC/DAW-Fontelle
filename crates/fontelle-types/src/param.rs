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
    /// Milliseconds. Its own unit rather than [`Seconds`](Self::Seconds)
    /// scaled, because a compressor's attack is *stored* in milliseconds and a
    /// read-out that guessed from the number said "10 s" for a ten-millisecond
    /// attack — which is not a rounding error, it is three orders of
    /// magnitude.
    Milliseconds,
}

impl Unit {
    /// The suffix a read-out puts after the number.
    pub fn suffix(self) -> &'static str {
        match self {
            Self::None | Self::Switch => "",
            Self::Decibels => " dB",
            Self::Hertz => " Hz",
            Self::Seconds => " s",
            Self::Milliseconds => " ms",
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
    /// What a **stepped** parameter's positions are called, in order.
    ///
    /// Empty for everything else, and for a stepped parameter whose positions
    /// really are numbers. A chooser whose read-out says "0.00" is a control
    /// nobody can set on purpose — which is what a compressor's detection mode
    /// looked like the first time it had a window — and the alternative is a
    /// per-effect table in whatever draws it, which is the second list §8.2
    /// exists to prevent.
    pub positions: &'static [&'static str],
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
    /// `channel:<channel>/gain` — an instrument channel's own level, which is
    /// **not** its mixer track's: several channels may share a track (TDD
    /// §13.1), so the two are different controls. See
    /// `fontelle_model::Channel::gain_db`.
    ChannelGain(crate::ChannelId),
    /// `channel:<channel>/pan` — the same, for its place in the stereo field.
    ChannelPan(crate::ChannelId),
    /// `channel:<channel>/patch/<parameter>` — one control **inside** the
    /// channel's instrument: a filter's cutoff, an envelope stage, an
    /// oscillator's level.
    ///
    /// `param` is the address the instrument panel uses for that control, the
    /// `patch/` prefix included, so the thing you right-click and the thing
    /// automation writes are named by the same string and cannot drift apart.
    ///
    /// **Anything after `patch/` parses.** Which controls a patch has is the
    /// patch's business, and INVARIANT 7 already says what happens to a name
    /// this build does not recognise: it changes nothing, and it is not an
    /// error. A project written by a later build has to open, and its lane has
    /// to survive being saved again.
    ChannelPatch {
        channel: crate::ChannelId,
        param: String,
    },
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
            Self::ChannelGain(channel) => format!("channel:{}/gain", channel.data().as_ffi()),
            Self::ChannelPan(channel) => format!("channel:{}/pan", channel.data().as_ffi()),
            Self::ChannelPatch { channel, param } => {
                format!("channel:{}/{param}", channel.data().as_ffi())
            }
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
        if let Some(rest) = text.strip_prefix("channel:") {
            let (id, field) = rest.split_once('/')?;
            let channel = crate::ChannelId::from(slotmap::KeyData::from_ffi(id.parse().ok()?));
            return match field {
                "gain" => Some(Self::ChannelGain(channel)),
                "pan" => Some(Self::ChannelPan(channel)),
                // §8.2's patch scheme. The whole of `patch/...` is kept as the
                // parameter name, because that is the address the instrument
                // panel gives the control you right-clicked.
                _ if field.starts_with("patch/") => Some(Self::ChannelPatch {
                    channel,
                    param: field.to_string(),
                }),
                // Anything else under `channel:` is not a scheme this build
                // has — `None` rather than a guess, so an automation clip
                // aimed at one does nothing instead of moving whichever
                // parameter parsed closest.
                _ => None,
            };
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
            // A channel's own controls are not a track's, which is the whole
            // reason they exist — see `fontelle_model::Channel::gain_db`.
            Self::Tempo
            | Self::ChannelGain(_)
            | Self::ChannelPan(_)
            | Self::ChannelPatch { .. } => None,
            Self::TrackGain(track) | Self::TrackPan(track) => Some(*track),
            Self::Insert { track, .. } => Some(*track),
        }
    }
}

/// A run of consecutive parameters a panel draws under one heading — see
/// `EffectConfig::sections`.
///
/// A name and a count rather than a list of ids: the section *is* the next
/// `count` rows of the effect's own table, so a parameter cannot be in two
/// sections or in none, and adding one to the table means adding one to a
/// count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParamSection {
    pub name: &'static str,
    pub count: usize,
}
