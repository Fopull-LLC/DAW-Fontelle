//! The document as it crosses a wire (`docs/collab-plan.md` §5.3).
//!
//! Two people work on one song by sending each other the edits they make —
//! every one of which is a [`Command`] already, and a command that has been
//! applied once is a complete description of its effect, **ids included**:
//! redo re-applies it and `Arena::insert_at` puts back exactly what it minted.
//! So an edit from another machine is a redo from somebody else's history,
//! and this module is how a command becomes bytes and back.
//!
//! # Why JSON, and what that makes permanent
//!
//! The plan said postcard. The document cannot travel in postcard: a patch's
//! body is a `serde_json::Value`, two of the effect configs are untagged
//! enums, and seventeen fields are skipped when empty — each of which a
//! format that does not describe itself cannot read back (§18, F52). The
//! document already saves as JSON, so every type in it is known to survive
//! JSON, and an edit is JSON too.
//!
//! JSON names a variant rather than numbering it, so the discipline the
//! relay's `RelayMsg` keeps for its numbers this keeps for its **names**: a
//! variant, once shipped, is never renamed, and a field is never renamed or
//! removed. `tests/wire.rs`'s `wire_variant_order_is_pinned` holds literals
//! from this build that every later build has to read. (v1 also requires both
//! ends to be the same Fontelle, §8.3, so this matters from the day that
//! rule relaxes.)

use crate::command::Command;
use crate::commands::*;

/// An edit that could not be read off the wire.
#[derive(Debug)]
pub struct WireError(pub String);

impl std::fmt::Display for WireError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for WireError {}

macro_rules! edits {
    ($($name:ident),* $(,)?) => {
        /// One edit, as it crosses a wire: one variant per command, including
        /// every `Restore*` inverse, because an undo is an edit like any other
        /// (§5.7).
        #[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
        pub enum Edit {
            $($name($name),)*
            /// Nests: its parts are edits, each with its own ids.
            Compound {
                label: String,
                parts: Vec<Edit>,
                applied: bool,
            },
        }

        impl Edit {
            /// Every name an edit can go by on the wire.
            pub const TAGS: &'static [&'static str] = &[$(stringify!($name),)* "Compound"];

            /// The name this edit goes by on the wire.
            pub fn tag(&self) -> &'static str {
                match self {
                    $(Edit::$name(_) => stringify!($name),)*
                    Edit::Compound { .. } => "Compound",
                }
            }

            /// The command this edit is, ready to apply the way redo does.
            pub fn into_command(self) -> Box<dyn Command> {
                match self {
                    $(Edit::$name(command) => Box::new(command),)*
                    Edit::Compound {
                        label,
                        parts,
                        applied,
                    } => Box::new(Compound::from_edits(label, parts, applied)),
                }
            }
        }
    };
}

edits!(
    NotApplied,
    AddChannel,
    RemoveChannel,
    RestoreChannel,
    DuplicateChannel,
    MoveLane,
    AddLane,
    RemoveLane,
    RestoreLane,
    RenameLane,
    AddMixerTrack,
    RemoveMixerTrack,
    RestoreMixerTrack,
    RenameMixerTrack,
    SetTrackOutput,
    AddSend,
    RemoveSend,
    RestoreSend,
    SetSendLevel,
    SetSendPreFader,
    SetChannelRoute,
    SetChannelKind,
    RestoreChannelKind,
    SetChannelPatch,
    RenameChannel,
    AddNotes,
    RemoveNotes,
    RestoreNotes,
    MoveNotes,
    ResizeNotes,
    SetNoteVelocity,
    RestoreNoteVelocities,
    SetNoteSlide,
    RestoreNoteSlides,
    SliceNotes,
    MergeSlices,
    SetNoteProperty,
    RestoreNoteProperties,
    NudgeNoteProperty,
    SetNotePropertyEach,
    SetNoteLengths,
    AddAudioClip,
    RemoveAudioClip,
    SetTrackInput,
    SetAudioClip,
    ImportParts,
    UnimportParts,
    AddClip,
    RemoveClip,
    RestoreClip,
    MoveClip,
    SetClipLoop,
    ResizeClip,
    TrimClipStart,
    ReplaceClip,
    DuplicateClip,
    SplitClip,
    UnsplitClip,
    SetNumber,
    SetFlag,
    SetLoopRange,
    ApplyTrackChain,
    RestoreTrackChain,
    AddInsert,
    RemoveInsert,
    RestoreInsert,
    MoveInsert,
    SetInsertBypassed,
    SetInsertParam,
    SetInsertNotes,
    SetInsertKey,
    RestoreInsertConfig,
    SetInsertMix,
    EditNotepad,
    EditLapse,
    SetEqBand,
    AddAutomationPoint,
    MoveAutomationPoints,
    PlaceAutomationPoints,
    RemoveAutomationPoints,
    RestoreAutomationPoints,
    SetPointCurve,
    RestorePointCurves,
    AddPluginInsert,
    SetChannelPlugin,
    RestoreChannelPlugin,
    SetPluginParam,
    RestorePluginParam,
    ApplyPreset,
    SwitchChannelAb,
    CopyChannelAb,
    RestoreChannelAbOther,
    RestoreDeviceState,
    SetPresetRef,
    AddPrefab,
    AddPrefabInstance,
    RenamePrefab,
    DetachPrefab,
    RestorePrefabLink,
    RemovePrefab,
    RestorePrefab,
    MakePrefabFromClip,
    RenameProject,
    AddMarker,
    RemoveMarker,
    RestoreMarker,
    RelocateAssets,
    RestoreAssets,
);

impl Edit {
    /// The bytes that cross the wire.
    pub fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("an edit is made of document types, which all write")
    }

    /// An edit read back off the wire.
    pub fn from_bytes(bytes: &[u8]) -> Result<Edit, WireError> {
        serde_json::from_slice(bytes).map_err(|e| WireError(format!("not an edit: {e}")))
    }
}

/// `Option<Option<T>>` as JSON can tell its two `None`s apart.
///
/// A command's `previous: Option<Option<_>>` is "not applied yet" as `None`
/// and "applied, and there was nothing there" as `Some(None)`, and JSON
/// writes both as `null` — so an applied command would arrive looking
/// unapplied and its inverse would refuse. Written as `null` and `[inner]`.
pub(crate) mod nested {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<T: Serialize, S: Serializer>(
        value: &Option<Option<T>>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        value.as_ref().map(|inner| (inner,)).serialize(serializer)
    }

    pub fn deserialize<'de, T: Deserialize<'de>, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<Option<T>>, D::Error> {
        Ok(Option::<(Option<T>,)>::deserialize(deserializer)?.map(|(inner,)| inner))
    }
}

// ------------------------------------------------------------- the session

/// The version of the conversation below. v1 also requires the same
/// Fontelle on both ends (§8.3), so this moves only once that is relaxed.
pub const PROTOCOL: u32 = 1;

/// What a shared song says about itself when somebody joins it — enough for
/// the joiner to find its own copy and say which is newer (§4.3).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ProjectHead {
    pub id: fontelle_types::PersistentId,
    pub name: String,
    pub saved_revision: u64,
    pub saved_at: String,
    pub saved_by: String,
    pub shared_revision: Option<(fontelle_types::PersistentId, u64)>,
    /// [`crate::Project::sync_hash`] of the song as the host has it now.
    pub hash: u64,
}

/// One file a song uses, named by what is in it (§7.1).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AssetEntry {
    pub hash: u64,
    pub size: u64,
    pub file_name: String,
    pub kind: fontelle_types::AssetKind,
}

/// One message between the studios in a shared session (§8.2).
///
/// **Append-only.** postcard numbers variants by declaration order, the way
/// the relay's own `RelayMsg` does: a new message goes at the end, and
/// `tests/wire.rs`'s `msg_variant_order_is_pinned` holds the numbers. An edit
/// rides inside as JSON bytes, because the document cannot travel in postcard
/// (F52); everything else here is postcard's own.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum Msg {
    // The handshake.
    /// Joiner to host, first. `install` names the studio, not the person, for
    /// the record of what two copies last agreed on (§4.5).
    Hello {
        protocol: u32,
        fontelle: String,
        name: String,
        install: fontelle_types::PersistentId,
    },
    /// Host to joiner: who you are in this session (your `peer`, which is
    /// also where you mint — see `crate::arena::minting_in`), and the song.
    Welcome {
        peer: u16,
        protocol: u32,
        fontelle: String,
        host: String,
        host_install: fontelle_types::PersistentId,
        project: ProjectHead,
        manifest: Vec<AssetEntry>,
    },
    Refuse {
        reason: String,
    },
    // The document.
    /// The song as JSON, in pieces under the relay's frame cap.
    SnapshotChunk {
        index: u32,
        of: u32,
        bytes: Vec<u8>,
    },
    /// The last piece has gone: the song's hash, and the edit it is at.
    SnapshotDone {
        hash: u64,
        seq: u64,
    },
    /// Joiner to host: an edit made and already shown on the joiner's
    /// screen, numbered by the joiner.
    Propose {
        local_seq: u64,
        #[serde(with = "as_json")]
        edit: Edit,
    },
    /// Host to everybody: the next edit in the one order there is, whose it
    /// was, and — when it is the last of what the host had to send — the
    /// song's hash after it.
    Applied {
        seq: u64,
        author: u16,
        author_seq: u64,
        #[serde(with = "as_json")]
        edit: Edit,
        hash: Option<u64>,
    },
    /// Host to a joiner: that proposal did not apply here.
    Refused {
        local_seq: u64,
        reason: String,
    },
    ResyncRequest,
    // Files.
    AssetRequest {
        hash: u64,
    },
    AssetChunk {
        hash: u64,
        index: u32,
        of: u32,
        bytes: Vec<u8>,
    },
    AssetDone {
        hash: u64,
    },
    AssetMissing {
        hash: u64,
    },
    // People.
    Joined {
        peer: u16,
        name: String,
        colour: u8,
    },
    Left {
        peer: u16,
    },
    Bye,
    // The host's two controls on somebody's row (§10.1, F49).
    /// Host to a joiner: your edits are refused from now on, or no longer.
    /// Sent so the joiner's panel can say so before they try.
    ViewOnly {
        view_only: bool,
    },
    /// Host to a joiner: `by` has taken you out of the session. The code is
    /// unchanged, so this is not a lock (§14.6).
    Removed {
        by: String,
    },
}

impl Msg {
    pub fn to_bytes(&self) -> Vec<u8> {
        postcard::to_allocvec(self).expect("a message always writes")
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Msg, WireError> {
        postcard::from_bytes(bytes).map_err(|e| WireError(format!("not a message: {e}")))
    }
}

/// An [`Edit`] inside a postcard message, as the JSON it survives (F52).
mod as_json {
    use serde::de::{Error, SeqAccess, Visitor};
    use serde::{Deserializer, Serializer};

    use super::Edit;

    pub fn serialize<S: Serializer>(edit: &Edit, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_bytes(&edit.to_bytes())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Edit, D::Error> {
        struct Bytes;
        impl<'de> Visitor<'de> for Bytes {
            type Value = Vec<u8>;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("an edit's bytes")
            }

            fn visit_bytes<E: Error>(self, bytes: &[u8]) -> Result<Vec<u8>, E> {
                Ok(bytes.to_vec())
            }

            fn visit_byte_buf<E: Error>(self, bytes: Vec<u8>) -> Result<Vec<u8>, E> {
                Ok(bytes)
            }

            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Vec<u8>, A::Error> {
                let mut bytes = Vec::new();
                while let Some(byte) = seq.next_element()? {
                    bytes.push(byte);
                }
                Ok(bytes)
            }
        }
        let bytes = deserializer.deserialize_byte_buf(Bytes)?;
        Edit::from_bytes(&bytes).map_err(|e| D::Error::custom(e.0))
    }
}
