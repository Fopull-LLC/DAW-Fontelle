//! The piano roll's scales: which pitch classes a key allows.
//!
//! > *"a scale tool so you can chose between any note and the mode or
//! > whatever ... should have a good extensive list of scales so it isnt
//! > missing anything that would be limiting to people."*
//!
//! One entry per **pitch set**. A scale known by several names — Phrygian
//! dominant is also Spanish, Freygish, Hijaz and Ahava Rabbah — is one row
//! with the other names as aliases, which the chooser's search reads. Two rows
//! with the same notes would be a list that looks longer than it is and a
//! question about which to pick that has no answer.
//!
//! Twelve-tone equal temperament only: a maqam's quarter tones, or a raga's
//! shrutis, have no row in a piano roll to land on. Where a tradition's scale
//! is commonly played in 12-TET (Hijaz, the ten thaats, the Japanese
//! pentatonics) its nearest reading is here under its own name.
//!
//! This is **not** Tune's `TuneScale`: that one's order is stored in projects
//! as a choice's index, so it cannot grow in the middle. A key here is saved
//! by its [`Scale::id`], so this list can be reordered and added to freely.

/// A group of related scales — a heading in the chooser.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScaleFamily {
    /// The major scale and its six modes — the church modes.
    Major,
    MelodicMinor,
    HarmonicMinor,
    HarmonicMajor,
    DoubleHarmonic,
    Pentatonic,
    /// The blues scales and the other six-note scales that are not symmetric.
    Hexatonic,
    Bebop,
    /// Scales that repeat inside the octave: whole tone, the diminished
    /// scales, Messiaen's modes.
    Symmetric,
    /// Seven- and eight-note scales named for a place or a tradition.
    World,
}

impl ScaleFamily {
    /// In the order the chooser lists them, which is the order of [`SCALES`].
    pub const ALL: [Self; 10] = [
        Self::Major,
        Self::MelodicMinor,
        Self::HarmonicMinor,
        Self::HarmonicMajor,
        Self::DoubleHarmonic,
        Self::Pentatonic,
        Self::Hexatonic,
        Self::Bebop,
        Self::Symmetric,
        Self::World,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Major => "Major & its modes",
            Self::MelodicMinor => "Melodic minor modes",
            Self::HarmonicMinor => "Harmonic minor modes",
            Self::HarmonicMajor => "Harmonic major modes",
            Self::DoubleHarmonic => "Double harmonic modes",
            Self::Pentatonic => "Pentatonic",
            Self::Hexatonic => "Blues & six-note",
            Self::Bebop => "Bebop",
            Self::Symmetric => "Symmetric",
            Self::World => "World",
        }
    }
}

/// One scale: its intervals above the root.
#[derive(Debug, PartialEq, Eq)]
pub struct Scale {
    /// What a project saves. **Permanent** — renaming a scale changes
    /// `name`, never this.
    pub id: &'static str,
    pub name: &'static str,
    /// Its other names, for search.
    pub aka: &'static [&'static str],
    pub family: ScaleFamily,
    /// Semitones above the root, ascending, starting at 0.
    pub steps: &'static [u8],
}

impl Scale {
    /// The twelve pitch classes this scale holds at `root`, one bit each —
    /// bit 0 is C, bit 11 is B (the same reading `TuneScale::mask` uses).
    pub fn mask(&self, root: u8) -> u16 {
        let root = u16::from(root % 12);
        self.steps
            .iter()
            .fold(0, |mask, step| mask | 1 << ((u16::from(*step) + root) % 12))
    }
}

macro_rules! scales {
    ($($family:ident { $($id:literal $name:literal [$($aka:literal),*] [$($s:literal),+])* })*) => {
        /// Every scale, grouped by family in [`ScaleFamily::ALL`]'s order.
        pub static SCALES: &[Scale] = &[
            $($(Scale {
                id: $id,
                name: $name,
                aka: &[$($aka),*],
                family: ScaleFamily::$family,
                steps: &[$($s),+],
            },)*)*
        ];
    };
}

scales! {
    Major {
        "major" "Major" ["Ionian", "Bilaval"] [0, 2, 4, 5, 7, 9, 11]
        "dorian" "Dorian" ["Kafi", "Russian minor"] [0, 2, 3, 5, 7, 9, 10]
        "phrygian" "Phrygian" ["Bhairavi", "Kurd"] [0, 1, 3, 5, 7, 8, 10]
        "lydian" "Lydian" ["Kalyan", "Yaman"] [0, 2, 4, 6, 7, 9, 11]
        "mixolydian" "Mixolydian" ["Dominant", "Khamaj"] [0, 2, 4, 5, 7, 9, 10]
        "natural-minor" "Natural minor" ["Aeolian", "Minor", "Asavari"] [0, 2, 3, 5, 7, 8, 10]
        "locrian" "Locrian" [] [0, 1, 3, 5, 6, 8, 10]
    }
    MelodicMinor {
        "melodic-minor" "Melodic minor" ["Jazz minor", "Ascending melodic minor"] [0, 2, 3, 5, 7, 9, 11]
        "dorian-b2" "Dorian b2" ["Phrygian #6", "Javanese"] [0, 1, 3, 5, 7, 9, 10]
        "lydian-augmented" "Lydian augmented" ["Lydian #5"] [0, 2, 4, 6, 8, 9, 11]
        "lydian-dominant" "Lydian dominant" ["Overtone", "Acoustic", "Lydian b7", "Mixolydian #4"] [0, 2, 4, 6, 7, 9, 10]
        "mixolydian-b6" "Mixolydian b6" ["Aeolian dominant", "Hindu", "Melodic major"] [0, 2, 4, 5, 7, 8, 10]
        "locrian-sharp2" "Locrian #2" ["Half-diminished", "Aeolian b5"] [0, 2, 3, 5, 6, 8, 10]
        "altered" "Altered" ["Super locrian", "Diminished whole tone"] [0, 1, 3, 4, 6, 8, 10]
    }
    HarmonicMinor {
        "harmonic-minor" "Harmonic minor" ["Nahawand", "Aeolian #7"] [0, 2, 3, 5, 7, 8, 11]
        "locrian-sharp6" "Locrian #6" [] [0, 1, 3, 5, 6, 9, 10]
        "ionian-sharp5" "Ionian #5" ["Augmented major"] [0, 2, 4, 5, 8, 9, 11]
        "dorian-sharp4" "Dorian #4" ["Ukrainian dorian", "Romanian minor", "Mi Sheberach", "Nikriz"] [0, 2, 3, 6, 7, 9, 10]
        "phrygian-dominant" "Phrygian dominant" ["Spanish", "Spanish gypsy", "Freygish", "Hijaz", "Ahava Rabbah", "Phrygian major"] [0, 1, 4, 5, 7, 8, 10]
        "lydian-sharp2" "Lydian #2" [] [0, 3, 4, 6, 7, 9, 11]
        "ultralocrian" "Ultralocrian" ["Super locrian bb7", "Altered diminished"] [0, 1, 3, 4, 6, 8, 9]
    }
    HarmonicMajor {
        "harmonic-major" "Harmonic major" ["Ionian b6"] [0, 2, 4, 5, 7, 8, 11]
        "dorian-b5" "Dorian b5" [] [0, 2, 3, 5, 6, 9, 10]
        "phrygian-b4" "Phrygian b4" [] [0, 1, 3, 4, 7, 8, 10]
        "lydian-b3" "Lydian b3" ["Lydian diminished", "Melodic minor #4"] [0, 2, 3, 6, 7, 9, 11]
        "mixolydian-b2" "Mixolydian b2" [] [0, 1, 4, 5, 7, 9, 10]
        "lydian-augmented-sharp2" "Lydian augmented #2" [] [0, 3, 4, 6, 8, 9, 11]
        "locrian-bb7" "Locrian bb7" [] [0, 1, 3, 5, 6, 8, 9]
    }
    DoubleHarmonic {
        "double-harmonic-major" "Double harmonic major" ["Byzantine", "Arabic", "Gypsy major", "Bhairav", "Hijaz Kar"] [0, 1, 4, 5, 7, 8, 11]
        "lydian-sharp2-sharp6" "Lydian #2 #6" [] [0, 3, 4, 6, 7, 10, 11]
        "ultraphrygian" "Ultraphrygian" [] [0, 1, 3, 4, 7, 8, 9]
        "hungarian-minor" "Hungarian minor" ["Gypsy minor", "Double harmonic minor"] [0, 2, 3, 6, 7, 8, 11]
        "oriental" "Oriental" [] [0, 1, 4, 5, 6, 9, 10]
        "ionian-sharp2-sharp5" "Ionian #2 #5" [] [0, 3, 4, 5, 8, 9, 11]
        "locrian-bb3-bb7" "Locrian bb3 bb7" [] [0, 1, 2, 5, 6, 8, 9]
    }
    Pentatonic {
        "major-pentatonic" "Major pentatonic" ["Gong", "Mongolian"] [0, 2, 4, 7, 9]
        "egyptian" "Egyptian" ["Suspended pentatonic", "Shang"] [0, 2, 5, 7, 10]
        "man-jue" "Man jue" ["Blues minor pentatonic", "Jue"] [0, 3, 5, 8, 10]
        "ritsusen" "Ritsusen" ["Yo", "Blues major pentatonic", "Man gong", "Zhi"] [0, 2, 5, 7, 9]
        "minor-pentatonic" "Minor pentatonic" ["Yu"] [0, 3, 5, 7, 10]
        "dominant-pentatonic" "Dominant pentatonic" [] [0, 2, 4, 7, 10]
        "minor-6-pentatonic" "Minor 6 pentatonic" ["Dorian pentatonic"] [0, 3, 5, 7, 9]
        "hirajoshi" "Hirajoshi" ["Japanese"] [0, 2, 3, 7, 8]
        "in-sen" "In-sen" ["Japanese", "Kokin-joshi"] [0, 1, 5, 7, 10]
        "in" "In" ["Japanese", "Sakura", "Miyako-bushi"] [0, 1, 5, 7, 8]
        "iwato" "Iwato" ["Japanese"] [0, 1, 5, 6, 10]
        "kumoi" "Kumoi" ["Japanese", "Akebono"] [0, 2, 3, 7, 9]
        "ryukyu" "Ryukyu" ["Okinawan", "Japanese"] [0, 4, 5, 7, 11]
        "pelog" "Pelog" ["Balinese", "Indonesian"] [0, 1, 3, 7, 8]
        "chinese" "Chinese" ["Lydian pentatonic"] [0, 4, 6, 7, 11]
    }
    Hexatonic {
        "blues" "Blues" ["Minor blues", "Blues hexatonic"] [0, 3, 5, 6, 7, 10]
        "major-blues" "Major blues" [] [0, 2, 3, 4, 7, 9]
        "major-hexatonic" "Major hexatonic" [] [0, 2, 4, 5, 7, 9]
        "minor-hexatonic" "Minor hexatonic" [] [0, 2, 3, 5, 7, 10]
        "prometheus" "Prometheus" ["Mystic chord"] [0, 2, 4, 6, 9, 10]
        "istrian" "Istrian" [] [0, 1, 3, 4, 6, 7]
    }
    Bebop {
        "bebop-dominant" "Bebop dominant" ["Bebop mixolydian"] [0, 2, 4, 5, 7, 9, 10, 11]
        "bebop-major" "Bebop major" [] [0, 2, 4, 5, 7, 8, 9, 11]
        "bebop-dorian" "Bebop dorian" ["Bebop minor"] [0, 2, 3, 4, 5, 7, 9, 10]
        "bebop-melodic-minor" "Bebop melodic minor" [] [0, 2, 3, 5, 7, 8, 9, 11]
        "bebop-natural-minor" "Bebop natural minor" ["Bebop harmonic minor"] [0, 2, 3, 5, 7, 8, 10, 11]
    }
    Symmetric {
        "chromatic" "Chromatic" ["All twelve"] [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]
        "whole-tone" "Whole tone" ["Messiaen mode 1"] [0, 2, 4, 6, 8, 10]
        "diminished-whole-half" "Diminished (whole-half)" ["Octatonic", "Messiaen mode 2"] [0, 2, 3, 5, 6, 8, 9, 11]
        "diminished-half-whole" "Diminished (half-whole)" ["Dominant diminished", "Octatonic"] [0, 1, 3, 4, 6, 7, 9, 10]
        "augmented" "Augmented" ["Augmented hexatonic"] [0, 3, 4, 7, 8, 11]
        "tritone" "Tritone" ["Petrushka"] [0, 1, 4, 6, 7, 10]
        "messiaen-3" "Messiaen mode 3" [] [0, 2, 3, 4, 6, 7, 8, 10, 11]
        "messiaen-4" "Messiaen mode 4" [] [0, 1, 2, 5, 6, 7, 8, 11]
        "messiaen-5" "Messiaen mode 5" [] [0, 1, 5, 6, 7, 11]
        "messiaen-6" "Messiaen mode 6" [] [0, 2, 4, 5, 6, 8, 10, 11]
        "messiaen-7" "Messiaen mode 7" [] [0, 1, 2, 3, 5, 6, 7, 8, 9, 11]
    }
    World {
        "neapolitan-major" "Neapolitan major" [] [0, 1, 3, 5, 7, 9, 11]
        "neapolitan-minor" "Neapolitan minor" [] [0, 1, 3, 5, 7, 8, 11]
        "hungarian-major" "Hungarian major" [] [0, 3, 4, 6, 7, 9, 10]
        "hungarian-gypsy" "Hungarian gypsy" [] [0, 2, 3, 6, 7, 8, 10]
        "enigmatic" "Enigmatic" [] [0, 1, 4, 6, 8, 10, 11]
        "persian" "Persian" [] [0, 1, 4, 5, 6, 8, 11]
        "major-locrian" "Major locrian" ["Arabian"] [0, 2, 4, 5, 6, 8, 10]
        "lydian-minor" "Lydian minor" [] [0, 2, 4, 6, 7, 8, 10]
        "leading-whole-tone" "Leading whole tone" [] [0, 2, 4, 6, 8, 10, 11]
        "todi" "Todi" ["Raga", "Thaat"] [0, 1, 3, 6, 7, 8, 11]
        "marwa" "Marwa" ["Raga", "Thaat"] [0, 1, 4, 6, 7, 9, 11]
        "purvi" "Purvi" ["Raga", "Thaat"] [0, 1, 4, 6, 7, 8, 11]
        "spanish-8-tone" "Spanish 8-tone" ["Flamenco", "Spanish phrygian"] [0, 1, 3, 4, 5, 6, 8, 10]
        "algerian" "Algerian" [] [0, 2, 3, 5, 6, 7, 8, 11]
    }
}

/// The scale saved as `id`, if this build knows it.
pub fn scale(id: &str) -> Option<&'static Scale> {
    SCALES.iter().find(|s| s.id == id)
}

/// Whether `query` finds `scale`: every word of it, in any order, somewhere
/// in its name, an alias or its family's heading — ignoring case. An empty
/// query finds everything.
pub fn scale_matches(scale: &Scale, query: &str) -> bool {
    let haystack = std::iter::once(scale.name)
        .chain(scale.aka.iter().copied())
        .chain(std::iter::once(scale.family.label()))
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    query
        .split_whitespace()
        .all(|word| haystack.contains(&word.to_lowercase()))
}

/// The twelve pitch classes, as the chooser names them — the same list Tune
/// names its keys from.
pub use crate::effect::TUNE_ROOTS as PITCH_NAMES;

/// A song's key: a root and a scale, as a project saves it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct KeyScale {
    /// The pitch class of the root, 0 = C.
    pub root: u8,
    /// A [`Scale::id`].
    pub scale: String,
}

impl KeyScale {
    pub fn new(root: u8, scale: &str) -> Self {
        Self {
            root: root % 12,
            scale: scale.to_string(),
        }
    }

    /// The pitch classes in it, or `None` for a scale this build does not
    /// know (a project from a newer one): no scale rather than a wrong one.
    pub fn mask(&self) -> Option<u16> {
        scale(&self.scale).map(|s| s.mask(self.root))
    }

    /// Whether `key` (a MIDI note) is in it. A key it cannot read allows
    /// every note.
    pub fn contains(&self, key: u8) -> bool {
        self.mask().is_none_or(|mask| mask & (1 << (key % 12)) != 0)
    }

    /// `"C major"`, `"F# phrygian dominant"`: the root and the scale's name
    /// in lower case, the way a key is said.
    pub fn label(&self) -> String {
        let root = PITCH_NAMES[usize::from(self.root % 12)];
        match scale(&self.scale) {
            Some(s) => format!("{root} {}", s.name.to_lowercase()),
            None => format!("{root} {}", self.scale),
        }
    }
}

/// The nearest MIDI key to `key` whose pitch class is in `mask`: as little
/// movement as possible, and **down** on a tie — the same way every time, so
/// a phrase fitted twice does not wander. Never off the keyboard. An empty
/// mask leaves the key where it is.
pub fn fit_to_scale(key: u8, mask: u16) -> u8 {
    if mask & 0x0fff == 0 {
        return key;
    }
    let inside = |k: i32| (0..=127).contains(&k) && mask & (1 << (k % 12)) != 0;
    let key = i32::from(key.min(127));
    for distance in 0..=127 {
        if inside(key - distance) {
            return (key - distance) as u8;
        }
        if inside(key + distance) {
            return (key + distance) as u8;
        }
    }
    key as u8
}

// ------------------------------------------------------- a key as text ---
//
// What Copy scale puts on the clipboard and Paste scale reads back, between
// the piano roll's chooser and the pitch corrector — and anywhere else, which
// is why it is plain words: "A minor", "C# major", "D dorian".

/// `key` as plain words: the root and the scale's name in lower case, the
/// natural minor said the way everybody says it — "A minor".
pub fn scale_text(key: &KeyScale) -> String {
    if key.scale == "natural-minor" {
        return format!("{} minor", PITCH_NAMES[usize::from(key.root % 12)]);
    }
    key.label()
}

/// The notes of `key`, from its root up, by the names the chooser uses.
/// Empty for a scale this build does not know.
pub fn scale_notes(key: &KeyScale) -> Vec<&'static str> {
    scale(&key.scale).map_or_else(Vec::new, |s| {
        s.steps
            .iter()
            .map(|step| PITCH_NAMES[usize::from((key.root % 12 + step) % 12)])
            .collect()
    })
}

/// A note name at the front of `text` — a letter, then any sharps or flats —
/// and what is left after it.
fn leading_note(text: &str) -> Option<(u8, &str)> {
    let letter = text.chars().next()?;
    let natural: i16 = match letter.to_ascii_uppercase() {
        'C' => 0,
        'D' => 2,
        'E' => 4,
        'F' => 5,
        'G' => 7,
        'A' => 9,
        'B' => 11,
        _ => return None,
    };
    let mut shift = 0i16;
    let mut rest = &text[letter.len_utf8()..];
    while let Some(sign) = rest.chars().next() {
        let after = &rest[sign.len_utf8()..];
        match sign {
            '#' | '\u{266f}' => shift += 1,
            '\u{266d}' => shift -= 1,
            // A "b" after the letter is a flat only when it is not the start
            // of a word: "Bb major" and "Bbm", but "B blues" is B.
            'b' if !after
                .chars()
                .next()
                .is_some_and(|c| c.is_alphabetic() && c != 'm') =>
            {
                shift -= 1
            }
            _ => break,
        }
        rest = after;
    }
    Some(((natural + shift).rem_euclid(12) as u8, rest))
}

/// The words people shorten, spelled out the way the catalogue spells them.
fn spelled_out(word: &str) -> &str {
    match word {
        "min" | "mi" => "minor",
        "maj" | "ma" => "major",
        "nat" => "natural",
        "harm" | "harmon" => "harmonic",
        "mel" | "melod" => "melodic",
        "pent" | "penta" => "pentatonic",
        "dim" => "diminished",
        "dom" => "dominant",
        "aug" => "augmented",
        other => other,
    }
}

/// A key typed or pasted, read leniently: "A minor", "A min", "Am", "a
/// natural minor", "C#maj", "Db major", "D dorian", "E spanish gypsy" — or
/// the notes of one, "A B C D E F G", the first being the root. A bare root
/// is its major. `None` for anything that is not a key this build knows.
pub fn parse_scale(text: &str) -> Option<KeyScale> {
    let text = text.trim();
    if let Some(spelled) = parse_note_list(text) {
        return spelled;
    }
    let (root, rest) = leading_note(text)?;
    let rest = rest.trim().trim_start_matches(['-', '_', ' ']).trim();
    // Case matters only here: "AM" is A major and "Am" A minor, the way a
    // chord symbol is written.
    let id = match rest {
        "" | "M" => "major",
        "m" | "-" => "natural-minor",
        _ => {
            let words: Vec<String> = rest
                .to_lowercase()
                .split(|c: char| c.is_whitespace() || c == '-' || c == '_' || c == ',')
                .filter(|w| !w.is_empty())
                .map(|w| spelled_out(w).to_string())
                .collect();
            let wanted = words.join(" ");
            find_scale(&wanted)?
        }
    };
    Some(KeyScale::new(root, id))
}

/// The catalogue's scale called `wanted` (lower case, single spaces): by
/// its id, its name or an alias, exactly; failing that, the one with the
/// shortest name every word of `wanted` is in.
fn find_scale(wanted: &str) -> Option<&'static str> {
    if wanted.is_empty() {
        return None;
    }
    let same = |name: &str| name.to_lowercase().replace('-', " ") == wanted;
    if let Some(s) = SCALES
        .iter()
        .find(|s| same(s.id) || same(s.name) || s.aka.iter().any(|a| same(a)))
    {
        return Some(s.id);
    }
    SCALES
        .iter()
        .filter(|s| {
            let name = s.name.to_lowercase();
            wanted
                .split(' ')
                .all(|word| name.split(' ').any(|own| own.starts_with(word)))
        })
        .min_by_key(|s| s.name.len())
        .map(|s| s.id)
}

/// "A B C D E F G": two notes or more and nothing else, read as the scale
/// whose notes they are, rooted on the first. `None` for text that is not
/// a list of notes; `Some(None)` for notes that are no scale in the
/// catalogue.
fn parse_note_list(text: &str) -> Option<Option<KeyScale>> {
    let tokens: Vec<&str> = text
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter(|t| !t.is_empty())
        .collect();
    if tokens.len() < 2 {
        return None;
    }
    let mut mask = 0u16;
    let mut root = None;
    for token in tokens {
        let (class, rest) = leading_note(token)?;
        if !rest.is_empty() {
            return None;
        }
        root.get_or_insert(class);
        mask |= 1 << class;
    }
    let root = root?;
    Some(
        SCALES
            .iter()
            .find(|s| s.mask(root) == mask)
            .map(|s| KeyScale::new(root, s.id)),
    )
}
