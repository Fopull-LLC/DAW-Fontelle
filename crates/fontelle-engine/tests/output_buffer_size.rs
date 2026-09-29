//! Which buffer size the output stream asks the sound card for.
//!
//! It was always `Fixed(BLOCK_SIZE)`, with nothing after it: a device that
//! would not take a 128-frame buffer was a studio that would not start
//! (from a Fedora user's report, "a million problems"). The device's
//! default comes after it, always.

use cpal::{BufferSize, SupportedBufferSize};
use fontelle_engine::{BLOCK_SIZE, output_buffer_sizes};

const BLOCK: u32 = BLOCK_SIZE as u32;

#[test]
fn a_device_that_takes_one_block_is_asked_for_one_then_its_own_default() {
    let sizes = output_buffer_sizes(&SupportedBufferSize::Range { min: 32, max: 8192 });
    assert_eq!(sizes, [BufferSize::Fixed(BLOCK), BufferSize::Default]);
}

#[test]
fn a_device_that_does_not_say_is_still_asked_for_one_block_first() {
    let sizes = output_buffer_sizes(&SupportedBufferSize::Unknown);
    assert_eq!(sizes, [BufferSize::Fixed(BLOCK), BufferSize::Default]);
}

#[test]
fn a_device_whose_range_leaves_a_block_out_is_asked_only_for_its_default() {
    for range in [
        SupportedBufferSize::Range {
            min: 256,
            max: 4096,
        },
        SupportedBufferSize::Range { min: 16, max: 64 },
    ] {
        assert_eq!(
            output_buffer_sizes(&range),
            [BufferSize::Default],
            "{range:?}"
        );
    }
}
