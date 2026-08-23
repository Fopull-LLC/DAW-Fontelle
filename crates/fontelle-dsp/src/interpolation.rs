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
pub fn interpolate(_buffer: &[f32], _position: f64, _mode: Interpolation) -> f32 {
    todo!("per-mode kernel: linear / 4-point Hermite / 8-point sinc / 16-point sinc+oversample")
}
