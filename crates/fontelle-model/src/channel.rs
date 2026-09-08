use fontelle_types::{MixerTrackId, PatchData, PluginState};

/// An instrument channel: the document-level handle a clip's `NoteData::channel`
/// points at. Its `fontelle-core::Patch` lives behind this — the model crate never
/// depends on `fontelle-core` (that dependency runs the other way, through
/// `fontelle-engine`), so `Channel` holds a stored patch representation rather
/// than a live `Sampler`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Channel {
    pub name: String,
    pub color: [u8; 4],
    /// Which mixer track this channel's audio arrives on. **`None` is the
    /// master**, and that is where a new channel goes.
    ///
    /// It used to be a track of the channel's own, minted by `AddChannel`.
    /// That was wrong in the way FL Studio is right: a mixer track is a
    /// *destination* somebody builds deliberately — a drum bus, a reverb
    /// return — not a thing that appears every time a soundfont is loaded.
    /// Twenty channels meant twenty strips nobody asked for, and the one
    /// question a mixer answers ("what is going where") had no way to be
    /// asked. TDD §13.1's "several channels may share a mixer track" is what
    /// this makes reachable.
    ///
    /// `Option` rather than the master's own id so that a channel does not
    /// have to be re-pointed when a project is built before its master is, and
    /// so that "wherever the master is" survives a document whose master id
    /// changed. A project written before this field became optional carries a
    /// bare id, which deserialises as `Some`.
    pub mixer_track: Option<MixerTrackId>,
    /// The channel's instrument, in the form `fontelle-core` writes (TDD §8.3).
    ///
    /// `None` is a channel with no instrument chosen yet — a real state, and
    /// the one a freshly created channel is in until a soundfont is dropped on
    /// it. It plays nothing rather than playing a default.
    ///
    /// This was an opaque `Vec<u8>` while the format was undesigned. It is a
    /// typed value now for two reasons: `serde_json` writes a byte vector as an
    /// array of decimal numbers, which would make `project.json` unreadable
    /// against TDD §17.2's whole reason for choosing JSON; and the version
    /// stamp has to be legible without deserialising the patch, so a file from
    /// a newer build can be refused by version rather than by a confusing
    /// field-level error.
    pub patch_data: Option<PatchData>,
    /// **Which of the three instruments this channel is** — see
    /// [`fontelle_types::InstrumentKind`].
    ///
    /// The choice, kept, rather than something read back off `patch_data`. A
    /// patch with layers in it says what it is; an *empty* one does not, and
    /// empty is a real state — a sampler with no sample and a soundfont player
    /// with no soundfont are the same patch, and both are where you sit while
    /// deciding what to load.
    ///
    /// `None` is a project written before this existed. The app resolves it
    /// from the patch that is actually loaded (`Session::channel_kind`), which
    /// is exactly the derivation this field replaces — so an old project opens
    /// saying what it always was, and says it in this field the moment
    /// anything is chosen.
    #[serde(default)]
    pub instrument: Option<fontelle_types::InstrumentKind>,
    /// The plugin this channel plays instead of a patch (TDD §8.4).
    ///
    /// Beside `patch_data` rather than inside it, for the reason
    /// [`crate::EffectSlot::plugin`] is beside its `config`: a `PatchData` is
    /// what `fontelle-core` writes, and `fontelle-core` knows nothing about
    /// plugins and must not (INVARIANT 4). A channel has one or the other, and
    /// [`instrument`](Self::instrument) is what says which.
    ///
    /// The two are not cleared when the other is set, and that is deliberate:
    /// somebody trying a plugin on a channel that had a soundfont on it, and
    /// then changing their mind, gets the soundfont back rather than an empty
    /// channel.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugin: Option<PluginState>,
    /// Where this channel sits in the stereo field: -1.0 hard left, 0.0
    /// centre, +1.0 hard right.
    ///
    /// **Not the same control as its mixer track's `pan`,** and the difference
    /// is audible. This is constant-power *placement* of a source the voice
    /// has not yet positioned; a track's pan is a *balance* control over a bus
    /// whose contents are already placed. Applying a pan law twice pulls a
    /// second 3 dB out of every centred part, and a balance control swung hard
    /// over throws half the signal away instead of moving it.
    ///
    /// It lives on the channel rather than on the track because TDD §13.1
    /// allows several channels to share one mixer track, and each of them
    /// needs its own place in the field.
    pub pan: f32,
    /// How loud this channel is, in decibels, before it reaches its bus.
    ///
    /// **Not the same control as its mixer track's `gain_db`,** for exactly
    /// the reason `pan` is not the same control as the track's `pan`, and the
    /// bug that made the difference obvious: the instrument panel's volume
    /// knob used to write the *track's* level, and since every channel goes to
    /// the master until somebody routes it elsewhere, two channels' panels
    /// were two knobs on one fader. Turning the choir down turned the piano
    /// down with it — reported as *"changing the instrument settings for one
    /// instrument was actually affecting the wrong instrument"*, which is what
    /// it looks like from the other side of the screen.
    ///
    /// It is applied at the sampler, ahead of the bus, so it stays this
    /// channel's however many channels share the track.
    ///
    /// Defaulted, so a project written before it existed opens at unity.
    #[serde(default)]
    pub gain_db: f32,
    /// The preset this device was loaded from, if it was loaded from one
    /// (`docs/flopsynth-plan.md` §P.5).
    ///
    /// **The name is remembered; the cleanliness is recognised.** This field
    /// survives every edit and never says whether the device still matches the
    /// file — that is computed by comparing the current state against the
    /// bank's (`Session::preset_state`), so an undo makes the bar's `*` go out
    /// with nothing to remember and no way for the two to disagree.
    ///
    /// Defaulted and omitted when empty, so every project written before the
    /// preset system opens and is written back unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<fontelle_types::PresetRef>,

    /// This channel plays nothing.
    ///
    /// A **sequencer** mute, the same reading TDD §10.3 gives a lane's: the
    /// compiler drops its clips' events rather than the fader zeroing its bus.
    /// It has to be, now — with every channel on the master by default, a
    /// switch that reached for the track would silence the whole song.
    ///
    /// Defaulted, so a project written before the switches moved off the
    /// mixer track opens unmuted rather than refusing to open.
    #[serde(default)]
    pub muted: bool,
    /// Only the soloed channels play. Also a sequencer mute; the mixer keeps
    /// its own solo, over tracks, which is a different question.
    #[serde(default)]
    pub soloed: bool,
    /// Draw the roll's key strip as a **list of names** rather than as a
    /// keyboard (TDD §16.4).
    ///
    /// A per-channel view preference, in the document rather than in the
    /// window, because it is a fact about the *instrument*: a drum kit's keys
    /// are a list of sounds and a piano's are a keyboard, and a project with
    /// both wants both. Saved for the same reason a channel's colour is —
    /// a preference that resets every time the song is opened is one nobody
    /// sets twice.
    ///
    /// Defaulted, so every project written before the view existed opens on
    /// the keyboard it had.
    #[serde(default)]
    pub named_keys: bool,
}
