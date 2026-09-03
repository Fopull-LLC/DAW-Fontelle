//! An automation clip's block on the arrangement: its anatomy, and the curve
//! drawn inside it (TDD §12, §16.4).
//!
//! Reported from using the window: *"i want the automation graph to be a
//! literal graph drawn inside the clip for that automation working like how
//! fl studio automation clips work in the arrangement."* Until this pass an
//! automation clip opened in a window of its own, and the window could not be
//! made to do anything.
//!
//! So a block has two parts. Across the top is a **caption band**, which is
//! the clip as every other clip is — grab it to move, select or erase the
//! block. Under it is the **curve area**, where the points live: a click on
//! bare curve makes a point, a drag moves one, and a right-click on one asks
//! for its shape. Geometry and hit-testing only, pure, per §2.5 of
//! `docs/first-usable-plan.md`; the gestures are `Timeline`'s and the edits
//! are the host's.

use fontelle_model::{AutomationPoint, curve_value};
use fontelle_types::{PointId, Tick};

use crate::document::{ClipInfo, CurvePoint};
use crate::layout::Rect;

/// How much air the curve keeps between itself and the block's own edges.
///
/// A point at 0.0 or 1.0 is drawn as a dot, and a dot centred on the edge is
/// half a dot — and half of it is over the lane next door.
const CURVE_INSET: f32 = 4.0;

/// The grab area of a point, in pixels.
const HANDLE: f32 = 8.0;

/// Where a block's parts are.
#[derive(Debug, Clone, PartialEq)]
pub struct AutomationBlock {
    /// The caption band. Pressing it is pressing the clip.
    pub header: Rect,
    /// The drawing area the curve is measured against: full width of the
    /// block less an inset, and the height under the band.
    pub area: Rect,
    /// A handle per point, in the order the clip listed them (time order).
    pub handles: Vec<(PointId, Rect)>,
}

/// Lays out `clip`'s block at `block`.
///
/// The curve keeps clear of the block's edges on three sides, and of the
/// **resize grip** on the fourth. The grip is what made the last point of
/// every automation clip ungrabbable: a clip is created flat with a point at
/// each end, the far one sits on the block's right-hand edge, and every press
/// aimed at it was a press on the grip. So the area stops half a handle short
/// of where the grip begins, which leaves the whole of that point's handle in
/// front of it. Sizing the block still works, and the point can be taken hold
/// of — see `tests/automation_blocks.rs`.
///
/// The vertical inset shrinks with the block rather than being a constant:
/// four pixels top and bottom out of a fourteen-pixel lane leaves less curve
/// than caption, which is the wrong way round on a block whose whole content
/// is its shape.
pub fn automation_block(block: Rect, clip: &ClipInfo) -> AutomationBlock {
    let (header, under) = crate::canvas::clip_bands(block);
    let right = crate::canvas::clip_grip(block) + HANDLE / 2.0;
    let vertical = (CURVE_INSET / 2.0).min(under.height / 6.0).max(0.0);
    // A block narrower than its own insets has no inside; drawing down its
    // middle is better than drawing nothing.
    let mut area = Rect::new(
        under.x + CURVE_INSET,
        under.y + vertical,
        under.width - CURVE_INSET - right,
        under.height - 2.0 * vertical,
    )
    .clamped();
    if area.is_empty() && !under.is_empty() {
        area = Rect::new(under.x, under.y + under.height / 2.0, under.width, 0.0);
    }
    let handles = clip
        .curve
        .iter()
        .map(|point| {
            let cx = block_x_of_tick(area, clip.length, point.tick);
            let cy = block_y_of_value(area, point.value);
            (
                point.id,
                Rect::new(cx - HANDLE / 2.0, cy - HANDLE / 2.0, HANDLE, HANDLE),
            )
        })
        .collect();
    AutomationBlock {
        header,
        area,
        handles,
    }
}

/// Where a tick of the clip is across its curve area.
pub fn block_x_of_tick(area: Rect, length: Tick, tick: Tick) -> f32 {
    if length <= 0 || area.width <= 0.0 {
        return area.x;
    }
    area.x + (tick.clamp(0, length) as f32 / length as f32) * area.width
}

/// The clip tick under `x`, **unsnapped**; the gesture snaps.
pub fn block_tick_at(area: Rect, length: Tick, x: f32) -> Tick {
    if length <= 0 || area.width <= 0.0 {
        return 0;
    }
    let t = ((x - area.x) / area.width).clamp(0.0, 1.0);
    (t as f64 * length as f64).round() as Tick
}

/// Up is more, which is the one convention nobody argues about.
pub fn block_y_of_value(area: Rect, value: f64) -> f32 {
    area.bottom() - (value.clamp(0.0, 1.0) as f32) * area.height
}

pub fn block_value_at(area: Rect, y: f32) -> f64 {
    if area.height <= 0.0 {
        return 0.0;
    }
    f64::from(((area.bottom() - y) / area.height).clamp(0.0, 1.0))
}

/// The curve, as points to draw a line through, in screen space.
///
/// Evaluated through the **same** function the audio thread's values come
/// from (`fontelle_model::curve_value`), so the shape in the block is the
/// shape that is heard — an S-curve draws bent, a hold draws flat. Sampled at
/// every point's own tick, so a corner is a corner, and every two pixels in
/// between, so a bend is smooth where it is looked at.
///
/// Everything is clamped into the block's curve area: nothing should produce
/// a point outside it, and one that escaped would be drawn over the lane
/// above.
pub fn automation_polyline(block: Rect, length: Tick, curve: &[CurvePoint]) -> Vec<(f32, f32)> {
    if block.is_empty() || curve.is_empty() {
        return Vec::new();
    }
    let area = automation_block(
        block,
        &ClipInfo {
            id: fontelle_types::ClipId::default(),
            lane: 0,
            start: 0,
            length,
            name: String::new(),
            muted: false,
            open: false,
            color: [0; 4],
            loop_length: None,
            kind: crate::document::ClipKind::Automation,
            curve: Vec::new(),
            notes: Vec::new(),
            audio: Default::default(),
        },
    )
    .area;
    let points: Vec<AutomationPoint> = curve
        .iter()
        .map(|p| AutomationPoint {
            tick: p.tick,
            value: p.value,
            curve: p.curve,
            tension: 0.0,
        })
        .collect();
    // A clip of no length is one instant: everything in it is at its left
    // edge rather than a division by zero.
    if length <= 0 {
        let value = curve_value(&points, 0).unwrap_or(0.0);
        return vec![(area.x, block_y_of_value(area, value))];
    }

    let mut ticks: Vec<Tick> = Vec::new();
    let steps = (area.width / 2.0).clamp(2.0, 400.0) as Tick;
    for step in 0..=steps {
        ticks.push(length * step / steps);
    }
    ticks.extend(curve.iter().map(|p| p.tick.clamp(0, length)));
    ticks.sort_unstable();
    ticks.dedup();

    ticks
        .into_iter()
        .map(|tick| {
            let value = curve_value(&points, tick).unwrap_or(0.0);
            (
                block_x_of_tick(area, length, tick).clamp(block.x, block.right()),
                block_y_of_value(area, value).clamp(block.y, block.bottom()),
            )
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

/// What each shape is called on a point's menu.
pub fn curve_label(curve: fontelle_model::CurveShape) -> &'static str {
    use fontelle_model::CurveShape::*;
    match curve {
        Linear => "Linear",
        SCurve => "Smooth",
        Exponential => "Ease in",
        Logarithmic => "Ease out",
        Stepped => "Step",
        Hold => "Hold",
    }
}

/// Every shape, in the order the menu lists them — the same order
/// [`next_curve`] walks.
pub const CURVE_SHAPES: [fontelle_model::CurveShape; 6] = [
    fontelle_model::CurveShape::Linear,
    fontelle_model::CurveShape::SCurve,
    fontelle_model::CurveShape::Exponential,
    fontelle_model::CurveShape::Logarithmic,
    fontelle_model::CurveShape::Stepped,
    fontelle_model::CurveShape::Hold,
];
