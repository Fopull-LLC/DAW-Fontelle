//! How the mixer is looked at: how wide a strip is (Ctrl+wheel, like the
//! arrangement), how tall the patch bay is (a handle along its top), where a
//! send's wire leaves from, and where a wire picked up off its jack lands.
//!
//! > *"please also make it so you can change the horizontal scale of the
//! > mixer by ctrl zooming similar to the arrangement zooming. currently
//! > youre only stuck to one size. i also cant control how big the wiring
//! > section is at the bottom ... instead just make it show at a default
//! > size and then give it a handle where it can be dragged up or down"*

use fontelle_ui::cables::{CableKey, CableRole, KNOB_RADIUS};
use fontelle_ui::canvas::{
    MAX_STRIP_WIDTH, MIN_PATCH_HEIGHT, MIN_STRIP_WIDTH, MixerHit, MixerView, PATCH_HEIGHT,
    STRIP_WIDTH, StripRoute, cable_drop_target, mixer_cables, mixer_hit, mixer_layout_view,
    mixer_zoomed, patch_height_at,
};
use fontelle_ui::document::{MixerStrip, SendInfo};
use fontelle_ui::layout::Rect;
use fontelle_ui::theme::Theme;

fn strip(name: &str) -> MixerStrip {
    MixerStrip {
        name: name.to_string(),
        gain_db: 0.0,
        pan: 0.0,
        mute: false,
        solo: false,
        is_master: false,
        color: [0x4f, 0x8f, 0xd0, 0xff],
        inserts: Vec::new(),
        sends: Vec::new(),
    }
}

/// `count` ordinary tracks and a master, last.
fn strips(count: usize) -> Vec<MixerStrip> {
    let mut out: Vec<MixerStrip> = (0..count)
        .map(|i| strip(&format!("Track {}", i + 1)))
        .collect();
    out.push(MixerStrip {
        is_master: true,
        ..strip("Master")
    });
    out
}

fn body() -> Rect {
    Rect::new(10.0, 40.0, 900.0, 400.0)
}

fn metrics() -> fontelle_ui::theme::Metrics {
    Theme::dark_default().metrics
}

fn view(strip_width: f32, patch_height: f32) -> MixerView {
    MixerView {
        strip_width,
        patch_height,
    }
}

#[test]
fn the_default_view_is_the_old_mixer() {
    assert_eq!(
        MixerView::default(),
        view(STRIP_WIDTH, PATCH_HEIGHT),
        "nobody who never zooms sees anything move"
    );
}

#[test]
fn a_wider_strip_is_wider_everywhere_it_is_measured() {
    let strips = strips(4);
    for width in [MIN_STRIP_WIDTH, STRIP_WIDTH, 120.0] {
        let l = mixer_layout_view(
            body(),
            &metrics(),
            &strips,
            0,
            None,
            view(width, PATCH_HEIGHT),
        );
        let master = l.master.as_ref().unwrap();
        assert!((master.frame.width - width).abs() < 1e-3, "the master too");
        for s in &l.strips {
            assert!(
                (s.frame.width - width).abs() < 1e-3,
                "{width}: {:?}",
                s.frame
            );
            let jacks = l.jacks(s.index).unwrap();
            assert!(
                jacks.input[0] > s.frame.x && jacks.output[0] < s.frame.right(),
                "{width}: the jacks stay under their strip"
            );
        }
        // A strip's neighbour starts one strip and a gap along.
        let step = l.strips[1].frame.x - l.strips[0].frame.x;
        assert!(step > width && step < width + 8.0, "{width}: step {step}");
    }
}

#[test]
fn ctrl_wheel_zooms_by_a_factor_and_stops_at_the_ends() {
    assert!(mixer_zoomed(STRIP_WIDTH, 1.15) > STRIP_WIDTH);
    assert!(mixer_zoomed(STRIP_WIDTH, 0.8) < STRIP_WIDTH);
    assert_eq!(mixer_zoomed(STRIP_WIDTH, 100.0), MAX_STRIP_WIDTH);
    assert_eq!(mixer_zoomed(STRIP_WIDTH, 0.01), MIN_STRIP_WIDTH);
    // In and out again is where it started: a zoom with drift is one you
    // cannot get back from by feel.
    let there_and_back = mixer_zoomed(mixer_zoomed(STRIP_WIDTH, 1.15), 1.0 / 1.15);
    assert!((there_and_back - STRIP_WIDTH).abs() < 1e-3);
}

#[test]
fn narrow_strips_fit_more_of_them() {
    let strips = strips(30);
    let narrow = mixer_layout_view(
        body(),
        &metrics(),
        &strips,
        0,
        None,
        view(MIN_STRIP_WIDTH, PATCH_HEIGHT),
    );
    let wide = mixer_layout_view(
        body(),
        &metrics(),
        &strips,
        0,
        None,
        view(MAX_STRIP_WIDTH, PATCH_HEIGHT),
    );
    assert!(narrow.strips.len() > wide.strips.len() * 2);
}

#[test]
fn the_bay_is_the_height_it_is_given() {
    let strips = strips(4);
    for height in [MIN_PATCH_HEIGHT, PATCH_HEIGHT, 160.0] {
        let l = mixer_layout_view(body(), &metrics(), &strips, 0, None, view(76.0, height));
        assert!(
            (l.patch.height - height).abs() < 1e-3,
            "{height}: {:?}",
            l.patch
        );
        assert!((l.patch.bottom() - body().bottom()).abs() < 1e-3);
        for s in &l.strips {
            assert!(s.frame.bottom() <= l.patch.y + 1e-3);
        }
    }
}

#[test]
fn a_short_panel_keeps_a_smaller_bay_rather_than_none() {
    // It used to vanish below 266 pixels, which is what made it feel
    // auto-sized: there one moment, gone the next. Now it gives way to the
    // strips by shrinking, and the strips keep a usable height.
    let strips = strips(4);
    let short = Rect::new(10.0, 40.0, 900.0, 200.0);
    let l = mixer_layout_view(short, &metrics(), &strips, 0, None, MixerView::default());
    assert!(!l.patch.is_empty(), "still there");
    assert!(l.patch.height >= MIN_PATCH_HEIGHT);
    assert!(
        l.strips[0].frame.height >= 120.0,
        "the faders still come first: {:?}",
        l.strips[0].frame
    );
}

#[test]
fn the_bay_has_a_handle_along_its_top() {
    let strips = strips(4);
    let l = mixer_layout_view(body(), &metrics(), &strips, 0, None, MixerView::default());
    assert!(!l.seam.is_empty());
    assert!(
        l.seam.y <= l.patch.y && l.seam.bottom() >= l.patch.y,
        "on the bay's top edge: {:?} {:?}",
        l.seam,
        l.patch
    );
    assert!(l.seam.x <= l.patch.x + 1e-3 && l.seam.right() >= l.patch.right() - 1e-3);
    let (x, y) = (l.seam.x + 40.0, l.seam.y + l.seam.height / 2.0);
    assert_eq!(mixer_hit(&l, x, y), MixerHit::BaySeam);
}

#[test]
fn dragging_the_handle_sets_the_height_and_cannot_lose_either_side() {
    let b = body();
    let at = |y| patch_height_at(b, y);
    assert!(
        (at(b.bottom() - 120.0) - 120.0).abs() < 1e-3,
        "follows the pointer"
    );
    assert_eq!(
        at(b.bottom() + 50.0),
        MIN_PATCH_HEIGHT,
        "never dragged away"
    );
    assert!(
        b.height - at(b.y - 50.0) >= 120.0,
        "and never over the faders"
    );
}

#[test]
fn a_send_leaves_from_its_own_jack_with_its_knob_on_it() {
    // *"the knob for selecting how much is being sent through the wire is
    // kind of odd how its just placed directly in the middle of it, instead
    // make the knob be on the origin point that the wire is coming from."*
    // Each send has its own origin under the output jack, so two sends from
    // one track are two knobs rather than one on top of the other.
    let mut strips = strips(3);
    for target in [1, 2] {
        strips[0].sends.push(SendInfo {
            target,
            target_name: String::new(),
            level_db: -6.0,
            pre_fader: false,
        });
    }
    let routes: Vec<StripRoute<usize>> = (0..4)
        .map(|id| StripRoute {
            id,
            output: None,
            output_on: true,
        })
        .collect();
    let l = mixer_layout_view(body(), &metrics(), &strips, 0, None, MixerView::default());
    let cables = mixer_cables(&l, &strips, &routes, 0);
    let from = |role| {
        cables
            .iter()
            .find(|c| c.key == CableKey { track: 0, role })
            .unwrap()
            .from
    };
    let out = from(CableRole::Output);
    let (one, two) = (from(CableRole::Send(0)), from(CableRole::Send(1)));
    assert_eq!(Some(one), l.send_jack(0, 0));
    assert_eq!(Some(two), l.send_jack(0, 1));
    for jack in [one, two] {
        assert!(l.patch.contains(jack[0], jack[1]), "in the bay: {jack:?}");
        assert!(
            jack[1] + KNOB_RADIUS <= l.patch.bottom(),
            "a whole knob: {jack:?}"
        );
    }
    assert!(
        (one[0] - two[0]).hypot(one[1] - two[1]) >= 2.0 * KNOB_RADIUS,
        "two sends, two knobs: {one:?} {two:?}"
    );
    assert!(
        (one[0] - out[0]).hypot(one[1] - out[1]) >= 2.0 * KNOB_RADIUS,
        "and neither on the output's plug"
    );
}

#[test]
fn a_wire_lands_on_whichever_strip_it_is_let_go_over() {
    let strips = strips(4);
    let l = mixer_layout_view(body(), &metrics(), &strips, 0, None, MixerView::default());
    let master = l.master.as_ref().unwrap();
    let over = |s: &fontelle_ui::canvas::MixerStripLayout| {
        cable_drop_target(&l, s.frame.x + s.frame.width / 2.0, l.patch.y + 10.0)
    };
    assert_eq!(over(&l.strips[2]), Some(l.strips[2].index));
    assert_eq!(over(master), Some(master.index), "the master takes wires");
    // Over the strip itself, not only its jacks: a drop is aimed at a track.
    let s = &l.strips[1];
    assert_eq!(
        cable_drop_target(&l, s.frame.x + 5.0, s.frame.y + 40.0),
        Some(s.index)
    );
    assert_eq!(
        cable_drop_target(&l, l.options_frame().x + 10.0, l.patch.y + 10.0),
        None,
        "the options column is not a track"
    );
    assert_eq!(
        cable_drop_target(&l, l.strips[0].frame.x, body().bottom() + 30.0),
        None,
        "off the panel is nowhere"
    );
}
