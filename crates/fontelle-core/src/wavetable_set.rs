//! The wavetables one patch needs, resolved before it plays.
//!
//! # The whole reason this type exists
//!
//! INVARIANT 1: **no allocation on the RT thread**. Building a wavetable
//! allocates half a megabyte and asking [`fontelle_dsp::WavetableBank`] for
//! one takes a lock. Both are perfectly fine in `Sampler::prepare`, which runs
//! off the audio thread, and forbidden in `render`, which does not.
//!
//! So the sampler walks the patch's layers once in `prepare`, pulls an `Arc`
//! for every table any of them names, and hands the resulting set to every
//! voice it renders. A voice looks a table up by a linear scan of at most a
//! handful of entries — no lock, no allocation, no hashing.
//!
//! # What a missing table means
//!
//! Silence for that layer, and nothing else. It happens when a patch names a
//! table and the set was built from a different patch — which is a bug, not a
//! user's problem — so the honest behaviour is the quiet one: a wrong sound is
//! harder to diagnose than no sound.

use std::sync::Arc;

use fontelle_dsp::{Wavetable, WavetableId, wavetables};

use crate::patch::{Patch, Source};

/// Every table one patch's layers name, resolved to live `Arc`s.
#[derive(Debug, Default, Clone)]
pub struct WavetableSet {
    /// A `Vec` of pairs rather than a map: a Flopsynth patch names at most
    /// five tables, and a linear scan over five is faster than hashing one —
    /// and it is the whole of what `get` has to be RT-safe about.
    entries: Vec<(WavetableId, Arc<Wavetable>)>,
    /// The patch's **own** tables, built from sounds somebody dropped in —
    /// `Patch::wavetables`, in the same order, so an index is the name.
    ///
    /// Built here rather than in the process-wide bank because they belong to
    /// one patch: two projects that both dropped a file called `pad.wav` are
    /// two different sounds, and a shared cache keyed by name would hand one
    /// of them the other's.
    ///
    /// `None` for a table this patch does not name, so building one costs
    /// nothing on a patch that only reads the bank — which is every factory
    /// preset.
    user: Vec<Option<Arc<Wavetable>>>,
    /// The patch's own **recordings**, `Patch::samples` in the same order —
    /// the ones a layer names, cloned here, which is a handful of `Arc`s
    /// (see `SampleZone::samples`) and no audio.
    samples: Vec<Option<crate::UserSample>>,
}

impl WavetableSet {
    /// The empty set. What a patch with no synth layers needs, and what the
    /// `Voice::render` convenience wrappers pass.
    pub const EMPTY: Self = Self {
        entries: Vec::new(),
        user: Vec::new(),
        samples: Vec::new(),
    };

    pub fn new() -> Self {
        Self::default()
    }

    /// Builds the set `patch` needs. **Off the RT thread only** — this locks
    /// the process-wide bank and may build a table.
    pub fn resolve(&mut self, patch: &Patch) {
        self.entries.clear();
        self.user.clear();
        self.user.resize(patch.wavetables.len(), None);
        self.samples.clear();
        self.samples.resize(patch.samples.len(), None);
        for layer in &patch.layers {
            let Source::Synth(osc) = &layer.source else {
                continue;
            };
            match osc.source {
                fontelle_dsp::SynthSource::Table(id) => {
                    if self.entries.iter().any(|(known, _)| *known == id) {
                        continue;
                    }
                    self.entries.push((id, wavetables().get(id)));
                }
                // One of the patch's own. Built here, off the RT thread, from
                // the samples the patch carries — see `UserWavetable`. A
                // layer naming one the patch does not have is left unbuilt
                // and renders silence.
                fontelle_dsp::SynthSource::User(at) => {
                    let at = at as usize;
                    let Some(slot) = self.user.get_mut(at) else {
                        continue;
                    };
                    if slot.is_some() {
                        continue;
                    }
                    let table = &patch.wavetables[at];
                    *slot = Some(Arc::new(Wavetable::from_samples(
                        &table.samples,
                        table.frames,
                    )));
                }
                // One of the patch's own recordings: the `Arc`s to its
                // zones, so a voice can borrow the audio without a copy.
                fontelle_dsp::SynthSource::Sample(at) => {
                    let at = at as usize;
                    if let (Some(slot), Some(sample)) =
                        (self.samples.get_mut(at), patch.samples.get(at))
                        && slot.is_none()
                    {
                        *slot = Some(sample.clone());
                    }
                }
                fontelle_dsp::SynthSource::Noise | fontelle_dsp::SynthSource::String => {}
            }
        }
    }

    /// One of the **patch's own** recordings, or `None` if this set was not
    /// built for a patch naming it.
    ///
    /// RT-safe: an index and a pointer.
    pub fn get_sample(&self, index: usize) -> Option<&crate::UserSample> {
        self.samples.get(index)?.as_ref()
    }

    /// One table, or `None` if this set was not built for a patch naming it.
    ///
    /// RT-safe: a scan of a handful of entries and a pointer.
    pub fn get(&self, id: WavetableId) -> Option<&Wavetable> {
        self.entries
            .iter()
            .find(|(known, _)| *known == id)
            .map(|(_, table)| table.as_ref())
    }

    /// One of the **patch's own** tables, or `None` if this set was not built
    /// for a patch naming it.
    ///
    /// RT-safe: an index and a pointer.
    pub fn get_user(&self, index: usize) -> Option<&Wavetable> {
        self.user.get(index)?.as_deref()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::patch::Layer;
    use fontelle_dsp::{SynthOsc, SynthSource};

    fn synth_layer(id: WavetableId) -> Layer {
        Layer {
            source: Source::Synth(SynthOsc {
                source: SynthSource::Table(id),
                ..SynthOsc::default()
            }),
            key_range: (0, 127),
            vel_range: (0, 127),
            root_key: 60,
            fine_tune_cents: 0.0,
            playback: crate::playback::PlaybackConfig::default(),
            gain_db: 0.0,
            pan: 0.0,
        }
    }

    #[test]
    fn it_resolves_every_table_a_patch_names_and_no_others() {
        let patch = Patch {
            layers: vec![
                synth_layer(WavetableId::Saw),
                synth_layer(WavetableId::Choir),
                // Named twice: one entry, because a set is a set.
                synth_layer(WavetableId::Saw),
            ],
            ..Default::default()
        };
        let mut set = WavetableSet::new();
        set.resolve(&patch);
        assert_eq!(set.len(), 2);
        assert!(set.get(WavetableId::Saw).is_some());
        assert!(set.get(WavetableId::Choir).is_some());
        assert!(
            set.get(WavetableId::Gong).is_none(),
            "a table nothing names is one this patch does not pay for"
        );
    }

    #[test]
    fn a_noise_layer_names_no_table() {
        let patch = Patch {
            layers: vec![Layer {
                source: Source::Synth(SynthOsc {
                    source: SynthSource::Noise,
                    ..SynthOsc::default()
                }),
                ..synth_layer(WavetableId::Saw)
            }],
            ..Default::default()
        };
        let mut set = WavetableSet::new();
        set.resolve(&patch);
        assert!(set.is_empty());
    }

    #[test]
    fn resolving_again_replaces_rather_than_grows() {
        let mut patch = Patch {
            layers: vec![synth_layer(WavetableId::Saw)],
            ..Default::default()
        };
        let mut set = WavetableSet::new();
        set.resolve(&patch);
        patch.layers = vec![synth_layer(WavetableId::Glass)];
        set.resolve(&patch);
        assert_eq!(set.len(), 1);
        assert!(
            set.get(WavetableId::Saw).is_none(),
            "a set that only ever grew would hold every table a channel had \
             ever played"
        );
    }
}
