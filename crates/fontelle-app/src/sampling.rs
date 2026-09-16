//! Turning a file — or a folder of them — into the recording a Flopsynth
//! oscillator plays (`fontelle_core::UserSample`).
//!
//! > *"we could actually sample a real piano sound and then do effects and
//! > modulating and layering with other oscilators and stuff etc."*
//!
//! Three questions a drop has to answer, each a pure function here so the
//! session's `load_sample` is only the plumbing:
//!
//! - **What note is this?** From the file's name when the name says
//!   ([`note_in_name`]) — `A4v8.wav`, `piano_C#3.flac`, `Db5-soft.wav` are
//!   how every sample library names its notes — and from the sound when it
//!   does not ([`detect_root`]), through the same YIN tracker the corrector
//!   uses.
//! - **Which keys does each recording serve?** ([`key_ranges`]) The nearest
//!   one: a folder of C3, C4, C5 splits the keyboard halfway between
//!   neighbours.
//! - **Is this a recording at all, or a wavetable?** ([`looks_like_wavetable`])
//!   A whole number of 2048-sample cycles is what a wavetable editor exports
//!   and what nothing recorded ever is.

use fontelle_dsp::{PitchTracker, RELAXED_TRACKING_THRESHOLD};

/// The MIDI key a note name in `name` spells, if there is one.
///
/// A note is a letter A–G, an optional `#` or `b`, and an octave −1..=9,
/// standing on its own: not preceded by a letter (so `Grand` is not a G and
/// `Bass` is not a B) and not followed by a digit (so `C12` is not C1). The
/// first one found wins — `A4v8` is A4, the velocity layer after it is not a
/// note.
///
/// Middle C is C4 = 60, the convention every sample library follows.
pub fn note_in_name(name: &str) -> Option<u8> {
    let chars: Vec<char> = name.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        let letter = match c.to_ascii_uppercase() {
            'C' => 0i32,
            'D' => 2,
            'E' => 4,
            'F' => 5,
            'G' => 7,
            'A' => 9,
            'B' => 11,
            _ => continue,
        };
        if i > 0 && chars[i - 1].is_ascii_alphabetic() {
            continue;
        }
        let mut at = i + 1;
        let mut semitone = letter;
        match chars.get(at) {
            Some('#') => {
                semitone += 1;
                at += 1;
            }
            // A flat is a lower-case b, and only where a digit follows: `Ab`
            // in a word is not A-flat.
            Some('b')
                if chars
                    .get(at + 1)
                    .is_some_and(|c| c.is_ascii_digit() || *c == '-') =>
            {
                semitone -= 1;
                at += 1;
            }
            _ => {}
        }
        let negative = chars.get(at) == Some(&'-');
        if negative {
            at += 1;
        }
        let Some(digit) = chars.get(at).and_then(|c| c.to_digit(10)) else {
            continue;
        };
        if chars.get(at + 1).is_some_and(|c| c.is_ascii_digit()) {
            continue;
        }
        let octave = if negative {
            -(digit as i32)
        } else {
            digit as i32
        };
        if !(-1..=9).contains(&octave) {
            continue;
        }
        let key = (octave + 1) * 12 + semitone;
        if (0..=127).contains(&key) {
            return Some(key as u8);
        }
    }
    None
}

/// The pitch of a recording, as the nearest MIDI key and how far off it the
/// recording sits, in cents. `None` when nothing periodic was found — a
/// drum, a texture, silence.
///
/// The median of the confident frames over the first second and a half of
/// sound: a piano note's pitch sags a little as it rings, a pluck's
/// overshoots at the start, and the median is the note through both.
pub fn detect_root(samples: &[f32], sample_rate: u32) -> Option<(u8, f32)> {
    if samples.is_empty() || sample_rate == 0 {
        return None;
    }
    // From the first sample that is not silence: a recording's leading air
    // is not the note.
    let start = samples.iter().position(|s| s.abs() > 0.01).unwrap_or(0);
    let end = samples.len().min(start + (sample_rate as usize * 3) / 2);
    let sound = &samples[start..end];
    let mut tracker = PitchTracker::new(27.5, 4_200.0, 256);
    tracker.set_threshold(RELAXED_TRACKING_THRESHOLD);
    tracker.prepare(sample_rate as f32);
    let mut cents = Vec::new();
    for block in sound.chunks(512) {
        tracker.push(block, &mut |frame| {
            if let Some(frame) = frame
                && frame.confidence > 0.6
            {
                cents.push(frame.cents);
            }
        });
    }
    if cents.len() < 3 {
        return None;
    }
    cents.sort_by(|a, b| a.total_cmp(b));
    let median = cents[cents.len() / 2];
    let key = (median / 100.0).round();
    if !(0.0..=127.0).contains(&key) {
        return None;
    }
    Some((key as u8, median - key * 100.0))
}

/// The keys each of `roots` serves — `fontelle_core::key_ranges`, which
/// lives there because the bank's own sampled sets split the keyboard by
/// the same rule.
pub use fontelle_core::key_ranges;

/// Whether a sound of `frames` samples is shaped like a **wavetable**: a
/// whole number of [`fontelle_dsp::WAVETABLE_LEN`]-sample cycles, no more
/// than a table holds. That is what a wavetable editor exports, and what a
/// recording — whose length is however long somebody held the note — never
/// is by anything but chance.
pub fn looks_like_wavetable(frames: usize) -> bool {
    frames > 0
        && frames.is_multiple_of(fontelle_dsp::WAVETABLE_LEN)
        && frames / fontelle_dsp::WAVETABLE_LEN <= fontelle_dsp::MAX_USER_FRAMES
}

/// The longest recording an oscillator keeps, in seconds. A patch carries
/// its recordings, so a ten-minute file dropped by mistake would be a
/// ten-minute preset; thirty seconds is longer than any note.
pub const MAX_SAMPLE_SECONDS: usize = 30;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_names_are_read_the_way_libraries_write_them() {
        assert_eq!(note_in_name("A4v8"), Some(69));
        assert_eq!(note_in_name("piano_C#3"), Some(49));
        assert_eq!(note_in_name("Grand-Db5-soft"), Some(73));
        assert_eq!(note_in_name("kick c1"), Some(24));
        assert_eq!(note_in_name("C-1"), Some(0));
        assert_eq!(note_in_name("G9"), Some(127));
        assert_eq!(note_in_name("Bass"), None, "a word is not a note");
        assert_eq!(note_in_name("Ab_Key"), None, "no octave, no note");
        assert_eq!(note_in_name("C12"), None, "two digits is not an octave");
        assert_eq!(note_in_name("thing"), None);
    }

    #[test]
    fn ranges_split_halfway_between_roots() {
        assert_eq!(
            key_ranges(&[48, 60, 72]),
            vec![(0, 54), (55, 66), (67, 127)]
        );
        assert_eq!(key_ranges(&[60]), vec![(0, 127)]);
        assert_eq!(key_ranges(&[]), Vec::<(u8, u8)>::new());
        // Every key is claimed exactly once.
        let ranges = key_ranges(&[36, 43, 60, 61, 100]);
        for key in 0..=127u8 {
            let claims = ranges
                .iter()
                .filter(|(lo, hi)| *lo <= key && key <= *hi)
                .count();
            assert_eq!(claims, 1, "key {key} is claimed {claims} times: {ranges:?}");
        }
    }

    #[test]
    fn a_wavetable_is_whole_cycles_and_nothing_else_is() {
        assert!(looks_like_wavetable(2_048));
        assert!(looks_like_wavetable(8_192));
        assert!(!looks_like_wavetable(8_191));
        assert!(!looks_like_wavetable(24_000));
        assert!(!looks_like_wavetable(0));
        assert!(!looks_like_wavetable(2_048 * 65), "more than a table holds");
    }

    #[test]
    fn the_root_of_a_sine_is_found_with_its_offset() {
        let sr = 48_000u32;
        let hz = 440.0 * 2f32.powf(20.0 / 1200.0);
        let samples: Vec<f32> = (0..sr as usize)
            .map(|i| (std::f32::consts::TAU * hz * i as f32 / sr as f32).sin() * 0.5)
            .collect();
        let (key, cents) = detect_root(&samples, sr).expect("a sine has a pitch");
        assert_eq!(key, 69);
        assert!((cents - 20.0).abs() < 10.0, "{cents}");
        // Noise has none.
        let mut x = 0x2545_f491u32;
        let noise: Vec<f32> = (0..sr as usize)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                (x >> 8) as f32 / 8_388_608.0 - 1.0
            })
            .collect();
        assert!(detect_root(&noise, sr).is_none());
    }
}
