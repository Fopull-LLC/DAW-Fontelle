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
    RestoreMarker
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
