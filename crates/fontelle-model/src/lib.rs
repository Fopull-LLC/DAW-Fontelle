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
mod recording;
mod storage;
mod tools;

pub use arena::Arena;
pub use asset_table::AssetTable;
pub use automation::{AutomationData, AutomationPoint, CurveShape, curve_value};
pub use channel::Channel;
pub use clip::{AudioClipData, Clip, ClipMap, ClipSource};
pub use command::{Command, CommandError, History};
pub use commands::{
    AddAudioClip, AddAutomationPoint, AddChannel, AddClip, AddInsert, AddLane, AddMixerTrack,
    AddNotes, AddSend,
    Compound,
    DuplicateChannel, DuplicateClip, FlagTarget, ImportPart, ImportParts, MIN_CLIP_LENGTH, MadePart, MoveAutomationPoints, MoveClip, MoveInsert,
    MoveLane, MoveNotes, NudgeNoteProperty, NumberTarget, PlaceAutomationPoints, RemoveAutomationPoints, RemoveChannel,
    NEW_SEND_DB, RemoveClip, RemoveInsert, RemoveLane, RemoveMixerTrack, RemoveNotes, RemoveSend,
    RenameChannel, RenameLane, RenameMixerTrack,
    ResizeClip, ResizeNotes, RestoreAutomationPoints, RestoreInsert, RestorePointCurves,
    SetAudioClip, SetChannelPatch, SetChannelRoute, SetClipLoop, SetEqBand, SetFlag,
    SetInsertBypassed,
    RestoreInsertConfig, SetInsertKey, SetInsertMix, SetInsertParam, SetInsertPreset,
    SetLoopRange, SetNoteProperty, SetNotePropertyEach, SetNoteSlide, SetNoteVelocity,
    SetNumber, SetPointCurve,
    SetSendLevel, SetSendPreFader, SetTrackOutput, SliceNotes, SplitClip,
};
pub use lane::Lane;
pub use mixer::{EffectSlot, Mixer, MixerTrack, PanLaw, Send};
pub use note::{Note, NoteData, NoteProperty};
pub use prefab::{ElementId, OverrideMap, Prefab, PrefabLink, PropKey, PropValue, resolve};
pub use project::{Marker, Project, ProjectMeta, TempoMap, TempoSegment, ViewState};
pub use recording::notes_from_capture;
pub use tools::{MAX_RANDOM_AMOUNT, RandomMode, RandomSpec, randomised};
pub use storage::{
    BUNDLE_DIRS, PROJECT_FILE, PROJECT_FORMAT_VERSION, StorageError, load_project, save_project,
};

/// What `target` is automated to at `tick` in song time, or `None` if nothing
/// has automated it yet (TDD §12.2).
///
/// The two rules the TDD says FL leaves implicit, and this project must not:
///
/// 1. **Overlapping clips on the same target: the later-starting one wins**
///    for the duration of the overlap. Additive blending was considered and
///    rejected as confusing — and a mix that is the sum of two automation
///    lanes is one nobody can reason about by looking at either.
/// 2. **After a clip ends the parameter holds the value it left**, rather than
///    snapping back to where the knob was sitting. This is what people expect
///    and it is what makes a sweep that ends high stay high.
///
/// `None` *before* any clip has started, though: rule 2 is about after. A clip
/// cannot reach back in time and hold a value from before it existed, and a
/// parameter nothing has automated yet is the knob's, not the lane's.
pub fn automation_at(
    project: &Project,
    target: &fontelle_types::ParamAddress,
    tick: fontelle_types::Tick,
) -> Option<f64> {
    let mut best: Option<(fontelle_types::Tick, f64)> = None;
    for (_, clip) in project.clips.iter() {
        if clip.muted {
            continue;
        }
        let ClipSource::Automation(data) = &clip.source else {
            continue;
        };
        if data.target != *target || clip.start > tick {
            continue;
        }
        // Inside the clip it is the curve; past the end it is the value the
        // clip left behind.
        let value = if tick < clip.start + clip.length {
            data.value_at(tick - clip.start)
        } else {
            data.final_value()
        };
        let Some(value) = value else { continue };
        // Later start wins, which is rule 1. Compared by *time* rather than by
        // position in the arena, because the arena's order is the order the
        // clips were drawn and not the order they play.
        if best.is_none_or(|(start, _)| clip.start >= start) {
            best = Some((clip.start, value));
        }
    }
    best.map(|(_, value)| value)
}

/// Every parameter this project automates, each named once however many clips
/// aim at it.
///
/// What the compiler needs in order to know what to emit, and what the
/// timeline needs in order to know which lanes to draw.
pub fn automated_targets(project: &Project) -> Vec<fontelle_types::ParamAddress> {
    let mut targets: Vec<fontelle_types::ParamAddress> = project
        .clips
        .iter()
        .filter_map(|(_, clip)| match &clip.source {
            ClipSource::Automation(data) => Some(data.target.clone()),
            _ => None,
        })
        .collect();
    targets.sort();
    targets.dedup();
    targets
}

/// How finely a tempo lane is turned into tempo segments, in ticks.
///
/// A sixteenth note. Finer and a four-minute ritardando is tens of thousands
/// of segments for a smoothness nobody can hear; coarser and it is a
/// staircase. The map's own conversions are a binary search, so the count
/// costs nothing per lookup.
pub const TEMPO_AUTOMATION_STEP: fontelle_types::Tick = fontelle_types::PPQN / 4;

/// The tempo map the song actually plays by: the document's own, bent by
/// whatever automation aims at the tempo (TDD §6.2, §12.3).
///
/// §12.3 is explicit that these are one system — *"the tempo map is
/// generated by evaluating the tempo automation"* — and this is where they
/// become one. The document stores the map the tempo box sets, which is the
/// tempo before any lane and the tempo of a song with none; a tempo lane is
/// an ordinary automation clip aimed at `transport/tempo`, and its value is
/// read through the mapping `fontelle_types::tempo_from_normalised` publishes.
///
/// The same two rules every other lane follows (see [`automation_at`]):
/// before the first clip the box's map stands, because a clip cannot reach
/// back in time; after the last one the tempo holds what the lane left. The
/// curve inside is sampled every [`TEMPO_AUTOMATION_STEP`] into constant
/// segments, with a run of equal steps kept as one segment so a flat lane
/// costs one.
///
/// Everything that converts a tick to a sample — the compiler, the window's
/// playhead, a loop range — has to go through *this* map rather than
/// `project.tempo_map`, or a ritardando is drawn, saved, and inaudible.
pub fn effective_tempo_map(project: &Project) -> TempoMap {
    let target = fontelle_types::ParamTarget::Tempo.address();
    let mut span: Option<(fontelle_types::Tick, fontelle_types::Tick)> = None;
    for (_, clip) in project.clips.iter() {
        if clip.muted {
            continue;
        }
        let ClipSource::Automation(data) = &clip.source else {
            continue;
        };
        if data.target != target {
            continue;
        }
        let (start, end) = (clip.start, clip.start + clip.length);
        span = Some(match span {
            Some((from, to)) => (from.min(start), to.max(end)),
            None => (start, end),
        });
    }
    let Some((from, to)) = span else {
        return project.tempo_map.clone();
    };

    // The box's own segments, up to where the lane begins.
    let mut segments: Vec<TempoSegment> = project
        .tempo_map
        .segments()
        .iter()
        .copied()
        .filter(|s| s.start_tick < from)
        .collect();
    let mut last_bpm: Option<f64> = None;
    let mut tick = from;
    loop {
        if let Some(value) = automation_at(project, &target, tick) {
            let bpm = fontelle_types::tempo_from_normalised(value);
            if last_bpm.is_none_or(|previous| (previous - bpm).abs() > 1e-9) {
                segments.push(TempoSegment {
                    start_tick: tick,
                    bpm,
                });
                last_bpm = Some(bpm);
            }
        }
        if tick >= to {
            break;
        }
        tick = (tick + TEMPO_AUTOMATION_STEP).min(to);
    }
    TempoMap::from_segments(segments, project.tempo_map.sample_rate_hz())
}
