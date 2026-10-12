//! The shared block's layout is a public protocol (`docs/asio.md`): a GPL
//! helper built from one Fontelle version talks to another, so every offset
//! is held here, not just "whatever `repr(C)` gives".

use std::mem::offset_of;

use fontelle_audio_ipc::layout::{Header, LayoutError, MAX_CHANNELS, MAX_FRAMES, SLOTS};
use fontelle_audio_ipc::{HEADER_BYTES, Layout, MAGIC, PROTOCOL_VERSION, Region, State};

#[test]
fn the_header_is_512_bytes_with_every_field_where_the_document_says() {
    assert_eq!(std::mem::size_of::<Header>(), HEADER_BYTES);
    assert_eq!(HEADER_BYTES, 512);
    assert_eq!(MAGIC.to_le_bytes(), *b"FAIO");
    assert_eq!(PROTOCOL_VERSION, 1);
    let at = [
        (offset_of!(Header, magic), 0),
        (offset_of!(Header, version), 4),
        (offset_of!(Header, header_bytes), 8),
        (offset_of!(Header, max_frames), 12),
        (offset_of!(Header, max_inputs), 16),
        (offset_of!(Header, max_outputs), 20),
        (offset_of!(Header, state), 24),
        (offset_of!(Header, buffer_frames), 28),
        (offset_of!(Header, sample_rate), 32),
        (offset_of!(Header, inputs), 36),
        (offset_of!(Header, outputs), 40),
        (offset_of!(Header, input_latency), 44),
        (offset_of!(Header, output_latency), 48),
        (offset_of!(Header, helper_pid), 52),
        (offset_of!(Header, client_pid), 56),
        (offset_of!(Header, request_seq), 64),
        (offset_of!(Header, request_frames), 68),
        (offset_of!(Header, reply_seq), 128),
        (offset_of!(Header, render_ns), 132),
        (offset_of!(Header, cycles), 192),
        (offset_of!(Header, device_xruns), 200),
        (offset_of!(Header, late_replies), 208),
        (offset_of!(Header, resets), 216),
        (offset_of!(Header, handoff_last_ns), 224),
        (offset_of!(Header, handoff_max_ns), 228),
        (offset_of!(Header, deadline_ns), 232),
    ];
    for (actual, documented) in at {
        assert_eq!(actual, documented);
    }
    // The two words the sides wait on are on cache lines of their own, so
    // one side's store never bounces the line the other is spinning on.
    assert_eq!(offset_of!(Header, request_seq) % 64, 0);
    assert_eq!(offset_of!(Header, reply_seq) % 64, 0);
    assert_eq!(State::Starting as u32, 0);
    assert_eq!(State::Running as u32, 1);
    assert_eq!(State::HelperStopped as u32, 2);
    assert_eq!(State::ClientClosed as u32, 3);
}

#[test]
fn slots_follow_the_header_inputs_first_and_never_overlap() {
    let layout = Layout::new(1024, 2, 6).unwrap();
    let f = 4;
    assert_eq!(layout.input_offset(0), 512);
    assert_eq!(layout.input_offset(1), 512 + 1024 * 2 * f);
    assert_eq!(layout.output_offset(0), 512 + 2 * 1024 * 2 * f);
    assert_eq!(
        layout.output_offset(1),
        512 + 2 * 1024 * 2 * f + 1024 * 6 * f
    );
    assert_eq!(layout.bytes(), 512 + SLOTS * 1024 * (2 + 6) * f);

    // No inputs at all is allowed — a playback-only output.
    let out_only = Layout::new(256, 0, 2).unwrap();
    assert_eq!(out_only.input_offset(0), out_only.output_offset(0));
    assert_eq!(out_only.bytes(), 512 + 2 * 256 * 2 * 4);

    assert!(Layout::new(0, 0, 2).is_none(), "no frames");
    assert!(Layout::new(256, 2, 0).is_none(), "no outputs");
    assert!(Layout::new(MAX_FRAMES + 1, 0, 2).is_none());
    assert!(Layout::new(256, MAX_CHANNELS + 1, 2).is_none());
    assert!(Layout::new(MAX_FRAMES, MAX_CHANNELS, MAX_CHANNELS).is_some());
}

#[test]
fn a_made_block_says_what_it_is_and_slots_are_the_size_it_says() {
    let layout = Layout::new(512, 2, 2).unwrap();
    let region = Region::heap(layout);
    let shared = region.shared();
    let h = shared.header();
    use std::sync::atomic::Ordering::Relaxed;
    assert_eq!(h.magic.load(Relaxed), MAGIC);
    assert_eq!(h.version.load(Relaxed), PROTOCOL_VERSION);
    assert_eq!(h.header_bytes.load(Relaxed), 512);
    assert_eq!(h.max_frames.load(Relaxed), 512);
    assert_eq!(h.max_inputs.load(Relaxed), 2);
    assert_eq!(h.max_outputs.load(Relaxed), 2);
    assert_eq!(h.client_pid.load(Relaxed), std::process::id());
    assert_eq!(shared.state(), Some(State::Starting));
    assert_eq!(shared.counters(), Default::default());
    // SAFETY: nobody else holds the block.
    unsafe {
        assert_eq!(shared.input_slot(0).len(), 1024);
        assert_eq!(shared.output_slot(1).len(), 1024);
        shared.output_slot(1).fill(0.5);
        assert!(shared.output_slot(0).iter().all(|s| *s == 0.0));
    }
}

#[test]
fn a_block_that_is_not_ours_is_refused_with_why() {
    let layout = Layout::new(64, 0, 2).unwrap();
    let mut bytes = vec![0u64; layout.bytes() / 8 + 8];
    let base = bytes.as_mut_ptr() as *mut u8;
    // Aligned to 64 inside the buffer.
    let skew = base.align_offset(64);
    let base = std::ptr::NonNull::new(unsafe { base.add(skew) }).unwrap();
    let len = layout.bytes();

    // SAFETY: `base..base+len` is inside `bytes`, which outlives each view.
    unsafe {
        assert_eq!(
            fontelle_audio_ipc::Shared::open(base, len).err(),
            Some(LayoutError::BadMagic(0))
        );
        fontelle_audio_ipc::Shared::init(base, len, layout).unwrap();
        assert!(fontelle_audio_ipc::Shared::open(base, len).is_ok());
        assert_eq!(
            fontelle_audio_ipc::Shared::open(base, 100).err(),
            Some(LayoutError::TooSmall { len: 100 })
        );
        let h = &*(base.as_ptr() as *const Header);
        h.version.store(2, std::sync::atomic::Ordering::Relaxed);
        assert_eq!(
            fontelle_audio_ipc::Shared::open(base, len).err(),
            Some(LayoutError::Version(2))
        );
        h.version.store(1, std::sync::atomic::Ordering::Relaxed);
        h.max_frames
            .store(1 << 20, std::sync::atomic::Ordering::Relaxed);
        assert_eq!(
            fontelle_audio_ipc::Shared::open(base, len).err(),
            Some(LayoutError::Slots)
        );
        assert_eq!(
            fontelle_audio_ipc::Shared::init(base, 64, layout).err(),
            Some(LayoutError::TooSmall { len: 64 })
        );
    }
}
