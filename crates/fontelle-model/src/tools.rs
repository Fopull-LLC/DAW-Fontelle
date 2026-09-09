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

use crate::note::{Note, NoteProperty};
use fontelle_types::Tick;

/// Every note stretched — or pulled back — until it touches the one after it.
///
/// > *"if i press ctrl l with a note selection in the piano roll it makes all
/// > the notes lengths not have gaps like how it does in fl studio with that
/// > same keybind. just makes all the notes cleanly connect to eachother
/// > basically in length."*
///
/// FL Studio's Quick Legato. `spans` is `(start, length)` per note in whatever
/// order the caller has them — the selection's, which is the order they were
/// clicked — and the answer is the new length for each, in the same order.
///
/// Three rules, and each of them is a test:
///
/// - **A start, not a note, is what a note reaches.** Notes sharing a tick are
///   one musical event: a chord's notes all reach the *next* event, and none
///   of them is "the next note" for the other two. Grouping by note instead
///   would collapse every voice of a chord but the top one to nothing.
/// - **It shortens as well as lengthens.** A note running under the one after
///   it is pulled back to it. "At least touch" would mean a phrase run through
///   the tool twice kept growing, and there would be no way back.
/// - **The last event keeps the length it had.** There is nothing after it to
///   touch, and picking a length for it — the previous gap, a beat, the end of
///   the clip — would be the tool inventing something nobody asked for.
///
/// Never zero: distinct starts are at least one tick apart, so the length this
/// hands back is at least one, which is what `SetNoteLengths` will accept.
pub fn legato_lengths(spans: &[(Tick, Tick)]) -> Vec<Tick> {
    // The distinct starts, in order. Small and already nearly sorted in
    // practice, and this is a keypress rather than a per-frame path.
    let mut starts: Vec<Tick> = spans.iter().map(|(start, _)| *start).collect();
    starts.sort_unstable();
    starts.dedup();

    spans
        .iter()
        .map(
            |(start, length)| match starts.iter().find(|next| *next > start) {
                Some(next) => next - start,
                None => *length,
            },
        )
        .collect()
}

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
pub fn randomised(values: &[i32], property: NoteProperty, spec: RandomSpec, seed: u64) -> Vec<i32> {
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

// ------------------------------------------------------- the arpeggiator ---

/// Which way an arpeggio walks the chord.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ArpDirection {
    /// Lowest to highest, then round again.
    #[default]
    Up,
    Down,
    /// Up and back down **without repeating the ends** — C E G E, not
    /// C E G G E C. Repeating them is the other reading and it stutters: the
    /// top note lands twice in a row and the run limps.
    UpDown,
    DownUp,
    /// The order the notes were written in. A chord entered high note first
    /// arpeggiates high note first, which is the only way to get an order the
    /// other five cannot give you.
    AsPlayed,
    /// A different note each step, never the same one twice running.
    Random,
}

impl ArpDirection {
    pub const ALL: [Self; 6] = [
        Self::Up,
        Self::Down,
        Self::UpDown,
        Self::DownUp,
        Self::AsPlayed,
        Self::Random,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Up => "up",
            Self::Down => "down",
            Self::UpDown => "up-down",
            Self::DownUp => "down-up",
            Self::AsPlayed => "as played",
            Self::Random => "random",
        }
    }
}

/// What an arpeggio is made of.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArpSpec {
    /// One step, in ticks — the grid the run sits on.
    pub step: Tick,
    pub direction: ArpDirection,
    /// How many octaves the run climbs before it starts again. One is the
    /// chord as written.
    pub octaves: u8,
    /// How much of its step each note sounds for, 0..=1. Under one is
    /// staccato and one is a legato run of touching notes.
    pub gate: f32,
    /// How many steps each pitch holds for before the run moves on.
    pub repeats: u8,
    /// How late the off-beat steps land, 0..=1 of the space they have.
    ///
    /// **Not FL's**, and the reason to add it: an arp on a perfectly straight
    /// grid is the most obviously machine-made thing anybody puts in a
    /// project. Half of one step's gap is a triplet feel; anything is better
    /// than none.
    pub swing: f32,
    /// How the weight moves across the run, −1..=1 — falling, flat, climbing.
    ///
    /// Also not FL's, and for the same reason: a run in which every note is
    /// struck identically reads as a preset rather than as playing.
    pub velocity_ramp: f32,
}

impl Default for ArpSpec {
    /// Sixteenths, up, one octave, half gate — the setting somebody would
    /// reach for first and the one FL opens on.
    fn default() -> Self {
        Self {
            step: fontelle_types::PPQN / 4,
            direction: ArpDirection::Up,
            octaves: 1,
            gate: 0.5,
            repeats: 1,
            swing: 0.0,
            velocity_ramp: 0.0,
        }
    }
}

/// The widest the octave range and the repeat count go.
pub const MAX_ARP_OCTAVES: u8 = 4;
pub const MAX_ARP_REPEATS: u8 = 4;

/// `notes`, turned into arpeggios — one run per **chord**.
///
/// A chord here is a group of notes that overlap in time, which is what a
/// chord is to the roll: three notes struck together are one, and two chords
/// one after the other are two runs rather than one long one across the gap
/// between them. Each run fills its own chord's span exactly and no further —
/// an arpeggiator that ran past the notes it replaced would be changing the
/// length of the part.
///
/// Pure, and it returns the notes to **insert**; the caller removes the ones
/// it was given. That split is what lets the roll do it as one undo entry
/// without this function knowing what an undo entry is.
pub fn arpeggiated(notes: &[Note], spec: ArpSpec) -> Vec<Note> {
    if notes.is_empty() || spec.step <= 0 {
        return Vec::new();
    }
    let mut out = Vec::new();
    for chord in chords(notes) {
        out.extend(one_arpeggio(&chord, spec));
    }
    out.sort_by_key(|note| (note.start, note.key));
    out
}

/// The notes grouped into chords: each group overlaps in time, in the order
/// they were written.
fn chords(notes: &[Note]) -> Vec<Vec<Note>> {
    let mut sorted: Vec<Note> = notes.to_vec();
    sorted.sort_by_key(|note| note.start);
    let mut groups: Vec<Vec<Note>> = Vec::new();
    for note in sorted {
        match groups.last_mut() {
            // Overlapping the group so far — the same chord. Measured against
            // the group's **end**, so a held bass note under a moving line
            // does not split the line into one chord per note.
            Some(group)
                if group
                    .iter()
                    .any(|held| note.start < held.start + held.length) =>
            {
                group.push(note);
            }
            _ => groups.push(vec![note]),
        }
    }
    groups
}

/// One chord's run.
fn one_arpeggio(chord: &[Note], spec: ArpSpec) -> Vec<Note> {
    let start = chord.iter().map(|note| note.start).min().unwrap_or(0);
    let end = chord
        .iter()
        .map(|note| note.start + note.length)
        .max()
        .unwrap_or(0);
    let span = (end - start).max(0);
    if span <= 0 {
        return Vec::new();
    }
    // The pitches, once each, in the order this direction wants them.
    let mut keys: Vec<u8> = Vec::new();
    for note in chord {
        if !keys.contains(&note.key) {
            keys.push(note.key);
        }
    }
    if spec.direction != ArpDirection::AsPlayed {
        keys.sort_unstable();
    }
    let octaves = spec.octaves.clamp(1, MAX_ARP_OCTAVES);
    let ladder: Vec<u8> = (0..octaves)
        .flat_map(|octave| {
            keys.iter()
                .filter_map(move |key| key.checked_add(octave * 12))
                .filter(|key| *key <= 127)
        })
        .collect();
    if ladder.is_empty() {
        return Vec::new();
    }

    let steps = (span / spec.step).max(1) as usize;
    let repeats = spec.repeats.clamp(1, MAX_ARP_REPEATS) as usize;
    let gate = spec.gate.clamp(0.05, 1.0);
    let swing = spec.swing.clamp(0.0, 1.0);
    let ramp = spec.velocity_ramp.clamp(-1.0, 1.0);
    // The weight of the chord it replaces, so a quiet chord makes a quiet run.
    let base = chord
        .iter()
        .map(|note| i32::from(note.velocity))
        .max()
        .unwrap_or(100);

    let mut out = Vec::with_capacity(steps);
    let mut state = 0x9E37_79B9u32;
    let mut last = usize::MAX;
    for index in 0..steps {
        let which = index / repeats;
        let at = match spec.direction {
            ArpDirection::Up | ArpDirection::AsPlayed => which % ladder.len(),
            ArpDirection::Down => ladder.len() - 1 - which % ladder.len(),
            // The turn is `2n − 2` long, not `2n`: the ends are not repeated.
            ArpDirection::UpDown | ArpDirection::DownUp => {
                let turn = (ladder.len() * 2).saturating_sub(2).max(1);
                let at = which % turn;
                let up = if at < ladder.len() { at } else { turn - at };
                if spec.direction == ArpDirection::UpDown {
                    up
                } else {
                    ladder.len() - 1 - up
                }
            }
            ArpDirection::Random => {
                // Never the same note twice running: a random arp that
                // repeats a pitch sounds like a mistake rather than a choice.
                let mut pick = last;
                for _ in 0..8 {
                    state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    let next = (state >> 16) as usize % ladder.len();
                    if next != last || ladder.len() == 1 {
                        pick = next;
                        break;
                    }
                }
                pick.min(ladder.len() - 1)
            }
        };
        last = at;

        // Where the step lands. The off-beats move late; the down-beats never
        // do, which is what keeps the bar in place while the feel changes.
        let grid = start + spec.step * index as Tick;
        let offset = if index % 2 == 1 {
            (spec.step as f32 * swing * 0.5) as Tick
        } else {
            0
        };
        let at_tick = grid + offset;
        // The note stops at its gate, at the next step's start, or at the end
        // of the chord — whichever comes first. A run that overshot the chord
        // would be an arpeggiator that made the part longer.
        let next = start
            + spec.step * (index as Tick + 1)
            + if (index + 1) % 2 == 1 {
                (spec.step as f32 * swing * 0.5) as Tick
            } else {
                0
            };
        let length = ((spec.step as f32 * gate) as Tick)
            .min(next - at_tick)
            .min(end - at_tick)
            .max(1);
        if at_tick >= end {
            break;
        }

        let along = if steps > 1 {
            index as f32 / (steps - 1) as f32
        } else {
            0.0
        };
        // The ramp is a share of the room the weight has above or below it,
        // so it can never push a note past full or under silence.
        let velocity = (base as f32 + ramp * along * 60.0).clamp(1.0, 127.0) as u8;

        // Everything else — pan, the mod values, the channel — comes from the
        // chord's own lowest note, so an arpeggiated part keeps whatever
        // character was set on the chord it came from.
        let template = chord
            .iter()
            .min_by_key(|note| note.key)
            .copied()
            .unwrap_or(chord[0]);
        out.push(Note {
            start: at_tick,
            length,
            key: ladder[at],
            velocity,
            // A run of slide notes would bend one voice through the whole
            // arpeggio and sound nothing at all like an arpeggio.
            slide: false,
            ..template
        });
    }
    out
}
