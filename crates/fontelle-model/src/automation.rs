use fontelle_types::{ParamAddress, PointId, Tick};

use crate::arena::Arena;

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum CurveShape {
    Linear,
    Exponential,
    Logarithmic,
    SCurve,
    Stepped,
    Hold,
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct AutomationPoint {
    /// Relative to clip start.
    pub tick: Tick,
    /// Normalised 0..1.
    pub value: f64,
    /// Shape of the segment *following* this point.
    pub curve: CurveShape,
    pub tension: f32,
}

/// An automation clip (TDD §12.1). Placed on the timeline like any other clip, so
/// it can be a prefab — build a filter sweep once, instance it everywhere, edit the
/// source and every instance updates.
///
/// Two rules that must stay explicit and visible in the UI (TDD §12.2):
/// overlapping clips on the same target — the later one wins; outside a clip's
/// bounds, the target holds its last automated value rather than snapping back.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AutomationData {
    pub target: ParamAddress,
    pub points: Arena<PointId, AutomationPoint>,
}
