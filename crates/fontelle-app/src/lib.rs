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
pub struct Song {
    pub project: Project,
    pub channel: ChannelId,
    pub node: NodeId,
}

impl Song {
    /// Wraps an imported MIDI file so it plays through the same document ->
    /// sequencer -> timeline path the built-in phrase does. A second playback
    /// route for files would be a second route none of this project's
    /// invariants cover.
    ///
    /// The import puts every selected MIDI channel's notes on one document
    /// channel, so this is monotimbral: one instrument for the whole file. See
    /// `fontelle_assets::midi_import` for why.
    pub fn from_midi(import: fontelle_assets::MidiImport, sample_rate: u32) -> Self {
        let mut project = import.project;
        project.tempo_map = TempoMap::new(import.bpm, sample_rate as f64);
        Self {
            project,
            channel: import.channel,
            node: NodeId::default(),
        }
    }

    /// The mapping `fontelle_sequencer::compile` needs to turn document
    /// channels into engine node targets.
    pub fn channel_nodes(&self) -> HashMap<ChannelId, NodeId> {
        HashMap::from([(self.channel, self.node)])
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

    // The engine-side identity the compiled events target. Nothing builds a
    // real graph-from-project step yet (engine work past this slice — see
    // PROGRESS.md), so it's minted here alongside the graph in `build_graph`.
    let mut node_ids: SlotMap<NodeId, ChannelId> = SlotMap::default();
    let node = node_ids.insert(channel);

    Song {
        project,
        channel,
        node,
    }
}

/// Assembles the M0 signal chain for `song`: sampler -> mixer track -> stereo
/// bus pair, ready to hand to `AudioDevice::start_output_stream`.
pub fn build_graph(song: &Song, sampler: Sampler, store: Arc<SampleStore>) -> CompiledGraph {
    let mut graph = CompiledGraph {
        schedule: vec![
            ScheduledNode {
                id: song.node,
                node: Box::new(SamplerNode::new(sampler, store)),
                input_buffers: Vec::new(),
                output_buffers: vec![0, 1],
            },
            ScheduledNode {
                id: NodeId::default(),
                node: Box::new(MixerTrackNode {
                    // Headroom, deliberately. The demo's chord is three voices
                    // at whatever gain the SF2's own InitialAttenuation gave
                    // them — often 0 dB — and three coincident voices sum well
                    // past full scale, which clips at the device and reads as a
                    // bug in the sampler. Velocity now pulls its weight (the
                    // chord's velocity of 100 is already about -4 dB), but a
                    // fader with headroom is the correct place to solve this,
                    // not a velocity value chosen to hide it.
                    gain_db: DEMO_TRACK_GAIN_DB,
                    ..MixerTrackNode::new()
                }),
                // Same buffers in and out: processes in place.
                input_buffers: vec![0, 1],
                output_buffers: vec![0, 1],
            },
        ],
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

/// Enough headroom for the demo's three-voice chord not to clip. Not a general
/// answer — a real project needs a master limiter, which is later work.
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
