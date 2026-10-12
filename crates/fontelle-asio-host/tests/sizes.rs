//! What buffer to ask a driver for, and what to say about a rate it will
//! not run at.

use fontelle_asio_host::sizes::{fit_buffer_size, rate_refusal};

#[test]
fn nothing_asked_is_the_drivers_own_preferred_size() {
    assert_eq!(fit_buffer_size(None, 64, 2048, 256, -1), 256);
    assert_eq!(fit_buffer_size(None, 48, 4800, 480, 48), 480);
    // A driver whose preferred size is outside its own range is brought in.
    assert_eq!(fit_buffer_size(None, 64, 2048, 4096, -1), 2048);
}

#[test]
fn powers_of_two_round_up_never_below_what_was_asked() {
    assert_eq!(fit_buffer_size(Some(128), 64, 2048, 256, -1), 128);
    assert_eq!(fit_buffer_size(Some(100), 64, 2048, 256, -1), 128);
    assert_eq!(fit_buffer_size(Some(16), 64, 2048, 256, -1), 64);
    assert_eq!(fit_buffer_size(Some(3000), 64, 2048, 256, -1), 2048);
    // A minimum that is not itself a power of two (some drivers say 96).
    assert_eq!(fit_buffer_size(Some(100), 96, 2048, 256, -1), 128);
    assert_eq!(fit_buffer_size(Some(64), 96, 2048, 256, -1), 96);
}

#[test]
fn steps_round_up_onto_the_drivers_grid() {
    assert_eq!(fit_buffer_size(Some(100), 48, 4800, 480, 48), 144);
    assert_eq!(fit_buffer_size(Some(144), 48, 4800, 480, 48), 144);
    assert_eq!(fit_buffer_size(Some(10), 48, 4800, 480, 48), 48);
    assert_eq!(fit_buffer_size(Some(9999), 48, 4800, 480, 48), 4800);
    assert_eq!(fit_buffer_size(Some(1), 1, 8192, 512, 1), 1);
}

#[test]
fn a_driver_with_one_size_gets_that_size() {
    assert_eq!(fit_buffer_size(Some(64), 512, 512, 512, 0), 512);
    assert_eq!(fit_buffer_size(Some(64), 128, 1024, 256, 0), 256);
}

#[test]
fn a_rate_refused_says_what_the_driver_offers_and_what_to_do() {
    assert_eq!(
        rate_refusal("ASIO4ALL v2", &[44_100], 48_000),
        "ASIO4ALL v2 cannot run at 48 kHz \u{2014} the driver offers only 44.1 kHz; \
         set it to 48 kHz in the driver's control panel"
    );
    assert_eq!(
        rate_refusal("X", &[44_100, 88_200, 96_000], 48_000),
        "X cannot run at 48 kHz \u{2014} the driver offers 44.1 kHz, 88.2 kHz and 96 kHz; \
         set it to 48 kHz in the driver's control panel"
    );
    assert_eq!(
        rate_refusal("X", &[], 48_000),
        "X cannot run at 48 kHz \u{2014} the driver says no rate it can run at; \
         set it to 48 kHz in the driver's control panel"
    );
}
