//! Lapse's factory bank: sixty-four rows in nine categories, and six kits
//! (`docs/lapse-plan.md` §8).
//!
//! Every one of them is two or three drawn points. That is the whole claim
//! this effect makes — that a stutter, a freeze, a scratch, a tape stop, a
//! rewind, a trance gate and a groove template are *one machine at seven
//! settings* — and the shortness of the recipes below is the evidence for it.
//!
//! # How to read a recipe
//!
//! A **time** point is `(phase, offset-in-lane-lengths, shape)`. Zero offset
//! is now; −0.5 is half a lane back. A segment's *slope* is what is heard:
//!
//! - flat: normal speed, delayed by whatever the offset is;
//! - falling one lane-length per lane (`0.0 → −1.0` across the whole lane): a
//!   **freeze**;
//! - half that: an octave down;
//! - twice that (`0.0 → −1.0` across *half* the lane): **reverse** at the
//!   song's own speed;
//! - a `Stepped` point: a jump backwards, which is a stutter's repeat.
//!
//! A **volume** point is the same shape with an amplitude: 1 is unity, 0 is
//! silence, and `Stepped` is a gate's edge.
//!
//! # Why Groove is the category to get right
//!
//! Eight curves of a few milliseconds of push and drag, applied to *audio*.
//! It is a groove template for a bounced loop, which nothing in this class
//! offers because nobody thought of the time lane as micro-timing — and it is
//! the one people will reach for on every project rather than on every
//! eighth build-up.

use crate::curve::CurveShape;
use crate::lapse::{
    LAPSE_SCENES, LapseBank, LapseConfig, LapseLaneKind, LapseLength, LapsePoint, LapseScene,
};

/// One factory row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LapsePreset {
    pub name: &'static str,
    pub category: &'static str,
    /// One sentence: what it is for.
    pub notes: &'static str,
    recipe: Recipe,
}

/// What a row is made of. A kit fills all twelve scenes; everything else
/// draws scene one and leaves the rest flat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Recipe {
    One(Scene),
    Kit(&'static [Scene]),
}

/// One scene's worth of drawing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Scene {
    time: &'static [P],
    volume: &'static [P],
    length: LapseLength,
}

impl Scene {
    const fn new(time: &'static [P], volume: &'static [P]) -> Self {
        Self {
            time,
            volume,
            length: LapseLength::Bar,
        }
    }

    const fn beat(time: &'static [P], volume: &'static [P]) -> Self {
        Self {
            time,
            volume,
            length: LapseLength::Beat,
        }
    }

    const fn two_bars(time: &'static [P], volume: &'static [P]) -> Self {
        Self {
            time,
            volume,
            length: LapseLength::TwoBars,
        }
    }
}

/// A point, in tenths of a thousandth so the table can be `const`: `at` and
/// `value` are milli-units (1000 = 1.0), which keeps every recipe readable as
/// integers and exact as fractions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct P(i32, i32, Shape);

/// The shapes a recipe uses, short enough to keep a table in one line each.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shape {
    /// Linear.
    L,
    /// Stepped — a jump at the next point.
    S,
    /// An S-curve, for the ones that should not start with a corner.
    C,
    /// Exponential: slow to start.
    E,
}

impl Shape {
    const fn shape(self) -> CurveShape {
        match self {
            Self::L => CurveShape::Linear,
            Self::S => CurveShape::Stepped,
            Self::C => CurveShape::SCurve,
            Self::E => CurveShape::Exponential,
        }
    }
}

fn points_of(points: &[P]) -> Vec<LapsePoint> {
    points
        .iter()
        .map(|P(at, value, shape)| {
            LapsePoint::new(*at as f64 / 1000.0, *value as f64 / 1000.0, shape.shape())
        })
        .collect()
}

/// The flat lanes a recipe does not draw on.
const FLAT_TIME: &[P] = &[P(0, 0, Shape::L)];
const FLAT_VOLUME: &[P] = &[P(0, 1000, Shape::L)];

impl LapsePreset {
    /// Every factory row, in the order the browser lists them.
    pub const ALL: &'static [LapsePreset] = ROWS;

    /// The bank and the knobs this row is.
    pub fn build(&self) -> (LapseConfig, LapseBank) {
        let mut bank = LapseBank::new();
        match self.recipe {
            Recipe::One(scene) => draw(&mut bank.scenes[0], &scene),
            Recipe::Kit(scenes) => {
                for (index, scene) in scenes.iter().take(LAPSE_SCENES).enumerate() {
                    draw(&mut bank.scenes[index], scene);
                }
            }
        }
        (LapseConfig::new(), bank)
    }

    /// Whether this row fills every scene — what the browser's row says, and
    /// what `preset_bank.rs`'s gate checks.
    pub fn is_kit(&self) -> bool {
        matches!(self.recipe, Recipe::Kit(_))
    }
}

fn draw(scene: &mut LapseScene, recipe: &Scene) {
    let time = scene.lane_mut(LapseLaneKind::Time).expect("four lanes");
    time.length = recipe.length;
    time.on = true;
    time.points = points_of(recipe.time);
    time.tidy(LapseLaneKind::Time);

    let volume = scene.lane_mut(LapseLaneKind::Volume).expect("four lanes");
    volume.length = recipe.length;
    volume.on = true;
    volume.points = points_of(recipe.volume);
    volume.tidy(LapseLaneKind::Volume);
}

const fn row(
    name: &'static str,
    category: &'static str,
    notes: &'static str,
    scene: Scene,
) -> LapsePreset {
    LapsePreset {
        name,
        category,
        notes,
        recipe: Recipe::One(scene),
    }
}

const fn kit(name: &'static str, notes: &'static str, scenes: &'static [Scene]) -> LapsePreset {
    LapsePreset {
        name,
        category: "Kits",
        notes,
        recipe: Recipe::Kit(scenes),
    }
}

// --------------------------------------------------------------- the rows

/// A repeat of the last `1/n` of the lane: the read jumps back by that much
/// at every division. The commonest shape in the bank, and the one that is
/// four points of arithmetic rather than an algorithm.
const ROLL_4: Scene = Scene::new(
    &[
        P(0, 0, Shape::S),
        P(250, -250, Shape::S),
        P(500, -500, Shape::S),
        P(750, -750, Shape::S),
    ],
    FLAT_VOLUME,
);
const ROLL_8: Scene = Scene::new(
    &[
        P(0, 0, Shape::S),
        P(125, -125, Shape::S),
        P(250, -250, Shape::S),
        P(375, -375, Shape::S),
        P(500, -500, Shape::S),
        P(625, -625, Shape::S),
        P(750, -750, Shape::S),
        P(875, -875, Shape::S),
    ],
    FLAT_VOLUME,
);
const ROLL_16: Scene = Scene::beat(
    &[
        P(0, 0, Shape::S),
        P(250, -250, Shape::S),
        P(500, -500, Shape::S),
        P(750, -750, Shape::S),
    ],
    FLAT_VOLUME,
);

static ROWS: &[LapsePreset] = &[
    // ------------------------------------------------------------ Stutter
    row(
        "Quarter Roll",
        "Stutter",
        "The bar's first beat, four times.",
        ROLL_4,
    ),
    row(
        "Eighth Roll",
        "Stutter",
        "Eight repeats of the first eighth.",
        ROLL_8,
    ),
    row(
        "Sixteenth Roll",
        "Stutter",
        "A beat of sixteenths, on a one-beat lane.",
        ROLL_16,
    ),
    row(
        "Triplet Roll",
        "Stutter",
        "Three repeats to the beat — the swung cousin of the 1/8.",
        Scene::new(
            &[
                P(0, 0, Shape::S),
                P(167, -167, Shape::S),
                P(333, -333, Shape::S),
                P(500, -500, Shape::S),
                P(667, -667, Shape::S),
                P(833, -833, Shape::S),
            ],
            FLAT_VOLUME,
        ),
    ),
    row(
        "Accelerando",
        "Stutter",
        "Repeats that get closer together: the build in one curve.",
        Scene::new(
            &[
                P(0, 0, Shape::S),
                P(400, -400, Shape::S),
                P(650, -250, Shape::S),
                P(800, -150, Shape::S),
                P(900, -100, Shape::S),
                P(960, -60, Shape::S),
            ],
            FLAT_VOLUME,
        ),
    ),
    row(
        "Ratchet 3",
        "Stutter",
        "Three fast repeats on the last beat, the rest untouched.",
        Scene::new(
            &[
                P(0, 0, Shape::S),
                P(750, 0, Shape::S),
                P(833, -83, Shape::S),
                P(917, -167, Shape::S),
            ],
            FLAT_VOLUME,
        ),
    ),
    row(
        "Ratchet 5",
        "Stutter",
        "Five of them, tighter. Good on a snare.",
        Scene::new(
            &[
                P(0, 0, Shape::S),
                P(750, 0, Shape::S),
                P(800, -50, Shape::S),
                P(850, -100, Shape::S),
                P(900, -150, Shape::S),
                P(950, -200, Shape::S),
            ],
            FLAT_VOLUME,
        ),
    ),
    row(
        "Buzz",
        "Stutter",
        "A thirty-second repeat: past rhythm and into a tone.",
        Scene::beat(
            &[
                P(0, 0, Shape::S),
                P(125, -125, Shape::S),
                P(250, -250, Shape::S),
                P(375, -375, Shape::S),
                P(500, -500, Shape::S),
                P(625, -625, Shape::S),
                P(750, -750, Shape::S),
                P(875, -875, Shape::S),
            ],
            FLAT_VOLUME,
        ),
    ),
    // --------------------------------------------------------------- Hold
    row(
        "Beat 1",
        "Hold",
        "The downbeat, held for the rest of the bar.",
        Scene::new(
            &[
                P(0, 0, Shape::L),
                P(250, -250, Shape::L),
                P(1000, -1000, Shape::L),
            ],
            FLAT_VOLUME,
        ),
    ),
    row(
        "Half Bar",
        "Hold",
        "Plays a half, then stops dead until the bar turns.",
        Scene::new(
            &[
                P(0, 0, Shape::S),
                P(500, 0, Shape::L),
                P(1000, -500, Shape::L),
            ],
            FLAT_VOLUME,
        ),
    ),
    row(
        "Last Eighth",
        "Hold",
        "Everything as it is, then the last eighth freezes.",
        Scene::new(
            &[
                P(0, 0, Shape::S),
                P(875, 0, Shape::L),
                P(1000, -125, Shape::L),
            ],
            FLAT_VOLUME,
        ),
    ),
    row(
        "Freeze & Release",
        "Hold",
        "Holds through the third beat and lets go on the fourth.",
        Scene::new(
            &[
                P(0, 0, Shape::S),
                P(500, 0, Shape::L),
                P(750, -250, Shape::S),
            ],
            FLAT_VOLUME,
        ),
    ),
    row(
        "Stutter Hold",
        "Hold",
        "Two repeats, then a freeze: the fill that answers itself.",
        Scene::new(
            &[
                P(0, 0, Shape::S),
                P(250, -250, Shape::S),
                P(500, -500, Shape::L),
                P(1000, -1000, Shape::L),
            ],
            FLAT_VOLUME,
        ),
    ),
    row(
        "Hold Fade",
        "Hold",
        "A freeze that fades out under itself.",
        Scene::new(
            &[P(0, 0, Shape::L), P(1000, -1000, Shape::L)],
            &[P(0, 1000, Shape::L), P(1000, 0, Shape::C)],
        ),
    ),
    row(
        "Suspend",
        "Hold",
        "Freezes the middle two beats and carries on.",
        Scene::new(
            &[
                P(0, 0, Shape::S),
                P(250, 0, Shape::L),
                P(750, -500, Shape::S),
            ],
            FLAT_VOLUME,
        ),
    ),
    row(
        "Glitch Hold",
        "Hold",
        "A freeze broken by two jumps back — the CD skip.",
        Scene::new(
            &[
                P(0, 0, Shape::L),
                P(400, -400, Shape::S),
                P(500, -200, Shape::L),
                P(900, -600, Shape::S),
                P(950, -300, Shape::L),
            ],
            FLAT_VOLUME,
        ),
    ),
    // ------------------------------------------------------------ Scratch
    row(
        "Baby",
        "Scratch",
        "Forward, back, forward: the first scratch anybody learns.",
        Scene::beat(
            &[
                P(0, 0, Shape::L),
                P(250, -250, Shape::L),
                P(500, 0, Shape::L),
                P(750, -250, Shape::L),
                P(1000, 0, Shape::L),
            ],
            FLAT_VOLUME,
        ),
    ),
    row(
        "Forward",
        "Scratch",
        "A push and a return, with the return twice as fast.",
        Scene::beat(
            &[
                P(0, 0, Shape::L),
                P(600, -300, Shape::L),
                P(1000, 0, Shape::L),
            ],
            FLAT_VOLUME,
        ),
    ),
    row(
        "Chirp",
        "Scratch",
        "A scratch cut short by the fader — the volume does the work.",
        Scene::beat(
            &[
                P(0, 0, Shape::L),
                P(500, -400, Shape::L),
                P(1000, 0, Shape::L),
            ],
            &[
                P(0, 1000, Shape::S),
                P(450, 0, Shape::S),
                P(550, 1000, Shape::S),
            ],
        ),
    ),
    row(
        "Transformer",
        "Scratch",
        "A steady drag chopped into six by the gate.",
        Scene::beat(
            &[P(0, 0, Shape::L), P(1000, -500, Shape::L)],
            &[
                P(0, 1000, Shape::S),
                P(167, 0, Shape::S),
                P(333, 1000, Shape::S),
                P(500, 0, Shape::S),
                P(667, 1000, Shape::S),
                P(833, 0, Shape::S),
            ],
        ),
    ),
    row(
        "Tear",
        "Scratch",
        "The pull back done in three steps rather than one.",
        Scene::beat(
            &[
                P(0, 0, Shape::L),
                P(400, -400, Shape::L),
                P(550, -300, Shape::L),
                P(750, -500, Shape::L),
                P(1000, 0, Shape::L),
            ],
            FLAT_VOLUME,
        ),
    ),
    row(
        "Rub",
        "Scratch",
        "Slow, heavy, back and forth over a bar.",
        Scene::new(
            &[
                P(0, 0, Shape::C),
                P(250, -350, Shape::C),
                P(500, 0, Shape::C),
                P(750, -350, Shape::C),
                P(1000, 0, Shape::C),
            ],
            FLAT_VOLUME,
        ),
    ),
    row(
        "Stab",
        "Scratch",
        "One short forward push, gated at both ends.",
        Scene::beat(
            &[
                P(0, 0, Shape::S),
                P(250, 0, Shape::L),
                P(500, -250, Shape::S),
            ],
            &[
                P(0, 0, Shape::S),
                P(250, 1000, Shape::S),
                P(500, 0, Shape::S),
            ],
        ),
    ),
    row(
        "Wikki",
        "Scratch",
        "Four quick rubs to the beat.",
        Scene::beat(
            &[
                P(0, 0, Shape::L),
                P(125, -150, Shape::L),
                P(250, 0, Shape::L),
                P(375, -150, Shape::L),
                P(500, 0, Shape::L),
                P(625, -150, Shape::L),
                P(750, 0, Shape::L),
                P(875, -150, Shape::L),
            ],
            FLAT_VOLUME,
        ),
    ),
    // --------------------------------------------------------------- Tape
    row(
        "Tape Stop",
        "Tape",
        "Slows to a standstill over the bar, and comes back on the next.",
        Scene::new(&[P(0, 0, Shape::E), P(1000, -1000, Shape::E)], FLAT_VOLUME),
    ),
    row(
        "Tape Start",
        "Tape",
        "Its mirror: up to speed from a standstill.",
        Scene::new(
            &[P(0, -700, Shape::C), P(700, 0, Shape::C)],
            &[P(0, 200, Shape::C), P(700, 1000, Shape::C)],
        ),
    ),
    row(
        "Slow Dive",
        "Tape",
        "Half speed by the end of the bar — an octave down and falling.",
        Scene::new(&[P(0, 0, Shape::L), P(1000, -500, Shape::L)], FLAT_VOLUME),
    ),
    row(
        "Speed Up",
        "Tape",
        "A bar that finishes early and waits.",
        Scene::new(
            &[
                P(0, -500, Shape::L),
                P(800, 0, Shape::L),
                P(1000, 0, Shape::L),
            ],
            FLAT_VOLUME,
        ),
    ),
    row(
        "Half Speed",
        "Tape",
        "Two bars of memory played over four: the whole thing an octave down.",
        Scene::two_bars(&[P(0, 0, Shape::L), P(1000, -500, Shape::L)], FLAT_VOLUME),
    ),
    row(
        "Double Speed",
        "Tape",
        "The bar twice over, an octave up.",
        Scene::new(
            &[
                P(0, -500, Shape::L),
                P(500, 0, Shape::S),
                P(501, -500, Shape::L),
            ],
            FLAT_VOLUME,
        ),
    ),
    // ------------------------------------------------------------ Reverse
    row(
        "Reverse Beat 4",
        "Reverse",
        "Three beats forwards and the fourth backwards.",
        Scene::new(
            &[
                P(0, 0, Shape::S),
                P(750, 0, Shape::L),
                P(1000, -500, Shape::L),
            ],
            FLAT_VOLUME,
        ),
    ),
    row(
        "Reverse Eighth",
        "Reverse",
        "The last eighth, backwards — the smallest useful one.",
        Scene::new(
            &[
                P(0, 0, Shape::S),
                P(875, 0, Shape::L),
                P(1000, -250, Shape::L),
            ],
            FLAT_VOLUME,
        ),
    ),
    row(
        "Reverse Bar",
        "Reverse",
        "The whole bar, backwards.",
        Scene::two_bars(
            &[
                P(0, 0, Shape::S),
                P(500, 0, Shape::L),
                P(1000, -1000, Shape::L),
            ],
            FLAT_VOLUME,
        ),
    ),
    row(
        "Rewind",
        "Reverse",
        "Backwards and accelerating: the radio rewind.",
        Scene::new(&[P(0, 0, Shape::E), P(1000, -1000, Shape::L)], FLAT_VOLUME),
    ),
    row(
        "Backspin",
        "Reverse",
        "A hard spin back, then silence until the bar turns.",
        Scene::new(
            &[P(0, 0, Shape::L), P(400, -800, Shape::S)],
            &[P(0, 1000, Shape::L), P(400, 0, Shape::S)],
        ),
    ),
    row(
        "Reverse Tail",
        "Reverse",
        "Forwards, then the last quarter played back over itself.",
        Scene::new(
            &[
                P(0, 0, Shape::S),
                P(750, -250, Shape::L),
                P(1000, -750, Shape::L),
            ],
            FLAT_VOLUME,
        ),
    ),
    // --------------------------------------------------------------- Gate
    row(
        "Eighth Gate",
        "Gate",
        "Eight on, eight off: the plain one.",
        Scene::new(
            FLAT_TIME,
            &[
                P(0, 1000, Shape::S),
                P(125, 0, Shape::S),
                P(250, 1000, Shape::S),
                P(375, 0, Shape::S),
                P(500, 1000, Shape::S),
                P(625, 0, Shape::S),
                P(750, 1000, Shape::S),
                P(875, 0, Shape::S),
            ],
        ),
    ),
    row(
        "Sixteenth Gate",
        "Gate",
        "Twice as fast, on a one-beat lane.",
        Scene::beat(
            FLAT_TIME,
            &[
                P(0, 1000, Shape::S),
                P(125, 0, Shape::S),
                P(250, 1000, Shape::S),
                P(375, 0, Shape::S),
                P(500, 1000, Shape::S),
                P(625, 0, Shape::S),
                P(750, 1000, Shape::S),
                P(875, 0, Shape::S),
            ],
        ),
    ),
    row(
        "Trance 16",
        "Gate",
        "Sixteen steps with two of them held open: the classic pattern.",
        Scene::new(
            FLAT_TIME,
            &[
                P(0, 1000, Shape::S),
                P(62, 0, Shape::S),
                P(125, 1000, Shape::S),
                P(187, 0, Shape::S),
                P(250, 1000, Shape::S),
                P(375, 0, Shape::S),
                P(500, 1000, Shape::S),
                P(562, 0, Shape::S),
                P(625, 1000, Shape::S),
                P(750, 0, Shape::S),
                P(812, 1000, Shape::S),
                P(875, 0, Shape::S),
            ],
        ),
    ),
    row(
        "Offbeat",
        "Gate",
        "Shut on the beat, open between: the reggae upstroke.",
        Scene::new(
            FLAT_TIME,
            &[
                P(0, 0, Shape::S),
                P(125, 1000, Shape::S),
                P(250, 0, Shape::S),
                P(375, 1000, Shape::S),
                P(500, 0, Shape::S),
                P(625, 1000, Shape::S),
                P(750, 0, Shape::S),
                P(875, 1000, Shape::S),
            ],
        ),
    ),
    row(
        "Triplet Gate",
        "Gate",
        "Three to the beat, which no step sequencer does easily.",
        Scene::new(
            FLAT_TIME,
            &[
                P(0, 1000, Shape::S),
                P(83, 0, Shape::S),
                P(167, 1000, Shape::S),
                P(250, 0, Shape::S),
                P(333, 1000, Shape::S),
                P(417, 0, Shape::S),
                P(500, 1000, Shape::S),
                P(583, 0, Shape::S),
                P(667, 1000, Shape::S),
                P(750, 0, Shape::S),
                P(833, 1000, Shape::S),
                P(917, 0, Shape::S),
            ],
        ),
    ),
    row(
        "Sidechain Pump",
        "Gate",
        "A duck on every beat, with no compressor and no kick to key it.",
        Scene::new(
            FLAT_TIME,
            &[
                P(0, 0, Shape::C),
                P(200, 1000, Shape::S),
                P(250, 0, Shape::C),
                P(450, 1000, Shape::S),
                P(500, 0, Shape::C),
                P(700, 1000, Shape::S),
                P(750, 0, Shape::C),
                P(950, 1000, Shape::S),
            ],
        ),
    ),
    row(
        "Long Pump",
        "Gate",
        "One long duck a bar, for pads.",
        Scene::new(FLAT_TIME, &[P(0, 0, Shape::C), P(900, 1000, Shape::S)]),
    ),
    row(
        "Breathe",
        "Gate",
        "A slow swell over two bars. Barely a gate at all.",
        Scene::two_bars(
            FLAT_TIME,
            &[
                P(0, 400, Shape::C),
                P(500, 1000, Shape::C),
                P(1000, 400, Shape::C),
            ],
        ),
    ),
    // ------------------------------------------------------------- Groove
    // Time offsets of a few milliseconds, which at 120 bpm is a value of
    // about 0.005 of a bar. These are the rows people will use on every
    // project (`docs/lapse-plan.md` §8).
    row(
        "Swing 16",
        "Groove",
        "Pushes every second sixteenth late: swing, on audio.",
        Scene::new(
            &[
                P(0, 0, Shape::S),
                P(62, -12, Shape::S),
                P(125, 0, Shape::S),
                P(187, -12, Shape::S),
                P(250, 0, Shape::S),
                P(312, -12, Shape::S),
                P(375, 0, Shape::S),
                P(437, -12, Shape::S),
                P(500, 0, Shape::S),
                P(562, -12, Shape::S),
                P(625, 0, Shape::S),
                P(687, -12, Shape::S),
                P(750, 0, Shape::S),
                P(812, -12, Shape::S),
                P(875, 0, Shape::S),
                P(937, -12, Shape::S),
            ],
            FLAT_VOLUME,
        ),
    ),
    row(
        "Shuffle",
        "Groove",
        "The same, harder — most of the way to a triplet.",
        Scene::new(
            &[
                P(0, 0, Shape::S),
                P(125, -28, Shape::S),
                P(250, 0, Shape::S),
                P(375, -28, Shape::S),
                P(500, 0, Shape::S),
                P(625, -28, Shape::S),
                P(750, 0, Shape::S),
                P(875, -28, Shape::S),
            ],
            FLAT_VOLUME,
        ),
    ),
    row(
        "Push",
        "Groove",
        "Everything a hair early. Wants a beat of look-ahead.",
        Scene::new(&[P(0, 6, Shape::L)], FLAT_VOLUME),
    ),
    row(
        "Drag",
        "Groove",
        "Everything a hair late — the laid-back feel, on one knob.",
        Scene::new(&[P(0, -6, Shape::L)], FLAT_VOLUME),
    ),
    row(
        "Half-time",
        "Groove",
        "Beats 3 and 4 play beats 1 and 2 again.",
        Scene::new(&[P(0, 0, Shape::S), P(500, -500, Shape::S)], FLAT_VOLUME),
    ),
    row(
        "Double-time",
        "Groove",
        "The first half of the bar twice, at speed.",
        Scene::new(
            &[
                P(0, -250, Shape::L),
                P(500, 0, Shape::S),
                P(501, -250, Shape::L),
            ],
            FLAT_VOLUME,
        ),
    ),
    row(
        "Laid Back",
        "Groove",
        "The backbeat late and the downbeat where it was.",
        Scene::new(
            &[
                P(0, 0, Shape::S),
                P(250, -10, Shape::S),
                P(500, 0, Shape::S),
                P(750, -10, Shape::S),
            ],
            FLAT_VOLUME,
        ),
    ),
    row(
        "Rushed",
        "Groove",
        "The backbeat early instead. Needs look-ahead.",
        Scene::new(
            &[
                P(0, 0, Shape::S),
                P(250, 10, Shape::S),
                P(500, 0, Shape::S),
                P(750, 10, Shape::S),
            ],
            FLAT_VOLUME,
        ),
    ),
    // --------------------------------------------------------------- Fill
    row(
        "Bar Fill",
        "Fill",
        "Three beats as they are, then a roll into the next bar.",
        Scene::new(
            &[
                P(0, 0, Shape::S),
                P(750, 0, Shape::S),
                P(812, -62, Shape::S),
                P(875, -125, Shape::S),
                P(937, -187, Shape::S),
            ],
            FLAT_VOLUME,
        ),
    ),
    row(
        "Riser Gate",
        "Fill",
        "A gate that speeds up across the bar.",
        Scene::new(
            FLAT_TIME,
            &[
                P(0, 1000, Shape::S),
                P(250, 0, Shape::S),
                P(375, 1000, Shape::S),
                P(500, 0, Shape::S),
                P(625, 1000, Shape::S),
                P(687, 0, Shape::S),
                P(750, 1000, Shape::S),
                P(812, 0, Shape::S),
                P(875, 1000, Shape::S),
                P(937, 0, Shape::S),
            ],
        ),
    ),
    row(
        "Drop Out",
        "Fill",
        "Silence for the last beat: the oldest trick there is.",
        Scene::new(FLAT_TIME, &[P(0, 1000, Shape::S), P(750, 0, Shape::S)]),
    ),
    row(
        "Build Stutter",
        "Fill",
        "Repeats that halve in length while the level climbs.",
        Scene::new(
            &[
                P(0, 0, Shape::S),
                P(500, -500, Shape::S),
                P(750, -250, Shape::S),
                P(875, -125, Shape::S),
                P(937, -62, Shape::S),
            ],
            &[P(0, 700, Shape::C), P(1000, 1000, Shape::L)],
        ),
    ),
    row(
        "Last Beat Roll",
        "Fill",
        "Four sixteenths of the last beat, repeated.",
        Scene::new(
            &[
                P(0, 0, Shape::S),
                P(750, 0, Shape::S),
                P(812, -62, Shape::S),
                P(875, -62, Shape::S),
                P(937, -125, Shape::S),
            ],
            FLAT_VOLUME,
        ),
    ),
    row(
        "Silence & Return",
        "Fill",
        "A beat of nothing, then the bar picks up where it would have been.",
        Scene::new(
            FLAT_TIME,
            &[
                P(0, 1000, Shape::S),
                P(500, 0, Shape::S),
                P(750, 1000, Shape::S),
            ],
        ),
    ),
    // ----------------------------------------------------------- Creative
    row(
        "Ghost Notes",
        "Creative",
        "Quiet repeats between the beats: something that was not played.",
        Scene::new(
            &[
                P(0, 0, Shape::S),
                P(125, -125, Shape::S),
                P(250, 0, Shape::S),
                P(375, -125, Shape::S),
                P(500, 0, Shape::S),
                P(625, -125, Shape::S),
                P(750, 0, Shape::S),
                P(875, -125, Shape::S),
            ],
            &[
                P(0, 1000, Shape::S),
                P(125, 350, Shape::S),
                P(250, 1000, Shape::S),
                P(375, 350, Shape::S),
                P(500, 1000, Shape::S),
                P(625, 350, Shape::S),
                P(750, 1000, Shape::S),
                P(875, 350, Shape::S),
            ],
        ),
    ),
    row(
        "Broken Tape",
        "Creative",
        "Speed that sags and recovers, twice a bar.",
        Scene::new(
            &[
                P(0, 0, Shape::C),
                P(200, -80, Shape::C),
                P(400, 0, Shape::C),
                P(700, -120, Shape::C),
                P(900, 0, Shape::C),
            ],
            FLAT_VOLUME,
        ),
    ),
    row(
        "Hiccup",
        "Creative",
        "One thirty-second jump back, once a bar. Nobody knows why.",
        Scene::new(
            &[
                P(0, 0, Shape::S),
                P(687, -31, Shape::S),
                P(719, 0, Shape::S),
            ],
            FLAT_VOLUME,
        ),
    ),
    row(
        "Time Melt",
        "Creative",
        "Slows, holds, then runs to catch up.",
        Scene::new(
            &[
                P(0, 0, Shape::L),
                P(400, -200, Shape::L),
                P(700, -500, Shape::L),
                P(1000, 0, Shape::L),
            ],
            FLAT_VOLUME,
        ),
    ),
    row(
        "Drunk",
        "Creative",
        "Every beat a little off, none of them the same way.",
        Scene::new(
            &[
                P(0, 0, Shape::S),
                P(250, -22, Shape::S),
                P(500, 14, Shape::S),
                P(750, -34, Shape::S),
            ],
            FLAT_VOLUME,
        ),
    ),
    row(
        "Quantum",
        "Creative",
        "Jumps to a different part of the bar every eighth.",
        Scene::new(
            &[
                P(0, 0, Shape::S),
                P(125, -625, Shape::S),
                P(250, -250, Shape::S),
                P(375, -875, Shape::S),
                P(500, -125, Shape::S),
                P(625, -500, Shape::S),
                P(750, -375, Shape::S),
                P(875, -750, Shape::S),
            ],
            FLAT_VOLUME,
        ),
    ),
    // --------------------------------------------------------------- Kits
    // Twelve scenes each, laid out C to B so the whole thing is playable
    // from one octave of a keyboard with the slot's notes pointed at a
    // channel (`docs/lapse-plan.md` §4.7).
    kit(
        "Roll Kit",
        "An octave of repeats, slowest at C and fastest at B.",
        &[
            Scene::new(FLAT_TIME, FLAT_VOLUME),
            ROLL_4,
            ROLL_8,
            ROLL_16,
            Scene::new(&[P(0, 0, Shape::S), P(500, -500, Shape::S)], FLAT_VOLUME),
            Scene::new(
                &[
                    P(0, 0, Shape::S),
                    P(333, -333, Shape::S),
                    P(667, -667, Shape::S),
                ],
                FLAT_VOLUME,
            ),
            Scene::beat(&[P(0, 0, Shape::S), P(500, -500, Shape::S)], FLAT_VOLUME),
            Scene::beat(
                &[
                    P(0, 0, Shape::S),
                    P(333, -333, Shape::S),
                    P(667, -667, Shape::S),
                ],
                FLAT_VOLUME,
            ),
            Scene::new(
                &[
                    P(0, 0, Shape::S),
                    P(167, -167, Shape::S),
                    P(333, -333, Shape::S),
                    P(500, -500, Shape::S),
                    P(667, -667, Shape::S),
                    P(833, -833, Shape::S),
                ],
                FLAT_VOLUME,
            ),
            Scene::beat(
                &[
                    P(0, 0, Shape::S),
                    P(125, -125, Shape::S),
                    P(250, -250, Shape::S),
                    P(375, -375, Shape::S),
                    P(500, -500, Shape::S),
                    P(625, -625, Shape::S),
                    P(750, -750, Shape::S),
                    P(875, -875, Shape::S),
                ],
                FLAT_VOLUME,
            ),
            Scene::new(&[P(0, 0, Shape::L), P(1000, -1000, Shape::L)], FLAT_VOLUME),
            Scene::new(
                &[
                    P(0, 0, Shape::S),
                    P(750, 0, Shape::L),
                    P(1000, -250, Shape::L),
                ],
                FLAT_VOLUME,
            ),
        ],
    ),
    kit(
        "Scratch Kit",
        "Twelve scratches under twelve keys: play the turntable.",
        &[
            Scene::new(FLAT_TIME, FLAT_VOLUME),
            Scene::beat(
                &[
                    P(0, 0, Shape::L),
                    P(500, -250, Shape::L),
                    P(1000, 0, Shape::L),
                ],
                FLAT_VOLUME,
            ),
            Scene::beat(
                &[
                    P(0, 0, Shape::L),
                    P(250, -250, Shape::L),
                    P(500, 0, Shape::L),
                    P(750, -250, Shape::L),
                ],
                FLAT_VOLUME,
            ),
            Scene::beat(
                &[
                    P(0, 0, Shape::L),
                    P(600, -300, Shape::L),
                    P(1000, 0, Shape::L),
                ],
                FLAT_VOLUME,
            ),
            Scene::beat(
                &[
                    P(0, 0, Shape::L),
                    P(500, -400, Shape::L),
                    P(1000, 0, Shape::L),
                ],
                &[
                    P(0, 1000, Shape::S),
                    P(450, 0, Shape::S),
                    P(550, 1000, Shape::S),
                ],
            ),
            Scene::beat(
                &[P(0, 0, Shape::L), P(1000, -500, Shape::L)],
                &[
                    P(0, 1000, Shape::S),
                    P(167, 0, Shape::S),
                    P(333, 1000, Shape::S),
                    P(500, 0, Shape::S),
                    P(667, 1000, Shape::S),
                    P(833, 0, Shape::S),
                ],
            ),
            Scene::beat(
                &[
                    P(0, 0, Shape::L),
                    P(400, -400, Shape::L),
                    P(550, -300, Shape::L),
                    P(750, -500, Shape::L),
                    P(1000, 0, Shape::L),
                ],
                FLAT_VOLUME,
            ),
            Scene::new(
                &[
                    P(0, 0, Shape::C),
                    P(250, -350, Shape::C),
                    P(500, 0, Shape::C),
                    P(750, -350, Shape::C),
                    P(1000, 0, Shape::C),
                ],
                FLAT_VOLUME,
            ),
            Scene::beat(
                &[
                    P(0, 0, Shape::S),
                    P(250, 0, Shape::L),
                    P(500, -250, Shape::S),
                ],
                &[
                    P(0, 0, Shape::S),
                    P(250, 1000, Shape::S),
                    P(500, 0, Shape::S),
                ],
            ),
            Scene::beat(
                &[
                    P(0, 0, Shape::L),
                    P(125, -150, Shape::L),
                    P(250, 0, Shape::L),
                    P(375, -150, Shape::L),
                    P(500, 0, Shape::L),
                    P(625, -150, Shape::L),
                    P(750, 0, Shape::L),
                    P(875, -150, Shape::L),
                ],
                FLAT_VOLUME,
            ),
            Scene::new(
                &[P(0, 0, Shape::L), P(400, -800, Shape::S)],
                &[P(0, 1000, Shape::L), P(400, 0, Shape::S)],
            ),
            Scene::new(&[P(0, 0, Shape::E), P(1000, -1000, Shape::E)], FLAT_VOLUME),
        ],
    ),
    kit(
        "Stop Kit",
        "Tape stops and freezes of every length, under one hand.",
        &[
            Scene::new(FLAT_TIME, FLAT_VOLUME),
            Scene::new(
                &[
                    P(0, 0, Shape::S),
                    P(875, 0, Shape::L),
                    P(1000, -125, Shape::L),
                ],
                FLAT_VOLUME,
            ),
            Scene::new(
                &[
                    P(0, 0, Shape::S),
                    P(750, 0, Shape::L),
                    P(1000, -250, Shape::L),
                ],
                FLAT_VOLUME,
            ),
            Scene::new(
                &[
                    P(0, 0, Shape::S),
                    P(500, 0, Shape::L),
                    P(1000, -500, Shape::L),
                ],
                FLAT_VOLUME,
            ),
            Scene::new(&[P(0, 0, Shape::L), P(1000, -1000, Shape::L)], FLAT_VOLUME),
            Scene::new(&[P(0, 0, Shape::E), P(1000, -1000, Shape::E)], FLAT_VOLUME),
            Scene::two_bars(&[P(0, 0, Shape::E), P(1000, -1000, Shape::E)], FLAT_VOLUME),
            Scene::new(&[P(0, 0, Shape::L), P(1000, -500, Shape::L)], FLAT_VOLUME),
            Scene::new(
                &[P(0, 0, Shape::L), P(1000, -1000, Shape::L)],
                &[P(0, 1000, Shape::L), P(1000, 0, Shape::C)],
            ),
            Scene::new(
                &[
                    P(0, 0, Shape::S),
                    P(750, 0, Shape::L),
                    P(1000, -500, Shape::L),
                ],
                FLAT_VOLUME,
            ),
            Scene::new(&[P(0, 0, Shape::E), P(1000, -1000, Shape::L)], FLAT_VOLUME),
            Scene::new(
                &[P(0, 0, Shape::L), P(400, -800, Shape::S)],
                &[P(0, 1000, Shape::L), P(400, 0, Shape::S)],
            ),
        ],
    ),
    kit(
        "Gate Kit",
        "Twelve gates from a slow pump to a buzz.",
        &[
            Scene::new(FLAT_TIME, FLAT_VOLUME),
            Scene::new(FLAT_TIME, &[P(0, 0, Shape::C), P(900, 1000, Shape::S)]),
            Scene::new(
                FLAT_TIME,
                &[
                    P(0, 0, Shape::C),
                    P(200, 1000, Shape::S),
                    P(250, 0, Shape::C),
                    P(450, 1000, Shape::S),
                    P(500, 0, Shape::C),
                    P(700, 1000, Shape::S),
                    P(750, 0, Shape::C),
                    P(950, 1000, Shape::S),
                ],
            ),
            Scene::new(
                FLAT_TIME,
                &[
                    P(0, 1000, Shape::S),
                    P(250, 0, Shape::S),
                    P(500, 1000, Shape::S),
                    P(750, 0, Shape::S),
                ],
            ),
            Scene::new(
                FLAT_TIME,
                &[
                    P(0, 1000, Shape::S),
                    P(125, 0, Shape::S),
                    P(250, 1000, Shape::S),
                    P(375, 0, Shape::S),
                    P(500, 1000, Shape::S),
                    P(625, 0, Shape::S),
                    P(750, 1000, Shape::S),
                    P(875, 0, Shape::S),
                ],
            ),
            Scene::new(
                FLAT_TIME,
                &[
                    P(0, 0, Shape::S),
                    P(125, 1000, Shape::S),
                    P(250, 0, Shape::S),
                    P(375, 1000, Shape::S),
                    P(500, 0, Shape::S),
                    P(625, 1000, Shape::S),
                    P(750, 0, Shape::S),
                    P(875, 1000, Shape::S),
                ],
            ),
            Scene::new(
                FLAT_TIME,
                &[
                    P(0, 1000, Shape::S),
                    P(83, 0, Shape::S),
                    P(167, 1000, Shape::S),
                    P(250, 0, Shape::S),
                    P(333, 1000, Shape::S),
                    P(417, 0, Shape::S),
                    P(500, 1000, Shape::S),
                    P(583, 0, Shape::S),
                    P(667, 1000, Shape::S),
                    P(750, 0, Shape::S),
                    P(833, 1000, Shape::S),
                    P(917, 0, Shape::S),
                ],
            ),
            Scene::beat(
                FLAT_TIME,
                &[
                    P(0, 1000, Shape::S),
                    P(125, 0, Shape::S),
                    P(250, 1000, Shape::S),
                    P(375, 0, Shape::S),
                    P(500, 1000, Shape::S),
                    P(625, 0, Shape::S),
                    P(750, 1000, Shape::S),
                    P(875, 0, Shape::S),
                ],
            ),
            Scene::new(
                FLAT_TIME,
                &[
                    P(0, 1000, Shape::S),
                    P(62, 0, Shape::S),
                    P(125, 1000, Shape::S),
                    P(187, 0, Shape::S),
                    P(250, 1000, Shape::S),
                    P(375, 0, Shape::S),
                    P(500, 1000, Shape::S),
                    P(562, 0, Shape::S),
                    P(625, 1000, Shape::S),
                    P(750, 0, Shape::S),
                    P(812, 1000, Shape::S),
                    P(875, 0, Shape::S),
                ],
            ),
            Scene::new(FLAT_TIME, &[P(0, 1000, Shape::S), P(750, 0, Shape::S)]),
            Scene::new(
                FLAT_TIME,
                &[
                    P(0, 1000, Shape::S),
                    P(500, 0, Shape::S),
                    P(750, 1000, Shape::S),
                ],
            ),
            Scene::two_bars(
                FLAT_TIME,
                &[
                    P(0, 400, Shape::C),
                    P(500, 1000, Shape::C),
                    P(1000, 400, Shape::C),
                ],
            ),
        ],
    ),
    kit(
        "Groove Kit",
        "Twelve feels, from a hair early to a hard shuffle.",
        &[
            Scene::new(FLAT_TIME, FLAT_VOLUME),
            Scene::new(&[P(0, 6, Shape::L)], FLAT_VOLUME),
            Scene::new(&[P(0, 3, Shape::L)], FLAT_VOLUME),
            Scene::new(&[P(0, -3, Shape::L)], FLAT_VOLUME),
            Scene::new(&[P(0, -6, Shape::L)], FLAT_VOLUME),
            Scene::new(&[P(0, -12, Shape::L)], FLAT_VOLUME),
            Scene::new(
                &[
                    P(0, 0, Shape::S),
                    P(125, -12, Shape::S),
                    P(250, 0, Shape::S),
                    P(375, -12, Shape::S),
                    P(500, 0, Shape::S),
                    P(625, -12, Shape::S),
                    P(750, 0, Shape::S),
                    P(875, -12, Shape::S),
                ],
                FLAT_VOLUME,
            ),
            Scene::new(
                &[
                    P(0, 0, Shape::S),
                    P(125, -28, Shape::S),
                    P(250, 0, Shape::S),
                    P(375, -28, Shape::S),
                    P(500, 0, Shape::S),
                    P(625, -28, Shape::S),
                    P(750, 0, Shape::S),
                    P(875, -28, Shape::S),
                ],
                FLAT_VOLUME,
            ),
            Scene::new(
                &[
                    P(0, 0, Shape::S),
                    P(250, -10, Shape::S),
                    P(500, 0, Shape::S),
                    P(750, -10, Shape::S),
                ],
                FLAT_VOLUME,
            ),
            Scene::new(
                &[
                    P(0, 0, Shape::S),
                    P(250, 10, Shape::S),
                    P(500, 0, Shape::S),
                    P(750, 10, Shape::S),
                ],
                FLAT_VOLUME,
            ),
            Scene::new(&[P(0, 0, Shape::S), P(500, -500, Shape::S)], FLAT_VOLUME),
            Scene::new(
                &[
                    P(0, -250, Shape::L),
                    P(500, 0, Shape::S),
                    P(501, -250, Shape::L),
                ],
                FLAT_VOLUME,
            ),
        ],
    ),
    kit(
        "DJ Kit",
        "One of everything: a hold, a roll, a scratch, a stop, a gate.",
        &[
            Scene::new(FLAT_TIME, FLAT_VOLUME),
            Scene::new(
                &[
                    P(0, 0, Shape::S),
                    P(875, 0, Shape::L),
                    P(1000, -125, Shape::L),
                ],
                FLAT_VOLUME,
            ),
            ROLL_4,
            ROLL_8,
            ROLL_16,
            Scene::beat(
                &[
                    P(0, 0, Shape::L),
                    P(250, -250, Shape::L),
                    P(500, 0, Shape::L),
                    P(750, -250, Shape::L),
                ],
                FLAT_VOLUME,
            ),
            Scene::new(&[P(0, 0, Shape::E), P(1000, -1000, Shape::E)], FLAT_VOLUME),
            Scene::new(
                &[
                    P(0, 0, Shape::S),
                    P(750, 0, Shape::L),
                    P(1000, -500, Shape::L),
                ],
                FLAT_VOLUME,
            ),
            Scene::new(
                &[P(0, 0, Shape::L), P(400, -800, Shape::S)],
                &[P(0, 1000, Shape::L), P(400, 0, Shape::S)],
            ),
            Scene::new(
                FLAT_TIME,
                &[
                    P(0, 1000, Shape::S),
                    P(125, 0, Shape::S),
                    P(250, 1000, Shape::S),
                    P(375, 0, Shape::S),
                    P(500, 1000, Shape::S),
                    P(625, 0, Shape::S),
                    P(750, 1000, Shape::S),
                    P(875, 0, Shape::S),
                ],
            ),
            Scene::new(FLAT_TIME, &[P(0, 1000, Shape::S), P(750, 0, Shape::S)]),
            Scene::new(&[P(0, 0, Shape::L), P(1000, -1000, Shape::L)], FLAT_VOLUME),
        ],
    ),
];
