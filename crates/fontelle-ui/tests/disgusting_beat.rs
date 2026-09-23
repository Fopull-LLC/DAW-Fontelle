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
