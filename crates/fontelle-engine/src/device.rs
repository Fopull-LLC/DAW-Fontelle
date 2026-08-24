use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::graph::CompiledGraph;
use crate::rt_guard::mark_current_thread_rt;
use crate::transport::{TransportSnapshot, TransportState};

/// The fixed block size the M0 vertical slice targets (TDD §22: "128 frames /
/// 48 kHz"). `CompiledGraph`'s `BufferPool` is sized to this; a backend that
/// won't honour the requested fixed buffer size still works — the callback
/// below processes in chunks of at most this size regardless of what the
/// device actually delivers per call.
pub const BLOCK_SIZE: usize = 128;

#[derive(Debug)]
pub struct DeviceError(pub String);

impl std::fmt::Display for DeviceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for DeviceError {}

/// Thin wrapper over `cpal`'s host/device selection (TDD §3.1: native PipeWire
/// backend, auto-selecting PipeWire > PulseAudio > ALSA on Linux; ASIO on Windows;
/// CoreAudio on macOS).
pub struct AudioDevice {
    host: cpal::Host,
    stream: Option<cpal::Stream>,
}

impl AudioDevice {
    pub fn default_host() -> Self {
        Self {
            host: cpal::default_host(),
            stream: None,
        }
    }

    pub fn default_output_name(&self) -> Option<String> {
        self.host
            .default_output_device()
            .and_then(|d| d.description().ok())
            .map(|desc| desc.name().to_string())
    }

    /// Builds and starts the output stream, driving `graph` from the real
    /// audio callback. Promotes the callback thread to RT priority
    /// (`audio_thread_priority`) and tags it via `rt_guard::mark_current_thread_rt`
    /// on its first invocation — both must happen *inside* the callback since
    /// cpal creates that OS thread itself; there's no separate "thread
    /// started" hook to do it from ahead of time.
    ///
    /// The returned stream is stopped and dropped when `self` is dropped or
    /// `stop` is called — cpal has no separate "close" step.
    pub fn start_output_stream(
        &mut self,
        mut graph: CompiledGraph,
        sample_rate: u32,
    ) -> Result<(), DeviceError> {
        let device = self
            .host
            .default_output_device()
            .ok_or_else(|| DeviceError("no default output device".into()))?;

        let mut config = device
            .default_output_config()
            .map_err(|e| DeviceError(e.to_string()))?
            .config();
        config.sample_rate = sample_rate;
        config.buffer_size = cpal::BufferSize::Fixed(BLOCK_SIZE as u32);
        let channels = config.channels as usize;

        let mut first_callback = true;
        let mut rt_handle: Option<audio_thread_priority::RtPriorityHandle> = None;
        let mut sample_counter: i64 = 0;

        let stream = device
            .build_output_stream(
                config,
                move |data: &mut [f32], _info: &cpal::OutputCallbackInfo| {
                    if first_callback {
                        // Treat the very first callback as warm-up, not
                        // steady-state RT processing: promoting priority goes
                        // through rtkit over D-Bus on Linux, which legitimately
                        // allocates for the one-time handshake, and some
                        // backends do their own first-use lazy setup (format
                        // conversion buffers etc.) on this call too. None of
                        // that is what INVARIANT 1 is meant to catch. Do the
                        // promotion, output one silent block, and only start
                        // tagging/enforcing RT from the second callback on —
                        // ~2.7ms of silence at 128 samples/48kHz, not audible.
                        rt_handle = audio_thread_priority::promote_current_thread_to_real_time(
                            BLOCK_SIZE as u32,
                            sample_rate,
                        )
                        .ok();
                        first_callback = false;
                        data.fill(0.0);
                        return;
                    }
                    mark_current_thread_rt();

                    let frames_total = data.len() / channels.max(1);
                    let mut written = 0;
                    while written < frames_total {
                        let chunk = (frames_total - written).min(BLOCK_SIZE);
                        let transport = TransportSnapshot {
                            state: TransportState::Playing,
                            position_sample: sample_counter,
                        };
                        graph.process_block(
                            &[],
                            transport,
                            sample_counter..sample_counter + chunk as i64,
                        );

                        let block = graph.buffer_pool.buffer_mut(0);
                        for i in 0..chunk {
                            let s = block[i];
                            for c in 0..channels {
                                data[(written + i) * channels + c] = s;
                            }
                        }

                        written += chunk;
                        sample_counter += chunk as i64;
                    }

                    let _ = &rt_handle; // kept alive for the stream's lifetime
                },
                |err| eprintln!("Fontelle: audio stream error: {err}"),
                None,
            )
            .map_err(|e| DeviceError(e.to_string()))?;

        stream.play().map_err(|e| DeviceError(e.to_string()))?;
        self.stream = Some(stream);
        Ok(())
    }

    pub fn stop(&mut self) {
        self.stream = None;
    }
}
