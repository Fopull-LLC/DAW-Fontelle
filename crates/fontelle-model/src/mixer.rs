use fontelle_types::{AudioInputId, MixerTrackId, ParamAddress};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PanLaw {
    /// -3dB, the default.
    Minus3Db,
    Minus4_5Db,
    Minus6Db,
    Linear,
}

/// A document-level reference to one effect instance in an insert chain. The DSP
/// itself (`fontelle-fx`) is instantiated and processed by `fontelle-engine`; the
/// model only stores which effect, its parameters, and whether it's bypassed.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EffectSlot {
    pub effect_id: ParamAddress,
    pub bypassed: bool,
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct Send {
    pub target: MixerTrackId,
    pub level_db: f32,
    pub pan: f32,
    pub pre_fader: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MixerTrack {
    pub name: String,
    pub color: [u8; 4],
    pub gain_db: f32,
    pub pan: f32,
    pub pan_law: PanLaw,
    pub mute: bool,
    pub solo: bool,
    pub phase_invert: bool,
    /// Ordered chain, each bypassable.
    pub inserts: Vec<EffectSlot>,
    /// May target any other track. The routing graph — both `output` and `sends` —
    /// must be validated acyclic on every mutation (TDD §13.2); reject the command,
    /// never let a feedback loop reach the graph compiler.
    pub sends: Vec<Send>,
    /// `None` = master.
    pub output: Option<MixerTrackId>,
    pub input: Option<AudioInputId>,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Mixer {
    pub tracks: slotmap::SlotMap<MixerTrackId, MixerTrack>,
    pub master: Option<MixerTrackId>,
}

impl Mixer {
    /// Depth-first search over `output` + `sends` for every track; must be run
    /// before committing any routing mutation.
    pub fn has_cycle(&self) -> bool {
        todo!("cycle check over output + sends per TDD §13.2")
    }
}
