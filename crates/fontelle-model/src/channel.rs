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
    pub mixer_track: MixerTrackId,
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
}
