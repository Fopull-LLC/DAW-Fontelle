use fontelle_types::ParamAddress;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueDistribution {
    Linear,
    Skewed,
    Stepped,
}

/// The parameter contract adapter (TDD §8.2, INVARIANT 7): every automatable
/// parameter carries a stable `ParamAddress` that never changes across versions.
/// This is `nice-plug`'s `Params` shape, adopted directly — the same addressing
/// scheme also serves DAW automation targets, preset serialisation, and MIDI
/// learn, so a second parallel addressing scheme anywhere is a design regression.
pub struct PluginParam {
    pub address: ParamAddress,
    pub display_name: String,
    pub range: (f64, f64),
    pub default: f64,
    pub unit: String,
    pub distribution: ValueDistribution,
}
