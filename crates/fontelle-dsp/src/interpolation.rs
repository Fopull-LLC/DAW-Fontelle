/// Playback and render quality are independent settings (TDD §7.6) — a user can
/// work at `Normal` and bounce at `High` without thinking about it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Interpolation {
    /// Linear. Preview while dragging, very high polyphony.
    Draft,
    /// 4-point Hermite. Default for playback.
    Normal,
    /// 8-point windowed sinc. Default for render/export.
    High,
    /// 16-point windowed sinc + 2x oversample. Extreme upward transposition.
    Ultra,
}

/// Half-width of the `High` kernel, in samples: 8 taps, 3 behind and 4 ahead.
const SINC_HALF_WIDTH: i32 = 4;

/// One tap of a Blackman-windowed sinc, `x` samples from the read position.
///
/// The Blackman window is zero at `±SINC_HALF_WIDTH` and unity at 0, so the
/// kernel is interpolating: at an integer position every tap but the centre one
/// lands on a zero of the sinc and the stored sample is returned unchanged.
fn sinc_kernel(x: f32) -> f32 {
    let half_width = SINC_HALF_WIDTH as f32;
    if x.abs() >= half_width {
        return 0.0;
    }
    let sinc = if x.abs() < 1e-6 {
        1.0
    } else {
        let pi_x = std::f32::consts::PI * x;
        pi_x.sin() / pi_x
    };
    let phase = std::f32::consts::PI * x / half_width;
    let window = 0.42 + 0.5 * phase.cos() + 0.08 * (2.0 * phase).cos();
    sinc * window
}

/// Reads a fractional sample position from `buffer` using `mode`. `position` is in
/// samples; the fractional part drives the interpolation kernel.
///
/// `Ultra` is not implemented. Its "16-point sinc + 2x oversample" (TDD §7.6)
/// is not expressible here: oversampling is a property of a *stream* of output
/// samples, so it needs a stateful resampler rather than a point-interpolator
/// like this one. Calling `interpolate` with it panics on purpose rather than
/// silently falling back to a cheaper kernel.
pub fn interpolate(buffer: &[f32], position: f64, mode: Interpolation) -> f32 {
    if buffer.is_empty() {
        return 0.0;
    }
    let last = buffer.len() - 1;
    let base = position.floor();
    let t = (position - base) as f32;
    let base = base as isize;

    let at = |offset: isize| -> f32 {
        let idx = (base + offset).clamp(0, last as isize) as usize;
        buffer[idx]
    };

    match mode {
        Interpolation::Draft => {
            let y0 = at(0);
            let y1 = at(1);
            y0 + (y1 - y0) * t
        }
        Interpolation::Normal => {
            // 4-point, 3rd-order Hermite (Catmull-Rom tangents). Passes exactly
            // through every sample point, including on perfectly linear data —
            // see the `hermite_reproduces_a_linear_ramp_exactly` test below.
            let y0 = at(-1);
            let y1 = at(0);
            let y2 = at(1);
            let y3 = at(2);

            let c0 = y1;
            let c1 = 0.5 * (y2 - y0);
            let c2 = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
            let c3 = 0.5 * (y3 - y0) + 1.5 * (y1 - y2);

            ((c3 * t + c2) * t + c1) * t + c0
        }
        Interpolation::High => {
            // 8-point Blackman-windowed sinc. Taps run from 3 samples behind
            // the read position to 4 ahead, so the window's half-width is
            // exactly SINC_HALF_WIDTH and the outermost tap lands on the
            // window's zero.
            //
            // The kernel is evaluated directly, which costs a handful of
            // transcendentals per output sample. That is affordable for the
            // render/export path this mode exists for, and it keeps
            // `interpolate` a pure function — a precomputed phase table would
            // be faster but has to be built somewhere, and building it lazily
            // would put an allocation on the audio thread (INVARIANT 1). If
            // this ever needs to run at playback rates, the table belongs in
            // `prepare()`, not behind a `LazyLock`.
            let mut sum = 0.0;
            let mut weight_sum = 0.0;
            for offset in -(SINC_HALF_WIDTH - 1)..=SINC_HALF_WIDTH {
                let weight = sinc_kernel(t - offset as f32);
                sum += at(offset as isize) * weight;
                weight_sum += weight;
            }
            // Normalise so the taps sum to unity. They don't naturally at an
            // arbitrary phase, and the residue shows up as a periodic
            // amplitude ripple on steady material.
            if weight_sum.abs() > f32::EPSILON {
                sum / weight_sum
            } else {
                sum
            }
        }
        Interpolation::Ultra => {
            todo!("16-point sinc + 2x oversample needs a stateful resampler (TDD §7.6)")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draft_returns_exact_samples_at_integer_positions() {
        let buffer = [0.0, 1.0, 2.0, 3.0];
        for i in 0..buffer.len() {
            assert_eq!(
                interpolate(&buffer, i as f64, Interpolation::Draft),
                buffer[i]
            );
        }
    }

    #[test]
    fn draft_interpolates_linearly_between_samples() {
        let buffer = [0.0, 10.0];
        assert_eq!(interpolate(&buffer, 0.5, Interpolation::Draft), 5.0);
        assert_eq!(interpolate(&buffer, 0.25, Interpolation::Draft), 2.5);
    }

    #[test]
    fn normal_returns_exact_samples_at_integer_positions() {
        let buffer = [3.0, -1.0, 4.0, 1.0, 5.0, 9.0];
        for i in 0..buffer.len() {
            let got = interpolate(&buffer, i as f64, Interpolation::Normal);
            assert!(
                (got - buffer[i]).abs() < 1e-5,
                "position {i}: expected {}, got {got}",
                buffer[i]
            );
        }
    }

    #[test]
    fn hermite_reproduces_a_linear_ramp_exactly() {
        let buffer: Vec<f32> = (0..16).map(|i| i as f32 * 0.5).collect();
        for tenth in 0..100 {
            let position = tenth as f64 / 10.0 + 2.0; // stay clear of the edges
            let got = interpolate(&buffer, position, Interpolation::Normal);
            let expected = position as f32 * 0.5;
            assert!(
                (got - expected).abs() < 1e-4,
                "position {position}: expected {expected}, got {got}"
            );
        }
    }

    #[test]
    fn clamps_at_buffer_edges_instead_of_reading_out_of_bounds() {
        let buffer = [1.0, 2.0, 3.0];
        // Positions past either edge must not panic, and should hold the edge value.
        assert_eq!(interpolate(&buffer, -1.0, Interpolation::Draft), 1.0);
        assert_eq!(interpolate(&buffer, 5.0, Interpolation::Draft), 3.0);
        assert_eq!(interpolate(&buffer, 0.0, Interpolation::Normal), 1.0);
        assert_eq!(interpolate(&buffer, 2.0, Interpolation::Normal), 3.0);
    }

    #[test]
    fn empty_buffer_returns_silence_instead_of_panicking() {
        let buffer: [f32; 0] = [];
        assert_eq!(interpolate(&buffer, 0.0, Interpolation::Draft), 0.0);
    }

    /// Sampled at `cycles_per_sample`, so 0.5 is Nyquist.
    fn sine(len: usize, cycles_per_sample: f64) -> Vec<f32> {
        (0..len)
            .map(|i| (std::f64::consts::TAU * cycles_per_sample * i as f64).sin() as f32)
            .collect()
    }

    fn rms_error_against_sine(mode: Interpolation, cycles_per_sample: f64) -> f32 {
        let buffer = sine(128, cycles_per_sample);
        let mut sum_sq = 0.0f64;
        let mut count = 0;
        // Stay far enough inside the buffer that no kernel clamps at an edge.
        for step in 0..500 {
            let position = 16.0 + step as f64 * 0.19;
            if position > 108.0 {
                break;
            }
            let got = interpolate(&buffer, position, mode) as f64;
            let want = (std::f64::consts::TAU * cycles_per_sample * position).sin();
            sum_sq += (got - want) * (got - want);
            count += 1;
        }
        (sum_sq / count as f64).sqrt() as f32
    }

    #[test]
    fn high_returns_exact_samples_at_integer_positions() {
        // A windowed sinc is an interpolating kernel: the centre tap is 1 and
        // every other tap lands on a zero of the sinc, so an integer position
        // must return the stored sample untouched.
        let buffer = sine(64, 0.11);
        for i in 8..56 {
            let got = interpolate(&buffer, i as f64, Interpolation::High);
            assert!(
                (got - buffer[i]).abs() < 1e-5,
                "position {i}: expected {}, got {got}",
                buffer[i]
            );
        }
    }

    #[test]
    fn high_preserves_dc() {
        // The taps of a windowed sinc do not sum to exactly 1 at an arbitrary
        // fractional phase, so the kernel has to be normalised. Without it a
        // steady signal develops a periodic amplitude ripple at the resampling
        // phase, which is audible as a whine on sustained material.
        let buffer = vec![0.75f32; 64];
        for step in 0..40 {
            let position = 16.0 + step as f64 * 0.025;
            let got = interpolate(&buffer, position, Interpolation::High);
            assert!(
                (got - 0.75).abs() < 1e-5,
                "position {position}: expected 0.75, got {got}"
            );
        }
    }

    #[test]
    fn high_tracks_a_sine_more_closely_than_hermite() {
        // The whole reason High exists (TDD §7.6). At 40% of Nyquist — ordinary
        // upper-mid content in a transposed sample — an 8-point windowed sinc
        // should be substantially more accurate than 4-point Hermite, not
        // marginally so.
        let f = 0.2;
        let normal = rms_error_against_sine(Interpolation::Normal, f);
        let high = rms_error_against_sine(Interpolation::High, f);
        assert!(
            high < normal * 0.5,
            "expected High to at least halve Hermite's error, got high {high} vs normal {normal}"
        );
        assert!(
            high < 0.02,
            "High's absolute error should be small, got {high}"
        );
    }

    #[test]
    fn high_clamps_at_buffer_edges_instead_of_reading_out_of_bounds() {
        // An 8-point kernel reaches 3 samples back and 4 forward, so every
        // position in a short buffer is an edge case.
        let buffer = [1.0, 2.0, 3.0];
        for step in -20..40 {
            let position = step as f64 * 0.25;
            let got = interpolate(&buffer, position, Interpolation::High);
            assert!(got.is_finite(), "position {position} produced {got}");
        }
        assert!((interpolate(&buffer, 0.0, Interpolation::High) - 1.0).abs() < 1e-5);
        assert!((interpolate(&buffer, 2.0, Interpolation::High) - 3.0).abs() < 1e-5);
    }
}
