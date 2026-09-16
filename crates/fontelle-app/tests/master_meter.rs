//! The master meter on the transport bar, across a graph rebuild.
//!
//! Reported from using the window: *"at the top right theres an audio monitor
//! but it doesnt work correctly. it seems to show sometimes but not always ...
//! it just doesnt render anything sometimes even when the master track is
//! clearly playing stuff."*
//!
//! The same wire the metronome switch was once broken on (`tests/click.rs`):
//! `realise` minted a fresh `MasterMeter` for every graph, and the window's
//! `EngineHost` went on reading the one the *first* graph published into. So
//! the bar's meter worked until the first thing that rebuilt the graph — a
//! new channel, an insert, a soundfont chosen — and then read silence for the
//! rest of the session, while the mixer's master strip, whose `TrackControls`
//! are kept across rebuilds, went on working. "Sometimes but not always" was
//! exactly that.
//!
//! The fix is the metronome's: the session keeps the meter and hands it back
//! to every rebuild, so the node in the schedule and the meter on the bar are
//! one thing for the life of the window.

mod common;

use std::path::PathBuf;
use std::sync::Arc;

use fontelle_app::{
    KeptTaps, RealiseOptions, SampleLibrary, Session, demo_project, realise, realise_hosting,
};
use fontelle_engine::{BLOCK_SIZE, TransportSnapshot, TransportState};
use fontelle_types::CompiledTimeline;
use fontelle_ui::document::StudioHost;

use common::SR;

fn options() -> RealiseOptions {
    RealiseOptions {
        sample_rate: SR,
        block_size: BLOCK_SIZE,
        quality: fontelle_app::PLAYBACK_QUALITY,
    }
}

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "fontelle-master-meter-{name}-{}",
        std::process::id()
    ));
    std::fs::remove_dir_all(&path).ok();
    std::fs::create_dir_all(&path).expect("the scratch folder must be creatable");
    path
}

#[test]
fn a_rebuild_handed_the_kept_meter_publishes_into_it_rather_than_a_new_one() {
    // The realise-level half: a meter passed in through `KeptTaps` is the one
    // the new graph's master node writes to.
    let project = demo_project(60, 120.0, SR);
    let library = SampleLibrary::new();
    let first = realise(&project, &library, options()).expect("the demo must realise");
    let kept = Arc::clone(&first.master);

    let again = realise_hosting(
        &project,
        &library,
        options(),
        &first.track_controls,
        Some(first.metronome.clone()),
        &KeptTaps {
            master: Some(Arc::clone(&kept)),
            ..KeptTaps::default()
        },
        None,
        None,
        &Default::default(),
    )
    .expect("the demo must realise again");
    assert!(
        Arc::ptr_eq(&again.master, &kept),
        "a rebuild minted a new master meter instead of keeping the one it was handed"
    );
}

#[test]
fn the_meter_the_bar_reads_survives_a_graph_rebuild() {
    // The session-level half, end to end: the graph the session publishes
    // after a rebuild still writes the peaks the bar's meter reads.
    let dir = scratch("rebuild");
    let project = common::a_project_with_a_clip(8, 120.0, SR);
    let clip = Session::first_clip(&project).expect("a blank project has one clip");
    let channel_nodes = fontelle_app::channel_nodes(&project);
    let (publisher, _timeline) = fontelle_engine::timeline_channel(CompiledTimeline::empty());
    let library = SampleLibrary::new();
    let realised = realise(&project, &library, options()).expect("an empty project must realise");
    let (graphs, mut source) = fontelle_engine::graph_channel(realised.graph);
    // What `main.rs` hands the transport bar's `EngineHost`.
    let bar_meter = Arc::clone(&realised.master);

    let mut session = Session::new(
        project,
        library,
        channel_nodes,
        publisher,
        options(),
        clip,
        None,
    )
    .with_graphs(graphs, realised.track_controls)
    .with_param_nodes(realised.param_nodes)
    .with_metronome(realised.metronome)
    .with_master_meter(Arc::clone(&realised.master))
    .with_settings_path(dir.join("settings.json"));

    assert!(
        Arc::ptr_eq(&session.master_meter(), &bar_meter),
        "the session must hold the meter the bar was given"
    );

    // Anything that rebuilds the graph. Adding a mixer track is the cheapest.
    session.add_mixer_track();
    assert!(
        source.take_update(),
        "adding a track must have published a new graph"
    );

    // Something on the master bus: the click, on the downbeat at sample 0.
    session.metronome().set_on(true);
    let graph = source.current();
    let transport = TransportSnapshot {
        state: TransportState::Playing,
        position_sample: 0,
        bpm: 120.0,
    };
    let empty = CompiledTimeline::empty();
    let mut cursor = 0usize;
    for block in 0..4 {
        let range = block * BLOCK_SIZE as i64..(block + 1) * BLOCK_SIZE as i64;
        let events = empty.events_for_block(&mut cursor, range.clone());
        graph.process_block(events, transport, range);
    }

    let peaks = bar_meter.take_peaks();
    assert!(
        peaks[0] > 0.0,
        "the click sounded on the master bus and the bar's meter read {peaks:?}: \
         the rebuilt graph is publishing into a meter nobody reads"
    );
    std::fs::remove_dir_all(&dir).ok();
}
