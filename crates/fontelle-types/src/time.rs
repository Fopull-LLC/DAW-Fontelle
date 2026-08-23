/// Musical time, TDD §6.1 (INVARIANT 5). Never a float, never stored on disk as beats.
pub type Tick = i64;

/// Audio time: samples from song start.
pub type Sample = i64;

/// Pulses per quarter note. Divides evenly by 2, 3, 4, 5, 6, 8, and 16.
pub const PPQN: i64 = 960;
