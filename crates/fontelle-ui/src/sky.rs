//! The bridge's canopy: a galaxy drawn from the instrument's own sound.
//!
//! > *"the background of the starry galaxy should be sort of like a shader
//! > that is an audio visualizer that simulates galaxies and stars and
//! > clusters of planets and whatnot ... as you play stuff the nebulas and
//! > stars and galaxies will shift in color and shape and orientation to
//! > make a totally unique pattern visualizing that waveform in a surreal
//! > and trippy manner."* — Ty, 2026-09-15
//!
//! # Why this is not a fragment shader
//!
//! The renderer draws a `vello::Scene`, and `draw_window` is a pure function
//! of its inputs so that a frame can be rendered and inspected with no GPU
//! and no window (`render/mod.rs`). A shader pass under the scene would
//! break that split for one window. So the nebula is shaded **here, on the
//! CPU, at a fraction of the canopy's resolution** — a few thousand pixels
//! of domain-warped noise a frame, which is cheaper than shaping the
//! window's text — and the renderer scales it up with a bilinear sampler,
//! which a nebula is soft enough to forgive. The stars, the planets, the
//! shooting stars and the aurora are vector sprites over it, at full
//! resolution, because points of light are the one thing an upscaled image
//! cannot be.
//!
//! # How it listens
//!
//! [`SkyState::tick`] is fed the instrument's sound once a frame — the
//! analyser's bands and a stretch of waveform, off the same tap the EQ's
//! spectrum reads — and eases a handful of numbers towards it: the level,
//! how much of it is low and how much high, where its centre is, and how
//! suddenly it arrived. Everything drawn is a function of those and of a
//! clock that runs faster the louder it is: the nebula's drift, the
//! galaxy's spin, the palette's lean, the warp that folds the clouds, the
//! stars thrown by a transient, and the aurora, which is the waveform
//! itself. A held chord is a slow bloom and a drum hit is a burst; silence
//! is a still sky with the stars still twinkling.
//!
//! Everything is a pure function of the state, so the same state draws the
//! same sky and nothing flickers on its own — `tests/sky.rs` holds it.

use crate::layout::Rect;
use crate::theme::{Color, Palette};

/// What the sky is fed each frame.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SkySound {
    /// The analyser's bands, low to high, in dBFS — `canvas::SPECTRUM_BANDS`
    /// of them, though the sky takes however many it is given.
    pub bands_db: Vec<f32>,
    /// A stretch of the waveform, −1..=1, oldest first.
    pub wave: Vec<f32>,
}

/// The colours the sky is shaded in, from the theme.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SkyPalette {
    /// Deep space, behind everything.
    pub deep: [f32; 3],
    /// The two nebula inks, and a third for the bright cores.
    pub cloud_a: [f32; 3],
    pub cloud_b: [f32; 3],
    pub core: [f32; 3],
    /// Starlight.
    pub star: [f32; 3],
}

impl SkyPalette {
    /// The theme's own sky: the window colour as deep space, the accent and
    /// the modulation violet as the two clouds, the text colour as starlight.
    /// A light theme gets a pale version of the same sky.
    pub fn for_theme(p: &Palette) -> Self {
        let f = |c: Color| {
            [
                c.0[0] as f32 / 255.0,
                c.0[1] as f32 / 255.0,
                c.0[2] as f32 / 255.0,
            ]
        };
        let deep = f(p.window);
        Self {
            deep: [deep[0] * 0.35, deep[1] * 0.35, deep[2] * 0.45],
            cloud_a: f(p.accent),
            cloud_b: f(p.modulation),
            core: f(p.playhead),
            star: f(p.text),
        }
    }
}

/// A shaded nebula, `width × height` RGBA8, opaque.
#[derive(Debug, Clone, PartialEq)]
pub struct SkyImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// One star, placed in a rectangle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StarSprite {
    pub x: f32,
    pub y: f32,
    /// Its size, in pixels.
    pub size: f32,
    /// How bright it is right now, 0..1 — twinkling.
    pub brightness: f32,
    /// A tint, 0..1, from cool to warm.
    pub warmth: f32,
}

/// A star thrown across the sky by a transient.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShootingStar {
    /// Head and tail, in the rectangle.
    pub from: (f32, f32),
    pub to: (f32, f32),
    /// 1 when thrown, fading to 0.
    pub life: f32,
}

/// A planet: a soft disc with a ring, drifting.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlanetSprite {
    pub x: f32,
    pub y: f32,
    pub radius: f32,
    /// Its ring's tilt, as the ratio of the ring's height to its width.
    pub ring: f32,
    /// How lit its day side is, 0..1.
    pub lit: f32,
    /// Which cloud ink it is coloured in, 0..1 between the two.
    pub ink: f32,
}

/// How many bands the sky averages its groups over. The analyser's ninety-six
/// split into thirds: the bottom third is the bass, the top third the air.
const GROUPS: usize = 3;

/// How many fixed stars the field has.
const STARS: usize = 140;

/// The most shooting stars in the air at once.
const MAX_SHOOTING: usize = 10;

/// How long the sky stays awake after the last sound, in seconds — long
/// enough for the last shooting star to land and the clouds to settle.
const AWAKE_S: f32 = 4.0;

#[derive(Debug, Clone, PartialEq)]
struct Thrown {
    from: (f32, f32),
    heading: (f32, f32),
    age: f32,
    speed: f32,
}

/// The sky's state: what it has heard, eased, and where everything is.
#[derive(Debug, Clone, PartialEq)]
pub struct SkyState {
    /// The nebula's clock: runs faster the louder it is.
    time: f32,
    /// The galaxy's rotation, in radians, integrating the energy.
    spin: f32,
    /// Level, 0..1, eased: fast up, slow down.
    level: f32,
    /// The three groups' levels, 0..1, eased the same way.
    groups: [f32; GROUPS],
    /// Where the energy sits, 0 (all bass) to 1 (all air), eased.
    centre: f32,
    /// How suddenly the last frame arrived: the flux, decaying.
    attack: f32,
    /// The level a frame ago, for the flux.
    last_level: f32,
    /// Seconds since anything was heard.
    quiet_for: f32,
    /// The waveform as last heard, for the aurora.
    wave: Vec<f32>,
    /// The fixed field: `(x, y, size, phase, warmth)` in 0..1 units.
    stars: Vec<(f32, f32, f32, f32, f32)>,
    shooting: Vec<Thrown>,
    thrown: usize,
    rng: u32,
}

impl SkyState {
    /// A sky, seeded: the same seed is the same star field.
    pub fn new(seed: u32) -> Self {
        let mut rng = seed.wrapping_mul(2_654_435_761).wrapping_add(0x9e37_79b9) | 1;
        let mut next = || {
            rng ^= rng << 13;
            rng ^= rng >> 17;
            rng ^= rng << 5;
            (rng >> 8) as f32 / 16_777_216.0
        };
        let stars = (0..STARS)
            .map(|_| {
                let x = next();
                let y = next();
                // Mostly small, a few big: the cube keeps the field from
                // reading as a sheet of equal dots.
                let size = 0.6 + next().powi(3) * 2.6;
                let phase = next() * std::f32::consts::TAU;
                let warmth = next();
                (x, y, size, phase, warmth)
            })
            .collect();
        Self {
            time: 0.0,
            spin: 0.0,
            level: 0.0,
            groups: [0.0; GROUPS],
            centre: 0.5,
            attack: 0.0,
            last_level: 0.0,
            quiet_for: AWAKE_S,
            wave: Vec::new(),
            stars,
            shooting: Vec::new(),
            thrown: 0,
            rng,
        }
    }

    fn next_random(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        (self.rng >> 8) as f32 / 16_777_216.0
    }

    /// One frame of listening. `dt` in seconds.
    pub fn tick(&mut self, sound: &SkySound, dt: f32) {
        let dt = dt.clamp(0.0, 0.25);
        // dBFS to 0..1 over sixty decibels: the range a synth's own output
        // occupies between "playing" and "gone".
        let lin = |db: f32| ((db + 60.0) / 60.0).clamp(0.0, 1.0);
        let bands = &sound.bands_db;
        let mut groups = [0.0f32; GROUPS];
        let mut weighted = 0.0f32;
        let mut total = 0.0f32;
        if !bands.is_empty() {
            for (i, db) in bands.iter().enumerate() {
                let v = lin(*db);
                let group = (i * GROUPS / bands.len()).min(GROUPS - 1);
                groups[group] = groups[group].max(v);
                weighted += v * v * i as f32 / (bands.len() - 1).max(1) as f32;
                total += v * v;
            }
        }
        let level = groups.iter().cloned().fold(0.0f32, f32::max);
        let centre = if total > 1e-6 {
            weighted / total
        } else {
            self.centre
        };

        // Ease: straight up on a rise so a transient is seen where it
        // happened, a fall over about half a second so it can be read.
        let ease = |current: f32, target: f32| {
            if target >= current {
                target
            } else {
                let fall = (dt * 2.5).min(1.0);
                current + (target - current) * fall
            }
        };
        let flux = (level - self.last_level).max(0.0);
        self.last_level = level;
        self.level = ease(self.level, level);
        for (g, target) in self.groups.iter_mut().zip(groups) {
            *g = ease(*g, target);
        }
        // The centre drifts rather than jumps: a palette that snapped between
        // notes would strobe.
        self.centre += (centre - self.centre) * (dt * 4.0).min(1.0);
        self.attack = (self.attack - dt * 3.0).max(0.0).max(flux);

        // The clock: a slow drift in silence, up to a brisk one at full level.
        self.time += dt * (0.04 + self.level * 0.5);
        self.spin += dt * (0.02 + self.level * 0.9);

        if level > 0.02 {
            self.quiet_for = 0.0;
        } else {
            self.quiet_for += dt;
        }
        if !sound.wave.is_empty() {
            self.wave.clear();
            self.wave.extend_from_slice(&sound.wave);
        } else if !self.wave.is_empty() && self.quiet_for > 0.0 {
            // Nothing arriving: the ribbon settles rather than freezing.
            for s in &mut self.wave {
                *s *= 0.85;
            }
        }

        // A transient throws a star, a big one throws two; a held note is
        // one transient at its start and then none.
        if flux > 0.12 && self.shooting.len() < MAX_SHOOTING {
            let count = if flux > 0.4 { 2 } else { 1 };
            for _ in 0..count {
                let from = (self.next_random(), self.next_random() * 0.7);
                let angle = 0.15 + self.next_random() * 0.5;
                let heading = (angle.cos(), angle.sin() * 0.6);
                let speed = 0.5 + self.next_random() * 0.6;
                self.shooting.push(Thrown {
                    from,
                    heading,
                    age: 0.0,
                    speed,
                });
                self.thrown += 1;
            }
        }
        for star in &mut self.shooting {
            star.age += dt;
        }
        self.shooting.retain(|star| star.age < 0.9);
    }

    /// The galaxy's rotation, in radians.
    pub fn spin(&self) -> f32 {
        self.spin
    }

    /// How many stars have been thrown since the sky was made.
    pub fn thrown_so_far(&self) -> usize {
        self.thrown
    }

    /// Whether the sky is moving — sounding, or settling after a sound —
    /// which is when the window has to keep drawing it.
    pub fn is_alive(&self) -> bool {
        self.quiet_for < AWAKE_S || !self.shooting.is_empty()
    }

    /// How loud it is, eased, 0..1.
    pub fn level(&self) -> f32 {
        self.level
    }

    /// The nebula, shaded at `width × height`.
    ///
    /// Domain-warped noise, twice: the first warp bends the coordinates by
    /// the sound's mid band, the second by the first, and the clouds are read
    /// off the result — the classic recipe for something that looks like gas
    /// rather than like noise. A spiral galaxy is laid across it, turning
    /// with [`spin`](Self::spin), and the palette leans warm or cool with the
    /// sound's centre.
    pub fn render(&self, width: u32, height: u32, palette: &SkyPalette) -> SkyImage {
        let mut rgba = Vec::with_capacity((width * height * 4) as usize);
        let t = self.time;
        let level = self.level;
        let bass = self.groups[0];
        let mid = self.groups[1];
        let air = self.groups[GROUPS - 1];
        let warp = 0.8 + mid * 2.2;
        // Lean the two cloud inks towards each other by the centre: bass
        // pulls towards ink A, air towards ink B, and a hot core shows through
        // at the loudest.
        let lean = (self.centre - 0.5) * 2.0;
        let a = mix3(
            palette.cloud_a,
            palette.cloud_b,
            (lean * 0.5).clamp(0.0, 1.0) * 0.6,
        );
        let b = mix3(
            palette.cloud_b,
            palette.cloud_a,
            (-lean * 0.5).clamp(0.0, 1.0) * 0.6,
        );
        let aspect = width as f32 / height.max(1) as f32;
        let (sin_s, cos_s) = self.spin.sin_cos();
        let tint = [
            1.0 + bass * 0.6 - air * 0.25,
            1.0 + bass * 0.1 + air * 0.1,
            1.0 + air * 0.6 - bass * 0.3,
        ];
        for py in 0..height {
            let v = py as f32 / height.max(1) as f32;
            for px in 0..width {
                let u = px as f32 / width.max(1) as f32;
                // World coordinates, aspect-correct, the sky wider than tall.
                let x = (u - 0.5) * aspect * 2.0;
                let y = (v - 0.5) * 2.0;

                let q = (
                    fbm(x * 1.1 + t * 0.15, y * 1.1 + t * 0.09),
                    fbm(x * 1.1 + 5.2 + t * 0.07, y * 1.1 + 1.3 - t * 0.11),
                );
                let r = (
                    fbm(x + warp * q.0 + 1.7 + t * 0.05, y + warp * q.1 + 9.2),
                    fbm(x + warp * q.0 + 8.3, y + warp * q.1 + 2.8 - t * 0.04),
                );
                let cloud = fbm(x * 1.4 + warp * r.0, y * 1.4 + warp * r.1);

                // The galaxy: a spiral, turning. Arms where the angle and the
                // log of the radius line up, fading with distance from the
                // core, and swelling with the bass.
                let gx = x * cos_s - y * sin_s;
                let gy = x * sin_s + y * cos_s;
                let radius = (gx * gx + gy * gy).sqrt() + 1e-4;
                let angle = gy.atan2(gx);
                let arm = (angle * 2.0 - radius.ln() * 3.0 + t * 0.3).cos();
                let arm = ((arm + 0.6) * 1.2).clamp(0.0, 1.0) * (1.0 - (radius * 0.7).min(1.0));
                let core = (-(radius * radius) * 6.0).exp();

                // Density and light: the clouds come up with the level, the
                // core glows with the bass, the arms with the air.
                let density = (cloud * 0.7 + r.0 * 0.3).clamp(0.0, 1.0);
                // Contrast: the clouds are thin where the noise is low and
                // bright in their knots, which is what makes gas read as gas.
                let knots = density * density * density;
                let glow = 0.5 + level * 1.1;
                let cloud_light = knots * glow + density * 0.08;
                let arm_light = arm * (0.25 + air * 0.6 + level * 0.3) * density;
                let core_light = core * (0.35 + bass * 1.4);

                // Which ink: the second warp's field, sharpened so the two
                // clouds are two clouds rather than one grey blend, with
                // the core's ink in the densest knots — the clusters.
                let which = ((r.1 - 0.5) * 2.4 + 0.5).clamp(0.0, 1.0);
                let ink = mix3(a, b, which);
                let ink = mix3(ink, palette.core, ((density - 0.72) * 3.0).clamp(0.0, 1.0) * 0.6);
                let mut rgb = palette.deep;
                for c in 0..3 {
                    rgb[c] +=
                        ink[c] * cloud_light + ink[c] * arm_light + palette.core[c] * core_light;
                    // The lean: bass warms the whole sky towards ember, air
                    // cools it towards ice — over and above the two inks,
                    // which a theme may have chosen close together.
                    rgb[c] *= tint[c];
                    // A soft shoulder rather than a clip, so a loud passage
                    // blooms instead of burning to white.
                    rgb[c] = 1.0 - (-rgb[c] * 1.4).exp();
                }
                rgba.push((rgb[0] * 255.0) as u8);
                rgba.push((rgb[1] * 255.0) as u8);
                rgba.push((rgb[2] * 255.0) as u8);
                rgba.push(255);
            }
        }
        SkyImage {
            width,
            height,
            rgba,
        }
    }

    /// The fixed stars, placed in `rect`, twinkling with the clock and
    /// brightening with the air.
    pub fn stars(&self, rect: Rect) -> Vec<StarSprite> {
        if rect.is_empty() {
            return Vec::new();
        }
        let air = self.groups[GROUPS - 1];
        self.stars
            .iter()
            .map(|(x, y, size, phase, warmth)| {
                let twinkle = 0.55 + 0.45 * (self.time * 6.0 + phase).sin();
                StarSprite {
                    x: rect.x + rect.width * x,
                    y: rect.y + rect.height * y,
                    size: *size,
                    brightness: (twinkle * (0.5 + air * 0.6 + self.level * 0.2)).clamp(0.0, 1.0),
                    warmth: *warmth,
                }
            })
            .collect()
    }

    /// The stars in flight, in `rect`.
    pub fn shooting(&self, rect: Rect) -> Vec<ShootingStar> {
        if rect.is_empty() {
            return Vec::new();
        }
        self.shooting
            .iter()
            .map(|star| {
                let travel = star.age * star.speed;
                let head = (
                    star.from.0 + star.heading.0 * travel,
                    star.from.1 + star.heading.1 * travel,
                );
                let tail = (
                    head.0 - star.heading.0 * 0.08,
                    head.1 - star.heading.1 * 0.08,
                );
                let at = |(x, y): (f32, f32)| {
                    (
                        rect.x + rect.width * x.clamp(0.0, 1.0),
                        rect.y + rect.height * y.clamp(0.0, 1.0),
                    )
                };
                ShootingStar {
                    from: at(tail),
                    to: at(head),
                    life: (1.0 - star.age / 0.9).clamp(0.0, 1.0),
                }
            })
            .collect()
    }

    /// The planets, drifting through `rect`.
    pub fn planets(&self, rect: Rect) -> Vec<PlanetSprite> {
        if rect.is_empty() {
            return Vec::new();
        }
        let t = self.time;
        let bass = self.groups[0];
        [
            (0.82, 0.62, 0.11, 0.35, 0.0, 0.0),
            (0.16, 0.30, 0.06, 0.0, 1.0, 2.1),
            (0.55, 0.18, 0.035, 0.22, 0.5, 4.2),
        ]
        .iter()
        .map(|(x, y, r, ring, ink, phase)| PlanetSprite {
            x: rect.x + rect.width * (x + 0.02 * (t * 0.07 + phase).sin()),
            y: rect.y + rect.height * (y + 0.03 * (t * 0.05 + phase).cos()),
            radius: rect.height * r * (1.0 + bass * 0.08),
            ring: *ring,
            lit: 0.45 + self.level * 0.5,
            ink: *ink,
        })
        .collect()
    }

    /// The aurora: the waveform as a ribbon across the lower half of
    /// `rect`, one point per column.
    pub fn aurora(&self, rect: Rect) -> Vec<(f32, f32)> {
        const COLUMNS: usize = 96;
        if rect.is_empty() {
            return Vec::new();
        }
        let base = rect.y + rect.height * 0.68;
        let swing = rect.height * 0.28;
        // Brought up to the ribbon's height whatever the instrument's level:
        // a piano at −20 dB and a supersaw at −3 are the same waveform to
        // look at, and a ribbon that only moved for the loud presets would
        // read as broken on the quiet ones. Silence is a flat line still.
        let peak = self.wave.iter().fold(0.0f32, |a, s| a.max(s.abs()));
        let gain = if peak > 1e-3 {
            (1.0 / peak).min(12.0)
        } else {
            0.0
        };
        // Inside the rectangle, including its far edges, which `contains`
        // treats as outside.
        let inside = |x: f32, y: f32| {
            (
                x.clamp(rect.x, rect.right() - 0.01),
                y.clamp(rect.y, rect.bottom() - 0.01),
            )
        };
        (0..COLUMNS)
            .map(|i| {
                let x = rect.x + rect.width * i as f32 / (COLUMNS - 1) as f32;
                let sample = if self.wave.is_empty() {
                    0.0
                } else {
                    let at = i * (self.wave.len() - 1) / (COLUMNS - 1).max(1);
                    self.wave[at.min(self.wave.len() - 1)]
                };
                inside(x, base - swing * (sample * gain).clamp(-1.0, 1.0))
            })
            .collect()
    }
}

fn mix3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    let t = t.clamp(0.0, 1.0);
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

/// A hash of an integer lattice point to 0..1.
fn hash(x: i32, y: i32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x8da6_b343) ^ (y as u32).wrapping_mul(0xd816_3841);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1_e995);
    h ^= h >> 15;
    (h & 0xffff) as f32 / 65_535.0
}

/// Smooth value noise, −1..=1.
fn noise(x: f32, y: f32) -> f32 {
    let (ix, iy) = (x.floor(), y.floor());
    let (fx, fy) = (x - ix, y - iy);
    let (ix, iy) = (ix as i32, iy as i32);
    let sx = fx * fx * (3.0 - 2.0 * fx);
    let sy = fy * fy * (3.0 - 2.0 * fy);
    let a = hash(ix, iy);
    let b = hash(ix + 1, iy);
    let c = hash(ix, iy + 1);
    let d = hash(ix + 1, iy + 1);
    let top = a + (b - a) * sx;
    let bottom = c + (d - c) * sx;
    (top + (bottom - top) * sy) * 2.0 - 1.0
}

/// Three octaves of noise, 0..1.
fn fbm(x: f32, y: f32) -> f32 {
    let mut value = 0.0;
    let mut amplitude = 0.5;
    let (mut x, mut y) = (x, y);
    for _ in 0..3 {
        value += amplitude * noise(x, y);
        x = x * 2.0 + 1.3;
        y = y * 2.0 + 0.7;
        amplitude *= 0.5;
    }
    (value * 0.5 + 0.5).clamp(0.0, 1.0)
}
