use std::sync::Arc;

use fontelle_core::{SampleStore, Sampler};
use fontelle_types::ParamAddress;

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
        if let Some(out) = ctx.outputs.first_mut() {
            self.sampler.render(&self.store, out);
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

pub struct MixerTrackNode {
    // Fader, pan, sends. TDD §13.
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
    use fontelle_dsp::{EnvelopeConfig, Interpolation, SvfMode};
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
                    interpolation: Interpolation::Draft,
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
