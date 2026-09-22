//! Lapse's window, without a window (`docs/lapse-plan.md` §7.6).
//!
//! Geometry and captions only: this crate may not shape text (INVARIANT 2),
//! so everything measured here is arithmetic the canvas already did. The one
//! test that can *see* is the `render_headless` case, which dumps a PNG.

use fontelle_types::{
    CurveShape, LapseConfig, LapseLaneKind, LapseLength, LapseLook, LapsePoint, LapseRate,
};
use fontelle_ui::canvas::{
    LAPSE_GRAB, LAPSE_SIZE, LaneView, LapseHit, LapseSnap, LapseTool, LapseView, freeze_slope,
    grid_position, hold_points, lapse_hit, lapse_layout, point_position, rate_caption,
};
use fontelle_ui::layout::Rect;

fn view() -> LapseView {
    LapseView {
        track: "Drums".to_string(),
        config: LapseConfig::new(),
        scene: 0,
        scene_names: (0..12).map(|_| String::new()).collect(),
        scene_used: vec![false; 12],
        lanes: LapseLaneKind::ALL
            .iter()
            .map(|kind| LaneView {
                kind: *kind,
                length: LapseLength::Bar,
                on: kind.on_by_default(),
                points: vec![LapsePoint::new(0.0, kind.neutral(), CurveShape::Linear)],
                open: kind.on_by_default(),
            })
            .collect(),
        phase: 0.25,
        offset: 0.0,
        rate: 1.0,
        clamped: false,
        filled_seconds: 4.0,
        memory: vec![(0.5, 0.3); 512],
        beats_per_bar: 4,
        bpm: 120.0,
        tool: LapseTool::Points,
        snap: LapseSnap::Sixteenth,
        zoom: 1.0,
    }
}

fn body() -> Rect {
    Rect::new(0.0, 0.0, LAPSE_SIZE.0 as f32, LAPSE_SIZE.1 as f32)
}

#[test]
fn the_whole_window_fits_the_window_it_opens_at() {
    let view = view();
    let body = body();
    let layout = lapse_layout(&view, body);
    let inside = |r: Rect, what: &str| {
        assert!(
            r.x >= body.x - 0.5
                && r.y >= body.y - 0.5
                && r.right() <= body.right() + 0.5
                && r.bottom() <= body.bottom() + 0.5,
            "{what} is outside the body: {r:?}"
        );
    };
    inside(layout.canopy, "the canopy");
    inside(layout.console, "the console");
    for (index, lane) in layout.lanes.iter().enumerate() {
        inside(*lane, &format!("lane {index}"));
    }
    for (index, scene) in layout.scenes.iter().enumerate() {
        inside(*scene, &format!("scene chip {index}"));
    }
    for (index, tool) in layout.tools.iter().enumerate() {
        inside(*tool, &format!("tool {index}"));
    }
    for (index, snap) in layout.snaps.iter().enumerate() {
        inside(*snap, &format!("snap {index}"));
    }
    // And nothing overlaps the console, which is what a lane running off the
    // bottom would look like.
    for lane in &layout.lanes {
        assert!(
            lane.bottom() <= layout.console.y + 0.5,
            "a lane runs into the console: {lane:?}"
        );
    }
}

#[test]
fn the_lanes_do_not_overlap_the_aside() {
    let view = view();
    let layout = lapse_layout(&view, body());
    for lane in &layout.lanes {
        for chip in &layout.scenes {
            assert!(
                lane.right() <= chip.x + 0.5,
                "a lane runs under the scene chips: {lane:?} against {chip:?}"
            );
        }
    }
}

#[test]
fn a_collapsed_lane_takes_one_row_and_still_hit_tests() {
    let view = view();
    let layout = lapse_layout(&view, body());
    // Tone and pan are off, so they are strips.
    let open = layout.lanes[0].height;
    let closed = layout.lanes[2].height;
    assert!(closed < open / 3.0, "a closed lane is a strip: {closed}");

    let strip = layout.lanes[2];
    let hit = lapse_hit(
        &layout,
        &view,
        strip.x + strip.width / 2.0,
        strip.y + strip.height / 2.0,
    );
    assert_eq!(hit, Some(LapseHit::LaneStrip { lane: 2 }));
}

#[test]
fn a_point_round_trips_through_the_hit_test() {
    // Pointer → (phase, value) → pixel, at three zooms and on every lane.
    for zoom in [0.25, 0.5, 1.0] {
        let mut view = view();
        view.zoom = zoom;
        let layout = lapse_layout(&view, body());
        for (index, lane) in view.lanes.iter().enumerate() {
            if !lane.open {
                continue;
            }
            let rect = layout.lanes[index];
            for (fx, fy) in [(0.25, 0.3), (0.5, 0.5), (0.8, 0.75)] {
                let (x, y) = (rect.x + rect.width * fx, rect.y + rect.height * fy);
                let (phase, value) = grid_position(&view, lane, rect, x, y);
                let (back_x, back_y) = point_position(&view, lane, rect, phase, value);
                assert!(
                    (back_x - x).abs() < 0.5 && (back_y - y).abs() < 0.5,
                    "lane {index} at zoom {zoom} lost ({x}, {y}) -> ({back_x}, {back_y})"
                );
            }
        }
    }
}

#[test]
fn a_point_is_grabbed_before_the_grid_under_it() {
    // Or a point could never be picked up off its own line.
    let mut view = view();
    view.lanes[0].points = vec![
        LapsePoint::new(0.0, 0.0, CurveShape::Linear),
        LapsePoint::new(0.5, -0.5, CurveShape::Linear),
    ];
    let layout = lapse_layout(&view, body());
    let rect = layout.lanes[0];
    let (x, y) = point_position(&view, &view.lanes[0], rect, 0.5, -0.5);
    assert_eq!(
        lapse_hit(&layout, &view, x, y),
        Some(LapseHit::Point { lane: 0, index: 1 })
    );
    // And a grab's width away, it is the grid again.
    let hit = lapse_hit(&layout, &view, x + LAPSE_GRAB * 2.0, y);
    assert!(
        matches!(hit, Some(LapseHit::Grid { lane: 0, .. })),
        "{hit:?}"
    );
}

#[test]
fn the_freeze_guide_is_forty_five_degrees_at_unit_zoom() {
    // The whole of what this window teaches: at the default reach, a hold is
    // a line you trace rather than a number you compute.
    let view = view();
    let layout = lapse_layout(&view, body());
    let rect = fontelle_ui::canvas::plot_area(layout.lanes[0]);
    let slope = freeze_slope(&view, layout.lanes[0]);
    // "45°" in the grid's own units: one lane across is one lane-length down.
    // It is a true 45° only on a square grid, and a lane is much wider than
    // it is tall — what matters, and what this measures, is that the guide
    // *is* the freeze slope.
    assert!(
        (slope * rect.width / rect.height - 1.0).abs() < 1e-3,
        "the guide is not the freeze: {slope}"
    );

    // And magnified to a quarter of the reach it is four times as steep,
    // because the axis is — which is what makes a groove template's
    // millisecond offsets something a hand can draw.
    let mut close = view.clone();
    close.zoom = 0.25;
    let steep = freeze_slope(&close, layout.lanes[0]);
    assert!(
        (steep - slope * 4.0).abs() < 1e-3,
        "{steep} against {slope}"
    );
}

#[test]
fn a_hold_drag_lays_down_the_freeze() {
    // Half a lane of drag falls half a lane-length: the slope that stops the
    // sound, whatever the lane's length is.
    let points = hold_points((0.25, 0.0), (0.75, -0.9));
    assert_eq!(points[0].at, 0.25);
    assert_eq!(points[0].value, 0.0);
    assert_eq!(points[1].at, 0.75);
    assert!(
        (points[1].value + 0.5).abs() < 1e-9,
        "the freeze ignores where the pointer went vertically: {}",
        points[1].value
    );
    // Dragging right to left is the same gesture.
    let backwards = hold_points((0.75, 0.0), (0.25, 0.0));
    assert_eq!(backwards[0].at, 0.25);
}

#[test]
fn the_rate_readout_says_what_the_slope_is_worth() {
    assert_eq!(rate_caption(1.0), "+0.0 st");
    assert_eq!(rate_caption(0.5), "-12.0 st");
    assert_eq!(rate_caption(2.0), "+12.0 st");
    // A freeze is not an infinitely deep pitch and a reverse is not a pitch
    // at all: taking the log of either would put "-inf st" on the face of a
    // musical instrument.
    assert_eq!(rate_caption(0.0), "frozen");
    assert!(rate_caption(-1.0).starts_with("reverse"));
}

#[test]
fn the_snap_lands_on_the_division_it_names() {
    // A sixteenth of a four-beat lane is a sixteenth of the lane.
    assert!((LapseSnap::Sixteenth.snap(0.26, 4.0) - 0.25).abs() < 1e-9);
    assert!((LapseSnap::Eighth.snap(0.3, 4.0) - 0.25).abs() < 1e-9);
    assert!((LapseSnap::Quarter.snap(0.3, 4.0) - 0.25).abs() < 1e-9);
    // Off is where the pointer is.
    assert!((LapseSnap::Off.snap(0.2637, 4.0) - 0.2637).abs() < 1e-9);
    // And a triplet is a third of a beat, not a quarter.
    let third = LapseSnap::Triplet.snap(0.09, 4.0);
    assert!((third - 1.0 / 12.0).abs() < 1e-9, "{third}");
}

#[test]
fn the_time_axis_makes_room_for_the_future_only_when_something_looks_ahead() {
    let mut view = view();
    let layout = lapse_layout(&view, body());
    let rect = layout.lanes[0];

    // Off: zero is the top of the lane's **plot**, which sits below the band
    // that carries its name.
    let plot = fontelle_ui::canvas::plot_area(rect);
    let (_, y) = point_position(&view, &view.lanes[0], rect, 0.0, 0.0);
    assert!((y - plot.y).abs() < 0.5, "zero should be the top: {y}");

    // On: the zero line slides down and a positive offset has somewhere to be.
    view.config.look = LapseLook::Beat;
    let (_, y) = point_position(&view, &view.lanes[0], rect, 0.0, 0.0);
    assert!(y > plot.y + 4.0, "and now there is room above it: {y}");
    let (_, above) = point_position(&view, &view.lanes[0], rect, 0.0, 0.2);
    assert!(above < y, "a forward offset is above the line");
}

#[test]
fn the_lane_length_and_the_rate_both_move_the_grid() {
    let mut view = view();
    assert!((view.time_beats() - 4.0).abs() < 1e-9);
    view.lanes[0].length = LapseLength::TwoBars;
    assert!((view.time_beats() - 8.0).abs() < 1e-9);
    // Half-time is a lane twice as long, which is what the chip says.
    view.config.rate = LapseRate::Half;
    assert!((view.time_beats() - 16.0).abs() < 1e-9);
}

#[test]
fn a_scene_chip_says_its_name_or_its_number() {
    let mut view = view();
    assert_eq!(view.scene_label(0), "1", "counted from one, as people do");
    view.scene_names[3] = "Baby scratch".to_string();
    assert_eq!(view.scene_label(3), "Baby scratch");
}

#[test]
fn every_tool_and_snap_has_a_word_and_a_sentence() {
    for tool in LapseTool::ALL {
        assert!(!tool.label().is_empty());
        assert!(
            tool.tip().len() > 10,
            "{tool:?} has no sentence to explain it"
        );
    }
    for snap in LapseSnap::ALL {
        assert!(!snap.label().is_empty());
    }
}

#[test]
fn every_chip_hit_tests_at_its_own_centre() {
    // The tool bar and the snap row are the two things a gesture *starts*
    // with, and a chip that does not answer at its own middle is a chip
    // nobody can press. Written after an afternoon on `:99` where the hold
    // tool would not light.
    let view = view();
    let layout = lapse_layout(&view, body());
    assert_eq!(layout.tools.len(), LapseTool::ALL.len());
    assert_eq!(layout.snaps.len(), LapseSnap::ALL.len());
    for (index, rect) in layout.tools.iter().enumerate() {
        let hit = lapse_hit(
            &layout,
            &view,
            rect.x + rect.width / 2.0,
            rect.y + rect.height / 2.0,
        );
        assert_eq!(
            hit,
            Some(LapseHit::Tool(LapseTool::ALL[index])),
            "tool {index} at {rect:?}"
        );
    }
    for (index, rect) in layout.snaps.iter().enumerate() {
        let hit = lapse_hit(
            &layout,
            &view,
            rect.x + rect.width / 2.0,
            rect.y + rect.height / 2.0,
        );
        assert_eq!(
            hit,
            Some(LapseHit::Snap(LapseSnap::ALL[index])),
            "snap {index} at {rect:?}"
        );
    }
    for (index, rect) in layout.scenes.iter().enumerate() {
        let hit = lapse_hit(
            &layout,
            &view,
            rect.x + rect.width / 2.0,
            rect.y + rect.height / 2.0,
        );
        assert_eq!(hit, Some(LapseHit::Scene(index)), "scene {index}");
    }
}

#[test]
fn the_chips_are_where_the_window_draws_them() {
    // The numbers the driver on `:99` clicks at, frozen: the hold tool's
    // centre, and the 1/16 snap's. If the bar is ever re-laid-out these move,
    // and this test is the note that says so.
    let view = view();
    let layout = lapse_layout(&view, body());
    // In the window's *body* coordinates — the driver on `:99` clicks these
    // plus the header's own height.
    let hold = layout.tools[3];
    assert!(
        hold.contains(230.0, 129.0),
        "the hold chip has moved: {hold:?}"
    );
}
