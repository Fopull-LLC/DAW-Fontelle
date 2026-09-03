//! The arithmetic behind the piano roll's tools.
//!
//! Here rather than in the window because what a note may hold is a document
//! fact (`NoteProperty::range` is the clamp `set` applies), and a randomizer
//! that produced a velocity of zero would be producing a note-off. The tools
//! that *change* a document are commands; this is the part that decides what
//! to change it to, and it is a pure function so it can be checked against
//! every property and a few thousand seeds without a project in sight.
//!
//! # Why a seed and not a random number generator
//!
//! A randomizer that reached for the system's entropy would be untestable,
//! and — more to the point — un-*re-rollable*: "give me another one" and
//! "give me that one again" are both things you want from a tool like this,
//! and both need the roll to hold a number it can step. It also keeps this
//! crate free of a random-number dependency for eight lines of arithmetic.

use crate::note::NoteProperty;

/// How far a randomizer moves things, and in what sense.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RandomSpec {
    /// How much, as a percentage. `0` is the identity in both modes — a dial
    /// whose bottom is not "leave it alone" is one you cannot back away from.
    /// Anything over 100 is 100.
    pub amount: i32,
    pub mode: RandomMode,
}

/// The two things "randomize" can reasonably mean.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RandomMode {
    /// A wobble around what is already there. The humanising one: a phrase
    /// shaped by hand keeps its shape, and no note strays further than
    /// `amount` percent of the property's range from where it was.
    Around,
    /// A fresh value from the whole range, mixed with the old one by
    /// `amount`. At 100 what was there stops mattering at all; in between it
    /// is a continuous slide from one to the other, so the dial has no jump
    /// in the middle.
    Anywhere,
}

impl RandomMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Around => "Around",
            Self::Anywhere => "Anywhere",
        }
    }

    /// The other one. Two states, so the chip that shows it toggles.
    pub fn next(self) -> Self {
        match self {
            Self::Around => Self::Anywhere,
            Self::Anywhere => Self::Around,
        }
    }
}

/// The widest a dial goes.
pub const MAX_RANDOM_AMOUNT: i32 = 100;

/// `values`, randomised — one roll per value, every answer inside what the
/// property may hold.
///
/// **A roll per value, not one per call.** A generator seeded once and walked
/// down the list would still give every note its own number, but seeding it
/// from the value's *position* is what makes two identical notes come out
/// different and makes the answer independent of how many notes were selected.
pub fn randomised(
    values: &[i32],
    property: NoteProperty,
    spec: RandomSpec,
    seed: u64,
) -> Vec<i32> {
    let (min, max) = property.range();
    let amount = spec.amount.clamp(0, MAX_RANDOM_AMOUNT);
    if amount == 0 {
        return values.to_vec();
    }
    let span = (max - min) as i64;

    values
        .iter()
        .enumerate()
        .map(|(index, value)| {
            let roll = split_mix(seed ^ (index as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15));
            let answer = match spec.mode {
                RandomMode::Around => {
                    // A signed offset up to `amount` percent of the range
                    // either way. Taken from the roll's full width and then
                    // scaled, so the distribution is even rather than
                    // clustered at one end.
                    let reach = span * amount as i64 / MAX_RANDOM_AMOUNT as i64;
                    let offset = if reach == 0 {
                        0
                    } else {
                        (roll % (reach * 2 + 1) as u64) as i64 - reach
                    };
                    *value as i64 + offset
                }
                RandomMode::Anywhere => {
                    let fresh = min as i64 + (roll % (span + 1) as u64) as i64;
                    // Towards the fresh value by `amount` — the same
                    // arithmetic a crossfade is, which is what makes the dial
                    // continuous.
                    *value as i64
                        + (fresh - *value as i64) * amount as i64 / MAX_RANDOM_AMOUNT as i64
                }
            };
            answer.clamp(min as i64, max as i64) as i32
        })
        .collect()
}

/// SplitMix64: one multiply-xor-shift round, which is enough for a tool whose
/// output a person is going to look at and re-roll if they do not like it.
///
/// Written out rather than depended on: it is eight lines, it is the standard
/// finaliser, and a random-number crate in the *document* layer would be a
/// dependency the audio thread's crate graph did not need.
fn split_mix(seed: u64) -> u64 {
    let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_mixer_does_not_return_its_own_seed() {
        // The one property a finaliser has to have: a seed of 0, 1, 2 must
        // not come out as 0, 1, 2, or "seed 0" would randomise nothing.
        for seed in 0..8u64 {
            assert_ne!(split_mix(seed), seed);
        }
    }

    #[test]
    fn neighbouring_seeds_do_not_give_neighbouring_answers() {
        // What makes stepping the seed by one a real re-roll.
        let a = split_mix(1000);
        let b = split_mix(1001);
        assert!(a.abs_diff(b) > u64::MAX / 16, "{a} and {b} are too close");
    }
}
