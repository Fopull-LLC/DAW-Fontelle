//! Key and scale, and how sure (plan §2.6).
//!
//! The pitch-class weight is correlated with the 24 rotations of two
//! published key profiles, Krumhansl–Kessler's (probe-tone ratings, 1982)
//! and Temperley's (Kostka–Payne corpus counts, 2007), averaged. The best
//! key's pitch set is then tried against its own modes from `SCALES`
//! (dorian, harmonic minor, mixolydian …): one wins only if it fits the
//! weight clearly better.
//!
//! The confidence is calibrated, not the raw correlation: a logistic over
//! the margin to the best key that is *not* the relative (a relative pair
//! shares its pitch set, so their rivalry is the separate tonic read-out),
//! the correlation itself, and how much material there was. The
//! coefficients are fitted by `examples/fit_key_calibration.rs` on note
//! material from `testsignals::key_material` — a synthetic floor, until a
//! labelled corpus of real songs is fitted (plan §2.6).

use fontelle_types::KeyScale;

/// One sounding pitch, as key detection weighs it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PitchWeight {
    pub midi: u8,
    /// How long it sounds.
    pub seconds: f32,
    /// How much it counts: amplitude times confidence, 0..1.
    pub weight: f32,
}

/// What [`detect_key`] read.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct KeyReading {
    /// The key: a root and a scale from `SCALES`.
    pub key: KeyScale,
    /// How likely the key's pitch set is right, 0..1 (calibrated, see
    /// [`key_confidence`]). The relative major or minor shares it.
    pub confidence: f32,
    /// Given the pitch set, how likely the tonic is this one rather than
    /// the relative's, 0..1.
    pub tonic_confidence: f32,
    /// The relative major or minor.
    pub relative: KeyScale,
    /// Other readings worth offering, best first (the relative, a mode).
    pub alternatives: Vec<KeyScale>,
    /// What the confidences were computed from.
    pub evidence: KeyEvidence,
}

/// The numbers behind a [`KeyReading`]'s confidences.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct KeyEvidence {
    /// The best key's profile correlation.
    pub best: f32,
    /// Its lead over the best key that is not its relative.
    pub margin: f32,
    /// Its lead over its relative.
    pub tonic_margin: f32,
    /// Seconds of weighted notes.
    pub mass: f32,
    /// The share of that inside the reading's scale.
    pub inside: f32,
}

/// Krumhansl–Kessler, C major and C minor.
const KK_MAJOR: [f32; 12] = [
    6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88,
];
const KK_MINOR: [f32; 12] = [
    6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17,
];
/// Temperley's Kostka–Payne profiles.
const TEMPERLEY_MAJOR: [f32; 12] = [
    0.748, 0.060, 0.488, 0.082, 0.670, 0.460, 0.096, 0.715, 0.104, 0.366, 0.057, 0.400,
];
const TEMPERLEY_MINOR: [f32; 12] = [
    0.712, 0.084, 0.474, 0.618, 0.049, 0.460, 0.105, 0.747, 0.404, 0.067, 0.133, 0.330,
];

/// Notes under this count half again.
const BASS_BELOW: u8 = 52;
const BASS_WEIGHT: f32 = 1.5;

/// The modes a reading may turn into, by the profile that found it.
const MAJOR_MODES: [&str; 3] = ["major", "mixolydian", "lydian"];
const MINOR_MODES: [&str; 4] = ["natural-minor", "dorian", "harmonic-minor", "phrygian"];
/// How much better (as a share of all the weight) a mode must fit than the
/// plain major or minor to be named instead.
const MODE_MARGIN: f32 = 0.04;
/// What weight outside a scale costs against weight inside it.
const OUTSIDE_COST: f32 = 2.0;

/// The fitted calibration:
/// `σ(A·margin + B·best + C·ln(1 + mass) + E·inside + D)`,
/// from `examples/fit_key_calibration.rs` (20 000 synthetic readings, half
/// of them with the right pitch set; predicted and observed agree within
/// about 0.07 in every decile).
const A: f32 = 2.321;
const B: f32 = 2.339;
const C: f32 = 1.236;
const D: f32 = -13.559;
const E: f32 = 9.948;
/// And the tonic's: `σ(T·(r_key − r_relative))`.
const T: f32 = 5.754;

/// Pitch-class weight: seconds times weight per pitch class, notes under E3
/// counted half again (a bass note says more about the key).
pub fn chroma_of(notes: &[PitchWeight]) -> [f32; 12] {
    let mut chroma = [0.0f32; 12];
    for n in notes {
        let bass = if n.midi < BASS_BELOW {
            BASS_WEIGHT
        } else {
            1.0
        };
        chroma[usize::from(n.midi % 12)] += n.seconds.max(0.0) * n.weight.max(0.0) * bass;
    }
    chroma
}

fn pearson(x: &[f32; 12], y: &[f32; 12], rotate: usize) -> f32 {
    let mx = x.iter().sum::<f32>() / 12.0;
    let my = y.iter().sum::<f32>() / 12.0;
    let (mut sxy, mut sxx, mut syy) = (0.0f32, 0.0f32, 0.0f32);
    for pc in 0..12 {
        let a = x[pc] - mx;
        let b = y[(pc + 12 - rotate) % 12] - my;
        sxy += a * b;
        sxx += a * a;
        syy += b * b;
    }
    if sxx <= 0.0 || syy <= 0.0 {
        0.0
    } else {
        sxy / (sxx * syy).sqrt()
    }
}

/// The 24 correlations: index `root` is major, `12 + root` minor.
fn correlations(chroma: &[f32; 12]) -> [f32; 24] {
    let mut r = [0.0f32; 24];
    for root in 0..12 {
        r[root] =
            0.5 * (pearson(chroma, &KK_MAJOR, root) + pearson(chroma, &TEMPERLEY_MAJOR, root));
        r[12 + root] =
            0.5 * (pearson(chroma, &KK_MINOR, root) + pearson(chroma, &TEMPERLEY_MINOR, root));
    }
    r
}

/// The relative of key index `k` (a major's sixth, a minor's third).
fn relative_of(k: usize) -> usize {
    if k < 12 {
        12 + (k + 9) % 12
    } else {
        (k - 12 + 3) % 12
    }
}

/// How well a scale at a root holds the weight: what is inside, less what
/// is outside at twice the cost.
fn fit(chroma: &[f32; 12], key: &KeyScale) -> f32 {
    let mask = key.mask().unwrap_or(0xfff);
    (0..12)
        .map(|pc| {
            if mask & (1 << pc) != 0 {
                chroma[pc]
            } else {
                -OUTSIDE_COST * chroma[pc]
            }
        })
        .sum()
}

/// The key of a set of pitches; `None` when there is nothing to read.
pub fn detect_key(notes: &[PitchWeight]) -> Option<KeyReading> {
    let chroma = chroma_of(notes);
    let mass: f32 = chroma.iter().sum();
    if mass <= 0.0 {
        return None;
    }
    let r = correlations(&chroma);
    let best = (0..24).max_by(|a, b| r[*a].total_cmp(&r[*b]))?;
    let relative = relative_of(best);
    let rival = (0..24)
        .filter(|&k| k != best && k != relative)
        .map(|k| r[k])
        .fold(f32::NEG_INFINITY, f32::max);
    let as_key = |k: usize| {
        if k < 12 {
            KeyScale::new(k as u8, "major")
        } else {
            KeyScale::new((k - 12) as u8, "natural-minor")
        }
    };

    // The modes of the winning tonic, major and minor families both (the
    // profiles can call A dorian "A major" for its F♯): the plain reading
    // unless another clearly fits better.
    let root = (best % 12) as u8;
    let plain = as_key(best);
    let plain_fit = fit(&chroma, &plain);
    let (mode, mode_fit) = MAJOR_MODES
        .iter()
        .chain(&MINOR_MODES)
        .map(|id| {
            let k = KeyScale::new(root, id);
            let f = fit(&chroma, &k);
            (k, f)
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .unwrap_or((plain.clone(), plain_fit));
    let key = if mode_fit > plain_fit + MODE_MARGIN * mass {
        mode
    } else {
        plain.clone()
    };
    let relative = relative_key(&key).unwrap_or_else(|| as_key(relative));

    let mut alternatives = vec![relative.clone()];
    if key != plain && plain != relative {
        alternatives.push(plain);
    }
    let relative_index = match relative.scale.as_str() {
        "major" => usize::from(relative.root),
        _ => 12 + usize::from(relative.root),
    };
    let evidence = KeyEvidence {
        best: r[best],
        margin: r[best] - rival,
        tonic_margin: r[best] - r[relative_index],
        mass,
        inside: inside_share(&chroma, &key),
    };
    Some(KeyReading {
        confidence: key_confidence(evidence.margin, evidence.best, mass, evidence.inside),
        tonic_confidence: tonic_confidence(evidence.tonic_margin),
        relative,
        key,
        alternatives,
        evidence,
    })
}

/// Given the pitch set, how likely the tonic is the reading's rather than
/// its relative's.
pub fn tonic_confidence(tonic_margin: f32) -> f32 {
    sigmoid(T * tonic_margin)
}

/// The other reading of a key's pitch set people name: a major's relative
/// minor, a minor's relative major, and for a church mode its parent major
/// (A dorian → G major). `None` for a scale with no such twin.
fn relative_key(key: &KeyScale) -> Option<KeyScale> {
    let (offset, scale) = match key.scale.as_str() {
        "major" => (9, "natural-minor"),
        "natural-minor" => (3, "major"),
        "dorian" => (10, "major"),
        "phrygian" => (8, "major"),
        "lydian" => (7, "major"),
        "mixolydian" => (5, "major"),
        "harmonic-minor" => (3, "major"),
        _ => return None,
    };
    Some(KeyScale::new((key.root + offset) % 12, scale))
}

fn sigmoid(x: f32) -> f32 {
    1.0 / (1.0 + (-x).exp())
}

/// How likely a reading's pitch set is right: a logistic over the best
/// key's correlation, its margin over the best key that is not its relative,
/// the amount of material (seconds of weighted notes) and the share of it
/// inside the scale.
pub fn key_confidence(margin: f32, best: f32, mass: f32, inside: f32) -> f32 {
    sigmoid(
        A * margin.max(0.0) + B * best + C * mass.max(0.0).ln_1p() + E * inside.clamp(0.0, 1.0) + D,
    )
}

/// The share of the weight inside a key's scale.
fn inside_share(chroma: &[f32; 12], key: &KeyScale) -> f32 {
    let mask = key.mask().unwrap_or(0xfff);
    let total: f32 = chroma.iter().sum();
    if total <= 0.0 {
        return 0.0;
    }
    (0..12)
        .filter(|pc| mask & (1 << pc) != 0)
        .map(|pc| chroma[pc])
        .sum::<f32>()
        / total
}
