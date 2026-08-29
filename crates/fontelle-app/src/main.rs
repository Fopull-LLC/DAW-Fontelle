use std::sync::Arc;

use fontelle_app::{RealiseOptions, SampleLibrary, demo_project, project_from_midi, realise};
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
    /// Printed once the graph is up, when there is something worth saying
    /// about how the project was built.
    announce: Option<String>,
}

fn play_sf2(path: &std::path::Path, options: PlayOptions<'_>) -> Result<(), String> {
    let PlayOptions {
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
            let mut project = demo_project(root_key, BPM, SAMPLE_RATE);
            let channel = project
                .channels
                .keys()
                .next()
                .expect("the demo project has one channel");
            fontelle_app::set_channel_patch(&mut project, channel, &patch, &library)
                .map_err(|e| format!("failed to store the imported patch: {e}"))?;
            project
        }
    };

    play_or_render(project, library, playback)
}

/// Opens a saved project and plays or renders it — the same path
/// `--play-sf2` takes once it has built one.
fn open_and_play(bundle: &std::path::Path, playback: Playback<'_>) -> Result<(), String> {
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
        announce,
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

    let mut realised = realise(
        &project,
        &library,
        RealiseOptions {
            sample_rate: SAMPLE_RATE,
            block_size: BLOCK_SIZE,
            quality,
        },
    )
    .map_err(|e| format!("{e}"))?;
    for (_, missing) in &realised.unresolved {
        println!("  ! layer {} has no audio", missing.layer);
    }

    let timeline = fontelle_sequencer::compile(&project, &realised.channel_nodes);

    // Written before anything is played: a bounce that takes two minutes
    // should not be standing between the user and their project being on
    // disk.
    if let Some(bundle) = save {
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
        // The same transport the device would be driven by, so a bounce of a
        // looped section is the section as it plays rather than a second code
        // path that has to be kept in step with the first.
        transport.set_state(fontelle_engine::TransportState::Rendering);
        let pcm = fontelle_app::render_offline_with_transport(
            &timeline,
            &mut realised.graph,
            duration_samples,
            &transport,
        );
        let reduction_db = realised.master.take_max_reduction_db();
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

    let graph = realised.graph;
    let mut device = AudioDevice::default_host();
    println!(
        "Fontelle: {} note events, {} on {:?}",
        timeline.events.len(),
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
    let mut capture = record.then(|| {
        let (writer, reader) =
            fontelle_engine::live_capture_channel(fontelle_engine::CAPTURE_CAPACITY);
        live_source.arm_capture(writer);
        reader
    });
    let mut hub = if midi_in {
        // The first part's instrument. There is no focus to follow yet
        // (TDD §14.3's default), and playing the first instrument in the song
        // is the answer that needs no UI.
        let target = project
            .channels
            .keys()
            .next()
            .and_then(|c| realised.channel_nodes.get(&c).copied())
            .unwrap_or_default();
        Some(fontelle_midi::MidiHub::new(fontelle_midi::RouteTo {
            node: target,
            // Distinct from anything the sequencer emits, so a sequenced
            // note-off cannot cut a note the player is holding (TDD §11.4).
            voice_context: LIVE_VOICE_CONTEXT,
        }))
    } else {
        None
    };

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
            graph,
            timeline,
            SAMPLE_RATE,
            transport.clone(),
            midi_in.then_some(live_source),
        )
        .map_err(|e| format!("failed to open the default output device: {e}"))?;

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

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let opening = args.iter().any(|a| a == "--open");
    if opening || args.iter().any(|a| a == "--play-sf2") {
        // A bad path is ordinary user error, not a bug — report it and exit
        // non-zero rather than dumping a panic and a backtrace hint.
        let path = if opening {
            std::path::PathBuf::new()
        } else {
            match fontelle_app::resolve_sf2_path(&args, |p| p.exists()) {
                Ok(path) => path,
                Err(e) => {
                    eprintln!("Fontelle: {e}");
                    std::process::exit(1);
                }
            }
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

        let midi_path = path_flag("--play-midi");
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

        let float_flag = |name: &str| {
            args.iter()
                .position(|a| a == name)
                .and_then(|i| args.get(i + 1))
                .and_then(|v| v.parse::<f64>().ok())
        };
        // `--loop <from>:<to>` in beats. A colon rather than two flags
        // because a loop is one range: half of it given and half defaulted is
        // never what anybody meant.
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

        let playback = Playback {
            render_wav: render_wav.as_deref(),
            save: save.as_deref(),
            gain_db,
            cue,
            midi_in,
            record,
            record_seconds: float_flag("--record-seconds"),
            announce: (midi.is_none() && !opening).then(|| {
                format!(
                    "  root key {root_key} at {BPM} bpm — a root/third/fifth run, \
                     then the triad held."
                )
            }),
        };

        let result = match &open {
            Some(bundle) => open_and_play(bundle, playback),
            None => play_sf2(
                &path,
                PlayOptions {
                    root_key,
                    preset,
                    midi,
                    playback,
                },
            ),
        };
        if let Err(e) = result {
            eprintln!("Fontelle: {e}");
            std::process::exit(1);
        }
        return;
    }

    // The full DAW window. Item 6 of `docs/first-usable-plan.md`: a window, a
    // wgpu surface, a vello scene and one themed panel — the walking skeleton
    // the transport bar (item 7), the piano roll (item 8) and the docked panel
    // set (item 9) grow inside. The audio path above is untouched by it.
    let theme = match theme_for(&args) {
        Ok(theme) => theme,
        Err(e) => {
            eprintln!("Fontelle: {e}");
            std::process::exit(1);
        }
    };
    // Seconds, then closes itself. There is no other way to run the window
    // unattended, and "it opened, drew, and then sat there drawing nothing" is
    // exactly the claim TDD §16.3 makes and §19 measures.
    let run_for = args
        .iter()
        .position(|a| a == "--run-for")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse::<f64>().ok())
        .map(std::time::Duration::from_secs_f64);

    match fontelle_ui::run_window(fontelle_ui::WindowOptions {
        title: "Fontelle".to_string(),
        panel_title: "Fontelle".to_string(),
        theme,
        size: (1280, 720),
        run_for,
    }) {
        Ok(app) => {
            if run_for.is_some() {
                println!("{} frames drawn", app.frames_drawn());
            }
        }
        Err(e) => {
            eprintln!("Fontelle: {e}");
            std::process::exit(1);
        }
    }
}

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
