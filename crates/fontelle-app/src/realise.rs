//! Turning a `Project` into something the engine can play.
//!
//! This is the step the document model was missing. Before it existed,
//! `fontelle-app` hand-assigned node ids and buses from a private `Song` type,
//! `Project::mixer` was read by nothing, and `Channel::patch_data` was left
//! empty — so the document was a description of the music that the thing
//! actually making sound did not consult. Save/load, undo, the mixer UI and
//! choosing an instrument all land here.
//!
//! It lives in `fontelle-app` because it is the one layer allowed to see both
//! the model and the engine (TDD §4.1). `fontelle-sequencer` may not, which is
//! why `compile` takes the `ChannelId -> NodeId` map as a parameter; this is
//! the step that owns it.

use std::collections::{HashMap, HashSet};

use fontelle_core::{Patch, PatchFormatError, PrepareContext, Sampler, UnresolvedSample};
use fontelle_engine::{
    BufferPool, CompiledGraph, MasterMeter, MixerTrackNode, SamplerNode, ScheduledNode,
};
use fontelle_model::{MixerTrack, Project};
use fontelle_types::{ChannelId, MixerTrackId, NodeId, PatchData, SampleRef};

use crate::library::SampleLibrary;

/// Buffers 0 and 1 are the master pair; every other track's bus follows them.
const MASTER_BUSES: usize = 2;

/// The session settings a document does not carry.
///
/// None of these are project data: the sample rate belongs to the audio
/// device, the block size to the engine, and the interpolation quality to
/// whether this is playback or an export (TDD §7.6).
#[derive(Debug, Clone, Copy)]
pub struct RealiseOptions {
    pub sample_rate: u32,
    pub block_size: usize,
    pub quality: fontelle_dsp::Interpolation,
}

/// Everything the engine needs in order to play a document.
pub struct Realised {
    pub graph: CompiledGraph,
    /// Which engine node renders each channel — what
    /// `fontelle_sequencer::compile` takes.
    ///
    /// Every channel is here, including one with no instrument yet. The
    /// timeline's shape then depends only on the notes, so choosing or
    /// changing an instrument does not invalidate a compiled timeline; the
    /// events simply reach a node that is not in the schedule, and nothing
    /// sounds.
    pub channel_nodes: HashMap<ChannelId, NodeId>,
    /// Master levels for anything off the RT thread. Has to be taken before
    /// the graph goes to the audio callback, because after that nothing owns
    /// the node.
    pub master: std::sync::Arc<MasterMeter>,
    /// Samples a channel's patch pointed at that this library does not have
    /// (TDD §17.4). Those layers are silent and the project still plays; the
    /// references are here for a relink dialog to work on.
    pub unresolved: Vec<(ChannelId, UnresolvedSample)>,
}

/// Why a document could not be turned into a graph.
#[derive(Debug)]
pub enum RealiseError {
    /// TDD §13.2: a feedback loop must never reach the graph compiler.
    MixerCycle,
    /// `Project::mixer.master` names nothing. Every project has a master
    /// (§13.1) and `Project::new` creates one, so this is a damaged document
    /// rather than an ordinary state.
    NoMaster,
    /// A channel's stored patch could not be read — a project from a newer
    /// build, or a corrupt one.
    Patch {
        channel: ChannelId,
        error: PatchFormatError,
    },
}

impl std::fmt::Display for RealiseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MixerCycle => f.write_str(
                "the mixer routes a track back into itself, which would feed back — \
                 fix the routing before playing",
            ),
            Self::NoMaster => f.write_str("this project has no master mixer track"),
            Self::Patch { error, .. } => {
                write!(f, "a channel's instrument could not be read: {error}")
            }
        }
    }
}

impl std::error::Error for RealiseError {}

/// Mints the engine node id for the nth channel of a document.
///
/// Index 0 is deliberately skipped: a slotmap's zero key is its null key, and
/// a null target would match every node that was not given an id of its own.
fn node_id(index: usize) -> NodeId {
    NodeId::from(slotmap::KeyData::from_ffi(index as u64 + 1))
}

/// Puts `patch` on `channel`, in the form the document stores (TDD §8.3).
///
/// The provenance comes from the library, because a stored layer names its
/// audio by file and only the library knows which file each decoded sample
/// came out of.
pub fn set_channel_patch(
    project: &mut Project,
    channel: ChannelId,
    patch: &Patch,
    library: &SampleLibrary,
) -> Result<(), PatchFormatError> {
    let data = patch.to_data(library.provenance())?;
    if let Some(channel) = project.channels.get_mut(channel) {
        channel.patch_data = Some(data);
    }
    Ok(())
}

/// How many steps from `track` to the master, following `output`.
///
/// Only called after [`fontelle_model::Mixer::has_cycle`] has said no, so the
/// walk terminates; the bound is a belt-and-braces guard against a document
/// that changed underneath, not the termination argument.
fn depth_to_master(project: &Project, track: MixerTrackId, master: MixerTrackId) -> usize {
    let mut steps = 0;
    let mut current = track;
    while current != master && steps <= project.mixer.tracks.len() {
        match project.mixer.tracks.get(current).and_then(|t| t.output) {
            Some(next) => {
                current = next;
                steps += 1;
            }
            // `output: None` means master (TDD §13.1).
            None => return steps + 1,
        }
    }
    steps
}

/// The tracks a solo leaves audible.
///
/// A soloed track routed into a group is inaudible unless the group stays
/// open, and soloing a group has to keep everything feeding it. So audibility
/// spreads in both directions along `output`: a track is audible if it is
/// soloed, if it feeds a soloed track, or if a soloed track feeds it.
///
/// `None` means nothing is soloed and every track is audible — which is not
/// the same as "the set of all tracks", because that would silence a channel
/// whose mixer track has been deleted.
fn soloed_audible(project: &Project, master: MixerTrackId) -> Option<HashSet<MixerTrackId>> {
    let soloed: Vec<MixerTrackId> = project
        .mixer
        .tracks
        .iter()
        .filter(|(_, t)| t.solo)
        .map(|(id, _)| id)
        .collect();
    if soloed.is_empty() {
        return None;
    }

    let mut audible: HashSet<MixerTrackId> = soloed.iter().copied().collect();
    // Upwards: everything carrying a soloed track to the speakers.
    for start in &soloed {
        let mut current = *start;
        for _ in 0..=project.mixer.tracks.len() {
            let Some(next) = project.mixer.tracks.get(current).and_then(|t| t.output) else {
                audible.insert(master);
                break;
            };
            if !audible.insert(next) && next == master {
                break;
            }
            current = next;
        }
    }
    // Downwards: everything feeding one. A track feeds a soloed track exactly
    // when its own walk to master passes through one, so this is the same walk
    // read the other way round.
    let feeders: Vec<MixerTrackId> = project
        .mixer
        .tracks
        .keys()
        .filter(|id| {
            let mut current = *id;
            for _ in 0..=project.mixer.tracks.len() {
                if soloed.contains(&current) {
                    return true;
                }
                match project.mixer.tracks.get(current).and_then(|t| t.output) {
                    Some(next) => current = next,
                    None => return false,
                }
            }
            false
        })
        .collect();
    audible.extend(feeders);
    Some(audible)
}

/// The engine node that will render each channel.
///
/// Split out of [`realise`] because compiling the timeline needs it and
/// building a graph does not: a project can be compiled to a `CompiledTimeline`
/// before any instrument has been chosen, and the events then reach nodes that
/// are not in the schedule and are heard by nobody.
pub fn channel_nodes(project: &Project) -> HashMap<ChannelId, NodeId> {
    project
        .channels
        .keys()
        .enumerate()
        .map(|(index, id)| (id, node_id(index)))
        .collect()
}

pub fn realise(
    project: &Project,
    library: &SampleLibrary,
    options: RealiseOptions,
) -> Result<Realised, RealiseError> {
    if project.mixer.has_cycle() {
        return Err(RealiseError::MixerCycle);
    }
    let master = project.mixer.master.ok_or(RealiseError::NoMaster)?;
    if !project.mixer.tracks.contains_key(master) {
        return Err(RealiseError::NoMaster);
    }

    // --- Buses. Master owns 0 and 1; every other track takes the next pair.
    let mut bus_of: HashMap<MixerTrackId, [usize; 2]> = HashMap::new();
    bus_of.insert(master, [0, 1]);
    for (index, id) in project
        .mixer
        .tracks
        .keys()
        .filter(|id| *id != master)
        .enumerate()
    {
        bus_of.insert(id, [MASTER_BUSES + index * 2, MASTER_BUSES + index * 2 + 1]);
    }
    let buffer_count = MASTER_BUSES + (bus_of.len() - 1) * 2;

    let audible = soloed_audible(project, master);
    let is_audible = |id: MixerTrackId| audible.as_ref().is_none_or(|set| set.contains(&id));

    // --- One node id per channel, whether or not it has an instrument yet.
    let channel_nodes = channel_nodes(project);

    let mut schedule: Vec<ScheduledNode> = Vec::new();
    let mut unresolved = Vec::new();

    // --- Sources first. They only ever add into a bus, so their order among
    // themselves does not matter; what does matter is that every one of them
    // has run before the fader on the bus it feeds, and every fader comes
    // after this loop.
    for (channel_id, channel) in project.channels.iter() {
        let Some(data) = &channel.patch_data else {
            continue; // no instrument chosen yet — it plays nothing
        };
        let loaded = load_patch(data, library).map_err(|error| RealiseError::Patch {
            channel: channel_id,
            error,
        })?;
        unresolved.extend(loaded.unresolved.into_iter().map(|u| (channel_id, u)));

        let mut sampler = Sampler::new(loaded.patch);
        sampler.prepare(&PrepareContext {
            sample_rate: options.sample_rate as f32,
            max_block_size: options.block_size as u32,
        });
        sampler.set_quality(options.quality);
        // Constant-power placement at the voice, not the track's balance
        // control — see `Channel::pan`.
        sampler.set_pan(channel.pan);

        // A channel pointing at a mixer track that is gone lands on the
        // master: a part you can hear and fix beats one that vanished.
        let bus = bus_of.get(&channel.mixer_track).copied().unwrap_or([0, 1]);
        schedule.push(ScheduledNode {
            id: channel_nodes[&channel_id],
            node: Box::new(SamplerNode::new(sampler, library.store())),
            input_buffers: Vec::new(),
            output_buffers: bus.to_vec(),
        });
    }

    // --- Then every track, deepest first, so a group's fader runs only after
    // everything feeding it has been summed in.
    let mut tracks: Vec<MixerTrackId> = project
        .mixer
        .tracks
        .keys()
        .filter(|id| *id != master)
        .collect();
    tracks.sort_by_key(|id| std::cmp::Reverse(depth_to_master(project, *id, master)));

    for id in tracks {
        let track = &project.mixer.tracks[id];
        let bus = bus_of[&id].to_vec();
        schedule.push(ScheduledNode {
            id: NodeId::default(),
            node: Box::new(fader(track, is_audible(id))),
            // Same buffers in and out: a fader processes in place.
            input_buffers: bus.clone(),
            output_buffers: bus.clone(),
        });
        let output = track.output.unwrap_or(master);
        schedule.push(ScheduledNode {
            id: NodeId::default(),
            node: Box::new(fontelle_engine::BusSumNode),
            input_buffers: bus,
            output_buffers: bus_of.get(&output).copied().unwrap_or([0, 1]).to_vec(),
        });
    }

    schedule.push(ScheduledNode {
        id: NodeId::default(),
        node: Box::new(fader(&project.mixer.tracks[master], is_audible(master))),
        input_buffers: vec![0, 1],
        output_buffers: vec![0, 1],
    });
    // Last, after every track has arrived: a brickwall limiter and the master
    // meters. This is what lets the master fader sit at unity — the peaks an
    // arrangement reaches are a property of the material, and picking a gain
    // that neither clips nor throws away 20 dB was a judgement the tool could
    // not make.
    let master_node = fontelle_engine::MasterNode::new();
    let meter = master_node.meter();
    schedule.push(ScheduledNode {
        id: NodeId::default(),
        node: Box::new(master_node),
        input_buffers: vec![0, 1],
        output_buffers: vec![0, 1],
    });

    let mut graph = CompiledGraph {
        schedule,
        buffer_pool: BufferPool::with_capacity(buffer_count, options.block_size),
    };
    graph.prepare(options.sample_rate as f32, options.block_size as u32);

    Ok(Realised {
        graph,
        channel_nodes,
        master: meter,
        unresolved,
    })
}

fn load_patch(
    data: &PatchData,
    library: &SampleLibrary,
) -> Result<fontelle_core::LoadedPatch, PatchFormatError> {
    Patch::from_data(data, |file: &SampleRef| library.resolve(file))
}

/// The engine node for one document mixer track.
///
/// **Inserts and sends are not compiled** — effects and sends are M4, and the
/// gate this is being built for balances a piece with gain, pan and mute. A
/// send is a `BusSumNode` with a level and a pan, so the shape is already here
/// when it is wanted.
fn fader(track: &MixerTrack, audible: bool) -> MixerTrackNode {
    MixerTrackNode {
        gain_db: track.gain_db,
        pan: track.pan,
        pan_law: track.pan_law,
        mute: track.mute || !audible,
        phase_invert: track.phase_invert,
    }
}
