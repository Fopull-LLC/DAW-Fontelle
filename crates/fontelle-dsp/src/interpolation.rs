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

/// Reads a fractional sample position from `buffer` using `mode`. `position` is in
/// samples; the fractional part drives the interpolation kernel.
///
/// `High`/`Ultra` (windowed sinc) are not implemented yet — TDD §7.6 flags them as
/// the render/export-quality path, not needed for the M0 vertical slice, which
/// plays back at `Normal`. Calling `interpolate` with either panics on purpose
/// rather than silently falling back to a cheaper kernel.
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
        Interpolation::High | Interpolation::Ultra => {
            todo!("windowed-sinc kernel — not needed for the M0 vertical slice (TDD §7.6)")
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
}
