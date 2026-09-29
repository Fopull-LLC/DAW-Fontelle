//! The piano roll's scales.
//!
//! > *"i also want the next update to include in the piano roll a scale tool
//! > so you can chose between any note and the mode or whatever and it will
//! > snap all of your notes to that scale and dim out all the lanes that arent
//! > in that scale. should have a good extensive list of scales so it isnt
//! > missing anything that would be limiting to people."*
//!
//! "Extensive" is only worth anything if every entry is right, so most of
//! what is here checks the intervals: each family of modes against the
//! rotations of its parent, every pitch set once (a second name is an alias,
//! found by search, not a second row), and the names people will type.

use fontelle_types::{KeyScale, SCALES, ScaleFamily, fit_to_scale, scale, scale_matches};

fn steps(id: &str) -> Vec<u8> {
    scale(id)
        .unwrap_or_else(|| panic!("no scale {id:?}"))
        .steps
        .to_vec()
}

/// The `n`th mode of `parent`: the same notes, starting on its `n`th degree.
fn rotation(parent: &[u8], n: usize) -> Vec<u8> {
    let start = parent[n];
    let mut out: Vec<u8> = parent.iter().map(|s| (s + 12 - start) % 12).collect();
    out.sort_unstable();
    out
}

#[test]
fn every_scale_is_a_well_formed_set_of_pitch_classes() {
    for s in SCALES {
        assert_eq!(s.steps.first(), Some(&0), "{} starts on its root", s.name);
        assert!(
            s.steps.windows(2).all(|w| w[0] < w[1]) && *s.steps.last().unwrap() < 12,
            "{}: {:?}",
            s.name,
            s.steps
        );
        assert!(s.steps.len() >= 5, "{} has at least five notes", s.name);
        assert!(!s.id.is_empty() && !s.name.is_empty());
        assert!(
            s.id.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
            "{:?} is a stable file-safe id",
            s.id
        );
    }
}

#[test]
fn ids_and_names_and_pitch_sets_are_each_unique() {
    for (i, a) in SCALES.iter().enumerate() {
        for b in &SCALES[i + 1..] {
            assert_ne!(a.id, b.id);
            assert_ne!(a.name, b.name);
            assert_ne!(
                a.steps, b.steps,
                "{} and {} are one scale: make one an alias of the other",
                a.name, b.name
            );
        }
    }
}

#[test]
fn the_list_is_extensive() {
    assert!(SCALES.len() >= 80, "{}", SCALES.len());
    // Every family has something in it, and the list is in family order so
    // the chooser's headings each come once.
    for family in ScaleFamily::ALL {
        assert!(SCALES.iter().any(|s| s.family == family), "{family:?}");
    }
    let order: Vec<usize> = SCALES
        .iter()
        .map(|s| {
            ScaleFamily::ALL
                .iter()
                .position(|f| *f == s.family)
                .unwrap()
        })
        .collect();
    assert!(order.windows(2).all(|w| w[0] <= w[1]), "{order:?}");
}

#[test]
fn the_seven_modes_of_the_major_scale_are_its_rotations() {
    let major = steps("major");
    assert_eq!(major, [0, 2, 4, 5, 7, 9, 11]);
    for (n, id) in [
        "major",
        "dorian",
        "phrygian",
        "lydian",
        "mixolydian",
        "natural-minor",
        "locrian",
    ]
    .iter()
    .enumerate()
    {
        assert_eq!(steps(id), rotation(&major, n), "{id}");
    }
}

#[test]
fn the_modes_of_melodic_minor_are_its_rotations() {
    let parent = steps("melodic-minor");
    assert_eq!(parent, [0, 2, 3, 5, 7, 9, 11]);
    for (n, id) in [
        "melodic-minor",
        "dorian-b2",
        "lydian-augmented",
        "lydian-dominant",
        "mixolydian-b6",
        "locrian-sharp2",
        "altered",
    ]
    .iter()
    .enumerate()
    {
        assert_eq!(steps(id), rotation(&parent, n), "{id}");
    }
}

#[test]
fn the_modes_of_harmonic_minor_are_its_rotations() {
    let parent = steps("harmonic-minor");
    assert_eq!(parent, [0, 2, 3, 5, 7, 8, 11]);
    for (n, id) in [
        "harmonic-minor",
        "locrian-sharp6",
        "ionian-sharp5",
        "dorian-sharp4",
        "phrygian-dominant",
        "lydian-sharp2",
        "ultralocrian",
    ]
    .iter()
    .enumerate()
    {
        assert_eq!(steps(id), rotation(&parent, n), "{id}");
    }
}

#[test]
fn the_modes_of_harmonic_major_are_its_rotations() {
    let parent = steps("harmonic-major");
    assert_eq!(parent, [0, 2, 4, 5, 7, 8, 11]);
    for (n, id) in [
        "harmonic-major",
        "dorian-b5",
        "phrygian-b4",
        "lydian-b3",
        "mixolydian-b2",
        "lydian-augmented-sharp2",
        "locrian-bb7",
    ]
    .iter()
    .enumerate()
    {
        assert_eq!(steps(id), rotation(&parent, n), "{id}");
    }
}

#[test]
fn the_modes_of_double_harmonic_major_are_its_rotations() {
    let parent = steps("double-harmonic-major");
    assert_eq!(parent, [0, 1, 4, 5, 7, 8, 11]);
    for (n, id) in [
        "double-harmonic-major",
        "lydian-sharp2-sharp6",
        "ultraphrygian",
        "hungarian-minor",
        "oriental",
        "ionian-sharp2-sharp5",
        "locrian-bb3-bb7",
    ]
    .iter()
    .enumerate()
    {
        assert_eq!(steps(id), rotation(&parent, n), "{id}");
    }
}

#[test]
fn the_pentatonic_modes_are_rotations_of_the_major_pentatonic() {
    let parent = steps("major-pentatonic");
    assert_eq!(parent, [0, 2, 4, 7, 9]);
    for (n, id) in [
        "major-pentatonic",
        "egyptian",
        "man-jue",
        "ritsusen",
        "minor-pentatonic",
    ]
    .iter()
    .enumerate()
    {
        assert_eq!(steps(id), rotation(&parent, n), "{id}");
    }
}

#[test]
fn the_symmetric_scales_repeat_inside_the_octave() {
    // Transposed by their period, they are themselves.
    for (id, period) in [
        ("whole-tone", 2),
        ("diminished-whole-half", 3),
        ("diminished-half-whole", 3),
        ("augmented", 4),
        ("messiaen-3", 4),
        ("messiaen-4", 6),
        ("messiaen-5", 6),
        ("messiaen-6", 6),
        ("messiaen-7", 6),
        ("tritone", 6),
    ] {
        let s = scale(id).unwrap();
        assert_eq!(s.mask(0), s.mask(period), "{id}");
    }
    assert_eq!(steps("chromatic").len(), 12);
}

#[test]
fn the_rest_are_the_intervals_the_literature_gives() {
    for (id, want) in [
        ("blues", &[0, 3, 5, 6, 7, 10][..]),
        ("major-blues", &[0, 2, 3, 4, 7, 9]),
        ("bebop-dominant", &[0, 2, 4, 5, 7, 9, 10, 11]),
        ("bebop-major", &[0, 2, 4, 5, 7, 8, 9, 11]),
        ("bebop-dorian", &[0, 2, 3, 4, 5, 7, 9, 10]),
        ("hirajoshi", &[0, 2, 3, 7, 8]),
        ("in-sen", &[0, 1, 5, 7, 10]),
        ("iwato", &[0, 1, 5, 6, 10]),
        ("kumoi", &[0, 2, 3, 7, 9]),
        ("neapolitan-major", &[0, 1, 3, 5, 7, 9, 11]),
        ("neapolitan-minor", &[0, 1, 3, 5, 7, 8, 11]),
        ("hungarian-major", &[0, 3, 4, 6, 7, 9, 10]),
        ("enigmatic", &[0, 1, 4, 6, 8, 10, 11]),
        ("persian", &[0, 1, 4, 5, 6, 8, 11]),
        ("prometheus", &[0, 2, 4, 6, 9, 10]),
        ("spanish-8-tone", &[0, 1, 3, 4, 5, 6, 8, 10]),
    ] {
        assert_eq!(steps(id), want, "{id}");
    }
}

#[test]
fn a_scale_is_found_by_any_of_its_names() {
    let found = |query: &str| -> Vec<&str> {
        SCALES
            .iter()
            .filter(|s| scale_matches(s, query))
            .map(|s| s.id)
            .collect()
    };
    assert!(found("aeolian").contains(&"natural-minor"));
    assert!(found("Spanish").contains(&"phrygian-dominant"));
    assert!(found("hijaz").contains(&"phrygian-dominant"));
    assert!(found("super locrian").contains(&"altered"));
    assert!(found("byzantine").contains(&"double-harmonic-major"));
    assert!(found("gypsy").contains(&"hungarian-minor"));
    assert!(
        found("penta minor").contains(&"minor-pentatonic"),
        "every word, any order"
    );
    assert!(found("japanese").contains(&"hirajoshi"));
    // The family's name finds its members.
    assert!(found("bebop").len() >= 4);
    assert_eq!(found("").len(), SCALES.len());
    assert!(found("zzzz").is_empty());
}

#[test]
fn a_key_is_its_root_and_its_scale() {
    let c_major = KeyScale::new(0, "major");
    assert_eq!(c_major.label(), "C major");
    assert_eq!(c_major.mask(), Some(0b1010_1011_0101));
    let a_minor = KeyScale::new(9, "natural-minor");
    assert_eq!(
        a_minor.mask(),
        c_major.mask(),
        "the relative minor has the same notes"
    );
    assert_eq!(KeyScale::new(1, "blues").label(), "C# blues");
    assert!(KeyScale::new(1, "blues").contains(1 + 12 * 4));
    assert!(!KeyScale::new(1, "blues").contains(2));

    // A scale this build does not know — a newer project — is no scale at
    // all rather than a wrong one.
    assert_eq!(KeyScale::new(0, "not-a-scale").mask(), None);

    // It saves as its id, so reordering the list never changes a song.
    let json = serde_json::to_string(&a_minor).unwrap();
    assert!(json.contains("natural-minor"), "{json}");
    let back: KeyScale = serde_json::from_str(&json).unwrap();
    assert_eq!(back, a_minor);
}

#[test]
fn a_note_fits_the_scale_by_moving_as_little_as_it_can() {
    let c_major = KeyScale::new(0, "major").mask().unwrap();
    assert_eq!(fit_to_scale(60, c_major), 60, "C stays");
    assert_eq!(
        fit_to_scale(61, c_major),
        60,
        "C# is a tie between C and D: down"
    );
    assert_eq!(fit_to_scale(66, c_major), 65, "F# to F");
    // A gap wider than a tone goes to the nearer side.
    let c_minor_pent = KeyScale::new(0, "minor-pentatonic").mask().unwrap(); // C Eb F G Bb
    assert_eq!(fit_to_scale(61, c_minor_pent), 60);
    assert_eq!(fit_to_scale(62, c_minor_pent), 63, "D is nearer Eb than C");
    assert_eq!(fit_to_scale(69, c_minor_pent), 70, "A to Bb");
    // At the ends of the keyboard it goes the only way it can.
    let d_major = KeyScale::new(2, "major").mask().unwrap(); // no C
    assert_eq!(fit_to_scale(0, d_major), 1, "C to C#");
    let only_c = KeyScale::new(0, "major").mask().unwrap() & 1;
    assert_eq!(fit_to_scale(127, only_c), 120);
    // Nothing in the mask: nothing to fit to.
    assert_eq!(fit_to_scale(64, 0), 64);
}
