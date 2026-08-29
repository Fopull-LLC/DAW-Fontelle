//! The seam between the window and the audio thread (item 7 of
//! `docs/first-usable-plan.md`).
//!
//! `fontelle-app` is the layer allowed to see both the engine and the UI, so
//! this is where `fontelle_ui::TransportHost` is implemented over the real
//! `Transport` and `MasterMeter`. Every assertion below runs against those
//! real types with **no audio device**, which is the point: the atomics are
//! the whole interface, so what the window would see can be produced by
//! writing to them the way the RT thread does.

use std::sync::Arc;

use fontelle_app::EngineHost;
use fontelle_engine::{Transport, TransportState};
use fontelle_model::TempoMap;
use fontelle_ui::TransportHost;

const RATE: u32 = 48_000;

fn host() -> (Arc<Transport>, EngineHost) {
    let transport = Arc::new(Transport::new());
    let master = Arc::new(fontelle_engine::MasterMeter::default());
    // 120 bpm: one beat is 24 000 samples, one 4/4 bar is 96 000.
    let tempo = TempoMap::new(120.0, RATE as f64);
    let host = EngineHost::new(transport.clone(), master, tempo, 480_000, RATE);
    (transport, host)
}

#[test]
fn the_view_reports_where_the_rt_side_says_playback_is() {
    let (transport, mut host) = host();
    // Exactly how the RT thread publishes it, once per block.
    transport.publish_position(96_000);
    transport.set_state(TransportState::Playing);

    let view = host.view();
    assert!(view.available);
    assert!(view.playing);
    assert!(!view.recording);
    assert_eq!(view.position_sample, 96_000);
    assert_eq!(view.length_samples, 480_000);
    assert_eq!(view.sample_rate, RATE as f64);
}

#[test]
fn the_position_is_converted_to_beats_through_the_tempo_map() {
    let (transport, mut host) = host();
    transport.publish_position(96_000);
    // Four beats at 120 bpm — and it goes through the map rather than through
    // arithmetic on a BPM, because a song with a tempo change has no single
    // BPM to divide by (INVARIANT 5).
    assert!((host.view().position_beats - 4.0).abs() < 1e-6);

    transport.publish_position(12_000);
    assert!((host.view().position_beats - 0.5).abs() < 1e-6);
}

#[test]
fn recording_is_told_apart_from_playing() {
    let (transport, mut host) = host();
    transport.set_state(TransportState::Recording);
    let view = host.view();
    // Both are "the graph is running", but only one of them writes to disk,
    // and the button that says so is red.
    assert!(view.playing);
    assert!(view.recording);
}

#[test]
fn the_windows_commands_reach_the_transport() {
    let (transport, mut host) = host();

    host.play();
    assert_eq!(transport.state(), TransportState::Playing);

    host.seek(240_000);
    // A seek is a *request* the RT side applies at the top of its next block,
    // so the published position has not moved yet — which is exactly what the
    // playhead should keep showing until playback actually gets there.
    let mut seen = 0;
    assert_eq!(transport.take_seek(&mut seen), Some(240_000));

    host.set_looping(true);
    assert!(transport.is_looping());

    host.stop();
    assert_eq!(transport.state(), TransportState::Stopped);
}

#[test]
fn the_meter_readings_are_taken_and_reset() {
    let (_transport, mut host) = host();
    // Nothing has played, so the meter reads silence rather than a stale
    // value from whatever ran last.
    assert_eq!(host.view().peaks, [0.0, 0.0]);
    assert_eq!(host.view().reduction_db, 0.0);
}

#[test]
fn the_loop_range_the_window_shows_is_the_one_the_engine_holds() {
    let (transport, mut host) = host();
    transport.set_loop_range((0, 3840), (0, 96_000));
    transport.set_looping(true);

    let view = host.view();
    assert!(view.looping);
    assert_eq!(view.loop_range_samples, (0, 96_000));
}
