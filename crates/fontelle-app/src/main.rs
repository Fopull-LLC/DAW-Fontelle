use std::sync::Arc;

use fontelle_app::{Song, build_graph, demo_song};
use fontelle_core::{PrepareContext, SampleStore, Sampler};
use fontelle_engine::{AudioDevice, BLOCK_SIZE};
use fontelle_types::PPQN;

// INVARIANT 1 enforcement (FONTELLE_TDD.md §20.4): only the final binary can set
// the process's global allocator, so it's installed here rather than in
// `fontelle-engine` itself.
#[global_allocator]
static ALLOCATOR: fontelle_engine::RtGuardAllocator = fontelle_engine::RtGuardAllocator;

const SAMPLE_RATE: u32 = 48_000;
const BPM: f64 = 120.0;

/// The M0 vertical slice (TDD §22), runnable for real: audio callback ->
/// compiled graph -> sampler voices reading a real SF2 zone -> mixer track ->
/// device out, every note triggered from a clip on a timeline, with the
/// zero-allocation debug assertion above active.
///
/// **Scope cut, honestly:** the `Project` is built in code by
/// `fontelle_app::demo_song`, not loaded from disk or drawn in a UI (neither
/// exists yet), and `Channel.patch_data` is left empty — the real `Patch`
/// comes straight from the SF2 import below rather than round-tripping
/// through the model's serialised form, which is work for whenever project
/// save/load lands. What *is* real: the document, its tempo map, the
/// sequencer compiling it to a `CompiledTimeline`, the engine reading events
/// out of that timeline block by block, and a mixer track in the signal path.
/// See `PROGRESS.md`.
fn play_sf2(
    path: &std::path::Path,
    root_key: u8,
    preset: usize,
    render_wav: Option<&std::path::Path>,
    midi: Option<(&std::path::Path, fontelle_assets::MidiChannels)>,
) -> Result<(), String> {
    // SF2 files store presets in arbitrary order, so "preset 0" is regularly
    // not the instrument anyone wants — `Secret_of_Mana.sf2` opens with a
    // whale sound effect and keeps its piano at index 27. Show the list
    // rather than silently picking one and letting it sound broken.
    let presets = fontelle_assets::list_presets(path)
        .map_err(|e| format!("failed to read {}: {e}", path.display()))?;
    println!("{} presets in {}:", presets.len(), path.display());
    for p in &presets {
        let marker = if p.index == preset { "->" } else { "  " };
        println!(
            "{marker} [{:>3}] prog={:<3} bank={:<3} {}",
            p.index, p.program, p.bank, p.name
        );
    }
    println!("  (choose another with --preset <index>)\n");

    let mut store = SampleStore::new();
    let patch = fontelle_assets::import_sf2_preset(path, preset, &mut store)
        .map_err(|e| format!("failed to import {}: {e}", path.display()))?;

    let mut sampler = Sampler::new(patch);
    sampler.prepare(&PrepareContext {
        sample_rate: SAMPLE_RATE as f32,
        max_block_size: BLOCK_SIZE as u32,
    });

    let song = match midi {
        Some((midi_path, channels)) => {
            let import = fontelle_assets::import_midi(midi_path, channels)
                .map_err(|e| format!("failed to import {}: {e}", midi_path.display()))?;
            println!("{}:", midi_path.display());
            for summary in &import.source_channels {
                // Zero-based internally, one-based here: every DAW and every
                // piece of MIDI documentation counts channels from 1.
                let taken = if channels_accept(channels, summary.channel) {
                    "playing"
                } else {
                    "skipped"
                };
                let program = summary
                    .program
                    .map(|p| format!(" program={p}"))
                    .unwrap_or_default();
                println!(
                    "  {taken}  channel {:<2} {:>6} notes{program}",
                    summary.channel + 1,
                    summary.notes
                );
            }
            println!(
                "  {:.1} bpm  (--midi-channel <n> to isolate one, 1-based)\n",
                import.bpm
            );
            Song::from_midi(import, SAMPLE_RATE)
        }
        None => demo_song(root_key, BPM, SAMPLE_RATE),
    };
    let timeline = song.compile();

    // One beat of tail so the final chord's release rings out instead of
    // being chopped off when the stream stops.
    let duration_samples = song.duration_samples(PPQN);
    let duration = std::time::Duration::from_secs_f64(duration_samples as f64 / SAMPLE_RATE as f64);

    // Offline bounce instead of the device: renders the identical signal
    // path, so the WAV is what you'd have heard — inspectable without a
    // sound card.
    if let Some(out) = render_wav {
        // An offline bounce is not real-time, so it renders at export quality
        // rather than at whatever the patch asks for during playback.
        sampler.set_quality(fontelle_app::RENDER_QUALITY);
        let mut graph = build_graph(&song, sampler, Arc::new(store));
        let pcm = fontelle_app::render_offline(&song, &mut graph, duration_samples);
        let clipped = fontelle_app::write_wav16(out, &pcm, 2, SAMPLE_RATE)
            .map_err(|e| format!("failed to write {}: {e}", out.display()))?;
        let peak = pcm.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        println!(
            "wrote {} ({} frames, peak {:.3}{}) at {:?} interpolation",
            out.display(),
            pcm.len() / 2,
            peak,
            if clipped > 0 {
                format!(", {clipped} CLIPPED samples")
            } else {
                String::new()
            },
            fontelle_app::RENDER_QUALITY
        );
        return Ok(());
    }

    let graph = build_graph(&song, sampler, Arc::new(store));
    let mut device = AudioDevice::default_host();
    println!(
        "Fontelle: {} note events from {} on {:?}",
        timeline.events.len(),
        path.display(),
        device.default_output_name()
    );
    if midi.is_none() {
        println!(
            "  root key {root_key} at {BPM} bpm — a root/third/fifth run, then the triad held."
        );
    }
    println!("  {:.2}s", duration.as_secs_f64());

    device
        .start_output_stream(graph, timeline, SAMPLE_RATE)
        .map_err(|e| format!("failed to open the default output device: {e}"))?;

    std::thread::sleep(duration);
    device.stop();
    Ok(())
}

/// Mirrors `MidiChannels`' own rule so the listing can say what it skipped.
fn channels_accept(channels: fontelle_assets::MidiChannels, channel: u8) -> bool {
    match channels {
        fontelle_assets::MidiChannels::Melodic => channel != 9,
        fontelle_assets::MidiChannels::All => true,
        fontelle_assets::MidiChannels::Only(wanted) => channel == wanted,
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--play-sf2") {
        // A bad path is ordinary user error, not a bug — report it and exit
        // non-zero rather than dumping a panic and a backtrace hint.
        let path = match fontelle_app::resolve_sf2_path(&args, |p| p.exists()) {
            Ok(path) => path,
            Err(e) => {
                eprintln!("Fontelle: {e}");
                std::process::exit(1);
            }
        };
        let numeric_flag = |name: &str| {
            args.iter()
                .position(|a| a == name)
                .and_then(|i| args.get(i + 1))
                .and_then(|v| v.parse::<usize>().ok())
        };
        let root_key = numeric_flag("--key").unwrap_or(60).min(127) as u8;
        let preset = numeric_flag("--preset").unwrap_or(0);

        let render_wav = args
            .iter()
            .position(|a| a == "--render-wav")
            .and_then(|i| args.get(i + 1))
            .map(std::path::PathBuf::from);

        let midi_path = args
            .iter()
            .position(|a| a == "--play-midi")
            .and_then(|i| args.get(i + 1))
            .map(std::path::PathBuf::from);
        // 1-based on the command line, 0-based in the file, because that is
        // how every DAW and every piece of MIDI documentation numbers them.
        let midi_channels = match numeric_flag("--midi-channel") {
            Some(n) if n >= 1 => fontelle_assets::MidiChannels::Only(n as u8 - 1),
            Some(_) => {
                eprintln!("Fontelle: --midi-channel is 1-based; channel 10 is percussion");
                std::process::exit(1);
            }
            None if args.iter().any(|a| a == "--midi-all") => fontelle_assets::MidiChannels::All,
            None => fontelle_assets::MidiChannels::Melodic,
        };
        let midi = midi_path.as_deref().map(|p| (p, midi_channels));

        if let Err(e) = play_sf2(&path, root_key, preset, render_wav.as_deref(), midi) {
            eprintln!("Fontelle: {e}");
            std::process::exit(1);
        }
        return;
    }

    // The full DAW: docked panels, timeline, transport. Not built yet — see
    // PROGRESS.md for what's real (the audio/sampler/sequencer path above)
    // versus what this still needs (fontelle-ui windowing, a document loaded
    // from disk rather than built in code).
    todo!(
        "winit event loop -> fontelle-ui docked panels -> fontelle-engine::AudioDevice \
         -> fontelle-sequencer::compile -> CompiledTimeline over triple_buffer \
         (run with `--play-sf2 <path.sf2> [--preset <n>] [--key <note>] \
         [--play-midi <file.mid>] [--midi-channel <1-16> | --midi-all] \
         [--render-wav <out.wav>]` for the M0 vertical slice instead)"
    )
}
