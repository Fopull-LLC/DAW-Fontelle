/// How a pan control attenuates each side as it moves off centre (TDD §13.1,
/// where `MixerTrack::pan_law` defaults to `-3dB`).
///
/// Lives here rather than in `fontelle-model` because both the document model
/// *and* `fontelle-engine`'s `MixerTrackNode` need it, and the engine can't
/// depend on the model (TDD §4.1).
///
/// **A judgment call the TDD doesn't settle:** it names four laws but doesn't
/// define them, and on the usual reading "-6 dB" and "linear" are the same
/// taper. They're kept distinct here as: `Minus6Db` is the linear *taper*
/// (centre reads 0.5, i.e. -6 dB on each side), while `Linear` is a
/// balance-style control that leaves centre at unity gain and only attenuates
/// the side you pan away from. Recorded in `PROGRESS.md` as an open question
/// against the TDD, not a settled decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PanLaw {
    /// Constant power (`cos`/`sin`). Centre = ~0.707 on each side. The default.
    Minus3Db,
    /// The usual compromise between constant-power and linear. Centre = ~0.595.
    Minus4_5Db,
    /// Linear taper. Centre = 0.5 on each side.
    Minus6Db,
    /// Balance-style: unity at centre, only the far side is attenuated.
    Linear,
}

impl PanLaw {
    /// Returns `(left_gain, right_gain)` for `pan` in `-1.0..=1.0`
    /// (-1 = hard left, 0 = centre, +1 = hard right). Values outside that
    /// range are clamped rather than extrapolated into negative gain.
    pub fn gains(self, pan: f32) -> (f32, f32) {
        let pan = pan.clamp(-1.0, 1.0);
        // 0.0 at hard left, 0.5 at centre, 1.0 at hard right.
        let position = (pan + 1.0) * 0.5;

        match self {
            Self::Minus3Db => {
                let angle = position * std::f32::consts::FRAC_PI_2;
                (angle.cos(), angle.sin())
            }
            Self::Minus4_5Db => {
                // Geometric mean of the constant-power and linear laws.
                let angle = position * std::f32::consts::FRAC_PI_2;
                let (cp_l, cp_r) = (angle.cos(), angle.sin());
                let (lin_l, lin_r) = (1.0 - position, position);
                ((cp_l * lin_l).sqrt(), (cp_r * lin_r).sqrt())
            }
            Self::Minus6Db => (1.0 - position, position),
            Self::Linear => ((1.0 - pan).min(1.0), (1.0 + pan).min(1.0)),
        }
    }
}

/// Turns the pan a document stores — `Note::pan` and its `-127..=127` range —
/// into the `-1.0..=1.0` position [`PanLaw::gains`] takes.
///
/// It lives here, beside the law it feeds, because it is the seam between the
/// two halves of the workspace: the model and the compiled event stream count
/// pan in bytes, and everything below `fontelle-core`'s note-on counts it in
/// unit intervals. Written once, so a note panned hard left in the roll is
/// hard left in the field rather than wherever the nearest divisor put it.
///
/// `i8` reaches one step further left than `NoteProperty::Pan` allows, so
/// -128 is clamped to hard left rather than overshooting into a value the law
/// would have to clamp anyway.
pub fn pan_unit(pan: i8) -> f32 {
    (f32::from(pan) / 127.0).clamp(-1.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The name of each law *is* its centre attenuation — if these drift, the
    /// enum variants are lying about what they do.
    #[test]
    fn each_law_attenuates_centre_by_the_amount_it_is_named_after() {
        let cases = [
            (PanLaw::Minus3Db, -3.0),
            (PanLaw::Minus4_5Db, -4.5),
            (PanLaw::Minus6Db, -6.0),
            (PanLaw::Linear, 0.0),
        ];
        for (law, expected_db) in cases {
            let (l, r) = law.gains(0.0);
            assert!(
                (l - r).abs() < 1e-6,
                "{law:?}: centre must be symmetric, got {l} / {r}"
            );
            let got_db = 20.0 * l.log10();
            assert!(
                (got_db - expected_db).abs() < 0.1,
                "{law:?}: centre should be {expected_db} dB, got {got_db} dB"
            );
        }
    }

    #[test]
    fn hard_left_and_hard_right_fully_silence_the_opposite_side() {
        for law in [
            PanLaw::Minus3Db,
            PanLaw::Minus4_5Db,
            PanLaw::Minus6Db,
            PanLaw::Linear,
        ] {
            let (l, r) = law.gains(-1.0);
            assert!(
                (l - 1.0).abs() < 1e-6,
                "{law:?}: hard left keeps left at 1.0"
            );
            assert!(r.abs() < 1e-6, "{law:?}: hard left silences right, got {r}");

            let (l, r) = law.gains(1.0);
            assert!(l.abs() < 1e-6, "{law:?}: hard right silences left, got {l}");
            assert!(
                (r - 1.0).abs() < 1e-6,
                "{law:?}: hard right keeps right at 1.0"
            );
        }
    }

    #[test]
    fn constant_power_law_holds_total_power_constant_across_the_sweep() {
        // The defining property of the -3dB law: l^2 + r^2 == 1 everywhere.
        for step in 0..=20 {
            let pan = step as f32 / 10.0 - 1.0;
            let (l, r) = PanLaw::Minus3Db.gains(pan);
            let power = l * l + r * r;
            assert!(
                (power - 1.0).abs() < 1e-5,
                "pan {pan}: constant-power law must keep l^2+r^2 == 1, got {power}"
            );
        }
    }

    #[test]
    fn out_of_range_pan_clamps_instead_of_producing_negative_gain() {
        let (l, r) = PanLaw::Minus6Db.gains(-5.0);
        assert!((l - 1.0).abs() < 1e-6 && r.abs() < 1e-6);
        let (l, r) = PanLaw::Minus6Db.gains(5.0);
        assert!(l.abs() < 1e-6 && (r - 1.0).abs() < 1e-6);
    }

    /// The document stores a note's pan as a byte and the audio path wants a
    /// unit interval. The conversion is written **once**, here, because the
    /// alternative is each layer dividing by whatever it remembers and a note
    /// panned hard left in the roll arriving somewhere else in the field.
    #[test]
    fn a_notes_pan_byte_maps_onto_the_whole_field() {
        assert!(pan_unit(0).abs() < 1e-6, "centre is centre");
        assert!((pan_unit(127) - 1.0).abs() < 1e-6, "127 is hard right");
        assert!((pan_unit(-127) + 1.0).abs() < 1e-6, "-127 is hard left");
        // `NoteProperty::Pan` stops at -127 so the two ends are the same
        // distance from centre, but the byte holding it reaches one step
        // further. Clamping rather than overshooting keeps a damaged document
        // out of `PanLaw::gains`'s clamp, where it would silently read as
        // hard left anyway with no way to notice.
        assert!(
            (pan_unit(-128) + 1.0).abs() < 1e-6,
            "-128 clamps to hard left rather than overshooting it"
        );
    }
}
