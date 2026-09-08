//! A plugin's parameters, and the wire a knob reaches the audio thread on.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// One parameter, as the plugin describes it.
///
/// Owned `String`s rather than `&'static str`, which is the one place a hosted
/// plugin does not fit [`fontelle_types::ParamSpec`]: a built-in effect's
/// parameter names are in the binary, and a plugin's arrive over a C ABI at
/// run time. Everything else about the two is the same shape, which is why the
/// panel that draws them did not have to change.
#[derive(Debug, Clone, PartialEq)]
pub struct HostedParam {
    /// The plugin's own stable id — CLAP requires it never to change, which is
    /// INVARIANT 7 written from the other side of the boundary. This is what
    /// an automation lane addresses.
    pub id: u32,
    pub name: String,
    /// The plugin's grouping for it, `/`-separated ("Oscillators/Wavetable 1").
    /// Empty when it offered none.
    pub module: String,
    pub min: f64,
    pub max: f64,
    pub default: f64,
    /// Whole numbers only. A stepped parameter with two positions is a switch;
    /// with more, a chooser.
    pub stepped: bool,
    /// The plugin says not to show it.
    pub hidden: bool,
    /// The plugin says it cannot be set.
    pub readonly: bool,
}

impl HostedParam {
    /// Where `plain` sits on a 0..1 lane. Clamped, not extrapolated — the same
    /// rule [`fontelle_types::ParamSpec::normalise`] follows.
    pub fn normalise(&self, plain: f64) -> f64 {
        if self.max <= self.min {
            return 0.0;
        }
        ((plain - self.min) / (self.max - self.min)).clamp(0.0, 1.0)
    }

    /// What a lane at `normalised` means in the plugin's own units.
    ///
    /// A stepped parameter lands **on** a position rather than between two,
    /// which is what makes automating a filter type produce filter types.
    pub fn plain(&self, normalised: f64) -> f64 {
        let value = self.min + normalised.clamp(0.0, 1.0) * (self.max - self.min);
        if self.stepped { value.round() } else { value }
    }

    /// How many positions a stepped parameter has, `None` for a continuous
    /// one.
    ///
    /// Capped, because the number comes from the plugin: a "stepped"
    /// parameter running from zero to a million is one somebody meant as a
    /// count, and a chooser with a million rows is not a control.
    pub fn steps(&self) -> Option<u32> {
        if !self.stepped {
            return None;
        }
        let span = (self.max - self.min).round();
        (span >= 1.0 && span < MAX_STEPS as f64).then(|| span as u32 + 1)
    }
}

/// The most positions a stepped parameter may have and still be offered as a
/// list of them.
pub const MAX_STEPS: u32 = 256;

/// What every parameter is set to, shared between the window and the audio
/// thread.
///
/// **This is the only thing a knob touches.** A parameter change cannot be a
/// CLAP call from the window, because CLAP says parameter changes reach a
/// running plugin as events inside `process` — so a knob writes here, and the
/// processor turns whatever moved into events at the top of the next block.
/// It is the same shape `fontelle_engine::TrackControls` uses for a fader and
/// for the same reason: the document is still the source of truth, this is how
/// a drag is *heard* before the next rebuild.
///
/// Fixed at construction — one slot per parameter the plugin declared — so
/// nothing here ever allocates.
pub struct ParamValues {
    slots: Vec<Slot>,
}

struct Slot {
    id: u32,
    /// The plugin's range for it, so [`ParamValues::set_normalised`] can put a
    /// lane's fraction where it belongs. Kept here rather than looked up on
    /// the [`HostedParam`] list because the audio thread reads it: an
    /// automation event arrives at the node, and the node has this and not the
    /// plugin.
    min: f64,
    max: f64,
    stepped: bool,
    value: AtomicU64,
    moved: AtomicBool,
}

impl ParamValues {
    pub fn new(params: &[HostedParam]) -> Self {
        Self {
            slots: params
                .iter()
                .map(|param| Slot {
                    id: param.id,
                    min: param.min,
                    max: param.max,
                    stepped: param.stepped,
                    value: AtomicU64::new(param.default.to_bits()),
                    moved: AtomicBool::new(false),
                })
                .collect(),
        }
    }

    /// Sets one, and marks it for the next block. `false` if the plugin has no
    /// such parameter — INVARIANT 7's rule for an address this build does not
    /// recognise, applied to one coming the other way.
    pub fn set(&self, id: u32, value: f64) -> bool {
        let Some(slot) = self.slots.iter().find(|slot| slot.id == id) else {
            return false;
        };
        slot.value.store(value.to_bits(), Ordering::Relaxed);
        slot.moved.store(true, Ordering::Release);
        true
    }

    /// **RT-safe.** Sets one from a **0..1 lane position** rather than from a
    /// value in the plugin's own units.
    ///
    /// This is the one place the two conventions meet. Every automation lane
    /// in this program is normalised — that is what makes a curve drawn on one
    /// mean the same as a curve drawn on another — and a plugin's parameters
    /// are stored and set plain (see `fontelle_types::PluginParamValue`). A
    /// lane at half height on a gain that runs to four means two, and without
    /// this it meant 0.5.
    pub fn set_normalised(&self, id: u32, normalised: f64) -> bool {
        let Some(slot) = self.slots.iter().find(|slot| slot.id == id) else {
            return false;
        };
        let span = slot.max - slot.min;
        let plain = slot.min + normalised.clamp(0.0, 1.0) * span;
        let plain = if slot.stepped { plain.round() } else { plain };
        slot.value.store(plain.to_bits(), Ordering::Relaxed);
        slot.moved.store(true, Ordering::Release);
        true
    }

    pub fn get(&self, id: u32) -> Option<f64> {
        self.slots
            .iter()
            .find(|slot| slot.id == id)
            .map(|slot| f64::from_bits(slot.value.load(Ordering::Relaxed)))
    }

    /// Every parameter and its value, in the order the plugin declared them.
    pub fn all(&self) -> impl Iterator<Item = (u32, f64)> {
        self.slots
            .iter()
            .map(|slot| (slot.id, f64::from_bits(slot.value.load(Ordering::Relaxed))))
    }

    /// **RT.** Calls `take` with everything that has moved since the last
    /// block, and marks it seen.
    ///
    /// A flag per parameter rather than one for the set: a plugin with two
    /// hundred parameters would otherwise send two hundred events every time
    /// one knob moved, and a plugin is entitled to treat an event as a
    /// gesture.
    pub fn drain(&self, mut take: impl FnMut(u32, f64)) {
        for slot in &self.slots {
            if slot.moved.swap(false, Ordering::Acquire) {
                take(slot.id, f64::from_bits(slot.value.load(Ordering::Relaxed)));
            }
        }
    }

    /// Marks every parameter, so the next block sends the lot.
    ///
    /// What a freshly activated plugin needs: it starts at its own defaults,
    /// and the document's opinion has to arrive before the first sample.
    pub fn mark_all(&self) {
        for slot in &self.slots {
            slot.moved.store(true, Ordering::Release);
        }
    }

    pub fn len(&self) -> usize {
        self.slots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }
}
