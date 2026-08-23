use cpal::traits::{DeviceTrait, HostTrait};

/// Thin wrapper over `cpal`'s host/device selection (TDD §3.1: native PipeWire
/// backend, auto-selecting PipeWire > PulseAudio > ALSA on Linux; ASIO on Windows;
/// CoreAudio on macOS).
pub struct AudioDevice {
    host: cpal::Host,
}

impl AudioDevice {
    pub fn default_host() -> Self {
        Self {
            host: cpal::default_host(),
        }
    }

    pub fn default_output_name(&self) -> Option<String> {
        self.host
            .default_output_device()
            .and_then(|d| d.description().ok())
            .map(|desc| desc.name().to_string())
    }

    /// Builds and starts the output stream, promotes the callback thread to RT
    /// priority (`audio_thread_priority`) and tags it via
    /// `rt_guard::mark_current_thread_rt` before the first callback runs. The M0
    /// gate (TDD §22) is this function driving a real `CompiledGraph`.
    pub fn start_output_stream(&mut self, _graph: crate::graph::CompiledGraph) {
        todo!(
            "cpal::build_output_stream + audio_thread_priority::promote_current_thread_to_real_time"
        )
    }
}
