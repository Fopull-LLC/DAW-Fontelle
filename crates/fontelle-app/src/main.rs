use std::sync::Arc;

use fontelle_app::{RealiseOptions, SampleLibrary, demo_project, project_from_midi};
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
/// `fontelle_app::demo_project` or imported from a MIDI file, not loaded from
/// disk or drawn in a UI (neither exists yet). Everything after that is the
/// real path: the imported patch goes *onto the document* in its serialised
/// form and comes back out through `realise`, which builds the graph, the bus
/// layout and the channel->node map from `Project::channels` and
/// `Project::mixer`. So every run of this exercises the save/load round trip
/// even before there is a file to save into.
/// Everything `--play-sf2` was asked for beyond the soundfont itself.
///
/// A struct rather than eight positional parameters: the flags are all
/// independent of each other, several are `Option`s, and two are booleans that
/// would be indistinguishable at a call site.
struct PlayOptions<'a> {
    /// Start from an empty clip rather than the built-in demo phrase — the
    /// difference between a demo you can poke and a tool you can write in.
    blank: bool,
    root_key: u8,
    preset: usize,
    midi: Option<(&'a std::path::Path, fontelle_assets::MidiChannels)>,
    playback: Playback<'a>,
}

/// What to do with a project once there is one — the half `--open` shares with
/// `--play-sf2`.
struct Playback<'a> {
    render_wav: Option<&'a std::path::Path>,
    save: Option<&'a std::path::Path>,
    gain_db: f32,
    cue: Cue,
    midi_in: bool,
    /// Capture what is played into a new clip (TDD §14.7). Needs `--midi-in`;
    /// there is nothing else for a keyboard to arrive through.
    record: bool,
    /// How long to record for. `None` records until the song ends, which is
    /// the only bound a headless run has.
    record_seconds: Option<f64>,
    /// Drive playback from the window's transport bar rather than playing
    /// straight through and exiting (item 7 of `docs/first-usable-plan.md`).
    window: bool,
    /// Printed once the graph is up, when there is something worth saying
    /// about how the project was built.
    announce: Option<String>,
    /// The window's theme (§16.6). Read here rather than in the window so a
    /// `--theme` file is honoured on every path that opens one.
    theme: fontelle_ui::Theme,
    /// Extra soundfont folders the user named on the command line. Remembered
    /// in the settings file, so it only has to be said once (INVARIANT 10).
    soundfont_dirs: Vec<std::path::PathBuf>,
    /// Close the window by itself after this long, and say how many frames it
    /// drew. There is no other way to run the window unattended, and "it
    /// opened, drew, and then sat there drawing nothing" is exactly the claim
    /// TDD §16.3 makes and §19 measures.
    run_for: Option<std::time::Duration>,
    /// Open on the start menu. A plain launch does; a project named on the
    /// command line goes straight to it, and `--no-menu` skips it too — for
    /// the driven runs that photograph the studio.
    welcome: bool,
}

fn play_sf2(path: &std::path::Path, options: PlayOptions<'_>) -> Result<(), String> {
    let PlayOptions {
        blank,
        root_key,
        preset,
        midi,
        playback,
    } = options;
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

    let mut library = SampleLibrary::new();

    // One instrument per part. For the built-in phrase that is a single
    // `--preset`; for a MIDI file it is whatever each channel's program change
    // asked for, which is what makes an arrangement play as written rather
    // than every part on one sound.
    let project = match midi {
        Some((midi_path, channels)) => {
            let import = fontelle_assets::import_midi(midi_path, channels)
                .map_err(|e| format!("failed to import {}: {e}", midi_path.display()))?;
            println!("{}:", midi_path.display());

            let mut chosen_presets = Vec::new();
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
                chosen_presets.push((part.channel, chosen));
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

            let mut project = project_from_midi(import, SAMPLE_RATE);
            for (channel, chosen) in chosen_presets {
                let patch = library
                    .import_sf2(path, chosen)
                    .map_err(|e| format!("failed to import preset {chosen}: {e}"))?;
                fontelle_app::set_channel_patch(&mut project, channel, &patch, &library)
                    .map_err(|e| format!("failed to store preset {chosen}: {e}"))?;
            }
            project
        }
        None => {
            let patch = library
                .import_sf2(path, preset)
                .map_err(|e| format!("failed to import {}: {e}", path.display()))?;
            let mut project = if blank {
                fontelle_app::blank_project(8, BPM, SAMPLE_RATE)
            } else {
                demo_project(root_key, BPM, SAMPLE_RATE)
            };
            let channel = project
                .channels
                .keys()
                .next()
                .expect("both starting projects have one channel");
            fontelle_app::set_channel_patch(&mut project, channel, &patch, &library)
                .map_err(|e| format!("failed to store the imported patch: {e}"))?;
            project
        }
    };

    play_or_render(project, library, playback)
}

/// Opens a saved project and plays or renders it — the same path
/// `--play-sf2` takes once it has built one.
fn open_and_play<'a>(
    bundle: &'a std::path::Path,
    mut playback: Playback<'a>,
) -> Result<(), String> {
    let opened = fontelle_app::open_project(bundle).map_err(|e| format!("{e}"))?;
    println!("{}: \"{}\"", bundle.display(), opened.project.meta.name);
    println!(
        "  {} channel(s), {} clip(s), {:.1} bpm",
        opened.project.channels.len(),
        opened.project.clips.len(),
        opened.project.tempo_map.tempo_at(0)
    );
    // TDD §17.4: the project opens and plays with placeholders. Saying which
    // files are gone is the whole point of not refusing.
    for missing in &opened.missing {
        println!(
            "  ! {} could not be read ({}) — {} channel(s) will be silent",
            missing.file.path.display(),
            missing.why,
            missing.channels.len()
        );
    }
    // Opening a project in the window makes that bundle the place Ctrl+S
    // writes to — anything else would be a "save" that quietly went somewhere
    // else, or refused.
    if playback.window && playback.save.is_none() {
        playback.save = Some(bundle);
    }
    play_or_render(opened.project, opened.library, playback)
}

fn play_or_render(
    mut project: fontelle_model::Project,
    library: SampleLibrary,
    options: Playback<'_>,
) -> Result<(), String> {
    let Playback {
        render_wav,
        save,
        gain_db,
        cue,
        midi_in,
        record,
        record_seconds,
        window,
        announce,
        theme,
        soundfont_dirs,
        run_for,
        welcome,
    } = options;
    // An offline bounce is not real-time, so it renders at export quality
    // rather than at whatever the patch asks for during playback (§7.6).
    let quality = if render_wav.is_some() {
        fontelle_app::RENDER_QUALITY
    } else {
        fontelle_app::PLAYBACK_QUALITY
    };

    // `--gain-db` is the master fader, and the master fader is a document
    // value now rather than a parameter threaded into the graph builder — set
    // through a command like every other mutation (INVARIANT 9).
    if let Some(master) = project.mixer.master {
        use fontelle_model::Command;
        fontelle_model::SetNumber::new(
            fontelle_model::NumberTarget::TrackGainDb(master),
            gain_db as f64,
        )
        .apply(&mut project)
        .map_err(|e| format!("{e}"))?;
    }

    // Written before anything is played: a bounce that takes two minutes
    // should not be standing between the user and their project being on
    // disk.
    if let Some(bundle) = save.filter(|_| !window) {
        fontelle_app::save_project(&project, bundle)
            .map_err(|e| format!("failed to save {}: {e}", bundle.display()))?;
        println!("  saved {}", bundle.display());
    }

    // One beat of tail so the final chord's release rings out instead of
    // being chopped off when the stream stops.
    let song_end_samples = fontelle_app::project_duration_samples(&project, PPQN);

    // Beats on the command line, ticks in the document, samples in the
    // engine — and the conversion goes through the song's own `TempoMap`
    // rather than by arithmetic on the BPM (INVARIANT 5). That matters here
    // and not only on principle: a file with a tempo change has no single BPM
    // to multiply by, so "loop bars 5 to 9" is only answerable by the map.
    let transport = Arc::new(fontelle_engine::Transport::new());
    let to_sample = |beats: f64| {
        project
            .tempo_map
            .tick_to_sample((beats * PPQN as f64).round() as i64)
    };
    let start_sample = to_sample(cue.start_beat);
    transport.seek(start_sample);
    let looped = cue.loop_beats.map(|(from, to)| {
        let ticks = (
            (from * PPQN as f64).round() as i64,
            (to * PPQN as f64).round() as i64,
        );
        let samples = (to_sample(from), to_sample(to));
        transport.set_loop_range(ticks, samples);
        transport.set_looping(true);
        samples
    });

    // How much audio there is to play: to the end of the song, or `--repeat`
    // passes of the loop, counted from wherever playback was cued.
    let duration_samples = match looped {
        Some((loop_start, loop_end)) => {
            let pass = loop_end - loop_start;
            (loop_end - start_sample.min(loop_end)) + pass * (cue.repeat.max(1) as i64 - 1)
        }
        None => (song_end_samples - start_sample).max(0),
    };
    let duration = std::time::Duration::from_secs_f64(duration_samples as f64 / SAMPLE_RATE as f64);

    // Offline bounce instead of the device: renders the identical signal
    // path, so the WAV is what you'd have heard — inspectable without a
    // sound card.
    if let Some(out) = render_wav {
        // **With the project's plugins.** The bounce used to build its graph
        // with no rack, so a channel playing a plugin rendered as silence;
        // the rack looks where the settings say, the way the studio's does.
        let (settings, settings_error) = fontelle_app::settings::Settings::load();
        // A settings file that would not read is a rack looking in the wrong
        // places, and "Sine is not installed" is the wrong diagnosis for it.
        if let Some(e) = settings_error {
            println!("  ! settings: {e}");
        }
        let mut plugins = fontelle_app::PluginRack::new();
        plugins.set_folders(settings.plugin_dirs.clone());
        // The same transport the device would be driven by, so a bounce of a
        // looped section is the section as it plays rather than a second code
        // path that has to be kept in step with the first.
        let bounced = fontelle_app::bounce(
            &project,
            &library,
            &mut plugins,
            &transport,
            fontelle_app::BounceOptions {
                quality,
                total_samples: duration_samples,
            },
        )
        .map_err(|e| format!("{e}"))?;
        // The graph is gone, so every plugin can be closed rather than left
        // for the exit to leak.
        plugins.close_all();
        for (_, missing) in &bounced.unresolved {
            println!("  ! layer {} has no audio", missing.layer);
        }
        if let Some(message) = &bounced.message {
            println!("  ! {message}");
        }
        let pcm = bounced.pcm;
        let reduction_db = bounced.reduction_db;
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

    // The plugins the project names, for a **headless** run: the same rack
    // the bounce above uses, kept until the device has stopped. The window
    // has a rack of its own inside its session, which hosts the project's
    // plugins on its first pump — so its first graph is built without one
    // here, and rebuilt there.
    let mut plugins = (!window).then(|| {
        let (settings, settings_error) = fontelle_app::settings::Settings::load();
        if let Some(e) = settings_error {
            println!("  ! settings: {e}");
        }
        let mut plugins = fontelle_app::PluginRack::new();
        plugins.set_folders(settings.plugin_dirs.clone());
        plugins
    });
    let wiring = plugins
        .as_mut()
        .map(|plugins| plugins.realise(&project, f64::from(SAMPLE_RATE), BLOCK_SIZE as u32))
        .unwrap_or_default();
    if let Some(message) = plugins.as_mut().and_then(|plugins| plugins.take_message()) {
        println!("  ! {message}");
    }
    let realised = fontelle_app::realise_hosting(
        &project,
        &library,
        RealiseOptions {
            sample_rate: SAMPLE_RATE,
            block_size: BLOCK_SIZE,
            quality,
        },
        &std::collections::HashMap::new(),
        None,
        &Default::default(),
        None,
        None,
        &wiring,
    )
    .map_err(|e| format!("{e}"))?;
    for (_, missing) in &realised.unresolved {
        println!("  ! layer {} has no audio", missing.layer);
    }

    let timeline =
        fontelle_sequencer::compile(&project, &realised.channel_nodes, &Default::default());

    // Taken before the graph goes to the audio callback, because after that
    // nothing on this side owns it.
    let master = realised.master.clone();
    // The stream reads the timeline through a channel from here on, so an edit
    // made while it is running reaches the RT thread at the next block
    // boundary (TDD §11.3). Nothing republishes it yet on the headless path;
    // the window does.
    let event_count = timeline.events.len();
    let (timeline_publisher, timeline_source) = fontelle_engine::timeline_channel(timeline);
    // The instruments reach the running stream the same way the notes do, so a
    // channel added from inside the window does not need the device restarted.
    let (graph_publisher, graph_source) = fontelle_engine::graph_channel(realised.graph);
    // The ring a live input is heard through (TDD §15.4), made **here**
    // because both ends need it and they are opened at different moments: the
    // output stream below asks it whether the graph has to keep running while
    // the transport is stopped, and the session opens the *input* stream that
    // fills it whenever a mixer track names a microphone. A second of stereo
    // at 48 kHz, which is far more than the gap between two callbacks.
    let monitor = std::sync::Arc::new(fontelle_engine::InputMonitor::new(96_000 * 2));
    let mut device = AudioDevice::default_host().with_monitor(std::sync::Arc::clone(&monitor));
    println!(
        "Fontelle: {} note events, {} on {:?}",
        event_count,
        project.meta.name,
        device.default_output_name()
    );
    if let Some(line) = &announce {
        println!("{line}");
    }
    match looped {
        Some((from, to)) => println!(
            "  looping {:.2}s-{:.2}s x{}  ({:.2}s total)",
            from as f64 / SAMPLE_RATE as f64,
            to as f64 / SAMPLE_RATE as f64,
            cue.repeat.max(1),
            duration.as_secs_f64()
        ),
        None if cue.start_beat > 0.0 => println!(
            "  from {:.2}s  ({:.2}s to the end)",
            start_sample as f64 / SAMPLE_RATE as f64,
            duration.as_secs_f64()
        ),
        None => println!("  {:.2}s", duration.as_secs_f64()),
    }

    // Live input, if asked for. The channel is built here, off the audio
    // thread: the consumer half goes into the callback and never leaves it,
    // and each device that connects claims a producer.
    let (mut live_source, mut live_ports) = fontelle_engine::live_event_channel(
        fontelle_engine::LIVE_PORT_COUNT,
        fontelle_engine::LIVE_PORT_CAPACITY,
    );
    // Armed before the source goes to the callback, because after that nothing
    // owns it. The transport state is what decides whether anything is
    // actually written down, so arming costs nothing when not recording.
    // `--record` on the command line, or a window, which has a record button.
    // Arming the *mirror* costs nothing when nothing is recording: the
    // transport state is what decides whether anything is written down.
    let mut capture = (record || window).then(|| {
        let (writer, reader) =
            fontelle_engine::live_capture_channel(fontelle_engine::CAPTURE_CAPACITY);
        live_source.arm_capture(writer);
        reader
    });
    // Where live MIDI is pointed. The first part's instrument to begin with —
    // §14.3's default, and the answer that needs no UI — and then whatever the
    // window selects, which `Session::with_live_target` publishes onto this
    // same cell. See `fontelle-midi/tests/focus.rs`.
    let live_target = std::sync::Arc::new(fontelle_midi::LiveTarget::new(
        project
            .channels
            .keys()
            .next()
            .and_then(|c| realised.channel_nodes.get(&c).copied())
            .unwrap_or_default(),
    ));
    // **A window always listens.** There is no `--midi-in` to type in a
    // window — you plug a keyboard in and play it — and requiring the flag is
    // why *"I connected my usb midi controller and was pressing keys but got
    // no output"*. On the command line it stays opt-in: a headless render must
    // not open every MIDI port on the machine.
    // How that keyboard's notes are read (TDD §14.3): the velocity curve, the
    // velocity window, transpose and the channel filter. The window's settings
    // tab writes into this same cell, so a change reaches a device that is
    // already plugged in — see `fontelle-midi/tests/input_settings.rs`. The
    // *values* come off the settings file, which `Session::with_input_settings`
    // publishes; until then it holds the identity, which is what a headless
    // `--midi-in` run wants and has always had.
    let live_input = std::sync::Arc::new(fontelle_midi::LiveMapping::default());
    // And which keys those devices are holding down, so the roll's keyboard
    // lights up under what is being played (TDD §14.1) — asked for from
    // playing the studio, so that a phrase can be found on a controller and
    // then written into the grid. Written by every device callback, read by
    // the window once a frame.
    let live_keys = std::sync::Arc::new(fontelle_midi::LiveKeys::default());
    let mut hub = (midi_in || window).then(|| {
        fontelle_midi::MidiHub::new(fontelle_midi::RouteTo {
            target: std::sync::Arc::clone(&live_target),
            // Distinct from anything the sequencer emits, so a sequenced
            // note-off cannot cut a note the player is holding (TDD §11.4).
            voice_context: LIVE_VOICE_CONTEXT,
        })
        .with_input_settings(std::sync::Arc::clone(&live_input))
        .with_live_keys(std::sync::Arc::clone(&live_keys))
    });

    let finish = match looped {
        Some(_) => Finish::LoopPasses(cue.repeat.max(1)),
        None => Finish::AtSample(song_end_samples),
    };

    // Playback is a state on the transport now, rather than a consequence of
    // the stream existing — which is what makes stop, seek and loop reachable
    // at all, and what a UI will drive when there is one.
    if record {
        transport.set_state(fontelle_engine::TransportState::Recording);
    } else {
        transport.play();
    }
    device
        .start_output_stream(
            graph_source,
            timeline_source,
            SAMPLE_RATE,
            transport.clone(),
            // Also without `--midi-in`: the window's audition path — clicking a
            // key, drawing a note — is the same live channel a MIDI keyboard
            // arrives through (TDD §14.1), and it is what makes what you draw
            // audible while the transport is stopped.
            (midi_in || window).then_some(live_source),
        )
        .map_err(|e| format!("failed to open the default output device: {e}"))?;

    if window {
        // **Why the last run went away**, before anything can go wrong in this
        // one — see [`fontelle_app::crashlog`]. Here rather than at the top of
        // `main` because this is where the project's name is known and where
        // the interactive life of the process begins; a bounce or a `--help`
        // has a terminal to print to and needs no file.
        let crash_dir = fontelle_app::settings::Settings::data_dir();
        let crash_news = crash_dir
            .as_ref()
            .and_then(|dir| fontelle_app::crashlog::begin(dir, Some(&project.meta.name)).message());
        if let Some(said) = &crash_news {
            // On the terminal too, for whoever launched it from one.
            eprintln!("Fontelle: {said}");
        }
        // The window owns the transport from here. It opens *stopped* and cued
        // where `--start-beat` asked for: a DAW that starts playing the moment
        // it opens is a DAW you have to race to the stop button.
        transport.stop();
        transport.seek(start_sample);
        // How far the transport bar's ruler reaches. The *notes* are what a
        // bounce is as long as, but an empty project has none and a ruler that
        // is zero samples wide has nowhere to put a playhead — so the window
        // takes the clips into account as well.
        let arranged = project
            .clips
            .values()
            .map(|clip| clip.start + clip.length)
            .max()
            .unwrap_or(0);
        let window_length = song_end_samples.max(project.tempo_map.tick_to_sample(arranged));
        let host = fontelle_app::EngineHost::new(
            transport.clone(),
            master,
            project.tempo_map.clone(),
            window_length,
            SAMPLE_RATE,
        )
        // The same switch the graph is playing through, so the button on the
        // bar and the node in the schedule are one thing.
        .with_metronome(realised.metronome.clone());
        // The studio the window drives. It used to be built only when the
        // project had a note clip for the roll to open, and that gate is what
        // made the window open **empty** — no rack, no browser, no arrangement
        // — the day a new project stopped arriving with a clip in it
        // (`tests/starting_project.rs`). A roll with nothing open is an
        // ordinary state: every read of the open clip goes through
        // `project.clips.get`, which answers `None` for an id that names
        // nothing, and `Session::adopt` has always opened a clip-less
        // project this way.
        let clip = fontelle_app::Session::first_clip(&project).unwrap_or_default();
        let document = Some({
            let mut session = fontelle_app::Session::new(
                project.clone(),
                library,
                realised.channel_nodes.clone(),
                timeline_publisher,
                RealiseOptions {
                    sample_rate: SAMPLE_RATE,
                    block_size: BLOCK_SIZE,
                    quality,
                },
                clip,
                save.map(std::path::Path::to_path_buf),
            )
            // The instruments reach the running stream through their own
            // channel, so choosing a soundfont from inside the window does not
            // restart the audio device.
            .with_graphs(graph_publisher, realised.track_controls.clone())
            // The meters the first graph was built with, so a project opened
            // on a synth says how many voices it is playing from the first
            // frame rather than after the next rebuild.
            .with_voice_meters(realised.voice_meters.clone())
            .with_param_nodes(realised.param_nodes.clone())
            // The same transport the bar drives, so a time selection dragged
            // out on a ruler — and clip mode — reach the thing that loops
            // (TDD §6.3). The document holds the range in ticks; only this
            // side can turn it into the samples the RT thread needs.
            .with_transport(transport.clone())
            // The same switch `EngineHost` above was given, so the button on
            // the transport bar and the node in the schedule stay one thing
            // across every rebuild — see `Session::with_metronome`.
            .with_metronome(realised.metronome.clone())
            // And the cell the MIDI hub's routers read, so a keyboard plays
            // whichever instrument is selected — including after a rebuild has
            // renumbered every node.
            .with_live_target(std::sync::Arc::clone(&live_target))
            // And the cell those devices read their input settings out of, so
            // the settings tab changes how the keyboard already plugged in
            // reads without reopening a port.
            .with_input_settings(std::sync::Arc::clone(&live_input))
            // And the cell those devices light their keys in, so the roll's
            // keyboard shows what is being played on them.
            .with_live_keys(std::sync::Arc::clone(&live_keys))
            // And the ring the output stream is already reading, so choosing
            // an input on a mixer strip is heard through that strip.
            .with_monitor(std::sync::Arc::clone(&monitor));
            // The recording end of the live channel, so pressing record in the
            // window keeps a take the same way `--record` does.
            if let Some(reader) = capture.take() {
                session = session.with_capture(reader);
            }
            if let Some(port) = live_ports.claim() {
                session = session.with_audition(Box::new(port));
            }
            for dir in &soundfont_dirs {
                session.add_soundfont_dir(dir);
            }
            if let Some(created) = session.open_bank() {
                println!("  soundfont folder: {}", created.display());
            }
            // The machine searched for plugins **now**, while the studio is
            // opening, rather than the first time a menu wants the list. See
            // `Session::scan_plugins`: the wait exists either way, and this is
            // where a person expects one.
            let (found, failed) = session.scan_plugins();
            match (found, failed) {
                (0, 0) => {}
                (n, 0) => println!("  plugins: {n}"),
                (n, f) => println!("  plugins: {n} ({f} would not load)"),
            }
            // And the projects folder, if the settings name one. Nothing is
            // created and nothing is guessed at (INVARIANT 10): with no folder
            // configured the Projects tab says so and offers to pick one.
            session.open_projects();
            // The start menu's update check, if the settings allow one. Built
            // here rather than started: the window starts it when the menu
            // goes up, so a launch straight into a project asks nothing.
            if session.checks_for_updates() {
                session = session.with_updater(fontelle_app::updates::Updater::new());
            }
            // And what happened to the last run, in the window's own status
            // line: a crash report nobody is told about is a file nobody
            // reads.
            if let Some(said) = &crash_news {
                session.announce(said.clone());
            }
            Box::new(session) as Box<dyn fontelle_ui::StudioHost>
        });
        // Live MIDI, on its own thread, for as long as the window is open.
        // `midir` has no hot-plug notification on any backend, so polling is
        // the only mechanism there is — and it must not be on the UI thread,
        // which is blocked inside the event loop until the window closes.
        let midi = hub.take().map(|hub| watch_midi_devices(hub, live_ports));

        // So the compositor has a face to put on the window: Wayland finds
        // an icon by app id and a desktop entry, and a build run from
        // `cargo` has no entry until this writes one. See
        // `desktop::register_desktop_entry` for the INVARIANT 10 reading.
        #[cfg(target_os = "linux")]
        if let (Some(data), Ok(exe)) = (
            fontelle_app::settings::Settings::data_dir()
                .and_then(|d| d.parent().map(std::path::Path::to_path_buf)),
            std::env::current_exe(),
        ) {
            match fontelle_app::desktop::register_desktop_entry(
                &data,
                &exe,
                fontelle_ui::branding::window_icon_png(),
            ) {
                // Written, so the desktop that is already running is told —
                // or it keeps showing the placeholder it decided on before
                // the files existed. See `desktop::refresh_desktop`.
                Ok(true) => fontelle_app::desktop::refresh_desktop(&data),
                Ok(false) => {}
                Err(e) => println!("  ! desktop entry: {e}"),
            }
        }

        let result = fontelle_ui::run_window(fontelle_ui::WindowOptions {
            title: format!("{} — Fontelle", project.meta.name),
            panel_title: project.meta.name.clone(),
            theme,
            size: (1280, 720),
            run_for,
            host: Some(Box::new(host)),
            document,
            welcome,
            version: fontelle_app::updates::CURRENT.to_string(),
        });
        // The keyboards first: closing a device releases whatever it was
        // holding, and those note-offs have to go through a callback that is
        // still running. Afterwards they would land in a queue nobody drains,
        // and the last chord played would be the last thing the speakers heard.
        if let Some(midi) = midi {
            midi.stop();
        }
        // Then through the transport before the stream goes away, so the
        // callback cuts its voices and hands the device silence rather than a
        // buffer that stops mid-note.
        transport.stop();
        std::thread::sleep(std::time::Duration::from_millis(50));
        device.stop();
        // The window has closed under its own steam, so the next launch has
        // no news. A panic never reaches this line, which is exactly what
        // makes the marker's absence mean something.
        if let Some(dir) = &crash_dir {
            fontelle_app::crashlog::end(dir);
        }
        return match result {
            Ok(app) => {
                if run_for.is_some() {
                    println!("{} frames drawn", app.frames_drawn());
                }
                Ok(())
            }
            Err(e) => Err(e.to_string()),
        };
    }

    // A recording has to end somewhere the process can act on, because what
    // comes after it — turning the take into a clip, writing the project — has
    // to happen before anything exits. Ctrl-C cannot do that.
    let mut take_events: Vec<fontelle_types::TimedEvent> = Vec::new();
    let stop_at = record.then_some(match record_seconds {
        Some(seconds) => (seconds * SAMPLE_RATE as f64) as i64,
        None => song_end_samples,
    });
    if let Some(at) = stop_at {
        println!(
            "  recording {:.1}s — play now.",
            at as f64 / SAMPLE_RATE as f64
        );
    }

    match hub.as_mut() {
        Some(hub) => {
            // Live input keeps the process alive on its own terms: the point
            // of playing along is that you are still playing when the song
            // ends. Stop it from the keyboard's own terminal, with Ctrl-C.
            if stop_at.is_none() {
                println!("\n  live MIDI in — play; Ctrl-C to stop.");
            }
            follow_midi_devices(
                hub,
                &mut live_ports,
                &transport,
                finish,
                capture.as_mut().map(|capture| Recording {
                    capture,
                    events: &mut take_events,
                }),
                stop_at,
            );
        }
        None => wait_for_playback(&transport, duration, finish),
    }
    let recorded_until = transport.position_sample();

    // Before the stream goes away, so anything still held is released through
    // a callback that is still running. Closing the devices afterwards would
    // send those note-offs into a queue nobody drains.
    if let Some(hub) = hub.as_mut() {
        hub.shutdown();
        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    // Stopped through the transport first: the callback cuts the voices and
    // fills silence, so the last thing the device is handed is silence rather
    // than a block torn off mid-note. Then a moment for that block to reach
    // the speakers before the stream goes away.
    transport.stop();
    std::thread::sleep(std::time::Duration::from_millis(50));
    device.stop();
    println!(
        "  stopped at {:.2}s",
        transport.position_sample() as f64 / SAMPLE_RATE as f64
    );

    // The take, turned into a clip by one command — which is also what makes
    // it undoable the moment there is a UI to undo it from.
    if let Some(capture) = capture.as_mut() {
        capture.drain_into(&mut take_events);
        if capture.dropped() > 0 {
            // A recording that quietly lost notes is worse than one that
            // failed, so it is said out loud.
            println!(
                "  ! {} events were dropped — the take has holes in it",
                capture.dropped()
            );
        }
        keep_the_take(&mut project, &take_events, recorded_until)?;
    }

    if let Some(bundle) = save
        && record
    {
        // Saved again, now that the take is in it. The earlier save is what
        // guarantees a project exists even if the recording goes wrong.
        fontelle_app::save_project(&project, bundle)
            .map_err(|e| format!("failed to save {}: {e}", bundle.display()))?;
        println!("  saved {} with the take", bundle.display());
    }
    Ok(())
}

/// Turns a captured take into a clip on a lane of its own.
///
/// One `AddClip` command, per INVARIANT 9 — and because a take is exactly the
/// kind of thing somebody wants to press Ctrl+Z on.
fn keep_the_take(
    project: &mut fontelle_model::Project,
    events: &[fontelle_types::TimedEvent],
    end_sample: i64,
) -> Result<(), String> {
    use fontelle_model::{AddClip, Clip, ClipSource, Command, Lane, NoteData};

    let Some(channel) = project.channels.keys().next() else {
        return Err("there is no channel for the take to play on".into());
    };
    let source = fontelle_model::notes_from_capture(events, &project.tempo_map, 0, end_sample);
    let ClipSource::Notes(data) = source else {
        unreachable!("a capture is always a note clip")
    };
    if data.notes.is_empty() {
        println!("  (nothing was played, so no clip was added)");
        return Ok(());
    }
    let count = data.notes.len();
    let length = data
        .notes
        .values()
        .map(|n| n.start + n.length)
        .max()
        .unwrap_or(0);

    let lane = project.lanes.insert(Lane {
        name: format!("Take {}", project.lanes.len() + 1),
        height: 32.0,
        color: [0xd0, 0x7f, 0x4f, 0xff],
        muted: false,
        locked: false,
        order: 0,
    });
    AddClip::new(Clip {
        lane,
        start: 0,
        length,
        source: ClipSource::Notes(NoteData {
            channel,
            notes: data.notes,
        }),
        prefab_link: None,
        color: None,
        muted: false,
        loop_length: None,
    })
    .apply(project)
    .map_err(|e| format!("{e}"))?;
    println!("  recorded {count} note(s)");
    Ok(())
}

/// Where playback starts and what, if anything, it repeats — the command-line
/// half of the transport. Beats, because that is the unit a musician has in
/// mind; the tempo map turns them into the samples the engine runs on.
#[derive(Debug, Clone, Copy)]
struct Cue {
    start_beat: f64,
    loop_beats: Option<(f64, f64)>,
    repeat: u32,
}

impl Default for Cue {
    fn default() -> Self {
        Self {
            start_beat: 0.0,
            loop_beats: None,
            repeat: 2,
        }
    }
}

/// The voice context every live note carries.
///
/// Anything but the sequencer's, which uses a clip's own id. A shared context
/// would let the arrangement's note-off for the same key cut the note the
/// player is holding, and vice versa (TDD §11.4).
const LIVE_VOICE_CONTEXT: u32 = u32::MAX;

/// A running MIDI watcher, and the way to stop it.
struct MidiWatch {
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    thread: std::thread::JoinHandle<()>,
}

impl MidiWatch {
    /// Closes every device and waits for the thread to finish doing it.
    ///
    /// Joined rather than detached, and that is the whole point of the type:
    /// `MidiHub::shutdown` releases the notes each device is holding, and a
    /// caller that walked away without waiting would tear the audio stream
    /// down underneath those note-offs.
    fn stop(self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        self.thread.join().ok();
    }
}

/// The window's counterpart to [`follow_midi_devices`]: opens every keyboard
/// on the machine, keeps opening them as they are plugged in, and says so.
///
/// It carries none of the loop's transport business — a window has a transport
/// bar, and the playhead is not this thread's problem. What is left is the
/// half that actually makes a controller work: enumerate, open, and hand each
/// device a port into the live queue.
///
/// 500 ms, for the reason [`follow_midi_devices`] gives: slow enough to cost
/// nothing, fast enough that plugging a keyboard in feels immediate.
fn watch_midi_devices(
    mut hub: fontelle_midi::MidiHub,
    mut ports: fontelle_engine::LiveEventPorts,
) -> MidiWatch {
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = std::sync::Arc::clone(&stop);
    let thread = std::thread::spawn(move || {
        let mut announced = false;
        while !flag.load(std::sync::atomic::Ordering::Relaxed) {
            match hub.poll(|| {
                ports
                    .claim()
                    .map(|port| Box::new(port) as Box<dyn fontelle_types::EventSink>)
            }) {
                Ok(report) => {
                    for key in &report.connected {
                        println!("  + MIDI in: {}", key.0);
                    }
                    for key in &report.disconnected {
                        println!("  - MIDI in: {} (its notes released)", key.0);
                    }
                    for key in &report.failed {
                        println!("  ! MIDI in: {} could not be opened", key.0);
                    }
                    if !announced && hub.connected_devices().is_empty() {
                        println!("  (no MIDI inputs found — plug one in and it will be picked up)");
                        announced = true;
                    }
                    if !report.connected.is_empty() {
                        announced = false;
                    }
                }
                // Reported once and then kept trying: a backend that is
                // momentarily unavailable is not a reason to stop listening
                // for the rest of the session.
                Err(e) => {
                    if !announced {
                        eprintln!("  ! MIDI enumeration failed: {e}");
                        announced = true;
                    }
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
        // Before the caller tears the audio stream down, which is what it is
        // waiting on this thread for.
        hub.shutdown();
    });
    MidiWatch { stop, thread }
}

/// Keeps live MIDI going for as long as the process runs: polls for devices
/// coming and going, and reports each change.
///
/// Polling is not a choice — `midir` has no hot-plug notification on any of
/// its backends, so re-enumeration is the only mechanism there is. 500 ms is
/// slow enough to cost nothing and fast enough that plugging a keyboard in
/// feels immediate.
///
/// It also keeps playing the song underneath, honouring the same finish
/// condition the playback-only path uses — but it does not exit when the song
/// ends, because the point of live input is that you are still playing after
/// the arrangement stops.
fn follow_midi_devices(
    hub: &mut fontelle_midi::MidiHub,
    ports: &mut fontelle_engine::LiveEventPorts,
    transport: &fontelle_engine::Transport,
    finish: Finish,
    mut recording: Option<Recording<'_>>,
    stop_at: Option<i64>,
) {
    let mut announced = false;
    let mut passes = 1;
    let mut last_position = transport.position_sample();

    loop {
        match hub.poll(|| {
            ports
                .claim()
                .map(|port| Box::new(port) as Box<dyn fontelle_types::EventSink>)
        }) {
            Ok(report) => {
                for key in &report.connected {
                    println!("  + {}", key.0);
                }
                for key in &report.disconnected {
                    println!("  - {} (its notes released)", key.0);
                }
                for key in &report.failed {
                    println!("  ! {} could not be opened", key.0);
                }
                if !announced && hub.connected_devices().is_empty() {
                    println!("  (no MIDI inputs found — plug one in and it will be picked up)");
                    announced = true;
                }
            }
            Err(e) => {
                eprintln!("  ! MIDI enumeration failed: {e}");
                return;
            }
        }

        // A recording is emptied every pass, so the ring never fills while
        // somebody is playing a long take.
        if let Some(recording) = recording.as_mut() {
            recording.capture.drain_into(recording.events);
        }

        // A recording ends where it was told to, because what comes after it —
        // turning the take into a clip, writing the project — has to happen
        // before anything exits, and Ctrl-C cannot do that.
        let now = transport.position_sample();
        if let Some(at) = stop_at
            && now >= at
        {
            transport.stop();
            // A last pass for whatever the callback wrote between the stop and
            // here, including the note-offs a lifted key produced.
            std::thread::sleep(std::time::Duration::from_millis(50));
            if let Some(recording) = recording.as_mut() {
                recording.capture.drain_into(recording.events);
            }
            return;
        }

        // Not while recording. A take runs past the end of the arrangement as
        // a matter of course — that is what recording onto empty bars is — and
        // stopping the transport would freeze the playhead that `stop_at` is
        // watching, so the take would never end at all.
        if stop_at.is_none() {
            let finished = match finish {
                Finish::AtSample(target) => now >= target,
                Finish::LoopPasses(wanted) => {
                    if now < last_position {
                        passes += 1;
                    }
                    passes > wanted
                }
            };
            if finished && transport.is_playing() {
                transport.stop();
                println!("  (song finished — the keyboard is still live)");
            }
        }
        last_position = now;

        // Polled twice as often while recording: the deadline is only as
        // accurate as this interval, and a take that runs half a second long
        // has half a second of silence stitched onto the end of it.
        std::thread::sleep(std::time::Duration::from_millis(if stop_at.is_some() {
            50
        } else {
            500
        }));
    }
}

/// The recording side of the live loop: the ring to empty, and the take being
/// assembled out of it.
struct Recording<'a> {
    capture: &'a mut fontelle_engine::CaptureReader,
    events: &'a mut Vec<fontelle_types::TimedEvent>,
}

/// What playback is waiting for.
#[derive(Debug, Clone, Copy)]
enum Finish {
    /// The playhead reaching a point in the song.
    AtSample(i64),
    /// A number of passes through the loop, counted by watching the playhead
    /// wrap.
    LoopPasses(u32),
}

/// Waits for playback to finish, watching the **playhead** rather than the
/// clock.
///
/// Sleeping for the song's duration was close enough when a stream played once
/// from the top and stopped. It is not close enough now: it assumes the device
/// consumes audio at exactly the rate the arithmetic says, it cannot notice a
/// stream that died, and with a loop running it has nothing to count passes
/// with. `Transport::position_sample` is what the audio thread actually
/// reached, and a wrap is visible as the playhead moving backwards — which is
/// the only thing that can move it backwards while nothing is seeking.
///
/// The wall-clock timeout stays, generously padded, as the answer to "the
/// device never called back at all": without it a dead stream hangs the
/// process forever.
fn wait_for_playback(
    transport: &fontelle_engine::Transport,
    timeout: std::time::Duration,
    finish: Finish,
) {
    let deadline = std::time::Instant::now() + timeout + std::time::Duration::from_secs(2);
    let mut passes = 1;
    let mut last = transport.position_sample();

    while std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(10));
        let now = transport.position_sample();

        match finish {
            Finish::AtSample(target) => {
                if now >= target {
                    return;
                }
            }
            Finish::LoopPasses(wanted) => {
                if now < last {
                    passes += 1;
                    if passes > wanted {
                        return;
                    }
                }
            }
        }
        last = now;
    }
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

/// The window `fontelle` opens with no arguments at all.
///
/// The gate sentence in `docs/first-usable-plan.md` §3 starts "launch
/// `fontelle` with no arguments", and until item 9 this was the one thing it
/// could not do: the window opened, but with no audio device behind it and no
/// way to reach a soundfont from inside itself. Now it is the same path
/// `--play-sf2 --window --blank` takes, minus the soundfont — the browser is
/// where that comes from.
fn start_empty(playback: Playback<'_>) -> Result<(), String> {
    let project = fontelle_app::blank_project(8, BPM, SAMPLE_RATE);
    play_or_render(project, SampleLibrary::new(), playback)
}

/// What `--help` prints.
///
/// Written out by hand rather than generated, and kept beside `main` where the
/// flags are read: a usage message that drifts from the code is worse than
/// none, and the only thing that keeps this one honest is that it is the next
/// thing you see when you add a flag.
///
/// The window is the default. Everything here is for the cases where it is
/// not: bouncing, playing something through once, and the two flags that make
/// this program testable from a terminal.
const HELP: &str = "\
Fontelle — a digital audio workstation.

    fontelle [options] [project]

With no options at all it opens the start menu — your recent projects, a new
one, and whether there is a newer Fontelle — which is what it is for.
Everything below is for the times it is not.

Opening things
  --open <bundle>         Open a project bundle and play it through.
  --blank                 Start with an empty clip to draw in (needs --window).
  --window                Show the window even when a flag would have played
                          through and exited.

Playing something once
  --play-sf2 <file>       Play a soundfont's preset and exit.
  --preset <n>            Which preset of it, by index.
  --play-midi <file>      Play a .mid file through whatever is loaded.
  --play-flopsynth [name] Play the built-in synthesiser: the Init patch with
                          no name, that preset with one, and the whole bank
                          listed with `list`.
  --key <n>               Which note to play, as a MIDI number (60 is C4).
  --repeat <n>            How many notes.
  --start-beat <n>        Where in the project to start.
  --run-for <seconds>     How long to play before stopping.
  --loop                  Loop the project rather than playing it once.
  --gain-db <n>           Master trim, in decibels.

Bouncing
  --render-wav <file>     Render the project to a WAV file and exit.
  --save <bundle>         Write the project out after whatever else happened.

Live input
  --midi-in               Open a MIDI keyboard.
  --midi-all              Listen on every channel rather than the one set.
  --midi-channel <n>      Which channel, 1-16.
  --record                Arm recording.
  --record-seconds <n>    How long to record for.

Where things live
  --soundfonts <dir>      Use this soundfont folder for this run.
  --theme <file>          Load a theme file.
  --light                 Use the light theme.
  --no-menu               Skip the start menu and open straight into the
                          studio.
  --version               Print the version and exit.
  --help                  Print this.
";

fn main() {
    let args: Vec<String> = std::env::args().collect();
    // The previous binary an upgrade could not delete (`updates::install`)
    // goes now, on the launch after — quietly, because it is housekeeping.
    if let Ok(exe) = std::env::current_exe() {
        fontelle_app::updates::tidy(&exe);
    }
    // Before anything else opens a device or reads a file: somebody asking
    // what the flags are should not have a stream opened at them.
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print!("{HELP}");
        return;
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("fontelle {}", fontelle_app::updates::CURRENT);
        return;
    }
    let opening = args.iter().any(|a| a == "--open");
    let playing_sf2 = args.iter().any(|a| a == "--play-sf2");
    // `--play-flopsynth` on its own is the Init patch; with a name it is that
    // preset; with `list` it prints the bank and exits.
    let flopsynth = args
        .iter()
        .position(|a| a == "--play-flopsynth")
        .map(|i| args.get(i + 1).cloned().unwrap_or_default());

    // A bad path is ordinary user error, not a bug — report it and exit
    // non-zero rather than dumping a panic and a backtrace hint.
    let path = if playing_sf2 {
        match fontelle_app::resolve_sf2_path(&args, |p| p.exists()) {
            Ok(path) => path,
            Err(e) => {
                eprintln!("Fontelle: {e}");
                std::process::exit(1);
            }
        }
    } else {
        std::path::PathBuf::new()
    };

    let path_flag = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .map(std::path::PathBuf::from)
    };
    let numeric_flag = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .and_then(|v| v.parse::<usize>().ok())
    };
    let root_key = numeric_flag("--key").unwrap_or(60).min(127) as u8;
    let preset = numeric_flag("--preset").unwrap_or(0);

    let render_wav = path_flag("--render-wav");
    let save = path_flag("--save");
    let open = path_flag("--open");
    if opening && open.is_none() {
        eprintln!("Fontelle: --open takes the path of a .fontelle project folder");
        std::process::exit(1);
    }

    // Every `--soundfonts <dir>`, remembered in the settings file so it only
    // has to be said once (INVARIANT 10: it is the user who says where).
    let soundfont_dirs: Vec<std::path::PathBuf> = args
        .iter()
        .enumerate()
        .filter(|(_, a)| *a == "--soundfonts")
        .filter_map(|(i, _)| args.get(i + 1))
        .map(std::path::PathBuf::from)
        .collect();

    let midi_path = path_flag("--play-midi");
    // 1-based on the command line, 0-based in the file, because that is how
    // every DAW and every piece of MIDI documentation numbers them.
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

    let gain_db = args
        .iter()
        .position(|a| a == "--gain-db")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse::<f32>().ok())
        .unwrap_or(fontelle_app::DEMO_TRACK_GAIN_DB);

    let float_flag = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .and_then(|v| v.parse::<f64>().ok())
    };
    // `--loop <from>:<to>` in beats. A colon rather than two flags because a
    // loop is one range: half of it given and half defaulted is never what
    // anybody meant.
    let loop_beats = match args.iter().position(|a| a == "--loop") {
        Some(i) => match args.get(i + 1).and_then(|v| v.split_once(':')) {
            Some((from, to)) => match (from.parse::<f64>(), to.parse::<f64>()) {
                (Ok(from), Ok(to)) if to > from && from >= 0.0 => Some((from, to)),
                _ => {
                    eprintln!(
                        "Fontelle: --loop takes a beat range as <from>:<to>, ending after \
                         it starts — e.g. --loop 0:8 for the first two bars of 4/4"
                    );
                    std::process::exit(1);
                }
            },
            None => {
                eprintln!("Fontelle: --loop takes a beat range as <from>:<to>, e.g. 0:8");
                std::process::exit(1);
            }
        },
        None => None,
    };
    let cue = Cue {
        start_beat: float_flag("--start-beat").unwrap_or(0.0).max(0.0),
        loop_beats,
        repeat: numeric_flag("--repeat").unwrap_or(2).max(1) as u32,
    };

    let midi_in = args.iter().any(|a| a == "--midi-in");
    let record = args.iter().any(|a| a == "--record");
    let blank = args.iter().any(|a| a == "--blank");
    // With no project named at all, the window is the whole point: there is
    // nothing to render offline and nothing to play through.
    // `--play-flopsynth` is a project named on the command line, like the
    // other two: it plays straight through and exits unless `--window` is
    // asked for, which is what makes it usable for a bounce and for the walk
    // through the bank that §7.4's listening half needs.
    let headless_project = playing_sf2 || opening || flopsynth.is_some();
    let window = args.iter().any(|a| a == "--window") || !headless_project;

    if blank && !window {
        eprintln!(
            "Fontelle: --blank opens an empty clip to draw in, which needs \
             the window — add --window"
        );
        std::process::exit(1);
    }
    if window && render_wav.is_some() {
        eprintln!(
            "Fontelle: --window opens the transport for you to play with and \
             --render-wav is an offline bounce that exits when it is done; \
             pick one"
        );
        std::process::exit(1);
    }
    if window && record {
        eprintln!(
            "Fontelle: --record has to know when the take ends so it can be \
             written down, and the window has no record button yet. Record \
             headless with --record --save <project>, then --open it --window"
        );
        std::process::exit(1);
    }
    if midi_in && render_wav.is_some() {
        eprintln!(
            "Fontelle: --midi-in is live playing and --render-wav is an offline bounce; \
             record the take first (--record --save <project>), then --open it and \
             --render-wav that"
        );
        std::process::exit(1);
    }
    if record && !midi_in {
        eprintln!("Fontelle: --record needs --midi-in; there is nothing else to record");
        std::process::exit(1);
    }

    let theme = match theme_for(&args) {
        Ok(theme) => theme,
        Err(e) => {
            eprintln!("Fontelle: {e}");
            std::process::exit(1);
        }
    };

    let playback = Playback {
        render_wav: render_wav.as_deref(),
        save: save.as_deref(),
        gain_db,
        cue,
        midi_in,
        record,
        record_seconds: float_flag("--record-seconds"),
        window,
        theme,
        soundfont_dirs,
        run_for: float_flag("--run-for").map(std::time::Duration::from_secs_f64),
        // Only a plain launch: a project or a demo named on the command line
        // is a person who has already said where they are going.
        welcome: window && !headless_project && !blank && !args.iter().any(|a| a == "--no-menu"),
        announce: match (playing_sf2, opening) {
            (false, false) => Some(WELCOME.to_string()),
            (true, false) if midi.is_none() => Some(if blank {
                format!("  an empty 8 bars at {BPM} bpm.\n{WELCOME}")
            } else {
                format!(
                    "  root key {root_key} at {BPM} bpm — a root/third/fifth run, \
                     then the triad held."
                )
            }),
            _ => None,
        },
    };

    let result = match (&open, playing_sf2) {
        (Some(bundle), _) => open_and_play(bundle, playback),
        _ if flopsynth.is_some() => play_flopsynth(flopsynth.as_deref(), playback),
        (None, true) => play_sf2(
            &path,
            PlayOptions {
                blank,
                root_key,
                preset,
                midi,
                playback,
            },
        ),
        // No project named: the empty studio, which is the whole of item 9.
        (None, false) => start_empty(playback),
    };
    if let Err(e) = result {
        eprintln!("Fontelle: {e}");
        std::process::exit(1);
    }
}

/// One Flopsynth preset, on one channel, ready to play
/// (`docs/flopsynth-plan.md` §11).
///
/// **The listening half of the bank's gate.** `flopsynth_presets.rs` measures
/// that every preset sounds, fits inside full scale, sits within three
/// decibels of its neighbours and is measurably apart from every other preset
/// in its category — and §7.4 says in as many words that three numbers are
/// *necessary and not sufficient*. This is how somebody hears them.
fn play_flopsynth(name: Option<&str>, playback: Playback<'_>) -> Result<(), String> {
    use fontelle_core::flopsynth::presets::{FACTORY, FlopsynthCategory};

    let name = match name {
        None | Some("") => None,
        Some("list") => {
            println!("{} Flopsynth presets:\n", FACTORY.len());
            for category in FlopsynthCategory::ALL {
                let rows: Vec<&str> = FACTORY
                    .iter()
                    .filter(|row| row.category == category)
                    .map(|row| row.name)
                    .collect();
                println!("  {} ({})", category.label(), rows.len());
                for chunk in rows.chunks(4) {
                    println!("      {}", chunk.join(", "));
                }
            }
            println!("\n  --play-flopsynth \"<name>\" plays one; with no name you get Init.");
            return Ok(());
        }
        Some(name) => Some(name),
    };

    let project = fontelle_app::flopsynth_project(name, 8, BPM, SAMPLE_RATE)?;
    match name {
        Some(name) => println!("Flopsynth: {name}"),
        None => println!("Flopsynth: the Init patch"),
    }
    play_or_render(project, SampleLibrary::new(), playback)
}

/// What the window says it can do, once, on stdout.
///
/// Printed rather than shown because there is no help panel yet; the toolbar
/// carries the same set, which is the half that does not need reading.
const WELCOME: &str = "\
  Your soundfonts live in a folder Fontelle scans. \"Open folder\" at the
  bottom left shows you where that is (and makes it if it is not there yet);
  \"Change...\" picks a different one, Ctrl+click to add one alongside.
  Pick a soundfont in the browser, then a preset:
    click a preset  -> puts it on the selected channel
    Ctrl+click      -> puts it on a new one
  A new project opens on Flopsynth playing the bank's Grand Piano, with an
  empty arrangement: draw the first clip where you want it.
  \"+ Add instrument\" makes a channel of whichever kind you pick; duplicating
  one copies the instrument and its settings and nothing on the arrangement.
  Right-click a channel to open, rename, duplicate, clear or delete it, and a
  lane's name to add, rename, mute or delete a row of the arrangement.
  The Prefabs tab beside Instruments lists content you can draw in more than
  one place: \"+ Make prefab\" makes one, click it to edit it in the roll, and
  with it selected the draw tool puts a place for it on the arrangement.
  Editing any place, or the prefab itself, changes every place at once.
  Draw with the left mouse button, delete with the right; the drag that
  follows carries the note you just drew, and a note's right edge lengthens it.
  C is the cut tool, in the roll and on the arrangement alike: drag a line
  across and everything it crosses is cut where it crossed — a diagonal stroke
  staggers the cuts across a chord, and a looped clip stays looping on both
  sides of the cut.
  The arrangement has the same tools: P draws a clip where you click, E
  marquees, and Ctrl+drag marquees without leaving the draw tool.
  Arrow keys move the selection: Ctrl+up/down by an octave, Ctrl+left/right by
  a bar, Shift+left/right change the length, Shift+up/down the lane's value.
  Space plays.  Ctrl+Z/Y undo.  Ctrl+S saves.  Ctrl+C/X/V/B copy, cut, paste,
  duplicate.  P/B/E/D or 1-7 pick a tool.  S cycles snap; L the note property
  the lane shows, and its chip opens the whole list.  G cycles the onion skin.
  Alt drags off the grid; Shift keeps a drag to one axis; Ctrl+drag marquees.
  Drag the seams between the panels to resize them: down the side of the
  channel rack, across it above the soundfonts, and under the arrangement.
  Wheel scrolls the keys, Shift+wheel the song, Ctrl+wheel zooms time and
  Ctrl+Shift+wheel zooms pitch — both about the pointer.
  The tempo and the time signature are the two boxes after the position
  read-out: drag the tempo or roll the wheel over it, Shift for a fine step;
  click the signature or roll it to change the beats in a bar.
  The Mixer tab beside the piano roll has a fader, a pan and mute/solo per
  track, with the master pinned to the right; both controls snap to the
  middle, and the read-out under each strip says the level. Click a strip to
  select it, and the + past the last one adds another.
  The column beside the master is that strip's: where its output goes, the
  effects on it — add, reorder by the grip, switch out by the dot, delete by
  the cross — and its sends, each with a level you drag and a pre/post switch.
  Right-click **any** knob — a fader, a pan, an EQ band, a compressor's
  threshold, an instrument's cutoff or one of its oscillators — to make an
  automation lane for it; it lands on the arrangement as a curve you can open
  and draw in, starting at the value the knob is on.
  An EQ's window draws the spectrum arriving at it behind the curve, so you
  can see what you are shaping. Its band chips switch bands on; Delete removes
  the one that is selected; and the transport, undo and save work in there as
  they do everywhere else. Every other effect opens a panel of its own knobs,
  and right-clicking any of them makes its automation lane too.
  Plug a MIDI keyboard in and it plays whichever channel is selected.
  Hover anything for a moment and it says what it does.";

/// The theme the window opens with: a file if one was named, otherwise the
/// light or dark default (TDD §16.6).
fn theme_for(args: &[String]) -> Result<fontelle_ui::Theme, String> {
    if let Some(path) = args
        .iter()
        .position(|a| a == "--theme")
        .and_then(|i| args.get(i + 1))
    {
        return fontelle_ui::Theme::load_from_file(std::path::Path::new(path))
            .map_err(|e| e.to_string());
    }
    Ok(if args.iter().any(|a| a == "--light") {
        fontelle_ui::Theme::light_default()
    } else {
        fontelle_ui::Theme::dark_default()
    })
}
