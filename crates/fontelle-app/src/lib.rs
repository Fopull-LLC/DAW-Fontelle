//! The DAW binary's non-UI guts, in a library so they're testable — a `[[bin]]`
//! can't be imported by an integration test, and "the thing we demo" deserves
//! coverage as much as anything else does.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use fontelle_core::{SampleStore, Sampler};
use fontelle_engine::{
    BLOCK_SIZE, BufferPool, CompiledGraph, MixerTrackNode, SamplerNode, ScheduledNode,
};
use fontelle_model::{Channel, Clip, ClipSource, Lane, Note, NoteData, Project, TempoMap};
use fontelle_types::{ChannelId, CompiledTimeline, NodeId, PPQN, Tick};
use slotmap::SlotMap;

/// Why `--play-sf2` couldn't produce a usable path.
#[derive(Debug, PartialEq, Eq)]
pub enum Sf2PathError {
    /// No `--play-sf2` flag at all, or nothing after it.
    NotRequested,
    /// The path doesn't exist. `rejoined` is `Some` when the surrounding
    /// arguments look like a single path containing spaces that the shell
    /// split apart — by far the most common way this goes wrong, since
    /// soundfont libraries live in directories like `FL 2026 Linux/`.
    NotFound {
        tried: PathBuf,
        rejoined: Option<PathBuf>,
    },
}

impl std::fmt::Display for Sf2PathError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotRequested => f.write_str("--play-sf2 requires a file path argument"),
            Self::NotFound {
                tried,
                rejoined: Some(actual),
            } => write!(
                f,
                "no such file: {}\n\n\
                 That path contains spaces, so your shell split it into separate arguments \
                 before Fontelle saw it. Quote it:\n    --play-sf2 \"{}\"",
                tried.display(),
                actual.display()
            ),
            Self::NotFound {
                tried,
                rejoined: None,
            } => write!(f, "no such file: {}", tried.display()),
        }
    }
}

impl std::error::Error for Sf2PathError {}

/// Pulls the `--play-sf2` path out of `args`, diagnosing the unquoted-path
/// case rather than reporting a confusing "no such file" for a fragment the
/// user never typed.
///
/// `exists` is injected so this is testable without touching the filesystem;
/// `main` passes `Path::exists`.
pub fn resolve_sf2_path(
    args: &[String],
    exists: impl Fn(&Path) -> bool,
) -> Result<PathBuf, Sf2PathError> {
    let flag = args
        .iter()
        .position(|a| a == "--play-sf2")
        .ok_or(Sf2PathError::NotRequested)?;
    let first = args.get(flag + 1).ok_or(Sf2PathError::NotRequested)?;

    let tried = PathBuf::from(first);
    if exists(&tried) {
        return Ok(tried);
    }

    // The value didn't resolve. If the arguments that follow it aren't flags,
    // they're very likely the rest of one space-containing path the shell
    // split up — rejoin and see whether *that* is the file the user meant.
    let fragments: Vec<&str> = args[flag + 1..]
        .iter()
        .take_while(|a| !a.starts_with("--"))
        .map(String::as_str)
        .collect();
    let rejoined = (fragments.len() > 1).then(|| PathBuf::from(fragments.join(" ")));

    Err(Sf2PathError::NotFound {
        tried,
        rejoined: rejoined.filter(|p| exists(p)),
    })
}

/// A demo document plus the identities needed to drive it: what
/// `fontelle-sequencer` compiles, and which engine node its events target.
/// Mints a distinct engine node id for the nth instrument in a song.
///
/// A real graph compiler would allocate these from the document; until one
/// exists, minting them deterministically here keeps them distinct, which is
/// all the event routing needs. Index 0 is deliberately skipped, because a
/// slotmap's zero key is its null key and a null target would match any node
/// that had not been given an id of its own.
fn node_id(index: usize) -> NodeId {
    NodeId::from(slotmap::KeyData::from_ffi(index as u64 + 1))
}

/// One part of a song: a document channel, the engine node that renders it,
/// and the mixer track that carries it to the master.
///
/// **Not the document's own mixer.** `Project::mixer` exists and is where this
/// belongs, but nothing compiles a graph from a `Project` yet — `fontelle-app`
/// hand-assigns node ids and buses, and this carries what that step needs. See
/// PROGRESS.md; the fix is the same "build the graph from the project" step
/// that would own `channel_nodes` too.
#[derive(Debug, Clone, Copy)]
pub struct SongChannel {
    pub channel: ChannelId,
    pub node: NodeId,
    /// The part's fader, in decibels. From the file's own CC7 for an imported
    /// song; unity for the built-in phrase.
    pub gain_db: f32,
}

pub struct Song {
    pub project: Project,
    /// Each part of the song, in a stable order. One entry for the built-in
    /// phrase; one per part for an imported file.
    pub channels: Vec<SongChannel>,
}

impl Song {
    /// Wraps an imported MIDI file so it plays through the same document ->
    /// sequencer -> timeline path the built-in phrase does. A second playback
    /// route for files would be a second route none of this project's
    /// invariants cover.
    ///
    /// Each of the file's channels becomes a node of its own, so the caller
    /// supplies one instrument per entry in `channels` and the parts play on
    /// different sounds.
    pub fn from_midi(import: fontelle_assets::MidiImport, sample_rate: u32) -> Self {
        let mut project = import.project;
        // `set_sample_rate`, not a fresh `TempoMap`: the import carries the
        // file's whole tempo curve, and building a constant map from
        // `import.bpm` would throw every tempo change away again.
        project.tempo_map.set_sample_rate(sample_rate as f64);
        Self {
            channels: import
                .channels
                .iter()
                .enumerate()
                .map(|(index, imported)| SongChannel {
                    channel: imported.channel,
                    node: node_id(index),
                    // The file's own balance between its parts. Discarding it
                    // and playing every part at its instrument's level is how
                    // an arrangement ends up with the drums on top of the
                    // melody.
                    gain_db: imported.volume_db,
                })
                .collect(),
            project,
        }
    }

    /// The mapping `fontelle_sequencer::compile` needs to turn document
    /// channels into engine node targets.
    pub fn channel_nodes(&self) -> HashMap<ChannelId, NodeId> {
        self.channels.iter().map(|c| (c.channel, c.node)).collect()
    }

    pub fn compile(&self) -> CompiledTimeline {
        fontelle_sequencer::compile(&self.project, &self.channel_nodes())
    }

    /// How long the song runs, in samples, including a tail so the last
    /// note's release isn't cut off mid-ring.
    pub fn duration_samples(&self, release_tail: Tick) -> i64 {
        let last_tick = self
            .project
            .clips
            .values()
            .filter_map(|clip| match &clip.source {
                ClipSource::Notes(data) => data
                    .notes
                    .values()
                    .map(|note| clip.start + note.start + note.length)
                    .max(),
                _ => None,
            })
            .max()
            .unwrap_or(0);
        self.project
            .tempo_map
            .tick_to_sample(last_tick + release_tail)
    }
}

/// Builds the phrase `--play-sf2` demos: an ascending root-third-fifth run in
/// eighth notes, then the full triad held as a chord.
///
/// The chord matters more than it looks: three notes starting on the *same*
/// tick is the case that exercises real polyphony, and it's what caught the
/// voice-mixing bug where each new voice re-enveloped the ones already mixed
/// into the shared output buffer.
pub fn demo_song(root_key: u8, bpm: f64, sample_rate: u32) -> Song {
    let eighth = PPQN / 2;
    let major_third = 4;
    let fifth = 7;

    let mut project = Project::new("Fontelle demo");
    project.tempo_map = TempoMap::new(bpm, sample_rate as f64);

    let channel = project.channels.insert(Channel {
        name: "Imported SF2".to_string(),
        color: [0x4f, 0x8f, 0xd0, 0xff],
        mixer_track: Default::default(),
        patch_data: None,
    });
    let lane = project.lanes.insert(Lane {
        name: "Lane 1".to_string(),
        height: 32.0,
        color: [0x4f, 0x8f, 0xd0, 0xff],
        muted: false,
        locked: false,
    });

    let mut notes = SlotMap::default();
    let mut add = |start: Tick, length: Tick, key: u8, velocity: u8| {
        notes.insert(Note {
            start,
            length,
            key,
            velocity,
            pan: 0,
            fine_pitch: 0,
            release: 0,
            mod_x: 0,
            mod_y: 0,
        });
    };

    // Ascending run: root, third, fifth as eighth notes, crescendo. The
    // velocities are the point as much as the pitches — SF2's default
    // velocity -> attenuation curve is quadratic, so 40/80/120 is roughly
    // -10 dB / -4 dB / -0.5 dB, an obvious swell rather than a subtle one.
    for (step, (interval, velocity)) in [(0, 40), (major_third, 80), (fifth, 120)]
        .iter()
        .enumerate()
    {
        add(
            step as Tick * eighth,
            eighth,
            root_key.saturating_add(*interval),
            *velocity,
        );
    }
    // Then the whole triad together, held for a half note, at a middling
    // velocity so it sits below the run's peak.
    let chord_start = 3 * eighth;
    let chord_length = PPQN * 2;
    for interval in [0, major_third, fifth] {
        add(
            chord_start,
            chord_length,
            root_key.saturating_add(interval),
            100,
        );
    }

    let clip_length = chord_start + chord_length;
    project.clips.insert(Clip {
        lane,
        start: 0,
        length: clip_length,
        source: ClipSource::Notes(NoteData { channel, notes }),
        prefab_link: None,
        color: None,
        muted: false,
    });

    Song {
        project,
        channels: vec![SongChannel {
            channel,
            node: node_id(0),
            gain_db: 0.0,
        }],
    }
}

/// Builds the audio graph for `song`: one sampler per entry in
/// `song.channels`, in the same order, **each on a mixer track of its own**,
/// every track summing into a stereo master pair.
///
/// The buses are laid out as `[0, 1]` for the master and `[2 + 2i, 3 + 2i]`
/// for part *i*, so a part's fader, mute and pan act on that part alone. One
/// shared fader was all there was before, which meant a song's balance could
/// only be set by editing the instruments.
///
/// # Panics
///
/// If `samplers` and `song.channels` differ in length. A part with no
/// instrument would be silent and an instrument with no part would never be
/// addressed; both are far easier to diagnose here than by ear.
pub fn build_graph(song: &Song, samplers: Vec<Sampler>, store: Arc<SampleStore>) -> CompiledGraph {
    build_graph_with_gain(song, samplers, store, DEMO_TRACK_GAIN_DB).graph
}

/// A built graph and the master levels anything off the RT thread can read
/// from it. The handle has to be taken before the graph goes to the audio
/// callback, because after that nothing owns the node any more.
pub struct BuiltGraph {
    pub graph: CompiledGraph,
    pub master: Arc<fontelle_engine::MasterMeter>,
}

/// As [`build_graph`], with the **master** fader set explicitly. Each part
/// keeps its own fader from `song.channels`.
pub fn build_graph_with_gain(
    song: &Song,
    samplers: Vec<Sampler>,
    store: Arc<SampleStore>,
    master_gain_db: f32,
) -> BuiltGraph {
    assert_eq!(
        samplers.len(),
        song.channels.len(),
        "build_graph needs exactly one sampler per song channel"
    );

    // A track fader is a *balance* control here, not a pan law: what reaches
    // it is already placed in the field by the voice, on the constant-power
    // taper, and a second pan law on top would pull another 3 dB out of every
    // centred track. Placing a part is `Sampler::set_pan`'s job; this is for
    // riding the levels between them.
    let track_fader = |gain_db: f32| MixerTrackNode {
        gain_db,
        pan_law: fontelle_types::PanLaw::Linear,
        ..MixerTrackNode::new()
    };

    let mut schedule: Vec<ScheduledNode> = Vec::new();
    for (index, (part, sampler)) in song.channels.iter().zip(samplers).enumerate() {
        let bus = vec![MASTER_BUSES + index * 2, MASTER_BUSES + index * 2 + 1];
        schedule.push(ScheduledNode {
            id: part.node,
            node: Box::new(SamplerNode::new(sampler, store.clone())),
            input_buffers: Vec::new(),
            output_buffers: bus.clone(),
        });
        schedule.push(ScheduledNode {
            id: NodeId::default(),
            node: Box::new(track_fader(part.gain_db)),
            // Same buffers in and out: the fader processes in place.
            input_buffers: bus.clone(),
            output_buffers: bus.clone(),
        });
        schedule.push(ScheduledNode {
            id: NodeId::default(),
            node: Box::new(fontelle_engine::BusSumNode),
            input_buffers: bus,
            output_buffers: vec![0, 1],
        });
    }

    schedule.push(ScheduledNode {
        id: NodeId::default(),
        node: Box::new(track_fader(master_gain_db)),
        input_buffers: vec![0, 1],
        output_buffers: vec![0, 1],
    });
    // Last in the schedule, after every track has arrived: a brickwall
    // limiter and the master meters. This is what lets the master fader sit at
    // unity — the peaks an arrangement reaches are a property of the material,
    // and picking a gain that neither clips nor throws away 20 dB was a
    // judgement the tool could not make.
    let master = fontelle_engine::MasterNode::new();
    let master_meter = master.meter();
    schedule.push(ScheduledNode {
        id: NodeId::default(),
        node: Box::new(master),
        input_buffers: vec![0, 1],
        output_buffers: vec![0, 1],
    });

    let mut graph = CompiledGraph {
        schedule,
        buffer_pool: BufferPool::with_capacity(MASTER_BUSES + song.channels.len() * 2, BLOCK_SIZE),
    };
    graph.prepare(SAMPLE_RATE as f32, BLOCK_SIZE as u32);
    BuiltGraph {
        graph,
        master: master_meter,
    }
}

/// Buffers 0 and 1 are the master pair; every part's bus starts after them.
const MASTER_BUSES: usize = 2;

/// The rate everything in the demo path runs at: the device is asked for it,
/// the tempo map converts against it, and offline renders match it exactly.
pub const SAMPLE_RATE: u32 = 48_000;

/// The two halves of TDD §7.6's independent quality settings.
///
/// A user works at `PLAYBACK_QUALITY` and bounces at `RENDER_QUALITY` without
/// thinking about it: an export is not real-time, so it can afford the better
/// kernel. Applied via `Sampler::set_quality`, which is session state — the
/// document is never touched, and a layer that pins its own mode keeps it.
pub const PLAYBACK_QUALITY: fontelle_dsp::Interpolation = fontelle_dsp::Interpolation::Normal;
/// See [`PLAYBACK_QUALITY`].
pub const RENDER_QUALITY: fontelle_dsp::Interpolation = fontelle_dsp::Interpolation::High;

/// The master fader's default: **unity**.
///
/// It was -12 dB of headroom chosen by hand, because a whole arrangement
/// summing onto one bus peaks wherever the material puts it and a gain that
/// neither clips nor throws away 20 dB is a judgement about the piece. The
/// master limiter makes that judgement unnecessary, so the fader is a fader
/// again. `--gain-db` still overrides it, and the render reports both its peak
/// and how hard the limiter had to work.
pub const DEMO_TRACK_GAIN_DB: f32 = 0.0;

/// Renders `song` through `graph` offline, as fast as the CPU allows, into
/// interleaved stereo `f32`.
///
/// Drives the graph exactly the way `AudioDevice`'s callback does — same
/// block size, same `events_for_block` cursor walk — so what comes out is
/// what the device would have played. That equivalence is the point: it makes
/// the audio inspectable (and diffable, and testable) without a sound card,
/// which is the only practical way to debug "it sounds wrong" and the
/// foundation of the offline bounce in TDD §22's M6.
pub fn render_offline(song: &Song, graph: &mut CompiledGraph, total_samples: i64) -> Vec<f32> {
    let transport = fontelle_engine::Transport::new();
    // `Rendering`, not `Playing`: same processing, and the difference is
    // visible to any node that asks — a bounce is not real time, and a node
    // that behaves differently when nobody is listening (a live input, a
    // random source that should be reproducible) needs to be able to tell.
    transport.set_state(fontelle_engine::TransportState::Rendering);
    render_offline_with_transport(song, graph, total_samples, &transport)
}

/// [`render_offline`] driven by a caller-supplied transport, so an offline
/// render can honour a loop, start from a cue point, or be stopped — the same
/// controls the device path has, through the same code.
///
/// `total_samples` is how much audio to *produce*. With a loop enabled that is
/// no longer the same thing as how far the playhead travels, which is the
/// point: bouncing four bars of a one-bar loop is a render of 4x the loop
/// length.
pub fn render_offline_with_transport(
    song: &Song,
    graph: &mut CompiledGraph,
    total_samples: i64,
    transport: &fontelle_engine::Transport,
) -> Vec<f32> {
    let timeline = song.compile();
    let mut reader = fontelle_engine::TransportReader::new();

    let mut out = Vec::with_capacity(total_samples as usize * 2);
    let mut produced = 0i64;

    while produced < total_samples {
        let remaining = (total_samples - produced) as usize;
        let step = reader.next_step(transport, &timeline, remaining, BLOCK_SIZE, false);
        // Silence is served whole, so it can be longer than a block; the graph
        // never renders more than one.
        let frames = step.frames.min(remaining);
        if step.reset {
            graph.reset_sequenced();
        }
        if step.process {
            graph.process_block(step.events, step.snapshot, step.range.clone());
            for i in 0..frames {
                out.push(graph.buffer_pool.buffer_mut(0)[i]);
                out.push(graph.buffer_pool.buffer_mut(1)[i]);
            }
        } else {
            out.extend(std::iter::repeat_n(0.0, frames * 2));
        }
        produced += frames as i64;
    }

    out
}

/// Writes interleaved `f32` samples as a 16-bit PCM WAV.
///
/// Samples outside `-1.0..=1.0` are **clamped, and the count is returned** —
/// silently wrapping them would turn clipping into noise that looks like a
/// synthesis bug, which is exactly the confusion this function exists to
/// resolve.
pub fn write_wav16(
    path: &Path,
    interleaved: &[f32],
    channels: u16,
    sample_rate: u32,
) -> std::io::Result<usize> {
    use std::io::Write;

    let bits = 16u16;
    let block_align = channels * bits / 8;
    let data_len = interleaved.len() as u32 * 2;

    let mut buf = Vec::with_capacity(44 + data_len as usize);
    buf.extend_from_slice(b"RIFF");
    buf.extend_from_slice(&(36 + data_len).to_le_bytes());
    buf.extend_from_slice(b"WAVE");

    buf.extend_from_slice(b"fmt ");
    buf.extend_from_slice(&16u32.to_le_bytes()); // PCM fmt chunk size
    buf.extend_from_slice(&1u16.to_le_bytes()); // format = PCM
    buf.extend_from_slice(&channels.to_le_bytes());
    buf.extend_from_slice(&sample_rate.to_le_bytes());
    buf.extend_from_slice(&(sample_rate * block_align as u32).to_le_bytes());
    buf.extend_from_slice(&block_align.to_le_bytes());
    buf.extend_from_slice(&bits.to_le_bytes());

    buf.extend_from_slice(b"data");
    buf.extend_from_slice(&data_len.to_le_bytes());

    let mut clipped = 0;
    for &s in interleaved {
        if s.abs() > 1.0 {
            clipped += 1;
        }
        let v = (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        buf.extend_from_slice(&v.to_le_bytes());
    }

    std::fs::File::create(path)?.write_all(&buf)?;
    Ok(clipped)
}
