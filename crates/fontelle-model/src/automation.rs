pub use fontelle_types::CurveShape;
use fontelle_types::{ParamAddress, PointId, Tick};

use crate::arena::Arena;

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

impl AutomationData {
    /// The value at `tick`, in the clip's own time, normalised 0..1.
    ///
    /// `None` for a clip with no points in it — not zero, which would slam
    /// every automated parameter to its minimum the moment somebody made an
    /// empty clip.
    ///
    /// Sorted here rather than kept sorted, because a person draws points
    /// wherever they like and the arena keeps the order they were *made* in:
    /// the alternative is every insertion re-sorting a collection whose ids
    /// have to stay stable for undo. §12.1's "points are kept sorted;
    /// evaluation is binary search" is the shape this grows into when a lane
    /// with thousands of points exists to make it matter — the ordering is
    /// what has to be right, and it is.
    pub fn value_at(&self, tick: Tick) -> Option<f64> {
        let mut points: Vec<AutomationPoint> = self.points.iter().map(|(_, p)| *p).collect();
        points.sort_by_key(|p| p.tick);
        curve_value(&points, tick)
    }

    /// The last value this clip produces — what §12.2's second rule says the
    /// parameter holds after the clip has ended.
    pub fn final_value(&self) -> Option<f64> {
        // Asked of the curve rather than of the last point, so a `Hold`
        // part-way through gives the same answer here as it does inside the
        // clip. Two ways of working out the same number is a place for them
        // to disagree.
        let last = self.points.iter().map(|(_, p)| p.tick).max()?;
        self.value_at(last)
    }
}

/// The value of a curve at `tick`, from its points **already in time order**.
///
/// `AutomationData::value_at` is this over a sorted copy of the arena. It is
/// public on its own because the arrangement draws an automation block's
/// curve from a flattened list of its points (a canvas may not see a
/// `Project`, INVARIANT 2), and the picture in the block has to be the curve
/// the audio thread hears — two evaluators would be a place for them to
/// disagree, and a shape you chose that draws as a straight line is a shape
/// you cannot see.
///
/// `None` for no points — not zero, which would slam every automated
/// parameter to its minimum the moment somebody made an empty clip.
pub fn curve_value(sorted: &[AutomationPoint], tick: Tick) -> Option<f64> {
    if sorted.is_empty() {
        return None;
    }
    debug_assert!(
        sorted.windows(2).all(|w| w[0].tick <= w[1].tick),
        "curve_value takes points in time order"
    );
    // A `Hold` ends the curve where it sits: everything after it is that
    // value. Truncating here rather than special-casing below means the rest
    // of this function — and `final_value` — get it for free.
    let points = match sorted.iter().position(|p| p.curve.freezes()) {
        Some(freeze) => &sorted[..=freeze],
        None => sorted,
    };

    // Before the first point, the first point's value: a curve that began
    // somewhere else would be a value nobody drew.
    let first = points[0];
    if tick <= first.tick {
        return Some(first.value.clamp(0.0, 1.0));
    }
    let last = points[points.len() - 1];
    if tick >= last.tick {
        return Some(last.value.clamp(0.0, 1.0));
    }

    let index = points.partition_point(|p| p.tick <= tick) - 1;
    let (from, to) = (points[index], points[index + 1]);
    if from.curve.holds() {
        return Some(from.value.clamp(0.0, 1.0));
    }
    let span = (to.tick - from.tick).max(1) as f64;
    let t = from
        .curve
        .eased((tick - from.tick) as f64 / span, from.tension);
    Some((from.value + (to.value - from.value) * t).clamp(0.0, 1.0))
}
