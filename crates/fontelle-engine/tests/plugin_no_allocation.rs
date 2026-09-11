//! Does hosting a plugin allocate on the audio thread (INVARIANT 1)?
//!
//! Its own binary, because a `#[global_allocator]` is per binary and
//! `no_allocation_during_render.rs` already owns one.
//!
//! **What this can and cannot promise.** Fontelle's half of a hosted block —
//! the copy in, the parameter drain, the event list, the port tables, the copy
//! out — is ours and must allocate nothing; every buffer it needs is taken in
//! `HostedPlugin::activate`. The plugin's half is foreign code bound by CLAP's
//! contract rather than by ours, and no host can promise anything about it. So
//! this test runs against `fontelle-testplug`, which allocates nothing in
//! `process`, and what it therefore measures is **our** half plus `clack`'s.
//! A real plugin that allocates is not a bug in this code and this test would
//! not see it.

use std::path::PathBuf;
use std::sync::Arc;

use fontelle_engine::{
    AudioNode, PluginNode, PluginRole, ProcessContext, RtGuardAllocator, TransportSnapshot,
    TransportState,
};
use fontelle_host::{PluginHost, ProcessorBay};
use fontelle_types::{EventPayload, NodeId, PluginKey, TimedEvent};

#[global_allocator]
static ALLOCATOR: RtGuardAllocator = RtGuardAllocator;

const BLOCK: usize = 128;
const BLOCKS: usize = 200;

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
    // Once per test binary, through a rename — the three tests here run on
    // three threads, and three copies into one staging file at once was a
    // truncated bundle on macOS. See `plugin_nodes.rs`, which does the same.
    static BUNDLE: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    BUNDLE
        .get_or_init(|| {
            let bundle = path.join("fontelle-testplug.clap");
            let staging = path.join(format!(
                "fontelle-testplug.alloc.{}.tmp",
                std::process::id()
            ));
            if std::fs::copy(&built, &staging).is_ok() {
                let _ = std::fs::rename(&staging, &bundle);
            }
            let _ = std::fs::remove_file(&staging);
            bundle
        })
        .clone()
}

fn note_on(sample: i64, key: u8) -> TimedEvent {
    TimedEvent {
        sample,
        target: NodeId::default(),
        payload: EventPayload::NoteOn {
            key,
            velocity: 100,
            pan: 0,
            fine_pitch: 0,
            release: 0,
            mod_x: 0,
            mod_y: 0,
            voice_context: 0,
        },
    }
}

fn run_blocks(node: &mut PluginNode, events: &[TimedEvent]) {
    let mut left = vec![0.5f32; BLOCK];
    let mut right = vec![0.5f32; BLOCK];

    // Everything above here is off-RT setup and exempt. From here on, this is
    // the audio callback.
    fontelle_engine::mark_current_thread_rt();
    for block in 0..BLOCKS {
        let start = (block * BLOCK) as i64;
        let mut channels: [&mut [f32]; 2] = [&mut left, &mut right];
        let mut ctx = ProcessContext {
            inputs: &[],
            outputs: &mut channels,
            all_events: if block == 0 { events } else { &[] },
            live_events: &[],
            audio: &[],
            node: NodeId::default(),
            transport: TransportSnapshot {
                state: TransportState::Playing,
                position_sample: start,
                bpm: fontelle_types::DEFAULT_BPM,
            },
            sample_range: start..start + BLOCK as i64,
        };
        node.process(&mut ctx);
    }
    fontelle_engine::unmark_current_thread_rt();
}

#[test]
fn an_insert_plugin_does_not_allocate_per_block() {
    let mut host = PluginHost::new();
    let mut plugin = host
        .open(&bundle(), &PluginKey::clap("com.fopull.fontelle.testgain"))
        .unwrap();
    let bay = Arc::new(ProcessorBay::new());
    bay.park(plugin.activate(48_000.0, BLOCK as u32).unwrap());
    let mut node = PluginNode::new(bay, Arc::clone(plugin.values()), PluginRole::Effect);

    run_blocks(&mut node, &[]);
}

#[test]
fn an_instrument_plugin_does_not_allocate_per_block_while_it_is_playing() {
    let mut host = PluginHost::new();
    let mut plugin = host
        .open(&bundle(), &PluginKey::clap("com.fopull.fontelle.testsine"))
        .unwrap();
    let bay = Arc::new(ProcessorBay::new());
    bay.park(plugin.activate(48_000.0, BLOCK as u32).unwrap());
    let mut node = PluginNode::new(bay, Arc::clone(plugin.values()), PluginRole::Instrument);

    // A note on the first block, so the plugin is sounding for the rest of the
    // run rather than idling through it.
    run_blocks(&mut node, &[note_on(0, 69)]);
}

#[test]
fn a_parameter_moving_every_block_does_not_allocate() {
    // The path a knob being dragged takes: the wire is drained and turned into
    // CLAP events at the top of every block. The obvious spelling of that
    // collects the drain into a `Vec`, which is one allocation per block on
    // the audio thread — this is the test that says so.
    let mut host = PluginHost::new();
    let mut plugin = host
        .open(&bundle(), &PluginKey::clap("com.fopull.fontelle.testgain"))
        .unwrap();
    let bay = Arc::new(ProcessorBay::new());
    bay.park(plugin.activate(48_000.0, BLOCK as u32).unwrap());
    let values = Arc::clone(plugin.values());
    let mut node = PluginNode::new(bay, values.clone(), PluginRole::Effect);

    let mut left = vec![0.5f32; BLOCK];
    let mut right = vec![0.5f32; BLOCK];
    fontelle_engine::mark_current_thread_rt();
    for block in 0..BLOCKS {
        // The knob moves between blocks, as a drag does.
        values.set(0, (block % 4) as f64 * 0.5);
        let start = (block * BLOCK) as i64;
        let mut channels: [&mut [f32]; 2] = [&mut left, &mut right];
        let mut ctx = ProcessContext {
            inputs: &[],
            outputs: &mut channels,
            all_events: &[],
            live_events: &[],
            audio: &[],
            node: NodeId::default(),
            transport: TransportSnapshot {
                state: TransportState::Playing,
                position_sample: start,
                bpm: fontelle_types::DEFAULT_BPM,
            },
            sample_range: start..start + BLOCK as i64,
        };
        node.process(&mut ctx);
    }
    fontelle_engine::unmark_current_thread_rt();
}
