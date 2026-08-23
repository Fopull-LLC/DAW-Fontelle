use fontelle_types::MixerTrackId;

/// An instrument channel: the document-level handle a clip's `NoteData::channel`
/// points at. Its `fontelle-core::Patch` lives behind this — the model crate never
/// depends on `fontelle-core` (that dependency runs the other way, through
/// `fontelle-engine`), so `Channel` holds a stored/serialised patch representation
/// rather than a live `Sampler`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Channel {
    pub name: String,
    pub color: [u8; 4],
    pub mixer_track: MixerTrackId,
    /// Serialised `fontelle-core::Patch` — see `fontelle-model`'s crate doc for why
    /// this isn't the live type.
    pub patch_data: Vec<u8>,
}
