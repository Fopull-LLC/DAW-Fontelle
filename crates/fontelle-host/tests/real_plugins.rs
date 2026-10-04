//! The installed plugins themselves, rather than the fixtures.
//!
//! Ignored, because what they find depends on what is installed:
//!
//! `cargo test -p fontelle-host --test real_plugins -- --ignored --nocapture`
//!
//! `FONTELLE_REAL_ONLY=OB-Xf` narrows it to names containing that.

use fontelle_host::{PluginHost, PluginScan};
use fontelle_types::PluginFormat;

/// Plugins that take the whole test binary down, left out unless asked for
/// by name. Odin2 (CLAP, 2.4) dereferences null inside its own `process` on
/// the first block — with a transport and every port handed over; not
/// understood yet (2026-09-29).
const CRASHES: &[&str] = &["Odin2"];

/// The folders the studio itself scans, so a plugin installed where a
/// distribution puts it (Fedora's `/usr/lib64/…`) is tried here too.
fn search_paths() -> Vec<std::path::PathBuf> {
    fontelle_host::search_paths()
}

fn peak(output: &[Vec<f32>]) -> f32 {
    output
        .iter()
        .flat_map(|c| c.iter())
        .fold(0.0f32, |a, b| a.max(b.abs()))
}

/// > *"i tried ob-xf and it was initially working but as soon as i tried
/// > actually encorperating it in my arrangement it would just stop
/// > producing sound or be doing pitch bends it wasnt doing before"*
///
/// The graph resets every node at a stop, a seek and each pass round a loop,
/// and a note still held there has its note-off on the far side of the cut.
/// CLAP's `reset` says it kills voices; a JUCE plugin's does not (JUCE's
/// `AudioProcessor::reset` is empty unless the plugin fills it), so each
/// held note was left sounding forever — until the voices ran out, or a new
/// note glided out of a stuck one. Whatever a plugin makes of its reset, a
/// reset processor is silent once its release has rung out.
#[test]
#[ignore]
fn every_installed_instrument_is_silent_after_a_reset() {
    let scan = PluginScan::of(&search_paths());
    let only = std::env::var("FONTELLE_REAL_ONLY").ok();
    let instruments: Vec<_> = scan
        .plugins
        .iter()
        .filter(|p| p.is_instrument() && p.key.format.hosted())
        .filter(|p| p.key.format != PluginFormat::Lv2 || only.is_some())
        .filter(|p| only.as_ref().is_none_or(|only| p.name.contains(only)))
        .filter(|p| only.is_some() || !CRASHES.iter().any(|name| p.name.contains(name)))
        .collect();
    assert!(!instruments.is_empty(), "no instruments installed");
    let mut host = PluginHost::new();
    let mut stuck = Vec::new();
    for info in instruments {
        let Ok(mut plugin) = host.open(&info.path, &info.key) else {
            continue;
        };
        let Ok(mut processor) = plugin.activate(48_000.0, 256) else {
            continue;
        };
        let mut output = vec![vec![0.0f32; 256]; plugin.audio_outputs().max(2) as usize];
        processor.note_on(0, 60, 0.8);
        let mut held = 0.0f32;
        for _ in 0..40 {
            processor.process_instrument(&mut output, 256);
            held = held.max(peak(&output));
        }
        if held < 1e-3 {
            eprintln!("{:<40} silent when played, skipped", info.name);
            continue;
        }
        processor.reset();
        // Four seconds for a release to ring out, then one more to listen.
        for _ in 0..750 {
            processor.process_instrument(&mut output, 256);
        }
        let mut after = 0.0f32;
        for _ in 0..190 {
            processor.process_instrument(&mut output, 256);
            after = after.max(peak(&output));
        }
        eprintln!("{:<40} held {held:.3}, after a reset {after:.4}", info.name);
        if after > held * 0.01 {
            stuck.push(info.name.clone());
        }
    }
    assert!(stuck.is_empty(), "still sounding after a reset: {stuck:?}");
}

/// Every installed plugin's own library, listed and one of it loaded: how
/// many, how long the listing took, and whether the plugin's state moved.
/// `FONTELLE_REAL_ONLY=OB-Xf` for one.
#[test]
#[ignore]
fn every_installed_plugins_own_presets_are_listed_and_load() {
    let scan = PluginScan::of(&search_paths());
    let only = std::env::var("FONTELLE_REAL_ONLY").ok();
    let roots = fontelle_host::PresetRoots::standard();
    let mut host = PluginHost::new();
    let mut refused = Vec::new();
    for info in scan
        .plugins
        .iter()
        .filter(|p| p.key.format.hosted())
        .filter(|p| only.as_ref().is_none_or(|only| p.name.contains(only)))
        .filter(|p| only.is_some() || !CRASHES.iter().any(|name| p.name.contains(name)))
    {
        // Named first, so a plugin that takes the binary down is the last
        // line printed.
        eprintln!("-> {} ({:?})", info.name, info.key.format);
        let started = std::time::Instant::now();
        let presets = host.own_presets(info, &roots);
        let took = started.elapsed();
        if presets.is_empty() {
            continue;
        }
        let Ok(mut plugin) = host.open(&info.path, &info.key) else {
            continue;
        };
        let Ok(mut processor) = plugin.activate(48_000.0, 256) else {
            continue;
        };
        let before = plugin.snapshot_with(&mut processor);
        let preset = &presets[presets.len() / 2];
        let loaded = if plugin.own_preset_needs_processor() {
            plugin.load_own_preset_with(&mut processor, preset)
        } else {
            plugin.load_own_preset(preset)
        };
        let moved = plugin.snapshot_with(&mut processor) != before;
        eprintln!(
            "{:<32} {:?} {:>5} presets in {:>6.0?}; loaded {:?} ({}): {:?}, state moved {moved}",
            info.name,
            info.key.format,
            presets.len(),
            took,
            preset.name,
            preset.category,
            loaded.as_ref().map(|_| "ok"),
        );
        if loaded.is_err() {
            refused.push(info.name.clone());
        }
        plugin.deactivate(processor);
    }
    assert!(
        refused.is_empty(),
        "would not load their own presets: {refused:?}"
    );
}

/// Set in the child: which plugin to soak, as `format|name`.
const SOAK_CHILD: &str = "FONTELLE_SOAK_ONE";

/// A session's worth of use, per installed instrument, each in a process of
/// its own so a plugin that crashes is named rather than taking the run down.
///
/// > *"instrument plugins now keep the preset saved when I log back in, But
/// > it might be worth looking into more advanced synths like vital, serum
/// > and surge as I'm still experiencing problems with those."*
///
/// What the studio does to a plugin, in the order it does it: chords with
/// the transport rolling, a bend and the mod wheel, blocks of every size the
/// graph hands out, the plugin's own preset, a reset at every loop seam, a
/// save and a reopen that must sound like what was saved, and a device that
/// changes its rate. `FONTELLE_REAL_ONLY=Surge` narrows it; `SOAK_NO_PRESET`
/// and `SOAK_NO_EDITS` leave those steps out, to tell which one a state that
/// did not come back went wrong in.
#[test]
#[ignore]
fn every_installed_instrument_survives_a_session() {
    if let Ok(which) = std::env::var(SOAK_CHILD) {
        soak(&which);
        return;
    }
    let scan = PluginScan::of(&search_paths());
    let only = std::env::var("FONTELLE_REAL_ONLY").ok();
    let mut failed = Vec::new();
    for info in scan
        .plugins
        .iter()
        .filter(|p| p.is_instrument() && p.key.format.hosted())
        .filter(|p| only.as_ref().is_none_or(|only| p.name.contains(only)))
    {
        let which = format!("{:?}|{}", info.key.format, info.name);
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "every_installed_instrument_survives_a_session",
                "--ignored",
                "--nocapture",
            ])
            .env(SOAK_CHILD, &which)
            .stdout(std::process::Stdio::null())
            .spawn()
            .expect("the child runs");
        // A plugin that hangs is as broken as one that crashes.
        let started = std::time::Instant::now();
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break Some(status);
            }
            if started.elapsed() > std::time::Duration::from_secs(120) {
                child.kill().ok();
                break None;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        };
        let verdict = match status {
            None => "HUNG".to_string(),
            Some(status) if status.success() => "ok".to_string(),
            Some(status) => crashed_by(&status).unwrap_or_else(|| "FAILED".to_string()),
        };
        eprintln!("== {which}: {verdict}");
        if verdict != "ok" {
            failed.push(format!("{which}: {verdict}"));
        }
    }
    assert!(failed.is_empty(), "{failed:#?}");
}

/// The signal a child died of, as a verdict — a crash rather than a failed
/// assertion. Windows has no signals; a crash there is an exit code, and a
/// test that failed is one too, so it says only "FAILED".
#[cfg(unix)]
fn crashed_by(status: &std::process::ExitStatus) -> Option<String> {
    use std::os::unix::process::ExitStatusExt;
    status
        .signal()
        .map(|signal| format!("CRASHED (signal {signal})"))
}

#[cfg(not(unix))]
fn crashed_by(_: &std::process::ExitStatus) -> Option<String> {
    None
}

fn rms(output: &[Vec<f32>], frames: usize) -> f32 {
    let (sum, n) = output.iter().fold((0.0f64, 0usize), |(s, n), c| {
        (
            s + c[..frames].iter().map(|x| (*x as f64).powi(2)).sum::<f64>(),
            n + frames,
        )
    });
    (sum / n.max(1) as f64).sqrt() as f32
}

/// Everything one block could be wrong about: not a number, or loud enough
/// to take a speaker with it.
fn check(output: &[Vec<f32>], frames: usize, when: &str) {
    for channel in output {
        for x in &channel[..frames] {
            assert!(x.is_finite(), "{when}: a sample that is not a number");
            assert!(
                x.abs() < 16.0,
                "{when}: a sample at {x}, +24 dB over full scale"
            );
        }
    }
}

/// Renders `seconds` of whatever is playing in blocks of the sizes the graph
/// hands out, the transport rolling. Returns the loudest block's RMS.
fn render(
    plugin: &mut fontelle_host::HostedPlugin,
    processor: &mut fontelle_host::HostedProcessor,
    output: &mut [Vec<f32>],
    transport: &mut fontelle_host::PluginTransport,
    rate: f64,
    seconds: f64,
    when: &str,
) -> f32 {
    const SIZES: [usize; 8] = [512, 1, 37, 256, 500, 64, 511, 128];
    let max = processor.max_block();
    let mut left = (seconds * rate) as usize;
    let mut loudest = 0.0f32;
    let mut i = 0;
    while left > 0 {
        let frames = SIZES[i % SIZES.len()].min(max).min(left);
        i += 1;
        // The studio's frame: the plugin's main-thread callback between
        // blocks, as `tick_plugin_editors` does.
        plugin.service_main_thread();
        processor.set_transport(transport);
        processor.process_instrument(output, frames);
        check(output, frames, when);
        loudest = loudest.max(rms(output, frames));
        left -= frames;
        transport.seconds += frames as f64 / rate;
        transport.beats += frames as f64 / rate * transport.tempo / 60.0;
        transport.bar_start_beats = (transport.beats / 4.0).floor() * 4.0;
        transport.bar_number = (transport.beats / 4.0) as i32;
    }
    loudest
}

/// One note from silence, for comparing two copies of the same sound:
/// its level, and how often it crosses zero (a crude stand-in for pitch and
/// brightness that a different patch will not match by accident).
fn fingerprint(
    plugin: &mut fontelle_host::HostedPlugin,
    processor: &mut fontelle_host::HostedProcessor,
    outputs: usize,
    rate: f64,
) -> (f32, f32) {
    let mut output = vec![vec![0.0f32; processor.max_block()]; outputs];
    let mut transport = fontelle_host::PluginTransport::default();
    processor.reset();
    render(
        plugin,
        processor,
        &mut output,
        &mut transport,
        rate,
        2.0,
        "ringing out",
    );
    let block = 256.min(processor.max_block());
    processor.note_on(0, 57, 0.8);
    let (mut sum, mut crossings, mut frames) = (0.0f64, 0usize, 0usize);
    for n in 0..((0.6 * rate) as usize / block) {
        if n == ((0.4 * rate) as usize / block) {
            processor.note_off(0, 57);
        }
        plugin.service_main_thread();
        processor.process_instrument(&mut output, block);
        check(&output, block, "fingerprint");
        let c = &output[0][..block];
        sum += c.iter().map(|x| (*x as f64).powi(2)).sum::<f64>();
        crossings += c
            .windows(2)
            .filter(|w| (w[0] < 0.0) != (w[1] < 0.0))
            .count();
        frames += block;
    }
    (
        (sum / frames as f64).sqrt() as f32,
        crossings as f32 / frames as f32,
    )
}

fn db(ratio: f32) -> f32 {
    20.0 * ratio.max(1e-9).log10()
}

fn soak(which: &str) {
    let (format, name) = which.split_once('|').unwrap();
    let scan = PluginScan::of(&search_paths());
    let info = scan
        .plugins
        .iter()
        .find(|p| format!("{:?}", p.key.format) == format && p.name == name)
        .expect("the plugin is still installed");
    let mut host = PluginHost::new();
    let rate = 48_000.0;
    let mut plugin = host.open(&info.path, &info.key).expect("it opens");
    let outputs = plugin.audio_outputs().max(2) as usize;
    let mut processor = plugin.activate(rate, 512).expect("it activates");
    let mut output = vec![vec![0.0f32; 512]; outputs];
    let mut transport = fontelle_host::PluginTransport {
        playing: true,
        ..Default::default()
    };

    // A chord, bent and modulated, through every block size.
    for key in [48, 60, 64, 67] {
        processor.note_on(0, key, 0.8);
    }
    let mut loudest = render(
        &mut plugin,
        &mut processor,
        &mut output,
        &mut transport,
        rate,
        0.5,
        "a chord",
    );
    for step in 0..8 {
        processor.pitch_bend(0, (step * 1000) as i16);
        processor.controller(0, 1, (step * 16) as u8);
        loudest = loudest.max(render(
            &mut plugin,
            &mut processor,
            &mut output,
            &mut transport,
            rate,
            0.1,
            "a bend",
        ));
    }
    processor.pitch_bend(0, 0);
    processor.controller(0, 1, 0);
    for key in [48, 60, 64, 67] {
        processor.note_off(0, key);
    }
    render(
        &mut plugin,
        &mut processor,
        &mut output,
        &mut transport,
        rate,
        1.0,
        "a release",
    );
    eprintln!("SOAK {which}: chord {:.1} dBFS rms", db(loudest));
    assert!(loudest > 1e-4, "silent when played");

    // A loop, sixteen times round: a note held across the seam each time.
    for pass in 0..16 {
        processor.note_on(0, 60 + (pass % 5) as u8, 0.7);
        render(
            &mut plugin,
            &mut processor,
            &mut output,
            &mut transport,
            rate,
            0.2,
            "a loop",
        );
        processor.reset();
        transport.beats = 0.0;
        transport.seconds = 0.0;
    }
    render(
        &mut plugin,
        &mut processor,
        &mut output,
        &mut transport,
        rate,
        4.0,
        "after the loop",
    );
    let after = render(
        &mut plugin,
        &mut processor,
        &mut output,
        &mut transport,
        rate,
        1.0,
        "after the loop",
    );
    eprintln!(
        "SOAK {which}: after sixteen loop seams {:.1} dBFS",
        db(after)
    );
    assert!(after < loudest * 0.01, "notes stuck after the loop seams");

    // The plugin's own preset, if it has a library we read.
    let init = fingerprint(&mut plugin, &mut processor, outputs, rate);
    let presets = host.own_presets(info, &fontelle_host::PresetRoots::standard());
    let mut chosen: Option<String> = None;
    let named = |state: &fontelle_types::PluginState, name: &str| {
        state
            .blob
            .as_deref()
            .and_then(fontelle_types::decode_base64)
            .is_some_and(|bytes| {
                !name.is_empty() && bytes.windows(name.len()).any(|w| w == name.as_bytes())
            })
    };
    if let Some(preset) = presets
        .get(presets.len() / 3)
        .filter(|_| std::env::var_os("SOAK_NO_PRESET").is_none())
    {
        // **Deactivated**, except where the format needs it running. This
        // test is one thread; the studio is two. Surge XT decides how to
        // load a patch by which thread asks, and loaded from the thread that
        // also runs its blocks — but outside a block — it went silent for
        // good. From the studio's window thread, with the audio on its own,
        // every preset loaded and played (2026-09-30).
        // The studio's way (`PluginRack::load_own_preset`): with the
        // processor in hand, then run until the preset is in. A JUCE VST 3
        // (Surge's) takes it a few blocks later and its controller reports
        // the patch before until it is told again; read straight away, the
        // knobs saved beside the new patch were the old one's.
        let before = plugin.settle_mark(&mut processor);
        let loaded = plugin.load_own_preset_with(&mut processor, preset);
        if loaded.is_ok() {
            plugin.settle_with(&mut processor, &before);
        }
        eprintln!(
            "SOAK {which}: preset {:?} ({}): {}",
            preset.name,
            preset.category,
            loaded.is_ok()
        );
        chosen = Some(preset.name.clone());
    }
    // Moved off its defaults the way a user would, too: a third of the way
    // along its first dozen parameters.
    let edits = if std::env::var_os("SOAK_NO_EDITS").is_some() {
        0
    } else {
        12
    };
    let ids: Vec<u32> = plugin.params().iter().take(edits).map(|p| p.id).collect();
    for id in ids {
        if let Some(param) = plugin.param(id) {
            let value = param.min + (param.max - param.min) * 0.33;
            plugin.set_param(id, value);
        }
    }
    let saved_sound = fingerprint(&mut plugin, &mut processor, outputs, rate);
    // The same note again, nothing changed: a plugin whose second note is
    // not its first (MDA JX10 — free-running LFOs and voice phases: 9 dB
    // apart) cannot be held to "reopened, it sounds like what was saved".
    let again = fingerprint(&mut plugin, &mut processor, outputs, rate);
    let repeatable = (db(again.0) - db(saved_sound.0)).abs() < 3.0;
    if !repeatable {
        eprintln!(
            "SOAK {which}: two notes in a row are {:.1} dB apart; not compared by ear",
            (db(again.0) - db(saved_sound.0)).abs()
        );
    }
    let state = plugin.snapshot_with(&mut processor);
    eprintln!(
        "SOAK {which}: init {:.1} dB / {:.4}, edited {:.1} dB / {:.4}, state {} bytes",
        db(init.0),
        init.1,
        db(saved_sound.0),
        saved_sound.1,
        state.blob.as_ref().map_or(0, |b| b.len())
    );
    plugin.deactivate(processor);
    drop(plugin);

    // Saved, closed, reopened: it must sound like what was saved.
    let mut plugin = host.open(&info.path, &info.key).expect("it opens again");
    assert!(plugin.restore(&state), "the state is its own");
    let mut processor = plugin.activate(rate, 512).expect("it activates again");
    let reopened = fingerprint(&mut plugin, &mut processor, outputs, rate);
    // What the plugin says it is now, against what was saved — which half of
    // a state that did not come back is the one that went missing.
    let now = plugin.snapshot_with(&mut processor);
    if let Some(name) = &chosen {
        eprintln!(
            "SOAK {which}: preset's name in the saved state {}, in the reopened one {}",
            named(&state, name),
            named(&now, name)
        );
    }
    let drifted: Vec<String> = state
        .params
        .iter()
        .filter_map(|saved| {
            let now = now.params.iter().find(|p| p.id == saved.id)?;
            ((now.value - saved.value).abs() > 1e-4 * (1.0 + saved.value.abs())).then(|| {
                let name = plugin.param(saved.id).map_or("?", |p| p.name.as_str());
                format!(
                    "{} {name:?}: saved {:.4}, now {:.4}",
                    saved.id, saved.value, now.value
                )
            })
        })
        .collect();
    eprintln!(
        "SOAK {which}: {} of {} parameters drifted, blob {}",
        drifted.len(),
        state.params.len(),
        if now.blob == state.blob {
            "identical"
        } else {
            "different"
        }
    );
    for line in drifted.iter().take(12) {
        eprintln!("SOAK {which}:   {line}");
    }
    eprintln!(
        "SOAK {which}: reopened {:.1} dB / {:.4}",
        db(reopened.0),
        reopened.1
    );
    let level = (db(reopened.0) - db(saved_sound.0)).abs();
    let bright = (reopened.1 - saved_sound.1).abs() / saved_sound.1.max(1e-4);
    assert!(
        drifted.is_empty(),
        "reopened, its parameters are not what was saved"
    );
    // Compared by ear only where the saved patch sounds for the one short
    // note the fingerprint plays: a slow pad, a patch that wants a chord, or
    // one this single-threaded test left silent (see the preset step) says
    // nothing about whether the state came back.
    assert!(
        !repeatable
            // Below -60 dB a note's zero crossings are mostly its noise
            // (Surge's X-Fade Ensemble, with the soak's edits, plays at -70).
            || saved_sound.0 < 1e-3
            // Wide on brightness: the same state rendered twice is up to
            // 28 % apart here (free-running oscillators and noise).
            || (level < 3.0 && bright < 0.5),
        "reopened, it does not sound like what was saved: {level:.1} dB apart, \
         zero crossings {:.0}% apart",
        bright * 100.0
    );

    // The device changes rate and block size under it.
    plugin.deactivate(processor);
    let rate = 44_100.0;
    let mut processor = plugin
        .activate(rate, 1024)
        .expect("it activates at 44.1 kHz");
    let mut output = vec![vec![0.0f32; 1024]; outputs];
    processor.note_on(0, 60, 0.8);
    let again = render(
        &mut plugin,
        &mut processor,
        &mut output,
        &mut transport,
        rate,
        0.5,
        "at 44.1 kHz",
    );
    processor.note_off(0, 60);
    render(
        &mut plugin,
        &mut processor,
        &mut output,
        &mut transport,
        rate,
        0.5,
        "at 44.1 kHz",
    );
    eprintln!("SOAK {which}: at 44.1 kHz {:.1} dBFS", db(again));
    assert!(
        again > 1e-4 || reopened.0 < 1e-4,
        "silent after the device changed rate"
    );
    plugin.deactivate(processor);
}
