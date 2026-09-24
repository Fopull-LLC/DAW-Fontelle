//! DisgustingBeat's window, without a window
//! (`docs/disgusting-beat-plan.md` §7.6).
//!
//! Geometry and captions only: this crate may not shape text (INVARIANT 2),
//! so everything measured here is arithmetic the canvas already did. The one
//! test that can *see* is the `render_headless` case, which dumps a PNG.

use fontelle_types::{
    CurveShape, DisgustingBeatConfig, DisgustingBeatLaneKind, DisgustingBeatLength,
    DisgustingBeatLook, DisgustingBeatPoint, DisgustingBeatRate,
};
use fontelle_ui::canvas::{
    DISGUSTING_BEAT_GRAB, DISGUSTING_BEAT_MENU_ROWS, DISGUSTING_BEAT_SIZE, DisgustingBeatHit,
    DisgustingBeatMenu, DisgustingBeatSnap, DisgustingBeatTool, DisgustingBeatView,
    DisgustingBeatZoom, LaneView, disgusting_beat_hit, disgusting_beat_layout,
    disgusting_beat_menu_label, freeze_slope, grid_position, hold_points, lane_clear_rect,
    lane_length_rect, point_position, rate_caption,
};
use fontelle_ui::layout::Rect;

fn view() -> DisgustingBeatView {
    DisgustingBeatView {
        track: "Drums".to_string(),
        config: DisgustingBeatConfig::new(),
        scene: 0,
        scene_names: (0..12).map(|_| String::new()).collect(),
        scene_used: vec![false; 12],
        lanes: DisgustingBeatLaneKind::ALL
            .iter()
            .map(|kind| LaneView {
                kind: *kind,
                length: DisgustingBeatLength::Bar,
                on: kind.on_by_default(),
                points: vec![DisgustingBeatPoint::new(
                    0.0,
                    kind.neutral(),
                    CurveShape::Linear,
                )],
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
        trail: vec![0.0; 512],
        tool: DisgustingBeatTool::Points,
        snap: DisgustingBeatSnap::Sixteenth,
        zoom: 1.0,
        menu: None,
    }
}

fn body() -> Rect {
    Rect::new(
        0.0,
        0.0,
        DISGUSTING_BEAT_SIZE.0 as f32,
        DISGUSTING_BEAT_SIZE.1 as f32,
    )
}

#[test]
fn the_whole_window_fits_the_window_it_opens_at() {
    let view = view();
    let body = body();
    let layout = disgusting_beat_layout(&view, body);
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
    let layout = disgusting_beat_layout(&view, body());
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
    let layout = disgusting_beat_layout(&view, body());
    // Tone and pan are off, so they are strips.
    let open = layout.lanes[0].height;
    let closed = layout.lanes[2].height;
    assert!(closed < open / 3.0, "a closed lane is a strip: {closed}");

    let strip = layout.lanes[2];
    let hit = disgusting_beat_hit(
        &layout,
        &view,
        strip.x + strip.width / 2.0,
        strip.y + strip.height / 2.0,
    );
    assert_eq!(hit, Some(DisgustingBeatHit::LaneStrip { lane: 2 }));
}

#[test]
fn a_point_round_trips_through_the_hit_test() {
    // Pointer → (phase, value) → pixel, at three zooms and on every lane.
    for zoom in [0.25, 0.5, 1.0] {
        let mut view = view();
        view.zoom = zoom;
        let layout = disgusting_beat_layout(&view, body());
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
        DisgustingBeatPoint::new(0.0, 0.0, CurveShape::Linear),
        DisgustingBeatPoint::new(0.5, -0.5, CurveShape::Linear),
    ];
    let layout = disgusting_beat_layout(&view, body());
    let rect = layout.lanes[0];
    let (x, y) = point_position(&view, &view.lanes[0], rect, 0.5, -0.5);
    assert_eq!(
        disgusting_beat_hit(&layout, &view, x, y),
        Some(DisgustingBeatHit::Point { lane: 0, index: 1 })
    );
    // And a grab's width away, it is the grid again.
    let hit = disgusting_beat_hit(&layout, &view, x + DISGUSTING_BEAT_GRAB * 2.0, y);
    assert!(
        matches!(hit, Some(DisgustingBeatHit::Grid { lane: 0, .. })),
        "{hit:?}"
    );
}

#[test]
fn the_freeze_guide_is_forty_five_degrees_at_unit_zoom() {
    // The whole of what this window teaches: at the default reach, a hold is
    // a line you trace rather than a number you compute.
    let view = view();
    let layout = disgusting_beat_layout(&view, body());
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
    assert!((DisgustingBeatSnap::Sixteenth.snap(0.26, 4.0) - 0.25).abs() < 1e-9);
    assert!((DisgustingBeatSnap::Eighth.snap(0.3, 4.0) - 0.25).abs() < 1e-9);
    assert!((DisgustingBeatSnap::Quarter.snap(0.3, 4.0) - 0.25).abs() < 1e-9);
    // Off is where the pointer is.
    assert!((DisgustingBeatSnap::Off.snap(0.2637, 4.0) - 0.2637).abs() < 1e-9);
    // And a triplet is a third of a beat, not a quarter.
    let third = DisgustingBeatSnap::Triplet.snap(0.09, 4.0);
    assert!((third - 1.0 / 12.0).abs() < 1e-9, "{third}");
}

#[test]
fn the_time_axis_makes_room_for_the_future_only_when_something_looks_ahead() {
    let mut view = view();
    let layout = disgusting_beat_layout(&view, body());
    let rect = layout.lanes[0];

    // Off: zero is the top of the lane's **plot**, which sits below the band
    // that carries its name.
    let plot = fontelle_ui::canvas::plot_area(rect);
    let (_, y) = point_position(&view, &view.lanes[0], rect, 0.0, 0.0);
    assert!((y - plot.y).abs() < 0.5, "zero should be the top: {y}");

    // On: the zero line slides down and a positive offset has somewhere to be.
    view.config.look = DisgustingBeatLook::Beat;
    let (_, y) = point_position(&view, &view.lanes[0], rect, 0.0, 0.0);
    assert!(y > plot.y + 4.0, "and now there is room above it: {y}");
    let (_, above) = point_position(&view, &view.lanes[0], rect, 0.0, 0.2);
    assert!(above < y, "a forward offset is above the line");
}

#[test]
fn the_lane_length_and_the_rate_both_move_the_grid() {
    let mut view = view();
    assert!((view.time_beats() - 4.0).abs() < 1e-9);
    view.lanes[0].length = DisgustingBeatLength::TwoBars;
    assert!((view.time_beats() - 8.0).abs() < 1e-9);
    // Half-time is a lane twice as long, which is what the chip says.
    view.config.rate = DisgustingBeatRate::Half;
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
    for tool in DisgustingBeatTool::ALL {
        assert!(!tool.label().is_empty());
        assert!(
            tool.tip().len() > 10,
            "{tool:?} has no sentence to explain it"
        );
    }
    for snap in DisgustingBeatSnap::ALL {
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
    let layout = disgusting_beat_layout(&view, body());
    assert_eq!(layout.tools.len(), DisgustingBeatTool::ALL.len());
    assert_eq!(layout.snaps.len(), DisgustingBeatSnap::ALL.len());
    for (index, rect) in layout.tools.iter().enumerate() {
        let hit = disgusting_beat_hit(
            &layout,
            &view,
            rect.x + rect.width / 2.0,
            rect.y + rect.height / 2.0,
        );
        assert_eq!(
            hit,
            Some(DisgustingBeatHit::Tool(DisgustingBeatTool::ALL[index])),
            "tool {index} at {rect:?}"
        );
    }
    for (index, rect) in layout.snaps.iter().enumerate() {
        let hit = disgusting_beat_hit(
            &layout,
            &view,
            rect.x + rect.width / 2.0,
            rect.y + rect.height / 2.0,
        );
        assert_eq!(
            hit,
            Some(DisgustingBeatHit::Snap(DisgustingBeatSnap::ALL[index])),
            "snap {index} at {rect:?}"
        );
    }
    for (index, rect) in layout.scenes.iter().enumerate() {
        let hit = disgusting_beat_hit(
            &layout,
            &view,
            rect.x + rect.width / 2.0,
            rect.y + rect.height / 2.0,
        );
        assert_eq!(hit, Some(DisgustingBeatHit::Scene(index)), "scene {index}");
    }
}

#[test]
fn the_chips_are_where_the_window_draws_them() {
    // The numbers the driver on `:99` clicks at, frozen: the hold tool's
    // centre, and the 1/16 snap's. If the bar is ever re-laid-out these move,
    // and this test is the note that says so.
    let view = view();
    let layout = disgusting_beat_layout(&view, body());
    // In the window's *body* coordinates — the driver on `:99` clicks these
    // plus the header's own height.
    let hold = layout.tools[3];
    assert!(
        hold.contains(230.0, 129.0),
        "the hold chip has moved: {hold:?}"
    );
}

// --------------------------------------------------------------- the lane's
// own header: what it is worth, and how to take it back

#[test]
fn a_lane_header_carries_its_length_and_a_way_to_clear_it() {
    // The per-lane length is the polyrhythm — a volume lane three beats long
    // over a time lane of four — and it was in the document with no way to
    // reach it. So was clearing one. Both are chips in the lane's own header,
    // beside the name, because that is the only row of a lane that is not
    // the picture.
    let view = view();
    let layout = disgusting_beat_layout(&view, body());
    let rect = layout.lanes[0];
    let length = lane_length_rect(rect);
    let clear = lane_clear_rect(rect);
    assert_eq!(
        disgusting_beat_hit(
            &layout,
            &view,
            length.x + length.width / 2.0,
            length.y + length.height / 2.0
        ),
        Some(DisgustingBeatHit::LaneLength { lane: 0 })
    );
    assert_eq!(
        disgusting_beat_hit(
            &layout,
            &view,
            clear.x + clear.width / 2.0,
            clear.y + clear.height / 2.0
        ),
        Some(DisgustingBeatHit::LaneClear { lane: 0 })
    );
    // And the name is still the switch, which is the gesture that was there
    // first.
    assert_eq!(
        disgusting_beat_hit(&layout, &view, rect.x + 8.0, rect.y + 8.0),
        Some(DisgustingBeatHit::LaneStrip { lane: 0 })
    );
}

#[test]
fn the_header_chips_keep_out_of_the_picture() {
    // A chip that reached into the plot would eat the clicks that draw, which
    // is the one thing in this window that has to work.
    let view = view();
    let layout = disgusting_beat_layout(&view, body());
    let rect = layout.lanes[0];
    let plot = fontelle_ui::canvas::plot_area(rect);
    for (what, chip) in [
        ("the length", lane_length_rect(rect)),
        ("clear", lane_clear_rect(rect)),
    ] {
        assert!(
            chip.bottom() <= plot.y + 0.5,
            "{what} chip reaches into the plot: {chip:?} against {plot:?}"
        );
        assert!(
            chip.right() <= rect.right(),
            "{what} chip runs out of the lane"
        );
    }
}

// -------------------------------------------------------------- the reach

#[test]
fn the_zoom_chips_choose_how_far_the_lane_reaches() {
    // `set_disgusting_beat_zoom` existed, was clamped, was carried in the view and was
    // reachable from nowhere at all: a groove template lives in the first few
    // milliseconds of the lane and at reach 1 it is a flat line.
    let view = view();
    let layout = disgusting_beat_layout(&view, body());
    assert_eq!(layout.zooms.len(), DisgustingBeatZoom::ALL.len());
    for (index, rect) in layout.zooms.iter().enumerate() {
        assert_eq!(
            disgusting_beat_hit(
                &layout,
                &view,
                rect.x + rect.width / 2.0,
                rect.y + rect.height / 2.0
            ),
            Some(DisgustingBeatHit::Zoom(DisgustingBeatZoom::ALL[index])),
            "zoom chip {index} at {rect:?}"
        );
        assert!(!DisgustingBeatZoom::ALL[index].label().is_empty());
        assert!(DisgustingBeatZoom::ALL[index].tip().len() > 10);
    }
    // Magnification, not reach: "4x" is twice as big as "2x", and the lane
    // then reaches a quarter of a lane-length either way.
    assert!((DisgustingBeatZoom::One.reach() - 1.0).abs() < 1e-6);
    assert!((DisgustingBeatZoom::Four.reach() - 0.25).abs() < 1e-6);
    assert_eq!(DisgustingBeatZoom::nearest(0.26), DisgustingBeatZoom::Four);
    assert_eq!(DisgustingBeatZoom::nearest(1.0), DisgustingBeatZoom::One);
}

// ---------------------------------------------------------- the shape menu

#[test]
fn a_point_carries_a_menu_of_shapes() {
    // Six shapes have been in the document since the automation lane, every
    // preset here uses them, and a hand-drawn point could only ever be
    // linear. The menu is the whole of that hole.
    let mut view = view();
    view.menu = Some(DisgustingBeatMenu {
        lane: 0,
        index: 0,
        at: (400.0, 300.0),
    });
    let layout = disgusting_beat_layout(&view, body());
    assert_eq!(layout.menu.len(), DISGUSTING_BEAT_MENU_ROWS);
    for (row, rect) in layout.menu.iter().enumerate() {
        assert_eq!(
            disgusting_beat_hit(
                &layout,
                &view,
                rect.x + rect.width / 2.0,
                rect.y + rect.height / 2.0
            ),
            Some(DisgustingBeatHit::MenuRow(row)),
            "row {row} at {rect:?}"
        );
        assert!(!disgusting_beat_menu_label(row).is_empty());
    }
    // The last row is the one the gesture used to do on its own, so nothing
    // that worked before stopped working.
    assert_eq!(
        disgusting_beat_menu_label(DISGUSTING_BEAT_MENU_ROWS - 1),
        "remove"
    );
}

#[test]
fn the_menu_stays_inside_the_window_and_covers_what_is_under_it() {
    let mut view = view();
    let body = body();
    // Asked for at the very corner, which is where a right-click on the last
    // point of the bottom lane lands.
    view.menu = Some(DisgustingBeatMenu {
        lane: 0,
        index: 0,
        at: (body.right() - 4.0, body.bottom() - 4.0),
    });
    let layout = disgusting_beat_layout(&view, body);
    for rect in &layout.menu {
        assert!(
            rect.x >= body.x - 0.5 && rect.right() <= body.right() + 0.5,
            "a menu row runs off the side: {rect:?}"
        );
        assert!(
            rect.y >= body.y - 0.5 && rect.bottom() <= body.bottom() + 0.5,
            "a menu row runs off the bottom: {rect:?}"
        );
    }
    // And an open menu takes the press wherever it is, or the click that
    // chooses a shape would also draw a point on the lane beneath.
    let mut over_a_lane = view.clone();
    let lane = disgusting_beat_layout(&view, body).lanes[0];
    over_a_lane.menu = Some(DisgustingBeatMenu {
        lane: 0,
        index: 0,
        at: (lane.x + 40.0, lane.y + 30.0),
    });
    let layout = disgusting_beat_layout(&over_a_lane, body);
    let row = layout.menu[1];
    assert_eq!(
        disgusting_beat_hit(
            &layout,
            &over_a_lane,
            row.x + row.width / 2.0,
            row.y + row.height / 2.0
        ),
        Some(DisgustingBeatHit::MenuRow(1))
    );
}

#[test]
fn the_canopy_head_is_measured_against_the_memory_not_against_what_is_in_it() {
    // Found by looking at the dump with the trail drawn beside the head: the
    // two disagreed. The canopy draws the **whole ring** — twelve seconds of
    // columns, the unwritten ones empty on the left — so a read a quarter of
    // a second back is a quarter of a second from the right edge whatever the
    // memory happens to hold. Dividing by what has been written instead
    // spread one second of memory over the whole canopy, and in the first
    // seconds after a start the head sat halfway across a picture of nothing.
    let mut view = view();
    view.offset = -0.125; // an eighth of a lane back: 0.25 s at 120 bpm
    let quarter_second = 0.25 / f64::from(fontelle_types::DISGUSTING_BEAT_MEMORY_SECONDS);
    view.filled_seconds = 12.0;
    let full = fontelle_ui::canvas::canopy_head(&view);
    view.filled_seconds = 1.0;
    let fresh = fontelle_ui::canvas::canopy_head(&view);
    assert!(
        (f64::from(full) - (1.0 - quarter_second)).abs() < 1e-3,
        "the head is in the wrong place on a full memory: {full}"
    );
    assert!(
        (full - fresh).abs() < 1e-4,
        "how much has been written moved the head: {full} against {fresh}"
    );
    // And a read that is live sits on the right edge, which is now.
    view.offset = 0.0;
    assert!((fontelle_ui::canvas::canopy_head(&view) - 1.0).abs() < 1e-6);
}

#[test]
fn everything_you_can_press_says_what_it_does() {
    // The window teaches by picture, but a picture cannot say what "4x"
    // means. Every chip has a sentence and the canopy carries it while the
    // pointer is on it — the one written rule this window has, and it is
    // beside the thing it explains rather than in a manual.
    use fontelle_ui::canvas::disgusting_beat_tip;
    let view = view();
    for tool in DisgustingBeatTool::ALL {
        assert!(disgusting_beat_tip(&view, DisgustingBeatHit::Tool(tool)).is_some());
    }
    for snap in DisgustingBeatSnap::ALL {
        assert!(disgusting_beat_tip(&view, DisgustingBeatHit::Snap(snap)).is_some());
    }
    for zoom in DisgustingBeatZoom::ALL {
        let tip = disgusting_beat_tip(&view, DisgustingBeatHit::Zoom(zoom))
            .expect("a reach says what it reaches");
        assert!(tip.len() > 10, "{zoom:?}: {tip}");
    }
    for hit in [
        DisgustingBeatHit::LaneStrip { lane: 0 },
        DisgustingBeatHit::LaneLength { lane: 0 },
        DisgustingBeatHit::LaneClear { lane: 0 },
        DisgustingBeatHit::Scene(3),
        DisgustingBeatHit::MenuRow(0),
        DisgustingBeatHit::Canopy,
    ] {
        assert!(
            disgusting_beat_tip(&view, hit).is_some(),
            "{hit:?} explains nothing"
        );
    }
    // And the two that are the picture itself do not: a sentence that follows
    // the pointer across the grid it is drawn on is noise.
    assert!(
        disgusting_beat_tip(
            &view,
            DisgustingBeatHit::Grid {
                lane: 0,
                phase: 0.5,
                value: 0.0
            }
        )
        .is_none()
    );
    assert!(disgusting_beat_tip(&view, DisgustingBeatHit::Point { lane: 0, index: 0 }).is_none());
}

#[test]
fn every_segment_that_can_bend_carries_a_handle() {
    // > *"letting me change the bend on the curve on the lines between
    // > points."* — Ty, 2026-09-23
    //
    // The bend has been in the document since the automation lane and has had
    // a gesture since this window — a press within seven pixels of the drawn
    // line. Seven pixels of a line nobody was told about is a feature that
    // does not exist, and it also took the one press that should have added a
    // point there. So the handle is **drawn**: a grip halfway along every
    // segment that has anywhere to bend to.
    use fontelle_ui::canvas::{DisgustingBeatHit, bend_handle};
    let mut view = view();
    view.lanes[0].points = vec![
        DisgustingBeatPoint::new(0.0, 0.0, CurveShape::Linear),
        DisgustingBeatPoint::new(0.5, -0.5, CurveShape::Linear),
        DisgustingBeatPoint::new(0.75, 0.0, CurveShape::Linear),
    ];
    let layout = disgusting_beat_layout(&view, body());
    let rect = layout.lanes[0];
    for carrier in [0, 1] {
        let (x, y) = bend_handle(&view, &view.lanes[0], rect, carrier)
            .unwrap_or_else(|| panic!("segment {carrier} has nowhere to grip"));
        assert_eq!(
            disgusting_beat_hit(&layout, &view, x, y),
            Some(DisgustingBeatHit::Bend { lane: 0, carrier }),
            "the handle for segment {carrier} is not what is under it"
        );
        // It sits **on the line**, which is the only place a grip for a line
        // can be: a handle floating beside one belongs to nothing.
        let phase = (view.lanes[0].points[carrier].at + view.lanes[0].points[carrier + 1].at) / 2.0;
        let value = fontelle_types::curve_at(&view.lanes[0].points, phase, 0.0);
        let (cx, cy) = point_position(&view, &view.lanes[0], rect, phase, value);
        assert!(
            (x - cx).abs() < 0.5 && (y - cy).abs() < 0.5,
            "segment {carrier}'s handle is off its own line"
        );
    }
}

#[test]
fn a_segment_with_nothing_to_bend_has_no_handle() {
    // Three of them. A **flat** segment is the same value whatever the
    // tension is, so the gesture would dirty the document and change nothing
    // (found on `:99`, where every fresh lane is flat). A **stepped** one
    // ignores where it is going. And the stretch **past the last point** is
    // not a segment at all — the lane holds there until it comes round again,
    // and there is nothing between two points to bend.
    use fontelle_ui::canvas::bend_handle;
    let mut view = view();
    view.lanes[0].points = vec![
        DisgustingBeatPoint::new(0.0, -0.25, CurveShape::Linear),
        DisgustingBeatPoint::new(0.25, -0.25, CurveShape::Linear),
        DisgustingBeatPoint::new(0.5, 0.0, CurveShape::Stepped),
        DisgustingBeatPoint::new(0.75, -0.5, CurveShape::Linear),
    ];
    let layout = disgusting_beat_layout(&view, body());
    let rect = layout.lanes[0];
    assert!(
        bend_handle(&view, &view.lanes[0], rect, 0).is_none(),
        "flat"
    );
    assert!(
        bend_handle(&view, &view.lanes[0], rect, 2).is_none(),
        "stepped"
    );
    assert!(
        bend_handle(&view, &view.lanes[0], rect, 3).is_none(),
        "the tail is not a segment"
    );
    assert!(
        bend_handle(&view, &view.lanes[0], rect, 1).is_some(),
        "and the one real segment does have one"
    );
}

#[test]
fn a_vertical_is_a_pair_of_handles_a_grab_apart() {
    // Two points at one phase, which is how the lane spells an instant. The
    // hit test has to tell them apart or only one of them could ever be
    // moved, and the one it cannot move is the one somebody just drew.
    let mut view = view();
    view.lanes[1].points = vec![
        DisgustingBeatPoint::new(0.0, 0.0, CurveShape::Linear),
        DisgustingBeatPoint::new(0.5, 0.0, CurveShape::Linear),
        DisgustingBeatPoint::new(0.5, 1.0, CurveShape::Linear),
    ];
    let layout = disgusting_beat_layout(&view, body());
    let rect = layout.lanes[1];
    for (index, value) in [(1usize, 0.0), (2, 1.0)] {
        let (x, y) = point_position(&view, &view.lanes[1], rect, 0.5, value);
        assert_eq!(
            disgusting_beat_hit(&layout, &view, x, y),
            Some(DisgustingBeatHit::Point { lane: 1, index }),
            "the point at {value} could not be picked up"
        );
    }
}

#[test]
fn a_drag_follows_the_point_it_grabbed_past_its_neighbours() {
    // The lane re-sorts after every edit, so the index a drag started with
    // names a **different point** the moment the one it is holding crosses
    // another. Before this, dragging a point past its neighbour handed the
    // drag the neighbour and the two of them walked off together.
    use fontelle_ui::canvas::grabbed_point;
    let mut view = view();
    view.lanes[0].points = vec![
        DisgustingBeatPoint::new(0.0, 0.0, CurveShape::Linear),
        DisgustingBeatPoint::new(0.25, -0.2, CurveShape::Linear),
        DisgustingBeatPoint::new(0.5, -0.6, CurveShape::Linear),
    ];
    // The drag started on index 1 and has just put it at 0.6 — past the one
    // that was after it, so the lane now holds it at index 2.
    view.lanes[0].points = vec![
        DisgustingBeatPoint::new(0.0, 0.0, CurveShape::Linear),
        DisgustingBeatPoint::new(0.5, -0.6, CurveShape::Linear),
        DisgustingBeatPoint::new(0.6, -0.2, CurveShape::Linear),
    ];
    assert_eq!(grabbed_point(&view.lanes[0], (0.6, -0.2), 1), Some(2));
    // And when nothing has moved, the index it was given stands.
    assert_eq!(grabbed_point(&view.lanes[0], (0.5, -0.6), 1), Some(1));
    // A point that is not there at all is **nothing**, not a guess: the edit
    // was refused, and moving whichever point sits at that index instead is a
    // curve changing under a hand that did not ask for it.
    assert_eq!(grabbed_point(&view.lanes[0], (0.9, -0.9), 1), None);
}

#[test]
fn the_readout_says_what_the_pointer_is_over_in_the_lanes_own_words() {
    // The console says what the machine is doing *now*; this says what the
    // thing under the hand is worth, which is the number somebody drawing
    // needs. On the time lane that is a **speed**, because the slope is the
    // sound and a slope of −0.5 means nothing to anybody.
    use fontelle_ui::canvas::disgusting_beat_readout;
    let mut view = view();
    view.lanes[0].points = vec![
        DisgustingBeatPoint::new(0.0, 0.0, CurveShape::Linear),
        DisgustingBeatPoint::new(0.5, -0.25, CurveShape::Linear),
    ];
    let over_point = disgusting_beat_readout(&view, DisgustingBeatHit::Point { lane: 0, index: 1 })
        .expect("a point says where it is");
    assert!(
        over_point.contains("beat"),
        "a point on a lane is at a place in the bar: {over_point}"
    );
    // A quarter of a lane-length dropped over half a lane is a slope of −0.5,
    // so a rate of a half — an octave down, and the number somebody drawing
    // is actually aiming at.
    let over_segment = disgusting_beat_readout(
        &view,
        DisgustingBeatHit::Bend {
            lane: 0,
            carrier: 0,
        },
    )
    .expect("a segment says what it plays at");
    assert!(
        over_segment.contains("12.0 st"),
        "half speed is an octave down: {over_segment}"
    );
    // And the volume lane says an amplitude, not a pitch.
    let volume = disgusting_beat_readout(&view, DisgustingBeatHit::Point { lane: 1, index: 0 })
        .expect("a volume point says how loud");
    assert!(!volume.contains("st"), "{volume}");
}

#[test]
fn the_shape_menu_can_split_a_point_into_a_vertical() {
    // A pair at one phase is drawn by clicking the grid above or below a
    // point that is already there, which works and which nobody would guess.
    // **Split** is the same thing said out loud: the menu a point already
    // carries names it, so the instant is a thing you can find rather than a
    // thing you have to be told.
    use fontelle_ui::canvas::{DISGUSTING_BEAT_MENU_ROWS, disgusting_beat_menu_splits};
    let rows: Vec<&str> = (0..DISGUSTING_BEAT_MENU_ROWS)
        .map(disgusting_beat_menu_label)
        .collect();
    assert!(rows.contains(&"split"), "{rows:?}");
    let split = rows.iter().position(|row| *row == "split").unwrap();
    assert!(disgusting_beat_menu_splits(split));
    assert!(fontelle_ui::canvas::disgusting_beat_menu_shape(split).is_none());
    // Remove stays last: right-click removed a point before this menu
    // existed, and the hand that learned that must not lose it.
    assert_eq!(rows[DISGUSTING_BEAT_MENU_ROWS - 1], "remove");
}

#[test]
fn the_split_row_is_greyed_where_a_vertical_would_have_nowhere_to_go() {
    // A vertical at the very start or the very end of a lane is half a
    // vertical: nothing arrives at phase 0 and nothing leaves phase 1, so the
    // lane keeps whichever half sounds and the other is dropped. A menu row
    // that quietly does nothing is worse than one that is plainly off, so it
    // says so.
    use fontelle_ui::canvas::{disgusting_beat_menu_enabled, disgusting_beat_menu_splits};
    let mut view = view();
    view.lanes[0].points = vec![
        DisgustingBeatPoint::new(0.0, 0.0, CurveShape::Linear),
        DisgustingBeatPoint::new(0.5, -0.5, CurveShape::Linear),
        DisgustingBeatPoint::new(1.0, -0.6, CurveShape::Linear),
    ];
    let split = (0..DISGUSTING_BEAT_MENU_ROWS)
        .find(|row| disgusting_beat_menu_splits(*row))
        .expect("the menu has a split row");
    let on = |index: usize| DisgustingBeatMenu {
        lane: 0,
        index,
        at: (0.0, 0.0),
    };
    assert!(!disgusting_beat_menu_enabled(&view, on(0), split), "at 0");
    assert!(
        disgusting_beat_menu_enabled(&view, on(1), split),
        "half way"
    );
    assert!(!disgusting_beat_menu_enabled(&view, on(2), split), "at 1");
    // Every other row is always live.
    for row in 0..DISGUSTING_BEAT_MENU_ROWS {
        if !disgusting_beat_menu_splits(row) {
            assert!(disgusting_beat_menu_enabled(&view, on(0), row), "row {row}");
        }
    }
}

#[test]
fn a_drawing_tool_draws_where_the_points_tool_would_grab() {
    // Found on `:99`, driving the **hold** tool's own gesture: a tape stop
    // starts at the top left of the lane, which is exactly where a flat
    // lane's one point sits. The press grabbed that point instead of
    // starting the freeze, the drag moved it, and the lane came out flat at
    // the bottom with one handle on it.
    //
    // A point is grabbed before the grid under it *with the points tool* —
    // or a point could never be picked up off its own line. With a tool that
    // **draws**, the lane is a canvas and nothing on it is a handle, which is
    // what every drawing program in the world does with a pencil.
    let mut view = view();
    view.lanes[0].points = vec![DisgustingBeatPoint::new(0.0, 0.0, CurveShape::Linear)];
    let layout = disgusting_beat_layout(&view, body());
    let rect = layout.lanes[0];
    let (x, y) = point_position(&view, &view.lanes[0], rect, 0.0, 0.0);
    let (x, y) = (x + 3.0, y + 2.0);

    view.tool = DisgustingBeatTool::Points;
    assert_eq!(
        disgusting_beat_hit(&layout, &view, x, y),
        Some(DisgustingBeatHit::Point { lane: 0, index: 0 })
    );
    for tool in [
        DisgustingBeatTool::Pencil,
        DisgustingBeatTool::Line,
        DisgustingBeatTool::Hold,
        DisgustingBeatTool::Step,
    ] {
        view.tool = tool;
        assert!(
            matches!(
                disgusting_beat_hit(&layout, &view, x, y),
                Some(DisgustingBeatHit::Grid { lane: 0, .. })
            ),
            "{tool:?} grabbed a handle instead of drawing"
        );
    }
}

#[test]
fn a_split_puts_its_twin_where_the_lane_can_show_it() {
    // The twin has to land somewhere the hand can see and grab, which is not
    // the same as somewhere inside the lane's range: the **time** lane's
    // range is a lane-length either way and what is on the screen is
    // whatever the reach chooses, so a quarter of the range is off the top
    // at 8x — and above the zero line it is a read of the future, which the
    // machine clamps away.
    use fontelle_ui::canvas::split_twin;
    let mut view = view();
    view.lanes[0].points = vec![
        DisgustingBeatPoint::new(0.0, 0.0, CurveShape::Linear),
        DisgustingBeatPoint::new(0.25, -0.05, CurveShape::Linear),
        DisgustingBeatPoint::new(0.75, -0.95, CurveShape::Linear),
    ];
    // Near the top of what is shown, the twin goes **down**; near the bottom,
    // up. Both land on the grid.
    let high = split_twin(&view, &view.lanes[0], 1).expect("a point can split");
    let low = split_twin(&view, &view.lanes[0], 2).expect("and so can this one");
    assert!(high < -0.05, "near the zero line it goes down: {high}");
    assert!(low > -0.95, "and near the floor it goes up: {low}");
    for twin in [high, low] {
        assert!(
            (-1.0..=0.0).contains(&twin),
            "{twin} is off the grid at this reach"
        );
    }
    // And at eight times the reach it stays inside the eighth of a
    // lane-length the lane is showing.
    view.zoom = 0.125;
    view.lanes[0].points = vec![
        DisgustingBeatPoint::new(0.0, 0.0, CurveShape::Linear),
        DisgustingBeatPoint::new(0.5, -0.06, CurveShape::Linear),
    ];
    let twin = split_twin(&view, &view.lanes[0], 1).expect("a point can split");
    assert!(
        (-0.125..=0.0).contains(&twin),
        "{twin} is off the magnified grid"
    );
    // The volume lane is its own range, top to bottom, whatever the time
    // lane's reach is.
    view.lanes[1].points = vec![
        DisgustingBeatPoint::new(0.0, 1.0, CurveShape::Linear),
        DisgustingBeatPoint::new(0.5, 0.4, CurveShape::Linear),
    ];
    let twin = split_twin(&view, &view.lanes[1], 1).expect("volume splits too");
    assert!((0.0..=1.0).contains(&twin), "{twin}");
    assert_eq!(
        split_twin(&view, &view.lanes[1], 0),
        None,
        "and a point at the lane's own edge has nowhere to put one"
    );
}
