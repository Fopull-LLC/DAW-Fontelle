//! A plugin somebody else wrote, as a node in the graph (TDD §8.4).
//!
//! Against the real `fontelle-testplug` bundle, across the real CLAP ABI —
//! see that crate for why a mock would have tested the mock.

use std::path::PathBuf;
use std::sync::Arc;

use fontelle_engine::{AudioNode, PluginNode, PluginRole, PrepareContext, ProcessContext};
use fontelle_engine::{TransportSnapshot, TransportState};
use fontelle_host::{HostedPlugin, PluginHost, ProcessorBay};
use fontelle_types::{EventPayload, NodeId, PluginKey, TimedEvent};

const GAIN: &str = "com.fopull.fontelle.testgain";
const SINE: &str = "com.fopull.fontelle.testsine";
const BLOCK: usize = 128;

fn bundle() -> PathBuf {
    let mut path = std::env::current_exe().unwrap();
    path.pop();
    path.pop();
    let built = path.join(if cfg!(target_os = "windows") {
        "fontelle_testplug.dll"
    } else if cfg!(target_os = "macos") {
        "libfontelle_testplug.dylib"
    } else {
        "libfontelle_testplug.so"
    });
    assert!(
        built.exists(),
        "{} is missing — run `cargo build -p fontelle-testplug`",
        built.display()
    );
    assert_bundle_is_fresh(&built);
    // Copied through a temporary and renamed, every time — see
    // `fontelle-host/tests/common`, which does the same for the same reasons.
    // Copied **once per test binary**, and through a rename. Once, because
    // tests inside a binary run in parallel and two of them writing the same
    // staging file is how a half-written bundle got loaded; through a rename,
    // because two test binaries run in parallel too and rename is atomic.
    static BUNDLE: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    BUNDLE
        .get_or_init(|| {
            let bundle = path.join("fontelle-testplug.clap");
            let staging = path.join(format!(
                "fontelle-testplug.{}.{:?}.tmp",
                std::process::id(),
                std::thread::current().id()
            ));
            if std::fs::copy(&built, &staging).is_ok() {
                let _ = std::fs::rename(&staging, &bundle);
            }
            let _ = std::fs::remove_file(&staging);
            bundle
        })
        .clone()
}

/// A plugin opened, activated and parked, with the node that will find it.
fn wire(id: &str, role: PluginRole) -> (PluginHost, HostedPlugin, Arc<ProcessorBay>, PluginNode) {
    wire_key(&PluginKey::clap(id), &bundle(), role, BLOCK)
}

/// [`wire`] for any format and block size — a pitch is counted in zero
/// crossings, and 128 frames of a 440 Hz sine hold two of them.
fn wire_key(
    key: &PluginKey,
    bundle: &std::path::Path,
    role: PluginRole,
    block: usize,
) -> (PluginHost, HostedPlugin, Arc<ProcessorBay>, PluginNode) {
    let mut host = PluginHost::new();
    let mut plugin = host.open(bundle, key).unwrap();
    let bay = Arc::new(ProcessorBay::new());
    bay.park(plugin.activate(48_000.0, block as u32).unwrap());
    let mut node = PluginNode::new(Arc::clone(&bay), Arc::clone(plugin.values()), role);
    // Prepared, as the graph prepares every node before its first block: an
    // instrument's scratch is sized here and nowhere else (INVARIANT 1).
    node.prepare(&PrepareContext {
        sample_rate: 48_000.0,
        max_block_size: block as u32,
    });
    (host, plugin, bay, node)
}

/// The `fontelle-testlv2` bundle, assembled beside the test binary the way
/// `fontelle-host/tests/common` assembles it, and for the same reasons.
/// Linux only, as LV2 hosting is.
#[cfg(target_os = "linux")]
fn lv2_bundle() -> PathBuf {
    let mut path = std::env::current_exe().unwrap();
    path.pop();
    path.pop();
    let built = path.join("libfontelle_testlv2.so");
    assert!(
        built.exists(),
        "{} is missing — run `cargo build -p fontelle-testlv2`",
        built.display()
    );
    static BUNDLE: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    BUNDLE
        .get_or_init(|| {
            let bundle = path.join("fontelle-testlv2.lv2");
            let staging = path.join(format!(
                "fontelle-testlv2.{}.{:?}.tmp",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&staging);
            std::fs::create_dir_all(&staging).unwrap();
            std::fs::copy(&built, staging.join(fontelle_testlv2::BINARY_NAME)).unwrap();
            std::fs::write(staging.join("manifest.ttl"), fontelle_testlv2::MANIFEST_TTL).unwrap();
            std::fs::write(staging.join("testlv2.ttl"), fontelle_testlv2::PLUGIN_TTL).unwrap();
            let _ = std::fs::remove_dir_all(&bundle);
            std::fs::rename(&staging, &bundle).unwrap();
            bundle
        })
        .clone()
}

fn note_on(key: u8) -> TimedEvent {
    TimedEvent {
        sample: 0,
        target: NodeId::default(),
        payload: EventPayload::NoteOn {
            key,
            velocity: 127,
            pan: 0,
            fine_pitch: 0,
            release: 0,
            mod_x: 0,
            mod_y: 0,
            voice_context: 0,
        },
    }
}

fn run<'a>(node: &mut dyn AudioNode, channels: &'a mut [&'a mut [f32]], events: &'a [TimedEvent]) {
    let frames = channels.iter().map(|c| c.len()).min().unwrap_or(0);
    let mut ctx = ProcessContext {
        inputs: &[],
        outputs: channels,
        all_events: events,
        live_events: &[],
        audio: &[],
        node: NodeId::default(),
        transport: TransportSnapshot {
            state: TransportState::Playing,
            position_sample: 0,
            bpm: 120.0,
        },
        sample_range: 0..frames as i64,
    };
    node.process(&mut ctx);
}

fn peak(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0f32, |m, s| m.max(s.abs()))
}

#[test]
fn an_insert_puts_the_bus_through_the_plugin() {
    let (_host, mut plugin, _bay, mut node) = wire(GAIN, PluginRole::Effect);
    plugin.set_param(0, 2.0);

    let mut left = vec![0.5f32; BLOCK];
    let mut right = vec![0.5f32; BLOCK];
    run(&mut node, &mut [&mut left, &mut right], &[]);

    assert!((left[0] - 1.0).abs() < 1e-5, "{}", left[0]);
    assert!((right[0] - 1.0).abs() < 1e-5, "{}", right[0]);
}

#[test]
fn a_bypassed_plugin_leaves_the_bus_exactly_as_it_arrived() {
    let (_host, mut plugin, _bay, node) = wire(GAIN, PluginRole::Effect);
    plugin.set_param(0, 4.0);
    let mut node = node.bypassed(true);

    let mut left = vec![0.25f32; BLOCK];
    let mut right = vec![0.25f32; BLOCK];
    run(&mut node, &mut [&mut left, &mut right], &[]);

    assert_eq!(left[0], 0.25);
}

#[test]
fn a_node_that_has_not_found_its_processor_yet_passes_signal_through() {
    // The gap between a graph swap and the next reclaim. A hole in the mix
    // would be worse than a few blocks of the dry signal.
    let mut host = PluginHost::new();
    let plugin = host.open(&bundle(), &PluginKey::clap(GAIN)).unwrap();
    let bay = Arc::new(ProcessorBay::new());
    let mut node = PluginNode::new(bay, Arc::clone(plugin.values()), PluginRole::Effect);

    let mut left = vec![0.75f32; BLOCK];
    let mut right = vec![0.75f32; BLOCK];
    run(&mut node, &mut [&mut left, &mut right], &[]);

    assert_eq!(left[0], 0.75);
}

#[test]
fn an_instrument_node_plays_the_notes_it_is_sent() {
    let (_host, _plugin, _bay, mut node) = wire(SINE, PluginRole::Instrument);
    let mut left = vec![0.0f32; BLOCK];
    let mut right = vec![0.0f32; BLOCK];

    run(&mut node, &mut [&mut left, &mut right], &[]);
    assert!(peak(&left) < 1e-6, "silent until it is played");

    let note = TimedEvent {
        sample: 0,
        target: NodeId::default(),
        payload: EventPayload::NoteOn {
            key: 69,
            velocity: 100,
            pan: 0,
            fine_pitch: 0,
            release: 0,
            mod_x: 0,
            mod_y: 0,
            voice_context: 0,
        },
    };
    run(&mut node, &mut [&mut left, &mut right], &[note]);
    assert!(peak(&left) > 0.1, "{}", peak(&left));
}

#[test]
fn an_instrument_node_hears_a_note_off() {
    let (_host, _plugin, _bay, mut node) = wire(SINE, PluginRole::Instrument);
    let mut left = vec![0.0f32; BLOCK];
    let mut right = vec![0.0f32; BLOCK];
    let on = TimedEvent {
        sample: 0,
        target: NodeId::default(),
        payload: EventPayload::NoteOn {
            key: 60,
            velocity: 127,
            pan: 0,
            fine_pitch: 0,
            release: 0,
            mod_x: 0,
            mod_y: 0,
            voice_context: 0,
        },
    };
    run(&mut node, &mut [&mut left, &mut right], &[on]);
    assert!(peak(&left) > 0.1);

    let off = TimedEvent {
        sample: 0,
        target: NodeId::default(),
        payload: EventPayload::NoteOff {
            key: 60,
            voice_context: 0,
        },
    };
    // A fresh bus, as the graph clears one before every block: a source
    // adds into it and does not write it.
    left.fill(0.0);
    right.fill(0.0);
    run(&mut node, &mut [&mut left, &mut right], &[off]);
    assert!(peak(&left) < 1e-6, "{}", peak(&left));
}

#[test]
fn a_note_is_played_where_in_the_block_it_happened() {
    let (_host, _plugin, _bay, mut node) = wire(SINE, PluginRole::Instrument);
    let mut left = vec![0.0f32; BLOCK];
    let mut right = vec![0.0f32; BLOCK];
    let note = TimedEvent {
        sample: 64,
        target: NodeId::default(),
        payload: EventPayload::NoteOn {
            key: 69,
            velocity: 127,
            pan: 0,
            fine_pitch: 0,
            release: 0,
            mod_x: 0,
            mod_y: 0,
            voice_context: 0,
        },
    };
    run(&mut node, &mut [&mut left, &mut right], &[note]);
    assert!(peak(&left[..64]) < 1e-6, "{}", peak(&left[..64]));
    assert!(peak(&left[64..]) > 0.1, "{}", peak(&left[64..]));
}

#[test]
fn automation_reaches_a_plugins_own_parameter() {
    // A lane is **normalised**, like every other lane in this program, and the
    // plugin's range is what turns half travel into what half travel means:
    // the test gain runs to four, so a lane at 0.5 is a gain of two.
    let (_host, _plugin, _bay, mut node) = wire(GAIN, PluginRole::Effect);
    let event = TimedEvent {
        sample: 0,
        target: NodeId::default(),
        payload: EventPayload::ParamValue {
            target: "mixer:1/insert[0]/param/0".into(),
            value: 0.5,
        },
    };
    let mut left = vec![0.25f32; BLOCK];
    let mut right = vec![0.25f32; BLOCK];
    run(&mut node, &mut [&mut left, &mut right], &[event]);
    assert!((left[0] - 0.5).abs() < 1e-5, "{}", left[0]);
}

#[test]
fn a_lane_at_the_bottom_of_its_travel_means_the_bottom_of_the_range() {
    let (_host, _plugin, _bay, mut node) = wire(GAIN, PluginRole::Effect);
    let event = TimedEvent {
        sample: 0,
        target: NodeId::default(),
        payload: EventPayload::ParamValue {
            target: "mixer:1/insert[0]/param/0".into(),
            value: 0.0,
        },
    };
    let mut left = vec![1.0f32; BLOCK];
    let mut right = vec![1.0f32; BLOCK];
    run(&mut node, &mut [&mut left, &mut right], &[event]);
    assert!(left[0].abs() < 1e-6, "{}", left[0]);
}

#[test]
fn an_address_naming_no_parameter_of_this_plugin_changes_nothing() {
    let (_host, mut plugin, _bay, mut node) = wire(GAIN, PluginRole::Effect);
    plugin.set_param(0, 2.0);
    let event = TimedEvent {
        sample: 0,
        target: NodeId::default(),
        payload: EventPayload::ParamValue {
            target: "mixer:1/insert[0]/param/8888".into(),
            value: 0.0,
        },
    };
    let mut left = vec![0.5f32; BLOCK];
    let mut right = vec![0.5f32; BLOCK];
    run(&mut node, &mut [&mut left, &mut right], &[event]);
    assert!((left[0] - 1.0).abs() < 1e-5, "{}", left[0]);
}

#[test]
fn a_retired_node_parks_its_processor_for_the_next_one() {
    let (_host, plugin, bay, mut node) = wire(GAIN, PluginRole::Effect);
    let mut left = vec![1.0f32; BLOCK];
    let mut right = vec![1.0f32; BLOCK];
    run(&mut node, &mut [&mut left, &mut right], &[]);
    assert!(!bay.is_parked(), "the node has it while it renders");

    drop(node);
    assert!(bay.is_parked(), "and gives it back when the graph is freed");

    // The next graph's node picks it up and carries on.
    let mut next = PluginNode::new(
        Arc::clone(&bay),
        Arc::clone(plugin.values()),
        PluginRole::Effect,
    );
    let mut left = vec![0.5f32; BLOCK];
    let mut right = vec![0.5f32; BLOCK];
    run(&mut next, &mut [&mut left, &mut right], &[]);
    assert!((left[0] - 0.5).abs() < 1e-5, "{}", left[0]);
}

#[test]
fn a_plugin_node_says_what_it_is_in_a_schedule() {
    let (_host, _plugin, _bay, node) = wire(GAIN, PluginRole::Effect);
    assert_eq!(node.debug_name(), "plugin-effect");
    let (_host, _plugin, _bay, node) = wire(SINE, PluginRole::Instrument);
    assert_eq!(node.debug_name(), "plugin-instrument");
}

#[test]
fn a_reset_cuts_what_the_plugin_was_playing() {
    let (_host, _plugin, _bay, mut node) = wire(SINE, PluginRole::Instrument);
    let mut left = vec![0.0f32; BLOCK];
    let mut right = vec![0.0f32; BLOCK];
    let note = TimedEvent {
        sample: 0,
        target: NodeId::default(),
        payload: EventPayload::NoteOn {
            key: 69,
            velocity: 127,
            pan: 0,
            fine_pitch: 0,
            release: 0,
            mod_x: 0,
            mod_y: 0,
            voice_context: 0,
        },
    };
    run(&mut node, &mut [&mut left, &mut right], &[note]);
    assert!(peak(&left) > 0.1);

    node.reset();
    // A fresh bus, as the graph clears one before every block.
    left.fill(0.0);
    right.fill(0.0);
    run(&mut node, &mut [&mut left, &mut right], &[]);
    assert!(peak(&left) < 1e-6, "{}", peak(&left));
}

/// Refuses to run against a bundle older than the source it was built from.
///
/// Worth its own check because the failure it prevents is deeply confusing:
/// depending on `fontelle-testplug` builds its **rlib**, and the `.clap` a
/// host test loads is the **cdylib**, which only a build of that package
/// itself produces. So `cargo test -p ...` alone can run new tests against an
/// old plugin and fail for reasons that are nowhere in the diff.
fn assert_bundle_is_fresh(built: &std::path::Path) {
    let Ok(binary) = built.metadata().and_then(|m| m.modified()) else {
        return;
    };
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../fontelle-testplug/src");
    let Ok(entries) = std::fs::read_dir(&source) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(changed) = entry.metadata().and_then(|m| m.modified()) else {
            continue;
        };
        assert!(
            changed <= binary,
            "{} is newer than the built plugin — run `cargo build -p fontelle-testplug`",
            entry.path().display()
        );
    }
}

// ------------------------------ sharing a bus with other instruments (2026-09-05)

/// A plugin instrument **adds** to its bus, as every source does.
///
/// > *"when adding a plugin instrument i cannot hear my other instruments
/// > anymore at the same time"*
///
/// The graph clears every bus once per block and each source adds into it —
/// that is what lets two channels share a track. A plugin node that wrote its
/// block over the bus silenced everything scheduled before it on the same
/// bus, which on a project whose channels all go to the master was every
/// other instrument.
#[test]
fn an_instrument_adds_to_a_bus_that_already_carries_another_instrument() {
    let (_host, _plugin, _bay, mut node) = wire(SINE, PluginRole::Instrument);
    // What a sampler scheduled earlier already put on the bus.
    let mut left = vec![0.25f32; BLOCK];
    let mut right = vec![0.25f32; BLOCK];
    run(&mut node, &mut [&mut left, &mut right], &[note_on(69)]);
    let max = left.iter().cloned().fold(f32::MIN, f32::max);
    let min = left.iter().cloned().fold(f32::MAX, f32::min);
    // The sine is half scale, placed centre at -3 dB: 0.354 either way.
    // Written over the bus it would run -0.354..0.354; added to it, it runs
    // -0.104..0.604.
    assert!(max > 0.5, "the quarter that was there is gone: max {max}");
    assert!(min > -0.2, "min {min}");
}

/// And the channel's own level and placement apply to **what the plugin
/// wrote**, not to whatever else was on the bus.
#[test]
fn a_plugin_channels_level_does_not_touch_the_other_instruments_on_its_bus() {
    let (_host, _plugin, _bay, node) = wire(SINE, PluginRole::Instrument);
    let mut node = node.on_channel(-60.0, 0.0);
    let mut left = vec![0.25f32; BLOCK];
    let mut right = vec![0.25f32; BLOCK];
    run(&mut node, &mut [&mut left, &mut right], &[note_on(69)]);
    let max = left.iter().cloned().fold(f32::MIN, f32::max);
    let min = left.iter().cloned().fold(f32::MAX, f32::min);
    assert!(
        min > 0.24 && max < 0.26,
        "the bus was scaled too: {min}..{max}"
    );
}

// ------------------------------------ lending the processor back (2026-09-05)

/// The audio thread hands the processor home when the bay asks, and takes it
/// back when it is parked again.
///
/// This is how an LV2 plugin's own state is saved while it is playing: the
/// state interface lives on the instance, the instance is in the processor,
/// and LV2 forbids calling `save` while `run` is executing — so the main
/// thread asks for the processor, the node parks it at the top of its next
/// block, and the blocks in between are silent. The silence is **bounded**:
/// one block to hand it over, however long the main thread holds it, and one
/// block to pick it up.
#[test]
fn a_node_hands_its_processor_back_when_the_bay_asks_and_carries_on_when_it_returns() {
    let (_host, _plugin, bay, mut node) = wire(SINE, PluginRole::Instrument);
    let mut left = vec![0.0f32; BLOCK];
    let mut right = vec![0.0f32; BLOCK];
    run(&mut node, &mut [&mut left, &mut right], &[note_on(69)]);
    assert!(peak(&left) > 0.1);
    assert!(!bay.is_parked(), "the node has it while it renders");

    bay.request_return();
    assert!(bay.wants_return());
    left.fill(0.0);
    run(&mut node, &mut [&mut left, &mut right], &[]);
    assert!(
        bay.is_parked(),
        "the node parked it at the top of the block"
    );
    assert!(peak(&left) < 1e-6, "and rendered silence for that block");

    let processor = bay
        .recall(std::time::Duration::from_millis(50))
        .expect("it is home");
    assert!(
        !bay.wants_return(),
        "a recall that was answered withdraws its request"
    );
    // The main thread has it: nothing renders while it does.
    run(&mut node, &mut [&mut left, &mut right], &[]);
    assert!(peak(&left) < 1e-6);

    bay.park(processor);
    run(&mut node, &mut [&mut left, &mut right], &[]);
    assert!(
        peak(&left) > 0.1,
        "the node took it back and the note it was holding is still sounding: {}",
        peak(&left)
    );
}

// -------------------------------------------- the external sidechain (2026-09-05)

/// A plugin insert given a key reads the tap the compiler wired, and hands
/// it to the plugin's sidechain port.
///
/// The same `KeyTap` a built-in compressor reads (`docs/effects-catalogue.md`
/// §2.1): the source track's node fills it earlier in the same block, and
/// this node copies it into a buffer of its own — sized in `prepare`, never
/// here — before the plugin runs.
#[test]
fn an_insert_with_a_key_hands_the_tap_to_the_plugins_sidechain() {
    let (_host, _plugin, _bay, node) = wire(GAIN, PluginRole::Effect);
    let tap = Arc::new(fontelle_engine::KeyTap::new(BLOCK));
    let mut node = node.with_key(Arc::clone(&tap));
    node.prepare(&PrepareContext {
        sample_rate: 48_000.0,
        max_block_size: BLOCK as u32,
    });

    // Before the source has ever run, the key is silence: the gain is heard
    // as it is.
    let mut left = vec![0.5f32; BLOCK];
    let mut right = vec![0.5f32; BLOCK];
    run(&mut node, &mut [&mut left, &mut right], &[]);
    assert!((left[0] - 0.5).abs() < 1e-5, "{}", left[0]);

    // The source wrote a full-scale block: the fixture ducks to nothing.
    let mut kick = vec![1.0f32; BLOCK];
    tap.write(&[&mut kick]);
    let mut left = vec![0.5f32; BLOCK];
    let mut right = vec![0.5f32; BLOCK];
    run(&mut node, &mut [&mut left, &mut right], &[]);
    assert!(left[0].abs() < 1e-5, "not ducked: {}", left[0]);
    assert!(right[0].abs() < 1e-5, "{}", right[0]);
}

/// Without a key the sidechain port is handed silence, and the bypass still
/// wins over both.
#[test]
fn a_keyed_insert_that_is_bypassed_leaves_the_bus_alone() {
    let (_host, _plugin, _bay, node) = wire(GAIN, PluginRole::Effect);
    let tap = Arc::new(fontelle_engine::KeyTap::new(BLOCK));
    let mut kick = vec![1.0f32; BLOCK];
    tap.write(&[&mut kick]);
    let mut node = node.with_key(tap).bypassed(true);
    node.prepare(&PrepareContext {
        sample_rate: 48_000.0,
        max_block_size: BLOCK as u32,
    });
    let mut left = vec![0.5f32; BLOCK];
    let mut right = vec![0.5f32; BLOCK];
    run(&mut node, &mut [&mut left, &mut right], &[]);
    assert_eq!(left[0], 0.5);
}

// ------------------------------------- the wheels reach the node (2026-09-05)

/// A controller event on the wire reaches the plugin, in the block it was
/// stamped for.
#[test]
fn a_controller_event_reaches_the_plugin() {
    let (_host, _plugin, _bay, mut node) = wire(SINE, PluginRole::Instrument);
    let mut left = vec![0.0f32; BLOCK];
    let mut right = vec![0.0f32; BLOCK];
    run(&mut node, &mut [&mut left, &mut right], &[note_on(69)]);
    assert!(peak(&left) > 0.1);

    let wheel = TimedEvent {
        sample: 0,
        target: NodeId::default(),
        payload: EventPayload::Controller {
            controller: 1,
            value: 0,
        },
    };
    left.fill(0.0);
    right.fill(0.0);
    run(&mut node, &mut [&mut left, &mut right], &[wheel]);
    assert!(peak(&left) < 1e-6, "the wheel at zero: {}", peak(&left));
}

#[test]
fn a_pitch_bend_and_a_pressure_event_reach_the_plugin() {
    let (_host, _plugin, _bay, mut node) = wire(SINE, PluginRole::Instrument);
    let mut left = vec![0.0f32; BLOCK];
    let mut right = vec![0.0f32; BLOCK];
    run(&mut node, &mut [&mut left, &mut right], &[note_on(69)]);
    let pressed = TimedEvent {
        sample: 0,
        target: NodeId::default(),
        payload: EventPayload::ChannelPressure { value: 127 },
    };
    left.fill(0.0);
    right.fill(0.0);
    run(&mut node, &mut [&mut left, &mut right], &[pressed]);
    assert!(
        peak(&left) < 1e-6,
        "full pressure ducks the fixture: {}",
        peak(&left)
    );

    let released = TimedEvent {
        sample: 0,
        target: NodeId::default(),
        payload: EventPayload::ChannelPressure { value: 0 },
    };
    let bend = TimedEvent {
        sample: 0,
        target: NodeId::default(),
        payload: EventPayload::PitchBend { value: 8191 },
    };
    left.fill(0.0);
    right.fill(0.0);
    run(&mut node, &mut [&mut left, &mut right], &[released, bend]);
    assert!(peak(&left) > 0.1, "{}", peak(&left));
}

// ------------------------------------------------- slides (2026-09-06)

/// Big enough to count a pitch in: 4096 frames of 440 Hz hold 75 crossings.
const LONG: usize = 4096;

fn crossings(samples: &[f32]) -> usize {
    samples
        .windows(2)
        .filter(|pair| (pair[0] >= 0.0) != (pair[1] >= 0.0))
        .count()
}

// Only the LV2 slide test lets a note go; see it for why that is Linux's.
#[cfg(target_os = "linux")]
fn note_off(key: u8) -> TimedEvent {
    TimedEvent {
        sample: 0,
        target: NodeId::default(),
        payload: EventPayload::NoteOff {
            key,
            voice_context: 0,
        },
    }
}

fn slide(key: u8, glide_samples: u32) -> TimedEvent {
    TimedEvent {
        sample: 0,
        target: NodeId::default(),
        payload: EventPayload::NoteSlide {
            key,
            glide_samples,
            voice_context: 0,
        },
    }
}

/// One block through an instrument node, counting the crossings it made.
fn block(node: &mut PluginNode, events: &[TimedEvent]) -> (usize, f32) {
    let mut left = vec![0.0f32; LONG];
    let mut right = vec![0.0f32; LONG];
    run(node, &mut [&mut left, &mut right], events);
    (crossings(&left), peak(&left))
}

/// A slide note bends what is sounding to its key and starts nothing — the
/// same thing `fontelle_core::Sampler::slide` does for a built-in
/// instrument, reaching a plugin as a per-note tuning. The node has to
/// remember which keys are sounding to do it, because a slide names only
/// the key it goes **to**.
#[test]
fn a_slide_bends_a_sounding_plugin_note_to_its_key() {
    let (_host, _plugin, _bay, mut node) = wire_key(
        &PluginKey::clap(SINE),
        &bundle(),
        PluginRole::Instrument,
        LONG,
    );
    let (unbent, _) = block(&mut node, &[note_on(69)]);
    let (octave, _) = block(&mut node, &[slide(81, 0)]);
    assert!(
        octave > unbent * 2 - 6 && octave < unbent * 2 + 6,
        "unbent {unbent}, an octave up {octave}"
    );
}

#[test]
fn a_slide_with_nothing_sounding_starts_no_note() {
    let (_host, _plugin, _bay, mut node) = wire_key(
        &PluginKey::clap(SINE),
        &bundle(),
        PluginRole::Instrument,
        LONG,
    );
    let (_, level) = block(&mut node, &[slide(81, 0)]);
    assert!(level < 1e-6, "{level}");
    // And a note after it sounds at its own pitch: the slide left nothing
    // behind to bend it with.
    let (unbent, _) = block(&mut node, &[note_on(69)]);
    let (_host2, _plugin2, _bay2, mut fresh) = wire_key(
        &PluginKey::clap(SINE),
        &bundle(),
        PluginRole::Instrument,
        LONG,
    );
    let (reference, _) = block(&mut fresh, &[note_on(69)]);
    assert!(unbent.abs_diff(reference) <= 1, "{unbent} vs {reference}");
}

/// A slide with a length glides over it, block by block — the pitch a
/// block after the slide is between the two keys, and after the length has
/// passed it is at the target. Block rate, the way the sampler's own glide
/// advances.
#[test]
fn a_slide_glides_over_its_length() {
    let (_host, _plugin, _bay, mut node) = wire_key(
        &PluginKey::clap(SINE),
        &bundle(),
        PluginRole::Instrument,
        LONG,
    );
    let (unbent, _) = block(&mut node, &[note_on(69)]);
    // Four blocks long: an octave in four steps of three semitones.
    let (at_start, _) = block(&mut node, &[slide(81, (LONG * 4) as u32)]);
    assert!(
        at_start.abs_diff(unbent) <= 1,
        "the slide begins where the note is: {at_start} vs {unbent}"
    );
    let (_, _) = block(&mut node, &[]);
    let (halfway, _) = block(&mut node, &[]);
    assert!(
        halfway > unbent + unbent / 4 && halfway < unbent * 2 - unbent / 4,
        "halfway through: unbent {unbent}, now {halfway}"
    );
    let (_, _) = block(&mut node, &[]);
    let (_, _) = block(&mut node, &[]);
    let (arrived, _) = block(&mut node, &[]);
    assert!(
        arrived > unbent * 2 - 6 && arrived < unbent * 2 + 6,
        "arrived: unbent {unbent}, now {arrived}"
    );
}

/// For a plugin that hears pitch as a **channel** bend — every LV2 one — a
/// slide is undone when the note it bent ends, so the next note on that
/// channel starts at its own pitch rather than the last slide's.
#[cfg(target_os = "linux")]
#[test]
fn a_slid_note_that_ends_leaves_the_next_one_unbent() {
    let (_host, _plugin, _bay, mut node) = wire_key(
        &PluginKey::new(
            fontelle_types::PluginFormat::Lv2,
            fontelle_testlv2::SINE_URI,
        ),
        &lv2_bundle(),
        PluginRole::Instrument,
        LONG,
    );
    let (unbent, _) = block(&mut node, &[note_on(69)]);
    let (bent, _) = block(&mut node, &[slide(71, 0)]);
    assert!(bent > unbent + 5, "unbent {unbent}, bent {bent}");
    let (_, _) = block(&mut node, &[note_off(69)]);
    let (again, _) = block(&mut node, &[note_on(69)]);
    assert!(
        again.abs_diff(unbent) <= 1,
        "the next note is unbent: {again} vs {unbent}"
    );
}
