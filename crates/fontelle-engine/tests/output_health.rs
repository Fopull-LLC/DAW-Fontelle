//! Whose click was it? The output counts a buffer the **device** lost apart
//! from one **Fontelle** was too late with, and keeps how busy rendering
//! is, so Settings, the status line and a bug report can say which —
//! the cure for one is the opposite of the cure for the other.

use std::time::Duration;

use fontelle_engine::{OutputHealth, OutputStats};

const MS: Duration = Duration::from_millis(1);

#[test]
fn the_device_and_fontelle_are_counted_apart() {
    let stats = OutputStats::default();
    // The backend said the device ran dry: the device's.
    stats.note_error(&cpal::Error::from(cpal::ErrorKind::Xrun));
    // A buffer rendered in longer than it lasts: Fontelle's.
    stats.note_cycle(6 * MS, 5 * MS);
    // And one in time.
    stats.note_cycle(MS, 5 * MS);
    let h = stats.health();
    assert_eq!(h.device_xruns, 1);
    assert_eq!(h.late, 1);
    assert_eq!(stats.xruns(), 2, "the total is both");
    assert!(h.load > 0.0, "{h:?}");
    assert!(h.peak >= 1.19, "{h:?}");

    // A helper's own counts arrive as amounts.
    stats.note_device_xruns(2);
    stats.note_late(3);
    let h = stats.health();
    assert_eq!((h.device_xruns, h.late), (3, 4));

    stats.clear_xruns();
    let h = stats.health();
    assert_eq!((h.device_xruns, h.late), (0, 0));
    assert_eq!(h.peak, 0.0, "the peak starts again with the count");
}

#[test]
fn a_buffer_of_no_length_counts_for_nothing() {
    let stats = OutputStats::default();
    stats.note_cycle(MS, Duration::ZERO);
    assert_eq!(stats.health(), OutputHealth::default());
}

#[test]
fn health_reads_as_words() {
    assert_eq!(OutputHealth::default().dropouts_label(), "None");
    let h = OutputHealth {
        device_xruns: 3,
        late: 1,
        load: 0.12,
        peak: 0.4,
        handoff_ns: Some(12_400),
    };
    assert_eq!(h.dropouts_label(), "3 from the device, 1 late (Fontelle)");
    assert_eq!(h.load_label(), "12% (peak 40%)");
    assert_eq!(
        h.log_line(),
        "3 device dropouts, 1 late buffer, load 12% (peak 40%), handoff 12 \u{b5}s"
    );
    let device_only = OutputHealth {
        device_xruns: 2,
        ..OutputHealth::default()
    };
    assert_eq!(device_only.dropouts_label(), "2 from the device");
    let late_only = OutputHealth {
        late: 1,
        ..OutputHealth::default()
    };
    assert_eq!(late_only.dropouts_label(), "1 late (Fontelle)");
    assert_eq!(
        late_only.log_line(),
        "0 device dropouts, 1 late buffer, load 0% (peak 0%)"
    );
}

// ------------------------------------------------- WASAPI low latency ---

use fontelle_engine::{LOW_LATENCY_AUTOMATIC, low_latency_period};

/// `IAudioClient3` offers periods in steps of a fundamental from a minimum:
/// a typical driver at 48 kHz says 480 default, 32 fundamental, 32 min, 480
/// max; the generic HD Audio one 480/480/480/480 (no small periods at all).
#[test]
fn a_low_latency_period_is_never_shorter_than_asked_and_is_on_the_drivers_steps() {
    assert_eq!(LOW_LATENCY_AUTOMATIC, 240);
    assert_eq!(low_latency_period(None, 32, 32, 480), 256);
    assert_eq!(low_latency_period(Some(128), 32, 32, 480), 128);
    assert_eq!(low_latency_period(Some(100), 32, 32, 480), 128);
    assert_eq!(low_latency_period(Some(16), 32, 32, 480), 32);
    assert_eq!(low_latency_period(Some(2048), 32, 32, 480), 480);
    // A driver with one period only.
    assert_eq!(low_latency_period(Some(64), 480, 480, 480), 480);
    assert_eq!(low_latency_period(None, 480, 480, 480), 480);
    // Steps from a minimum that is not itself a multiple.
    assert_eq!(low_latency_period(Some(200), 48, 144, 960), 240);
}
