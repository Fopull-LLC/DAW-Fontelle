use std::sync::Arc;

use fontelle_core::{SampleStore, Sampler};
use fontelle_types::{PanLaw, ParamAddress};

use crate::graph::{AudioNode, ParamSet, PrepareContext, ProcessContext};

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
}

impl SamplerNode {
    pub fn new(sampler: Sampler, store: Arc<SampleStore>) -> Self {
        Self { sampler, store }
    }
}

impl AudioNode for SamplerNode {
    fn prepare(&mut self, ctx: &PrepareContext) {
        self.sampler.prepare(&fontelle_core::PrepareContext {
            sample_rate: ctx.sample_rate,
            max_block_size: ctx.max_block_size,
        });
    }

    fn process(&mut self, ctx: &mut ProcessContext) {
        for event in ctx.events {
            match &event.payload {
                fontelle_types::EventPayload::NoteOn {
                    key,
                    velocity,
                    voice_context,
                } => {
                    self.sampler.note_on(*key, *velocity, *voice_context);
                }
                fontelle_types::EventPayload::NoteOff { key, voice_context } => {
                    self.sampler.note_off(*key, *voice_context);
                }
                _ => {}
            }
        }
        // `fontelle-core` renders mono today (`Layer::pan` isn't applied
        // per-voice yet — see PROGRESS.md). Render once into the first
        // channel, then copy it across the rest, which is exactly what
        // feeding a mono source into a stereo bus means. When the sampler
        // grows real per-voice panning this becomes a true stereo render
        // rather than a duplicate.
        let Some((first, rest)) = ctx.outputs.split_first_mut() else {
            return;
        };
        self.sampler.render(&self.store, first);
        for channel in rest {
            channel.copy_from_slice(first);
        }
    }

    fn reset(&mut self) {
        todo!("silence all active voices")
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

pub struct SendNode {
    // Pre/post-fader tap to another track. TDD §13.2.
}

pub struct AudioClipNode {
    // TDD §15 (M6).
}

pub struct MasterNode {
    // Metering (peak/RMS + LUFS/true-peak) lives here. TDD §13.3.
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
                events: &[],
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
                events: &events,
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
}
