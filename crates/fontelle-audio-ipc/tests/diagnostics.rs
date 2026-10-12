//! Telling a device that missed a buffer from a Fontelle that missed its
//! deadline.

use fontelle_audio_ipc::{GapDetector, LoadMeter};

const MS: u64 = 1_000_000;

#[test]
fn a_device_calling_on_time_or_with_jitter_has_lost_nothing() {
    let period = 5 * MS;
    let mut gaps = GapDetector::new(period);
    assert_eq!(
        gaps.tick(0),
        0,
        "the first call has nothing to compare with"
    );
    let mut t = 0;
    let ms = MS as i64;
    for jitter in [0, 2 * ms, -2 * ms, 3 * ms, -3 * ms] {
        t = (t as i64 + period as i64 + jitter) as u64;
        assert_eq!(gaps.tick(t), 0, "jitter {jitter}");
    }
    // Two calls close together after one late one (some USB drivers do
    // this): still nothing lost.
    t += 8 * MS;
    assert_eq!(gaps.tick(t), 0);
    t += MS;
    assert_eq!(gaps.tick(t), 0);
}

#[test]
fn a_gap_of_whole_buffers_is_that_many_lost() {
    let period = 5 * MS;
    let mut gaps = GapDetector::new(period);
    gaps.tick(0);
    assert_eq!(gaps.tick(10 * MS), 1, "two periods: one buffer skipped");
    assert_eq!(gaps.tick(30 * MS), 3, "four periods: three");
    assert_eq!(gaps.tick(39 * MS), 1, "1.8 periods: one");
    assert_eq!(gaps.tick(43 * MS), 0);
}

#[test]
fn a_pause_on_purpose_is_not_a_dropout() {
    let mut gaps = GapDetector::new(5 * MS);
    gaps.tick(0);
    gaps.reset();
    assert_eq!(gaps.tick(1_000 * MS), 0, "after a stop or a reset");
    gaps.set_period(10 * MS);
    assert_eq!(gaps.tick(2_000 * MS), 0, "a new buffer size starts afresh");
    assert_eq!(gaps.tick(2_015 * MS), 0, "1.5 of the new period");
    assert_eq!(gaps.tick(2_045 * MS), 2, "3 of the new period");
}

#[test]
fn load_is_the_share_of_the_buffer_spent_rendering_smoothed() {
    let mut meter = LoadMeter::new();
    for _ in 0..500 {
        assert!(!meter.note(MS, 4 * MS));
    }
    assert!((meter.load() - 0.25).abs() < 0.01, "{}", meter.load());
    assert!((meter.peak() - 0.25).abs() < 0.01, "{}", meter.peak());

    // One slow buffer: late, the peak shows it at once, the load barely.
    assert!(meter.note(5 * MS, 4 * MS), "longer than the buffer is late");
    assert!(meter.peak() >= 1.2, "{}", meter.peak());
    assert!(meter.load() < 0.4, "{}", meter.load());
    // Exactly the buffer is late too: nothing was ready when it was due.
    assert!(meter.note(4 * MS, 4 * MS));
    // The peak falls back, not at once.
    for _ in 0..10 {
        meter.note(MS, 4 * MS);
    }
    assert!(meter.peak() > 0.9, "{}", meter.peak());
    for _ in 0..5_000 {
        meter.note(MS, 4 * MS);
    }
    assert!(meter.peak() < 0.3, "{}", meter.peak());
}

#[test]
fn a_buffer_of_no_length_is_neither_load_nor_late() {
    let mut meter = LoadMeter::new();
    assert!(!meter.note(MS, 0));
    assert_eq!(meter.load(), 0.0);
}
