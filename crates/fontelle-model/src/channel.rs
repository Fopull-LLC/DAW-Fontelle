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
}
