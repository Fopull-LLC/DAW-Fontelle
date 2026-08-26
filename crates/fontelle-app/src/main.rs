use std::sync::Arc;

use fontelle_app::{Song, demo_song};
use fontelle_core::{PrepareContext, SampleStore, Sampler};
use fontelle_engine::{AudioDevice, BLOCK_SIZE};
use fontelle_types::PPQN;

// INVARIANT 1 enforcement (FONTELLE_TDD.md §20.4): only the final binary can set
// the process's global allocator, so it's installed here rather than in
// `fontelle-engine` itself.
#[global_allocator]
static ALLOCATOR: fontelle_engine::RtGuardAllocator = fontelle_engine::RtGuardAllocator;

use fontelle_app::SAMPLE_RATE;
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
    gain_db: f32,
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

    // One instrument per part. For the built-in phrase that is a single
    // `--preset`; for a MIDI file it is whatever each channel's program change
    // asked for, which is what makes an arrangement play as written rather
    // than every part on one sound.
    let mut midi_pans: Option<Vec<f32>> = None;
    let (song, samplers) = match midi {
        Some((midi_path, channels)) => {
            let import = fontelle_assets::import_midi(midi_path, channels)
                .map_err(|e| format!("failed to import {}: {e}", midi_path.display()))?;
            println!("{}:", midi_path.display());

            let mut patches = Vec::new();
            for part in &import.channels {
                let chosen = choose_preset(&presets, part, preset);
                let name = presets
                    .get(chosen)
                    .map(|p| p.name.as_str())
                    .unwrap_or("<unknown>");
                let asked = part
                    .program
                    .map(|p| format!(" program={p}"))
                    .unwrap_or_else(|| " (no program change)".to_string());
                // Zero-based internally, one-based here: every DAW and every
                // piece of MIDI documentation counts channels from 1.
                // Pan and level are printed because they change what you
                // hear and the file is the only place they came from: a part
                // that arrives silent because its CC7 said so should be
                // visible here rather than a mystery.
                println!(
                    "  channel {:<2} {:>6} notes{asked} -> preset {chosen} \"{name}\"\n\
                     {:>16} {:+.2} pan  {:+.1} dB",
                    part.midi_channel + 1,
                    part.notes,
                    "",
                    part.pan,
                    part.volume_db,
                );
                patches.push(
                    fontelle_assets::import_sf2_preset(path, chosen, &mut store)
                        .map_err(|e| format!("failed to import preset {chosen}: {e}"))?,
                );
            }
            for skipped in &import.skipped {
                println!(
                    "  channel {:<2} {:>6} notes  skipped{}",
                    skipped.channel + 1,
                    skipped.notes,
                    if skipped.channel == 9 {
                        " (percussion — --midi-all to include it)"
                    } else {
                        ""
                    }
                );
            }
            println!(
                "  {:.1} bpm{}  (--midi-channel <n> to isolate one, 1-based)\n",
                import.bpm,
                match import.tempo_changes - 1 {
                    0 => String::new(),
                    1 => " to start, then 1 tempo change".to_string(),
                    n => format!(" to start, then {n} tempo changes"),
                }
            );
            midi_pans = Some(import.channels.iter().map(|c| c.pan).collect());
            (Song::from_midi(import, SAMPLE_RATE), patches)
        }
        None => {
            let patch = fontelle_assets::import_sf2_preset(path, preset, &mut store)
                .map_err(|e| format!("failed to import {}: {e}", path.display()))?;
            (demo_song(root_key, BPM, SAMPLE_RATE), vec![patch])
        }
    };

    let quality = if render_wav.is_some() {
        // An offline bounce is not real-time, so it renders at export quality
        // rather than at whatever the patch asks for during playback.
        fontelle_app::RENDER_QUALITY
    } else {
        fontelle_app::PLAYBACK_QUALITY
    };
    // Where each part sits in the stereo field. Applied at the sampler rather
    // than at its mixer track because the track fader is a balance control
    // over an already-placed stereo bus, while this is the constant-power
    // placement of a part that is essentially mono — the same distinction a
    // DAW draws between a mono and a stereo track.
    let pans: Vec<f32> = match &midi_pans {
        Some(pans) => pans.clone(),
        None => vec![0.0; samplers.len()],
    };
    let samplers: Vec<Sampler> = samplers
        .into_iter()
        .zip(pans)
        .map(|(patch, pan)| {
            let mut sampler = Sampler::new(patch);
            sampler.prepare(&PrepareContext {
                sample_rate: SAMPLE_RATE as f32,
                max_block_size: BLOCK_SIZE as u32,
            });
            sampler.set_quality(quality);
            sampler.set_pan(pan);
            sampler
        })
        .collect();

    let timeline = song.compile();

    // One beat of tail so the final chord's release rings out instead of
    // being chopped off when the stream stops.
    let duration_samples = song.duration_samples(PPQN);
    let duration = std::time::Duration::from_secs_f64(duration_samples as f64 / SAMPLE_RATE as f64);

    // Offline bounce instead of the device: renders the identical signal
    // path, so the WAV is what you'd have heard — inspectable without a
    // sound card.
    if let Some(out) = render_wav {
        let built = fontelle_app::build_graph_with_gain(&song, samplers, Arc::new(store), gain_db);
        let mut graph = built.graph;
        let pcm = fontelle_app::render_offline(&song, &mut graph, duration_samples);
        let reduction_db = built.master.take_max_reduction_db();
        let clipped = fontelle_app::write_wav16(out, &pcm, 2, SAMPLE_RATE)
            .map_err(|e| format!("failed to write {}: {e}", out.display()))?;
        let peak = pcm.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        println!(
            "wrote {} ({} frames, peak {:.3}{}{}) at {:?} interpolation",
            out.display(),
            pcm.len() / 2,
            peak,
            if reduction_db > 0.01 {
                format!(", limiter took {reduction_db:.1} dB at its hardest")
            } else {
                String::new()
            },
            if clipped > 0 {
                format!(", {clipped} CLIPPED samples")
            } else {
                String::new()
            },
            fontelle_app::RENDER_QUALITY
        );
        return Ok(());
    }

    let graph =
        fontelle_app::build_graph_with_gain(&song, samplers, Arc::new(store), gain_db).graph;
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

/// Picks the SF2 preset a MIDI part asked for.
///
/// General MIDI puts melodic programs in bank 0 and drum kits in bank 128, and
/// a program change names the program within that bank. Matching on both is
/// what lets a file's bass part come out as a bass; matching on program alone
/// would hand a drum channel whichever melodic instrument shared its number.
///
/// Falls back to `default_preset` when the part named no program, or named one
/// the file does not contain — a soundfont is under no obligation to be a
/// complete General MIDI set, and playing the part on something is better than
/// dropping it silently.
fn choose_preset(
    presets: &[fontelle_assets::PresetInfo],
    part: &fontelle_assets::ImportedMidiChannel,
    default_preset: usize,
) -> usize {
    const PERCUSSION_BANK: u16 = 128;
    let bank = if part.is_percussion {
        PERCUSSION_BANK
    } else {
        0
    };
    part.program
        .and_then(|program| {
            presets
                .iter()
                .find(|p| p.bank == bank && p.program == program as u16)
        })
        // A percussion part with no program change still wants a kit, and
        // every General MIDI bank 128 starts with one.
        .or_else(|| {
            part.is_percussion
                .then(|| presets.iter().find(|p| p.bank == PERCUSSION_BANK))
                .flatten()
        })
        .map(|p| p.index)
        .unwrap_or(default_preset)
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

        // The demo phrase's three coincident voices need the default headroom;
        // a whole arrangement through the same fader lands about 20 dB down.
        // Until there is a master limiter, that is a judgement about the
        // material rather than something the tool can settle.
        let gain_db = args
            .iter()
            .position(|a| a == "--gain-db")
            .and_then(|i| args.get(i + 1))
            .and_then(|v| v.parse::<f32>().ok())
            .unwrap_or(fontelle_app::DEMO_TRACK_GAIN_DB);

        if let Err(e) = play_sf2(
            &path,
            root_key,
            preset,
            render_wav.as_deref(),
            midi,
            gain_db,
        ) {
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
         [--gain-db <db>] [--render-wav <out.wav>]` for the M0 vertical slice instead)"
    )
}
