//! Levels: one of a note's six properties, shaped across a selection (FL
//! Studio's Scale levels, Cubase's Velocity, Logic's Crescendo). It replaces
//! the old Adjust tool, whose add and take-off it still is.

use super::ToolResult;
use crate::note::{Note, NoteProperty};
use fontelle_types::NoteId;

/// What Levels does to the values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum LevelsMode {
    /// `amount` added (or taken off, when it is negative).
    #[default]
    Add,
    /// Everything to `amount`.
    Set,
    /// Each multiplied by `percent`.
    Scale,
    /// Each pulled towards the average by `percent` of its distance from it:
    /// under 100 evens them out, over 100 spreads them.
    Compress,
    /// Kept between `low` and `high`.
    Limit,
    /// From `from` at the first start to `to` at the last — a crescendo.
    Ramp,
}

/// What Levels is set to.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct LevelsSpec {
    pub property: NoteProperty,
    pub mode: LevelsMode,
    pub amount: i32,
    pub percent: i32,
    pub low: i32,
    pub high: i32,
    pub from: i32,
    pub to: i32,
}

impl Default for LevelsSpec {
    fn default() -> Self {
        Self {
            property: NoteProperty::Velocity,
            mode: LevelsMode::Add,
            amount: 10,
            percent: 80,
            low: 40,
            high: 110,
            from: 40,
            to: 120,
        }
    }
}

/// `scope` with `spec.property` shaped, every answer inside what the
/// property may hold.
pub fn levelled(scope: &[(NoteId, Note)], spec: LevelsSpec) -> ToolResult {
    if scope.is_empty() {
        return ToolResult::default();
    }
    let property = spec.property;
    let values: Vec<f64> = scope
        .iter()
        .map(|(_, n)| f64::from(property.get(n)))
        .collect();
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    let first = scope.iter().map(|(_, n)| n.start).min().unwrap_or(0);
    let last = scope.iter().map(|(_, n)| n.start).max().unwrap_or(first);
    let percent = f64::from(spec.percent) / 100.0;
    let after = scope
        .iter()
        .zip(&values)
        .map(|((_, n), value)| {
            let wanted = match spec.mode {
                LevelsMode::Add => value + f64::from(spec.amount),
                LevelsMode::Set => f64::from(spec.amount),
                LevelsMode::Scale => value * percent,
                LevelsMode::Compress => mean + (value - mean) * percent,
                LevelsMode::Limit => {
                    let (low, high) = (spec.low.min(spec.high), spec.low.max(spec.high));
                    value.clamp(f64::from(low), f64::from(high))
                }
                LevelsMode::Ramp => {
                    let x = if last > first {
                        (n.start - first) as f64 / (last - first) as f64
                    } else {
                        0.0
                    };
                    f64::from(spec.from) + f64::from(spec.to - spec.from) * x
                }
            };
            let mut n = n.clone();
            // `set` clamps into what the property may hold — a velocity
            // never reaches zero, which would be a note-off.
            property.set(&mut n, wanted.round() as i32);
            n
        })
        .collect();
    ToolResult::rewriting(scope, after)
}
