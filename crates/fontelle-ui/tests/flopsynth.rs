//! Flopsynth's window: the geometry (`docs/flopsynth-plan.md` §8).
//!
//! > *"we need to ensure that this has a really polished, clean, and pretty
//! > user interface. it should be organized, have actual design, flow cleanly
//! > for the users eyes while also having tons of complexity to allow for
//! > massive configuration."*
//!
//! # What a synthesiser's window is
//!
//! §8.1's first rule: **the layout is the signal path**, left to right and top
//! to bottom. Sources on the top row, filters and the amp envelope under them,
//! modulators under those. Somebody who has never seen it reads it in the
//! order the sound is made — which the generic knob grid cannot do, because it
//! draws a list.
//!
//! Everything here is **pure geometry and pure arithmetic**, and that is the
//! point: what is drawn is decided by functions a test can call, so the
//! drawing has nothing left to get wrong but colours (§8.1 rule 10).

use fontelle_ui::canvas::{
    ADD_EFFECT, CANOPY_MAX, CANOPY_MIN, CARD_GAP, CARD_HEADER, CELL_FLOOR, EnvNode, FLOP_CELL_H,
    FLOP_CELL_W, FlopsynthCard, FlopsynthHit, FlopsynthPage, FlopsynthPicture, FlopsynthRoute,
    FlopsynthView, InstrumentGroup, InstrumentParam, MatrixHit, ParamKind, PresetBrowse,
    PresetChoice, PresetShelf, PresetsHit, RING_GAP, badge_at, cell_span, env_curve_points,
    env_node_at, env_node_drag, filter_xy_at, flop_knob_rect, flopsynth_hit, flopsynth_layout,
    flopsynth_tab_at, lfo_curve_points, matrix_depth_at, matrix_hit, preset_page_rows,
    preset_shelves, presets_hit, ring_depth, ring_hit, wave_position_at,
};
use fontelle_ui::layout::Rect;
use fontelle_ui::theme::Theme;

/// The window §8.8 sizes Flopsynth at, minus the chrome around the panel.
const BODY: Rect = Rect::new(0.0, 0.0, 1180.0, 700.0);
/// And the smallest it is allowed to get.
const SMALL: Rect = Rect::new(0.0, 0.0, 980.0, 580.0);

fn knob(address: &str, label: &str, value: f32) -> InstrumentParam {
    InstrumentParam {
        address: fontelle_types::ParamAddress::new(address),
        label: label.to_string(),
        value,
        display: format!("{value:.2}"),
        kind: ParamKind::Knob,
        automated: false,
    }
}

/// A view shaped like the one the app builds: one group per card, in the order
/// §8.3 draws them.
fn a_view() -> FlopsynthView {
    let osc = |name: &str, table: &str| {
        InstrumentGroup {
            name: name.to_string(),
            params: vec![
                knob("patch/layer[0]/synth/position", "pos", 0.3),
                knob("patch/layer[0]/synth/warp", "amount", 0.0),
                knob("patch/layer[0]/gain", "level", 0.6),
                knob("patch/layer[0]/pan", "pan", 0.5),
            ],
        }
        .tap(table)
    };
    FlopsynthView {
        title: "Choir Ahh".to_string(),
        cards: vec![
            FlopsynthCard {
                oscillator: None,
                group: osc("OSC A", "Saw"),
                row: 0,
                aside: false,
                columns: 0,
                removable: false,
                picture: FlopsynthPicture::Wave {
                    points: (0..128).map(|i| (i as f32 / 128.0).sin()).collect(),
                    position: 0.3,
                },
            },
            FlopsynthCard {
                oscillator: None,
                group: osc("OSC B", "Analog Morph"),
                row: 0,
                aside: false,
                columns: 0,
                removable: false,
                picture: FlopsynthPicture::Wave {
                    points: vec![0.0; 128],
                    position: 0.0,
                },
            },
            FlopsynthCard {
                oscillator: None,
                group: osc("OSC C", "Sawstack"),
                row: 0,
                aside: false,
                columns: 0,
                removable: false,
                picture: FlopsynthPicture::None,
            },
            FlopsynthCard {
                oscillator: None,
                group: InstrumentGroup {
                    name: "FILTER 1".to_string(),
                    params: vec![
                        knob("patch/filter[0]/cutoff", "cutoff", 0.8),
                        knob("patch/filter[0]/resonance", "res", 0.2),
                    ],
                },
                row: 1,
                aside: false,
                columns: 0,
                removable: false,
                picture: FlopsynthPicture::Response {
                    points: vec![0.0; 96],
                    cutoff: 0.8,
                    resonance: 0.2,
                },
            },
            FlopsynthCard {
                oscillator: None,
                group: InstrumentGroup {
                    name: "ENV 1 \u{b7} amp".to_string(),
                    params: vec![
                        knob("patch/env[0]/attack", "attack", 0.1),
                        knob("patch/env[0]/decay", "decay", 0.3),
                        knob("patch/env[0]/sustain", "sustain", 0.8),
                        knob("patch/env[0]/release", "release", 0.4),
                    ],
                },
                row: 1,
                aside: false,
                columns: 0,
                removable: false,
                picture: FlopsynthPicture::Envelope {
                    attack: 0.1,
                    decay: 0.3,
                    sustain: 0.8,
                    release: 0.4,
                },
            },
            FlopsynthCard {
                oscillator: None,
                group: InstrumentGroup {
                    name: "LFO 1".to_string(),
                    params: vec![knob("patch/lfo[0]/rate", "rate", 0.5)],
                },
                row: 2,
                aside: false,
                columns: 0,
                removable: false,
                picture: FlopsynthPicture::Lfo {
                    points: vec![0.0; 64],
                    phase: 0.25,
                },
            },
        ],
        page: FlopsynthPage::Synth,
        sources: Vec::new(),
        routes: Vec::new(),
        voices: 0,
        ..FlopsynthView::default()
    }
}

/// A tiny helper so the fixture above reads as a sentence — the table's name
/// rides on the card's heading, which is what the window draws.
trait Tap {
    fn tap(self, table: &str) -> Self;
}
impl Tap for InstrumentGroup {
    fn tap(mut self, table: &str) -> Self {
        self.name = format!("{} \u{b7} {table}", self.name);
        self
    }
}

fn metrics() -> fontelle_ui::theme::Metrics {
    Theme::dark_default().metrics
}

#[test]
fn every_card_gets_a_header_a_picture_and_a_cell_for_every_control() {
    for body in [BODY, SMALL] {
        let view = a_view();
        let layout = flopsynth_layout(body, &metrics(), &view);
        assert_eq!(
            layout.cards.len(),
            view.cards.len(),
            "every card is laid out at {}x{}",
            body.width,
            body.height
        );
        for (index, card) in layout.cards.iter().enumerate() {
            assert!(!card.frame.is_empty(), "card {index} has no frame");
            assert!(
                (card.header.height - CARD_HEADER).abs() < 0.01,
                "card {index}'s header is {}",
                card.header.height
            );
            assert_eq!(
                card.cells.len(),
                view.cards[index].group.params.len(),
                "card {index} lost a control"
            );
            for (_, cell) in &card.cells {
                assert!(
                    card.frame.contains(cell.x + 1.0, cell.y + 1.0),
                    "a control of card {index} is outside its own card"
                );
            }
            // A card with a picture gets somewhere to draw it; one without
            // gets the space back for its controls.
            match view.cards[index].picture {
                FlopsynthPicture::None => assert!(card.picture.is_empty()),
                _ => assert!(!card.picture.is_empty(), "card {index} has no picture"),
            }
        }
    }
}

/// §8.1 rule 1: the layout **is** the signal path. Sources across the top,
/// then filters and envelopes, then modulators — so somebody reads it in the
/// order the sound is made.
#[test]
fn the_cards_run_left_to_right_and_top_to_bottom() {
    let view = a_view();
    let layout = flopsynth_layout(BODY, &metrics(), &view);
    let frames: Vec<Rect> = layout.cards.iter().map(|c| c.frame).collect();

    // The three oscillators share a row, in order.
    assert!(
        (frames[0].y - frames[1].y).abs() < 0.01 && (frames[1].y - frames[2].y).abs() < 0.01,
        "the three oscillators are one row"
    );
    assert!(frames[0].x < frames[1].x && frames[1].x < frames[2].x);
    // And what comes after them is below them.
    assert!(
        frames[3].y >= frames[0].bottom(),
        "the filter is under the oscillators, not beside them"
    );
}

#[test]
fn nothing_overlaps_and_nothing_leaves_the_body() {
    for body in [BODY, SMALL] {
        let layout = flopsynth_layout(body, &metrics(), &a_view());
        let mut rects: Vec<Rect> = Vec::new();
        for card in &layout.cards {
            assert!(
                card.frame.x >= body.x - 0.01
                    && card.frame.right() <= body.right() + 0.01
                    && card.frame.y >= body.y - 0.01,
                "a card at {}x{} is outside the panel: {:?}",
                body.width,
                body.height,
                card.frame
            );
            rects.push(card.frame);
        }
        for (i, a) in rects.iter().enumerate() {
            for b in &rects[i + 1..] {
                let overlaps = a.x < b.right() - 0.01
                    && b.x < a.right() - 0.01
                    && a.y < b.bottom() - 0.01
                    && b.y < a.bottom() - 0.01;
                assert!(!overlaps, "two cards overlap: {a:?} and {b:?}");
            }
        }
    }
}

#[test]
fn the_cards_are_parted_by_air_rather_than_touching() {
    let layout = flopsynth_layout(BODY, &metrics(), &a_view());
    let a = layout.cards[0].frame;
    let b = layout.cards[1].frame;
    assert!(
        (b.x - a.right() - CARD_GAP).abs() < 0.51,
        "cards sit {} apart and the gap is {CARD_GAP}",
        b.x - a.right()
    );
}

#[test]
fn a_hit_names_the_control_under_it() {
    let view = a_view();
    let layout = flopsynth_layout(BODY, &metrics(), &view);
    for (card_index, card) in layout.cards.iter().enumerate() {
        for (param, cell) in &card.cells {
            let hit = flopsynth_hit(
                &layout,
                cell.x + cell.width * 0.5,
                cell.y + cell.height * 0.5,
            );
            assert_eq!(
                hit,
                Some(FlopsynthHit::Control {
                    card: card_index,
                    param: *param
                }),
                "the middle of a cell has to be that control"
            );
        }
        // And the picture is its own target — dragging it is a different
        // gesture from turning a knob (§8.7).
        if !card.picture.is_empty() {
            let hit = flopsynth_hit(
                &layout,
                card.picture.x + card.picture.width * 0.5,
                card.picture.y + card.picture.height * 0.5,
            );
            assert_eq!(hit, Some(FlopsynthHit::Picture { card: card_index }));
        }
    }
    // Empty air is nothing.
    assert_eq!(flopsynth_hit(&layout, -50.0, -50.0), None);
}

/// §8.7: dragging across a wave picture sets the position, and the two edges
/// are the two ends of the table.
#[test]
fn dragging_the_wave_picture_maps_its_width_onto_the_position() {
    let picture = Rect::new(100.0, 40.0, 200.0, 60.0);
    assert!((wave_position_at(picture, 100.0) - 0.0).abs() < 1e-4);
    assert!((wave_position_at(picture, 300.0) - 1.0).abs() < 1e-4);
    assert!((wave_position_at(picture, 200.0) - 0.5).abs() < 1e-4);
    // Past either end is that end, not a value off the table.
    assert_eq!(wave_position_at(picture, -500.0), 0.0);
    assert_eq!(wave_position_at(picture, 5_000.0), 1.0);
}

/// And dragging on a filter's response moves the cutoff on x and the
/// resonance on y — the EQ handle's gesture, which this window borrows rather
/// than inventing a second one.
#[test]
fn dragging_the_filter_response_moves_cutoff_and_resonance() {
    let picture = Rect::new(0.0, 0.0, 200.0, 100.0);
    let (low, quiet) = filter_xy_at(picture, 0.0, 100.0);
    let (high, loud) = filter_xy_at(picture, 200.0, 0.0);
    assert!(
        (low - 0.0).abs() < 1e-4,
        "the left edge is the bottom cutoff"
    );
    assert!((high - 1.0).abs() < 1e-4, "and the right edge the top");
    assert!(
        quiet < loud,
        "up is more resonance: {quiet} at the bottom against {loud} at the top"
    );
    assert!((quiet - 0.0).abs() < 1e-4 && (loud - 1.0).abs() < 1e-4);
}

/// §8.3: the envelope is drawn on a **square-root time axis**, so a 5 ms
/// attack and a 2 s release are both visible. On a linear axis the attack is
/// less than a pixel.
#[test]
fn the_envelope_curve_starts_at_nothing_peaks_holds_and_falls() {
    let rect = Rect::new(0.0, 0.0, 200.0, 80.0);
    let points = env_curve_points(rect, 0.1, 0.3, 0.6, 0.4);
    assert!(points.len() >= 4, "a curve needs points");
    for (x, y) in &points {
        assert!(
            *x >= rect.x - 0.01 && *x <= rect.right() + 0.01,
            "a point is outside the picture"
        );
        assert!(*y >= rect.y - 0.01 && *y <= rect.bottom() + 0.01);
    }
    // Starts at the floor, reaches the top, ends at the floor. `y` grows
    // downward, so the top is the smallest.
    assert!((points[0].1 - rect.bottom()).abs() < 0.51, "starts silent");
    let peak = points.iter().map(|(_, y)| *y).fold(f32::MAX, f32::min);
    assert!((peak - rect.y).abs() < 0.51, "the attack reaches full");
    assert!(
        (points.last().unwrap().1 - rect.bottom()).abs() < 0.51,
        "and the release ends silent"
    );

    // The square-root axis: a very short attack still has width to see.
    let quick = env_curve_points(rect, 0.005, 0.3, 0.6, 0.4);
    let attack_ends = quick
        .iter()
        .position(|(_, y)| (*y - rect.y).abs() < 0.51)
        .expect("the attack reaches the top");
    assert!(
        quick[attack_ends].0 - rect.x > 2.0,
        "a 5 ms attack has to be more than a pixel wide, and it is {}",
        quick[attack_ends].0 - rect.x
    );
}

#[test]
fn the_lfo_cycle_is_drawn_from_the_shape_it_plays() {
    let rect = Rect::new(10.0, 20.0, 120.0, 40.0);
    let points = lfo_curve_points(rect, fontelle_types::LfoWave::Sine, 0.0);
    assert!(points.len() >= 32);
    for (x, y) in &points {
        assert!(*x >= rect.x - 0.01 && *x <= rect.right() + 0.01);
        assert!(*y >= rect.y - 0.01 && *y <= rect.bottom() + 0.01);
    }
    // A sine crosses the middle at the start and at the halfway point, and
    // a square does not — the picture is the shape, not a decoration.
    let middle = rect.y + rect.height * 0.5;
    assert!((points[0].1 - middle).abs() < 1.0, "a sine starts at rest");
    let square = lfo_curve_points(rect, fontelle_types::LfoWave::Square, 0.0);
    assert!(
        (square[1].1 - middle).abs() > rect.height * 0.3,
        "a square is at one end or the other, never at rest"
    );
}

/// The window does not scroll (§8.1 rule 7): what does not fit is on another
/// page. So a body too small for the cards has to *shrink them* rather than
/// run off the bottom.
#[test]
fn a_small_body_loses_air_and_then_picture_rather_than_overflowing() {
    let view = a_view();
    let big = flopsynth_layout(BODY, &metrics(), &view);
    let small = flopsynth_layout(SMALL, &metrics(), &view);
    let bottom = |l: &fontelle_ui::canvas::FlopsynthLayout| {
        l.cards.iter().map(|c| c.frame.bottom()).fold(0.0, f32::max)
    };
    assert!(
        bottom(&small) <= SMALL.bottom() + 0.01,
        "the cards run off the bottom at the minimum size: {} past {}",
        bottom(&small),
        SMALL.bottom()
    );
    // Air first, then the pictures — the controls keep their size longest.
    let picture = |l: &fontelle_ui::canvas::FlopsynthLayout| l.cards[0].picture.height;
    assert!(
        picture(&small) <= picture(&big),
        "the picture shrinks before the controls do"
    );
    let cell = |l: &fontelle_ui::canvas::FlopsynthLayout| l.cards[0].cells[0].1.height;
    assert!(
        (cell(&small) - cell(&big)).abs() < 0.01,
        "a control's cell is the same size at both, so the knobs stay usable"
    );
}

#[test]
fn a_view_with_no_cards_lays_out_to_nothing_rather_than_panicking() {
    let empty = FlopsynthView::default();
    let layout = flopsynth_layout(BODY, &metrics(), &empty);
    assert!(layout.cards.is_empty());
    assert_eq!(flopsynth_hit(&layout, 10.0, 10.0), None);
    // And a body with nothing in it is not a division by zero.
    let none = flopsynth_layout(Rect::ZERO, &metrics(), &a_view());
    assert!(none.cards.iter().all(|c| c.frame.is_empty()));
}

// ============================================================ the gestures
//
// §8.7's table, minus the rows the knob grid already answers. Each of these is
// a pure function of a rectangle and a point, for the reason every other piece
// of layout in this crate is: "you can aim at it" is then a test rather than
// something to notice by looking at it once.

/// A picture rectangle to aim at, big enough that a pixel either way is not
/// the difference between two answers.
fn a_picture() -> Rect {
    Rect::new(100.0, 200.0, 180.0, 54.0)
}

#[test]
fn an_envelope_node_is_where_its_corner_is() {
    // The four nodes are the four corners of the drawn curve, so a person
    // aims at what they can see. Read off `env_curve_points` rather than
    // computed a second way — two answers to "where is the decay node" is
    // exactly the defect that makes a handle you cannot grab.
    let picture = a_picture();
    let (attack, decay, sustain, release) = (0.3, 0.4, 0.6, 0.5);
    let points = env_curve_points(picture, attack, decay, sustain, release);
    for (node, at) in [
        (EnvNode::Attack, points[1]),
        (EnvNode::Decay, points[2]),
        (EnvNode::Sustain, points[3]),
        (EnvNode::Release, points[4]),
    ] {
        assert_eq!(
            env_node_at(picture, attack, decay, sustain, release, at.0, at.1),
            Some(node),
            "at {at:?}"
        );
    }
}

#[test]
fn the_first_point_of_an_envelope_is_not_a_node() {
    // It is where the note started, which is not a thing to move: an attack
    // that began late is not an envelope any synthesiser has.
    let picture = a_picture();
    let points = env_curve_points(picture, 0.3, 0.4, 0.6, 0.5);
    assert_eq!(
        env_node_at(picture, 0.3, 0.4, 0.6, 0.5, points[0].0, points[0].1),
        None
    );
}

#[test]
fn a_press_in_the_open_part_of_an_envelope_hits_nothing() {
    let picture = a_picture();
    assert_eq!(
        env_node_at(
            picture,
            0.3,
            0.4,
            0.6,
            0.5,
            picture.x + picture.width * 0.5,
            picture.y + 2.0
        ),
        None
    );
}

#[test]
fn dragging_an_envelope_node_sideways_is_its_time_and_upwards_is_its_level() {
    // Which way each node moves is the shape of the envelope itself: the
    // three time stages are horizontal and the sustain is the one level.
    let picture = a_picture();
    let start = (0.3f32, 0.4f32, 0.6f32, 0.5f32);
    let right = env_node_drag(picture, EnvNode::Attack, start.0, 40.0, 0.0);
    assert!(right > start.0, "dragging right lengthens the attack");
    let left = env_node_drag(picture, EnvNode::Attack, start.0, -40.0, 0.0);
    assert!(left < start.0);

    let up = env_node_drag(picture, EnvNode::Sustain, start.2, 0.0, -20.0);
    assert!(up > start.2, "up is more level");
    let down = env_node_drag(picture, EnvNode::Sustain, start.2, 0.0, 20.0);
    assert!(down < start.2);
}

#[test]
fn an_envelope_node_cannot_be_dragged_out_of_its_range() {
    let picture = a_picture();
    for node in [
        EnvNode::Attack,
        EnvNode::Decay,
        EnvNode::Sustain,
        EnvNode::Release,
    ] {
        let far = env_node_drag(picture, node, 0.5, 10_000.0, -10_000.0);
        let back = env_node_drag(picture, node, 0.5, -10_000.0, 10_000.0);
        assert!((0.0..=1.0).contains(&far), "{node:?} ran past 1: {far}");
        assert!((0.0..=1.0).contains(&back), "{node:?} ran past 0: {back}");
    }
}

// ------------------------------------------------------ the modulation ring

#[test]
fn the_ring_is_outside_the_knob_and_not_inside_it() {
    // A band around the groove, so a knob with a route on it can still be
    // *turned*: the depth and the value are two controls in one place, and
    // the one you get is the one you aimed at.
    let cell = Rect::new(0.0, 0.0, FLOP_CELL_W, FLOP_CELL_H);
    let knob = flop_knob_rect(cell);
    let middle = (knob.x + knob.width / 2.0, knob.y + knob.height / 2.0);
    assert!(!ring_hit(cell, middle.0, middle.1), "the middle turns it");
    // Just outside the groove, level with the spindle.
    let outside = middle.0 + knob.width / 2.0 + RING_GAP;
    assert!(ring_hit(cell, outside, middle.1), "the ring is a band");
    // And well outside it is the cell's own air, which is nothing.
    assert!(!ring_hit(cell, cell.right() + 20.0, middle.1));
}

#[test]
fn dragging_the_ring_upwards_deepens_the_route() {
    // Bipolar, because a modulation depth is: up adds and down subtracts, and
    // the middle is a route that does nothing rather than a route that is not
    // there.
    assert!(ring_depth(0.0, -40.0) > 0.0);
    assert!(ring_depth(0.0, 40.0) < 0.0);
    assert_eq!(ring_depth(0.5, 0.0), 0.5);
    assert!((-1.0..=1.0).contains(&ring_depth(0.9, -10_000.0)));
    assert!((-1.0..=1.0).contains(&ring_depth(-0.9, 10_000.0)));
}

// ============================================================== the pages
//
// §8.3–§8.6. Four pages, and the same window: the tabs are a strip along the
// top of the body and everything under them is laid out in the room that is
// left. What is *on* each page is the host's decision (which cards it puts in
// the view); what this file holds is that the strip exists, that it can be
// pressed, and that nothing below it is drawn over it.

fn a_view_on(page: FlopsynthPage) -> FlopsynthView {
    let mut view = a_view();
    view.page = page;
    view
}

#[test]
fn every_page_has_a_tab_and_the_tabs_are_in_the_body() {
    let theme = Theme::dark_default();
    let layout = flopsynth_layout(BODY, &theme.metrics, &a_view());
    assert_eq!(layout.tabs.len(), FlopsynthPage::ALL.len());
    for (page, rect) in &layout.tabs {
        assert!(!rect.is_empty(), "{page:?} has no tab");
        assert!(
            rect.y >= BODY.y - 0.01 && rect.right() <= BODY.right() + 0.01,
            "{page:?}'s tab is outside the body"
        );
    }
}

#[test]
fn a_tab_is_hit_at_its_own_centre() {
    let theme = Theme::dark_default();
    let layout = flopsynth_layout(BODY, &theme.metrics, &a_view());
    for (page, rect) in layout.tabs.clone() {
        assert_eq!(
            flopsynth_tab_at(
                &layout,
                rect.x + rect.width / 2.0,
                rect.y + rect.height / 2.0
            ),
            Some(page)
        );
    }
}

#[test]
fn the_cards_start_under_the_tabs() {
    // Not over them: the strip is chrome and the cards are content, and a card
    // drawn across a tab is a tab that cannot be pressed.
    let theme = Theme::dark_default();
    let layout = flopsynth_layout(BODY, &theme.metrics, &a_view());
    let strip = layout
        .tabs
        .iter()
        .map(|(_, rect)| rect.bottom())
        .fold(0.0f32, f32::max);
    for card in &layout.cards {
        assert!(
            card.frame.y >= strip - 0.01,
            "a card at {} runs over the tab strip ending at {strip}",
            card.frame.y
        );
    }
}

// ------------------------------------------------------ the source badges

#[test]
fn the_modulation_page_lays_out_a_badge_for_every_source() {
    let theme = Theme::dark_default();
    let mut view = a_view_on(FlopsynthPage::Modulation);
    view.sources = vec!["ENV 1".into(), "LFO 1".into(), "Wheel".into()];
    let layout = flopsynth_layout(BODY, &theme.metrics, &view);
    assert_eq!(layout.badges.len(), 3);
    for (index, rect) in layout.badges.iter().enumerate() {
        assert!(!rect.is_empty(), "badge {index} has no room");
        assert_eq!(
            badge_at(
                &layout,
                rect.x + rect.width / 2.0,
                rect.y + rect.height / 2.0
            ),
            Some(index)
        );
    }
}

#[test]
fn the_synth_page_has_no_badges() {
    // The badges belong to the page that is *about* modulation. A row of them
    // on every page would be a row of buttons that mean nothing where they are.
    let theme = Theme::dark_default();
    let mut view = a_view_on(FlopsynthPage::Synth);
    view.sources = vec!["ENV 1".into()];
    assert!(
        flopsynth_layout(BODY, &theme.metrics, &view)
            .badges
            .is_empty()
    );
}

// -------------------------------------------------------------- the matrix

#[test]
fn every_route_gets_a_row_with_something_to_press() {
    let theme = Theme::dark_default();
    let mut view = a_view_on(FlopsynthPage::Modulation);
    view.routes = vec![
        FlopsynthRoute {
            source: "ENV 2".into(),
            destination: "Filter 1 cutoff".into(),
            depth: 0.62,
        },
        FlopsynthRoute {
            source: "LFO 1".into(),
            destination: "OSC A pitch".into(),
            depth: -0.05,
        },
    ];
    let layout = flopsynth_layout(BODY, &theme.metrics, &view);
    assert_eq!(layout.routes.len(), 2);
    for (index, row) in layout.routes.iter().enumerate() {
        assert!(!row.frame.is_empty());
        assert!(row.depth.intersects(&row.frame), "the slider is in the row");
        assert!(row.remove.intersects(&row.frame), "so is the ✕");
        assert!(
            !row.depth.intersects(&row.remove),
            "the slider and the ✕ overlap on row {index}"
        );
        assert_eq!(
            matrix_hit(
                &layout,
                row.remove.x + row.remove.width / 2.0,
                row.remove.y + row.remove.height / 2.0
            ),
            Some(MatrixHit::Remove(index))
        );
        assert_eq!(
            matrix_hit(
                &layout,
                row.depth.x + row.depth.width / 2.0,
                row.depth.y + row.depth.height / 2.0
            ),
            Some(MatrixHit::Depth(index))
        );
    }
}

/// A matrix with more routes than the panel has rows for **scrolls**, and
/// the page still does not (§8.1 rule 7; `docs/flopsynth-next.md` §3.4).
///
/// Found by the Modulation fit test (§1.4(2)): the Grand Piano's nineteen
/// routes want 448 pixels and the page, with the canopy at nothing and every
/// card at its floor, has room for fourteen. The rows that do not fit are not
/// drawn rather than drawn off the window's edge; the wheel brings them up;
/// the layout says how far it can go.
#[test]
fn a_matrix_with_more_rows_than_room_scrolls_rather_than_running_off() {
    let theme = Theme::dark_default();
    let mut view = a_view_on(FlopsynthPage::Modulation);
    view.routes = (0..40)
        .map(|i| FlopsynthRoute {
            source: format!("LFO {}", i % 4 + 1),
            destination: format!("dest {i}"),
            depth: 0.5,
        })
        .collect();
    let layout = flopsynth_layout(BODY, &theme.metrics, &view);
    assert_eq!(layout.routes.len(), 40, "every route keeps its index");
    let drawn: Vec<usize> = (0..40)
        .filter(|i| !layout.routes[*i].frame.is_empty())
        .collect();
    assert!(
        drawn.len() < 40 && drawn.len() >= 4,
        "forty rows do not fit a 700-pixel body, and some do: {drawn:?}"
    );
    assert_eq!(drawn[0], 0, "unscrolled, the first row is the first route");
    for index in &drawn {
        let row = layout.routes[*index].frame;
        assert!(
            row.y >= layout.matrix.y + CARD_HEADER - 0.01
                && row.bottom() <= layout.matrix.bottom() + 0.01,
            "row {index} at {row:?} is drawn outside the panel {:?}",
            layout.matrix
        );
    }
    let hidden = 40 - drawn.len();
    assert!(
        (layout.matrix_max_scroll - hidden as f32 * fontelle_ui::canvas::MATRIX_ROW).abs() < 0.01,
        "the most it can scroll is the rows that are hidden: {} vs {hidden} rows",
        layout.matrix_max_scroll
    );
    assert!(
        !layout.matrix_scrollbar.is_empty()
            && layout.matrix_scrollbar.right() <= layout.matrix.right() + 0.01,
        "a thumb says where the list is: {:?}",
        layout.matrix_scrollbar
    );

    // Scrolled to the end: the last route is drawn and the first is not.
    view.matrix_scroll = layout.matrix_max_scroll;
    let scrolled = flopsynth_layout(BODY, &theme.metrics, &view);
    assert!(scrolled.routes[0].frame.is_empty());
    let last = scrolled.routes[39].frame;
    // Whole rows: the foot may have less than a row of air under the last.
    assert!(
        !last.is_empty()
            && last.bottom() <= scrolled.matrix.bottom() + 0.01
            && last.bottom() > scrolled.matrix.bottom() - fontelle_ui::canvas::MATRIX_ROW - 8.0,
        "the last row sits at the panel's foot: {last:?} in {:?}",
        scrolled.matrix
    );
    assert_eq!(
        matrix_hit(
            &scrolled,
            last.x + last.width - 9.0,
            last.y + last.height / 2.0
        ),
        Some(MatrixHit::Remove(39)),
        "and a press on it is a press on route 39, not on whatever was there unscrolled"
    );
    assert_eq!(
        matrix_hit(
            &scrolled,
            layout.routes[0].depth.x + 1.0,
            layout.routes[0].depth.y + 1.0
        )
        .filter(|hit| matches!(hit, MatrixHit::Depth(0) | MatrixHit::Remove(0))),
        None,
        "route 0 is off the top and cannot be pressed"
    );
    // Past the end is the end.
    view.matrix_scroll = 10_000.0;
    let clamped = flopsynth_layout(BODY, &theme.metrics, &view);
    assert_eq!(clamped.routes[39].frame, last);

    // And with room for every row, nothing scrolls.
    view.routes.truncate(2);
    view.matrix_scroll = 0.0;
    let short = flopsynth_layout(BODY, &theme.metrics, &view);
    assert_eq!(short.matrix_max_scroll, 0.0);
    assert!(short.matrix_scrollbar.is_empty());
}

/// The cards do not shrink to make room for the matrix. They did: at the
/// size the window opens at, the Grand Piano's routes pushed ENV 3 and ENV 4
/// to the cell floor, and their captions read "release a shaped shaper
/// shape" (`docs/flopsynth-next.md` §1.4(8)). The matrix scrolls; a card's
/// captions cannot.
#[test]
fn the_matrix_scrolls_before_a_card_shrinks() {
    let mut view = a_view_on(FlopsynthPage::Modulation);
    // Two cards with two rows of cells each, and a matrix that wants more
    // than the page has.
    let envelope = || FlopsynthPicture::Envelope {
        attack: 0.1,
        decay: 0.3,
        sustain: 0.8,
        release: 0.4,
    };
    view.cards = vec![
        card("ENV 3", 1, false, 5, envelope(), env_params(3)),
        card("ENV 4", 1, false, 5, envelope(), env_params(4)),
    ];
    view.sources = (0..21).map(|i| format!("source {i}")).collect();
    view.routes = (0..40)
        .map(|i| FlopsynthRoute {
            source: "ENV 3".into(),
            destination: format!("dest {i}"),
            depth: 0.5,
        })
        .collect();
    let body = real_body(fontelle_ui::layout::FLOPSYNTH_SIZE);
    let layout = assert_fits(body, &view);
    for card in &layout.cards {
        for (_, cell) in &card.cells {
            assert!(
                (cell.height - FLOP_CELL_H).abs() < 0.01,
                "a cell shrank to {} to make room for the matrix",
                cell.height
            );
        }
    }
    let drawn = layout.routes.iter().filter(|r| !r.frame.is_empty()).count();
    assert!(
        drawn >= 3 && layout.matrix_max_scroll > 0.0,
        "the matrix shows {drawn} rows and scrolls {}",
        layout.matrix_max_scroll
    );
    let cards_bottom = layout
        .cards
        .iter()
        .map(|c| c.frame.bottom())
        .fold(0.0f32, f32::max);
    assert!(layout.matrix.y >= cards_bottom - 0.01);
}

#[test]
fn a_depth_slider_reads_the_press_as_a_bipolar_value() {
    // A slider rather than a knob, because the row is 22 pixels: the audio
    // editor's argument, and the same shape.
    let row = Rect::new(100.0, 50.0, 120.0, 18.0);
    assert!(
        (matrix_depth_at(row, row.x) + 1.0).abs() < 0.01,
        "the left end is -1"
    );
    assert!((matrix_depth_at(row, row.right()) - 1.0).abs() < 0.01);
    assert!(
        matrix_depth_at(row, row.x + row.width / 2.0).abs() < 0.01,
        "the middle is nothing"
    );
}

#[test]
fn the_matrix_says_when_it_is_empty_rather_than_drawing_nothing() {
    let theme = Theme::dark_default();
    let view = a_view_on(FlopsynthPage::Modulation);
    let layout = flopsynth_layout(BODY, &theme.metrics, &view);
    assert!(layout.routes.is_empty());
    assert!(
        !layout.matrix.is_empty(),
        "the panel is there even with no rows in it, so it can say so"
    );
}

// ======================================================= the whole page
//
// The tests above hold the rules with a handful of cards. These hold them with
// **every card the Synth page really has** — five oscillators, three filters,
// two envelopes, the voice, the macros and the channel — at the size the
// window opens at, because that is the case that overflowed: the first build
// put the voice card above the oscillators, wrapped the noise off the bottom,
// and never drew a filter at all. A layout test with four cards in it could
// not see any of that.

/// The body Flopsynth's window really lays out into, at the size it opens at.
fn real_body(size: (u32, u32)) -> Rect {
    let theme = Theme::dark_default();
    fontelle_ui::layout::editor_window_layout(size.0 as f32, size.1 as f32, &theme.metrics).body
}

fn chooser(address: &str, label: &str, options: &[&str]) -> InstrumentParam {
    InstrumentParam {
        address: fontelle_types::ParamAddress::new(address),
        label: label.to_string(),
        value: 0.0,
        display: options[0].to_string(),
        kind: ParamKind::Choice(options.iter().map(|o| o.to_string()).collect()),
        automated: false,
    }
}

fn switch(address: &str, label: &str) -> InstrumentParam {
    InstrumentParam {
        address: fontelle_types::ParamAddress::new(address),
        label: label.to_string(),
        value: 1.0,
        display: "on".to_string(),
        kind: ParamKind::Switch,
        automated: false,
    }
}

/// One card, the way the host declares it: what it is called, which band it
/// sits in, whether it stands aside, how many cells across, and its controls.
fn card(
    name: &str,
    row: usize,
    aside: bool,
    columns: usize,
    picture: FlopsynthPicture,
    params: Vec<InstrumentParam>,
) -> FlopsynthCard {
    FlopsynthCard {
        oscillator: None,
        group: InstrumentGroup {
            name: name.to_string(),
            params,
        },
        picture,
        row,
        aside,
        columns,
        removable: name.starts_with("FX "),
    }
}

/// An oscillator's seventeen controls, with the table chooser first — whose
/// names run to "NES Pulse 12.5" and want a double cell.
fn osc_params(layer: usize) -> Vec<InstrumentParam> {
    let at = |tail: &str| format!("patch/layer[{layer}]/synth/{tail}");
    vec![
        chooser(
            &at("table"),
            "table",
            &["Saw", "Analog Morph", "NES Pulse 12.5"],
        ),
        knob(&at("position"), "pos", 0.3),
        knob(&format!("patch/layer[{layer}]/gain"), "level", 0.6),
        knob(&format!("patch/layer[{layer}]/pan"), "pan", 0.5),
        chooser(&at("warp_mode"), "warp", &["Off", "Bend", "Sync", "FM"]),
        knob(&at("warp"), "amount", 0.0),
        chooser(&at("modulator"), "mod from", &["none", "OSC B", "OSC C"]),
        knob(&at("unison/voices"), "unison", 0.0),
        knob(&at("unison/detune"), "detune", 0.2),
        knob(&at("unison/blend"), "blend", 1.0),
        knob(&at("unison/width"), "width", 0.5),
        knob(&at("phase"), "phase", 0.0),
        switch(&at("random_phase"), "random"),
        knob(&at("semitones"), "semis", 0.5),
        knob(&format!("patch/layer[{layer}]/tune"), "fine", 0.5),
        switch(&at("key_track"), "key"),
        chooser(&at("route"), "route", &["F1", "F2", "F1→F2", "Bypass"]),
    ]
}

fn filter_params(slot: usize) -> Vec<InstrumentParam> {
    let at = |tail: &str| format!("patch/filter[{slot}]/{tail}");
    vec![
        switch(&at("enabled"), "on"),
        chooser(
            &at("model"),
            "model",
            &["Clean", "Ladder", "Formant", "Comb"],
        ),
        chooser(&at("mode"), "shape", &["LP", "HP", "BP", "Notch"]),
        chooser(&at("slope"), "slope", &["12 dB", "24 dB"]),
        knob(&at("cutoff"), "cutoff", 0.8),
        knob(&at("resonance"), "res", 0.2),
        knob(&at("drive"), "drive", 0.0),
        knob(&at("key_track"), "key trk", 0.0),
        knob(&at("character"), "drive", 0.0),
    ]
}

fn env_params(index: usize) -> Vec<InstrumentParam> {
    let at = |tail: &str| format!("patch/env[{index}]/{tail}");
    vec![
        knob(&at("delay"), "delay", 0.0),
        knob(&at("attack"), "attack", 0.1),
        knob(&at("hold"), "hold", 0.0),
        knob(&at("decay"), "decay", 0.3),
        knob(&at("sustain"), "sustain", 0.8),
        knob(&at("release"), "release", 0.4),
        knob(&at("attack_shape"), "a shape", 0.5),
        knob(&at("decay_shape"), "d shape", 0.5),
        knob(&at("release_shape"), "r shape", 0.5),
    ]
}

/// The Synth page as the host builds it — **in the host's order**, which puts
/// the channel and the voice first because that is where the parameter list
/// starts. The bands say where they go; the order they are listed in does not.
///
/// Three bands: the sources; the filters with the channel's own two knobs at
/// the end of the row, which is where the sound goes out; and the two
/// envelopes with the voice and the macros beside them.
fn synth_page() -> FlopsynthView {
    let wave = || FlopsynthPicture::Wave {
        points: vec![0.0; 128],
        position: 0.0,
    };
    let response = || FlopsynthPicture::Response {
        points: vec![0.0; 96],
        cutoff: 0.5,
        resonance: 0.2,
    };
    let envelope = || FlopsynthPicture::Envelope {
        attack: 0.1,
        decay: 0.3,
        sustain: 0.8,
        release: 0.4,
    };
    FlopsynthView {
        title: "Init".to_string(),
        cards: vec![
            card(
                "Channel",
                1,
                false,
                2,
                FlopsynthPicture::None,
                vec![
                    knob("mixer/gain", "volume", 0.8),
                    knob("mixer/pan", "pan", 0.5),
                ],
            ),
            card(
                "Voice",
                2,
                false,
                3,
                FlopsynthPicture::None,
                vec![
                    chooser("patch/voice/mode", "mode", &["Poly", "Mono", "Legato"]),
                    knob("patch/voice/polyphony", "voices", 0.5),
                    knob("patch/voice/glide", "glide", 0.0),
                    knob("patch/voice/bend_range", "bend", 0.1),
                    knob("patch/output", "output", 0.7),
                ],
            ),
            card("OSC A", 0, false, 6, wave(), osc_params(0)),
            card("OSC B", 0, false, 6, wave(), osc_params(1)),
            card("OSC C", 0, false, 6, wave(), osc_params(2)),
            card("SUB", 0, true, 3, wave(), osc_params(3)),
            card(
                "NOISE",
                0,
                true,
                3,
                FlopsynthPicture::None,
                vec![
                    knob("patch/layer[4]/synth/noise_colour", "colour", 0.2),
                    knob("patch/layer[4]/gain", "level", 0.0),
                    knob("patch/layer[4]/pan", "pan", 0.5),
                    knob("patch/layer[4]/synth/semitones", "semis", 0.5),
                    knob("patch/layer[4]/tune", "fine", 0.5),
                    switch("patch/layer[4]/synth/key_track", "key"),
                    chooser(
                        "patch/layer[4]/synth/route",
                        "route",
                        &["F1", "F2", "Bypass"],
                    ),
                ],
            ),
            card("Filter 1", 1, false, 5, response(), filter_params(0)),
            card("Filter 2", 1, false, 5, response(), filter_params(1)),
            card("Filter 3", 1, false, 5, response(), filter_params(2)),
            card("ENV 1 \u{b7} amp", 2, false, 5, envelope(), env_params(0)),
            card(
                "ENV 2 \u{b7} filter",
                2,
                false,
                5,
                envelope(),
                env_params(1),
            ),
            card(
                "Macros",
                2,
                false,
                4,
                FlopsynthPicture::None,
                (0..4)
                    .map(|i| {
                        knob(
                            &format!("patch/macro[{i}]"),
                            &format!("macro {}", i + 1),
                            0.0,
                        )
                    })
                    .collect(),
            ),
        ],
        page: FlopsynthPage::Synth,
        ..FlopsynthView::default()
    }
}

fn frame_of(
    layout: &fontelle_ui::canvas::FlopsynthLayout,
    view: &FlopsynthView,
    name: &str,
) -> Rect {
    let index = view
        .cards
        .iter()
        .position(|c| c.group.name == name)
        .unwrap_or_else(|| panic!("no card called {name}"));
    layout.cards[index].frame
}

/// Everything on the page, checked against the body: nothing outside it,
/// nothing over anything else, and every control drawn — at the size the
/// window opens at.
fn assert_fits(body: Rect, view: &FlopsynthView) -> fontelle_ui::canvas::FlopsynthLayout {
    let layout = flopsynth_layout(body, &metrics(), view);
    let mut frames = Vec::new();
    for (index, placed) in layout.cards.iter().enumerate() {
        let name = &view.cards[index].group.name;
        assert!(
            !placed.frame.is_empty(),
            "{name} was not drawn at {}x{}",
            body.width,
            body.height
        );
        assert!(
            placed.frame.x >= body.x - 0.01
                && placed.frame.right() <= body.right() + 0.01
                && placed.frame.y >= body.y - 0.01
                && placed.frame.bottom() <= body.bottom() + 0.01,
            "{name} runs outside the {}x{} body: {:?}",
            body.width,
            body.height,
            placed.frame
        );
        assert_eq!(
            placed.cells.len(),
            view.cards[index].group.params.len(),
            "{name} lost a control"
        );
        for (param, cell) in &placed.cells {
            assert!(
                cell.x >= placed.frame.x - 0.01
                    && cell.right() <= placed.frame.right() + 0.01
                    && cell.bottom() <= placed.frame.bottom() + 0.01,
                "{name}'s control {param} is outside its card"
            );
        }
        frames.push((name.clone(), placed.frame));
    }
    for (i, (a_name, a)) in frames.iter().enumerate() {
        for (b_name, b) in &frames[i + 1..] {
            let overlaps = a.x < b.right() - 0.01
                && b.x < a.right() - 0.01
                && a.y < b.bottom() - 0.01
                && b.y < a.bottom() - 0.01;
            assert!(!overlaps, "{a_name} and {b_name} overlap: {a:?} and {b:?}");
        }
    }
    layout
}

#[test]
fn the_whole_synth_page_fits_the_window_it_opens_at() {
    let view = synth_page();
    let body = real_body(fontelle_ui::layout::FLOPSYNTH_SIZE);
    let layout = assert_fits(body, &view);
    // And at the size that opens, the controls are their design size: the
    // page was sized to fit, and shrinking would mean it was not.
    let cell = layout.cards[2].cells[1].1;
    assert!(
        (cell.width - FLOP_CELL_W).abs() < 0.01 && (cell.height - FLOP_CELL_H).abs() < 0.01,
        "a control at the default size is {}x{}, not {FLOP_CELL_W}x{FLOP_CELL_H}",
        cell.width,
        cell.height
    );
}

#[test]
fn the_cards_are_placed_by_band_whatever_order_they_are_listed_in() {
    // The host lists the channel and the voice first, because that is where
    // the parameter list begins. They are the *last* band, and the first
    // build drew them above the oscillators — a list, which is exactly what
    // §8.1's first rule forbids.
    let view = synth_page();
    let layout = assert_fits(real_body(fontelle_ui::layout::FLOPSYNTH_SIZE), &view);
    let at = |name: &str| frame_of(&layout, &view, name);
    assert!(
        at("OSC A").y < at("Filter 1").y,
        "the oscillators are above the filters"
    );
    assert!(
        at("Filter 1").bottom() <= at("ENV 1 \u{b7} amp").y + 0.01,
        "the filters are above the envelopes"
    );
    assert!(
        at("Channel").y >= at("Filter 1").y - 0.01
            && at("Voice").y >= at("ENV 1 \u{b7} amp").y - 0.01,
        "the channel and the voice are in the later bands, not the first"
    );
    // Within a band, the listed order holds.
    assert!(at("OSC A").x < at("OSC B").x && at("OSC B").x < at("OSC C").x);
    assert!(at("Filter 1").x < at("Filter 2").x && at("Filter 2").x < at("Filter 3").x);
}

#[test]
fn aside_cards_stack_down_the_right_edge_beside_the_bands() {
    // The sub and the noise are sources too, but they are slim and the three
    // oscillators are not: set aside in a column, they stand beside the
    // oscillators *and* the filters, and the page is three bands tall rather
    // than four.
    let view = synth_page();
    let body = real_body(fontelle_ui::layout::FLOPSYNTH_SIZE);
    let layout = assert_fits(body, &view);
    let at = |name: &str| frame_of(&layout, &view, name);
    let (sub, noise) = (at("SUB"), at("NOISE"));
    assert!(
        sub.x >= at("OSC C").right() - 0.01,
        "the sub is to the right of the last oscillator: {sub:?} vs {:?}",
        at("OSC C")
    );
    assert!(
        (sub.right() - body.right()).abs() < 0.51,
        "the aside column sits against the body's right edge: {} vs {}",
        sub.right(),
        body.right()
    );
    assert!(
        (noise.x - sub.x).abs() < 0.01 && noise.y >= sub.bottom() - 0.01,
        "the noise is stacked under the sub: {noise:?} under {sub:?}"
    );
    assert!(
        (sub.y - at("OSC A").y).abs() < 0.01,
        "the column starts level with the first band"
    );
    // The bands wrap in the room beside the column while it is there.
    assert!(
        at("Filter 3").right() <= sub.x + 0.01,
        "the filters keep clear of the column beside them"
    );
}

#[test]
fn a_card_declares_its_own_width_in_columns() {
    // Sized by what it *is*, not by a count: the oscillator's seventeen
    // controls go six across so three oscillators share a row; the same
    // seventeen on the sub go three across so it can stand aside.
    let view = synth_page();
    let layout = assert_fits(real_body(fontelle_ui::layout::FLOPSYNTH_SIZE), &view);
    let across = |name: &str| {
        let index = view
            .cards
            .iter()
            .position(|c| c.group.name == name)
            .unwrap();
        let cells = &layout.cards[index].cells;
        let first_y = cells[1].1.y;
        cells
            .iter()
            .filter(|(_, cell)| (cell.y - first_y).abs() < 0.01)
            .count()
    };
    assert_eq!(
        across("OSC A"),
        5,
        "six cells across, one of them the double table chooser"
    );
    assert_eq!(
        across("SUB"),
        2,
        "three cells across, one of them the double table chooser"
    );
    assert_eq!(across("Filter 1"), 5);
    assert_eq!(across("Channel"), 2);
}

#[test]
fn a_chooser_with_long_names_gets_a_double_cell() {
    // "NES Pulse 12.5" in a cell fifty pixels wide is "NES Pu", and a chooser
    // whose value cannot be read is a chooser nobody can set on purpose. The
    // table takes two cells; "Poly" and "24 dB" take one.
    let long = chooser(
        "patch/layer[0]/synth/table",
        "table",
        &["Saw", "NES Pulse 12.5"],
    );
    let short = chooser("patch/voice/mode", "mode", &["Poly", "Mono", "Legato"]);
    assert_eq!(cell_span(&long), 2);
    assert_eq!(cell_span(&short), 1);
    assert_eq!(cell_span(&knob("patch/output", "output", 0.5)), 1);

    let view = synth_page();
    let layout = assert_fits(real_body(fontelle_ui::layout::FLOPSYNTH_SIZE), &view);
    let osc = &layout.cards[2].cells;
    assert!(
        (osc[0].1.width - osc[1].1.width * 2.0).abs() < 0.01,
        "the table's cell is {} wide and a knob's is {}",
        osc[0].1.width,
        osc[1].1.width
    );
    assert!(
        (osc[0].1.y - osc[1].1.y).abs() < 0.01,
        "the double cell shares its row with the next control"
    );
}

/// At the smallest size the window may be dragged to, every cell is its
/// design size — because the captions and values were designed for that
/// cell and a smaller one clips them ("20.00 kH", "Hardne:macro 3" at the
/// old 980×620 minimum: `docs/flopsynth-next.md` §1.4(3)). The layout has no
/// text to measure; what it can promise is the cell the text was drawn for.
/// The pictures may still be at their floor here — a picture has no caption.
#[test]
fn at_the_minimum_size_every_cell_is_its_design_size() {
    let view = synth_page();
    let body = real_body(fontelle_ui::layout::FLOPSYNTH_MINIMUM);
    let layout = assert_fits(body, &view);
    for (index, card) in layout.cards.iter().enumerate() {
        for (param, cell) in &card.cells {
            let control = &view.cards[index].group.params[*param];
            if fontelle_ui::canvas::is_nameplate_control(control) {
                continue;
            }
            let span = cell_span(control) as f32;
            assert!(
                (cell.width - FLOP_CELL_W * span).abs() < 0.01
                    && (cell.height - FLOP_CELL_H).abs() < 0.01,
                "{}'s {} is {}x{} at the minimum size, not {}x{}",
                view.cards[index].group.name,
                control.label,
                cell.width,
                cell.height,
                FLOP_CELL_W * span,
                FLOP_CELL_H
            );
        }
    }
}

#[test]
fn a_full_page_in_a_small_window_shrinks_its_cells_rather_than_overflowing() {
    // §8.8: below the default size the cards lose their air, then their
    // pictures, and — this is the new step — then the controls shrink
    // together, down to a floor. A window at its minimum has every control on
    // it, smaller, rather than a page with its bottom band missing.
    //
    // The window refuses a size this small now (the test above says why);
    // the layout asked anyway still answers with every control on the page.
    let view = synth_page();
    let body = real_body((980, 620));
    let layout = assert_fits(body, &view);
    let cell = layout.cards[2].cells[1].1;
    assert!(
        cell.width < FLOP_CELL_W && cell.height < FLOP_CELL_H,
        "the cells did not shrink at the minimum size: {}x{}",
        cell.width,
        cell.height
    );
    assert!(
        cell.width >= FLOP_CELL_W * CELL_FLOOR - 0.01
            && cell.height >= FLOP_CELL_H * CELL_FLOOR - 0.01,
        "the cells shrank past the floor: {}x{}",
        cell.width,
        cell.height
    );
    // The knob shrinks with its cell, and is still somewhere inside it.
    let knob = flop_knob_rect(cell);
    assert!(
        knob.width < 24.0 && knob.width > 12.0,
        "the knob is {} wide",
        knob.width
    );
    assert!(cell.contains(knob.x + 1.0, knob.y + 1.0));
}

#[test]
fn a_page_that_fits_is_not_shrunk() {
    // The floor is for the page that overflows. Four cards in a big window
    // are drawn at their design size, exactly as before.
    let layout = flopsynth_layout(BODY, &metrics(), &a_view());
    let cell = layout.cards[0].cells[0].1;
    assert!((cell.width - FLOP_CELL_W).abs() < 0.01);
    assert!((cell.height - FLOP_CELL_H).abs() < 0.01);
}

// ======================================================== the Presets page
//
// §8.6: a view over the bank filtered to this device, so a person does not
// have to leave the synth to browse. Shelves down the left, the presets in
// the middle under a search box, and the one that is loaded described on the
// right. The first build left this page blank.

fn a_preset(name: &str, category: &str, favourite: bool, mine: bool) -> PresetChoice {
    PresetChoice {
        name: name.to_string(),
        category: category.to_string(),
        origin: if mine {
            fontelle_types::PresetOrigin::User
        } else {
            fontelle_types::PresetOrigin::Factory
        },
        favourite,
    }
}

fn a_bank() -> Vec<PresetChoice> {
    vec![
        a_preset("Glass", "Pad", false, false),
        a_preset("Warm", "Pad", true, false),
        a_preset("Sub Sine", "Bass", false, false),
        a_preset("Reese", "Bass", false, false),
        a_preset("Choir Ahh", "Choir & Vocal", true, false),
        a_preset("My Pad", "Pad", false, true),
    ]
}

fn presets_view(browse: PresetBrowse) -> FlopsynthView {
    FlopsynthView {
        page: FlopsynthPage::Presets,
        bank: a_bank(),
        browse,
        ..FlopsynthView::default()
    }
}

#[test]
fn the_shelves_are_the_favourites_everything_each_category_and_what_is_mine() {
    let shelves = preset_shelves(&a_bank());
    assert_eq!(
        shelves,
        [
            PresetShelf::Favourites,
            PresetShelf::All,
            PresetShelf::Category("Pad".into()),
            PresetShelf::Category("Bass".into()),
            PresetShelf::Category("Choir & Vocal".into()),
            PresetShelf::Mine,
        ]
    );
    // A bank with no stars and nothing of the user's has neither shelf: a
    // shelf that is always empty is a shelf that teaches nothing.
    let plain: Vec<PresetChoice> = a_bank()
        .into_iter()
        .filter(|p| !p.favourite && p.origin == fontelle_types::PresetOrigin::Factory)
        .collect();
    let shelves = preset_shelves(&plain);
    assert!(!shelves.contains(&PresetShelf::Favourites));
    assert!(!shelves.contains(&PresetShelf::Mine));
    assert_eq!(shelves[0], PresetShelf::All);
}

#[test]
fn a_shelf_lists_its_own_presets_in_the_banks_order() {
    let bank = a_bank();
    assert_eq!(
        preset_page_rows(&bank, &PresetShelf::All, ""),
        [0, 1, 2, 3, 4, 5]
    );
    assert_eq!(
        preset_page_rows(&bank, &PresetShelf::Category("Pad".into()), ""),
        [0, 1, 5]
    );
    assert_eq!(
        preset_page_rows(&bank, &PresetShelf::Favourites, ""),
        [1, 4]
    );
    assert_eq!(preset_page_rows(&bank, &PresetShelf::Mine, ""), [5]);
}

#[test]
fn the_search_narrows_the_shelf_to_what_matches() {
    // The menu's own rule: a plain substring, ignoring case, anywhere in the
    // name — and on top of whatever shelf is chosen, not instead of it.
    let bank = a_bank();
    assert_eq!(preset_page_rows(&bank, &PresetShelf::All, "pad"), [5]);
    assert_eq!(preset_page_rows(&bank, &PresetShelf::All, "S"), [0, 2, 3]);
    assert_eq!(
        preset_page_rows(&bank, &PresetShelf::Category("Bass".into()), "sub"),
        [2]
    );
    assert!(preset_page_rows(&bank, &PresetShelf::All, "zzz").is_empty());
}

#[test]
fn the_presets_page_lays_out_its_three_columns_and_every_row_showing() {
    let view = presets_view(PresetBrowse::default());
    let layout = flopsynth_layout(BODY, &metrics(), &view);
    let page = &layout.presets;
    assert_eq!(page.shelves.len(), preset_shelves(&view.bank).len());
    assert!(!page.search.is_empty(), "there is a search box");
    assert!(!page.list.is_empty() && !page.about.is_empty());
    // Shelves, then the list, then the about column, left to right.
    let shelf = page.shelves[0].1;
    assert!(shelf.right() <= page.list.x + 0.01);
    assert!(page.list.right() <= page.about.x + 0.01);
    assert!(
        page.search.bottom() <= page.rows[0].1.y + 0.01,
        "the search sits over the rows"
    );
    // Six presets in the All shelf, all of them showing in a big window.
    assert_eq!(page.rows.len(), 6);
    for (which, rect) in &page.rows {
        assert!(!rect.is_empty(), "preset {which} has no row");
        assert!(page.list.contains(rect.x + 1.0, rect.y + 1.0));
    }
    // And no cards on this page, whatever the host put in the view.
    assert!(layout.cards.is_empty() || layout.cards.iter().all(|c| c.frame.is_empty()));
}

#[test]
fn a_press_on_the_presets_page_names_what_it_landed_on() {
    let view = presets_view(PresetBrowse::default());
    let layout = flopsynth_layout(BODY, &metrics(), &view);
    let page = &layout.presets;
    let middle = |r: Rect| (r.x + r.width / 2.0, r.y + r.height / 2.0);
    for (index, (_, rect)) in page.shelves.iter().enumerate() {
        let (x, y) = middle(*rect);
        assert_eq!(presets_hit(&layout, x, y), Some(PresetsHit::Shelf(index)));
    }
    let (x, y) = middle(page.search);
    assert_eq!(presets_hit(&layout, x, y), Some(PresetsHit::Search));
    for (which, rect) in &page.rows {
        // The row's middle is the preset; its right-hand end is the star.
        let (x, y) = middle(*rect);
        assert_eq!(presets_hit(&layout, x, y), Some(PresetsHit::Row(*which)));
        assert_eq!(
            presets_hit(&layout, rect.right() - 6.0, y),
            Some(PresetsHit::Star(*which)),
            "the right end of a row is its star"
        );
    }
    let (x, y) = middle(page.about);
    assert_eq!(
        presets_hit(&layout, x, y),
        None,
        "the about column is a read-out"
    );
}

#[test]
fn the_list_scrolls_and_what_is_out_of_sight_is_not_drawn() {
    // A hundred and twenty-eight presets is more than a page. The rows past
    // the bottom are **empty**, which the renderer skips and the hit test
    // cannot land on — the menu's rule, and for the same reason.
    let mut view = presets_view(PresetBrowse::default());
    view.bank = (0..128)
        .map(|i| a_preset(&format!("Preset {i}"), "Pad", false, false))
        .collect();
    let layout = flopsynth_layout(BODY, &metrics(), &view);
    let page = &layout.presets;
    assert_eq!(page.rows.len(), 128);
    assert!(!page.rows[0].1.is_empty());
    assert!(
        page.rows[127].1.is_empty(),
        "the far end is drawn before it is scrolled to"
    );
    assert!(
        page.content_height > page.list.height,
        "the list knows it is longer than its panel"
    );

    view.browse.scroll = page.max_scroll();
    let layout = flopsynth_layout(BODY, &metrics(), &view);
    let page = &layout.presets;
    assert!(
        !page.rows[127].1.is_empty(),
        "scrolled to the end, the last row shows"
    );
    assert!(page.rows[0].1.is_empty());
    let last = page.rows[127].1;
    assert!(
        last.bottom() <= page.list.bottom() + 0.01,
        "a visible row is inside the list"
    );
    assert_eq!(
        presets_hit(
            &layout,
            last.x + last.width / 2.0,
            last.y + last.height / 2.0
        ),
        Some(PresetsHit::Row(127))
    );
}

#[test]
fn choosing_a_shelf_and_typing_change_which_rows_are_laid_out() {
    let view = presets_view(PresetBrowse {
        shelf: PresetShelf::Category("Bass".into()),
        query: "re".into(),
        scroll: 0.0,
    });
    let layout = flopsynth_layout(BODY, &metrics(), &view);
    let which: Vec<usize> = layout.presets.rows.iter().map(|(w, _)| *w).collect();
    assert_eq!(which, [3], "Reese is the one bass preset with 're' in it");
}

#[test]
fn the_matrix_takes_the_room_under_the_cards() {
    // Pinned to the bottom with the cards at the top, the Modulation page had
    // a dead band across its middle and a matrix two rows tall. The matrix
    // starts a gap under the last card and runs to the bottom, so a long
    // list of routes has the room and a short one has air.
    let theme = Theme::dark_default();
    let mut view = a_view_on(FlopsynthPage::Modulation);
    view.sources = vec!["ENV 1".into(), "LFO 1".into()];
    view.routes = vec![FlopsynthRoute {
        source: "ENV 2".into(),
        destination: "Filter 1 cutoff".into(),
        depth: 0.6,
    }];
    let layout = flopsynth_layout(BODY, &theme.metrics, &view);
    let cards_bottom = layout
        .cards
        .iter()
        .map(|c| c.frame.bottom())
        .fold(0.0f32, f32::max);
    assert!(
        (layout.matrix.y - (cards_bottom + CARD_GAP)).abs() < 0.51,
        "the matrix starts at {} and the cards end at {cards_bottom}",
        layout.matrix.y
    );
    // `layout.body` is the strip the cards were given, which stops above the
    // matrix; the matrix itself runs to the bottom of the window's body.
    assert!(
        (layout.matrix.bottom() - BODY.bottom()).abs() < 0.51,
        "the matrix runs to the bottom of the body: {} vs {}",
        layout.matrix.bottom(),
        BODY.bottom()
    );
    assert!(!layout.routes[0].frame.is_empty());
}

// ======================================================== the Effects page
//
// §8.5: the chain after the voice, one card per slot — and a way to put a
// slot there. The first build drew the cards a preset came with and nothing
// else, so the Init patch's Effects page was an empty sky with no word on it.

fn fx_view(cards: usize, room: bool) -> FlopsynthView {
    let mut view = a_view_on(FlopsynthPage::Effects);
    view.cards = (0..cards)
        .map(|i| {
            card(
                &format!("FX {} \u{b7} Chorus", i + 1),
                5,
                false,
                0,
                FlopsynthPicture::None,
                vec![
                    switch(&format!("patch/fx[{i}]/enabled"), "on"),
                    knob(&format!("patch/fx[{i}]/rate"), "rate", 0.3),
                    knob(&format!("patch/fx[{i}]/depth"), "depth", 0.5),
                    knob(&format!("patch/fx[{i}]/mix"), "mix", 0.5),
                ],
            )
        })
        .collect();
    view.fx_room = room;
    view
}

#[test]
fn the_effects_page_offers_an_effect_after_its_cards_until_the_chain_is_full() {
    let layout = flopsynth_layout(BODY, &metrics(), &fx_view(1, true));
    let button = layout.add_effect;
    assert!(!button.is_empty(), "the page offers an effect");
    let card = layout.cards[0].frame;
    assert!(
        button.x >= card.right() - 0.01 || button.y >= card.bottom() - 0.01,
        "the button comes after the card: {button:?} vs {card:?}"
    );
    assert!(!button.intersects(&card));
    assert!(button.right() <= BODY.right() + 0.01 && button.bottom() <= BODY.bottom() + 0.01);
    assert_eq!(
        flopsynth_hit(
            &layout,
            button.x + button.width / 2.0,
            button.y + button.height / 2.0
        ),
        Some(FlopsynthHit::AddEffect)
    );

    // A full chain has nothing to offer, and says so by offering nothing.
    let full = flopsynth_layout(BODY, &metrics(), &fx_view(4, false));
    assert!(full.add_effect.is_empty());

    // No effects at all: the button is the first thing on the page.
    let empty = flopsynth_layout(BODY, &metrics(), &fx_view(0, true));
    assert!(!empty.add_effect.is_empty());
    assert!(
        empty.add_effect.y < empty.canopy.bottom() + CARD_GAP + 1.0,
        "the button is at the top when the page is bare — under the canopy"
    );

    // And nothing of this on the Synth page.
    let synth = flopsynth_layout(BODY, &metrics(), &a_view());
    assert!(synth.add_effect.is_empty());
    let _ = ADD_EFFECT;
}

#[test]
fn an_effect_card_can_be_removed_and_a_source_card_cannot() {
    let layout = flopsynth_layout(BODY, &metrics(), &fx_view(2, true));
    for (index, placed) in layout.cards.iter().enumerate() {
        let remove = placed.remove;
        assert!(
            !remove.is_empty(),
            "effect card {index} has no remove button"
        );
        assert!(
            placed.header.contains(remove.x + 1.0, remove.y + 1.0),
            "the remove button is in the header"
        );
        assert_eq!(
            flopsynth_hit(
                &layout,
                remove.x + remove.width / 2.0,
                remove.y + remove.height / 2.0
            ),
            Some(FlopsynthHit::Remove { card: index })
        );
        // The rest of the header is still the header.
        assert_eq!(
            flopsynth_hit(&layout, placed.header.x + 4.0, placed.header.y + 4.0),
            Some(FlopsynthHit::Header { card: index })
        );
    }
    let synth = flopsynth_layout(BODY, &metrics(), &a_view());
    assert!(
        synth.cards.iter().all(|c| c.remove.is_empty()),
        "an oscillator cannot be removed"
    );
}

/// A drag by an effect card's header lands on another effect card — the
/// one under the pointer — and nowhere else (`docs/flopsynth-next.md`
/// §1.4(5): `FlopsynthHit::Header` was returned by the hit test and matched
/// by nothing, a dead affordance since the first build).
#[test]
fn a_dragged_effect_header_lands_on_the_effect_card_under_the_pointer() {
    let view = fx_view(3, true);
    let layout = flopsynth_layout(BODY, &metrics(), &view);
    for (index, placed) in layout.cards.iter().enumerate() {
        let (cx, cy) = (
            placed.frame.x + placed.frame.width / 2.0,
            placed.frame.y + placed.frame.height / 2.0,
        );
        assert_eq!(
            fontelle_ui::canvas::effect_card_at(&layout, &view, cx, cy),
            Some(index),
            "the middle of card {index} is card {index}"
        );
    }
    assert_eq!(
        fontelle_ui::canvas::effect_card_at(
            &layout,
            &view,
            BODY.right() - 1.0,
            BODY.bottom() - 1.0
        ),
        None,
        "the hull is nobody's"
    );
    // An oscillator is not a slot: on the Synth page nothing takes the drop.
    let synth = a_view();
    let synth_layout = flopsynth_layout(BODY, &metrics(), &synth);
    let osc = synth_layout.cards[0].frame;
    assert_eq!(
        fontelle_ui::canvas::effect_card_at(
            &synth_layout,
            &synth,
            osc.x + osc.width / 2.0,
            osc.y + osc.height / 2.0
        ),
        None
    );
}

// --- The Presets search box's caption, focused and not ---------------------
//
// The box used to take every key while the Presets page was open, so a typed
// letter went into it and Space never reached the transport. It is
// click-to-focus now; these pin the two captions that focus chooses between.

#[test]
fn the_search_box_shows_a_hint_when_empty_and_unfocused() {
    // Nothing typed and the box does not have the keyboard: the prompt, no
    // caret, because a caret on a box nobody is typing in reads as focus.
    assert_eq!(
        fontelle_ui::render::search_caption(""),
        fontelle_ui::render::PRESET_SEARCH_HINT
    );
}

#[test]
fn a_focused_empty_search_box_shows_a_caret_rather_than_the_hint() {
    // Clicked into but nothing typed yet: a caret, so it does not look dead.
    let focused = fontelle_ui::render::focused_search_caption("");
    assert_ne!(focused, fontelle_ui::render::PRESET_SEARCH_HINT);
    assert!(
        focused.contains(fontelle_ui::canvas::NAME_CARET),
        "a focused box shows where typing will land: {focused:?}"
    );
}

#[test]
fn a_query_shows_with_a_caret_whether_or_not_focus_is_named() {
    // Once there is text, both captions are the text with a caret — the box is
    // plainly in use either way.
    assert_eq!(
        fontelle_ui::render::search_caption("bass"),
        fontelle_ui::render::focused_search_caption("bass")
    );
    assert!(fontelle_ui::render::search_caption("bass").starts_with("bass"));
}

// ------------------------------------------------------------ the canopy ---

/// The bridge's window onto the sky sits under the tabs and over the
/// consoles, on every page, at least a slit tall — and at the size the
/// window opens at, with every control still at its design size.
#[test]
fn the_canopy_is_under_the_tabs_and_over_every_card() {
    for view in [
        synth_page(),
        a_view_on(FlopsynthPage::Modulation),
        fx_view(2, true),
    ] {
        let body = real_body(fontelle_ui::layout::FLOPSYNTH_SIZE);
        let layout = assert_fits(body, &view);
        let canopy = layout.canopy;
        assert!(
            canopy.height >= CANOPY_MIN - 0.01,
            "{:?}: the canopy is a slit at least: {canopy:?}",
            view.page
        );
        let tabs_bottom = layout
            .tabs
            .iter()
            .map(|(_, r)| r.bottom())
            .fold(0.0, f32::max);
        assert!(canopy.y >= tabs_bottom - 0.01, "under the tabs");
        for (index, placed) in layout.cards.iter().enumerate() {
            assert!(
                placed.frame.y >= canopy.bottom() - 0.01,
                "{} is drawn over the canopy: {:?} against {canopy:?}",
                view.cards[index].group.name,
                placed.frame
            );
        }
        for badge in &layout.badges {
            assert!(badge.y >= canopy.bottom() - 0.01, "a badge over the canopy");
        }
    }
    // At the opening size the consoles keep their design size: the window
    // grew for the canopy rather than the canopy taking it from the knobs.
    let layout = flopsynth_layout(
        real_body(fontelle_ui::layout::FLOPSYNTH_SIZE),
        &metrics(),
        &synth_page(),
    );
    let cell = layout.cards[2].cells[1].1;
    assert!((cell.width - FLOP_CELL_W).abs() < 0.01);
}

/// A taller window is more sky, not more air between the consoles — up to a
/// limit, past which the room goes back to being air.
#[test]
fn a_taller_window_gives_the_room_to_the_sky() {
    let view = synth_page();
    let short = flopsynth_layout(real_body((1180, 840)), &metrics(), &view);
    let tall = flopsynth_layout(real_body((1180, 1000)), &metrics(), &view);
    assert!(
        tall.canopy.height > short.canopy.height + 100.0,
        "{} against {}",
        tall.canopy.height,
        short.canopy.height
    );
    assert!(tall.canopy.height <= CANOPY_MAX + 0.01);
    let huge = flopsynth_layout(real_body((1180, 1400)), &metrics(), &view);
    assert!(
        (huge.canopy.height - CANOPY_MAX).abs() < 0.01,
        "and no more than the most"
    );
}

/// In a window too small for both, the sky gives before the controls: the
/// consoles still fit at their floor and the canopy is whatever is left,
/// which may be nothing.
#[test]
fn the_sky_gives_before_the_controls_do() {
    let view = synth_page();
    let layout = assert_fits(SMALL, &view);
    assert!(
        layout.canopy.height < CANOPY_MIN,
        "the canopy gave: {}",
        layout.canopy.height
    );
    assert!(layout.canopy.height >= 0.0);
}

// --------------------------------------------------------- the nameplate ---

/// A card's **kind** chooser — what the module *is* — sits on its nameplate
/// beside the name rather than in the grid with the knobs: it is the label
/// on the module, and putting it in the grid cost every oscillator a row.
#[test]
fn the_kind_chooser_sits_on_the_nameplate() {
    let mut view = synth_page();
    // Give OSC A a kind chooser first, the way the host lists it.
    let osc = view
        .cards
        .iter_mut()
        .find(|c| c.group.name == "OSC A")
        .expect("OSC A");
    osc.group.params.insert(
        0,
        InstrumentParam {
            address: fontelle_types::ParamAddress::new("patch/layer[0]/synth/kind"),
            label: "kind".to_string(),
            value: 0.0,
            display: "Table".to_string(),
            kind: ParamKind::Choice(vec![
                "Table".to_string(),
                "Sample".to_string(),
                "String".to_string(),
            ]),
            automated: false,
        },
    );
    let before = flopsynth_layout(BODY, &metrics(), &synth_page());
    let layout = flopsynth_layout(BODY, &metrics(), &view);
    let index = view
        .cards
        .iter()
        .position(|c| c.group.name == "OSC A")
        .unwrap();
    let placed = &layout.cards[index];
    let (param, cell) = placed.cells[0];
    assert_eq!(param, 0);
    assert!(
        placed
            .header
            .contains(cell.x + cell.width / 2.0, cell.y + cell.height / 2.0),
        "the kind's cell is on the nameplate: {cell:?} in {:?}",
        placed.header
    );
    assert!(cell.right() <= placed.header.right() - CARD_GAP / 2.0);
    // The other cells start under the picture as before, and the card is
    // no taller for it.
    assert!(
        (placed.frame.height - before.cards[index].frame.height).abs() < 0.01,
        "a kind chooser costs no height: {} against {}",
        placed.frame.height,
        before.cards[index].frame.height
    );
    // And it is hit like any control.
    assert_eq!(
        flopsynth_hit(
            &layout,
            cell.x + cell.width / 2.0,
            cell.y + cell.height / 2.0
        ),
        Some(FlopsynthHit::Control {
            card: index,
            param: 0
        })
    );
}
