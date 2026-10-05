//! Choosing the audio output: which backend, which device, how big a buffer,
//! changed while the studio runs.
//!
//! Reported: *"audio drivers not configurable enough so pretty sure its
//! defaulting to default audio drivers for a lot of users causing things to
//! sound like failing audio drivers sometimes"*. The output was always the
//! default host's default device at 128 frames, with no way to say
//! otherwise and no way to see that it was dropping out.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use fontelle_engine::{CallbackSlot, OutputStats, output_device_names, output_host_names};

// ------------------------------------------------------------ the backends ---

#[test]
fn the_backends_to_choose_from_are_listed_by_name_with_the_default_among_them() {
    let hosts = output_host_names();
    assert!(!hosts.is_empty(), "every platform has at least one");
    let default = cpal::default_host().id().name().to_string();
    assert!(hosts.contains(&default), "{default} missing from {hosts:?}");
    #[cfg(target_os = "linux")]
    assert!(hosts.iter().any(|h| h == "ALSA"), "{hosts:?}");
}

#[test]
fn a_backend_that_is_not_there_has_no_devices() {
    assert!(output_device_names(Some("No Such Backend")).is_empty());
}

// ------------------------------------------------------------- the dropouts ---

#[test]
fn a_dropout_the_backend_reports_is_counted() {
    let stats = OutputStats::default();
    assert_eq!(stats.xruns(), 0);
    stats.note_error(&cpal::Error::from(cpal::ErrorKind::Xrun));
    stats.note_error(&cpal::Error::from(cpal::ErrorKind::Xrun));
    assert_eq!(stats.xruns(), 2);
    assert!(!stats.lost());
}

#[test]
fn a_device_that_goes_away_is_noticed_and_is_not_a_dropout() {
    let stats = OutputStats::default();
    stats.note_error(&cpal::Error::from(cpal::ErrorKind::DeviceNotAvailable));
    assert!(stats.lost());
    assert_eq!(stats.xruns(), 0);
    stats.clear_lost();
    assert!(!stats.lost());
}

// ------------------------------------------------- handing the state back ---

/// What a callback owns lives in a slot the stream borrows it from, so a
/// stream torn down for another device hands it back rather than taking it
/// with it.
#[test]
fn a_callback_reaches_its_state_until_the_slot_is_parked() {
    let slot = CallbackSlot::new(0_u32);
    assert_eq!(
        slot.with(|n| {
            *n += 1;
            *n
        }),
        Some(1)
    );
    slot.park();
    assert_eq!(slot.with(|n| *n), None, "a parked slot is silence");
    assert_eq!(slot.with_parked(|n| *n), Some(1), "and is the caller's");
    slot.unpark();
    assert_eq!(slot.with(|n| *n), Some(1));
    assert_eq!(slot.with_parked(|n| *n), None, "and the callback's again");
}

#[test]
fn parking_waits_for_a_callback_already_inside() {
    let slot = Arc::new(CallbackSlot::new(0_u32));
    let inside = Arc::new(AtomicBool::new(false));
    let audio = {
        let slot = Arc::clone(&slot);
        let inside = Arc::clone(&inside);
        std::thread::spawn(move || {
            slot.with(|n| {
                inside.store(true, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(100));
                *n = 7;
            })
        })
    };
    while !inside.load(Ordering::SeqCst) {
        std::thread::yield_now();
    }
    slot.park();
    assert_eq!(
        slot.with_parked(|n| *n),
        Some(7),
        "the park came back only once the callback was done with it"
    );
    assert_eq!(audio.join().unwrap(), Some(()));
}
