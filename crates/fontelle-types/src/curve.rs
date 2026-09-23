//! The shape of a segment between two drawn points.
//!
//! It lived in `fontelle-model` beside the automation lane until
//! DisgustingBeat (`docs/disgusting-beat-plan.md` §3.5), and it is here now
//! because the audio thread reads these shapes: a DisgustingBeat lane is a
//! curve in them, evaluated per sample in `fontelle-fx`, and `fontelle-engine`
//! may not see `fontelle-model` (INVARIANT 4). `fontelle-model` re-exports the
//! name, so nothing that already used it changed.
//!
//! The move is also what finally gives [`AutomationPoint::tension`] a value
//! to have. The comment beside it said so:
//!
//! > *"`tension` is not read yet. It is in the document (§12.1) and the
//! > shapes below are its zero position; a curve editor that can bend one is
//! > what gives it a value to have."*
//!
//! DisgustingBeat's lane editor is that editor, and the automation lane gets
//! the bend for nothing because both read this function.
//!
//! [`AutomationPoint::tension`]: fontelle_model

/// How a segment gets from the point that carries this shape to the next one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum CurveShape {
    Linear,
    Exponential,
    Logarithmic,
    SCurve,
    Stepped,
    Hold,
}

impl CurveShape {
    /// Every shape, in the order a chooser offers them.
    pub const ALL: [Self; 6] = [
        Self::Linear,
        Self::Exponential,
        Self::Logarithmic,
        Self::SCurve,
        Self::Stepped,
        Self::Hold,
    ];

    /// What the menu on a point calls it.
    pub fn label(self) -> &'static str {
        match self {
            Self::Linear => "linear",
            Self::Exponential => "exponential",
            Self::Logarithmic => "logarithmic",
            Self::SCurve => "S-curve",
            Self::Stepped => "stepped",
            Self::Hold => "hold",
        }
    }

    /// Where a segment of this shape is, `t` of the way through it, bent by
    /// `tension`.
    ///
    /// Every shape is an interpolation between the *same* two points, so all
    /// of them return 0 at `t = 0` and 1 at `t = 1` — a shape that missed its
    /// own endpoint would put a jump at every point it touched, and
    /// `tests/curve.rs` holds that for every shape at every tension.
    ///
    /// **`tension`** is bipolar in −1..1 and zero is exactly the unbent
    /// shape, bit for bit: the automation lane has stored one on every point
    /// since §12.1 and has never set one, so anything else would change the
    /// shape of every clip in every project the day this landed.
    ///
    /// The bend is the rational one, `t / (t + k(1 − t))` — two multiplies
    /// and a divide, monotone, and it keeps both endpoints. A power curve
    /// would want a `powf` per sample on the audio thread for the same
    /// picture.
    pub fn eased(self, t: f64, tension: f32) -> f64 {
        let t = t.clamp(0.0, 1.0);
        let shaped = match self {
            Self::Linear => t,
            // Slow to start, fast at the end — a fade that sounds even.
            Self::Exponential => t * t,
            // Its mirror.
            Self::Logarithmic => 1.0 - (1.0 - t) * (1.0 - t),
            // Eased at both ends. Smoothstep, which is the cheapest curve with
            // zero slope at each end, so a sweep neither starts nor stops with
            // a corner in it.
            Self::SCurve => t * t * (3.0 - 2.0 * t),
            // Neither of these interpolates at all; they are handled before
            // this is called and are here so the match is total.
            Self::Stepped | Self::Hold => 0.0,
        };
        bend(shaped, tension)
    }

    /// Whether this shape ignores where the segment is *going* — the value
    /// stays put until something else happens.
    pub fn holds(self) -> bool {
        matches!(self, Self::Stepped | Self::Hold)
    }

    /// Whether it ignores every later point as well, not just the next one.
    ///
    /// This is the whole difference between the two flat shapes:
    ///
    /// - **`Stepped`** is a staircase: hold this value until the next point,
    ///   then jump to it. What a rhythmic lane is made of.
    /// - **`Hold`** is a full stop: this value, for the rest of the curve,
    ///   whatever is drawn after it. It is how a lane says "this parameter
    ///   stops moving here" without deleting the points beyond.
    ///
    /// Two flat shapes that both jumped at the next point would be one shape
    /// with two names.
    pub fn freezes(self) -> bool {
        matches!(self, Self::Hold)
    }
}

/// Bends `t` towards one end without moving either end.
///
/// A `tension` that is not finite is a tension of zero rather than a `NaN`
/// travelling into a read position on the audio thread; a drag that ran off
/// the top of a lane is where one comes from.
fn bend(t: f64, tension: f32) -> f64 {
    let tension = if tension.is_finite() { tension } else { 0.0 };
    let tension = tension.clamp(-1.0, 1.0) as f64;
    if tension.abs() < 1e-9 || t <= 0.0 || t >= 1.0 {
        return t;
    }
    // At |tension| → 1 the curve approaches the corner; 0.98 keeps `k` away
    // from zero and the divide away from an infinity.
    let squeeze = 1.0 - tension.abs() * 0.98;
    let k = if tension > 0.0 {
        squeeze
    } else {
        1.0 / squeeze
    };
    t / (t + k * (1.0 - t))
}
