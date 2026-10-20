//! Humanize: the randomizer grown to cover time. Ty decided 2026-10-20
//! (`docs/roll-tools-plan.md` §7 Q4) to let it move timing and length;
//! pitch stays out, as PROGRESS decided in September — randomised pitch
//! mangles music, and the transposer is the deliberate case.

use super::{RandomMode, ToolResult};
use crate::note::{Note, NoteProperty};
use fontelle_types::{NoteId, Tick};

/// What Humanize is set to.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct HumanizeSpec {
    /// The property the old randomizer acted on, and how far.
    pub property: NoteProperty,
    pub amount: i32,
    pub mode: RandomMode,
    /// The furthest a start may move either way, in ticks.
    pub timing: Tick,
    /// The most a length may change either way, in percent.
    pub length: i32,
    pub seed: u64,
}

impl Default for HumanizeSpec {
    fn default() -> Self {
        Self {
            property: NoteProperty::Velocity,
            amount: 20,
            mode: RandomMode::Around,
            timing: 0,
            length: 0,
            seed: 1,
        }
    }
}

/// `scope`, humanised. No start moves past another event's start — a
/// wobble that reordered the phrase would not be a wobble.
pub fn humanized(scope: &[(NoteId, Note)], spec: HumanizeSpec) -> ToolResult {
    let starting: Vec<i32> = scope.iter().map(|(_, n)| spec.property.get(n)).collect();
    let values = super::randomised(
        &starting,
        spec.property,
        super::RandomSpec {
            amount: spec.amount,
            mode: spec.mode,
        },
        spec.seed,
    );
    let mut events: Vec<Tick> = scope.iter().map(|(_, n)| n.start).collect();
    events.sort_unstable();
    events.dedup();
    let timing = spec.timing.max(0);
    let length = i64::from(spec.length.clamp(0, 100));
    // One roll each, salted so timing, length and the property never move
    // together — a note that came out late would otherwise always come out
    // long and loud too.
    let roll = |index: usize, salt: u64, reach: i64| -> i64 {
        if reach <= 0 {
            return 0;
        }
        let r = super::split_mix(
            spec.seed.wrapping_mul(0xD6E8_FEB8_6659_FD93) ^ salt ^ (index as u64) << 8,
        );
        (r % (reach as u64 * 2 + 1)) as i64 - reach
    };
    let after = scope
        .iter()
        .enumerate()
        .map(|(i, (_, n))| {
            let mut n = n.clone();
            spec.property.set(&mut n, values[i]);
            // Never as far as half the way to the next event either side,
            // so neighbours moving towards each other cannot cross.
            let at = events.binary_search(&n.start).unwrap_or(0);
            let mut reach = timing;
            if at > 0 {
                reach = reach.min((n.start - events[at - 1]) / 2 - 1);
            }
            if let Some(next) = events.get(at + 1) {
                reach = reach.min((next - n.start) / 2 - 1);
            }
            n.start = (n.start + roll(i, 0x51, reach)).max(0);
            let wobble = n.length * length / 100;
            n.length = (n.length + roll(i, 0x1E, wobble)).max(1);
            n
        })
        .collect();
    ToolResult::rewriting(scope, after)
}
