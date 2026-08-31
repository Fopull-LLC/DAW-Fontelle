use fontelle_types::{MixerTrackId, PatchData};

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
}
