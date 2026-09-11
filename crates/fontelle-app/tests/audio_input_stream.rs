//! The stream behind a mixer strip's input button (TDD §15.4): when it is
//! opened, when it is closed, and what happens when it will not open.
//!
//! Reported from using the window:
//!
//! > *"when opening a project that has a track with an input set, you have
//! > to change the input then change it back for it to actually start
//! > capturing the sound otherwise it will just look like its not capturing
//! > any input at all. also if you try changing the input it often just
//! > crashed for me when i set it to no input briefly to try and change it
//! > back to fix the issue."*
//!
//! The crash is `fontelle-engine`'s (`tests/input_teardown.rs`). What is
//! here is the session's half: a device that would not open used to be
//! remembered as failed **for as long as the session lived** and never
//! tried again — so an input that was busy, suspended, or not yet there when
//! the project opened stayed silent until somebody chose another input and
//! chose back, which is the report's "change the input then change it back"
//! to the letter. And clearing the input dropped the device without telling
//! the monitor ring, which went on saying a stream was open.
//!
//! There is no microphone a test can rely on, so the input that will not
//! open is one that does not exist, and the ring is pushed in by hand where
//! a real stream would have opened one.

mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use fontelle_app::{RealiseOptions, SampleLibrary, Session};
use fontelle_engine::{InputMonitor, graph_channel, input_capture_channel, timeline_channel};
use fontelle_types::CompiledTimeline;
use fontelle_ui::document::{DocumentHost, StudioHost};

use common::SR;

const NOWHERE: &str = "no such microphone";

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "fontelle-input-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::remove_dir_all(&path).ok();
    std::fs::create_dir_all(&path).expect("creatable");
    path
}

/// A saved project with a mixer track to put an input on, and a monitor
/// ring behind it — what the window's session has.
fn a_session(dir: &Path, monitor: &Arc<InputMonitor>) -> (Session, usize) {
    let project = common::a_project_with_a_clip(8, 120.0, SR);
    let clip = Session::first_clip(&project).expect("a blank project has one clip");
    let channel_nodes = fontelle_app::channel_nodes(&project);
    let (publisher, _timeline) = timeline_channel(CompiledTimeline::empty());
    let library = SampleLibrary::new();
    let options = RealiseOptions {
        sample_rate: SR,
        block_size: fontelle_engine::BLOCK_SIZE,
        quality: fontelle_app::PLAYBACK_QUALITY,
    };
    let realised =
        fontelle_app::realise(&project, &library, options).expect("an empty project must realise");
    let (graphs, _source) = graph_channel(realised.graph);
    let bundle = dir.join("Song.fontelle");
    std::fs::create_dir_all(&bundle).expect("creatable");
    let mut session = Session::new(
        project,
        library,
        channel_nodes,
        publisher,
        options,
        clip,
        Some(bundle),
    )
    .with_graphs(graphs, realised.track_controls)
    .with_param_nodes(realised.param_nodes)
    .with_settings_path(dir.join("settings.json"))
    .with_monitor(Arc::clone(monitor));
    session.add_mixer_track();
    let mic = session.selected_mixer_track();
    (session, mic)
}

fn could_not_open(message: Option<&str>) -> bool {
    message.is_some_and(|m| m.starts_with("could not open") && m.contains(NOWHERE))
}

// ------------------------------------------------ closing what was open ---

#[test]
fn choosing_no_input_tells_the_monitor_the_stream_is_gone() {
    let dir = scratch("closes");
    let monitor = Arc::new(InputMonitor::new(4_096));
    let (mut session, mic) = a_session(&dir, &monitor);
    session.set_track_input(mic, Some(NOWHERE.to_string()));
    // The stream a real device would have opened, pushed in by hand — and
    // the ring it opens the moment it starts.
    let (_writer, reader) = input_capture_channel(1_024);
    session.set_audio_input(reader, 48_000, 1);
    monitor.open(48_000, 1);
    assert!(monitor.is_live());

    session.set_track_input(mic, None);

    assert!(
        !monitor.is_live(),
        "a ring still saying a stream is open keeps the graph awake for a \
         microphone nobody chose"
    );
    std::fs::remove_dir_all(&dir).ok();
}

// ------------------------------------------- an input that would not open ---

#[test]
fn an_input_that_will_not_open_says_so_once_and_is_not_retried_every_frame() {
    let dir = scratch("memo");
    let monitor = Arc::new(InputMonitor::new(4_096));
    let (mut session, mic) = a_session(&dir, &monitor);

    session.set_track_input(mic, Some(NOWHERE.to_string()));
    assert!(
        could_not_open(session.take_message().as_deref()),
        "the input button has to say why the microphone is not there"
    );

    session.pump();
    session.pump();
    assert_eq!(
        session.take_message(),
        None,
        "sixty retries a second is sixty device probes a second"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn an_input_that_would_not_open_is_tried_again_later() {
    // *"you have to change the input then change it back for it to actually
    // start capturing"* — because a failed open was remembered for the life
    // of the session. A device that is busy or suspended when the project
    // opens is not a device that is gone.
    let dir = scratch("retry");
    let monitor = Arc::new(InputMonitor::new(4_096));
    let (mut session, mic) = a_session(&dir, &monitor);
    session.retry_inputs_every(Duration::ZERO);

    session.set_track_input(mic, Some(NOWHERE.to_string()));
    assert!(could_not_open(session.take_message().as_deref()));

    session.pump();
    assert!(
        could_not_open(session.take_message().as_deref()),
        "with the retry due, the next frame tries the device again"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_project_opened_again_asks_for_its_input_again() {
    // The memo belongs to the session; the question belongs to the project.
    // Opening a project — this one again, or another naming the same
    // microphone — is a fresh question, whatever the last answer was.
    let dir = scratch("reopen");
    let monitor = Arc::new(InputMonitor::new(4_096));
    let (mut session, mic) = a_session(&dir, &monitor);
    session.set_track_input(mic, Some(NOWHERE.to_string()));
    assert!(could_not_open(session.take_message().as_deref()));
    session.save().expect("the project has a bundle");
    let bundle = dir.join("Song.fontelle");

    session
        .open_project_path(&bundle)
        .expect("the bundle just written opens");
    session.pump();

    assert!(
        could_not_open(session.take_message().as_deref()),
        "the reopened project's input was never tried"
    );
    std::fs::remove_dir_all(&dir).ok();
}
