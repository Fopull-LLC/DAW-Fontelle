//! The handover a graph rebuild goes through.

mod common;

use fontelle_host::{PluginHost, ProcessorBay};
use fontelle_types::PluginKey;

#[test]
fn a_processor_can_be_parked_and_taken_once() {
    let mut host = PluginHost::new();
    let mut plugin = host
        .open(&common::bundle(), &PluginKey::clap(common::GAIN))
        .unwrap();
    let bay = ProcessorBay::new();
    assert!(!bay.is_parked());

    bay.park(plugin.activate(48_000.0, 32).unwrap());
    assert!(bay.is_parked());

    let taken = bay.take();
    assert!(taken.is_some(), "the first taker gets it");
    assert!(bay.take().is_none(), "and the second gets nothing");
    assert!(!bay.is_parked());

    // Put it back so the plugin can be stopped properly — a plugin dropped
    // while its processor is out is deliberately leaked by clack.
    plugin.deactivate(taken.unwrap());
}

#[test]
fn an_empty_bay_hands_out_nothing_rather_than_waiting() {
    let bay = ProcessorBay::new();
    assert!(bay.take().is_none());
    assert!(bay.reclaim().is_none());
}

#[test]
fn a_processor_that_went_out_and_came_back_still_works() {
    let mut host = PluginHost::new();
    let mut plugin = host
        .open(&common::bundle(), &PluginKey::clap(common::GAIN))
        .unwrap();
    plugin.set_param(0, 2.0);
    let bay = ProcessorBay::new();
    bay.park(plugin.activate(48_000.0, 32).unwrap());

    let mut processor = bay.take().unwrap();
    let input = vec![vec![1.0f32; 4], vec![1.0f32; 4]];
    let mut output = vec![vec![0.0f32; 4], vec![0.0f32; 4]];
    processor.process_effect(&input, &mut output, 4);
    assert!((output[0][0] - 2.0).abs() < 1e-5);

    bay.park(processor);
    let mut processor = bay.take().unwrap();
    processor.process_effect(&input, &mut output, 4);
    assert!((output[0][0] - 2.0).abs() < 1e-5);

    plugin.deactivate(processor);
}

// ------------------------------------------ asking for it back (2026-09-05)

/// A recall of a processor that is already home answers at once.
#[test]
fn a_recall_answers_at_once_when_the_processor_is_home() {
    let mut host = PluginHost::new();
    let mut plugin = host
        .open(&common::bundle(), &PluginKey::clap(common::GAIN))
        .unwrap();
    let bay = ProcessorBay::new();
    bay.park(plugin.activate(48_000.0, 32).unwrap());
    let started = std::time::Instant::now();
    let processor = bay
        .recall(std::time::Duration::from_secs(5))
        .expect("home already");
    assert!(started.elapsed() < std::time::Duration::from_millis(500));
    assert!(!bay.wants_return(), "nothing left asked for");
    plugin.deactivate(processor);
}

/// A recall nobody answers — an audio thread that has stopped, a graph that
/// is never processed — gives up after its timeout rather than hanging the
/// studio, and takes its request back with it so a node that wakes up later
/// does not park a processor nobody is waiting for.
#[test]
fn a_recall_nobody_answers_gives_up_after_its_timeout_and_withdraws_the_request() {
    let bay = ProcessorBay::new();
    let started = std::time::Instant::now();
    assert!(bay.recall(std::time::Duration::from_millis(40)).is_none());
    let waited = started.elapsed();
    assert!(waited >= std::time::Duration::from_millis(40), "{waited:?}");
    assert!(waited < std::time::Duration::from_secs(2), "{waited:?}");
    assert!(!bay.wants_return());
}

/// The audio thread's half: the request is visible, and parking from there
/// never waits on the lock.
#[test]
fn the_audio_thread_sees_the_request_and_parks_without_waiting() {
    let mut host = PluginHost::new();
    let mut plugin = host
        .open(&common::bundle(), &PluginKey::clap(common::GAIN))
        .unwrap();
    let bay = ProcessorBay::new();
    let processor = plugin.activate(48_000.0, 32).unwrap();
    assert!(!bay.wants_return());
    bay.request_return();
    assert!(bay.wants_return());
    let processor = match bay.try_park(processor) {
        Ok(()) => bay.reclaim().expect("parked"),
        Err(processor) => processor,
    };
    assert!(!bay.is_parked());
    bay.withdraw_return();
    assert!(!bay.wants_return());
    plugin.deactivate(processor);
}
