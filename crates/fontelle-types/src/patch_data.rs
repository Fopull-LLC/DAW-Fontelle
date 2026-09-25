use crate::{AssetKind, AssetRef};

/// A serialised `fontelle-core::Patch`, as a project or a preset file stores it
/// (TDD §8.3, §17.2).
///
/// The *format* belongs to `fontelle-core` — §8.3 is explicit that a patch built
/// in the DAW and one built in the plugin are the same bytes, which only holds if
/// one crate owns the definition. This type is only the shape the document holds,
/// so `fontelle-model` can carry a channel's instrument without depending on the
/// sampler crate (INVARIANT 4).
///
/// The body is a `serde_json::Value` rather than typed fields for one reason: the
/// migration chain has to read shapes this build's structs no longer describe. A
/// v0 body deserialised into a v3 struct is exactly the failure a migration exists
/// to prevent.
///
/// It is also what makes `project.json` readable. The previous form was
/// `Vec<u8>`, which `serde_json` writes as an array of decimal numbers — TDD
/// §17.2 chose JSON to be "diffable, inspectable, greppable, and recoverable by
/// hand", and a patch stored as `[123,34,108,...]` is none of those.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PatchData {
    /// Which revision of the format `body` is written in. Read before `body` is
    /// looked at, so a file from a newer build is refused by version rather than
    /// by a confusing field-level parse error.
    pub format_version: u32,
    pub body: serde_json::Value,
}

/// One decoded sample inside a file, named portably.
///
/// A `fontelle-core::Layer` points at its audio with an `AssetId`, which is a
/// `slotmap` key — an index and a generation, minted by whichever `SampleStore`
/// happened to decode the file. INVARIANT 8 forbids putting that on disk, and it
/// would be useless there anyway: the same soundfont imported into a different
/// store gets different keys.
///
/// So a saved patch names its audio the way TDD §8.3 says a preset must — by
/// asset reference (§17.4), not by sample data — plus which sample *inside* the
/// file, because an `AssetRef` names a file and a soundfont holds hundreds of
/// samples.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct SampleRef {
    pub file: AssetRef,
    /// Index of the sample header within the file, as the format itself orders
    /// them. Stable for a given file, which is all a relink needs.
    pub sample: u32,
}

impl AssetRef {
    /// Whether two references name the same content, for relinking (§17.4).
    ///
    /// Deliberately ignores [`AssetRef::id`]. That field is a runtime handle
    /// into one studio's sample library, so a reference embedded in a preset
    /// from another machine has no meaningful value for it — comparing it would
    /// make every imported preset's samples unresolvable.
    pub fn same_content(&self, other: &Self) -> bool {
        self.path == other.path
            && self.content_hash == other.content_hash
            && self.size == other.size
            && self.kind == other.kind
    }

    /// A reference to a file on disk with no library entry behind it yet.
    ///
    /// The `id` is null, and that is the honest value: a sample library mints
    /// ids, and an importer building a reference for a preset has no library
    /// to mint from. Relinking matches on [`AssetRef::same_content`], never on
    /// the id.
    pub fn unregistered(
        path: std::path::PathBuf,
        content_hash: u64,
        size: u64,
        kind: AssetKind,
    ) -> Self {
        use slotmap::Key;
        Self {
            id: crate::AssetId::null(),
            path,
            content_hash,
            size,
            kind,
        }
    }
}
