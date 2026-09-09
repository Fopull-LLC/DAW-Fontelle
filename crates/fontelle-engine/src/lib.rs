//! Audio graph, transport, voice management, mixer topology, device I/O
//! (FONTELLE_TDD.md §5-6). Owns the RT thread. INVARIANT 1: the RT thread never
//! allocates, locks, blocks, or performs a syscall — enforced in debug/test builds
//! by [`rt_guard::RtGuardAllocator`].

mod audio_input;
mod device;
mod effect_channel;
mod graph;
mod graph_channel;
mod input_monitor;
mod key_tap;
mod live;
mod nodes;
mod pipewire;
mod plugin_node;
mod rt_guard;
mod spectrum_tap;
mod timeline_channel;
mod transport;
mod tune_tap;

pub use audio_input::{InputCapture, InputReader, InputWriter, input_capture_channel};
pub use device::{AudioDevice, BLOCK_SIZE, DeviceError};
pub use effect_channel::{EffectControls, EffectSource, effect_channel};
pub use fontelle_fx::LimiterConfig;
pub use graph::{
    AudioNode, BufferPool, CompiledGraph, ParamSet, PrepareContext, ProcessContext, ScheduledNode,
};
pub use graph_channel::{GRAPH_QUEUE_CAPACITY, GraphPublisher, GraphSource, graph_channel};
pub use input_monitor::{InputMonitor, MonitorNode};
pub use key_tap::{KeyTap, KeyTapNode};
pub use live::{
    CAPTURE_CAPACITY, CaptureReader, CaptureWriter, IdleGate, LIVE_PORT_CAPACITY, LIVE_PORT_COUNT,
    LiveEventPorts, LiveEventSource, LivePort, live_capture_channel, live_event_channel,
};
pub use nodes::{
    AudioClipNode, BusSumNode, CHANNEL_GAIN_MAX_DB, CHANNEL_GAIN_MIN_DB, DelayNode, EffectNode,
    GAIN_MAX_DB, GAIN_MIN_DB, HeldKeys, MAX_HELD_KEYS, MasterMeter, MasterNode, Metronome,
    MetronomeNode, MixerTrackNode, SEND_MIN_DB, SamplerNode, SendControls, SendNode, TrackControls,
    VoiceMeter, insert_latency_samples, max_insert_latency_samples,
};
pub use pipewire::{
    PipeWireInput, PipeWireSource, find_pipewire_source, parse_pipewire_default_source,
    parse_pipewire_sources, pipewire_default_source, pipewire_pcm, pipewire_sources, source_menu,
};
pub use plugin_node::{PluginNode, PluginRole};
pub use rt_guard::{
    RtGuardAllocator, current_thread_is_rt, mark_current_thread_rt, unmark_current_thread_rt,
    with_rt_thread,
};
pub use spectrum_tap::SpectrumTap;
pub use timeline_channel::{TimelinePublisher, TimelineSource, timeline_channel};
pub use transport::{Step, Transport, TransportReader, TransportSnapshot, TransportState};
pub use tune_tap::{TUNE_TRACE_FRAMES, TuneTap};
