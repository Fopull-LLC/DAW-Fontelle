//! The metronome, from the side that builds it.
//!
//! Reported from using the window: *"the metronome is not audible"* — the
//! node had no beat, because the one thing that told it one was never
//! called. And 2026-10-19: *"the metronome absolutely needs to match the
//! correct signature you are in"* — a beat pushed in from outside could only
//! ever be one signature at one tempo.
//!
//! So the click has no beat of its own any more. The transport hands it each
//! block's beats off the compiled timeline's meter and tempo table
//! (`fontelle-engine/tests/meter_clock.rs`), and what is left to check from
//! this side is the wire: a song compiled the way the studio compiles it
//! clicks in its own signature, and the switch on the transport bar stays
//! connected to the node the graph is playing across every rebuild.

mod common;

use std::path::PathBuf;

use fontelle_app::{RealiseOptions, SampleLibrary, Session, demo_project, realise};
use fontelle_types::CompiledTimeline;
use fontelle_ui::document::StudioHost;

use common::SR;

/// One beat at 120 BPM, 48 kHz.
const BEAT_AT_120: u32 = 24_000;

fn options() -> RealiseOptions {
    RealiseOptions {
        sample_rate: SR,
        block_size: fontelle_engine::BLOCK_SIZE,
        quality: fontelle_app::PLAYBACK_QUALITY,
    }
}

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("fontelle-click-{name}-{}", std::process::id()));
    std::fs::remove_dir_all(&path).ok();
    std::fs::create_dir_all(&path).expect("the scratch folder must be creatable");
    path
}

fn studio(dir: &std::path::Path) -> Session {
    let project = common::a_project_with_a_clip(8, 120.0, SR);
    let clip = Session::first_clip(&project).expect("a blank project has one clip");
    let channel_nodes = fontelle_app::channel_nodes(&project);
    let (publisher, _timeline) = fontelle_engine::timeline_channel(CompiledTimeline::empty());
    let library = SampleLibrary::new();
    let realised = realise(&project, &library, options()).expect("an empty project must realise");
    let (graphs, _source) = fontelle_engine::graph_channel(realised.graph);

    Session::new(
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
    .with_settings_path(dir.join("settings.json"))
}

// ------------------------------------------------- a click with a meter ---

/// Every click a compiled `project` hands out over its first `bars` bars,
/// as (sample, accent).
fn clicks_of(project: &fontelle_model::Project, bars: u32) -> Vec<(i64, fontelle_types::Accent)> {
    use fontelle_engine::{BLOCK_SIZE, Transport, TransportReader, TransportState};
    let timeline = fontelle_sequencer::compile(
        project,
        &std::collections::HashMap::new(),
        &std::collections::HashMap::new(),
    );
    let end = project
        .tempo_map
        .tick_to_sample(project.meter().tick_of_bar(bars));
    let transport = Transport::new();
    transport.set_state(TransportState::Playing);
    let mut reader = TransportReader::new();
    let mut clicks = Vec::new();
    let mut at = 0;
    while at < end {
        let step = reader.next_step(&transport, &timeline, BLOCK_SIZE, BLOCK_SIZE, false);
        for (offset, accent) in step.snapshot.clicks.iter() {
            clicks.push((step.range.start + i64::from(offset), accent));
        }
        at += step.frames as i64;
    }
    clicks.retain(|c| c.0 < end);
    clicks
}

/// A song in 7/8 felt 3+2+2, compiled as the studio compiles it, clicks
/// seven eighths a bar, the bar's first loudest and each group's first next.
#[test]
fn a_song_clicks_in_its_own_signature() {
    use fontelle_types::Accent::{Bar, Beat, Group};
    let mut project = demo_project(60, 120.0, SR);
    project.beats_per_bar = 7;
    project.beat_unit = 8;
    project.beat_groups = vec![3, 2, 2];
    let clicks = clicks_of(&project, 1);
    let eighth = i64::from(BEAT_AT_120) / 2;
    assert_eq!(
        clicks.iter().map(|c| c.0).collect::<Vec<_>>(),
        (0..7).map(|i| i * eighth).collect::<Vec<_>>()
    );
    assert_eq!(
        clicks.iter().map(|c| c.1).collect::<Vec<_>>(),
        vec![Bar, Beat, Beat, Group, Beat, Group, Beat]
    );
}

/// And a change part way through is followed from its own bar line.
#[test]
fn a_signature_change_is_clicked_from_its_bar() {
    let mut project = demo_project(60, 120.0, SR);
    project.meter_changes = vec![fontelle_types::MeterChange::new(1, 3, 4)];
    let clicks = clicks_of(&project, 3);
    let beat = i64::from(BEAT_AT_120);
    let bars: Vec<i64> = clicks
        .iter()
        .filter(|c| c.1 == fontelle_types::Accent::Bar)
        .map(|c| c.0)
        .collect();
    assert_eq!(bars, vec![0, 4 * beat, 7 * beat]);
}

#[test]
fn the_click_is_still_off_by_default() {
    // The other half of the sentence `Metronome::new` is written around: a
    // window that clicks at you the first time you press play is one you have
    // to go and find the switch for. Giving it a tempo must not turn it on.
    let project = demo_project(60, 120.0, SR);
    let realised =
        realise(&project, &SampleLibrary::new(), options()).expect("the demo must realise");
    assert!(!realised.metronome.is_on());
}

#[test]
fn the_switch_survives_a_graph_rebuild() {
    // This is the one that made the button dead in the running window.
    // `rebuild_graph` used to pass `None`, so `realise` minted a fresh
    // metronome and the session adopted it — while the transport bar went on
    // holding the *original* `Arc`. Everything still compiled, every unit test
    // still passed, and the metronome button stopped working the first time
    // anybody chose a soundfont.
    let dir = scratch("rebuild");
    let mut session = studio(&dir);

    let before = session.metronome();
    before.set_on(true);

    // Anything that rebuilds the graph. Adding a mixer track is the cheapest.
    session.add_mixer_track();

    let after = session.metronome();
    assert!(
        std::sync::Arc::ptr_eq(&before, &after),
        "the graph was rebuilt around a different metronome than the one the \
         transport bar holds"
    );
    assert!(
        after.is_on(),
        "the click was switched on and a rebuild turned it off"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_switch_the_window_reads_is_the_one_it_writes() {
    let dir = scratch("switch");
    let mut session = studio(&dir);
    assert!(!session.metronome_on());
    session.set_metronome(true);
    assert!(session.metronome_on());
    assert!(
        session.metronome().is_on(),
        "the button and the node must be one thing"
    );
    session.set_metronome(false);
    assert!(!session.metronome().is_on());
    std::fs::remove_dir_all(&dir).ok();
}
