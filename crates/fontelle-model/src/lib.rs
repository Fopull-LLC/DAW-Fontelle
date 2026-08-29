//! The document model (FONTELLE_TDD.md §10). Depends on nothing in the workspace
//! except `fontelle-types` (INVARIANT 4's counterpart for the model layer). Every
//! addressable element — including individual notes and automation points — carries
//! a persistent, stable ID; indices are never identity and never serialised as
//! references (INVARIANT 8).

mod arena;
mod asset_table;
mod automation;
mod channel;
mod clip;
mod command;
mod commands;
mod lane;
mod mixer;
mod note;
mod prefab;
mod project;
mod storage;

pub use arena::Arena;
pub use asset_table::AssetTable;
pub use automation::{AutomationData, AutomationPoint, CurveShape};
pub use channel::Channel;
pub use clip::{AudioClipData, Clip, ClipMap, ClipSource};
pub use command::{Command, CommandError, History};
pub use commands::{
    AddChannel, AddClip, AddNotes, DuplicateClip, FlagTarget, MoveClip, MoveNotes, NumberTarget,
    RemoveChannel, RemoveClip, RemoveNotes, ResizeNotes, SetChannelPatch, SetFlag, SetLoopRange,
    SetNumber,
};
pub use lane::Lane;
pub use mixer::{EffectSlot, Mixer, MixerTrack, PanLaw, Send};
pub use note::{Note, NoteData};
pub use prefab::{ElementId, OverrideMap, Prefab, PrefabLink, PropKey, PropValue, resolve};
pub use project::{Marker, Project, ProjectMeta, TempoMap, TempoSegment, ViewState};
pub use storage::{
    BUNDLE_DIRS, PROJECT_FILE, PROJECT_FORMAT_VERSION, StorageError, load_project, save_project,
};
