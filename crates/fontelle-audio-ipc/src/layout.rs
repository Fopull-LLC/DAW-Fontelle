//! The shared block: a 512-byte header, then two input slots and two output
//! slots of interleaved `f32` frames.
//!
//! ```text
//! 0      header (512 bytes, below)
//! 512    input slot 0   max_frames × max_inputs  f32
//!        input slot 1   max_frames × max_inputs  f32
//!        output slot 0  max_frames × max_outputs f32
//!        output slot 1  max_frames × max_outputs f32
//! ```
//!
//! Cycle `seq` uses slot `seq & 1`, so a Fontelle that answers one buffer
//! late writes into the slot the helper has already given up on rather than
//! the one it is filling. Every field is little-endian (both ends are the
//! same machine), naturally aligned, and only ever touched through atomics
//! or inside the handshake's ownership rules. Fields are only ever added in
//! the reserved tail, and a change to any existing one is a new
//! [`PROTOCOL_VERSION`].

use std::ptr::NonNull;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

/// `"FAIO"`, the block's first four bytes.
pub const MAGIC: u32 = u32::from_le_bytes(*b"FAIO");

/// The protocol's version: the shared block, the handshake and the control
/// lines together. Both sides refuse a version they do not know.
pub const PROTOCOL_VERSION: u32 = 1;

/// The header's size; the slots start here.
pub const HEADER_BYTES: usize = 512;

/// How many buffers each direction has: cycle `seq` uses `seq & 1`.
pub const SLOTS: usize = 2;

/// The most frames a buffer may have. ASIO drivers top out at 4096 or 8192.
pub const MAX_FRAMES: u32 = 16_384;

/// The most channels each way.
pub const MAX_CHANNELS: u32 = 64;

/// Who is doing what, in [`Header::state`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum State {
    /// Fontelle made the block; the helper has not started the driver yet.
    Starting = 0,
    /// The driver is running and the helper rings once per buffer.
    Running = 1,
    /// The helper stopped (driver gone, an error, told to). No more rings.
    HelperStopped = 2,
    /// Fontelle closed its end. The helper stops the driver and exits.
    ClientClosed = 3,
}

impl State {
    pub fn from_u32(v: u32) -> Option<Self> {
        Some(match v {
            0 => Self::Starting,
            1 => Self::Running,
            2 => Self::HelperStopped,
            3 => Self::ClientClosed,
            _ => return None,
        })
    }
}

/// The first 512 bytes of the block. Offsets are part of the protocol and
/// held by `tests/layout.rs`.
#[repr(C, align(64))]
pub struct Header {
    /// 0: [`MAGIC`].
    pub magic: AtomicU32,
    /// 4: [`PROTOCOL_VERSION`].
    pub version: AtomicU32,
    /// 8: [`HEADER_BYTES`].
    pub header_bytes: AtomicU32,
    /// 12: the slots' size in frames, fixed for the block's life.
    pub max_frames: AtomicU32,
    /// 16: input channels exchanged — every frame in an input slot has
    /// this many samples.
    pub max_inputs: AtomicU32,
    /// 20: output channels exchanged.
    pub max_outputs: AtomicU32,
    /// 24: a [`State`].
    pub state: AtomicU32,
    /// 28: the driver's buffer in frames, as running. Written by the helper.
    pub buffer_frames: AtomicU32,
    /// 32: the driver's sample rate in Hz.
    pub sample_rate: AtomicU32,
    /// 36: input channels the driver has. Informational: the slots carry
    /// `max_inputs` of them, the first ones.
    pub inputs: AtomicU32,
    /// 40: output channels the driver has; the slots carry `max_outputs`.
    pub outputs: AtomicU32,
    /// 44: the driver's input latency, frames.
    pub input_latency: AtomicU32,
    /// 48: the driver's output latency, frames.
    pub output_latency: AtomicU32,
    /// 52: the helper's process id.
    pub helper_pid: AtomicU32,
    /// 56: Fontelle's process id.
    pub client_pid: AtomicU32,
    _pad0: [u32; 1],
    /// 64, its own cache line: the cycle the helper has rung for. Fontelle
    /// waits for it to move.
    pub request_seq: AtomicU32,
    /// 68: frames in that cycle, stored before `request_seq`.
    pub request_frames: AtomicU32,
    _pad1: [u32; 14],
    /// 128, its own cache line: the last cycle Fontelle finished.
    pub reply_seq: AtomicU32,
    /// 132: how long Fontelle spent rendering that cycle, ns.
    pub render_ns: AtomicU32,
    _pad2: [u32; 14],
    /// 192: buffers the helper has rung for.
    pub cycles: AtomicU64,
    /// 200: buffers the **device** lost — the driver said so, or did not call
    /// for one on time. Not Fontelle's doing.
    pub device_xruns: AtomicU64,
    /// 208: buffers Fontelle did not answer before the deadline; silence went
    /// out instead. Fontelle's doing.
    pub late_replies: AtomicU64,
    /// 216: times the driver asked to be reset and the helper did.
    pub resets: AtomicU64,
    /// 224: the last round trip's cost beyond Fontelle's own rendering, ns.
    pub handoff_last_ns: AtomicU32,
    /// 228: the largest of those since the block was made, ns.
    pub handoff_max_ns: AtomicU32,
    /// 232: how long the helper waits for an answer, ns.
    pub deadline_ns: AtomicU32,
    _reserved: [u32; 69],
}

const _: () = assert!(std::mem::size_of::<Header>() == HEADER_BYTES);

/// How big the slots are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layout {
    pub max_frames: u32,
    pub max_inputs: u32,
    pub max_outputs: u32,
}

impl Layout {
    /// A layout, or `None` past [`MAX_FRAMES`] / [`MAX_CHANNELS`] or with no
    /// frames or no outputs.
    pub fn new(max_frames: u32, max_inputs: u32, max_outputs: u32) -> Option<Self> {
        let fits = (1..=MAX_FRAMES).contains(&max_frames)
            && max_inputs <= MAX_CHANNELS
            && (1..=MAX_CHANNELS).contains(&max_outputs);
        fits.then_some(Self {
            max_frames,
            max_inputs,
            max_outputs,
        })
    }

    /// The whole block's size in bytes.
    pub fn bytes(&self) -> usize {
        self.output_offset(SLOTS)
    }

    fn slot_bytes(&self, channels: u32) -> usize {
        self.max_frames as usize * channels as usize * std::mem::size_of::<f32>()
    }

    /// Where input slot `slot` starts, in bytes from the block's start.
    pub fn input_offset(&self, slot: usize) -> usize {
        HEADER_BYTES + slot * self.slot_bytes(self.max_inputs)
    }

    /// Where output slot `slot` starts.
    pub fn output_offset(&self, slot: usize) -> usize {
        self.input_offset(SLOTS) + slot * self.slot_bytes(self.max_outputs)
    }
}

/// Why a block is not one this side can use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayoutError {
    TooSmall { len: usize },
    BadMagic(u32),
    Version(u32),
    HeaderBytes(u32),
    Slots,
}

impl std::fmt::Display for LayoutError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooSmall { len } => write!(f, "the shared block is only {len} bytes"),
            Self::BadMagic(m) => write!(f, "the shared block is not Fontelle's ({m:#010x})"),
            Self::Version(v) => write!(
                f,
                "the shared block is protocol {v}; this side speaks {PROTOCOL_VERSION}"
            ),
            Self::HeaderBytes(n) => write!(f, "the shared block's header is {n} bytes"),
            Self::Slots => write!(f, "the shared block's slot sizes do not fit it"),
        }
    }
}

impl std::error::Error for LayoutError {}

/// A view of a block somebody else owns the memory of — a mapping
/// ([`crate::Region`]) or, in tests, a buffer on the heap.
pub struct Shared {
    base: NonNull<u8>,
    len: usize,
    layout: Layout,
}

// SAFETY: everything reached through `Shared` is either an atomic or a slot
// whose single writer the handshake decides (see `handshake`).
unsafe impl Send for Shared {}
unsafe impl Sync for Shared {}

impl Shared {
    /// Writes a fresh header for `layout` into `len` bytes at `base` and
    /// returns the view. Fontelle's side, which makes the block.
    ///
    /// # Safety
    /// `base` is valid for reads and writes of `len` bytes for the view's
    /// life, aligned to 64, and nothing else writes the header meanwhile.
    pub unsafe fn init(base: NonNull<u8>, len: usize, layout: Layout) -> Result<Self, LayoutError> {
        if len < layout.bytes() {
            return Err(LayoutError::TooSmall { len });
        }
        // SAFETY: the caller's promise; the header fits, as `len` covers the
        // whole layout.
        unsafe { std::ptr::write_bytes(base.as_ptr(), 0, HEADER_BYTES) };
        let view = Self { base, len, layout };
        let h = view.header();
        h.version.store(PROTOCOL_VERSION, Ordering::Relaxed);
        h.header_bytes.store(HEADER_BYTES as u32, Ordering::Relaxed);
        h.max_frames.store(layout.max_frames, Ordering::Relaxed);
        h.max_inputs.store(layout.max_inputs, Ordering::Relaxed);
        h.max_outputs.store(layout.max_outputs, Ordering::Relaxed);
        h.state.store(State::Starting as u32, Ordering::Relaxed);
        h.client_pid.store(std::process::id(), Ordering::Relaxed);
        // Last, released: a block whose magic reads right is whole.
        h.magic.store(MAGIC, Ordering::Release);
        Ok(view)
    }

    /// The view of a block another side made, checked. The helper's side.
    ///
    /// # Safety
    /// As [`init`](Self::init), except that the header is the maker's.
    pub unsafe fn open(base: NonNull<u8>, len: usize) -> Result<Self, LayoutError> {
        if len < HEADER_BYTES {
            return Err(LayoutError::TooSmall { len });
        }
        // SAFETY: the caller's promise, and the header fits.
        let h = unsafe { &*(base.as_ptr() as *const Header) };
        let magic = h.magic.load(Ordering::Acquire);
        if magic != MAGIC {
            return Err(LayoutError::BadMagic(magic));
        }
        let version = h.version.load(Ordering::Relaxed);
        if version != PROTOCOL_VERSION {
            return Err(LayoutError::Version(version));
        }
        let header_bytes = h.header_bytes.load(Ordering::Relaxed);
        if header_bytes as usize != HEADER_BYTES {
            return Err(LayoutError::HeaderBytes(header_bytes));
        }
        let layout = Layout::new(
            h.max_frames.load(Ordering::Relaxed),
            h.max_inputs.load(Ordering::Relaxed),
            h.max_outputs.load(Ordering::Relaxed),
        )
        .filter(|l| l.bytes() <= len)
        .ok_or(LayoutError::Slots)?;
        Ok(Self { base, len, layout })
    }

    pub fn layout(&self) -> Layout {
        self.layout
    }

    pub fn header(&self) -> &Header {
        // SAFETY: `init`/`open` checked the block holds a header, and it is
        // aligned to 64.
        unsafe { &*(self.base.as_ptr() as *const Header) }
    }

    pub fn state(&self) -> Option<State> {
        State::from_u32(self.header().state.load(Ordering::Acquire))
    }

    pub fn set_state(&self, state: State) {
        self.header().state.store(state as u32, Ordering::Release);
    }

    /// Input slot `slot`, `max_frames × max_inputs` samples.
    ///
    /// # Safety
    /// The caller is the slot's writer or reader as the handshake says, and
    /// no other reference to it is alive.
    #[allow(clippy::mut_from_ref)]
    pub unsafe fn input_slot(&self, slot: usize) -> &mut [f32] {
        let at = self.layout.input_offset(slot % SLOTS);
        let n = self.layout.max_frames as usize * self.layout.max_inputs as usize;
        // SAFETY: inside the block (`bytes() <= len`, checked on making the
        // view), aligned (the block is, and every offset is a multiple of 4),
        // and exclusive by the caller's promise.
        unsafe { self.slice(at, n) }
    }

    /// Output slot `slot`, `max_frames × max_outputs` samples.
    ///
    /// # Safety
    /// As [`input_slot`](Self::input_slot).
    #[allow(clippy::mut_from_ref)]
    pub unsafe fn output_slot(&self, slot: usize) -> &mut [f32] {
        let at = self.layout.output_offset(slot % SLOTS);
        let n = self.layout.max_frames as usize * self.layout.max_outputs as usize;
        // SAFETY: as `input_slot`.
        unsafe { self.slice(at, n) }
    }

    /// # Safety
    /// `at + n × 4 <= len`, `at` a multiple of 4, and exclusive access.
    #[allow(clippy::mut_from_ref)]
    unsafe fn slice(&self, at: usize, n: usize) -> &mut [f32] {
        debug_assert!(at + n * 4 <= self.len);
        // SAFETY: the caller's promise.
        unsafe { std::slice::from_raw_parts_mut(self.base.as_ptr().add(at) as *mut f32, n) }
    }

    /// The counters, read once.
    pub fn counters(&self) -> Counters {
        let h = self.header();
        Counters {
            cycles: h.cycles.load(Ordering::Relaxed),
            device_xruns: h.device_xruns.load(Ordering::Relaxed),
            late_replies: h.late_replies.load(Ordering::Relaxed),
            resets: h.resets.load(Ordering::Relaxed),
            handoff_last_ns: h.handoff_last_ns.load(Ordering::Relaxed),
            handoff_max_ns: h.handoff_max_ns.load(Ordering::Relaxed),
        }
    }
}

/// [`Header`]'s counters at one moment.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counters {
    pub cycles: u64,
    pub device_xruns: u64,
    pub late_replies: u64,
    pub resets: u64,
    pub handoff_last_ns: u32,
    pub handoff_max_ns: u32,
}
