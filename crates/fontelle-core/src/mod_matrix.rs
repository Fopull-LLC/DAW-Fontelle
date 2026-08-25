#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModSource {
    Envelope(u8),
    Lfo(u8),
    Velocity,
    Key,
    Aftertouch,
    ModWheel,
    PitchBend,
    Random,
    NoteOnCounter,
}

/// Any continuous patch parameter, addressed by stable ID (TDD §7.5). Minimum set:
/// layer pitch/gain/pan, sample start offset, loop start/length, filter cutoff and
/// resonance, every envelope stage time/level, every LFO rate/depth, unison detune.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModDest {
    LayerPitch(u8),
    LayerGain(u8),
    LayerPan(u8),
    SampleStartOffset(u8),
    LoopStart(u8),
    LoopLength(u8),
    FilterCutoff(u8),
    FilterResonance(u8),
    EnvelopeStageTime(u8, u8),
    EnvelopeStageLevel(u8, u8),
    LfoRate(u8),
    LfoDepth(u8),
    UnisonDetune,
}

/// How a route reshapes its source before applying depth.
///
/// Every curve is an identity at 0 and at ±1 — a full-depth route must still
/// reach full depth whatever curve it carries — and every one preserves sign,
/// so a bipolar source such as pitch bend or an LFO is shaped symmetrically
/// rather than folded to one side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Curve {
    Linear,
    /// Squared: slow to leave zero, then accelerating.
    Exponential,
    /// Square root: the mirror of `Exponential`, quick off zero then flattening.
    Logarithmic,
    /// Smoothstep: flat at both ends, steep through the middle.
    SCurve,
    /// Snapped to `steps` equal divisions of the 0..1 magnitude — the way to
    /// get semitone-stepped pitch from a continuous source. The count lives on
    /// the variant rather than in a constant because there is no step count
    /// that is right for both a two-position switch and a 24-note arpeggio.
    Quantised {
        steps: u8,
    },
}

impl Curve {
    /// Shapes a source value. Magnitude is reshaped, sign is carried through
    /// untouched.
    fn apply(self, value: f32) -> f32 {
        let magnitude = value.abs().min(1.0);
        let shaped = match self {
            Self::Linear => magnitude,
            Self::Exponential => magnitude * magnitude,
            Self::Logarithmic => magnitude.sqrt(),
            Self::SCurve => magnitude * magnitude * (3.0 - 2.0 * magnitude),
            Self::Quantised { steps } => {
                if steps == 0 {
                    magnitude
                } else {
                    let steps = steps as f32;
                    (magnitude * steps).round() / steps
                }
            }
        };
        shaped.copysign(value)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ModRoute {
    pub source: ModSource,
    pub destination: ModDest,
    /// Bipolar.
    pub depth: f32,
    pub curve: Curve,
    /// A secondary modulator scaling this route's depth — e.g. "LFO depth
    /// controlled by mod wheel" (TDD §7.5).
    pub via: Option<ModSource>,
    /// Use `1 - source` in place of `source`, so the route is at full strength
    /// when the source is at rest and falls away as it rises.
    ///
    /// Not in TDD §7.5's field list, but required by the format the importer
    /// has to represent: an SF2 modulator carries a direction bit, and its two
    /// always-present defaults both use the negative direction. Velocity to
    /// filter cutoff at -2400 cents means "full cutoff at full velocity,
    /// two octaves down at silence" — an offset from full scale, which a plain
    /// product of source and depth cannot express at any depth. Only
    /// meaningful for unipolar sources; a bipolar one is already symmetric.
    pub invert: bool,
}

#[derive(Debug, Clone, Default)]
pub struct ModMatrix {
    pub routes: Vec<ModRoute>,
}

impl ModDest {
    /// The destination's own unit range for a route at full depth.
    ///
    /// `ModRoute::depth` is normalised bipolar (TDD §7.5), so every destination
    /// has to say what depth 1.0 means in the units it actually works in.
    /// Cutoff and pitch are in cents, gain in decibels; the ranges are wide
    /// enough to cover what SF2 can ask for — its filter modulators reach
    /// -2400 cents and its attenuation modulators 960 centibels — without
    /// being so wide that ordinary depths sit in the bottom of the dial.
    pub fn full_scale(self) -> f32 {
        match self {
            Self::FilterCutoff(_) | Self::LayerPitch(_) => 9_600.0, // cents, 8 octaves
            Self::LayerGain(_) => 96.0,                             // dB
            _ => 1.0,
        }
    }
}

impl ModMatrix {
    /// Sums every route targeting `dest` into a single modulation value for this
    /// voice's current source values. Called once per block per destination that
    /// has at least one route — never allocates (INVARIANT 1).
    ///
    /// The result is deliberately not clamped. What a sum of ±1 contributions
    /// means is the destination's business — cents for pitch, decibels for
    /// gain, a multiplier for cutoff — and clamping here would silently cap
    /// combinations the destination could represent perfectly well.
    pub fn evaluate(&self, dest: ModDest, source_values: &dyn Fn(ModSource) -> f32) -> f32 {
        let mut total = 0.0;
        for route in &self.routes {
            if route.destination != dest {
                continue;
            }
            let value = source_values(route.source);
            let value = if route.invert { 1.0 - value } else { value };
            let shaped = route.curve.apply(value);
            let via = route.via.map_or(1.0, source_values);
            total += shaped * route.depth * via;
        }
        total
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn route(source: ModSource, destination: ModDest, depth: f32) -> ModRoute {
        ModRoute {
            source,
            destination,
            depth,
            curve: Curve::Linear,
            via: None,
            invert: false,
        }
    }

    /// Every source reads 0.5 unless named otherwise — a value that is its own
    /// distinct number under every curve, unlike 0 or 1.
    fn sources(overrides: &[(ModSource, f32)]) -> impl Fn(ModSource) -> f32 + '_ {
        move |s| {
            overrides
                .iter()
                .find(|(k, _)| *k == s)
                .map(|(_, v)| *v)
                .unwrap_or(0.5)
        }
    }

    #[test]
    fn a_destination_with_no_routes_is_not_modulated() {
        let matrix = ModMatrix::default();
        assert_eq!(
            matrix.evaluate(ModDest::FilterCutoff(0), &sources(&[])),
            0.0
        );
    }

    #[test]
    fn a_linear_route_is_the_source_scaled_by_depth() {
        let matrix = ModMatrix {
            routes: vec![route(ModSource::Velocity, ModDest::FilterCutoff(0), 0.5)],
        };
        let got = matrix.evaluate(
            ModDest::FilterCutoff(0),
            &sources(&[(ModSource::Velocity, 0.8)]),
        );
        assert!((got - 0.4).abs() < 1e-6, "expected 0.8 * 0.5, got {got}");
    }

    #[test]
    fn routes_aimed_elsewhere_are_ignored() {
        let matrix = ModMatrix {
            routes: vec![
                route(ModSource::Velocity, ModDest::FilterResonance(0), 1.0),
                route(ModSource::Velocity, ModDest::FilterCutoff(1), 1.0),
            ],
        };
        assert_eq!(
            matrix.evaluate(ModDest::FilterCutoff(0), &sources(&[])),
            0.0,
            "neither a different destination nor a different slot index may leak"
        );
    }

    #[test]
    fn several_routes_to_one_destination_sum() {
        let matrix = ModMatrix {
            routes: vec![
                route(ModSource::Velocity, ModDest::FilterCutoff(0), 0.5),
                route(ModSource::ModWheel, ModDest::FilterCutoff(0), 0.25),
                route(ModSource::Key, ModDest::FilterCutoff(0), -0.5),
            ],
        };
        let got = matrix.evaluate(ModDest::FilterCutoff(0), &sources(&[]));
        // 0.5*0.5 + 0.5*0.25 + 0.5*-0.5 = 0.125
        assert!((got - 0.125).abs() < 1e-6, "expected 0.125, got {got}");
    }

    #[test]
    fn via_scales_the_routes_contribution() {
        // TDD §7.5's example: "LFO depth controlled by mod wheel".
        let mut r = route(ModSource::Lfo(0), ModDest::LayerPitch(0), 1.0);
        r.via = Some(ModSource::ModWheel);
        let matrix = ModMatrix { routes: vec![r] };

        let closed = matrix.evaluate(
            ModDest::LayerPitch(0),
            &sources(&[(ModSource::Lfo(0), 1.0), (ModSource::ModWheel, 0.0)]),
        );
        let open = matrix.evaluate(
            ModDest::LayerPitch(0),
            &sources(&[(ModSource::Lfo(0), 1.0), (ModSource::ModWheel, 1.0)]),
        );
        assert_eq!(closed, 0.0, "a closed via must mute the route entirely");
        assert!(
            (open - 1.0).abs() < 1e-6,
            "an open via must pass it, got {open}"
        );
    }

    #[test]
    fn curves_shape_the_source_without_changing_its_sign() {
        let evaluate = |curve: Curve, value: f32| {
            let mut r = route(ModSource::PitchBend, ModDest::LayerPitch(0), 1.0);
            r.curve = curve;
            ModMatrix { routes: vec![r] }.evaluate(
                ModDest::LayerPitch(0),
                &sources(&[(ModSource::PitchBend, value)]),
            )
        };

        // Exponential is slower to leave zero, logarithmic faster; both must
        // still pass 0 and ±1 through untouched, or a full-depth route would
        // no longer reach full depth.
        for value in [-1.0f32, 0.0, 1.0] {
            for curve in [Curve::Exponential, Curve::Logarithmic, Curve::SCurve] {
                assert!(
                    (evaluate(curve, value) - value).abs() < 1e-6,
                    "{curve:?} must be an identity at {value}"
                );
            }
        }
        assert!(evaluate(Curve::Exponential, 0.5) < 0.5);
        assert!(evaluate(Curve::Logarithmic, 0.5) > 0.5);
        // Sign preserved: a bipolar source must not fold to one side.
        assert!(evaluate(Curve::Exponential, -0.5) > -0.5);
        assert!(evaluate(Curve::Exponential, -0.5) < 0.0);
        assert!(evaluate(Curve::Logarithmic, -0.5) < -0.5);
    }

    #[test]
    fn an_s_curve_is_flat_at_the_ends_and_steep_in_the_middle() {
        let evaluate = |value: f32| {
            let mut r = route(ModSource::ModWheel, ModDest::LayerGain(0), 1.0);
            r.curve = Curve::SCurve;
            ModMatrix { routes: vec![r] }.evaluate(
                ModDest::LayerGain(0),
                &sources(&[(ModSource::ModWheel, value)]),
            )
        };
        assert!(
            (evaluate(0.5) - 0.5).abs() < 1e-6,
            "symmetric about the middle"
        );
        assert!(evaluate(0.1) < 0.1, "shallow near the bottom");
        assert!(evaluate(0.9) > 0.9, "and near the top");
    }

    #[test]
    fn a_quantised_curve_snaps_to_its_own_step_count() {
        let evaluate = |steps: u8, value: f32| {
            let mut r = route(ModSource::Lfo(0), ModDest::LayerPitch(0), 1.0);
            r.curve = Curve::Quantised { steps };
            ModMatrix { routes: vec![r] }.evaluate(
                ModDest::LayerPitch(0),
                &sources(&[(ModSource::Lfo(0), value)]),
            )
        };
        // 4 steps over 0..1 means the only reachable values are 0, .25, .5, .75, 1.
        for (input, expected) in [(0.0, 0.0), (0.3, 0.25), (0.6, 0.5), (0.9, 1.0)] {
            let got = evaluate(4, input);
            assert!(
                (got - expected).abs() < 1e-6,
                "4 steps at {input}: expected {expected}, got {got}"
            );
        }
        assert!(
            (evaluate(4, -0.3) + 0.25).abs() < 1e-6,
            "quantisation must be symmetric for bipolar sources"
        );
        assert_eq!(
            evaluate(0, 0.37),
            0.37,
            "zero steps cannot quantise anything"
        );
    }
}
