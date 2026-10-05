//! Which buffer size the output stream asks the sound card for.
//!
//! It was `Fixed(BLOCK_SIZE)` — 128 frames, 2.7 ms at 48 kHz — whenever the
//! device's range had room for it, and almost every device's does. Reported:
//! *"audio drivers not configurable enough so pretty sure its defaulting to
//! default audio drivers for a lot of users causing things to sound like
//! failing audio drivers sometimes"*. 2.7 ms is a period most desktops cannot
//! keep filled through ALSA's PipeWire or PulseAudio plugin once a plugin or a
//! busy moment takes a share of it, and every miss is a click. The default is
//! now [`DEFAULT_OUTPUT_BUFFER`] frames, the person can ask for any size in
//! Settings, and the device's own default always comes last so a card that
//! takes neither still plays.

use cpal::{BufferSize, SupportedBufferSize};
use fontelle_engine::{DEFAULT_OUTPUT_BUFFER, output_buffer_sizes};

const WIDE: SupportedBufferSize = SupportedBufferSize::Range { min: 32, max: 8192 };

#[test]
fn the_default_is_a_buffer_a_desktop_can_keep_filled() {
    // 128 frames is the size that crackled.
    const { assert!(DEFAULT_OUTPUT_BUFFER >= 256) };
    assert_eq!(
        output_buffer_sizes(&WIDE, None),
        [
            BufferSize::Fixed(DEFAULT_OUTPUT_BUFFER),
            BufferSize::Default
        ]
    );
}

#[test]
fn a_device_that_does_not_say_is_still_asked_for_the_default_first() {
    assert_eq!(
        output_buffer_sizes(&SupportedBufferSize::Unknown, None),
        [
            BufferSize::Fixed(DEFAULT_OUTPUT_BUFFER),
            BufferSize::Default
        ]
    );
}

#[test]
fn a_chosen_size_is_asked_for_first_then_the_default_then_the_devices_own() {
    assert_eq!(
        output_buffer_sizes(&WIDE, Some(128)),
        [
            BufferSize::Fixed(128),
            BufferSize::Fixed(DEFAULT_OUTPUT_BUFFER),
            BufferSize::Default
        ]
    );
    // Asking for the default by number is the default, once.
    assert_eq!(
        output_buffer_sizes(&WIDE, Some(DEFAULT_OUTPUT_BUFFER)),
        [
            BufferSize::Fixed(DEFAULT_OUTPUT_BUFFER),
            BufferSize::Default
        ]
    );
}

#[test]
fn a_size_outside_the_devices_range_is_brought_inside_it() {
    let narrow = SupportedBufferSize::Range {
        min: 1024,
        max: 4096,
    };
    assert_eq!(
        output_buffer_sizes(&narrow, None),
        [BufferSize::Fixed(1024), BufferSize::Default]
    );
    assert_eq!(
        output_buffer_sizes(&narrow, Some(64)),
        [BufferSize::Fixed(1024), BufferSize::Default]
    );
    let small = SupportedBufferSize::Range { min: 16, max: 64 };
    assert_eq!(
        output_buffer_sizes(&small, Some(2048)),
        [BufferSize::Fixed(64), BufferSize::Default]
    );
}
