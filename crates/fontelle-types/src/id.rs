use slotmap::new_key_type;

new_key_type! {
    pub struct ChannelId;
    pub struct ClipId;
    pub struct LaneId;
    pub struct NoteId;
    pub struct PointId;
    pub struct PrefabId;
    pub struct MixerTrackId;
    pub struct AssetId;
    pub struct AudioInputId;
    pub struct MarkerId;
    pub struct NodeId;
}

impl NodeId {
    /// The key as a plain `u64`, and back.
    ///
    /// For the one place that has to keep a `NodeId` in an atomic:
    /// `fontelle_midi::LiveTarget`, which is read by a device callback that
    /// may not take a lock. A `slotmap` key *is* a `u64` — `as_ffi` and
    /// `from_ffi` are its own round trip — so nothing here is a
    /// reinterpretation, and putting the pair beside the definition keeps
    /// `slotmap` out of every crate that needs one.
    pub fn to_bits(self) -> u64 {
        use slotmap::Key as _;
        self.data().as_ffi()
    }

    pub fn from_bits(bits: u64) -> Self {
        Self::from(slotmap::KeyData::from_ffi(bits))
    }
}

/// The on-disk counterpart to an in-memory `slotmap` key (TDD §10.2): time-ordered,
/// stable across a save/load round trip, and safe to use as a serialised reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct PersistentId(pub uuid::Uuid);

impl PersistentId {
    pub fn new() -> Self {
        Self(uuid::Uuid::now_v7())
    }
}

impl Default for PersistentId {
    fn default() -> Self {
        Self::new()
    }
}
