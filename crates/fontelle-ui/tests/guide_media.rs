//! The guide's animations: decoding an animated PNG and playing it.
//!
//! `docs/ux-routing-and-learning-plan.md` step 7. Each clip is stored as an
//! APNG whose frames hold only what changed, so playing one means laying
//! each frame over the last — that composition is what these pin down, on a
//! tiny file made here, and then that every shipped clip decodes into
//! something worth watching.

use fontelle_ui::canvas::GuideMedia;
use fontelle_ui::guide_media::{MEDIA_HEIGHT, MEDIA_WIDTH, Player, bytes, decode_apng};

const RED: [u8; 4] = [255, 0, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];
const HALF_GREEN: [u8; 4] = [0, 255, 0, 128];

struct Frame {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    fill: [u8; 4],
    over: bool,
    delay_ms: u16,
}

/// A 4×4 APNG: a red frame, then the given frames on top of it.
fn apng(frames: &[Frame]) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, 4, 4);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_animated(frames.len() as u32 + 1, 0).unwrap();
        let mut w = enc.write_header().unwrap();
        w.set_frame_delay(100, 1000).unwrap();
        w.write_image_data(&RED.repeat(16)).unwrap();
        for f in frames {
            w.set_frame_dimension(f.w, f.h).unwrap();
            w.set_frame_position(f.x, f.y).unwrap();
            w.set_frame_delay(f.delay_ms, 1000).unwrap();
            w.set_blend_op(if f.over {
                png::BlendOp::Over
            } else {
                png::BlendOp::Source
            })
            .unwrap();
            w.set_dispose_op(png::DisposeOp::None).unwrap();
            w.write_image_data(&f.fill.repeat((f.w * f.h) as usize))
                .unwrap();
        }
        w.finish().unwrap();
    }
    out
}

fn pixel(rgba: &[u8], x: usize, y: usize) -> [u8; 4] {
    let i = (y * 4 + x) * 4;
    rgba[i..i + 4].try_into().unwrap()
}

#[test]
fn a_frame_that_holds_a_corner_changes_only_that_corner() {
    let file = apng(&[Frame {
        x: 2,
        y: 2,
        w: 2,
        h: 2,
        fill: BLUE,
        over: false,
        delay_ms: 100,
    }]);
    let anim = decode_apng(&file).expect("decodes");
    assert_eq!((anim.width, anim.height), (4, 4));
    assert_eq!(anim.frames.len(), 2);
    let mut player = Player::new(anim);
    player.advance_to(0);
    assert_eq!(pixel(player.rgba(), 3, 3), RED);
    player.advance_to(150);
    assert_eq!(pixel(player.rgba(), 3, 3), BLUE);
    assert_eq!(pixel(player.rgba(), 0, 0), RED, "the rest is kept");
}

#[test]
fn an_over_frame_blends_and_a_source_frame_replaces() {
    let file = apng(&[Frame {
        x: 0,
        y: 0,
        w: 4,
        h: 1,
        fill: HALF_GREEN,
        over: true,
        delay_ms: 100,
    }]);
    let mut player = Player::new(decode_apng(&file).unwrap());
    player.advance_to(100);
    let p = pixel(player.rgba(), 0, 0);
    assert!(
        (120..=135).contains(&p[0]) && (120..=135).contains(&p[1]),
        "{p:?}"
    );
    assert_eq!(p[3], 255);
}

#[test]
fn it_loops_and_starts_again_from_the_first_frame() {
    let file = apng(&[
        Frame {
            x: 0,
            y: 0,
            w: 1,
            h: 1,
            fill: BLUE,
            over: false,
            delay_ms: 100,
        },
        Frame {
            x: 1,
            y: 0,
            w: 1,
            h: 1,
            fill: BLUE,
            over: false,
            delay_ms: 300,
        },
    ]);
    let anim = decode_apng(&file).unwrap();
    assert_eq!(anim.total_ms(), 500);
    assert_eq!(anim.frame_at(0), 0);
    assert_eq!(anim.frame_at(99), 0);
    assert_eq!(anim.frame_at(100), 1);
    assert_eq!(anim.frame_at(200), 2);
    assert_eq!(anim.frame_at(499), 2);
    assert_eq!(anim.frame_at(500), 0, "round again");
    let mut player = Player::new(anim);
    player.advance_to(450);
    assert_eq!(pixel(player.rgba(), 1, 0), BLUE);
    player.advance_to(520);
    assert_eq!(pixel(player.rgba(), 1, 0), RED, "the loop starts clean");
    assert_eq!(pixel(player.rgba(), 0, 0), RED);
}

#[test]
fn advancing_says_whether_the_picture_changed() {
    let file = apng(&[Frame {
        x: 0,
        y: 0,
        w: 1,
        h: 1,
        fill: BLUE,
        over: false,
        delay_ms: 100,
    }]);
    let mut player = Player::new(decode_apng(&file).unwrap());
    assert!(player.advance_to(0), "the first picture is a change");
    assert!(!player.advance_to(50));
    assert!(player.advance_to(120));
    assert!(!player.advance_to(150));
}

#[test]
fn a_file_that_is_not_an_animation_is_refused() {
    assert!(decode_apng(b"not a png").is_err());
}

#[test]
fn every_clip_decodes_into_something_worth_watching() {
    for media in GuideMedia::ALL {
        let file = bytes(*media);
        assert!(
            file.len() < 2 * 1024 * 1024,
            "{media:?} is {} bytes",
            file.len()
        );
        let anim = decode_apng(file).unwrap_or_else(|e| panic!("{media:?}: {e}"));
        assert_eq!(
            (anim.width, anim.height),
            (MEDIA_WIDTH, MEDIA_HEIGHT),
            "{media:?}"
        );
        assert!(anim.frames.len() >= 20, "{media:?} barely moves");
        let total = anim.total_ms();
        assert!(
            (2_500..=16_000).contains(&total),
            "{media:?} runs {total} ms"
        );
    }
}

#[test]
fn a_frame_with_no_delay_is_laid_with_the_next_and_takes_no_time() {
    // The encoder stores two far-apart changes (the timer and the playhead)
    // as two frames, the first with no delay: one moment of the clip.
    let file = apng(&[
        Frame {
            x: 0,
            y: 0,
            w: 1,
            h: 1,
            fill: BLUE,
            over: false,
            delay_ms: 0,
        },
        Frame {
            x: 3,
            y: 3,
            w: 1,
            h: 1,
            fill: BLUE,
            over: false,
            delay_ms: 100,
        },
    ]);
    let anim = decode_apng(&file).unwrap();
    assert_eq!(anim.total_ms(), 200);
    let mut player = Player::new(anim);
    player.advance_to(100);
    assert_eq!(pixel(player.rgba(), 0, 0), BLUE);
    assert_eq!(pixel(player.rgba(), 3, 3), BLUE);
}
