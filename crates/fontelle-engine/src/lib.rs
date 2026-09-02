//! Audio graph, transport, voice management, mixer topology, device I/O
//! (FONTELLE_TDD.md §5-6). Owns the RT thread. INVARIANT 1: the RT thread never
//! allocates, locks, blocks, or performs a syscall — enforced in debug/test builds
//! by [`rt_guard::RtGuardAllocator`].

mod device;
mod effect_channel;
mod graph;
mod graph_channel;
mod live;
mod nodes;
mod rt_guard;
mod key_tap;
mod spectrum_tap;
mod timeline_channel;
mod transport;

pub use device::{AudioDevice, BLOCK_SIZE, DeviceError};
pub use effect_channel::{EffectControls, EffectSource, effect_channel};
pub use fontelle_fx::LimiterConfig;
pub use graph::{
    AudioNode, BufferPool, CompiledGraph, ParamSet, PrepareContext, ProcessContext, ScheduledNode,
};
pub use graph_channel::{GRAPH_QUEUE_CAPACITY, GraphPublisher, GraphSource, graph_channel};
pub use live::{
    CAPTURE_CAPACITY, CaptureReader, CaptureWriter, IdleGate, LIVE_PORT_CAPACITY, LIVE_PORT_COUNT,
    LiveEventPorts, LiveEventSource, LivePort, live_capture_channel, live_event_channel,
};
pub use nodes::{
    AudioClipNode, BusSumNode, CHANNEL_GAIN_MAX_DB, CHANNEL_GAIN_MIN_DB, EffectNode, GAIN_MAX_DB,
    GAIN_MIN_DB, MasterMeter, MasterNode,
    Metronome, MetronomeNode, MixerTrackNode, SEND_MIN_DB, SamplerNode, SendControls,
    SendNode, TrackControls,
};
pub use key_tap::{KeyTap, KeyTapNode};
pub use spectrum_tap::SpectrumTap;
pub use rt_guard::{
    RtGuardAllocator, current_thread_is_rt, mark_current_thread_rt, unmark_current_thread_rt,
    with_rt_thread,
};
pub use timeline_channel::{TimelinePublisher, TimelineSource, timeline_channel};
pub use transport::{Step, Transport, TransportReader, TransportSnapshot, TransportState};
