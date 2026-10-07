//! The wavtool's arithmetic: where each unit starts, how it overlaps the
//! one before, and which part of its sample plays when.

use std::collections::BTreeMap;
use std::path::Path;

use fontelle_sing::{Phrase, PhraseNote};
use fontelle_utau::bank::Voicebank;
use fontelle_utau::oto::OtoEntry;
use fontelle_utau::phonemize::{Plan, PlannedUnit};
use fontelle_utau::render::{self, Placed};

fn e(alias: &str, pre: f64, ovl: f64) -> OtoEntry {
    OtoEntry {
        alias: alias.into(),
        file: format!("{alias}.wav").into(),
        folder: String::new(),
        offset: 100.0,
        consonant: 150.0,
        cutoff: -500.0,
        preutterance: pre,
        overlap: ovl,
    }
}

fn phrase(notes: &[(f64, f64)]) -> Phrase {
    Phrase {
        voice: "v".into(),
        language: "ja".into(),
        notes: notes
            .iter()
            .map(|&(s, d)| PhraseNote::new(s, d, 60.0, "a"))
            .collect(),
        curves: BTreeMap::new(),
        extra_curves: BTreeMap::new(),
        settings: BTreeMap::new(),
        state: None,
    }
}

fn unit(alias: &str, note: usize, anchor: f64) -> PlannedUnit {
    PlannedUnit {
        alias: alias.into(),
        note,
        anchor,
        start: None,
        tail: false,
    }
}

fn place(entries: Vec<OtoEntry>, units: Vec<PlannedUnit>, p: &Phrase) -> Vec<Placed> {
    let bank = Voicebank::from_parts(Path::new("/b"), "b", entries, vec![]);
    let plan = Plan {
        units,
        guessed: vec![false; p.notes.len()],
    };
    render::place(&bank, &plan, p, &|_| Some(2000.0))
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

#[test]
fn a_unit_starts_its_preutterance_before_the_note_and_the_next_overlaps_it() {
    let p = phrase(&[(0.0, 0.5), (0.5, 0.5)]);
    let placed = place(
        vec![e("A", 50.0, 10.0), e("B", 100.0, 30.0)],
        vec![unit("A", 0, 0.0), unit("B", 1, 0.5)],
        &p,
    );
    assert_eq!(placed.len(), 2);
    let (a, b) = (&placed[0], &placed[1]);
    assert!(close(a.start, -0.05), "{}", a.start);
    assert!(close(b.start, 0.4));
    assert!(close(a.end, 0.43), "{}", a.end);
    assert!(close(b.fade_in, 0.03) && close(a.fade_out, 0.03));
    // The last unit sounds to its note's end.
    assert!(close(b.end, 1.0), "{}", b.end);
    assert!(close(a.consonant_scale, 1.0));
}

#[test]
fn a_consonant_longer_than_half_the_unit_before_is_pulled_in() {
    let p = phrase(&[(0.0, 0.1), (0.1, 0.5)]);
    let placed = place(
        vec![e("A", 20.0, 10.0), e("B", 100.0, 20.0)],
        vec![unit("A", 0, 0.0), unit("B", 1, 0.1)],
        &p,
    );
    let b = &placed[1];
    // (100 - 20) ms against 50 ms: scaled by 0.625.
    assert!(close(b.start, 0.1 - 0.0625), "{}", b.start);
    assert!(close(b.fade_in, 0.0125));
    assert!(close(b.consonant_scale, 0.625));
}

#[test]
fn velocity_quickens_the_consonant() {
    let mut p = phrase(&[(0.0, 0.5), (0.5, 0.5)]);
    p.notes[1].velocity = 2.0;
    let placed = place(
        vec![e("A", 50.0, 10.0), e("B", 100.0, 30.0)],
        vec![unit("A", 0, 0.0), unit("B", 1, 0.5)],
        &p,
    );
    let b = &placed[1];
    assert!(close(b.start, 0.45));
    assert!(close(b.fade_in, 0.015));
    assert!(close(b.consonant_scale, 0.5));
}

#[test]
fn the_consonant_plays_as_recorded_and_the_vowel_stretches() {
    let p = phrase(&[(1.0, 1.0)]);
    let placed = place(vec![e("A", 120.0, 10.0)], vec![unit("A", 0, 1.0)], &p);
    let a = &placed[0];
    assert!(close(a.start, 0.88));
    assert!(close(a.end, 2.0));
    assert!((a.source_ms(0.88) - 100.0).abs() < 1e-6);
    assert!((a.source_ms(0.98) - 200.0).abs() < 1e-6);
    assert!((a.source_ms(1.03) - 250.0).abs() < 1e-6);
    // 350 ms of vowel over 970 ms.
    assert!((a.source_ms(2.0) - 600.0).abs() < 1e-6);
    let mid = 1.03 + 0.97 / 2.0;
    assert!((a.source_ms(mid) - 425.0).abs() < 1e-6);
}

#[test]
fn a_short_note_plays_the_vowel_as_recorded() {
    let p = phrase(&[(1.0, 0.2)]);
    let placed = place(vec![e("A", 120.0, 10.0)], vec![unit("A", 0, 1.0)], &p);
    let a = &placed[0];
    assert!(close(a.end, 1.2));
    assert!((a.source_ms(1.2) - 420.0).abs() < 1e-6);
}

#[test]
fn weights_fade_in_over_the_overlap_and_out_at_the_end() {
    let p = phrase(&[(0.0, 0.5), (0.5, 0.5)]);
    let placed = place(
        vec![e("A", 50.0, 10.0), e("B", 100.0, 30.0)],
        vec![unit("A", 0, 0.0), unit("B", 1, 0.5)],
        &p,
    );
    let (a, b) = (&placed[0], &placed[1]);
    assert!(close(b.weight(0.4), 0.0));
    assert!(close(b.weight(0.415), 0.5));
    assert!(close(b.weight(0.6), 1.0));
    assert!(close(a.weight(0.415), 0.5));
    assert!(close(a.weight(0.2), 1.0));
    assert!(close(a.weight(0.43), 0.0));
    assert!(close(a.weight(0.0) + 0.0, 1.0));
    assert!(close(b.weight(1.5), 0.0));
}

#[test]
fn a_tail_plays_its_own_length_and_a_rest_ends_the_unit_before() {
    let p = phrase(&[(0.0, 0.5), (1.0, 0.5)]);
    let mut t = unit("R", 0, 0.5);
    t.tail = true;
    let placed = place(
        vec![e("A", 50.0, 10.0), e("R", 40.0, 20.0), e("B", 60.0, 10.0)],
        vec![unit("A", 0, 0.0), t, unit("B", 1, 1.0), unit("A", 1, 1.3)],
        &p,
    );
    let r = &placed[1];
    assert!(close(r.start, 0.46));
    assert!(close(placed[0].end, 0.48), "chained into the tail");
    // R plays offset..end of its sample: 500 ms.
    assert!(close(r.end, 0.96), "{}", r.end);
    let pre_rest = place(
        vec![e("A", 50.0, 10.0), e("B", 60.0, 10.0)],
        vec![unit("A", 0, 0.0), unit("B", 1, 1.0)],
        &p,
    );
    assert!(close(pre_rest[0].end, 0.5), "a rest: the note's end");
}

#[test]
fn a_hand_placed_start_is_kept() {
    let p = phrase(&[(0.0, 0.5), (0.5, 0.5)]);
    let mut b = unit("B", 1, 0.5);
    b.start = Some(0.3);
    let placed = place(
        vec![e("A", 50.0, 10.0), e("B", 100.0, 30.0)],
        vec![unit("A", 0, 0.0), b],
        &p,
    );
    assert!(close(placed[1].start, 0.3));
    assert!(close(placed[0].end, 0.33));
}
