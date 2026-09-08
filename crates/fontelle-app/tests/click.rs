//! The metronome, from the side that has to hand it a tempo.
//!
//! Reported from using the window: *"the metronome is not audible"*. The
//! `MetronomeNode` itself was right and had eight tests
//! (`fontelle-engine/tests/metronome.rs`) — it clicks once a beat, accents the
//! downbeat, lands on the beat after a seek, and adds into its bus rather than
//! over it. What it did not have was a beat.
//!
//! `Metronome::new` starts at **zero samples per beat**, which is the "no
//! tempo yet" value, and a node with no tempo is silent rather than dividing
//! by zero. The only thing that ever called `set_beat` was
//! `Session::publish_metronome`, and the window's `Session` held no metronome
//! to publish to: `Session::new` never took one, so `rebuild_graph` passed
//! `None` and `realise` minted a fresh one every time.
//!
//! Which is two bugs in one wire, and both are here:
//!
//! 1. A realised graph clicks at the project's own tempo, with nobody having
//!    to remember to tell it.
//! 2. The switch on the transport bar stays connected to the node the graph is
//!    playing, across every rebuild — choosing a soundfont must not silently
//!    orphan the metronome button.

mod common;

use std::path::PathBuf;

use fontelle_app::{RealiseOptions, SampleLibrary, Session, demo_project, realise};
use fontelle_types::CompiledTimeline;
use fontelle_ui::document::{DocumentHost, StudioHost};

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

// ------------------------------------------------- a click with a tempo ---

#[test]
fn realising_a_project_tells_the_click_where_the_beats_are() {
    // The bug, stated as the fix: nobody should have to call `set_beat` for a
    // metronome to be audible. `realise` is the one place that sees both the
    // project's tempo map and the node it is building, so it is where the
    // answer belongs.
    let project = demo_project(60, 120.0, SR);
    let realised =
        realise(&project, &SampleLibrary::new(), options()).expect("the demo must realise");

    assert_eq!(
        realised.metronome.samples_per_beat(),
        BEAT_AT_120,
        "a realised graph must know how long a beat is, or its click is silent"
    );
    assert_eq!(realised.metronome.beats_per_bar(), project.beats_per_bar);
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
fn a_project_at_another_tempo_gets_another_beat() {
    // Measured through the tempo map rather than divided out of a BPM, which
    // is the rule `seconds_per_tick` follows and the reason it stays right
    // when the map grows segments.
    let project = demo_project(60, 60.0, SR);
    let realised =
        realise(&project, &SampleLibrary::new(), options()).expect("the demo must realise");
    assert_eq!(
        realised.metronome.samples_per_beat(),
        SR,
        "one beat a second at 60 BPM"
    );
}

// ------------------------------------------ the switch stays connected ---

#[test]
fn the_studio_hands_the_click_its_tempo_too() {
    let dir = scratch("tempo");
    let session = studio(&dir);
    assert_eq!(
        session.metronome().samples_per_beat(),
        BEAT_AT_120,
        "the session's click must know the tempo the moment it is built"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn changing_the_tempo_moves_the_beats() {
    let dir = scratch("retempo");
    let mut session = studio(&dir);
    session.set_tempo(60.0);
    assert_eq!(
        session.metronome().samples_per_beat(),
        SR,
        "the click counts in samples, so a new tempo is a new beat length"
    );
    std::fs::remove_dir_all(&dir).ok();
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
    assert_eq!(
        after.samples_per_beat(),
        BEAT_AT_120,
        "a rebuild must not lose the beat"
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
