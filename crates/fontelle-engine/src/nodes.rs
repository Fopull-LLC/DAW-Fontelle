use std::sync::Arc;

use fontelle_core::{SampleStore, Sampler};
use fontelle_types::{PanLaw, ParamAddress};

use crate::graph::{AudioNode, ParamSet, PrepareContext, ProcessContext};

/// The widest render `SamplerNode` performs: a stereo pair. Surround is not a
/// feature yet, and a fixed width is what keeps the scratch allocation in
/// `prepare` (INVARIANT 1).
const MAX_CHANNELS: usize = 2;

struct EmptyParams;
impl ParamSet for EmptyParams {
    fn get(&self, _addr: &ParamAddress) -> Option<f64> {
        None
    }
    fn set(&self, _addr: &ParamAddress, _value: f64) -> bool {
        false
    }
}

/// Wraps `fontelle-core::Sampler` as a graph node (TDD §5.1). This is where the
/// engine hands the sampler its slice of the compiled event timeline and reads
/// back rendered audio — the sampler itself still knows nothing about the graph.
///
/// `store` is shared (`Arc`) rather than owned: TDD §7.7 requires sample data be
/// shared across every patch/channel referencing the same file, so many
/// `SamplerNode`s legitimately point at the same store. **M0 scope:** the store
/// is populated before it's handed to the RT thread and never mutated after —
/// there's no synchronisation for a live re-import while a `SamplerNode` holding
/// it is already playing.
pub struct SamplerNode {
    sampler: Sampler,
    store: Arc<SampleStore>,
    /// Two channels' worth, laid out back to back and split in `process`.
    /// Sized in `prepare`, so `process` never allocates (INVARIANT 1). See the
    /// note in `process` for why the render can't go straight to the bus.
    scratch: Vec<f32>,
    /// Frames per channel in `scratch` — its length is twice this.
    scratch_frames: usize,
}

impl SamplerNode {
    pub fn new(sampler: Sampler, store: Arc<SampleStore>) -> Self {
        Self {
            sampler,
            store,
            scratch: Vec::new(),
            scratch_frames: 0,
        }
    }
}

impl AudioNode for SamplerNode {
    fn prepare(&mut self, ctx: &PrepareContext) {
        self.sampler.prepare(&fontelle_core::PrepareContext {
            sample_rate: ctx.sample_rate,
            max_block_size: ctx.max_block_size,
        });
        self.scratch_frames = ctx.max_block_size as usize;
        self.scratch.resize(self.scratch_frames * MAX_CHANNELS, 0.0);
    }

    fn process(&mut self, ctx: &mut ProcessContext) {
        for (origin, event) in ctx.events_with_origin() {
            match &event.payload {
                fontelle_types::EventPayload::NoteOn {
                    key,
                    velocity,
                    voice_context,
                } => {
                    // The origin is recorded on the voice, so a transport stop
                    // can cut what the song started without cutting what the
                    // player is holding.
                    self.sampler
                        .note_on_from(*key, *velocity, *voice_context, origin);
                }
                fontelle_types::EventPayload::NoteOff { key, voice_context } => {
                    self.sampler.note_off(*key, *voice_context);
                }
                _ => {}
            }
        }
        // Rendered into scratch and added, not written straight to the bus:
        // several instruments share one output, and `Sampler::render` clears
        // what it is given because that is the contract a plugin host expects
        // of `fontelle-core`'s boundary (TDD §8.1). The scratch buffers are
        // allocated in `prepare`, never here (INVARIANT 1).
        //
        // The sampler renders as many channels as the bus has, up to a stereo
        // pair, so a panned layer arrives placed rather than centred. A bus
        // wider than two gets the pair fanned across it — which is what
        // feeding a stereo source into a wider bus means, and is the only
        // thing this node can honestly do until surround is a real feature.
        let frames = ctx.outputs.first().map_or(0, |o| o.len());
        let frames = frames.min(self.scratch_frames);
        let channels = ctx.outputs.len().min(MAX_CHANNELS);
        let (first, second) = self.scratch.split_at_mut(self.scratch_frames);
        let mut rendered: [&mut [f32]; MAX_CHANNELS] =
            [&mut first[..frames], &mut second[..frames]];
        self.sampler.render(&self.store, &mut rendered[..channels]);

        for (index, channel) in ctx.outputs.iter_mut().enumerate() {
            let source = &rendered[index.min(channels.saturating_sub(1))];
            for (out, sample) in channel[..frames].iter_mut().zip(source.iter()) {
                *out += *sample;
            }
        }
    }

    fn reset(&mut self) {
        // A hard cut, not a release: a release tail from before a seek would
        // play over the top of wherever playback landed.
        self.sampler.reset();
        self.scratch.fill(0.0);
    }

    fn reset_sequenced(&mut self) {
        // Transport stop and seek. Same hard cut, but only for the voices the
        // timeline started — the ones a player is holding belong to them, and
        // stop is a statement about the sequencer.
        self.sampler.reset_sequenced();
        self.scratch.fill(0.0);
    }

    fn params(&self) -> &dyn ParamSet {
        &EmptyParams
    }
}

/// Wraps one `fontelle-fx` effect. Which effect is behind `Box<dyn ...>` is decided
/// when the node is built from the document's `EffectSlot` — `fontelle-fx` itself
/// exposes no shared trait, since it must not depend on this crate (TDD §4.1).
pub struct EffectNode {
    // Concrete effect + its ParamSet adapter land alongside the mixer/insert wiring
    // in M4.
}

/// A mixer track's fader stage (TDD §13.1): gain, pan, mute, phase invert.
///
/// **Processes in place.** Its buffers arrive already carrying the signal
/// feeding it (see `CompiledGraph::process_block`'s in-place convention), so
/// it reads, scales, and writes back the same buffers rather than copying
/// between an input and an output set. This is how an insert chain works in
/// every plugin API, and it's what lets the RT thread avoid both a copy and
/// the disjoint-borrow problem entirely.
///
/// Stereo when given two buffers (`[left, right]`), mono when given one — in
/// mono, `pan` has nowhere to go and is ignored rather than silently
/// half-attenuating the signal.
///
/// **Not yet:** inserts and sends (TDD §13.1's `inserts`/`sends`), metering
/// (§13.3), solo. Those are M4; this is the fader only, which is what the M0
/// gate's "→ mixer track →" actually requires.
pub struct MixerTrackNode {
    pub gain_db: f32,
    /// -1.0 hard left, 0.0 centre, +1.0 hard right.
    pub pan: f32,
    pub pan_law: PanLaw,
    pub mute: bool,
    pub phase_invert: bool,
}

impl MixerTrackNode {
    /// Unity gain, centred, unmuted — a track that passes audio through
    /// unchanged.
    pub fn new() -> Self {
        Self {
            gain_db: 0.0,
            pan: 0.0,
            pan_law: PanLaw::Minus3Db,
            mute: false,
            phase_invert: false,
        }
    }
}

impl Default for MixerTrackNode {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioNode for MixerTrackNode {
    fn prepare(&mut self, _ctx: &PrepareContext) {}

    fn process(&mut self, ctx: &mut ProcessContext) {
        if self.mute {
            for channel in ctx.outputs.iter_mut() {
                channel.fill(0.0);
            }
            return;
        }

        let mut gain = 10f32.powf(self.gain_db / 20.0);
        if self.phase_invert {
            gain = -gain;
        }

        // Pan only means something with two channels to balance between. On a
        // mono track there's nowhere for it to go, so applying the centre
        // pan-law gain would just make every mono track quietly -3dB down.
        if ctx.outputs.len() == 2 {
            let (left_gain, right_gain) = self.pan_law.gains(self.pan);
            let (left, right) = ctx.outputs.split_at_mut(1);
            for sample in left[0].iter_mut() {
                *sample *= gain * left_gain;
            }
            for sample in right[0].iter_mut() {
                *sample *= gain * right_gain;
            }
        } else {
            for channel in ctx.outputs.iter_mut() {
                for sample in channel.iter_mut() {
                    *sample *= gain;
                }
            }
        }
    }

    fn reset(&mut self) {}

    fn params(&self) -> &dyn ParamSet {
        &EmptyParams
    }
}

/// Adds one bus into another — the compiled form of `MixerTrack::output`
/// (TDD §13.1), which is how every track in the song reaches the master.
///
/// The only node in the graph whose inputs are a different set from its
/// outputs, and the reason `CompiledGraph::process_block` supports that shape
/// at all. It **adds** rather than overwrites, because a destination bus has
/// as many tracks arriving at it as the user routed there, and it leaves its
/// source untouched, because a bus may be routed *and* tapped by a send.
///
/// Stateless and parameterless on purpose: a send (§13.2) is this plus a level
/// and a pan, and that is M4 work along with the rest of the send system.
pub struct BusSumNode;

impl AudioNode for BusSumNode {
    fn prepare(&mut self, _ctx: &PrepareContext) {}

    fn process(&mut self, ctx: &mut ProcessContext) {
        for (source, dest) in ctx.inputs.iter().zip(ctx.outputs.iter_mut()) {
            for (sample, out) in source.iter().zip(dest.iter_mut()) {
                *out += *sample;
            }
        }
    }

    fn reset(&mut self) {}

    fn params(&self) -> &dyn ParamSet {
        &EmptyParams
    }
}

pub struct SendNode {
    // Pre/post-fader tap to another track. TDD §13.2.
}

pub struct AudioClipNode {
    // TDD §15 (M6).
}

/// What the master bus publishes for a meter to read: peak per channel and
/// the most gain reduction the limiter applied, in positive decibels.
///
/// Shared with the RT thread as plain atomics rather than through the
/// downsampled ring TDD §13.3 describes, because these are three scalars
/// rather than a waveform: a relaxed store per block is cheaper than a ring,
/// and there is nothing here whose *history* matters. Reading a value takes
/// it — the reader is the one that knows when it has drawn what it read.
#[derive(Debug, Default)]
pub struct MasterMeter {
    peaks: [std::sync::atomic::AtomicU32; MAX_CHANNELS],
    max_reduction_db: std::sync::atomic::AtomicU32,
}

impl MasterMeter {
    /// The highest peak since this was last called, per channel, and resets.
    pub fn take_peaks(&self) -> [f32; MAX_CHANNELS] {
        std::array::from_fn(|index| {
            f32::from_bits(self.peaks[index].swap(0, std::sync::atomic::Ordering::Relaxed))
        })
    }

    /// The most gain reduction since this was last called, in positive
    /// decibels, and resets. Zero means the limiter never engaged.
    pub fn take_max_reduction_db(&self) -> f32 {
        f32::from_bits(
            self.max_reduction_db
                .swap(0, std::sync::atomic::Ordering::Relaxed),
        )
    }

    /// RT: a load, a compare and a store. Single writer, so the read-modify-
    /// write needs no compare-exchange loop.
    fn record(&self, index: usize, value: f32) {
        let Some(slot) = self.peaks.get(index) else {
            return;
        };
        let current = f32::from_bits(slot.load(std::sync::atomic::Ordering::Relaxed));
        if value > current {
            slot.store(value.to_bits(), std::sync::atomic::Ordering::Relaxed);
        }
    }

    fn record_reduction(&self, value: f32) {
        let current = f32::from_bits(
            self.max_reduction_db
                .load(std::sync::atomic::Ordering::Relaxed),
        );
        if value > current {
            self.max_reduction_db
                .store(value.to_bits(), std::sync::atomic::Ordering::Relaxed);
        }
    }
}

/// The master bus: a brickwall limiter, then peak/RMS metering (TDD §13.3).
///
/// **Processes in place**, at the end of the schedule, after every track has
/// summed into the master pair.
///
/// The limiter is here rather than in an insert slot because it is not an
/// effect the user chose — it is the thing that makes "play any file and it
/// does not clip" true without a judgement about the material. Bypassable, for
/// when a mix is going somewhere that wants the peaks intact.
///
/// **Not yet:** LUFS-M/S/I and true-peak metering, which §13.3 also asks of
/// the master. Both need their own filters and an oversampled peak detector;
/// the peak/RMS pair is what a fader needs to be usable.
pub struct MasterNode {
    limiter: fontelle_fx::Limiter,
    pub limiter_config: fontelle_fx::LimiterConfig,
    pub limiter_enabled: bool,
    meters: [fontelle_dsp::PeakRmsMeter; MAX_CHANNELS],
    /// The half of the metering anything off the RT thread can read.
    published: Arc<MasterMeter>,
    /// Kept from `prepare` so `reset` can rebuild the limiter without being
    /// handed a `PrepareContext` it has no way to obtain.
    sample_rate: f32,
}

impl MasterNode {
    pub fn new() -> Self {
        Self {
            limiter: fontelle_fx::Limiter::new(),
            limiter_config: fontelle_fx::LimiterConfig::default(),
            limiter_enabled: true,
            meters: [fontelle_dsp::PeakRmsMeter::new(); MAX_CHANNELS],
            published: Arc::new(MasterMeter::default()),
            sample_rate: 48_000.0,
        }
    }

    /// A handle on the master's levels that outlives handing this node to the
    /// RT thread — which is the only way anything can read them once the graph
    /// is in the audio callback.
    pub fn meter(&self) -> Arc<MasterMeter> {
        self.published.clone()
    }

    /// Peak and RMS per channel, for a meter. The peak is held until
    /// [`MasterNode::reset_peaks`].
    pub fn channel_meter(&self, channel: usize) -> Option<&fontelle_dsp::PeakRmsMeter> {
        self.meters.get(channel)
    }

    pub fn reset_peaks(&mut self) {
        for meter in &mut self.meters {
            meter.reset_peak();
            meter.clear_clip_latch();
        }
    }
}

impl Default for MasterNode {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioNode for MasterNode {
    fn prepare(&mut self, ctx: &PrepareContext) {
        self.sample_rate = ctx.sample_rate;
        self.limiter.prepare(ctx.sample_rate, &self.limiter_config);
    }

    fn process(&mut self, ctx: &mut ProcessContext) {
        if self.limiter_enabled {
            self.limiter.process(ctx.outputs, &self.limiter_config);
        }
        // Metered *after* the limiter, because what the meter is for is
        // showing what left the machine.
        for (index, (meter, channel)) in self.meters.iter_mut().zip(ctx.outputs.iter()).enumerate()
        {
            meter.process_block(channel);
            self.published.record(index, meter.peak());
        }
        self.published
            .record_reduction(self.limiter.take_max_reduction_db());
    }

    fn reset(&mut self) {
        // Re-preparing is what clears the delay line and the gain state; a
        // separate "flush" would be one more thing to keep in step with it.
        let config = self.limiter_config;
        self.limiter.prepare(self.sample_rate, &config);
        self.reset_peaks();
    }

    fn latency_samples(&self) -> u32 {
        if self.limiter_enabled {
            self.limiter.latency_samples()
        } else {
            0
        }
    }

    fn params(&self) -> &dyn ParamSet {
        &EmptyParams
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fontelle_core::{
        FilterSlot, Layer, LoopMode, ModMatrix, Patch, PlaybackConfig, Source, VoiceConfig,
    };
    use fontelle_dsp::{EnvelopeConfig, EnvelopeCurve, Interpolation, SvfMode};
    use fontelle_types::{EventPayload, TimedEvent};

    use crate::transport::{TransportSnapshot, TransportState};

    const SR: f32 = 48_000.0;

    fn test_patch(store: &mut SampleStore) -> Patch {
        let asset = store.insert(fontelle_core::SampleBuffer {
            data: Arc::from(vec![1.0; 10_000]),
            sample_rate: SR as u32,
        });
        let disabled_filter = FilterSlot {
            mode: SvfMode::Lowpass,
            cutoff_hz: 20_000.0,
            resonance: 0.0,
            enabled: false,
        };
        let instant = EnvelopeConfig {
            delay_s: 0.0,
            attack_s: 0.0,
            hold_s: 0.0,
            decay_s: 0.0,
            sustain_level: 1.0,
            release_s: 0.01,
            curve: EnvelopeCurve::Linear,
        };
        Patch {
            layers: vec![Layer {
                source: Source::Sample { file: asset },
                key_range: (0, 127),
                vel_range: (0, 127),
                root_key: 60,
                fine_tune_cents: 0.0,
                playback: PlaybackConfig {
                    loop_mode: LoopMode::Off,
                    interpolation: Some(Interpolation::Draft),
                    end_offset: 10_000.0,
                    ..PlaybackConfig::default()
                },
                gain_db: 0.0,
                pan: 0.0,
            }],
            filters: [disabled_filter, disabled_filter],
            envelopes: vec![instant, instant],
            lfos: Vec::new(),
            mod_matrix: ModMatrix::default(),
            voice_config: VoiceConfig::default(),
        }
    }

    fn rms(buf: &[f32]) -> f32 {
        (buf.iter().map(|s| s * s).sum::<f32>() / buf.len() as f32).sqrt()
    }

    /// Drives a `MixerTrackNode` over buffers pre-filled with `input`,
    /// returning what it left behind (it processes in place).
    fn run_mixer(node: &mut MixerTrackNode, channels: usize, input: f32) -> Vec<Vec<f32>> {
        let mut buffers: Vec<Vec<f32>> = (0..channels).map(|_| vec![input; 64]).collect();
        {
            let mut slices: Vec<&mut [f32]> =
                buffers.iter_mut().map(|b| b.as_mut_slice()).collect();
            let mut ctx = ProcessContext {
                inputs: &[],
                outputs: &mut slices,
                all_events: &[],
                live_events: &[],
                node: fontelle_types::NodeId::default(),
                transport: TransportSnapshot {
                    state: TransportState::Playing,
                    position_sample: 0,
                },
                sample_range: 0..64,
            };
            node.process(&mut ctx);
        }
        buffers
    }

    /// A centred stereo track is *not* transparent, and shouldn't be: the
    /// default -3dB pan law puts 0.707 on each side so that total power is
    /// preserved as a source pans across the field. Asserting "unchanged"
    /// here would be asserting that the pan law does nothing.
    #[test]
    fn a_centred_stereo_track_applies_the_constant_power_pan_law() {
        let mut node = MixerTrackNode::new();
        let out = run_mixer(&mut node, 2, 1.0);

        let expected = std::f32::consts::FRAC_1_SQRT_2; // -3dB
        for (channel, samples) in out.iter().enumerate() {
            for &s in samples {
                assert!(
                    (s - expected).abs() < 1e-5,
                    "channel {channel}: centre pan at -3dB law should read {expected}, got {s}"
                );
            }
        }

        let power = out[0][0] * out[0][0] + out[1][0] * out[1][0];
        assert!(
            (power - 1.0).abs() < 1e-5,
            "the whole point of the -3dB law: total power stays 1.0, got {power}"
        );
    }

    /// The transparency claim belongs to a *mono* track, where no pan law
    /// applies — unity gain in, unity gain out, bit for bit.
    #[test]
    fn a_default_mono_track_passes_audio_through_unchanged() {
        let mut node = MixerTrackNode::new();
        let out = run_mixer(&mut node, 1, 0.5);
        for &s in &out[0] {
            assert_eq!(s, 0.5, "a default mono track must be bit-transparent");
        }
    }

    #[test]
    fn gain_db_scales_the_signal() {
        let mut node = MixerTrackNode {
            gain_db: -6.0,
            ..MixerTrackNode::new()
        };
        let out = run_mixer(&mut node, 1, 1.0);
        let expected = 10f32.powf(-6.0 / 20.0);
        assert!(
            (out[0][0] - expected).abs() < 1e-4,
            "-6dB should scale 1.0 to ~{expected}, got {}",
            out[0][0]
        );
    }

    #[test]
    fn mute_silences_every_channel() {
        let mut node = MixerTrackNode {
            mute: true,
            ..MixerTrackNode::new()
        };
        let out = run_mixer(&mut node, 2, 1.0);
        for channel in &out {
            assert!(
                channel.iter().all(|&s| s == 0.0),
                "mute must zero the buffer"
            );
        }
    }

    #[test]
    fn phase_invert_flips_the_sign_without_changing_magnitude() {
        let mut node = MixerTrackNode {
            phase_invert: true,
            ..MixerTrackNode::new()
        };
        let out = run_mixer(&mut node, 1, 0.25);
        assert!(
            (out[0][0] + 0.25).abs() < 1e-6,
            "phase invert should give -0.25, got {}",
            out[0][0]
        );
    }

    #[test]
    fn panning_hard_left_silences_the_right_channel() {
        let mut node = MixerTrackNode {
            pan: -1.0,
            ..MixerTrackNode::new()
        };
        let out = run_mixer(&mut node, 2, 1.0);
        assert!(
            (out[0][0] - 1.0).abs() < 1e-5,
            "left must stay at full scale, got {}",
            out[0][0]
        );
        assert!(
            out[1][0].abs() < 1e-5,
            "right must be silent, got {}",
            out[1][0]
        );
    }

    /// A mono track has nowhere to pan to. Attenuating by the centre pan-law
    /// gain anyway would make every mono track quietly 3dB down for no reason
    /// the user can see.
    #[test]
    fn pan_is_ignored_on_a_mono_track_rather_than_attenuating_it() {
        let mut node = MixerTrackNode {
            pan: 0.0,
            ..MixerTrackNode::new()
        };
        let out = run_mixer(&mut node, 1, 1.0);
        assert!(
            (out[0][0] - 1.0).abs() < 1e-6,
            "a centred mono track must stay at unity, got {}",
            out[0][0]
        );
    }

    #[test]
    fn a_note_on_event_produces_sound_through_the_audio_node_interface() {
        let mut store = SampleStore::new();
        let patch = test_patch(&mut store);
        let store = Arc::new(store);

        let mut node = SamplerNode::new(Sampler::new(patch), store);
        node.prepare(&PrepareContext {
            sample_rate: SR,
            max_block_size: 128,
        });

        let events = [TimedEvent {
            sample: 0,
            target: fontelle_types::NodeId::default(),
            payload: EventPayload::NoteOn {
                key: 60,
                velocity: 127,
                voice_context: 0,
            },
        }];

        let mut channel = vec![0.0; 128];
        {
            let mut out_slices: Vec<&mut [f32]> = vec![&mut channel];
            let mut ctx = ProcessContext {
                inputs: &[],
                outputs: &mut out_slices,
                all_events: &events,
                live_events: &[],
                node: fontelle_types::NodeId::default(),
                transport: TransportSnapshot {
                    state: TransportState::Playing,
                    position_sample: 0,
                },
                sample_range: 0..128,
            };
            node.process(&mut ctx);
        }

        assert!(
            rms(&channel) > 0.5,
            "expected near-full-scale output from the NoteOn, got rms {}",
            rms(&channel)
        );
    }

    /// Drives a `MasterNode` over buffers pre-filled with `input`.
    fn run_master(node: &mut MasterNode, frames: usize, input: f32) -> Vec<Vec<f32>> {
        let mut buffers: Vec<Vec<f32>> = (0..2).map(|_| vec![input; frames]).collect();
        {
            let mut slices: Vec<&mut [f32]> =
                buffers.iter_mut().map(|b| b.as_mut_slice()).collect();
            let mut ctx = ProcessContext {
                inputs: &[],
                outputs: &mut slices,
                all_events: &[],
                live_events: &[],
                node: fontelle_types::NodeId::default(),
                transport: TransportSnapshot {
                    state: TransportState::Playing,
                    position_sample: 0,
                },
                sample_range: 0..frames as i64,
            };
            node.process(&mut ctx);
        }
        buffers
    }

    /// The whole reason the master track exists: an arrangement summing onto
    /// one bus peaks wherever the material puts it, and a fader set by hand
    /// either clips or throws away headroom.
    #[test]
    fn the_master_holds_the_bus_under_full_scale() {
        let mut node = MasterNode::new();
        node.prepare(&PrepareContext {
            sample_rate: SR,
            max_block_size: 4_096,
        });
        let out = run_master(&mut node, 4_096, 3.0);
        let peak = out.iter().flatten().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(
            peak <= node.limiter_config.ceiling + 1e-4,
            "3x full scale in should come out at the ceiling, got {peak}"
        );
    }

    #[test]
    fn a_bypassed_master_limiter_is_transparent() {
        let mut node = MasterNode {
            limiter_enabled: false,
            ..MasterNode::new()
        };
        node.prepare(&PrepareContext {
            sample_rate: SR,
            max_block_size: 256,
        });
        let out = run_master(&mut node, 256, 0.5);
        assert!(out[0].iter().all(|&s| s == 0.5));
        assert_eq!(node.latency_samples(), 0, "and it costs no latency either");
    }

    /// Metered after the limiter, because what a master meter is for is
    /// showing what left the machine.
    #[test]
    fn the_master_meters_what_it_actually_output() {
        let mut node = MasterNode::new();
        node.prepare(&PrepareContext {
            sample_rate: SR,
            max_block_size: 4_096,
        });
        run_master(&mut node, 4_096, 3.0);

        let meter = node
            .channel_meter(0)
            .expect("a stereo master has a left meter");
        assert!(
            meter.peak() <= node.limiter_config.ceiling + 1e-4,
            "the meter must read the limited signal, not the 3.0 that arrived: \
             {}",
            meter.peak()
        );
        assert!(
            !meter.clip_latched(),
            "and nothing should have clipped in the first place"
        );
        // The published handle is the only way to read this once the node is
        // inside the graph on the RT thread, so it is what the test reads.
        let published = node.meter();
        assert!(
            published.take_max_reduction_db() > 5.0,
            "the limiter worked hard and must be able to say so"
        );
        assert_eq!(
            published.take_max_reduction_db(),
            0.0,
            "reading it resets it, or a meter shows the loudest moment of the \
             session forever"
        );
    }

    /// The meter handle has to be taken *before* the node is boxed into the
    /// schedule and handed to the audio thread, and keep working afterwards.
    #[test]
    fn the_master_meter_handle_outlives_handing_the_node_to_the_graph() {
        let mut node = MasterNode::new();
        node.prepare(&PrepareContext {
            sample_rate: SR,
            max_block_size: 256,
        });
        let meter = node.meter();

        let mut boxed: Box<dyn AudioNode> = Box::new(node);
        let mut buffers: Vec<Vec<f32>> = (0..2).map(|_| vec![0.5; 256]).collect();
        {
            let mut slices: Vec<&mut [f32]> =
                buffers.iter_mut().map(|b| b.as_mut_slice()).collect();
            let mut ctx = ProcessContext {
                inputs: &[],
                outputs: &mut slices,
                all_events: &[],
                live_events: &[],
                node: fontelle_types::NodeId::default(),
                transport: TransportSnapshot {
                    state: TransportState::Playing,
                    position_sample: 0,
                },
                sample_range: 0..256,
            };
            boxed.process(&mut ctx);
        }

        let peaks = meter.take_peaks();
        assert!(
            (peaks[0] - 0.5).abs() < 1e-6,
            "the handle must still be reading the node's output, got {}",
            peaks[0]
        );
        assert_eq!(meter.take_peaks()[0], 0.0, "and reading it resets it");
    }
}
