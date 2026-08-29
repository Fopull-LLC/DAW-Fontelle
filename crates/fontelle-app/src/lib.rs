//! The DAW binary's non-UI guts, in a library so they're testable — a `[[bin]]`
//! can't be imported by an integration test, and "the thing we demo" deserves
//! coverage as much as anything else does.

pub mod bank;
mod bundle;
pub mod desktop;
mod library;
mod realise;
mod session;
pub mod settings;
mod window;

use std::path::{Path, PathBuf};

use fontelle_engine::{BLOCK_SIZE, CompiledGraph};
use fontelle_model::{
    AddChannel, AddClip, Arena, Clip, ClipSource, Command, Lane, Note, NoteData, Project, TempoMap,
};
use fontelle_types::{CompiledTimeline, PPQN, Tick};

pub use bundle::{MissingAsset, OpenError, OpenedProject, open_project, save_project};
pub use library::SampleLibrary;
pub use realise::{
    RealiseError, RealiseOptions, Realised, channel_nodes, realise, set_channel_patch,
};
pub use session::Session;
pub use window::EngineHost;

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

/// The demo phrase `--play-sf2` plays, as a document: an ascending
/// root-third-fifth run in eighth notes, then the full triad held as a chord.
///
/// The chord matters more than it looks: three notes starting on the *same*
/// tick is the case that exercises real polyphony, and it is what caught the
/// voice-mixing bug where each new voice re-enveloped the ones already mixed
/// into the shared output buffer.
///
/// The one channel comes back with no instrument on it — `set_channel_patch`
/// puts one there once the caller has imported a soundfont. That order is not
/// an accident: it is the order the UI will do it in.
pub fn demo_project(root_key: u8, bpm: f64, sample_rate: u32) -> Project {
    let eighth = PPQN / 2;
    let major_third = 4;
    let fifth = 7;

    let mut project = Project::new("Fontelle demo");
    project.tempo_map = TempoMap::new(bpm, sample_rate as f64);

    // Through commands, like every other mutation (INVARIANT 9). There is no
    // history to record into while a document is being built, but going
    // through the same path is what keeps the command set honest about being
    // able to express everything the app does.
    let mut add_channel = AddChannel::new("Imported SF2", None);
    add_channel
        .apply(&mut project)
        .expect("a fresh project must take a channel");
    let channel = add_channel.channel().expect("just applied");

    let lane = project.lanes.insert(Lane {
        name: "Lane 1".to_string(),
        height: 32.0,
        color: [0x4f, 0x8f, 0xd0, 0xff],
        muted: false,
        locked: false,
    });

    let mut notes = Arena::default();
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

    AddClip::new(Clip {
        lane,
        start: 0,
        length: chord_start + chord_length,
        source: ClipSource::Notes(NoteData { channel, notes }),
        prefab_link: None,
        color: None,
        muted: false,
    })
    .apply(&mut project)
    .expect("a fresh project must take a clip");

    project
}

/// An empty project with one channel and one empty clip, ready to be written
/// in.
///
/// `demo_project` exists to *demonstrate* — it arrives with a phrase already on
/// it, which is right for `--play-sf2` and wrong for someone who opened the
/// window to write something. This is the same shape with nothing in it: one
/// instrument, one clip long enough to fill the roll, and a piano roll pointed
/// at it.
pub fn blank_project(bars: i64, bpm: f64, sample_rate: u32) -> Project {
    let mut project = Project::new("Untitled");
    project.tempo_map = TempoMap::new(bpm, sample_rate as f64);

    let mut add_channel = AddChannel::new("Channel 1", None);
    add_channel
        .apply(&mut project)
        .expect("a fresh project must take a channel");
    let channel = add_channel.channel().expect("just applied");

    let lane = project.lanes.insert(Lane {
        name: "Lane 1".to_string(),
        height: 32.0,
        color: [0x4f, 0x8f, 0xd0, 0xff],
        muted: false,
        locked: false,
    });

    // 4/4 until the document has somewhere to keep a time signature.
    AddClip::new(Clip {
        lane,
        start: 0,
        length: PPQN * 4 * bars.max(1),
        source: ClipSource::Notes(NoteData {
            channel,
            notes: Arena::default(),
        }),
        prefab_link: None,
        color: None,
        muted: false,
    })
    .apply(&mut project)
    .expect("a fresh project must take a clip");

    project
}

/// Wraps an imported MIDI file so it plays through the same document ->
/// realisation -> sequencer -> engine path everything else does. A second
/// playback route for files would be a second route none of this project's
/// invariants cover.
///
/// All this has left to do is the sample rate: the importer builds the whole
/// document, including a mixer track per part carrying the file's own CC7 and
/// the channel pan from its CC10.
pub fn project_from_midi(import: fontelle_assets::MidiImport, sample_rate: u32) -> Project {
    let mut project = import.project;
    // `set_sample_rate`, not a fresh `TempoMap`: the import carries the file's
    // whole tempo curve, and building a constant map from `import.bpm` would
    // throw every tempo change away again.
    project.tempo_map.set_sample_rate(sample_rate as f64);
    project
}

/// How long a project runs, in samples, including a tail so the last note's
/// release is not cut off mid-ring.
pub fn project_duration_samples(project: &Project, release_tail: Tick) -> i64 {
    let last_tick = project
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
    project.tempo_map.tick_to_sample(last_tick + release_tail)
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

/// The master fader's default: **unity**.
///
/// It was -12 dB of headroom chosen by hand, because a whole arrangement
/// summing onto one bus peaks wherever the material puts it and a gain that
/// neither clips nor throws away 20 dB is a judgement about the piece. The
/// master limiter makes that judgement unnecessary, so the fader is a fader
/// again. `--gain-db` writes it onto the project's master track, and the
/// render reports both its peak and how hard the limiter had to work.
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
pub fn render_offline(
    timeline: &CompiledTimeline,
    graph: &mut CompiledGraph,
    total_samples: i64,
) -> Vec<f32> {
    let transport = fontelle_engine::Transport::new();
    // `Rendering`, not `Playing`: same processing, and the difference is
    // visible to any node that asks — a bounce is not real time, and a node
    // that behaves differently when nobody is listening (a live input, a
    // random source that should be reproducible) needs to be able to tell.
    transport.set_state(fontelle_engine::TransportState::Rendering);
    render_offline_with_transport(timeline, graph, total_samples, &transport)
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
    timeline: &CompiledTimeline,
    graph: &mut CompiledGraph,
    total_samples: i64,
    transport: &fontelle_engine::Transport,
) -> Vec<f32> {
    let mut reader = fontelle_engine::TransportReader::new();

    let mut out = Vec::with_capacity(total_samples as usize * 2);
    let mut produced = 0i64;

    while produced < total_samples {
        let remaining = (total_samples - produced) as usize;
        let step = reader.next_step(transport, timeline, remaining, BLOCK_SIZE, false);
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
