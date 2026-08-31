//! The automation editor: points on a curve, over time (TDD §12).
//!
//! Geometry and hit-testing, pure, per §2.5 of `docs/first-usable-plan.md`.
//!
//! It is the piano roll's shape with one axis changed: time runs across, and
//! down the side is a **value** rather than a keyboard. That is deliberate —
//! an automation clip is a clip, it opens in the editor column the way a note
//! clip opens in the roll, and the gestures are the ones already in the hand:
//! click empty space to make a point, drag it, drag a box round several,
//! delete.

use crate::layout::Rect;
use crate::theme::Metrics;
use fontelle_types::{PointId, Tick};

/// One point, as the editor draws it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointInfo {
    pub id: PointId,
    /// In the clip's own ticks.
    pub tick: Tick,
    /// Normalised, 0..1 — §12.1's unit.
    pub value: f64,
    pub selected: bool,
    pub curve: fontelle_model::CurveShape,
}

/// Everything the automation editor shows.
#[derive(Debug, Clone, PartialEq)]
pub struct AutomationView {
    /// What is being automated, in words — "Master — EQ band 1 gain".
    pub title: String,
    /// The clip's length, which is the width of the editor's time axis.
    pub length: Tick,
    pub points: Vec<PointInfo>,
}

/// The grab area of a point.
const HANDLE: f32 = 9.0;

/// Where everything is.
#[derive(Debug, Clone, PartialEq)]
pub struct AutomationLayout {
    pub body: Rect,
    /// The drawing area — what [`auto_x_of_tick`] and friends measure against.
    pub grid: Rect,
    /// A strip along the top naming the target.
    pub header: Rect,
    /// A handle per point, in the order the view listed them.
    pub handles: Vec<(PointId, Rect)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutomationHit {
    Point(PointId),
    /// Empty grid — where a click makes a point.
    Grid,
    Nothing,
}

/// What the editor asks the host to do. The canvas never touches a document
/// (INVARIANT 2); it produces these and the window turns each into a command.
#[derive(Debug, Clone, PartialEq)]
pub enum AutomationEdit {
    Add {
        tick: Tick,
        value: f64,
    },
    /// Deltas, **relative to the previous step of the same drag** — what
    /// `MoveAutomationPoints` takes and what lets a gesture merge.
    Move {
        ids: Vec<PointId>,
        tick_delta: Tick,
        value_delta: f64,
    },
    Remove(Vec<PointId>),
    SetCurve {
        ids: Vec<PointId>,
        curve: fontelle_model::CurveShape,
    },
}

pub fn auto_x_of_tick(grid: Rect, length: Tick, tick: Tick) -> f32 {
    if length <= 0 || grid.width <= 0.0 {
        return grid.x;
    }
    grid.x + (tick.clamp(0, length) as f32 / length as f32) * grid.width
}

pub fn auto_tick_at(grid: Rect, length: Tick, x: f32) -> Tick {
    if length <= 0 || grid.width <= 0.0 {
        return 0;
    }
    let t = ((x - grid.x) / grid.width).clamp(0.0, 1.0);
    (t * length as f32).round() as Tick
}

/// Up is more, which is the one convention nobody argues about.
pub fn auto_y_of_value(grid: Rect, value: f64) -> f32 {
    grid.bottom() - (value.clamp(0.0, 1.0) as f32) * grid.height
}

pub fn auto_value_at(grid: Rect, y: f32) -> f64 {
    if grid.height <= 0.0 {
        return 0.0;
    }
    f64::from(((grid.bottom() - y) / grid.height).clamp(0.0, 1.0))
}

pub fn automation_layout(body: Rect, metrics: &Metrics, view: &AutomationView) -> AutomationLayout {
    let header_height = metrics.row_height.min(body.height.max(0.0));
    let (header, grid) = body.split_top(header_height);
    let handles = view
        .points
        .iter()
        .map(|point| {
            let cx = auto_x_of_tick(grid, view.length, point.tick);
            let cy = auto_y_of_value(grid, point.value);
            (
                point.id,
                Rect::new(cx - HANDLE / 2.0, cy - HANDLE / 2.0, HANDLE, HANDLE),
            )
        })
        .collect();
    AutomationLayout {
        body,
        grid,
        header,
        handles,
    }
}

pub fn automation_hit(layout: &AutomationLayout, x: f32, y: f32) -> AutomationHit {
    // The last one first: points drawn later sit on top, so the one on top is
    // the one grabbed.
    if let Some((id, _)) = layout
        .handles
        .iter()
        .rev()
        .find(|(_, rect)| rect.contains(x, y))
    {
        return AutomationHit::Point(*id);
    }
    if layout.grid.contains(x, y) {
        return AutomationHit::Grid;
    }
    AutomationHit::Nothing
}

/// The curve, as points to draw a line through — the same numbers the audio
/// path reads, so what is on screen is what is heard.
///
/// Evenly spaced in pixels rather than in ticks, so the line is smooth where
/// it is looked at.
pub fn automation_curve(
    layout: &AutomationLayout,
    view: &AutomationView,
    data: &fontelle_model::AutomationData,
) -> Vec<(f32, f32)> {
    const STEPS: usize = 160;
    let grid = layout.grid;
    if grid.width <= 0.0 || grid.height <= 0.0 || view.length <= 0 {
        return Vec::new();
    }
    (0..=STEPS)
        .map(|step| {
            let x = grid.x + grid.width * step as f32 / STEPS as f32;
            let tick = auto_tick_at(grid, view.length, x);
            let value = data.value_at(tick).unwrap_or(0.0);
            (x, auto_y_of_value(grid, value))
        })
        .collect()
}

/// The next shape round, for a key that cycles one.
///
/// Ordered as they are used rather than as they are declared: linear is the
/// one you have, the eased ones are the ones you reach for, and the two flat
/// ones are the end of the list because they are the ones that stop a curve
/// being a curve.
pub fn next_curve(curve: fontelle_model::CurveShape) -> fontelle_model::CurveShape {
    use fontelle_model::CurveShape::*;
    match curve {
        Linear => SCurve,
        SCurve => Exponential,
        Exponential => Logarithmic,
        Logarithmic => Stepped,
        Stepped => Hold,
        Hold => Linear,
    }
}
