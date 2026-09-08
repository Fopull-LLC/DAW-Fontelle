//! Bending a fade: the node on a clip's fade curve (TDD §15.2).
//!
//! *"you can bend the control node to bend the curve like fl studios too."*
//!
//! FL's fade has a handle at its midpoint, and dragging it up or down bends
//! the curve between the two ends without moving them. Here that is a
//! **tension**, −1..1, kept on the fade beside its length and its shape:
//! zero is the shape as it was, positive holds the gain back before letting
//! it rise (a slower start), negative brings it up early. The ends never
//! move — a fade still starts at silence and ends at full — which is what
//! keeps a bend a bend and not a different length.
//!
//! The player, the block on the arrangement and the editor's waveform all
//! read the same [`Fade::at`], so what is drawn is what is heard.

use fontelle_types::{Fade, FadeCurve, tension_for_midpoint};

fn bent(tension: f32) -> Fade {
    Fade {
        frames: 1000,
        curve: FadeCurve::Linear,
        tension,
    }
}

#[test]
fn with_no_tension_a_fade_is_its_curve() {
    for curve in FadeCurve::ALL {
        let fade = Fade {
            frames: 1000,
            curve,
            tension: 0.0,
        };
        for i in 0..=20 {
            let t = i as f64 / 20.0;
            assert!(
                (fade.at(t) - curve.at(t)).abs() < 1e-9,
                "{curve:?} at {t} bent with no tension"
            );
        }
    }
}

#[test]
fn a_fade_saved_before_tension_existed_reads_back_unbent() {
    let json = r#"{"frames":480,"curve":"Linear"}"#;
    let fade: Fade = serde_json::from_str(json).expect("an old fade still reads");
    assert_eq!(fade.frames, 480);
    assert_eq!(fade.tension, 0.0);
}

#[test]
fn the_ends_never_move() {
    for tension in [-1.0, -0.5, 0.0, 0.3, 1.0] {
        let fade = bent(tension);
        assert!(
            (fade.at(0.0)).abs() < 1e-9,
            "tension {tension} starts above silence"
        );
        assert!(
            (fade.at(1.0) - 1.0).abs() < 1e-9,
            "tension {tension} ends short of full"
        );
    }
}

#[test]
fn positive_tension_holds_the_fade_back_and_negative_brings_it_forward() {
    let slow = bent(0.7).at(0.5);
    let plain = bent(0.0).at(0.5);
    let fast = bent(-0.7).at(0.5);
    assert!(slow < plain, "{slow} is not below {plain}");
    assert!(fast > plain, "{fast} is not above {plain}");
    assert!((plain - 0.5).abs() < 1e-9);
}

#[test]
fn a_bent_fade_still_only_rises() {
    for tension in [-1.0, -0.4, 0.4, 1.0] {
        let fade = bent(tension);
        let mut last = -1.0;
        for i in 0..=100 {
            let t = i as f64 / 100.0;
            let g = fade.at(t);
            assert!(g >= last, "tension {tension} fell at {t}");
            assert!((0.0..=1.0).contains(&g));
            last = g;
        }
    }
}

#[test]
fn the_bend_is_what_the_node_asks_for() {
    // Dragging the node to a height is asking for that gain at the midpoint;
    // `tension_for_midpoint` is the inverse, so the node lands under the
    // pointer rather than near it.
    for wanted in [0.1f64, 0.25, 0.5, 0.7, 0.8] {
        let tension = tension_for_midpoint(wanted as f32);
        let got = bent(tension).at(0.5);
        assert!(
            (got - wanted).abs() < 1e-3,
            "asked for {wanted}, got {got} (tension {tension})"
        );
    }
}

#[test]
fn tension_is_kept_within_its_range() {
    // A node dragged to the very top or bottom asks for more than a bend
    // can give; the answer is the most it can, not a curve that breaks.
    assert!((-1.0..=1.0).contains(&tension_for_midpoint(0.0)));
    assert!((-1.0..=1.0).contains(&tension_for_midpoint(1.0)));
    assert!((-1.0..=1.0).contains(&tension_for_midpoint(0.999)));
    assert_eq!(tension_for_midpoint(0.5), 0.0);
    // And a tension written by hand outside it is read as the edge.
    let fade = Fade {
        frames: 10,
        curve: FadeCurve::Linear,
        tension: 5.0,
    };
    assert!((fade.at(0.5) - bent(1.0).at(0.5)).abs() < 1e-9);
}

#[test]
fn the_players_gain_goes_through_the_bend() {
    // `AudioClipData::fade_gain` is what the audio thread reads; a bend
    // that reached the picture and not the sound would be a lie.
    use fontelle_types::{AssetId, AssetKind, AssetRef, AudioClipData};
    let asset = AssetRef {
        id: AssetId::default(),
        path: "take.wav".into(),
        content_hash: 0,
        size: 0,
        kind: AssetKind::Sample,
    };
    let mut clip = AudioClipData::whole(asset, 1000, 48_000);
    clip.fade_in = bent(0.8);
    let plain = 0.25f32; // 250 of 1000, linear
    assert!(clip.fade_gain(250.0) < plain);
    clip.fade_in = bent(-0.8);
    assert!(clip.fade_gain(250.0) > plain);
}
