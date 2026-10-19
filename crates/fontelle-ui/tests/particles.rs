//! A deleted note bursts into a few fading flecks of its colour (Ty,
//! 2026-10-18) — under half a second, and never a storm: a big delete
//! bursts from the selection's outline, not from every note.

use fontelle_ui::canvas::{MAX_BURSTS, PARTICLE_SECONDS, Particles};
use fontelle_ui::layout::Rect;

const RED: [u8; 4] = [220, 60, 60, 255];

#[test]
fn a_burst_starts_on_the_note_and_is_gone_in_under_half_a_second() {
    let mut p = Particles::default();
    let note = Rect::new(100.0, 200.0, 60.0, 14.0);
    p.burst(&[note], RED);
    let flecks = p.flecks();
    assert!(!flecks.is_empty());
    for f in &flecks {
        assert!(
            note.inset(-1.0).contains(f.x, f.y),
            "{f:?} starts off the note"
        );
        assert_eq!([f.colour[0], f.colour[1], f.colour[2]], [220, 60, 60]);
    }
    const { assert!(PARTICLE_SECONDS <= 0.4) };
    p.step(PARTICLE_SECONDS * 0.5);
    let half = p.flecks();
    assert!(!half.is_empty());
    assert!(half.iter().all(|f| f.colour[3] < 255), "they do not fade");
    let moved = half
        .iter()
        .zip(&flecks)
        .any(|(a, b)| (a.x - b.x).abs() + (a.y - b.y).abs() > 1.0);
    assert!(moved, "they do not move");
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
