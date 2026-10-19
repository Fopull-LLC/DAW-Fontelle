//! A deleted note is zapped out of existence (Ty, 2026-10-19: *"id prefer
//! it to look more like it got poofed or zapped out of existance fading to
//! light and then dust"* — the old burst read as *"clumps of blue dirt"*):
//! the note flashes to light and collapses to a thin line, and fine sparks,
//! white-hot at first, cool to the note's colour as they drift and fade.
//! Under half a second, and never a storm: a big delete bursts from the
//! selection's outline, not from every note.

use fontelle_ui::canvas::{Fleck, FleckKind, MAX_BURSTS, PARTICLE_SECONDS, Particles};
use fontelle_ui::layout::Rect;

const RED: [u8; 4] = [220, 60, 60, 255];

fn flashes(p: &Particles) -> Vec<Fleck> {
    p.flecks()
        .into_iter()
        .filter(|f| f.kind == FleckKind::Flash)
        .collect()
}

fn sparks(p: &Particles) -> Vec<Fleck> {
    p.flecks()
        .into_iter()
        .filter(|f| f.kind == FleckKind::Spark)
        .collect()
}

/// How far a colour is from white, summed over the channels.
fn from_white(c: [u8; 4]) -> u32 {
    c[..3].iter().map(|&v| 255 - u32::from(v)).sum()
}

#[test]
fn the_note_flashes_to_light_where_it_was() {
    let mut p = Particles::default();
    let note = Rect::new(100.0, 200.0, 60.0, 14.0);
    p.burst(&[note], RED);
    let flash = flashes(&p);
    assert_eq!(flash.len(), 1, "one flash per note");
    let f = flash[0];
    assert!((f.x - (note.x + note.width / 2.0)).abs() < 0.5);
    assert!((f.y - (note.y + note.height / 2.0)).abs() < 0.5);
    assert!((f.width - note.width).abs() < 1.0, "{f:?}");
    assert!((f.height - note.height).abs() < 1.0, "{f:?}");
    // Brighter than the note: light, not paint.
    for (lit, was) in f.colour[..3].iter().zip(&RED[..3]) {
        assert!(lit >= was, "{f:?} is not lighter than the note");
    }
    assert!(from_white(f.colour) < from_white(RED) / 2);

    // Half way through, it has collapsed to a thin line and is fading.
    p.step(PARTICLE_SECONDS * 0.4);
    let f = flashes(&p);
    assert!(
        f.iter()
            .all(|f| f.height < note.height * 0.5 && f.colour[3] < 255),
        "{f:?}"
    );
}

/// Sparks are fine, not clumps: never more than a couple of pixels.
#[test]
fn sparks_are_fine_dust() {
    let mut p = Particles::default();
    let note = Rect::new(100.0, 200.0, 60.0, 14.0);
    p.burst(&[note], RED);
    let mut seen = 0;
    let mut t = 0.0;
    while t < PARTICLE_SECONDS {
        for s in sparks(&p) {
            seen += 1;
            assert!(
                s.size <= 2.5 && s.width <= 2.5 && s.height <= 2.5,
                "{s:?} is a clump"
            );
        }
        p.step(0.02);
        t += 0.02;
    }
    assert!(seen > 0, "no sparks");
}

/// Sparks start on the note, white-hot, and cool to its colour as they go.
#[test]
fn sparks_start_hot_and_cool_to_the_notes_colour() {
    let mut p = Particles::default();
    let note = Rect::new(100.0, 200.0, 60.0, 14.0);
    p.burst(&[note], RED);
    p.step(0.01);
    let early = sparks(&p);
    assert!(early.len() >= 16, "{} sparks", early.len());
    for s in &early {
        assert!(
            note.inset(-2.0).contains(s.x, s.y),
            "{s:?} starts off the note"
        );
        assert!(
            from_white(s.colour) < from_white(RED) / 2,
            "{s:?} is not hot"
        );
    }
    p.step(PARTICLE_SECONDS * 0.6);
    let late = sparks(&p);
    assert!(!late.is_empty());
    // Red is the strongest channel again: the note's own hue.
    assert!(
        late.iter().all(|s| s.colour[0] > s.colour[1] + 40),
        "{late:?}"
    );
    assert!(late.iter().all(|s| s.colour[3] < 200), "they do not fade");
    let moved = late
        .iter()
        .zip(&early)
        .any(|(a, b)| (a.x - b.x).abs() + (a.y - b.y).abs() > 1.0);
    assert!(moved, "they do not move");
}

#[test]
fn it_is_gone_in_under_half_a_second() {
    const { assert!(PARTICLE_SECONDS <= 0.45) };
    let mut p = Particles::default();
    p.burst(&[Rect::new(0.0, 0.0, 30.0, 10.0)], RED);
    assert!(p.is_moving());
    p.step(PARTICLE_SECONDS * 0.5);
    assert!(p.is_moving());
    p.step(PARTICLE_SECONDS);
    assert!(p.flecks().is_empty());
    assert!(!p.is_moving());
}

/// Two hundred notes are not two hundred bursts.
#[test]
fn a_big_delete_bursts_from_the_outline_not_every_note() {
    let mut p = Particles::default();
    let notes: Vec<Rect> = (0..200)
        .map(|i| Rect::new(10.0 + i as f32 * 4.0, 300.0, 3.0, 10.0))
        .collect();
    p.burst(&notes, RED);
    assert!(p.bursts() <= MAX_BURSTS, "{} bursts", p.bursts());
    let small: Vec<Rect> = notes[..3].to_vec();
    let mut q = Particles::default();
    q.burst(&small, RED);
    assert_eq!(q.bursts(), 3, "a few notes burst each");
}

/// The same delete makes the same burst: nothing random about it.
#[test]
fn a_burst_is_the_same_every_time() {
    let note = Rect::new(40.0, 50.0, 30.0, 12.0);
    let mut a = Particles::default();
    let mut b = Particles::default();
    a.burst(&[note], RED);
    b.burst(&[note], RED);
    a.step(0.1);
    b.step(0.1);
    assert_eq!(a.flecks(), b.flecks());
}
