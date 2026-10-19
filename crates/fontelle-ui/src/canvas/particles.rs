//! A deleted note bursts into a few fading flecks of its colour, and is
//! gone in under half a second. Ty, 2026-10-18.
//!
//! Pure and stepped by time, the way the mixer's cables are: no clock is read
//! here, and the same delete makes the same burst. A big delete bursts from
//! the outline of what went, not from every note — two hundred bursts would
//! be a storm, not a flourish.

use crate::layout::Rect;

/// How long a fleck lives, in seconds.
pub const PARTICLE_SECONDS: f32 = 0.35;
/// The most bursts one delete makes; past it the outline bursts instead.
pub const MAX_BURSTS: usize = 12;
/// Flecks a burst throws.
const FLECKS: usize = 10;

/// One fleck as it is drawn now: where, how big, and its colour with the
/// fade in its alpha.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fleck {
    pub x: f32,
    pub y: f32,
    pub size: f32,
    pub colour: [u8; 4],
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Live {
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
    size: f32,
    colour: [u8; 4],
    age: f32,
}

/// Every burst still in the air.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Particles {
    live: Vec<Live>,
    bursts: usize,
}

/// A small fixed sequence in 0..1, so a burst is the same every time.
fn wobble(seed: u32) -> f32 {
    let mut x = seed.wrapping_mul(0x9E37_79B9).wrapping_add(0x7F4A_7C15);
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^= x >> 12;
    (x & 0xFFFF) as f32 / 65535.0
}

impl Particles {
    /// Bursts each of `notes` (or, when there are more than [`MAX_BURSTS`],
    /// the outline round them all) in `colour`.
    pub fn burst(&mut self, notes: &[Rect], colour: [u8; 4]) {
        let outline;
        let rects: &[Rect] = if notes.len() > MAX_BURSTS {
            let mut r = notes[0];
            for n in &notes[1..] {
                let (x0, y0) = (r.x.min(n.x), r.y.min(n.y));
                let (x1, y1) = (r.right().max(n.right()), r.bottom().max(n.bottom()));
                r = Rect::new(x0, y0, x1 - x0, y1 - y0);
            }
            outline = [r];
            &outline
        } else {
            notes
        };
        for (i, r) in rects.iter().enumerate() {
            if r.is_empty() {
                continue;
            }
            self.bursts += 1;
            for k in 0..FLECKS {
                let seed = (i * FLECKS + k) as u32;
                let along = wobble(seed * 3 + 1);
                let angle = wobble(seed * 3 + 2) * std::f32::consts::TAU;
                let speed = 40.0 + 80.0 * wobble(seed * 3 + 3);
                self.live.push(Live {
                    x: r.x + r.width * along,
                    y: r.y + r.height * 0.5,
                    vx: angle.cos() * speed,
                    vy: angle.sin() * speed - 30.0,
                    size: 3.5 + 3.0 * wobble(seed * 5 + 7),
                    colour,
                    age: 0.0,
                });
            }
        }
    }

    /// Moves everything on by `dt` seconds: flecks drift, fall a little and
    /// fade; the spent ones go.
    pub fn step(&mut self, dt: f32) {
        for f in &mut self.live {
            f.age += dt;
            f.x += f.vx * dt;
            f.y += f.vy * dt;
            f.vy += 220.0 * dt;
            f.vx *= 1.0 - (3.0 * dt).min(1.0);
        }
        self.live.retain(|f| f.age < PARTICLE_SECONDS);
        if self.live.is_empty() {
            self.bursts = 0;
        }
    }

    /// The flecks as they are drawn now.
    pub fn flecks(&self) -> Vec<Fleck> {
        self.live
            .iter()
            .map(|f| {
                let left = (1.0 - f.age / PARTICLE_SECONDS).clamp(0.0, 1.0);
                let mut colour = f.colour;
                // Bright for most of the flight, then gone: the square root
                // keeps a fleck visible until near its end.
                let fade = if f.age > 0.0 { left.sqrt() * 0.95 } else { 1.0 };
                colour[3] = (f32::from(colour[3]) * fade) as u8;
                Fleck {
                    x: f.x,
                    y: f.y,
                    size: f.size * (0.5 + 0.5 * left),
                    colour,
                }
            })
            .collect()
    }

    /// Whether anything is still in the air — what keeps frames coming.
    pub fn is_moving(&self) -> bool {
        !self.live.is_empty()
    }

    /// How many bursts are in the air.
    pub fn bursts(&self) -> usize {
        self.bursts
    }
}
