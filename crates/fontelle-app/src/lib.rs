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

pub struct Song {
    pub project: Project,
    /// Each document channel and the engine node that renders it, in a stable
    /// order. One entry for the built-in phrase; one per part for an imported
    /// file.
    pub channels: Vec<(ChannelId, NodeId)>,
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
        project.tempo_map = TempoMap::new(import.bpm, sample_rate as f64);
        Self {
            channels: import
                .channels
                .iter()
                .enumerate()
                .map(|(index, imported)| (imported.channel, node_id(index)))
                .collect(),
            project,
        }
    }

    /// The mapping `fontelle_sequencer::compile` needs to turn document
    /// channels into engine node targets.
    pub fn channel_nodes(&self) -> HashMap<ChannelId, NodeId> {
        self.channels.iter().copied().collect()
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
        patch_data: Vec::new(),
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
        channels: vec![(channel, node_id(0))],
    }
}

/// Assembles the M0 signal chain for `song`: sampler -> mixer track -> stereo
/// bus pair, ready to hand to `AudioDevice::start_output_stream`.
/// Builds the audio graph for `song`: one sampler per entry in
/// `song.channels`, in the same order, all summing onto one stereo bus and
/// through a single mixer track.
///
/// # Panics
///
/// If `samplers` and `song.channels` differ in length. A part with no
/// instrument would be silent and an instrument with no part would never be
/// addressed; both are far easier to diagnose here than by ear.
pub fn build_graph(song: &Song, samplers: Vec<Sampler>, store: Arc<SampleStore>) -> CompiledGraph {
    build_graph_with_gain(song, samplers, store, DEMO_TRACK_GAIN_DB)
}

/// As [`build_graph`], with the mixer track's fader set explicitly.
pub fn build_graph_with_gain(
    song: &Song,
    samplers: Vec<Sampler>,
    store: Arc<SampleStore>,
    gain_db: f32,
) -> CompiledGraph {
    assert_eq!(
        samplers.len(),
        song.channels.len(),
        "build_graph needs exactly one sampler per song channel"
    );
    let mut schedule: Vec<ScheduledNode> = song
        .channels
        .iter()
        .zip(samplers)
        .map(|((_, node), sampler)| ScheduledNode {
            id: *node,
            node: Box::new(SamplerNode::new(sampler, store.clone())),
            input_buffers: Vec::new(),
            // Every instrument writes the same pair: the graph clears the bus
            // each block and sources add into it.
            output_buffers: vec![0, 1],
        })
        .collect();
    schedule.push(ScheduledNode {
        id: NodeId::default(),
        node: Box::new(MixerTrackNode {
            // Headroom, deliberately. Coincident voices sum well past full
            // scale — three at whatever gain the SF2's own InitialAttenuation
            // gave them, often 0 dB — which clips at the device and reads as a
            // bug in the sampler. With a whole arrangement summing here rather
            // than one part, the fader matters more, not less. Velocity pulls
            // its weight now, but a fader with headroom is the correct place
            // to solve this, not a velocity value chosen to hide it.
            gain_db,
            // A *balance* control, not a pan law. The signal reaching this
            // track is genuinely stereo now — the voice places each layer in
            // the field on the constant-power taper — and a pan law is for
            // putting a mono source somewhere. Applying one to an
            // already-placed stereo signal just pulls another 3 dB out of a
            // centred track for nothing.
            pan_law: fontelle_types::PanLaw::Linear,
            ..MixerTrackNode::new()
        }),
        // Same buffers in and out: processes in place.
        input_buffers: vec![0, 1],
        output_buffers: vec![0, 1],
    });
    let mut graph = CompiledGraph {
        schedule,
        buffer_pool: BufferPool::with_capacity(2, BLOCK_SIZE),
    };
    graph.prepare(SAMPLE_RATE as f32, BLOCK_SIZE as u32);
    graph
}

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

/// Enough headroom for the demo's three-voice chord not to clip.
///
/// Explicitly not a general answer. A whole arrangement summing through this
/// fader lands around 20 dB down, which is why it is a default rather than a
/// constant: `--gain-db` overrides it, and the render reports its peak so the
/// choice can be made on evidence. The real answer is a master limiter, which
/// is later work.
pub const DEMO_TRACK_GAIN_DB: f32 = -12.0;

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
    let timeline = song.compile();
    let transport = fontelle_engine::TransportSnapshot {
        state: fontelle_engine::TransportState::Playing,
        position_sample: 0,
    };

    let mut out = Vec::with_capacity(total_samples as usize * 2);
    let mut cursor = 0usize;
    let mut sample = 0i64;

    while sample < total_samples {
        let chunk = BLOCK_SIZE.min((total_samples - sample) as usize);
        let range = sample..sample + chunk as i64;
        let events = timeline.events_for_block(&mut cursor, range.clone());
        graph.process_block(events, transport, range);

        for i in 0..chunk {
            out.push(graph.buffer_pool.buffer_mut(0)[i]);
            out.push(graph.buffer_pool.buffer_mut(1)[i]);
        }
        sample += chunk as i64;
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
