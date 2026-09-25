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
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct PersistentId(pub uuid::Uuid);

impl PersistentId {
    pub fn new() -> Self {
        Self(uuid::Uuid::now_v7())
    }

    /// An id worked out from `seed` rather than minted: the same seed gives
    /// the same id on every machine and every run.
    ///
    /// For a thing written before it had an id of its own — a format-0
    /// project, whose id comes from its `created` and its `name`
    /// (`docs/collab-plan.md` §4.1) — so two copies of one old song on two
    /// machines agree about who they are, which minting on load would break.
    /// Nothing made today should use this; `new` is the rule.
    ///
    /// FNV-1a over 128 bits, stamped as a UUIDv8 ("custom"). Not
    /// cryptographic and not meant to be: the seeds are a song's own birth
    /// details, not an adversary's. **The constants are part of every old
    /// project's identity** and must never change.
    pub fn derived(seed: &str) -> Self {
        const OFFSET: u128 = 0x6c62272e07bb014262b821756295c58d;
        const PRIME: u128 = 0x0000000001000000000000000000013b;
        let hash = seed.bytes().fold(OFFSET, |hash, byte| {
            (hash ^ byte as u128).wrapping_mul(PRIME)
        });
        Self(uuid::Builder::from_custom_bytes(hash.to_be_bytes()).into_uuid())
    }
}

impl Default for PersistentId {
    fn default() -> Self {
        Self::new()
    }
}
