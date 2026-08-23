use std::fmt;

/// The stable string address of an automatable parameter (TDD §8.2, INVARIANT 7).
/// Serves plugin export, DAW automation targets, preset serialisation, MIDI learn,
/// and undo command targets — one addressing scheme for all five. Never changes
/// across versions once assigned.
///
/// Examples: `channel:<uuid>/patch/layer[2]/filter1.cutoff`, `transport/tempo`.
#[derive(
    Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub struct ParamAddress(String);

impl ParamAddress {
    pub fn new(address: impl Into<String>) -> Self {
        Self(address.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ParamAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for ParamAddress {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}
