//! Placing phonemes in time, frames, curves and noise.

use fontelle_diffsinger::curves::{
    apply_gain, db_delta, gender_input, midi_to_hz, resample, sample, tension_delta,
};
use fontelle_diffsinger::noise::Gaussian;
use fontelle_diffsinger::phonemize::{NotePhonemes, Ph};
use fontelle_diffsinger::timing::{
    DEFAULT_CONSONANT, MIN_PHONEME, Placed, dur_input, flatten, frames_of, pitch_notes, place,
    segments,
};
use fontelle_sing::{Curve, PhraseNote};

fn np(lead: &[&str], body: &[&str]) -> NotePhonemes {
    NotePhonemes {
        lead: lead.iter().map(|s| Ph::new(*s)).collect(),
        body: body.iter().map(|s| Ph::new(*s)).collect(),
        guessed: false,
    }
}

fn vowel(s: &str) -> bool {
    matches!(s, "a" | "i" | "o" | "SP" | "AP")
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

fn find<'a>(p: &'a [Placed], symbol: &str) -> &'a Placed {
    p.iter().find(|x| x.symbol == symbol).unwrap()
}

#[test]
fn flatten_lists_each_notes_lead_then_body() {
    let per = [np(&["k"], &["a", "n"]), np(&[], &["i"])];
    assert_eq!(flatten(&per), vec![(0, "k"), (0, "a"), (0, "n"), (1, "i")]);
}

#[test]
fn onsets_end_on_the_note_and_vowels_fill_it() {
    let notes = [
        PhraseNote::new(0.0, 0.5, 60.0, "ka"),
        PhraseNote::new(0.5, 0.5, 62.0, "si"),
    ];
    let per = [np(&["k"], &["a"]), np(&["s"], &["i"])];
    let p = place(&notes, &per, None, &vowel);
    assert_eq!(p.len(), 4);
    let k = find(&p, "k");
    assert!(close(k.start, -DEFAULT_CONSONANT) && close(k.end, 0.0));
    let a = find(&p, "a");
    assert!(close(a.start, 0.0));
    // The vowel runs into the next onset, which ends on the next note.
    let s = find(&p, "s");
    assert!(close(a.end, s.start));
    assert!(close(s.end, 0.5));
    assert!(close(find(&p, "i").end, 1.0));
    // In order, back to back.
    for w in p.windows(2) {
        assert!(w[0].start < w[1].start);
        assert!(w[0].end <= w[1].start + 1e-12);
    }
}

#[test]
fn velocity_shortens_the_onset() {
    let mut n = PhraseNote::new(0.0, 1.0, 60.0, "ka");
    n.velocity = 2.0;
    let p = place(&[n], &[np(&["k"], &["a"])], None, &vowel);
    assert!(close(find(&p, "k").start, -DEFAULT_CONSONANT / 2.0));
    // With the duration model's lengths.
    let mut n = PhraseNote::new(0.0, 1.0, 60.0, "ka");
    n.velocity = 0.5;
    let p = place(&[n], &[np(&["k"], &["a"])], Some(&[0.08, 0.3]), &vowel);
    assert!(close(find(&p, "k").start, -0.16));
}

#[test]
fn an_onset_that_does_not_fit_is_squeezed_into_the_note_before() {
    let notes = [
        PhraseNote::new(0.0, 0.2, 60.0, "a"),
        PhraseNote::new(0.2, 0.5, 60.0, "stra"),
    ];
    let per = [np(&[], &["a"]), np(&["s", "t", "r"], &["a"])];
    let p = place(&notes, &per, Some(&[0.1, 0.2, 0.2, 0.2, 0.1]), &vowel);
    // 0.6 s of onset, but only half the 0.2 s note before may go.
    let s = &p[1];
    assert_eq!(s.symbol, "s");
    assert!(close(s.start, 0.1), "{}", s.start);
    assert!(close(p[0].end, s.start));
}

#[test]
fn a_rest_takes_the_onset_and_leaves_silence() {
    let notes = [
        PhraseNote::new(0.0, 0.4, 60.0, "a"),
        PhraseNote::new(0.55, 0.4, 60.0, "ka"),
    ];
    let per = [np(&[], &["a"]), np(&["k"], &["a"])];
    let p = place(&notes, &per, None, &vowel);
    assert!(close(p[0].end, 0.4), "the vowel stops at its note's end");
    assert!(close(p[1].start, 0.55 - DEFAULT_CONSONANT));
    let s = segments(&p, 0.1, 0.1);
    let symbols: Vec<&str> = s.iter().map(|x| x.symbol.as_str()).collect();
    assert_eq!(symbols, ["SP", "a", "SP", "k", "a", "SP"]);
    assert!(close(s[0].start, -0.1));
    assert!(close(s[2].start, 0.4) && close(s[2].end, 0.55 - DEFAULT_CONSONANT));
    assert!(close(s[5].end, 0.95 + 0.1));
    assert_eq!(s[3].placed, Some(1));
    assert_eq!(s[0].placed, None);
}

#[test]
fn codas_close_the_span_and_melismas_extend_it() {
    let notes = [
        PhraseNote::new(0.0, 0.5, 60.0, "an"),
        PhraseNote::new(0.5, 0.5, 64.0, "-"),
    ];
    let per = [np(&[], &["a", "n"]), np(&[], &[])];
    let p = place(&notes, &per, None, &vowel);
    assert_eq!(p.len(), 2);
    assert!(close(p[0].start, 0.0));
    let n = find(&p, "n");
    assert!(
        close(n.end, 1.0),
        "the slur holds the vowel; the coda ends it"
    );
    assert!(close(n.start, 1.0 - DEFAULT_CONSONANT));
}

#[test]
fn hand_placed_phonemes_stay_put() {
    let notes = [PhraseNote::new(0.0, 1.0, 60.0, "ka")];
    let mut per = [np(&["k"], &["a"])];
    per[0].lead[0].offset = Some(-0.2);
    per[0].body[0].offset = Some(0.05);
    let p = place(&notes, &per, None, &vowel);
    assert!(close(p[0].start, -0.2));
    assert!(close(p[0].end, 0.05));
    assert!(close(p[1].start, 0.05));
    // Offsets that cross are put back in order.
    let mut per = [np(&["k"], &["a"])];
    per[0].lead[0].offset = Some(0.3);
    per[0].body[0].offset = Some(0.1);
    let p = place(&notes, &per, None, &vowel);
    assert!(p[1].start >= p[0].start + MIN_PHONEME - 1e-12);
}

#[test]
fn frames_round_at_boundaries_and_never_vanish() {
    let notes = [PhraseNote::new(0.0, 1.0, 60.0, "a")];
    let p = place(&notes, &[np(&[], &["a"])], None, &vowel);
    let s = segments(&p, 0.05, 0.05);
    let f = frames_of(&s, 0.01);
    assert_eq!(f, vec![5, 100, 5]);
    // A segment shorter than a frame still gets one, and the total holds.
    let s = vec![
        fontelle_diffsinger::timing::Segment {
            symbol: "SP".into(),
            start: 0.0,
            end: 0.031,
            placed: None,
        },
        fontelle_diffsinger::timing::Segment {
            symbol: "k".into(),
            start: 0.031,
            end: 0.033,
            placed: Some(0),
        },
        fontelle_diffsinger::timing::Segment {
            symbol: "a".into(),
            start: 0.033,
            end: 0.1,
            placed: Some(1),
        },
    ];
    let f = frames_of(&s, 0.01);
    assert!(f.iter().all(|&x| x >= 1), "{f:?}");
    assert_eq!(f.iter().sum::<i64>(), 10);
}

#[test]
fn the_duration_input_groups_syllables_from_vowel_to_vowel() {
    let notes = [
        PhraseNote::new(0.0, 0.5, 60.0, "ka"),
        PhraseNote::new(0.5, 0.5, 62.0, "sin"),
        PhraseNote::new(1.2, 0.5, 64.0, "to"),
    ];
    let per = [
        np(&["k"], &["a"]),
        np(&["s"], &["i", "n"]),
        np(&["t"], &["o"]),
    ];
    let d = dur_input(&notes, &per, 0.01);
    assert_eq!(d.symbols, ["SP", "k", "a", "s", "i", "n", "SP", "t", "o"]);
    assert_eq!(d.word_div, [2, 2, 2, 2, 1]);
    // Padding before the first note, each syllable to the next note, the
    // rest, the last note.
    assert_eq!(d.word_dur, [50, 50, 50, 20, 50]);
    assert_eq!(d.ph_midi, [60, 60, 60, 62, 62, 62, 64, 64, 64]);
    assert_eq!(
        d.index,
        [
            None,
            Some(0),
            Some(1),
            Some(2),
            Some(3),
            Some(4),
            None,
            Some(5),
            Some(6)
        ]
    );
}

#[test]
fn pitch_notes_fill_rests_with_neighbouring_pitches() {
    let notes = [
        PhraseNote::new(0.0, 0.5, 60.0, "a"),
        PhraseNote::new(0.7, 0.5, 67.0, "a"),
    ];
    let pn = pitch_notes(&notes, &[true, true], -0.1, 0.01, 140);
    assert_eq!(pn.rest, [true, false, true, false, true]);
    assert_eq!(pn.dur, [10, 50, 20, 50, 10]);
    assert_eq!(pn.dur.iter().sum::<i64>(), 140);
    assert_eq!(pn.midi[0], 60.0);
    assert_eq!(pn.midi[4], 67.0);
    // The gap takes either neighbour's pitch.
    assert!(pn.midi[2] == 60.0 || pn.midi[2] == 67.0);
    // A note that sings no vowel is a rest to the model.
    let pn = pitch_notes(&notes, &[true, false], -0.1, 0.01, 140);
    assert!(pn.rest[3]);
    assert_eq!(pn.midi[3], 60.0);
}

#[test]
fn curves_sample_resample_and_convert() {
    assert!((midi_to_hz(69.0) - 440.0).abs() < 1e-3);
    assert!((midi_to_hz(57.0) - 220.0).abs() < 1e-3);
    let c = Curve {
        start: 0.0,
        step: 0.1,
        values: vec![0.0, 1.0],
    };
    assert_eq!(sample(Some(&c), 0.0, 0.05, 3, 9.0), vec![0.0, 0.5, 1.0]);
    assert_eq!(sample(None, 0.0, 0.05, 2, 9.0), vec![9.0, 9.0]);
    let r = resample(&[0.0, 10.0, 20.0], 0.0, 0.1, 0.05, 0.05, 6);
    assert_eq!(r.len(), 6);
    assert!((r[0] - 5.0).abs() < 1e-4);
    assert!((r[5] - 20.0).abs() < 1e-4, "held past the end");
    // Gender: an octave of range either side would be ±1.
    assert!((gender_input(500.0, (-5.0, 5.0)) - 1.0).abs() < 1e-6);
    assert!((gender_input(-250.0, (-5.0, 10.0)) + 0.5).abs() < 1e-6);
    assert_eq!(gender_input(5000.0, (-5.0, 5.0)), 1.0, "clamped");
    assert_eq!(gender_input(300.0, (0.0, 0.0)), 0.0, "no range, no shift");
    assert_eq!(db_delta(0.0), 0.0);
    assert!(db_delta(1.0) > 0.0 && db_delta(-1.0) < 0.0);
    assert!(tension_delta(1.0) > 0.0);
}

#[test]
fn dynamics_are_gain_in_db() {
    let mut s = vec![1.0f32; 4];
    let c = Curve {
        start: 0.0,
        step: 0.5,
        values: vec![0.0, -20.0],
    };
    apply_gain(&mut s, 4, 0.0, &c);
    assert!((s[0] - 1.0).abs() < 1e-6);
    assert!((s[2] - 0.1).abs() < 1e-4, "{s:?}");
}

#[test]
fn noise_is_normal_and_follows_the_seed() {
    let mut a = Gaussian::new(7);
    let mut b = Gaussian::new(7);
    let mut c = Gaussian::new(8);
    let mut va = vec![0.0; 100_000];
    let mut vb = vec![0.0; 100_000];
    let mut vc = vec![0.0; 100_000];
    a.fill(&mut va);
    b.fill(&mut vb);
    c.fill(&mut vc);
    assert_eq!(va, vb);
    assert_ne!(va, vc);
    let mean = va.iter().map(|&x| f64::from(x)).sum::<f64>() / va.len() as f64;
    let var = va
        .iter()
        .map(|&x| (f64::from(x) - mean).powi(2))
        .sum::<f64>()
        / va.len() as f64;
    assert!(mean.abs() < 0.02, "{mean}");
    assert!((var - 1.0).abs() < 0.03, "{var}");
    assert!(va.iter().all(|x| x.is_finite()));
}

#[test]
fn word_mode_divides_at_sung_vowels() {
    use fontelle_diffsinger::timing::{Segment, word_division};
    let seg = |symbol: &str, placed: Option<usize>| Segment {
        symbol: symbol.into(),
        start: 0.0,
        end: 0.0,
        placed,
    };
    let segs = [
        seg("SP", None),
        seg("k", Some(0)),
        seg("a", Some(1)),
        seg("s", Some(2)),
        seg("i", Some(3)),
        seg("n", Some(4)),
        seg("SP", None),
        seg("t", Some(5)),
        seg("o", Some(6)),
        seg("SP", None),
    ];
    let frames = [8, 5, 40, 5, 40, 5, 10, 5, 40, 8];
    let (div, dur) = word_division(&segs, &frames, &vowel);
    assert_eq!(div, [2, 2, 4, 2]);
    assert_eq!(dur, [13, 45, 60, 48]);
    // No vowel at all: the last phoneme starts the only word after the
    // leading silence (as OpenUtau divides it).
    let segs = [seg("SP", None), seg("k", Some(0)), seg("SP", None)];
    let (div, dur) = word_division(&segs, &[8, 5, 8], &vowel);
    assert_eq!(div, [1, 2]);
    assert_eq!(dur, [8, 13]);
}
