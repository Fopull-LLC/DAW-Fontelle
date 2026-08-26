//! The document model (FONTELLE_TDD.md §10). Depends on nothing in the workspace
//! except `fontelle-types` (INVARIANT 4's counterpart for the model layer). Every
//! addressable element — including individual notes and automation points — carries
//! a persistent, stable ID; indices are never identity and never serialised as
//! references (INVARIANT 8).

mod asset_table;
mod automation;
mod channel;
mod clip;
mod command;
mod lane;
mod mixer;
mod note;
mod prefab;
mod project;

pub use asset_table::AssetTable;
pub use automation::{AutomationData, AutomationPoint, CurveShape};
pub use channel::Channel;
pub use clip::{AudioClipData, Clip, ClipMap, ClipSource};
pub use command::{Command, CommandError, History};
pub use lane::Lane;
pub use mixer::{EffectSlot, Mixer, MixerTrack, PanLaw, Send};
pub use note::{Note, NoteData};
pub use prefab::{ElementId, OverrideMap, Prefab, PrefabLink, PropKey, PropValue, resolve};
pub use project::{Marker, Project, ProjectMeta, TempoMap, TempoSegment, ViewState};
