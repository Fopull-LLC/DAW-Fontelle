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
    BufferPool, CompiledGraph, MasterMeter, Metronome, MetronomeNode, MixerTrackNode, SamplerNode,
    SendControls, SendNode,
    ScheduledNode, TrackControls,
};
use fontelle_model::{Command, CommandError, MixerTrack, Project};
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
    /// Which engine node plays the **audio clips** routed to each mixer track
    /// (TDD §15) — the other half of what `fontelle_sequencer::compile_with`
    /// takes.
    ///
    /// Every track is here, whether or not any clip names it, for the reason
    /// every channel is in `channel_nodes`: the timeline's shape then depends
    /// only on the clips, so dropping a file on a track does not have to wait
    /// for a graph rebuild to be heard.
    ///
    /// **`None` and `Some(master)` are both here and are the same node**,
    /// matching `AudioClipData::mixer_track`, so a clip that names the master
    /// and one that leaves it unset play through one player rather than two.
    pub audio_nodes: HashMap<Option<MixerTrackId>, NodeId>,
    /// Master levels for anything off the RT thread. Has to be taken before
    /// the graph goes to the audio callback, because after that nothing owns
    /// the node.
    pub master: std::sync::Arc<MasterMeter>,
    /// The live end of every track's fader, and its meter — see
    /// [`fontelle_engine::TrackControls`]. Taken here for the same reason
    /// `master` is: once the graph is on the audio thread nothing else owns
    /// the nodes.
    ///
    /// This is what makes a mixer usable. Without it the only way to change a
    /// level while the project is playing is to build a whole new graph, which
    /// deserialises every channel's patch — fine for a click, hopeless for a
    /// drag.
    pub track_controls: HashMap<MixerTrackId, std::sync::Arc<TrackControls>>,
    /// The live end of every insert on every track, addressed the way the
    /// mixer panel addresses one: the strip, and the slot in its chain.
    ///
    /// Taken here for the reason `track_controls` is, and it exists for the
    /// same reason too — an EQ knob is dragged, and rebuilding the graph to
    /// move one number deserialises every channel's patch. A rebuild mints
    /// fresh ones from the document rather than carrying the old ones over:
    /// unlike a fader's `Arc`, a triple buffer's two ends cannot be re-paired,
    /// and the document is the source of truth either way (INVARIANT 9), so
    /// what a fresh channel starts at is what the last one was told.
    pub effect_controls: HashMap<(MixerTrackId, usize), fontelle_engine::EffectControls>,
    /// One analyser tap per insert, keyed the same way.
    ///
    /// **Carried across a rebuild**, unlike the controls above it: a tap is a
    /// ring of recent samples behind an `Arc`, so the two ends *can* be
    /// re-paired, and an EQ window that went blank every time somebody added a
    /// channel would be a window you could not use while you worked. See
    /// [`fontelle_engine::SpectrumTap`].
    pub spectrum_taps:
        HashMap<(MixerTrackId, usize), std::sync::Arc<fontelle_engine::SpectrumTap>>,
    /// Every automatable parameter this graph has, by its stable address
    /// (INVARIANT 7), and the node that owns it.
    ///
    /// This is the map that makes automation possible, and it lives here for
    /// the reason `channel_nodes` does: the document has addresses and the
    /// graph has node ids, and this is the one step that sees both (§4.1). The
    /// sequencer takes it and resolves every automation clip's target once,
    /// off the audio thread — matching a string per event per block on the RT
    /// side is work it should never be doing.
    pub param_nodes: HashMap<fontelle_types::ParamAddress, NodeId>,
    /// One live control surface per send, keyed by `(track, index into that
    /// track's `sends`)`.
    ///
    /// The same reason `track_controls` exists: a send level is something
    /// somebody **drags**, and `realise` deserialises every channel's patch —
    /// so a drag that rebuilt the graph per pixel would be unusable.
    pub send_controls: HashMap<(MixerTrackId, usize), std::sync::Arc<SendControls>>,
    /// The metronome's switch, for whoever is driving the transport.
    ///
    /// Taken here for the same reason `master` is: once the graph is on the
    /// audio thread nothing else owns the nodes. It is **not** document data —
    /// a project sent to somebody else must not arrive with a woodblock on
    /// every beat — so it is a session setting the window toggles.
    pub metronome: std::sync::Arc<Metronome>,
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
/// came out of. The write itself goes through a `SetChannelPatch` command like
/// every other mutation (INVARIANT 9) — this is the serialisation step, not a
/// second way into the document.
pub fn set_channel_patch(
    project: &mut Project,
    channel: ChannelId,
    patch: &Patch,
    library: &SampleLibrary,
) -> Result<(), CommandError> {
    let data = patch
        .to_data(library.provenance())
        .map_err(|e| CommandError(e.to_string()))?;
    fontelle_model::SetChannelPatch::new(channel, Some(data)).apply(project)
}

/// How many steps from `track` to the master, following `output`.
///
/// Only called after [`fontelle_model::Mixer::has_cycle`] has said no, so the
/// walk terminates; the bound is a belt-and-braces guard against a document
/// that changed underneath, not the termination argument.
fn depth_to_master(
    project: &Project,
    track: MixerTrackId,
    master: MixerTrackId,
    keyed_by: &HashMap<MixerTrackId, Vec<MixerTrackId>>,
) -> usize {
    // **Both edge kinds**, and the *longest* path, because a track has to be
    // scheduled after everything that feeds it — a send crossing from a
    // shallow track into a deeper bus is a feeding edge like any other, and
    // one measured along `output` alone would put the tap after the bus it
    // feeds had already been read.
    //
    // Depth-first with a visited set rather than a walk, since sends make this
    // a DAG rather than a chain. Acyclic by construction: every command that
    // writes an edge checks `Mixer::has_cycle` first (§13.2), and `realise`
    // refuses a document that slipped through.
    fn walk(
        project: &Project,
        track: MixerTrackId,
        master: MixerTrackId,
        keyed_by: &HashMap<MixerTrackId, Vec<MixerTrackId>>,
        seen: &mut HashSet<MixerTrackId>,
    ) -> usize {
        if track == master || !seen.insert(track) {
            return 0;
        }
        let Some(node) = project.mixer.tracks.get(track) else {
            return 0;
        };
        // `output: None` means master (TDD §13.1), which is depth one.
        let mut deepest = match node.output {
            Some(next) => walk(project, next, master, keyed_by, seen) + 1,
            None => 1,
        };
        for send in &node.sends {
            deepest = deepest.max(walk(project, send.target, master, keyed_by, seen) + 1);
        }
        // And every track this one **keys**: an insert reading another track's
        // bus is fed by it, so the source has to be scheduled first, which
        // means deeper. The third kind of edge, and the reason this takes a
        // reverse map — the document says which track an insert listens *to*,
        // and what is wanted here is who listens to this one.
        for keyed in keyed_by.get(&track).into_iter().flatten() {
            deepest = deepest.max(walk(project, *keyed, master, keyed_by, seen) + 1);
        }
        seen.remove(&track);
        deepest
    }
    walk(project, track, master, keyed_by, &mut HashSet::new())
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

    // Everything `track` feeds, one hop: its output and every send's target.
    // **Both**, because a send is a signal path like any other — a soloed
    // vocal whose reverb bus was closed would be a solo that took the reverb
    // away with everything else, and §13.2 is explicit that the two edge kinds
    // are the same kind of thing.
    let downstream = |track: MixerTrackId| -> Vec<MixerTrackId> {
        let Some(node) = project.mixer.tracks.get(track) else {
            return Vec::new();
        };
        let mut out: Vec<MixerTrackId> = vec![node.output.unwrap_or(master)];
        out.extend(node.sends.iter().map(|send| send.target));
        out
    };

    let mut audible: HashSet<MixerTrackId> = soloed.iter().copied().collect();
    // Upwards: everything carrying a soloed track to the speakers, by either
    // kind of edge. Breadth-first rather than a walk, since a track with sends
    // reaches the master by more than one path.
    let mut queue: Vec<MixerTrackId> = soloed.clone();
    while let Some(track) = queue.pop() {
        for next in downstream(track) {
            if audible.insert(next) && next != master {
                queue.push(next);
            }
        }
    }
    audible.insert(master);
    // Downwards: everything feeding one. A track feeds a soloed track exactly
    // when some path from it reaches one, so this is the same walk read the
    // other way round.
    let feeds_a_soloed_track = |start: MixerTrackId| -> bool {
        let mut seen: HashSet<MixerTrackId> = HashSet::new();
        let mut queue = vec![start];
        while let Some(track) = queue.pop() {
            if soloed.contains(&track) {
                return true;
            }
            if track == master || !seen.insert(track) {
                continue;
            }
            queue.extend(downstream(track));
        }
        false
    };
    let feeders: Vec<MixerTrackId> = project
        .mixer
        .tracks
        .keys()
        .filter(|id| feeds_a_soloed_track(*id))
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
    realise_reusing(project, library, options, &HashMap::new())
}

/// As [`realise`], but keeping the live control surfaces the graph being
/// replaced was already using, for every track that still exists.
///
/// What a running session calls. See [`fader`] for why the reuse matters.
pub fn realise_reusing(
    project: &Project,
    library: &SampleLibrary,
    options: RealiseOptions,
    existing: &HashMap<MixerTrackId, std::sync::Arc<TrackControls>>,
) -> Result<Realised, RealiseError> {
    realise_with(project, library, options, existing, None)
}

/// As [`realise_reusing`], keeping the metronome the previous graph was using.
///
/// The switch has to survive a rebuild — choosing a soundfont with the click
/// on must not turn it off — and so does the node's own beat, which the model
/// side wrote into it.
pub fn realise_with(
    project: &Project,
    library: &SampleLibrary,
    options: RealiseOptions,
    existing: &HashMap<MixerTrackId, std::sync::Arc<TrackControls>>,
    metronome: Option<std::sync::Arc<Metronome>>,
) -> Result<Realised, RealiseError> {
    realise_keeping(project, library, options, existing, metronome, &HashMap::new())
}

/// As [`realise_with`], keeping the analyser taps too — see
/// [`Realised::spectrum_taps`].
pub fn realise_keeping(
    project: &Project,
    library: &SampleLibrary,
    options: RealiseOptions,
    existing: &HashMap<MixerTrackId, std::sync::Arc<TrackControls>>,
    metronome: Option<std::sync::Arc<Metronome>>,
    existing_taps: &HashMap<(MixerTrackId, usize), std::sync::Arc<fontelle_engine::SpectrumTap>>,
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
    // Past every channel's id, so the two sets cannot collide. Declared here
    // rather than beside the tracks below because the audio players are minted
    // among the sources, ahead of them.
    let mut next_id = project.channels.len() as u64 + 1;
    // Declared here rather than beside the tracks below because the channels
    // register theirs as they are built — see `ParamTarget::ChannelGain`.
    let mut param_nodes: HashMap<fontelle_types::ParamAddress, NodeId> = HashMap::new();

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
        // And this channel's own level, ahead of the bus — see
        // `Channel::gain_db`. Several channels may share a mixer track, so
        // this cannot be the track's fader.
        sampler.set_gain_db(channel.gain_db);

        // `None` is the master, and so is a route at a track that has been
        // deleted since: a part you can hear and fix beats one that vanished.
        let bus = channel
            .mixer_track
            .and_then(|id| bus_of.get(&id).copied())
            .unwrap_or([0, 1]);
        // Its own two controls are addressable, so a lane can sweep them —
        // and they are the *channel's*, not its bus's, which is the whole
        // reason they live here. See `Channel::gain_db`.
        param_nodes.insert(
            fontelle_types::ParamTarget::ChannelGain(channel_id).address(),
            channel_nodes[&channel_id],
        );
        param_nodes.insert(
            fontelle_types::ParamTarget::ChannelPan(channel_id).address(),
            channel_nodes[&channel_id],
        );
        // And every knob **inside** the instrument, taken from the panel's own
        // list rather than from a second one written here. That is the whole
        // trick: what you can right-click is what the graph can reach, because
        // it is one list. A parameter missing from this map emits no events at
        // all, which is a lane that silently does nothing — the failure this
        // arrangement makes unreachable.
        for address in crate::instrument::patch_addresses(sampler.patch()) {
            param_nodes.insert(
                fontelle_types::ParamTarget::ChannelPatch {
                    channel: channel_id,
                    param: address,
                }
                .address(),
                channel_nodes[&channel_id],
            );
        }
        schedule.push(ScheduledNode {
            id: channel_nodes[&channel_id],
            node: Box::new(SamplerNode::new(sampler, library.store())),
            input_buffers: Vec::new(),
            output_buffers: bus.to_vec(),
        });
    }

    // --- A player per mixer track, for the audio clips routed to it (TDD
    // §15). Among the sources, and for the same reason a sampler is: it only
    // ever adds into a bus, so its order among the other sources does not
    // matter — what matters is that it has run before the fader on the bus it
    // feeds, and every fader comes after this point.
    //
    // One per track whether or not any clip names it. A node with no
    // placements returns immediately, and the alternative — building them only
    // where a clip already sits — means dropping a file on a track it has
    // never been dropped on before is silent until the graph is rebuilt.
    let audio_store = library.audio_store();
    let mut audio_nodes: HashMap<Option<MixerTrackId>, NodeId> = HashMap::new();
    for (&track, bus) in &bus_of {
        let id = mint(&mut next_id);
        audio_nodes.insert(Some(track), id);
        if track == master {
            // `None` is the master — see `AudioClipData::mixer_track`. The
            // same node, not a second one: two players on one bus would be two
            // filter pools and two chances to disagree.
            audio_nodes.insert(None, id);
        }
        schedule.push(ScheduledNode {
            id,
            node: Box::new(fontelle_engine::AudioClipNode::new(std::sync::Arc::clone(
                &audio_store,
            ))),
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
    // Who listens to whom, built once: it orders the schedule and then wires
    // the taps.
    // Who listens to whom. From the document rather than rebuilt here, so the
    // order this schedule runs in and the cycle the document refuses are
    // reasoning about the same edges — see `Mixer::key_listeners`.
    let listeners = project.mixer.key_listeners();
    tracks.sort_by_key(|id| std::cmp::Reverse(depth_to_master(project, *id, master, &listeners)));

    // One tap per track something keys, made before the loop so that both
    // ends — the node that fills it on the source track and the insert that
    // reads it — get the same `Arc`.
    //
    // **Fresh on every rebuild**, unlike the analyser's rings above. A key tap
    // holds one block and is refilled before it is read, so the most a rebuild
    // can cost is a block of silence on the key — under three milliseconds,
    // during which a compressor opens slightly. The analyser's ring holds two
    // thousand samples of history and a window reads it, which is why that one
    // has to survive.
    let key_taps: HashMap<MixerTrackId, std::sync::Arc<fontelle_engine::KeyTap>> = listeners
        .keys()
        .map(|source| {
            (
                *source,
                std::sync::Arc::new(fontelle_engine::KeyTap::new(options.block_size)),
            )
        })
        .collect();

    let mut track_controls: HashMap<MixerTrackId, std::sync::Arc<TrackControls>> = HashMap::new();
    let mut effect_controls: HashMap<(MixerTrackId, usize), fontelle_engine::EffectControls> =
        HashMap::new();
    let mut spectrum_taps: HashMap<
        (MixerTrackId, usize),
        std::sync::Arc<fontelle_engine::SpectrumTap>,
    > = HashMap::new();
    let mut send_controls: HashMap<(MixerTrackId, usize), std::sync::Arc<SendControls>> =
        HashMap::new();
    for id in tracks {
        let track = &project.mixer.tracks[id];
        let bus = bus_of[&id].to_vec();
        schedule_inserts(
            &mut schedule,
            &mut effect_controls,
            &mut spectrum_taps,
            existing_taps,
            &key_taps,
            &mut param_nodes,
            &mut next_id,
            id,
            track,
            &bus,
        );
        // Pre-fader sends: after the inserts, before the fader. "Pre-fader" is
        // a claim about the *fader* and not about the chain — an EQ on a track
        // is part of the track's sound, and a send that bypassed it would be
        // sending a different instrument.
        schedule_sends(
            &mut schedule,
            &mut send_controls,
            &TrackPlan {
                id,
                track,
                bus: &bus,
                bus_of: &bus_of,
                audible: is_audible(id),
            },
            true,
        );
        let (node, controls) = fader(track, is_audible(id), existing.get(&id));
        track_controls.insert(id, controls);
        let fader_id = mint(&mut next_id);
        param_nodes.insert(
            fontelle_types::ParamTarget::TrackGain(id).address(),
            fader_id,
        );
        param_nodes.insert(
            fontelle_types::ParamTarget::TrackPan(id).address(),
            fader_id,
        );
        schedule.push(ScheduledNode {
            id: fader_id,
            node: Box::new(node),
            // Same buffers in and out: a fader processes in place.
            input_buffers: bus.clone(),
            output_buffers: bus.clone(),
        });
        // Post-fader sends: after it, so pulling the part down takes its
        // reverb with it. Before the bus sum only because that is where the
        // bus stops being this track's — the sum leaves its source untouched,
        // so either order would read the same signal.
        schedule_sends(
            &mut schedule,
            &mut send_controls,
            &TrackPlan {
                id,
                track,
                bus: &bus,
                bus_of: &bus_of,
                audible: is_audible(id),
            },
            false,
        );
        // The key tap, alongside the post-fader sends and for the same
        // reason: what a sidechain should hear is what the track sends to the
        // speakers, so pulling the kick down lets the bass back up. It reads
        // the bus in place and writes nothing — a key that altered the track
        // it listened to would be a sidechain you could hear on the wrong
        // channel.
        if let Some(tap) = key_taps.get(&id) {
            schedule.push(ScheduledNode {
                id: NodeId::default(),
                node: Box::new(fontelle_engine::KeyTapNode::new(std::sync::Arc::clone(tap))),
                input_buffers: bus.clone(),
                output_buffers: bus.clone(),
            });
        }
        let output = track.output.unwrap_or(master);
        schedule.push(ScheduledNode {
            id: NodeId::default(),
            node: Box::new(fontelle_engine::BusSumNode),
            input_buffers: bus,
            output_buffers: bus_of.get(&output).copied().unwrap_or([0, 1]).to_vec(),
        });
    }

    // Master's own chain, before its fader, exactly as every other track's.
    schedule_inserts(
        &mut schedule,
        &mut effect_controls,
        &mut spectrum_taps,
        existing_taps,
        &key_taps,
        &mut param_nodes,
        &mut next_id,
        master,
        &project.mixer.tracks[master],
        &[0, 1],
    );
    let (master_fader, master_controls) = fader(
        &project.mixer.tracks[master],
        is_audible(master),
        existing.get(&master),
    );
    track_controls.insert(master, master_controls);
    let master_fader_id = mint(&mut next_id);
    param_nodes.insert(
        fontelle_types::ParamTarget::TrackGain(master).address(),
        master_fader_id,
    );
    param_nodes.insert(
        fontelle_types::ParamTarget::TrackPan(master).address(),
        master_fader_id,
    );
    schedule.push(ScheduledNode {
        id: master_fader_id,
        node: Box::new(master_fader),
        input_buffers: vec![0, 1],
        output_buffers: vec![0, 1],
    });
    // The click, into the master pair — after the master fader so the fader
    // does not move it, and before the limiter so it cannot clip. It is not
    // music: it is not saved, it does not bounce, and it belongs to no channel.
    let metronome = metronome.unwrap_or_else(|| std::sync::Arc::new(Metronome::new()));
    // **And it is told where the beats are, here.** `Metronome::new` starts at
    // zero samples per beat — the "no tempo yet" value, which clicks nothing
    // rather than dividing by it — so a metronome nobody publishes to is a
    // metronome nobody can hear. This is the one place that sees both the
    // project's tempo map and the node being built out of it, so remembering
    // to call `set_beat` stops being anybody's job.
    metronome.set_beat(beat_samples(project), project.beats_per_bar);
    schedule.push(ScheduledNode {
        id: NodeId::default(),
        node: Box::new(MetronomeNode::new(std::sync::Arc::clone(&metronome))),
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
        audio_nodes,
        master: meter,
        track_controls,
        effect_controls,
        param_nodes,
        spectrum_taps,
        send_controls,
        metronome,
        unresolved,
    })
}

/// How long one beat of `project` is, in samples.
///
/// Measured **through the tempo map** rather than divided out of a BPM: that
/// is the rule `Session::seconds_per_tick` follows, and it is what keeps the
/// answer right when the map grows segments. A **constant** beat, which is
/// exact for a song at one tempo and drifts across a tempo change — see
/// `fontelle_engine::Metronome::set_beat` for why the RT side cannot be handed
/// the map itself (INVARIANT 3).
///
/// One function rather than the same subtraction in `realise` and in
/// `Session`, because two copies of it is somewhere for the click the graph
/// plays and the click the tempo box moves to drift apart.
pub fn beat_samples(project: &Project) -> u32 {
    let map = &project.tempo_map;
    let beat = map.tick_to_sample(fontelle_types::PPQN) - map.tick_to_sample(0);
    beat.max(0) as u32
}

fn load_patch(
    data: &PatchData,
    library: &SampleLibrary,
) -> Result<fontelle_core::LoadedPatch, PatchFormatError> {
    Patch::from_data(data, |file: &SampleRef| library.resolve(file))
}

/// One track, and everything scheduling a node on it needs to know.
///
/// A struct rather than five more parameters: they travel together, two of
/// them are only meaningful against each other (a bus is this track's bus),
/// and the pair that would be easiest to swap by accident — `id` and the
/// `audible` flag *about* that id — cannot be, this way.
#[derive(Clone, Copy)]
struct TrackPlan<'a> {
    id: MixerTrackId,
    track: &'a fontelle_model::MixerTrack,
    bus: &'a [usize],
    bus_of: &'a HashMap<MixerTrackId, [usize; 2]>,
    /// Whether a solo elsewhere leaves this track in the mix.
    audible: bool,
}

/// Schedules `track`'s sends whose tap point matches `pre_fader`.
///
/// A send **adds** into its target's bus and leaves its source untouched,
/// which is the whole difference between a send and an output: routing an
/// output moves the signal, a send takes a copy and the dry path carries on.
///
/// A send at a track that is not there is **dropped** rather than sent to the
/// master, which is the opposite of what a channel with a dead route does and
/// deliberately so. A channel with nowhere to go is silent, which is a bug
/// nobody can see; a send that quietly became a second dry path would be a mix
/// that is wrong with no visible reason. `RemoveMixerTrack` takes the sends
/// that fed a deleted track with it, so this is the belt to that braces.
fn schedule_sends(
    schedule: &mut Vec<ScheduledNode>,
    controls: &mut HashMap<(MixerTrackId, usize), std::sync::Arc<SendControls>>,
    plan: &TrackPlan<'_>,
    pre_fader: bool,
) {
    let TrackPlan {
        id,
        track,
        bus,
        bus_of,
        audible,
    } = *plan;
    for (index, send) in track.sends.iter().enumerate() {
        if send.pre_fader != pre_fader || send.target == id {
            continue;
        }
        let Some(target) = bus_of.get(&send.target).copied() else {
            continue;
        };
        // Silenced with the track that feeds it, so a solo elsewhere does not
        // leave a reverb ringing from a part nobody can hear. Which tracks a
        // solo leaves audible is a property of the whole routing graph, which
        // is why the node is told rather than left to work it out.
        let live = std::sync::Arc::new(SendControls::new(
            send.level_db,
            send.pan,
            track.mute || !audible,
        ));
        controls.insert((id, index), std::sync::Arc::clone(&live));
        schedule.push(ScheduledNode {
            id: NodeId::default(),
            node: Box::new(SendNode::new(live, track.pan_law)),
            input_buffers: bus.to_vec(),
            output_buffers: target.to_vec(),
        });
    }
}

/// Schedules one track's insert chain onto its own bus, in order, and hands
/// back the live end of each.
///
/// **Before the fader**, which is where every mixer's inserts are: the fader is
/// the last thing on a strip, so pulling it down turns the effects' output
/// down rather than starving them of input.
///
/// **In place, on the same pair of buffers** — which is what makes a chain a
/// chain. Three inserts are three nodes scheduled in a row on one bus, and the
/// order they were scheduled in is the order the sound goes through them; the
/// scheduler does not have to find a spare buffer per slot.
#[allow(clippy::too_many_arguments)]
fn schedule_inserts(
    schedule: &mut Vec<ScheduledNode>,
    controls: &mut HashMap<(MixerTrackId, usize), fontelle_engine::EffectControls>,
    taps: &mut HashMap<(MixerTrackId, usize), std::sync::Arc<fontelle_engine::SpectrumTap>>,
    existing_taps: &HashMap<(MixerTrackId, usize), std::sync::Arc<fontelle_engine::SpectrumTap>>,
    key_taps: &HashMap<MixerTrackId, std::sync::Arc<fontelle_engine::KeyTap>>,
    param_nodes: &mut HashMap<fontelle_types::ParamAddress, NodeId>,
    next_id: &mut u64,
    id: MixerTrackId,
    track: &fontelle_model::MixerTrack,
    bus: &[usize],
) {
    for (index, slot) in track.inserts.iter().enumerate() {
        let (mut live, source) = fontelle_engine::effect_channel(slot.config);
        live.set_bypassed(slot.bypassed);
        let mut node = fontelle_engine::EffectNode::new(slot.config).with_controls(source);
        node.set_bypassed(slot.bypassed);
        controls.insert((id, index), live);
        // The analyser's ring is **kept across a rebuild**, like the fader's
        // controls above it: an EQ window open while somebody adds a channel
        // must not go blank for a frame, and a fresh ring is a blank graph.
        let tap = existing_taps
            .get(&(id, index))
            .map(std::sync::Arc::clone)
            .unwrap_or_else(|| std::sync::Arc::new(fontelle_engine::SpectrumTap::new()));
        node = node.with_spectrum(std::sync::Arc::clone(&tap));
        taps.insert((id, index), tap);
        // The external key, when the slot names one and the effect has a
        // detector to use it. A key naming a track that has been deleted is
        // **dropped** rather than left dangling, the same rule a send at a
        // missing target follows: a compressor whose key vanished falls back
        // to listening to itself, which is what it was doing before somebody
        // pointed it elsewhere.
        if let Some(key) = slot.effective_key()
            && let Some(tap) = key_taps.get(&key)
        {
            node = node.with_key(std::sync::Arc::clone(tap));
        }

        // A real id, not the default: an automation event has to be addressed
        // to *this* insert, and a node with the default id would receive every
        // other defaulted node's traffic.
        let node_id = mint(next_id);
        for spec in slot.config.specs() {
            param_nodes.insert(
                fontelle_types::ParamTarget::Insert {
                    track: id,
                    slot: index,
                    param: spec.id.to_string(),
                }
                .address(),
                node_id,
            );
        }
        schedule.push(ScheduledNode {
            id: node_id,
            node: Box::new(node),
            input_buffers: bus.to_vec(),
            output_buffers: bus.to_vec(),
        });
    }
}

/// The next free engine node id.
///
/// Counted on from where the channels stopped, so a channel's node and an
/// effect's can never collide — an event addressed to one would otherwise
/// reach the other.
fn mint(next: &mut u64) -> NodeId {
    *next += 1;
    NodeId::from(slotmap::KeyData::from_ffi(*next))
}

/// The engine node for one document mixer track, and the live end of its
/// fader.
///
/// The node's own `gain_db`/`pan`/`mute` fields are seeded from the document as
/// well as the controls, so the two agree from the first block even though only
/// the controls are read while the graph is running.
///
/// The chain around it is [`schedule_inserts`] before and [`schedule_sends`]
/// on either side: a pre-fader send is taken between the inserts and this, and
/// a post-fader one between this and the bus sum.
fn fader(
    track: &MixerTrack,
    audible: bool,
    existing: Option<&std::sync::Arc<TrackControls>>,
) -> (MixerTrackNode, std::sync::Arc<TrackControls>) {
    let mute = track.mute || !audible;
    // Reused where one already exists, so a track's control surface lives as
    // long as the *track* rather than as long as a graph. Two things depend on
    // that: a meter keeps its reading across an instrument change instead of
    // dropping to silence, and nothing holding one of these can end up writing
    // to a graph that has been thrown away — which is a fader that moves the
    // document and is not heard.
    let controls = match existing {
        Some(controls) => {
            controls.set_gain_db(track.gain_db);
            controls.set_pan(track.pan);
            controls.set_mute(mute);
            std::sync::Arc::clone(controls)
        }
        None => std::sync::Arc::new(TrackControls::new(track.gain_db, track.pan, mute)),
    };
    let node = MixerTrackNode {
        // Nothing is automated at build time; the first `ParamValue` says
        // otherwise, and a rebuild mid-song gets one within a block.
        automated_gain_db: None,
        automated_pan: None,
        gain_db: track.gain_db,
        pan: track.pan,
        pan_law: track.pan_law,
        mute,
        phase_invert: track.phase_invert,
        controls: Some(std::sync::Arc::clone(&controls)),
    };
    (node, controls)
}

/// Writes the document's levels, pans and effective mutes into a set of live
/// controls a graph is already playing through.
///
/// The other half of [`fader`], and what a fader drag calls instead of
/// rebuilding the graph. The **effective** mute is what goes in — a track's own
/// switch, or a solo elsewhere silencing it — because audibility under solo is
/// a property of the whole routing graph and not of any one node.
///
/// A track the map does not name is skipped: the graph it belongs to has been
/// replaced since, and the next rebuild will seed a fresh set anyway.
pub fn apply_mixer_controls(
    project: &Project,
    controls: &HashMap<MixerTrackId, std::sync::Arc<TrackControls>>,
) {
    let Some(master) = project.mixer.master else {
        return;
    };
    let audible = soloed_audible(project, master);
    for (id, track) in project.mixer.tracks.iter() {
        let Some(live) = controls.get(&id) else {
            continue;
        };
        live.set_gain_db(track.gain_db);
        live.set_pan(track.pan);
        live.set_mute(track.mute || audible.as_ref().is_some_and(|set| !set.contains(&id)));
    }
}

/// The same, for every send (TDD §13.2).
///
/// Its own function rather than another parameter on
/// [`apply_mixer_controls`], because a caller that has no sends to publish —
/// every offline path — should not have to say so.
///
/// A send is silenced with the track that feeds it, so a mute or a solo
/// flicked while listening takes its reverb with it rather than leaving the
/// bus ringing from a part nobody can hear.
pub fn apply_send_controls(
    project: &Project,
    controls: &HashMap<(MixerTrackId, usize), std::sync::Arc<SendControls>>,
) {
    let Some(master) = project.mixer.master else {
        return;
    };
    let audible = soloed_audible(project, master);
    for (id, track) in project.mixer.tracks.iter() {
        let silent = track.mute || audible.as_ref().is_some_and(|set| !set.contains(&id));
        for (index, send) in track.sends.iter().enumerate() {
            let Some(live) = controls.get(&(id, index)) else {
                continue;
            };
            live.set_level_db(send.level_db);
            live.set_pan(send.pan);
            live.set_mute(silent);
        }
    }
}
