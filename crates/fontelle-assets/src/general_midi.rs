//! The 128 General MIDI program names.
//!
//! Here because an imported part called *"Channel 4"* tells you nothing and a
//! part called *"Fretless Bass"* tells you what it is. A MIDI file names its
//! parts in at most three places — the track's name, the program it selects,
//! and whether it is the percussion channel — and this is the second of them.
//!
//! The names are General MIDI Level 1's own, in its own order and spelling, so
//! that a part imported here is called what every other program calls it.

/// GM Level 1's name for a program number.
///
/// Every value of a `u8` has an answer: the standard defines 0..=127, and
/// anything above that cannot come out of a MIDI file's program change (the
/// wire carries seven bits) but can come out of arithmetic, so it is named
/// rather than left to panic.
pub fn general_midi_name(program: u8) -> &'static str {
    GENERAL_MIDI
        .get(program as usize)
        .copied()
        .unwrap_or("Program")
}

/// The instrument family a program belongs to — GM groups its programs in
/// eights, and the family is what a browser would sort by.
pub fn general_midi_family(program: u8) -> &'static str {
    const FAMILIES: [&str; 16] = [
        "Piano",
        "Chromatic Percussion",
        "Organ",
        "Guitar",
        "Bass",
        "Strings",
        "Ensemble",
        "Brass",
        "Reed",
        "Pipe",
        "Synth Lead",
        "Synth Pad",
        "Synth Effects",
        "Ethnic",
        "Percussive",
        "Sound Effects",
    ];
    FAMILIES[(program as usize / 8).min(15)]
}

const GENERAL_MIDI: [&str; 128] = [
    "Acoustic Grand Piano",
    "Bright Acoustic Piano",
    "Electric Grand Piano",
    "Honky-tonk Piano",
    "Electric Piano 1",
    "Electric Piano 2",
    "Harpsichord",
    "Clavi",
    "Celesta",
    "Glockenspiel",
    "Music Box",
    "Vibraphone",
    "Marimba",
    "Xylophone",
    "Tubular Bells",
    "Dulcimer",
    "Drawbar Organ",
    "Percussive Organ",
    "Rock Organ",
    "Church Organ",
    "Reed Organ",
    "Accordion",
    "Harmonica",
    "Tango Accordion",
    "Acoustic Guitar (nylon)",
    "Acoustic Guitar (steel)",
    "Electric Guitar (jazz)",
    "Electric Guitar (clean)",
    "Electric Guitar (muted)",
    "Overdriven Guitar",
    "Distortion Guitar",
    "Guitar harmonics",
    "Acoustic Bass",
    "Electric Bass (finger)",
    "Electric Bass (pick)",
    "Fretless Bass",
    "Slap Bass 1",
    "Slap Bass 2",
    "Synth Bass 1",
    "Synth Bass 2",
    "Violin",
    "Viola",
    "Cello",
    "Contrabass",
    "Tremolo Strings",
    "Pizzicato Strings",
    "Orchestral Harp",
    "Timpani",
    "String Ensemble 1",
    "String Ensemble 2",
    "SynthStrings 1",
    "SynthStrings 2",
    "Choir Aahs",
    "Voice Oohs",
    "Synth Voice",
    "Orchestra Hit",
    "Trumpet",
    "Trombone",
    "Tuba",
    "Muted Trumpet",
    "French Horn",
    "Brass Section",
    "SynthBrass 1",
    "SynthBrass 2",
    "Soprano Sax",
    "Alto Sax",
    "Tenor Sax",
    "Baritone Sax",
    "Oboe",
    "English Horn",
    "Bassoon",
    "Clarinet",
    "Piccolo",
    "Flute",
    "Recorder",
    "Pan Flute",
    "Blown Bottle",
    "Shakuhachi",
    "Whistle",
    "Ocarina",
    "Lead 1 (square)",
    "Lead 2 (sawtooth)",
    "Lead 3 (calliope)",
    "Lead 4 (chiff)",
    "Lead 5 (charang)",
    "Lead 6 (voice)",
    "Lead 7 (fifths)",
    "Lead 8 (bass + lead)",
    "Pad 1 (new age)",
    "Pad 2 (warm)",
    "Pad 3 (polysynth)",
    "Pad 4 (choir)",
    "Pad 5 (bowed)",
    "Pad 6 (metallic)",
    "Pad 7 (halo)",
    "Pad 8 (sweep)",
    "FX 1 (rain)",
    "FX 2 (soundtrack)",
    "FX 3 (crystal)",
    "FX 4 (atmosphere)",
    "FX 5 (brightness)",
    "FX 6 (goblins)",
    "FX 7 (echoes)",
    "FX 8 (sci-fi)",
    "Sitar",
    "Banjo",
    "Shamisen",
    "Koto",
    "Kalimba",
    "Bag pipe",
    "Fiddle",
    "Shanai",
    "Tinkle Bell",
    "Agogo",
    "Steel Drums",
    "Woodblock",
    "Taiko Drum",
    "Melodic Tom",
    "Synth Drum",
    "Reverse Cymbal",
    "Guitar Fret Noise",
    "Breath Noise",
    "Seashore",
    "Bird Tweet",
    "Telephone Ring",
    "Helicopter",
    "Applause",
    "Gunshot",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_is_the_size_general_midi_says_it_is() {
        assert_eq!(GENERAL_MIDI.len(), 128);
    }

    #[test]
    fn a_program_out_of_range_is_named_rather_than_a_panic() {
        assert_eq!(general_midi_name(200), "Program");
        assert_eq!(general_midi_family(200), "Sound Effects");
    }

    #[test]
    fn the_families_line_up_with_the_programs_in_them() {
        assert_eq!(general_midi_family(0), "Piano");
        assert_eq!(general_midi_family(33), "Bass");
        assert_eq!(general_midi_family(127), "Sound Effects");
    }
}
