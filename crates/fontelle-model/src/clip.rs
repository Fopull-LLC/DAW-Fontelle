use fontelle_types::{ClipId, LaneId, Tick};

use crate::automation::AutomationData;
use crate::note::NoteData;
use crate::prefab::PrefabLink;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AudioClipData {
    // Fleshed out in TDD §15 (M6). Left as a marker variant until then so
    // `ClipSource` has its full v1 shape from commit one.
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum ClipSource {
    Notes(NoteData),
    Automation(AutomationData),
    Audio(AudioClipData),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Clip {
    pub lane: LaneId,
    pub start: Tick,
    pub length: Tick,
    pub source: ClipSource,
    pub prefab_link: Option<PrefabLink>,
    /// Overrides the source's own colour.
    pub color: Option<[u8; 4]>,
    pub muted: bool,
    /// How long the clip's content is before it **repeats**, in ticks.
    ///
    /// `None` is a clip that plays once, which is what every clip was before
    /// this existed. `Some(period)` is a genuine loop: one set of notes, played
    /// again every `period` until [`Clip::length`] runs out.
    ///
    /// # Looping is not copying
    ///
    /// The two look identical on the arrangement and are nothing alike
    /// underneath, and conflating them is the report this field answers:
    ///
    /// - **Copying** makes new clips with their own notes. Editing one leaves
    ///   the others alone, which is the point of it.
    /// - **Looping** is *one* clip whose content repeats. There is one set of
    ///   notes, so editing bar 1 changes every repeat — which is what "repeat
    ///   this drum pattern" means and what a copy can never give you.
    ///
    /// Defaulted, so a project written before loops existed opens as the
    /// play-once clips it was made of.
    #[serde(default)]
    pub loop_length: Option<Tick>,
}

impl Clip {
    /// How many times the content sounds, a partial repeat at the end
    /// included.
    ///
    /// One for a clip that does not loop, and never zero: a clip shorter than
    /// its own period still plays the front of it, which is what the
    /// arrangement draws and what the compiler emits.
    pub fn repeats(&self) -> usize {
        match self.loop_length {
            // Rounded up by hand: `i64::div_ceil` is not stable on this
            // project's MSRV. A partial repeat at the end counts, because it
            // is drawn and the notes inside it that start before the clip's
            // end still sound.
            Some(period) if period > 0 => {
                (((self.length.max(0) + period - 1) / period).max(1)) as usize
            }
            _ => 1,
        }
    }

    /// Where repeat `index` starts, in the clip's own ticks.
    pub fn repeat_start(&self, index: usize) -> Tick {
        match self.loop_length {
            Some(period) if period > 0 => period * index as Tick,
            _ => 0,
        }
    }
}

pub type ClipMap = crate::arena::Arena<ClipId, Clip>;
