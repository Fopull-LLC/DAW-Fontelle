//! The bridge's canopy: a galaxy that listens.
//!
//! > *"the background of the starry galaxy should be sort of like a shader
//! > that is an audio visualizer that simulates galaxies and stars and
//! > clusters of planets ... as you play stuff the nebulas and stars and
//! > galaxies will shift in color and shape and orientation to make a totally
//! > unique pattern visualizing that waveform in a surreal and trippy
//! > manner."* — Ty, 2026-09-15
//!
//! The sky is a state machine fed the instrument's own sound once a frame
//! ([`SkyState::tick`]) and a picture drawn from that state ([`SkyState::
//! render`], a CPU-shaded nebula the renderer scales up; the stars, planets
//! and aurora are vector sprites over it). What these tests hold is that it
//! **listens**: silence is still a sky, sound makes it brighter and turns
//! it, bass and treble colour it differently, attacks throw stars across it,
//! and the aurora is the waveform. And that it is a pure function of its
//! state — the same state draws the same sky, so a frame never flickers on
//! its own.

use fontelle_ui::layout::Rect;
use fontelle_ui::sky::{SkyPalette, SkySound, SkyState};

fn palette() -> SkyPalette {
    SkyPalette::for_theme(&fontelle_ui::theme::Theme::dark_default().palette)
}

/// A sound: `bands` in dBFS across the analyser's bands, low to high, and a
/// waveform.
fn sound(low_db: f32, high_db: f32, wave_amp: f32) -> SkySound {
    let bands = 96;
    SkySound {
        bands_db: (0..bands)
            .map(|i| low_db + (high_db - low_db) * i as f32 / (bands - 1) as f32)
            .collect(),
        wave: (0..512)
            .map(|i| (i as f32 / 512.0 * std::f32::consts::TAU * 4.0).sin() * wave_amp)
            .collect(),
    }
}

fn silence() -> SkySound {
    sound(-120.0, -120.0, 0.0)
}

fn mean_brightness(rgba: &[u8]) -> f32 {
    let sum: u64 = rgba
        .as_chunks::<4>()
        .0
        .iter()
        .map(|px| u64::from(px[0]) + u64::from(px[1]) + u64::from(px[2]))
        .sum();
    sum as f32 / (rgba.len() / 4) as f32 / 3.0
}

/// Red minus blue, on average: the sign says which way the palette leans.
fn warmth(rgba: &[u8]) -> f32 {
    let (mut r, mut b) = (0i64, 0i64);
    for px in rgba.as_chunks::<4>().0 {
        r += i64::from(px[0]);
        b += i64::from(px[2]);
    }
    (r - b) as f32 / (rgba.len() / 4) as f32
}

#[test]
fn silence_is_still_a_sky() {
    let mut sky = SkyState::new(7);
    for _ in 0..30 {
        sky.tick(&silence(), 1.0 / 30.0);
    }
    let image = sky.render(240, 60, &palette());
    assert_eq!(image.width, 240);
    assert_eq!(image.height, 60);
    assert_eq!(image.rgba.len(), 240 * 60 * 4);
    assert!(
        image.rgba.as_chunks::<4>().0.iter().all(|px| px[3] == 255),
        "opaque"
    );
    let brightness = mean_brightness(&image.rgba);
    assert!(
        brightness > 4.0 && brightness < 90.0,
        "a quiet sky is dim but not black: {brightness}"
    );
    // And it has stars in it, whatever the sound.
    assert!(!sky.stars(Rect::new(0.0, 0.0, 1180.0, 160.0)).is_empty());
}

#[test]
fn sound_makes_the_sky_brighter_and_turns_it() {
    let mut quiet = SkyState::new(7);
    let mut loud = SkyState::new(7);
    for _ in 0..60 {
        quiet.tick(&silence(), 1.0 / 30.0);
        loud.tick(&sound(-12.0, -24.0, 0.5), 1.0 / 30.0);
    }
    let a = quiet.render(240, 60, &palette());
    let b = loud.render(240, 60, &palette());
    assert!(
        mean_brightness(&b.rgba) > mean_brightness(&a.rgba) * 1.3,
        "sound should light the nebula: {} against {}",
        mean_brightness(&b.rgba),
        mean_brightness(&a.rgba)
    );
    // The galaxy turns with the energy: the same two seconds spin it
    // further when there is sound in them.
    assert!(
        loud.spin() > quiet.spin() + 0.1,
        "{} vs {}",
        loud.spin(),
        quiet.spin()
    );
}

#[test]
fn bass_and_treble_colour_the_sky_differently() {
    let mut bass = SkyState::new(7);
    let mut treble = SkyState::new(7);
    for _ in 0..60 {
        bass.tick(&sound(-6.0, -60.0, 0.5), 1.0 / 30.0);
        treble.tick(&sound(-60.0, -6.0, 0.5), 1.0 / 30.0);
    }
    let a = bass.render(240, 60, &palette());
    let b = treble.render(240, 60, &palette());
    assert!(
        (warmth(&a.rgba) - warmth(&b.rgba)).abs() > 6.0,
        "bass and treble should lean the palette apart: {} against {}",
        warmth(&a.rgba),
        warmth(&b.rgba)
    );
}

#[test]
fn an_attack_throws_stars_across_the_sky_and_they_fade() {
    let mut sky = SkyState::new(3);
    for _ in 0..30 {
        sky.tick(&silence(), 1.0 / 30.0);
    }
    assert!(sky.shooting(Rect::new(0.0, 0.0, 1000.0, 200.0)).is_empty());
    // Silence to a hit: a transient.
    sky.tick(&sound(-6.0, -12.0, 0.8), 1.0 / 30.0);
    sky.tick(&sound(-6.0, -12.0, 0.8), 1.0 / 30.0);
    let thrown = sky.shooting(Rect::new(0.0, 0.0, 1000.0, 200.0));
    assert!(!thrown.is_empty(), "a transient should throw a star");
    for _ in 0..120 {
        sky.tick(&silence(), 1.0 / 30.0);
    }
    assert!(
        sky.shooting(Rect::new(0.0, 0.0, 1000.0, 200.0)).is_empty(),
        "and it should be gone four seconds later"
    );
    // A steady sound is not a stream of transients.
    let mut steady = SkyState::new(3);
    for _ in 0..90 {
        steady.tick(&sound(-6.0, -12.0, 0.8), 1.0 / 30.0);
    }
    let before = steady.thrown_so_far();
    for _ in 0..90 {
        steady.tick(&sound(-6.0, -12.0, 0.8), 1.0 / 30.0);
    }
    assert!(
        steady.thrown_so_far() - before <= 2,
        "a held note should not keep throwing stars: {} more",
        steady.thrown_so_far() - before
    );
}

#[test]
fn the_aurora_is_the_waveform() {
    let rect = Rect::new(0.0, 0.0, 600.0, 150.0);
    let mut flat = SkyState::new(1);
    let mut wavy = SkyState::new(1);
    for _ in 0..10 {
        flat.tick(&sound(-30.0, -30.0, 0.0), 1.0 / 30.0);
        wavy.tick(&sound(-30.0, -30.0, 0.9), 1.0 / 30.0);
    }
    let spread = |points: &[(f32, f32)]| {
        let top = points.iter().map(|p| p.1).fold(f32::MAX, f32::min);
        let bottom = points.iter().map(|p| p.1).fold(f32::MIN, f32::max);
        bottom - top
    };
    let a = flat.aurora(rect);
    let b = wavy.aurora(rect);
    assert!(a.len() >= 64 && b.len() == a.len());
    assert!(
        spread(&b) > spread(&a) + 20.0,
        "the ribbon should swing with the wave: {} against {}",
        spread(&b),
        spread(&a)
    );
    assert!(
        b.iter().all(|(x, y)| rect.contains(*x, *y)),
        "and stay in the window"
    );
}

#[test]
fn the_same_state_draws_the_same_sky() {
    let mut sky = SkyState::new(11);
    for _ in 0..20 {
        sky.tick(&sound(-20.0, -30.0, 0.3), 1.0 / 30.0);
    }
    let a = sky.render(120, 40, &palette());
    let b = sky.render(120, 40, &palette());
    assert_eq!(a.rgba, b.rgba, "rendering is a pure function of the state");
    let stars_a = sky.stars(Rect::new(0.0, 0.0, 500.0, 100.0));
    let stars_b = sky.stars(Rect::new(0.0, 0.0, 500.0, 100.0));
    assert_eq!(stars_a.len(), stars_b.len());
}

/// The sky keeps moving while a note is held — the nebula drifts — and
/// settles when nothing has sounded for a while, so the window can sleep.
#[test]
fn the_sky_says_when_it_is_alive() {
    let mut sky = SkyState::new(5);
    for _ in 0..30 {
        sky.tick(&silence(), 1.0 / 30.0);
    }
    assert!(
        !sky.is_alive(),
        "a sky that has heard nothing for a second is at rest"
    );
    sky.tick(&sound(-12.0, -20.0, 0.5), 1.0 / 30.0);
    assert!(sky.is_alive());
    for _ in 0..30 {
        sky.tick(&silence(), 1.0 / 30.0);
    }
    assert!(
        sky.is_alive(),
        "and it stays alive for a while after the sound stops"
    );
    for _ in 0..300 {
        sky.tick(&silence(), 1.0 / 30.0);
    }
    assert!(!sky.is_alive());
}
