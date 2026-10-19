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
use crate::lane_routing::{LaneUpkeep, SetClipChannel, SetRoutingMode};
use crate::study::{
    AddStudy, RemoveStudy, RestoreStudy, SetStudyClean, SetStudyEdits, SetStudyMarkers,
    SetStudyOriginal, SetStudyRender, SetStudyTakes,
};
use crate::vocal::{
    SetLyrics, SetNoteVoices, SetSinger, SetVocalLane, SetVocalSettings, SetVocalState,
    SetVocalStyle,
};
use crate::vocal_files::{FreezeVocal, ImportVocal, UnfreezeVocal, UnimportVocal};

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
    MoveMixerTrack,
    DuplicateMixerTrack,
    SetMixerOrder,
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
    SetNoteKeys,
    SetKey,
    AddAudioClip,
    RemoveAudioClip,
    SetTrackInput,
    SetAudioClip,
    SetAudioClips,
    ImportParts,
    UnimportParts,
    AddClip,
    RemoveClip,
    RestoreClip,
    MoveClip,
    MoveClips,
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
    SetMixerTrackColor,
    SetRoutingMode,
    LaneUpkeep,
    SetClipChannel,
    SetNotePath,
    ResizeClips,
    RenameClip,
    AddStudy,
    RemoveStudy,
    RestoreStudy,
    SetStudyEdits,
    SetStudyRender,
    SetStudyMarkers,
    SetStudyClean,
    SetStudyTakes,
    SetStudyOriginal,
    // v0.26.2: a delete that takes an instrument's notes, not the clips it
    // shares (`DeleteChannel`). Appended: a name on the wire is forever.
    DeleteChannel,
    RestoreDeletedChannel,
    // The vocal track (feature/vocal-track), after the v0.26.2 release.
    SetLyrics,
    SetSinger,
    SetNoteVoices,
    SetVocalLane,
    SetVocalSettings,
    SetVocalState,
    ImportVocal,
    UnimportVocal,
    FreezeVocal,
    UnfreezeVocal,
    SetVocalStyle,
);

/// What an edit changes in the running studio, and so what the studio has to
/// redo when it arrives from somebody else — see [`Edit::reach`].
///
/// Ordered by cost, so a batch of edits costs its dearest (`max`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Reach {
    /// What plays when: the compiled timeline, republished. Every sound that
    /// is ringing keeps ringing.
    Timeline,
    /// A live value — a fader, a send, an effect's knob — set in place on
    /// the running graph.
    Values,
    /// What is wired to what, or what an instrument is: the graph rebuilt,
    /// which ends every voice that was sounding.
    Graph,
}

impl Edit {
    /// What this edit reaches — the same as what the studio does when it
    /// makes this edit itself, so an edit that arrives from somebody else
    /// costs no more than the local one.
    ///
    /// > *"when the other person makes changes it pauses my audio playback so
    /// > cuts my notes short"* (2026-10-18): every arriving edit rebuilt the
    /// > graph, a note moved by a collaborator included.
    ///
    /// **Exhaustive on purpose**: a new edit does not compile until somebody
    /// has said what it reaches, and anything not known to be lighter is
    /// [`Reach::Graph`].
    pub fn reach(&self) -> Reach {
        use crate::{FlagTarget, NumberTarget};
        match self {
            // As dear as its dearest part; an empty one changes nothing.
            Edit::Compound { parts, .. } => parts
                .iter()
                .map(Edit::reach)
                .max()
                .unwrap_or(Reach::Timeline),
            // What plays when.
            Edit::AddNotes(_)
            | Edit::RemoveNotes(_)
            | Edit::RestoreNotes(_)
            | Edit::MoveNotes(_)
            | Edit::ResizeNotes(_)
            | Edit::SetNoteVelocity(_)
            | Edit::RestoreNoteVelocities(_)
            | Edit::SetNoteSlide(_)
            | Edit::RestoreNoteSlides(_)
            | Edit::SliceNotes(_)
            | Edit::MergeSlices(_)
            | Edit::SetNoteProperty(_)
            | Edit::RestoreNoteProperties(_)
            | Edit::NudgeNoteProperty(_)
            | Edit::SetNotePropertyEach(_)
            | Edit::SetNoteLengths(_)
            | Edit::SetNoteKeys(_)
            | Edit::SetNotePath(_)
            | Edit::SetKey(_)
            | Edit::AddClip(_)
            | Edit::RemoveClip(_)
            | Edit::RestoreClip(_)
            | Edit::MoveClip(_)
            | Edit::MoveClips(_)
            | Edit::SetClipLoop(_)
            | Edit::ResizeClip(_)
            | Edit::ResizeClips(_)
            | Edit::TrimClipStart(_)
            | Edit::DuplicateClip(_)
            | Edit::SplitClip(_)
            | Edit::UnsplitClip(_)
            | Edit::RenameClip(_)
            | Edit::SetAudioClip(_)
            | Edit::SetAudioClips(_)
            | Edit::SetLoopRange(_)
            | Edit::AddAutomationPoint(_)
            | Edit::MoveAutomationPoints(_)
            | Edit::PlaceAutomationPoints(_)
            | Edit::RemoveAutomationPoints(_)
            | Edit::RestoreAutomationPoints(_)
            | Edit::SetPointCurve(_)
            | Edit::RestorePointCurves(_)
            | Edit::AddMarker(_)
            | Edit::RemoveMarker(_)
            | Edit::RestoreMarker(_)
            | Edit::RenameLane(_)
            | Edit::MoveLane(_)
            | Edit::RenameProject(_)
            | Edit::RenameMixerTrack(_)
            | Edit::SetMixerTrackColor(_)
            | Edit::RenameChannel(_)
            | Edit::RenamePrefab(_)
            | Edit::EditNotepad(_) => Reach::Timeline,
            Edit::SetNumber(set) => match set.target() {
                NumberTarget::TrackGainDb(_) | NumberTarget::TrackPan(_) => Reach::Values,
                NumberTarget::Tempo | NumberTarget::BeatsPerBar => Reach::Timeline,
                // The channel's own level and placement are rebuilt locally
                // too (`set_instrument_param`).
                NumberTarget::ChannelPan(_) | NumberTarget::ChannelGainDb(_) => Reach::Graph,
            },
            Edit::SetFlag(set) => match set.target() {
                FlagTarget::TrackMute(_) | FlagTarget::TrackSolo(_) => Reach::Values,
                // Sequencer mutes: what is compiled to play.
                FlagTarget::ClipMuted(_)
                | FlagTarget::LaneMuted(_)
                | FlagTarget::LaneSoloed(_)
                | FlagTarget::ChannelMuted(_)
                | FlagTarget::ChannelSoloed(_) => Reach::Timeline,
                FlagTarget::TrackOutputOn(_)
                | FlagTarget::ChannelNamedKeys(_)
                | FlagTarget::ChannelPluginMpe(_) => Reach::Graph,
            },
            // A live value, published to the running graph.
            Edit::SetSendLevel(_)
            | Edit::SetInsertParam(_)
            | Edit::SetInsertMix(_)
            | Edit::SetEqBand(_)
            | Edit::SetInsertBypassed(_) => Reach::Values,
            // Everything else changes what the graph is.
            Edit::NotApplied(_)
            | Edit::AddChannel(_)
            | Edit::RemoveChannel(_)
            | Edit::RestoreChannel(_)
            | Edit::DuplicateChannel(_)
            | Edit::AddLane(_)
            | Edit::RemoveLane(_)
            | Edit::RestoreLane(_)
            | Edit::AddMixerTrack(_)
            | Edit::RemoveMixerTrack(_)
            | Edit::RestoreMixerTrack(_)
            | Edit::MoveMixerTrack(_)
            | Edit::DuplicateMixerTrack(_)
            | Edit::SetMixerOrder(_)
            | Edit::SetTrackOutput(_)
            | Edit::AddSend(_)
            | Edit::RemoveSend(_)
            | Edit::RestoreSend(_)
            | Edit::SetSendPreFader(_)
            | Edit::SetChannelRoute(_)
            | Edit::SetChannelKind(_)
            | Edit::RestoreChannelKind(_)
            | Edit::SetChannelPatch(_)
            | Edit::AddAudioClip(_)
            | Edit::RemoveAudioClip(_)
            | Edit::SetTrackInput(_)
            | Edit::ImportParts(_)
            | Edit::UnimportParts(_)
            | Edit::ReplaceClip(_)
            | Edit::ApplyTrackChain(_)
            | Edit::RestoreTrackChain(_)
            | Edit::AddInsert(_)
            | Edit::RemoveInsert(_)
            | Edit::RestoreInsert(_)
            | Edit::MoveInsert(_)
            | Edit::SetInsertNotes(_)
            | Edit::SetInsertKey(_)
            | Edit::RestoreInsertConfig(_)
            | Edit::EditLapse(_)
            | Edit::AddPluginInsert(_)
            | Edit::SetChannelPlugin(_)
            | Edit::RestoreChannelPlugin(_)
            | Edit::SetPluginParam(_)
            | Edit::RestorePluginParam(_)
            | Edit::ApplyPreset(_)
            | Edit::SwitchChannelAb(_)
            | Edit::CopyChannelAb(_)
            | Edit::RestoreChannelAbOther(_)
            | Edit::RestoreDeviceState(_)
            | Edit::SetPresetRef(_)
            | Edit::AddPrefab(_)
            | Edit::AddPrefabInstance(_)
            | Edit::DetachPrefab(_)
            | Edit::RestorePrefabLink(_)
            | Edit::RemovePrefab(_)
            | Edit::RestorePrefab(_)
            | Edit::MakePrefabFromClip(_)
            | Edit::RelocateAssets(_)
            | Edit::RestoreAssets(_)
            | Edit::SetRoutingMode(_)
            | Edit::LaneUpkeep(_)
            | Edit::SetClipChannel(_)
            | Edit::AddStudy(_)
            | Edit::RemoveStudy(_)
            | Edit::RestoreStudy(_)
            | Edit::SetStudyEdits(_)
            | Edit::SetStudyRender(_)
            | Edit::SetStudyMarkers(_)
            | Edit::SetStudyClean(_)
            | Edit::SetStudyTakes(_)
            | Edit::SetStudyOriginal(_)
            | Edit::DeleteChannel(_)
            | Edit::RestoreDeletedChannel(_)
            | Edit::SetLyrics(_)
            | Edit::SetSinger(_)
            | Edit::SetNoteVoices(_)
            | Edit::SetVocalLane(_)
            | Edit::SetVocalSettings(_)
            | Edit::SetVocalState(_)
            | Edit::ImportVocal(_)
            | Edit::UnimportVocal(_)
            | Edit::FreezeVocal(_)
            | Edit::UnfreezeVocal(_)
            | Edit::SetVocalStyle(_) => Reach::Graph,
        }
    }
}

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

/// The version of the conversation below, **and of the song as written**:
/// two studios compare a fingerprint of the written song, so a build that
/// writes it differently cannot share with one that does not, and is refused
/// at the door. `fontelle-app`'s `tests/song_form.rs` fails until this moves
/// with the song's form. The same Fontelle on both ends is also required
/// (§8.3); this is what still tells two test builds of one version apart.
///
/// 2: solo is no longer written (each studio's own).
pub const PROTOCOL: u32 = 2;

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
    // A link that dropped and came back (v0.25.1).
    /// Host to a joiner it has back — its own connection to the relay
    /// returned, or the joiner's did — just before a fresh copy of the song:
    /// the last of the joiner's proposals (`Propose::local_seq`) it has
    /// applied or refused. The copy has every one of those that was applied;
    /// the joiner proposes again whatever came after, which never arrived.
    Resumed {
        handled: u64,
    },
    /// Joiner to host, ahead of every `Hello`: which join this is, the same
    /// for as long as the joiner holds the song open. A hello again under the
    /// same session is the same person back, not a new one.
    Session {
        id: fontelle_types::PersistentId,
    },
    // Singers (F93).
    /// Host to everybody: every singer the song uses, and how somebody who
    /// has not got one gets it — sent with the song, and again whenever the
    /// song starts using another.
    Singers {
        singers: Vec<SingerNeed>,
    },
}

/// A singer a shared song uses (`Msg::Singers`), as the song names it
/// (`VocalVoice`'s engine and singer), with what a studio without it can do.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SingerNeed {
    pub engine: String,
    pub singer: String,
    /// What it is called, for a person.
    pub name: String,
    pub offer: SingerOffer,
}

/// How a studio without a singer gets it.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SingerOffer {
    /// A Fontelle Singer bank whose terms let it be passed on: the host
    /// sends it, packed, by this hash, once the person has read `terms` and
    /// agreed.
    Send {
        hash: u64,
        size: u64,
        file_name: String,
        terms: Option<String>,
    },
    /// A Fontelle Singer bank that may not be passed on (or nobody knows
    /// whether it may): where to get it — the catalogue's download or the
    /// author's page — when anybody knows.
    GetIt { link: Option<String> },
    /// A singer of another engine's: the engine has to be installed, and
    /// cannot be sent.
    Engine,
    /// A voice made for research only, whose output must not be shared:
    /// never sent.
    ResearchOnly,
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
