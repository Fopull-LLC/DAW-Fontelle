//! A drawn LFO shape (`docs/flopsynth-next.md` §3.4).
//!
//! Here beside [`LfoWave`](crate::LfoWave) for the reason its `value` is
//! here: what the shape is worth at a phase is a fact the picture and the
//! voice both need, and one function of it is one to get wrong. The voice
//! reads it in Phase 3; until then a shape is drawn and kept.

use crate::LfoWave;

/// The most points a shape holds. Sixty-four is a step per sixteenth of
/// four bars, and a shape drawn finer than that is a wavetable's job.
pub const MAX_LFO_POINTS: usize = 64;

/// Whether the shape is read between its points or held at each.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum LfoShapeMode {
    #[default]
    Smooth,
    Step,
}

/// One point of a shape: where in the cycle (0..1), how high (−1..=1), and
/// how the segment **after** it is bent (−1..=1; 0 is a straight line).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LfoPoint {
    pub x: f32,
    pub y: f32,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub tension: f32,
}

fn is_zero(v: &f32) -> bool {
    *v == 0.0
}

/// The shape: its points in `x` order, the grid a dragged point snaps to
/// (divisions of the cycle; 0 for none), and how it is read.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LfoShape {
    pub points: Vec<LfoPoint>,
    #[serde(default)]
    pub grid: u8,
    #[serde(default)]
    pub mode: LfoShapeMode,
}

impl LfoShape {
    /// The shape's value at `phase` (wrapped into 0..1), in −1..=1.
    ///
    /// Between two points the segment is a straight line bent by the
    /// first point's tension — a power curve on the segment's progress,
    /// the bend `fontelle_dsp::shape_progress` gives an envelope stage —
    /// and the cycle closes from the last point back to the first. A
    /// stepped shape holds each point's level until the next.
    pub fn value(&self, phase: f32) -> f32 {
        let n = self.points.len();
        if n == 0 {
            return 0.0;
        }
        if n == 1 {
            return self.points[0].y;
        }
        let phase = phase.rem_euclid(1.0);
        // The segment `phase` is in: from the last point at or before it.
        let after = self.points.iter().position(|p| p.x > phase).unwrap_or(n);
        let from = if after == 0 { n - 1 } else { after - 1 };
        let to = after % n;
        let a = self.points[from];
        let b = self.points[to];
        if self.mode == LfoShapeMode::Step {
            return a.y;
        }
        // Across the wrap the segment runs from the last point to the
        // first one a cycle later.
        let (ax, bx) = if to <= from {
            if phase < a.x {
                (a.x - 1.0, b.x)
            } else {
                (a.x, b.x + 1.0)
            }
        } else {
            (a.x, b.x)
        };
        let span = bx - ax;
        if span <= 1e-6 {
            return b.y;
        }
        let t = ((phase - ax) / span).clamp(0.0, 1.0);
        let t = bend(t, a.tension);
        a.y + (b.y - a.y) * t
    }

    /// A shape sampled off a wave, `count` points across the cycle (capped),
    /// which is what "draw on this" starts from.
    pub fn from_wave(wave: LfoWave, count: usize) -> Self {
        let count = count.clamp(2, MAX_LFO_POINTS);
        let points = (0..count)
            .map(|i| {
                let x = i as f32 / count as f32;
                LfoPoint {
                    x,
                    y: wave.value(x),
                    tension: 0.0,
                }
            })
            .collect();
        Self {
            points,
            grid: 8,
            mode: LfoShapeMode::Smooth,
        }
    }

    /// Adds a point, kept in `x` order; `false` when the shape is full.
    pub fn add_point(&mut self, x: f32, y: f32) -> bool {
        if self.points.len() >= MAX_LFO_POINTS {
            return false;
        }
        let x = x.clamp(0.0, 1.0);
        let at = self
            .points
            .iter()
            .position(|p| p.x > x)
            .unwrap_or(self.points.len());
        self.points.insert(
            at,
            LfoPoint {
                x,
                y: y.clamp(-1.0, 1.0),
                tension: 0.0,
            },
        );
        true
    }

    /// Takes a point out; `false` for the first or the last, which the
    /// cycle needs, or one that is not there.
    pub fn remove_point(&mut self, index: usize) -> bool {
        if index == 0 || index + 1 >= self.points.len() {
            return false;
        }
        self.points.remove(index);
        true
    }

    /// The factory shapes the shapes menu offers, each with a name.
    pub fn presets() -> Vec<(&'static str, Self)> {
        let pt = |x: f32, y: f32, tension: f32| LfoPoint { x, y, tension };
        let smooth = |points: Vec<LfoPoint>| Self {
            points,
            grid: 8,
            mode: LfoShapeMode::Smooth,
        };
        let stepped = |points: Vec<LfoPoint>| Self {
            points,
            grid: 8,
            mode: LfoShapeMode::Step,
        };
        let steps = |count: usize, rising: bool| {
            stepped(
                (0..count)
                    .map(|i| {
                        let x = i as f32 / count as f32;
                        let level = i as f32 / (count - 1) as f32 * 2.0 - 1.0;
                        pt(x, if rising { level } else { -level }, 0.0)
                    })
                    .collect(),
            )
        };
        vec![
            (
                "Ramp up",
                smooth(vec![pt(0.0, -1.0, 0.0), pt(1.0, 1.0, 0.0)]),
            ),
            (
                "Ramp down",
                smooth(vec![pt(0.0, 1.0, 0.0), pt(1.0, -1.0, 0.0)]),
            ),
            (
                "Triangle",
                smooth(vec![
                    pt(0.0, -1.0, 0.0),
                    pt(0.5, 1.0, 0.0),
                    pt(1.0, -1.0, 0.0),
                ]),
            ),
            (
                "Pulse",
                stepped(vec![pt(0.0, 1.0, 0.0), pt(0.25, -1.0, 0.0)]),
            ),
            (
                "Bounce",
                smooth(vec![
                    pt(0.0, 1.0, 0.6),
                    pt(0.5, -1.0, -0.6),
                    pt(1.0, 1.0, 0.0),
                ]),
            ),
            (
                "Swell",
                smooth(vec![
                    pt(0.0, -1.0, 0.7),
                    pt(0.8, 1.0, -0.5),
                    pt(1.0, -1.0, 0.0),
                ]),
            ),
            ("Steps up 4", steps(4, true)),
            ("Steps up 8", steps(8, true)),
            ("Steps down 8", steps(8, false)),
            (
                "Sidechain",
                smooth(vec![
                    pt(0.0, -1.0, -0.8),
                    pt(0.35, 1.0, 0.0),
                    pt(0.98, 1.0, 0.0),
                ]),
            ),
        ]
    }
}

/// Bends a segment's progress by its tension: positive is slow to leave
/// then fast, negative the mirror, and the ends stay put.
fn bend(t: f32, tension: f32) -> f32 {
    let tension = tension.clamp(-1.0, 1.0);
    if tension == 0.0 {
        return t;
    }
    t.clamp(0.0, 1.0).powf(2f32.powf(2.0 * tension))
}
