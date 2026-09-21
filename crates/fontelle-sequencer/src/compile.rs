use std::collections::HashMap;

use fontelle_model::{ClipSource, Project, TempoMap};
use fontelle_types::{
    AudioPlacement, ChannelId, ClipId, CompiledTimeline, EventPayload, MixerTrackId, NodeId,
    ParamAddress, TimedEvent,
};

use crate::collision::voice_context_for_clip;

/// Turns the whole document — clips, resolved prefab instances, lane mutes,
/// automation, the tempo map — into a flat, sample-timestamped event list
/// (TDD §11.1). Prefab resolution, override merging, and tempo conversion all
/// happen here, on the model thread, ahead of time: playback cost is therefore
/// independent of how deeply prefabs are nested (INVARIANT 3).
///
/// **Prefab instances are resolved here**, on the model thread, which is
/// INVARIANT 3: the RT thread never sees the prefab graph, and playback cost
/// is therefore independent of how many places a prefab is drawn in. Nothing
/// below reads `clip.source` directly — every read goes through
/// [`Project::clip_source`], because a clip that follows a prefab holds no
/// notes of its own and reading its own source gets an empty arena. That is
/// the failure mode this whole design has: silent instances, and every test
/// written before prefabs existed still passing.
///
/// `channel_nodes` maps each document `ChannelId` to the engine-side `NodeId`
/// its compiled `SamplerNode` was assigned when the audio graph was built.
/// This crate depends on `fontelle-types` + `fontelle-model` only (TDD §4.1)
/// — it has no way to discover that mapping itself, so whoever builds the
/// graph (currently `fontelle-app`; a real "build graph from project" step
/// belongs to the engine eventually) passes it in. A channel missing from the
/// map produces no events for its notes rather than panicking — the document
/// can reference a channel before the graph has caught up with it.
pub fn compile(
    project: &Project,
    channel_nodes: &HashMap<ChannelId, NodeId>,
    param_nodes: &HashMap<ParamAddress, NodeId>,
) -> CompiledTimeline {
    compile_scoped(project, channel_nodes, param_nodes, CompileScope::Song)
}

/// Which engine node plays what.
///
/// This crate depends on `fontelle-types` and `fontelle-model` only (TDD §4.1),
/// so it has no way to discover any of these itself: whoever builds the graph
/// passes them in. Grouped into one type rather than added to the argument list
/// one at a time, because there are three of them now and the list was already
/// two positional maps that are easy to swap by mistake.
///
/// A target missing from a map compiles to **nothing for that target**, never a
/// panic: the document can name a channel, a parameter or a mixer track before
/// the graph has caught up with it, which is what happens on the frame one is
/// created.
#[derive(Debug, Clone, Copy)]
pub struct NodeMaps<'a> {
    /// Each channel's `SamplerNode`.
    pub channels: &'a HashMap<ChannelId, NodeId>,
    /// Each automatable parameter's owning node.
    pub params: &'a HashMap<ParamAddress, NodeId>,
    /// The `AudioClipNode` in front of each mixer track. **`None` is the
    /// master**, matching `AudioClipData::mixer_track`.
    pub audio: &'a HashMap<Option<MixerTrackId>, NodeId>,
}

impl Default for NodeMaps<'_> {
    fn default() -> Self {
        // Leaked-free empty maps with the program's own lifetime. A `static`
        // rather than an owned field, so `NodeMaps` stays `Copy` and a caller
        // can write `NodeMaps { audio: &mine, ..Default::default() }`.
        static NO_CHANNELS: std::sync::OnceLock<HashMap<ChannelId, NodeId>> =
            std::sync::OnceLock::new();
        static NO_PARAMS: std::sync::OnceLock<HashMap<ParamAddress, NodeId>> =
            std::sync::OnceLock::new();
        static NO_AUDIO: std::sync::OnceLock<HashMap<Option<MixerTrackId>, NodeId>> =
            std::sync::OnceLock::new();
        Self {
            channels: NO_CHANNELS.get_or_init(HashMap::new),
            params: NO_PARAMS.get_or_init(HashMap::new),
            audio: NO_AUDIO.get_or_init(HashMap::new),
        }
    }
}

/// How much of the document a compile reads.
///
/// **Clip mode** is FL Studio's pattern mode: the transport plays only the
/// clip you are editing, round and round, so a part can be heard on its own
/// while it is written. Fontelle's clips are its patterns, so the equivalent
/// is a compile that reads one clip — notes or automation — and leaves every
/// other clip out, other lanes' automation included: a tempo lane elsewhere
/// in the song is not part of the part being soloed. The loop the transport
/// runs over the clip's bars is the session's business; this decides what is
/// on the timeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompileScope {
    /// Everything: the compile there has always been.
    Song,
    /// One clip, where it sits in the song. Not moved to the front: the loop
    /// the session sets is over the clip's own bars, and the roll draws its
    /// playhead against the clip's own start.
    Clip(ClipId),
    /// One **row** of the arrangement, where it sits in the song.
    ///
    /// What rendering a track to audio compiles (*"the ability to render a
    /// track into an audio clip"*): everything on that row and nothing else,
    /// so the bounce is the row rather than the row plus whatever happened to
    /// be playing beside it.
    ///
    /// The automation on *other* rows is left out with it, exactly as a clip
    /// scope leaves it out — a row is the unit being soloed, and a curve on
    /// another row is not part of it.
    Lane(fontelle_types::LaneId),
}

impl CompileScope {
    /// Whether `clip` is in this scope. `lane` is the row it sits on, which
    /// only [`Lane`](Self::Lane) reads.
    fn includes(self, id: ClipId, lane: fontelle_types::LaneId) -> bool {
        match self {
            Self::Song => true,
            Self::Clip(only) => only == id,
            Self::Lane(only) => only == lane,
        }
    }
}

/// [`compile`], over `scope` — see [`CompileScope`].
pub fn compile_scoped(
    project: &Project,
    channel_nodes: &HashMap<ChannelId, NodeId>,
    param_nodes: &HashMap<ParamAddress, NodeId>,
    scope: CompileScope,
) -> CompiledTimeline {
    compile_with(
        project,
        &NodeMaps {
            channels: channel_nodes,
            params: param_nodes,
            ..Default::default()
        },
        scope,
    )
}

/// [`compile_scoped`], knowing every node the document might reach — audio
/// clips included.
pub fn compile_with(
    project: &Project,
    nodes: &NodeMaps<'_>,
    scope: CompileScope,
) -> CompiledTimeline {
    let channel_nodes = nodes.channels;
    let param_nodes = nodes.params;
    let mut events = Vec::new();
    let mut audio = Vec::new();
    // Which row each placement is on, beside it: the crossfade below is a
    // question about two clips on one row, and a placement does not carry
    // its lane (the audio thread has no use for one).
    let mut audio_lanes: Vec<fontelle_types::LaneId> = Vec::new();

    // Every tick becomes a sample through the *automated* tempo map (TDD
    // §12.3), built once for the pass. In clip scope the tempo lane is out
    // of scope like every other clip, so the box's own map is the one used.
    let tempo = match scope {
        CompileScope::Song => fontelle_model::effective_tempo_map(project),
        // In a narrowed scope the tempo lane is out of it like every other
        // clip, so the box's own map is the one used.
        CompileScope::Clip(_) | CompileScope::Lane(_) => project.tempo_map.clone(),
    };

    // The channel rack's two switches, resolved once for the whole pass. They
    // are *sequencer* mutes — the same reading a lane's has — and they have to
    // be: a channel plays through the master by default now, so a switch that
    // reached for its mixer track would silence the song. See
    // `Channel::muted`.
    //
    // `None` means nothing is soloed and every channel plays, which is not the
    // same as "the set of all channels": that distinction is what stops a solo
    // being sticky after the soloed channel is deleted.
    let soloed: Option<Vec<ChannelId>> = {
        let soloed: Vec<ChannelId> = project
            .channels
            .iter()
            .filter(|(_, channel)| channel.soloed)
            .map(|(id, _)| id)
            .collect();
        (!soloed.is_empty()).then_some(soloed)
    };
    let audible = |id: ChannelId| {
        project.channels.get(id).is_some_and(|channel| {
            !channel.muted && soloed.as_ref().is_none_or(|set| set.contains(&id))
        })
    };

    for (clip_index, (clip_id, clip)) in project.clips.iter().enumerate() {
        if clip.muted || !scope.includes(clip_id, clip.lane) {
            continue;
        }
        if project.lanes.get(clip.lane).is_some_and(|lane| lane.muted) {
            continue;
        }

        // **What this clip actually holds** — its own content, or the content
        // of the prefab it follows. `clip.source` is empty for an instance;
        // see this function's docs and `Project::clip_source`. The clip's
        // *placement* — its row, start, length and loop — is still its own,
        // which is the whole distinction: a prefab is the notes, and a place
        // for it is the window on them.
        //
        // Borrowed for a mirror instance, so the common case allocates
        // nothing; owned only when there is something to resolve.
        let Some(source) = project.clip_source(clip_id) else {
            continue;
        };
        let source = source.as_ref();

        // An audio clip is not events (TDD §15): it is a range on the song
        // that a block either falls inside or does not, so it compiles to a
        // placement and there is nothing to schedule.
        if let ClipSource::Audio(data) = source {
            let Some(&target) = nodes.audio.get(&data.mixer_track) else {
                continue; // no player for that track yet
            };
            audio_lanes.push(clip.lane);
            audio.push(AudioPlacement {
                target,
                clip: clip_id,
                range: tempo.tick_to_sample(clip.start)
                    ..tempo.tick_to_sample(clip.start + clip.length),
                // Filled in below, once every placement on the row is known.
                crossfade_in: 0,
                crossfade_out: 0,
                // **Not** unrolled into one placement per pass, which is what
                // a looped note clip does. A note is a moment and has to be
                // emitted again on every pass; a stream is one range that
                // comes round — and unrolling would give a sixteen-bar
                // one-bar loop sixteen filter states instead of one.
                repeat: match clip.loop_length.filter(|p| *p > 0) {
                    Some(period) => {
                        tempo.tick_to_sample(clip.start + period) - tempo.tick_to_sample(clip.start)
                    }
                    None => 0,
                },
                data: data.clone(),
            });
            continue;
        }

        let ClipSource::Notes(note_data) = source else {
            continue; // automation clips are compiled below
        };

        let voice_context = voice_context_for_clip(clip_index as u32);

        // A **looped** clip is one set of notes played again every
        // `loop_length` until the clip runs out — as against a copy, which is
        // several clips with notes of their own. `repeats()` is 1 for a clip
        // that does not loop, so the plain case walks this loop once and comes
        // out exactly where it used to.
        let period = clip.loop_length.filter(|p| *p > 0);
        for repeat in 0..clip.repeats() {
            let offset = clip.repeat_start(repeat);

            for note in note_data.notes.values() {
                // Only the notes inside one period belong to a repeat. A note
                // written past the period is content the loop does not
                // contain — it is what the clip would play if it were not
                // looping, and playing it on every pass would be a
                // second, invisible loop.
                if period.is_some_and(|p| note.start >= p) {
                    continue;
                }
                // **Per note**, not per clip: a clip may hold several
                // instruments (`Note::channel`), so which channel is muted,
                // soloed or wired is asked of the channel this note plays.
                // A note naming a channel with no node yet is left out
                // rather than played on the clip's — a bass line on the
                // drums for one frame is worse than a bass line late.
                let channel = note.channel_or(note_data.channel);
                if !audible(channel) {
                    continue; // muted in the rack, or another channel is soloed
                }
                let Some(&node_id) = channel_nodes.get(&channel) else {
                    continue; // channel not wired into the compiled graph yet
                };
                let start = note.start + offset;
                // **The clip's end is the end**, whether or not it loops.
                //
                // > *"clip endings don't actually cut the clip short audibly
                // > right now it keeps playing"*
                //
                // A clip's length is the window on its content, which is
                // what an audio clip's placement has always meant
                // (`clip.start .. clip.start + clip.length`, above) and what
                // dragging a right edge in is *for*. Until this, only a
                // looped clip clamped — a loop whose last pass is longer than
                // the others is obviously wrong — while a plain clip drawn
                // shorter than its notes went on playing all of them, so
                // trimming one changed the picture and nothing else.
                //
                // A note that begins at the end is outside it: the range is
                // half-open, so a clip ending at bar two and one beginning
                // there do not both sound the same tick.
                if start >= clip.length {
                    continue;
                }
                let on_tick = clip.start + start;
                // Cut, not silenced: the note-off lands on the boundary and
                // the instrument's own release rings out from there. Stopping
                // the sound dead there would be a click.
                //
                // **Two boundaries, and the nearer one wins.** The clip's own
                // end, and — inside a loop — the end of *this pass*:
                //
                // > *"it should just cut off wherever you put the ending to
                // > be and then cleanly loop from that point"*
                //
                // A note written longer than the period used to ring on
                // through the passes after it, so the second pass played over
                // the first one's tail and the third over both. A loop that
                // gets thicker as it goes is not a loop; every pass sounds
                // like the one before it now.
                let pass_end = period.map_or(i64::MAX, |period| clip.start + offset + period);
                let off_tick = (on_tick + note.length)
                    .min(pass_end)
                    .min(clip.start + clip.length);

                // A **slide note** starts no voice and ends none: it bends
                // whatever is already sounding on this channel to its pitch,
                // over its own length. See `fontelle_model::Note::slide`, and
                // `fontelle_core::Sampler::slide` for what happens when
                // nothing is sounding (nothing).
                if note.slide {
                    let on = tempo.tick_to_sample(on_tick);
                    let off = tempo.tick_to_sample(off_tick);
                    events.push(TimedEvent {
                        sample: on,
                        target: node_id,
                        payload: EventPayload::NoteSlide {
                            key: note.key,
                            glide_samples: (off - on).max(0) as u32,
                            voice_context,
                        },
                    });
                    continue;
                }

                events.push(TimedEvent {
                    sample: tempo.tick_to_sample(on_tick),
                    target: node_id,
                    payload: EventPayload::NoteOn {
                        key: note.key,
                        velocity: note.velocity,
                        // All five of §16.5's per-note properties. They ride
                        // on the note-on rather than being parameters of the
                        // channel's node because that is what *per note*
                        // means: two notes sounding together on one
                        // instrument can differ in every one of them.
                        pan: note.pan,
                        fine_pitch: note.fine_pitch,
                        release: note.release,
                        mod_x: note.mod_x,
                        mod_y: note.mod_y,
                        voice_context,
                    },
                });
                events.push(TimedEvent {
                    sample: tempo.tick_to_sample(off_tick),
                    target: node_id,
                    payload: EventPayload::NoteOff {
                        key: note.key,
                        voice_context,
                    },
                });
            }
        }
    }

    compile_automation(project, param_nodes, &tempo, scope, &mut events);

    // Sorted once, at the end, over notes and automation together: the RT side
    // walks this list forwards and never sorts, so a sweep interleaved out of
    // order would be applied backwards.
    sort_events(&mut events);

    // The automatic crossfade (TDD §15.2): where two audio clips on one row
    // overlap, the earlier fades out over the overlap and the later fades in
    // over it. *"it should also blend together like a transition the timing
    // based on how long the overlap section is."* Measured here because
    // this pass can see both clips at once and already owns every
    // tick-to-sample conversion. Muted clips and muted rows never got a
    // placement, so nothing fades against them — the clip beside a muted
    // one plays plain, as it would with the muted one deleted.
    //
    // A clip that ends *inside* the other — dropped wholly within a longer
    // one — has no tail in the overlap to fade, and none is invented: only
    // a clip whose end the overlap reaches fades out, and only a clip whose
    // start it reaches fades in.
    for i in 0..audio.len() {
        for j in (i + 1)..audio.len() {
            if audio_lanes[i] != audio_lanes[j] {
                continue;
            }
            let (earlier, later) = if audio[i].range.start <= audio[j].range.start {
                (i, j)
            } else {
                (j, i)
            };
            let from = audio[earlier].range.start.max(audio[later].range.start);
            let to = audio[earlier].range.end.min(audio[later].range.end);
            let overlap = to - from;
            if overlap <= 0 {
                continue;
            }
            if audio[later].range.start >= from {
                audio[later].crossfade_in = audio[later].crossfade_in.max(overlap);
            }
            if audio[earlier].range.end <= to {
                audio[earlier].crossfade_out = audio[earlier].crossfade_out.max(overlap);
            }
        }
    }

    CompiledTimeline {
        events,
        audio,
        // Bar-granularity seek index needs the tempo map's time-signature
        // track, which doesn't exist yet (see TempoMap's scope cut). Not
        // required for correctness — `events_for_block` walks the sorted
        // `events` Vec directly — only for the O(1)-seek optimisation TDD
        // §11.1 describes. M3 work.
        index: Vec::new(),
        // The tempo, in the block contract's own units, because the audio
        // thread cannot see a `TempoMap` (INVARIANT 4) and a node that wants
        // to know how long a beat is has nowhere else to ask. Converting it
        // here rather than shipping ticks is the point: this pass already owns
        // every tick-to-sample conversion in the project.
        tempo: tempo
            .segments()
            .iter()
            .map(|segment| (tempo.tick_to_sample(segment.start_tick), segment.bpm as f32))
            .collect(),
    }
}

/// Where an event sits among the others sharing its sample.
///
/// Time is the first key and this is the second, because *"at the same moment"*
/// is not an order and the RT side applies the list in the order it is given.
/// Three rules, and each one is a bug that was reachable without it:
///
/// 1. **A value before the note it belongs to.** An automation point written at
///    the same tick as a note-on is the value that note is meant to sound at.
/// 2. **A note-off before a note-on.** Two notes on one key, the first ending
///    exactly where the second begins, is the commonest figure in music there
///    is — a walking bass, a repeated pedal tone. Emitted the other way round,
///    the off finds the voice the on has just taken (the pool hands out the
///    lowest free slot, and the first note's voice can already be free) and
///    releases it: the note is there, and silent. That it depended on arena
///    order is what made it *"a chance"* rather than a reproducible failure.
/// 3. **A slide between them.** It bends what is already sounding, so it has to
///    arrive after the offs of the notes that ended and before the ons of the
///    notes that have not started.
fn rank(payload: &EventPayload) -> u8 {
    match payload {
        // A performance event is never compiled from a clip — it comes off a
        // device, live — but the order has to answer for every payload, and
        // a wheel that arrived at the same sample as a note belongs before
        // it, where a parameter does.
        EventPayload::ParamValue { .. }
        | EventPayload::Controller { .. }
        | EventPayload::PitchBend { .. }
        | EventPayload::ChannelPressure { .. } => 0,
        EventPayload::ClipStop => 1,
        EventPayload::NoteOff { .. } => 2,
        // A note's own pressure or bend moves what is sounding, like a
        // slide, and orders with one.
        EventPayload::NoteSlide { .. } | EventPayload::NoteMod { .. } => 3,
        EventPayload::ClipStart => 4,
        EventPayload::NoteOn { .. } => 5,
    }
}

/// Puts a compiled event list in the order the RT side walks it: by sample,
/// then by [`rank`].
///
/// Public so that whoever splices freshly compiled events into an existing list
/// — `recompile_dirty`, when it is written — orders them by the same rule this
/// pass does. A splice with a rule of its own would reintroduce exactly the
/// failure this exists to stop. **Stable**, so two events with the same sample
/// and the same rank keep the order the document gave them.
pub fn sort_events(events: &mut [fontelle_types::TimedEvent]) {
    events.sort_by_key(|e| (e.sample, rank(&e.payload)));
}

/// How often an automated parameter is re-stated, in samples.
///
/// The control rate, and it is a compromise with a reason on each side. Per
/// sample is a hundred thousand events a second per lane for a smoothness
/// nobody can hear. Per block is what the engine would like and what the
/// sequencer cannot do — it has no block size, and the same timeline is played
/// at whatever size the device asks for.
///
/// So: a fixed interval shorter than any block the engine runs (§5.1's
/// `BLOCK_SIZE` is 512), which means a node applying the last value it saw
/// gets one per block whatever the device is doing. Five milliseconds at
/// 48 kHz, which is finer than a hand moves.
pub const AUTOMATION_INTERVAL: fontelle_types::Sample = 256;

/// How much a value has to move before it is worth another event.
///
/// A curve that is not going anywhere costs one event rather than one per
/// interval — which is the difference between a five-minute song with four
/// flat lanes carrying four events and one carrying a hundred thousand.
const AUTOMATION_EPSILON: f64 = 1e-4;

/// Turns every automation clip into `ParamValue` events at the node that owns
/// its target (TDD §12).
///
/// **The address is resolved here, off the audio thread.** `param_nodes` comes
/// from the realisation step, which is the only place that knows both the
/// document's parameter addresses and the graph's node ids — the same reason
/// `channel_nodes` is a parameter of this function (§4.1). Matching a string
/// per event per block on the RT side is work it should never be doing.
///
/// A target no node owns emits nothing: a project whose automation names a
/// track that has since been deleted plays, rather than panicking or filling
/// the timeline with events nobody reads.
///
/// **The tempo is not here.** It has no node to send a value to: a tempo lane
/// is realised as the map every other tick in the pass is converted through
/// (`fontelle_model::effective_tempo_map`), and a `ParamValue` for it would
/// be a value nothing reads. `param_nodes` never carries it, so the `continue`
/// below is what leaves it out — by construction rather than by a special
/// case.
fn compile_automation(
    project: &Project,
    param_nodes: &HashMap<fontelle_types::ParamAddress, NodeId>,
    tempo: &TempoMap,
    scope: CompileScope,
    events: &mut Vec<TimedEvent>,
) {
    for target in fontelle_model::automated_targets(project) {
        let Some(node) = param_nodes.get(&target).copied() else {
            continue;
        };
        // The span this target is automated over: from the first clip aimed at
        // it to the last one's end. Outside it there is nothing to say — before,
        // the parameter is the knob's; after, it holds what it was last told.
        let Some((from, to)) = automated_span(project, &target, scope) else {
            continue;
        };

        let mut last: Option<f64> = None;
        let mut tick = from;
        while tick <= to {
            if let Some(value) = value_in_scope(project, &target, tick, scope)
                && last.is_none_or(|previous| (value - previous).abs() > AUTOMATION_EPSILON)
            {
                events.push(TimedEvent {
                    sample: tempo.tick_to_sample(tick),
                    target: node,
                    payload: EventPayload::ParamValue {
                        target: target.clone(),
                        value,
                    },
                });
                last = Some(value);
            }
            // Stepped in *samples* rather than ticks, so the rate is the same
            // through a tempo change — a sweep must not get coarser because
            // the song slowed down.
            let next = tempo.tick_to_sample(tick) + AUTOMATION_INTERVAL;
            let stepped = tempo.sample_to_tick(next).max(tick + 1);
            if stepped > to && tick < to {
                tick = to; // one last event exactly at the end
            } else {
                tick = stepped;
            }
        }
    }
}

/// What `target` is automated to at `tick`, reading only the clips in scope.
///
/// In song scope this is `fontelle_model::automation_at`, rules and all. In
/// clip scope only the one clip can speak, and it holds its last value past
/// its end the way any lane does (§12.2's second rule).
fn value_in_scope(
    project: &Project,
    target: &fontelle_types::ParamAddress,
    tick: fontelle_types::Tick,
    scope: CompileScope,
) -> Option<f64> {
    match scope {
        CompileScope::Song => fontelle_model::automation_at(project, target, tick),
        // A row's own curves, read the way the song's are but from that row
        // alone — the last clip on it that aims at `target`.
        CompileScope::Lane(lane) => {
            let mut best: Option<f64> = None;
            for (id, clip) in project.clips.iter() {
                if clip.muted || clip.lane != lane {
                    continue;
                }
                // Through `clip_source` like every other read, so an
                // automation prefab drawn in four places sweeps in all four.
                let Some(source) = project.clip_source(id) else {
                    continue;
                };
                let ClipSource::Automation(data) = source.as_ref() else {
                    continue;
                };
                if &data.target != target {
                    continue;
                }
                if tick < clip.start {
                    continue;
                }
                best = data.value_at(tick - clip.start).or(best);
            }
            best
        }
        CompileScope::Clip(id) => {
            let clip = project.clips.get(id)?;
            let source = project.clip_source(id)?;
            let ClipSource::Automation(data) = source.as_ref() else {
                return None;
            };
            if clip.muted || data.target != *target || clip.start > tick {
                return None;
            }
            if tick < clip.start + clip.length {
                data.value_at(tick - clip.start)
            } else {
                data.final_value()
            }
        }
    }
}

/// The first tick any clip in scope aims at `target`, and the last tick any
/// of them ends at.
fn automated_span(
    project: &Project,
    target: &fontelle_types::ParamAddress,
    scope: CompileScope,
) -> Option<(fontelle_types::Tick, fontelle_types::Tick)> {
    let mut span: Option<(fontelle_types::Tick, fontelle_types::Tick)> = None;
    for (id, clip) in project.clips.iter() {
        if clip.muted || !scope.includes(id, clip.lane) {
            continue;
        }
        let Some(source) = project.clip_source(id) else {
            continue;
        };
        let fontelle_model::ClipSource::Automation(data) = source.as_ref() else {
            continue;
        };
        if data.target != *target {
            continue;
        }
        let (start, end) = (clip.start, clip.start + clip.length);
        span = Some(match span {
            Some((from, to)) => (from.min(start), to.max(end)),
            None => (start, end),
        });
    }
    span
}

#[cfg(test)]
mod tests {
    use fontelle_model::Arena;
    use fontelle_model::{Clip, ClipSource, Lane, Note, NoteData, Project, TempoMap};
    use fontelle_types::EventPayload;
    use slotmap::SlotMap;

    use super::*;

    const SR: f64 = 48_000.0;
    const BPM: f64 = 120.0;

    fn project_with_one_note() -> (Project, ChannelId, NodeId) {
        let mut project = Project::new("test");
        project.tempo_map = TempoMap::new(BPM, SR);

        let channel_id = project.channels.insert(fontelle_model::Channel {
            preset: None,
            instrument: None,
            name: "ch".into(),
            color: [0, 0, 0, 255],
            mixer_track: None,
            patch_data: None,
            plugin: None,
            pan: 0.0,
            muted: false,
            soloed: false,
            named_keys: false,
            gain_db: 0.0,
        });

        let lane_id = project.lanes.insert(Lane {
            name: "lane".into(),
            height: 32.0,
            color: [0, 0, 0, 255],
            muted: false,
            locked: false,
            order: 0,
        });

        let mut notes = Arena::default();
        notes.insert(Note {
            start: 0,
            length: fontelle_types::PPQN, // one quarter note
            key: 60,
            velocity: 100,
            pan: 0,
            fine_pitch: 0,
            release: 0,
            mod_x: 0,
            mod_y: 0,
            slide: false,
            channel: None,
        });

        project.clips.insert(Clip {
            lane: lane_id,
            start: 0,
            length: fontelle_types::PPQN,
            source: ClipSource::Notes(NoteData {
                channel: channel_id,
                notes,
            }),
            prefab_link: None,
            color: None,
            muted: false,
            loop_length: None,
        });

        let mut node_ids: SlotMap<NodeId, ChannelId> = SlotMap::default();
        let node_id = node_ids.insert(channel_id);

        (project, channel_id, node_id)
    }

    #[test]
    fn one_note_compiles_to_a_note_on_and_note_off_targeting_its_channels_node() {
        let (project, channel_id, node_id) = project_with_one_note();
        let mut channel_nodes = HashMap::new();
        channel_nodes.insert(channel_id, node_id);

        let timeline = compile(&project, &channel_nodes, &Default::default());

        assert_eq!(timeline.events.len(), 2, "one NoteOn and one NoteOff");

        let note_on = &timeline.events[0];
        assert_eq!(note_on.sample, 0);
        assert_eq!(note_on.target, node_id);
        match note_on.payload {
            EventPayload::NoteOn { key, velocity, .. } => {
                assert_eq!(key, 60);
                assert_eq!(velocity, 100);
            }
            ref other => panic!("expected NoteOn, got {other:?}"),
        }

        let note_off = &timeline.events[1];
        assert_eq!(
            note_off.sample, 24_000,
            "a quarter note at 120bpm/48kHz is 24000 samples long"
        );
        assert_eq!(note_off.target, node_id);
        assert!(matches!(
            note_off.payload,
            EventPayload::NoteOff { key: 60, .. }
        ));
    }

    #[test]
    fn events_are_sorted_by_sample() {
        let (project, channel_id, node_id) = project_with_one_note();
        let mut channel_nodes = HashMap::new();
        channel_nodes.insert(channel_id, node_id);

        let timeline = compile(&project, &channel_nodes, &Default::default());

        let mut sorted = timeline.events.clone();
        sorted.sort_by_key(|e| e.sample);
        assert_eq!(
            timeline.events.iter().map(|e| e.sample).collect::<Vec<_>>(),
            sorted.iter().map(|e| e.sample).collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_muted_clip_produces_no_events() {
        let (mut project, channel_id, node_id) = project_with_one_note();
        for clip in project.clips.values_mut() {
            clip.muted = true;
        }
        let mut channel_nodes = HashMap::new();
        channel_nodes.insert(channel_id, node_id);

        let timeline = compile(&project, &channel_nodes, &Default::default());
        assert!(timeline.events.is_empty());
    }

    #[test]
    fn a_muted_lane_produces_no_events() {
        let (mut project, channel_id, node_id) = project_with_one_note();
        for lane in project.lanes.values_mut() {
            lane.muted = true;
        }
        let mut channel_nodes = HashMap::new();
        channel_nodes.insert(channel_id, node_id);

        let timeline = compile(&project, &channel_nodes, &Default::default());
        assert!(timeline.events.is_empty());
    }

    #[test]
    fn a_channel_missing_from_the_node_map_produces_no_events() {
        let (project, _channel_id, _node_id) = project_with_one_note();
        let channel_nodes = HashMap::new(); // deliberately empty

        let timeline = compile(&project, &channel_nodes, &Default::default());
        assert!(timeline.events.is_empty());
    }

    /// §16.5's per-note pan, which the roll's property lane has been able to
    /// draw and edit since the lane existed — and which stopped dead here.
    /// The document stored it, the file round-tripped it, and this function
    /// built a `NoteOn` without it, so panning a note was a picture of a
    /// change rather than a change.
    #[test]
    fn a_notes_pan_reaches_the_compiled_note_on() {
        let (mut project, channel_id, node_id) = project_with_one_note();
        for clip in project.clips.values_mut() {
            if let ClipSource::Notes(data) = &mut clip.source {
                for note in data.notes.values_mut() {
                    note.pan = -100;
                }
            }
        }
        let mut channel_nodes = HashMap::new();
        channel_nodes.insert(channel_id, node_id);

        let timeline = compile(&project, &channel_nodes, &Default::default());

        match timeline.events[0].payload {
            EventPayload::NoteOn { pan, .. } => assert_eq!(
                pan, -100,
                "the note's own pan has to be on the wire — nothing downstream \
                 can see the document to go and fetch it"
            ),
            ref other => panic!("expected NoteOn, got {other:?}"),
        }
    }
}
