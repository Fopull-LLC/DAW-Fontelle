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
