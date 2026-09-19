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
    ADD_EFFECT, CANOPY_HEIGHT, CARD_GAP, CARD_HEADER, EnvNode, FLOP_CELL_H, FLOP_CELL_HALF,
    FLOP_CELL_W, FlopsynthCard, FlopsynthHit, FlopsynthPage, FlopsynthPicture, FlopsynthRoute,
    FlopsynthView, InstrumentGroup, InstrumentParam, KnobSize, MatrixHit, ParamKind, PresetBrowse,
    PresetChoice, PresetShelf, PresetsHit, RING_GAP, SCALES, badge_at, cell_anatomy, cell_span,
    env_curve_points, env_node_at, env_node_drag, filter_xy_at, flopsynth_hit, flopsynth_layout,
    flopsynth_tab_at, lfo_curve_points, matrix_depth_at, matrix_hit, preset_page_rows,
    preset_shelves, presets_hit, ring_depth, ring_hit, wave_position_at,
};
use fontelle_ui::layout::Rect;
use fontelle_ui::theme::Theme;

/// A body the fixtures fit: the design width, and the design height's
/// worth of room for three one-row bands under the canopy.
const BODY: Rect = Rect::new(0.0, 0.0, 1180.0, 840.0);
/// And one smaller than the page — which the window refuses to be, and the
/// layout still answers for.
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
                sizes: Vec::new(),
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
                sizes: Vec::new(),
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
                sizes: Vec::new(),
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
                sizes: Vec::new(),
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
                sizes: Vec::new(),
                picture: FlopsynthPicture::Envelope(fontelle_ui::canvas::EnvelopePicture {
                    attack: 0.1,
                    decay: 0.3,
                    sustain: 0.8,
                    release: 0.4,
                    ..Default::default()
                }),
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
                sizes: Vec::new(),
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
    let pic = |attack: f32, decay: f32, sustain: f32, release: f32| {
        fontelle_ui::canvas::EnvelopePicture {
            attack,
            decay,
            sustain,
            release,
            ..Default::default()
        }
    };
    let points = env_curve_points(rect, &pic(0.1, 0.3, 0.6, 0.4));
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
    let quick = env_curve_points(rect, &pic(0.005, 0.3, 0.6, 0.4));
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
/// Nothing shrinks (§3.1). The shrink cascade of v0.9.0 — air, then the
/// pictures, then every cell to a floor — is gone: a body too small for the
/// page is a window the layout's caller refuses to open, and a layout asked
/// for one anyway keeps every cell and picture at its scale, running off
/// the bottom rather than lying about the knobs.
#[test]
fn a_small_body_does_not_shrink_the_cells_or_the_pictures() {
    let view = a_view();
    let big = flopsynth_layout(BODY, &metrics(), &view);
    let small = flopsynth_layout(SMALL, &metrics(), &view);
    let cell = |l: &fontelle_ui::canvas::FlopsynthLayout| l.cards[0].cells[0].1;
    assert!((cell(&small).height - cell(&big).height).abs() < 0.01);
    assert!((cell(&small).width - cell(&big).width).abs() < 0.01);
    let picture = |l: &fontelle_ui::canvas::FlopsynthLayout| l.cards[0].picture.height;
    assert!((picture(&small) - picture(&big)).abs() < 0.01);
    assert!((small.canopy.height - big.canopy.height).abs() < 0.01);
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

fn an_envelope(
    attack: f32,
    decay: f32,
    sustain: f32,
    release: f32,
) -> fontelle_ui::canvas::EnvelopePicture {
    fontelle_ui::canvas::EnvelopePicture {
        attack,
        decay,
        sustain,
        release,
        ..Default::default()
    }
}

#[test]
fn an_envelope_node_is_where_its_corner_is() {
    // The four nodes are the four corners of the drawn curve, so a person
    // aims at what they can see. Read off `env_curve_points` rather than
    // computed a second way — two answers to "where is the decay node" is
    // exactly the defect that makes a handle you cannot grab.
    let picture = a_picture();
    let pic = an_envelope(0.3, 0.4, 0.6, 0.5);
    let points = env_curve_points(picture, &pic);
    for (node, at) in [
        (EnvNode::Attack, points[1]),
        (EnvNode::Decay, points[2]),
        (EnvNode::Sustain, points[3]),
        (EnvNode::Release, points[4]),
    ] {
        assert_eq!(
            env_node_at(picture, &pic, at.0, at.1),
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
    let pic = an_envelope(0.3, 0.4, 0.6, 0.5);
    let points = env_curve_points(picture, &pic);
    assert_eq!(env_node_at(picture, &pic, points[0].0, points[0].1), None);
}

#[test]
fn a_press_in_the_open_part_of_an_envelope_hits_nothing() {
    let picture = a_picture();
    assert_eq!(
        env_node_at(
            picture,
            &an_envelope(0.3, 0.4, 0.6, 0.5),
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
    let knob = cell_anatomy(cell, KnobSize::Medium, &ParamKind::Knob, 1.0).control;
    let middle = (knob.x + knob.width / 2.0, knob.y + knob.height / 2.0);
    assert!(!ring_hit(knob, middle.0, middle.1), "the middle turns it");
    // Just outside the groove, level with the spindle.
    let outside = middle.0 + knob.width / 2.0 + RING_GAP;
    assert!(ring_hit(knob, outside, middle.1), "the ring is a band");
    // And well outside it is the cell's own air, which is nothing.
    assert!(!ring_hit(knob, cell.right() + 20.0, middle.1));
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
fn the_synth_page_has_the_badges_too_now_on_the_strip() {
    // The badges used to belong to the page that is *about* modulation; the
    // strip (§3.4) puts them on every page, where a badge dragged onto a
    // knob is what "modulate this" means wherever the knob is.
    let theme = Theme::dark_default();
    let mut view = a_view_on(FlopsynthPage::Synth);
    view.sources = vec!["ENV 1".into()];
    let layout = flopsynth_layout(BODY, &theme.metrics, &view);
    assert_eq!(layout.badges.len(), 1);
    assert!(
        layout
            .strip
            .contains(layout.badges[0].x + 1.0, layout.badges[0].y + 1.0)
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
            ..Default::default()
        },
        FlopsynthRoute {
            source: "LFO 1".into(),
            destination: "OSC A pitch".into(),
            depth: -0.05,
            ..Default::default()
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
    // One card over the matrix, so the page has room for a good many rows
    // and not for forty.
    view.cards.truncate(1);
    view.routes = (0..40)
        .map(|i| FlopsynthRoute {
            source: format!("LFO {}", i % 4 + 1),
            destination: format!("dest {i}"),
            depth: 0.5,
            ..Default::default()
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
    let envelope = || {
        FlopsynthPicture::Envelope(fontelle_ui::canvas::EnvelopePicture {
            attack: 0.1,
            decay: 0.3,
            sustain: 0.8,
            release: 0.4,
            ..Default::default()
        })
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
            ..Default::default()
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
        sizes: Vec::new(),
    }
}

/// `card`, with a knob size per control: `L`, `M` or `S` for each, in
/// order — the way the host declares them (§3.1).
fn sized(mut card: FlopsynthCard, sizes: &str) -> FlopsynthCard {
    card.sizes = sizes
        .chars()
        .filter(|c| !c.is_whitespace())
        .map(|c| match c {
            'L' => KnobSize::Large,
            'S' => KnobSize::Small,
            _ => KnobSize::Medium,
        })
        .collect();
    card
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
/// Two bands since Ty's call of 2026-09-18 (`docs/flopsynth-next.md` §3.5,
/// with the envelopes off the page): the sources, with the sub and the
/// noise set aside; and the filters with the channel's two knobs, the voice
/// and the macros. The envelopes live in the strip's inspector. Sizes as
/// §3.5 lists them: one Large knob per card, the continuous ones Medium,
/// the fine adjustments Small.
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
    // table pos level pan warp amount modfrom unison detune blend width
    // phase random semis fine key route
    const OSC: &str = "M L M S M M M M M S S S M S S M M";
    // on model shape slope cutoff res drive keytrk character
    const FILTER: &str = "M M M M L M M S S";
    FlopsynthView {
        title: "Init".to_string(),
        cards: vec![
            // The channel's own two knobs ride on the Voice card: the aside
            // column stands beside both bands, and five cards did not fit
            // the room it leaves.
            sized(
                card(
                    "Voice",
                    1,
                    false,
                    3,
                    FlopsynthPicture::None,
                    vec![
                        chooser("patch/voice/mode", "mode", &["Poly", "Mono", "Legato"]),
                        knob("patch/voice/polyphony", "voices", 0.5),
                        knob("patch/voice/glide", "glide", 0.0),
                        knob("patch/voice/bend_range", "bend", 0.1),
                        knob("patch/output", "output", 0.7),
                        knob("mixer/gain", "volume", 0.8),
                        knob("mixer/pan", "pan", 0.5),
                    ],
                ),
                "M M M S M M M",
            ),
            sized(card("OSC A", 0, false, 5, wave(), osc_params(0)), OSC),
            sized(card("OSC B", 0, false, 5, wave(), osc_params(1)), OSC),
            sized(card("OSC C", 0, false, 5, wave(), osc_params(2)), OSC),
            sized(
                card("SUB", 0, true, 4, wave(), osc_params(3)),
                "M S M S M S M S S S S S M S S M M",
            ),
            sized(
                card(
                    "NOISE",
                    0,
                    true,
                    4,
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
                "M M S S S M M",
            ),
            sized(
                card("Filter 1", 1, false, 4, response(), filter_params(0)),
                FILTER,
            ),
            sized(
                card("Filter 2", 1, false, 4, response(), filter_params(1)),
                FILTER,
            ),
            card(
                "Macros",
                1,
                false,
                4,
                FlopsynthPicture::None,
                (0..8)
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
    // page was sized to fit, and nothing shrinks.
    let cell = layout.cards[2].cells[1].1;
    assert!(
        (cell.width - FLOP_CELL_W).abs() < 0.01 && (cell.height - FLOP_CELL_H).abs() < 0.01,
        "a control at the default size is {}x{}, not {FLOP_CELL_W}x{FLOP_CELL_H}",
        cell.width,
        cell.height
    );
}

/// §3.2: the window opens at `FLOPSYNTH_SIZE` times its scale, and at every
/// scale the page fits it with every cell, picture and the canopy at the
/// design size times the scale — the scale is the only thing that sizes
/// anything.
#[test]
fn the_whole_synth_page_fits_at_every_scale() {
    for scale in SCALES {
        let mut view = synth_page();
        view.scale = scale;
        let (w, h) = fontelle_ui::layout::flopsynth_window_size(scale);
        let body = real_body((w, h));
        let layout = assert_fits(body, &view);
        let cell = layout.cards[2].cells[1].1;
        assert!(
            (cell.width - FLOP_CELL_W * scale).abs() < 0.01
                && (cell.height - FLOP_CELL_H * scale).abs() < 0.01,
            "at {scale}: a cell is {}x{}",
            cell.width,
            cell.height
        );
        assert!(
            (layout.canopy.height - CANOPY_HEIGHT * scale).abs() < 0.01,
            "at {scale}: the canopy is {}",
            layout.canopy.height
        );
        let picture = layout.cards[2].picture.height;
        assert!(
            (picture - fontelle_ui::canvas::PICTURE_HEIGHT * scale).abs() < 0.01,
            "at {scale}: the picture is {picture}"
        );
        assert!(
            !layout.scale_chip.is_empty()
                && flopsynth_hit(
                    &layout,
                    layout.scale_chip.x + layout.scale_chip.width / 2.0,
                    layout.scale_chip.y + layout.scale_chip.height / 2.0
                ) == Some(FlopsynthHit::Scale),
            "at {scale}: the scale chooser is on the strip and can be pressed"
        );
    }
}

/// A Small knob, a chooser and a switch take **half a cell** and stack two
/// to a column; the Large and Medium knobs take a whole one and come
/// **first**, whatever order the card lists them in (§3.1; `wanted`'s two
/// passes). So the knob a player reaches for is the first thing on the card
/// and a seventeen-control oscillator is three rows tall.
#[test]
fn half_cells_stack_two_to_a_column_and_the_whole_cells_come_first() {
    let view = FlopsynthView {
        cards: vec![sized(
            card(
                "Test",
                0,
                false,
                3,
                FlopsynthPicture::None,
                vec![
                    chooser("patch/a", "mode", &["Poly", "Mono"]),
                    knob("patch/b", "fine", 0.5),
                    knob("patch/c", "cutoff", 0.5),
                    switch("patch/d", "key"),
                    knob("patch/e", "res", 0.5),
                    knob("patch/f", "pan", 0.5),
                ],
            ),
            "M S L M M S",
        )],
        page: FlopsynthPage::Synth,
        ..FlopsynthView::default()
    };
    let layout = flopsynth_layout(BODY, &metrics(), &view);
    let cells = &layout.cards[0].cells;
    let cell = |param: usize| {
        cells
            .iter()
            .find(|(p, _)| *p == param)
            .map(|(_, c)| *c)
            .unwrap()
    };
    // The two whole cells first, on the first row, left to right.
    let (cutoff, res) = (cell(2), cell(4));
    assert!((cutoff.height - FLOP_CELL_H).abs() < 0.01 && (res.height - FLOP_CELL_H).abs() < 0.01);
    assert!(cutoff.x < res.x && (cutoff.y - res.y).abs() < 0.01);
    assert!(
        cutoff.x < cell(0).x || cutoff.y < cell(0).y,
        "the Large knob is before the chooser"
    );
    // The four halves: the first two stacked in the row's third column, the
    // next two along the top of the next row.
    let (mode, fine, key, pan) = (cell(0), cell(1), cell(3), cell(5));
    for half in [mode, fine, key, pan] {
        assert!(
            (half.height - FLOP_CELL_HALF).abs() < 0.01,
            "{half:?} is not a half cell"
        );
    }
    assert!(
        (mode.x - fine.x).abs() < 0.01 && (fine.y - mode.bottom()).abs() < 0.01,
        "{mode:?} over {fine:?}"
    );
    assert!(
        (mode.x - res.right()).abs() < 0.01 && (mode.y - res.y).abs() < 0.01,
        "in the column after the last whole cell"
    );
    assert!((key.x - layout.cards[0].frame.x - fontelle_ui::canvas::CARD_PAD).abs() < 0.01);
    // Reading order: the top tier across the row before the bottom, so pan
    // is beside key, not under it.
    assert!(
        (key.y - cutoff.bottom()).abs() < 0.01,
        "{key:?} under {cutoff:?}"
    );
    assert!(
        (pan.y - key.y).abs() < 0.01 && (pan.x - key.right()).abs() < 0.01,
        "{pan:?} beside {key:?}"
    );
    // Two rows, so the card is two cells tall under its header.
    assert!(
        (layout.cards[0].frame.height
            - (CARD_HEADER + fontelle_ui::canvas::CARD_PAD * 2.0 + 2.0 * FLOP_CELL_H))
            .abs()
            < 0.01
    );
    // And the anatomy of a half knob puts the caption across the top and the
    // knob at the left with its read-out beside it, where a whole cell
    // stacks the three bands.
    let small = cell_anatomy(fine, KnobSize::Small, &ParamKind::Knob, 1.0);
    assert!(small.caption.bottom() <= small.control.y + 0.01);
    assert!(small.control.right() <= small.readout.x + 0.01);
    assert!(
        (small.caption.width - fine.width).abs() < 0.01,
        "the caption has the whole width"
    );
    assert!((small.control.width - fontelle_ui::canvas::KNOB_SMALL).abs() < 0.01);
    let large = cell_anatomy(cutoff, KnobSize::Large, &ParamKind::Knob, 1.0);
    assert!((large.control.width - fontelle_ui::canvas::KNOB_LARGE).abs() < 0.01);
    assert!(large.caption.bottom() <= large.control.y && large.control.bottom() <= large.readout.y);
    assert!(
        cutoff.contains(large.control.x, large.control.y)
            && cutoff.contains(large.control.right() - 0.1, large.control.bottom() - 0.1)
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
        at("Voice").y >= at("Filter 1").y - 0.01 && at("Macros").y >= at("Filter 1").y - 0.01,
        "the voice and the macros are in the second band, not the first"
    );
    // Within a band, the listed order holds.
    assert!(at("OSC A").x < at("OSC B").x && at("OSC B").x < at("OSC C").x);
    assert!(at("Voice").x < at("Filter 1").x && at("Filter 1").x < at("Filter 2").x);
}

#[test]
fn aside_cards_stack_down_the_right_edge_beside_the_bands() {
    // The sub and the noise are sources too, but they are slim and the three
    // oscillators are not: set aside in a column, they stand beside the
    // oscillators *and* the filters, and the page is two bands tall rather
    // than three.
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
    // The second band keeps clear of the column while it is beside it.
    assert!(
        at("Macros").right() <= sub.x + 0.01,
        "the second band keeps clear of the column beside it"
    );
}

#[test]
fn a_card_declares_its_own_width_in_columns() {
    // Sized by what it *is*, not by a count: the oscillator's seventeen
    // controls go five across so three oscillators share a row beside the
    // aside column; the same seventeen on the sub go four across so it can
    // stand aside. Counted in **whole** cells on the first row — the halves
    // in the top tier share its y.
    let view = synth_page();
    let layout = assert_fits(real_body(fontelle_ui::layout::FLOPSYNTH_SIZE), &view);
    let across = |name: &str| {
        let index = view
            .cards
            .iter()
            .position(|c| c.group.name == name)
            .unwrap();
        let cells = &layout.cards[index].cells;
        let whole: Vec<Rect> = cells
            .iter()
            .map(|(_, c)| *c)
            .filter(|c| c.height >= FLOP_CELL_H - 0.01)
            .collect();
        let first_y = whole.iter().map(|c| c.y).fold(f32::MAX, f32::min);
        whole
            .iter()
            .filter(|c| (c.y - first_y).abs() < 0.01)
            .count()
    };
    // Whole cells on the first row: the oscillator's Large and four Mediums
    // fill its five columns; the sub has two Mediums and the rest is halves.
    assert_eq!(across("OSC A"), 5, "five whole cells across");
    assert_eq!(
        across("SUB"),
        1,
        "the sub's level; everything else on it is halves"
    );
    assert_eq!(
        across("Filter 1"),
        3,
        "cutoff, res and drive — the fourth column is halves"
    );
    assert_eq!(across("Voice"), 3);
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
    let table = osc.iter().find(|(p, _)| *p == 0).map(|(_, c)| *c).unwrap();
    let pos = osc.iter().find(|(p, _)| *p == 1).map(|(_, c)| *c).unwrap();
    assert!(
        (table.width - pos.width * 2.0).abs() < 0.01,
        "the table's cell is {} wide and a knob's is {}",
        table.width,
        pos.width
    );
    assert!(
        (table.height - FLOP_CELL_HALF).abs() < 0.01,
        "and a chooser's cell is a half, whatever its width"
    );
}

/// A chooser spans two cells when its widest option, **measured**, would not
/// fit the chip's text room in one — and a caption likewise against the
/// cell. Counting characters said "Bypass" (six) fit and it read "Bypas";
/// "Reverse" read "Revers" and "chorus" "choru"
/// (`docs/flopsynth-next.md` §1.4(7), open since v0.7.0). The window hands
/// the layout the shaper's own widths; without one the layout estimates from
/// the count, which is what the fixture tests run on.
#[test]
fn a_chooser_spans_two_cells_when_its_widest_option_measures_too_wide_for_one() {
    use fontelle_ui::canvas::{CHIP_TEXT_ROOM, cell_span_measured};
    let route = chooser(
        "patch/layer[0]/synth/warp_mode",
        "warp",
        &["Off", "Bend", "Mirror", "Sync", "Quantise", "FM", "RM"],
    );
    // The real widths at the small size: "Quantise" is 47 px, "Reverse"
    // 40.7, "Bypass" 36.1; the chip's room in a 56 px cell holds the last
    // two and not the first.
    let real = |text: &str| -> f32 {
        match text {
            "Quantise" => 47.0,
            "Reverse" => 40.7,
            "Bypass" => 36.1,
            "Mirror" => 34.0,
            "warp" => 24.0,
            _ => 12.0,
        }
    };
    let room = std::hint::black_box(CHIP_TEXT_ROOM);
    assert!(
        (40.7..47.0).contains(&room),
        "the room is {room}: Reverse must fit and Quantise must not"
    );
    assert_eq!(
        cell_span_measured(&route, &real),
        2,
        "Quantise does not fit"
    );
    let bypass = chooser(
        "patch/layer[0]/route",
        "route",
        &["F1", "Bypass", "Reverse"],
    );
    assert_eq!(
        cell_span_measured(&bypass, &real),
        1,
        "Bypass fits — it read \"Bypas\""
    );
    let narrow = |text: &str| -> f32 { if text == "warp" { 24.0 } else { 10.0 } };
    assert_eq!(
        cell_span_measured(&route, &narrow),
        1,
        "in a face where every option is narrow it takes one cell"
    );
    // A caption wider than its cell takes two whatever its kind.
    let knob = knob("patch/voice/glide", "portamento time", 0.2);
    let wide_caption = |text: &str| -> f32 {
        if text == "portamento time" {
            FLOP_CELL_W + 5.0
        } else {
            10.0
        }
    };
    assert_eq!(cell_span_measured(&knob, &wide_caption), 2);
    assert_eq!(cell_span_measured(&knob, &narrow), 1);

    // And the layout takes the measure: the same page laid out with the two
    // faces puts the warp chooser in a double cell under one and a single
    // under the other.
    let mut view = a_view();
    view.cards[0].group.params.push(route.clone());
    let body = real_body(fontelle_ui::layout::FLOPSYNTH_SIZE);
    let last = view.cards[0].group.params.len() - 1;
    let wide = fontelle_ui::canvas::flopsynth_layout_with(body, &metrics(), &view, &real);
    let cell_of = |layout: &fontelle_ui::canvas::FlopsynthLayout| {
        layout.cards[0]
            .cells
            .iter()
            .find(|(p, _)| *p == last)
            .map(|(_, c)| *c)
            .expect("the route has a cell")
    };
    assert!((cell_of(&wide).width - FLOP_CELL_W * 2.0).abs() < 0.01);
    let single = fontelle_ui::canvas::flopsynth_layout_with(body, &metrics(), &view, &narrow);
    assert!((cell_of(&single).width - FLOP_CELL_W).abs() < 0.01);
}

/// The smallest the window may be is the page at its scale — there is no
/// size under it at which the text still fits, because nothing shrinks
/// (`docs/flopsynth-next.md` §3.2). At 100 % that is the size it opens at.
#[test]
fn the_window_refuses_to_be_smaller_than_the_page_at_its_scale() {
    assert_eq!(
        fontelle_ui::layout::flopsynth_window_size(1.0),
        fontelle_ui::layout::FLOPSYNTH_SIZE
    );
    let (w, h) = fontelle_ui::layout::flopsynth_window_size(0.75);
    assert!(w < fontelle_ui::layout::FLOPSYNTH_SIZE.0 && h < fontelle_ui::layout::FLOPSYNTH_SIZE.1);
    let (w, h) = fontelle_ui::layout::flopsynth_window_size(1.5);
    assert!(w > fontelle_ui::layout::FLOPSYNTH_SIZE.0 && h > fontelle_ui::layout::FLOPSYNTH_SIZE.1);
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
        ..Default::default()
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
    // `layout.body` is the band the cards were given, which stops above the
    // matrix; the matrix itself runs down to the strip.
    assert!(
        (layout.matrix.bottom() - (layout.strip.y - CARD_GAP)).abs() < 0.51,
        "the matrix runs down to the strip: {} vs {}",
        layout.matrix.bottom(),
        layout.strip.y
    );
    assert!(!layout.routes[0].frame.is_empty());
}

// ======================================================== the Effects page
//
// §3.6: a **rack**, not a grid — the slots in a column down the left, each
// with its on/off, its wet/dry and its level meter, the `+ effect` under
// them, and the selected slot's card with its picture filling the right.
// The first build drew the cards a preset came with and nothing else, so
// the Init patch's Effects page was an empty sky with no word on it.

fn fx_view(slots: usize, room: bool) -> FlopsynthView {
    use fontelle_ui::canvas::FxRackSlot;
    let mut view = a_view_on(FlopsynthPage::Effects);
    view.rack = (0..slots)
        .map(|i| FxRackSlot {
            name: format!("FX {} \u{b7} Chorus", i + 1),
            enabled: i != 1,
            mix: 0.5,
            level: 0.3,
        })
        .collect();
    view.fx_slot = (slots > 0).then_some(0);
    // The host puts the selected slot's card, and only that, in the view.
    view.cards = view
        .fx_slot
        .map(|i| {
            card(
                &format!("FX {} \u{b7} Chorus", i + 1),
                0,
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
        .into_iter()
        .collect();
    view.fx_room = room;
    view
}

#[test]
fn the_effects_page_is_a_rack_of_slots_beside_the_selected_slots_card() {
    use fontelle_ui::canvas::{FX_RACK_W, FxRackHit, fx_rack_hit, fx_rack_landing};
    let view = fx_view(3, true);
    let layout = flopsynth_layout(BODY, &metrics(), &view);
    assert_eq!(layout.rack.len(), 3);
    let column_right = BODY.x + FX_RACK_W;
    let mut bottom = layout.canopy.bottom();
    for (index, row) in layout.rack.iter().enumerate() {
        assert!(!row.frame.is_empty(), "slot {index} has no row");
        assert!(
            row.frame.right() <= column_right + 0.01,
            "in the column: {:?}",
            row.frame
        );
        assert!(
            row.frame.y >= bottom - 0.01,
            "rows run down: {:?}",
            row.frame
        );
        bottom = row.frame.bottom();
        let mut right = row.frame.x - 0.01;
        for (name, rect, hit) in [
            ("grip", row.grip, FxRackHit::Grip(index)),
            ("power", row.power, FxRackHit::Power(index)),
            ("name", row.name, FxRackHit::Select(index)),
            ("mix", row.mix, FxRackHit::Mix(index)),
        ] {
            assert!(!rect.is_empty(), "slot {index}: no {name}");
            assert!(
                row.frame.contains(rect.x + 1.0, rect.y + 1.0),
                "slot {index}: {name} is outside its row"
            );
            assert!(
                rect.x >= right - 0.01,
                "slot {index}: {name} overlaps what is before it"
            );
            right = rect.right();
            assert_eq!(
                fx_rack_hit(
                    &layout,
                    rect.x + rect.width / 2.0,
                    rect.y + rect.height / 2.0
                ),
                Some(hit),
                "slot {index}: {name}"
            );
        }
        assert!(!row.meter.is_empty() && row.frame.contains(row.meter.x + 1.0, row.meter.y + 1.0));
    }
    // The `+ effect` under the last row, still in the column.
    let button = layout.add_effect;
    assert!(!button.is_empty());
    assert!(
        button.y >= bottom - 0.01 && button.right() <= column_right + 0.01,
        "{button:?}"
    );
    assert_eq!(
        flopsynth_hit(
            &layout,
            button.x + button.width / 2.0,
            button.y + button.height / 2.0
        ),
        Some(FlopsynthHit::AddEffect)
    );
    // The selected slot's card fills the right of the column, its whole
    // height, with room for a picture.
    assert_eq!(layout.cards.len(), 1);
    let card = layout.cards[0].frame;
    assert!(
        card.x >= column_right + CARD_GAP - 0.01,
        "beside the rack: {card:?}"
    );
    assert!(card.right() <= BODY.right() + 0.01);
    assert!(card.width >= 600.0, "{card:?}");
    assert!(layout.cards[0].cells.len() == 4);
    // Where a dragged row lands.
    assert_eq!(
        fx_rack_landing(&layout, layout.rack[1].frame.y + 2.0),
        Some(1)
    );
    assert_eq!(
        fx_rack_landing(&layout, layout.rack[2].frame.bottom() + 6.0),
        Some(3)
    );
    assert_eq!(fx_rack_landing(&layout, layout.canopy.y), None);

    // A full chain has nothing to offer, and says so by offering nothing.
    let full = flopsynth_layout(BODY, &metrics(), &fx_view(8, false));
    assert_eq!(full.rack.len(), 8);
    assert!(full.add_effect.is_empty());
    assert!(
        full.rack
            .iter()
            .all(|r| !r.frame.is_empty() && r.frame.bottom() <= full.strip.y + 0.01),
        "eight rows fit above the strip"
    );

    // No effects at all: the button is the first thing in the column, and
    // there is no card.
    let empty = flopsynth_layout(BODY, &metrics(), &fx_view(0, true));
    assert!(!empty.add_effect.is_empty());
    assert!(empty.add_effect.y < empty.canopy.bottom() + CARD_GAP + 1.0);
    assert!(empty.cards.is_empty() && empty.rack.is_empty());

    // And nothing of this on the Synth page.
    let synth = flopsynth_layout(BODY, &metrics(), &a_view());
    assert!(synth.add_effect.is_empty() && synth.rack.is_empty());
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
            (canopy.height - CANOPY_HEIGHT).abs() < 0.01,
            "{:?}: the canopy is the same height on every page: {canopy:?}",
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

/// A taller window is more air under the consoles, not a taller sky: the
/// canopy is the instrument's eyes (§3.2) and the same picture whatever the
/// window's height — so the scope and the spectrum are always where a
/// player left them.
#[test]
fn a_taller_window_keeps_the_canopy_the_same_height() {
    let view = synth_page();
    let short = flopsynth_layout(real_body((1180, 840)), &metrics(), &view);
    let tall = flopsynth_layout(real_body((1180, 1000)), &metrics(), &view);
    assert!((tall.canopy.height - short.canopy.height).abs() < 0.01);
    assert!((tall.canopy.height - CANOPY_HEIGHT).abs() < 0.01);
    // The Presets page too — it used to give the canopy nothing.
    let mut presets = synth_page();
    presets.page = FlopsynthPage::Presets;
    let layout = flopsynth_layout(real_body((1180, 840)), &metrics(), &presets);
    assert!((layout.canopy.height - CANOPY_HEIGHT).abs() < 0.01);
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

// ------------------------------------------------------- the canopy's eyes
//
// `docs/flopsynth-next.md` §3.2, Ty's decision §9.1: the canopy is the
// instrument's eyes — the oscilloscope, the spectrum with the filters'
// responses over it, the voice lamps — with the sky behind and the planets
// gone. The geometry is pure; the renderer draws into it.

#[test]
fn the_canopy_holds_a_scope_a_spectrum_and_the_lamps_left_to_right() {
    use fontelle_ui::canvas::{canopy_eyes, lamp_dots, spectrum_bars};
    let canopy = Rect::new(8.0, 64.0, 1164.0, 120.0);
    let eyes = canopy_eyes(canopy, 1.0);
    for (name, rect) in [
        ("scope", eyes.scope),
        ("spectrum", eyes.spectrum),
        ("lamps", eyes.lamps),
    ] {
        assert!(!rect.is_empty(), "{name} has no room");
        assert!(
            rect.x >= canopy.x
                && rect.right() <= canopy.right() + 0.01
                && rect.y >= canopy.y
                && rect.bottom() <= canopy.bottom() + 0.01,
            "{name} is outside the canopy: {rect:?}"
        );
    }
    assert!(eyes.scope.right() <= eyes.spectrum.x && eyes.spectrum.right() <= eyes.lamps.x);
    assert!(
        eyes.spectrum.width > eyes.scope.width * 0.8,
        "the spectrum is the wide one"
    );
    // Scaled, the same picture at the scale.
    let big = canopy_eyes(Rect::new(8.0, 64.0, 1746.0, 180.0), 1.5);
    assert!((big.lamps.width - eyes.lamps.width * 1.5).abs() < 0.5);

    // The spectrum: one bar per band, across the width, taller the louder,
    // nothing at the floor.
    let bands: Vec<f32> = (0..96)
        .map(|i| {
            if i == 10 {
                -6.0
            } else if i == 50 {
                -30.0
            } else {
                -90.0
            }
        })
        .collect();
    let bars = spectrum_bars(eyes.spectrum, &bands);
    assert_eq!(bars.len(), 96);
    assert!(bars[10].height > bars[50].height && bars[50].height > 0.0);
    assert!(bars[0].height < 0.01, "sixty decibels down is the floor");
    assert!(
        (bars[10].bottom() - eyes.spectrum.bottom()).abs() < 0.01,
        "bars stand on the floor"
    );
    assert!(bars[95].right() <= eyes.spectrum.right() + 0.01);
    assert!(bars[1].x >= bars[0].right() - 0.01, "left to right");

    // The lamps: one per voice of the polyphony, the first `sounding` lit.
    let dots = lamp_dots(eyes.lamps, 32, 1.0);
    assert_eq!(dots.len(), 32);
    assert!(
        dots.iter()
            .all(|d| eyes.lamps.contains(d.x + 0.5, d.y + 0.5))
    );
    assert!(dots[1].x > dots[0].x, "along a row");
    let taller = lamp_dots(eyes.lamps, 8, 1.0);
    assert_eq!(taller.len(), 8);
}

// ---------------------------------------------------- the rings per source
//
// §3.3: one band per route, stacked outward from the knob, each in its
// source's colour; a press on a band edits that route; a moving source
// draws a dot on its band at the live value.

#[test]
fn the_rings_stack_outward_one_band_per_route_and_a_press_names_its_band() {
    use fontelle_ui::canvas::{ring_bands, ring_dot, ring_hit_index};
    let knob = Rect::new(100.0, 100.0, 32.0, 32.0);
    let centre = (116.0, 116.0);
    for count in 1..=3 {
        let bands = ring_bands(knob, count);
        assert_eq!(bands.len(), count);
        for (i, (inner, thick)) in bands.iter().enumerate() {
            assert!(*inner > knob.width / 2.0, "band {i} is outside the groove");
            assert!(*thick > 0.0);
            if i > 0 {
                let (prev_inner, prev_thick) = bands[i - 1];
                assert!(
                    *inner >= prev_inner + prev_thick,
                    "band {i} is outside band {}",
                    i - 1
                );
            }
        }
        // A press on the middle of band `i`, level with the spindle, names
        // band `i`; on the groove, none; well outside, none.
        for (i, (inner, thick)) in bands.iter().enumerate() {
            let x = centre.0 + inner + thick / 2.0;
            assert_eq!(ring_hit_index(knob, x, centre.1, count), Some(i));
        }
        assert_eq!(ring_hit_index(knob, centre.0, centre.1, count), None);
        assert_eq!(
            ring_hit_index(knob, centre.0 + 200.0, centre.1, count),
            None
        );
    }
    // A point on a band, by the knob's own sweep: half way is twelve
    // o'clock, above the spindle; the end has swung clockwise, to the right
    // and below the top; the start is to the left.
    let bands = ring_bands(knob, 2);
    let top = ring_dot(knob, bands[1], 0.5);
    assert!((top.0 - centre.0).abs() < 0.01 && top.1 < centre.1);
    let right = ring_dot(knob, bands[1], 1.0);
    assert!(right.0 > centre.0 + 5.0 && right.1 > top.1);
    let left = ring_dot(knob, bands[1], 0.0);
    assert!(left.0 < centre.0 - 5.0);
    // A dot on the outer band is further out than one on the inner.
    let inner_dot = ring_dot(knob, bands[0], 0.5);
    assert!(inner_dot.1 > top.1);

    // The band's arc is the route's **range** (§3.3): from the knob's own
    // value out by the depth — a unipolar source (an envelope, a macro)
    // pushes one way, a bipolar one (an LFO) both — and clamped to the
    // travel, and the live dot sits where the source is now inside it.
    use fontelle_ui::canvas::{ring_live, ring_range};
    let close = |a: (f32, f32), b: (f32, f32)| (a.0 - b.0).abs() < 1e-5 && (a.1 - b.1).abs() < 1e-5;
    assert!(close(ring_range(0.3, 0.5, false), (0.3, 0.8)));
    assert!(
        close(ring_range(0.3, -0.5, false), (0.0, 0.3)),
        "a negative depth pulls down, to the floor"
    );
    assert!(close(ring_range(0.3, 0.5, true), (0.0, 0.8)));
    assert!(close(ring_range(0.9, 0.5, true), (0.4, 1.0)));
    assert!(
        (ring_live(0.3, 0.5, 1.0) - 0.8).abs() < 1e-6,
        "a source at full is at the range's end"
    );
    assert!(
        (ring_live(0.3, 0.5, 0.0) - 0.3).abs() < 1e-6,
        "a source at nothing is at the value"
    );
    assert!(
        (ring_live(0.3, 0.5, -1.0)).abs() < 1e-6,
        "a bipolar source at its bottom, clamped"
    );
}

// ------------------------------------------------- the strip and the inspector
//
// `docs/flopsynth-next.md` §3.4: a band across the bottom of **every** page
// with a badge per modulation source — drag one onto any knob on any page
// to route; click one to open its editor in the inspector, a drawer over
// the page above the strip. The Modulation page's badge rows were the
// strip's first draft, on one page.

fn twenty_one_sources() -> Vec<String> {
    let mut sources: Vec<String> = (1..=4).map(|i| format!("ENV {i}")).collect();
    sources.extend((1..=4).map(|i| format!("LFO {i}")));
    sources.extend(["Brightness", "Hardness", "M3", "M4"].map(String::from));
    sources.extend(
        [
            "Velocity",
            "Key",
            "Aftertouch",
            "Wheel",
            "Bend",
            "Random",
            "Counter",
            "Note X",
            "Note Y",
        ]
        .map(String::from),
    );
    sources
}

#[test]
fn the_strip_runs_along_the_foot_of_every_page_with_a_badge_per_source() {
    use fontelle_ui::canvas::STRIP_HEIGHT;
    let body = real_body(fontelle_ui::layout::FLOPSYNTH_SIZE);
    for page in FlopsynthPage::ALL {
        let mut view = match page {
            FlopsynthPage::Synth => synth_page(),
            FlopsynthPage::Effects => fx_view(2, true),
            FlopsynthPage::Presets => presets_view(PresetBrowse::default()),
            FlopsynthPage::Modulation => a_view_on(page),
        };
        view.page = page;
        view.sources = twenty_one_sources();
        let layout = flopsynth_layout(body, &metrics(), &view);
        let strip = layout.strip;
        assert!(!strip.is_empty(), "{page:?}: no strip");
        assert!(
            (strip.bottom() - body.bottom()).abs() < 0.51
                && (strip.height - STRIP_HEIGHT).abs() < 0.01,
            "{page:?}: the strip is along the foot: {strip:?} in {body:?}"
        );
        assert_eq!(layout.badges.len(), 21, "{page:?}: a badge per source");
        for (index, badge) in layout.badges.iter().enumerate() {
            assert!(!badge.is_empty(), "{page:?}: badge {index} has no room");
            assert!(
                badge.y >= strip.y - 0.01 && badge.bottom() <= strip.bottom() + 0.01,
                "{page:?}: badge {index} is outside the strip"
            );
            assert_eq!(
                badge_at(
                    &layout,
                    badge.x + badge.width / 2.0,
                    badge.y + badge.height / 2.0
                ),
                Some(index)
            );
            if index > 0 {
                assert!(
                    badge.x >= layout.badges[index - 1].right() - 0.01,
                    "left to right"
                );
            }
        }
        // Everything on the page keeps clear of the strip.
        for (index, placed) in layout.cards.iter().enumerate() {
            assert!(
                placed.frame.is_empty() || placed.frame.bottom() <= strip.y + 0.01,
                "{page:?}: {} runs into the strip",
                view.cards[index].group.name
            );
        }
        if page == FlopsynthPage::Modulation {
            assert!(
                layout.matrix.bottom() <= strip.y + 0.01,
                "the matrix keeps clear"
            );
        }
        if page == FlopsynthPage::Presets {
            assert!(
                layout.presets.list.bottom() <= strip.y + 0.01,
                "the list keeps clear"
            );
        }
    }
}

#[test]
fn the_inspector_is_a_drawer_over_the_page_above_the_strip_and_is_hit_first() {
    use fontelle_ui::canvas::INSPECTOR_ROW;
    let body = real_body(fontelle_ui::layout::FLOPSYNTH_SIZE);
    let mut view = synth_page();
    view.sources = twenty_one_sources();
    // The inspected source's card, marked as the inspector's: the LFO's
    // controls, eight across so it is one row.
    let lfo = sized(
        card(
            "LFO 1",
            INSPECTOR_ROW,
            false,
            8,
            FlopsynthPicture::Lfo {
                points: vec![0.0; 64],
                phase: 0.0,
            },
            vec![
                chooser("patch/lfo[0]/wave", "WAVE", &["sine", "tri"]),
                knob("patch/lfo[0]/rate", "RATE", 0.5),
                knob("patch/lfo[0]/depth", "DEPTH", 0.5),
                knob("patch/lfo[0]/phase", "PHASE", 0.0),
            ],
        ),
        "M L M S",
    );
    view.cards.push(lfo);
    view.inspector = Some(4);
    let index = view.cards.len() - 1;
    let layout = flopsynth_layout(body, &metrics(), &view);
    let drawer = layout.inspector;
    assert!(!drawer.is_empty(), "the drawer is open");
    assert!(
        (drawer.bottom() - layout.strip.y).abs() < fontelle_ui::canvas::CARD_GAP + 0.01,
        "above the strip"
    );
    assert!(
        (drawer.x - body.x).abs() < 0.01 && (drawer.right() - body.right()).abs() < 0.01,
        "full width"
    );
    let placed = &layout.cards[index];
    assert!(
        !placed.frame.is_empty() && drawer.contains(placed.frame.x + 1.0, placed.frame.y + 1.0)
    );
    assert!(placed.frame.bottom() <= drawer.bottom() + 0.01);
    // The inspected card is as wide as the drawer, whatever columns the
    // host declared: the editor of one source has the whole width to
    // itself, which is what the LFO's and the envelope's pictures want.
    assert!(
        placed.frame.width
            >= drawer.width
                - fontelle_ui::canvas::FLOP_CELL_W
                - 2.0 * fontelle_ui::canvas::CARD_PAD,
        "the card is {} wide in a drawer {} wide",
        placed.frame.width,
        drawer.width
    );
    assert!(
        !layout.inspector_close.is_empty()
            && drawer.contains(
                layout.inspector_close.x + 1.0,
                layout.inspector_close.y + 1.0
            )
    );
    // The drawer lies over the page's second band; a press on one of its
    // cells is the inspector's control, not the card underneath.
    let (param, cell) = placed.cells[1];
    let under = layout
        .cards
        .iter()
        .enumerate()
        .filter(|(i, c)| *i != index && c.frame.contains(cell.x + 1.0, cell.y + 1.0))
        .count();
    assert!(under >= 1, "the drawer covers a card: {cell:?}");
    assert_eq!(
        flopsynth_hit(
            &layout,
            cell.x + cell.width / 2.0,
            cell.y + cell.height / 2.0
        ),
        Some(FlopsynthHit::Control { card: index, param })
    );
    assert_eq!(
        flopsynth_hit(
            &layout,
            layout.inspector_close.x + 2.0,
            layout.inspector_close.y + 2.0
        ),
        Some(FlopsynthHit::InspectorClose)
    );
    // Closed, the same view lays out with no drawer and the card unplaced.
    view.inspector = None;
    view.cards.pop();
    let closed = flopsynth_layout(body, &metrics(), &view);
    assert!(closed.inspector.is_empty());
}

#[test]
fn on_the_matrix_page_the_inspector_is_the_top_of_the_page_and_the_table_the_rest() {
    // §3.4: the Matrix page is "the inspector plus the full table". The
    // drawer that lies over the foot of every other page sits under the
    // canopy here, and the table takes what is left — the page has no
    // cards of its own to cover.
    use fontelle_ui::canvas::{FlopsynthPage, INSPECTOR_ROW};
    let body = real_body(fontelle_ui::layout::FLOPSYNTH_SIZE);
    let mut view = a_view_on(FlopsynthPage::Modulation);
    // The host sends this page no cards of its own.
    view.cards.clear();
    view.sources = twenty_one_sources();
    view.routes = (0..30)
        .map(|i| FlopsynthRoute {
            source: "LFO 1".into(),
            destination: format!("Filter 1 cutoff {i}"),
            depth: 0.3,
            ..Default::default()
        })
        .collect();
    view.cards.push(sized(
        card(
            "LFO 1",
            INSPECTOR_ROW,
            false,
            8,
            FlopsynthPicture::Lfo {
                points: vec![0.0; 64],
                phase: 0.0,
            },
            vec![
                chooser("patch/lfo[0]/wave", "WAVE", &["sine", "tri"]),
                knob("patch/lfo[0]/rate", "RATE", 0.5),
            ],
        ),
        "M L",
    ));
    view.inspector = Some(4);
    let layout = flopsynth_layout(body, &metrics(), &view);
    let drawer = layout.inspector;
    assert!(!drawer.is_empty());
    assert!(
        drawer.y < layout.canopy.bottom() + fontelle_ui::canvas::STRIP_HEIGHT,
        "under the canopy, not over the strip: {drawer:?} vs {:?}",
        layout.canopy
    );
    assert!(
        layout.matrix.y >= drawer.bottom() - 0.01,
        "the table starts under the drawer: {:?} vs {drawer:?}",
        layout.matrix
    );
    assert!(
        layout.matrix.bottom() <= layout.strip.y + 0.01,
        "and ends above the strip"
    );
    assert!(
        layout.matrix.height > 100.0,
        "with room for rows: {:?}",
        layout.matrix
    );
    let shown: Vec<_> = layout
        .routes
        .iter()
        .filter(|r| !r.frame.is_empty())
        .collect();
    assert!(shown.len() >= 4, "{} rows showing", shown.len());
    assert!(shown.iter().all(|r| r.frame.y >= drawer.bottom() - 0.01));
    assert!(layout.matrix_max_scroll > 0.0, "the rest scroll");
}

#[test]
fn a_badge_caption_is_shortened_with_an_ellipsis_to_fit_its_badge() {
    use fontelle_ui::canvas::{BADGE_W, badge_anatomy, badge_caption};
    // Six pixels a glyph: "Brightness" is sixty, a badge's name band is
    // forty-six, and "Bright…" is forty-two.
    let measure = |s: &str| s.chars().count() as f32 * 6.0;
    let badge = fontelle_ui::layout::Rect::new(0.0, 0.0, BADGE_W, 44.0);
    let room = badge_anatomy(badge, 1.0).name.width;
    assert_eq!(badge_caption("ENV 1", room, &measure), "ENV 1");
    assert_eq!(
        badge_caption("Brightness", room, &measure),
        "Bright\u{2026}"
    );
    // Trailing spaces go before the ellipsis, and a name that fits is
    // itself, whatever the room.
    assert_eq!(
        badge_caption("Note X Y Z", 42.0, &measure),
        "Note X\u{2026}"
    );
    assert_eq!(badge_caption("M1", 5.0, &measure), "M\u{2026}");
}

// ------------------------------------------------------ the full table (§3.4)

/// §3.4's table: every row has a grip to drag it by, a source, a
/// destination, the depth slider, a via, a curve, an invert switch and an
/// on/off switch, then the ✕ — left to right, none overlapping, each hit
/// by its own name; the header names the columns, the source and
/// destination heads sort, and a `+` on the header adds a row.
#[test]
fn every_row_of_the_table_has_its_eight_cells_and_the_header_sorts_and_adds() {
    let theme = Theme::dark_default();
    let mut view = a_view_on(FlopsynthPage::Modulation);
    view.cards.clear();
    view.routes = vec![
        FlopsynthRoute {
            source: "ENV 2".into(),
            destination: "Filter 1 cutoff".into(),
            depth: 0.62,
            via: Some("Wheel".into()),
            curve: "Linear".into(),
            invert: false,
            bypass: false,
        },
        FlopsynthRoute {
            source: "LFO 1".into(),
            destination: "OSC A pitch".into(),
            depth: -0.05,
            via: None,
            curve: "S-curve".into(),
            invert: true,
            bypass: true,
        },
    ];
    let layout = flopsynth_layout(BODY, &theme.metrics, &view);
    assert_eq!(layout.routes.len(), 2);
    let centre = |r: Rect| (r.x + r.width / 2.0, r.y + r.height / 2.0);
    for (index, row) in layout.routes.iter().enumerate() {
        let cells = [
            ("grip", row.grip, MatrixHit::Grip(index)),
            ("source", row.source, MatrixHit::Source(index)),
            (
                "destination",
                row.destination,
                MatrixHit::Destination(index),
            ),
            ("depth", row.depth, MatrixHit::Depth(index)),
            ("via", row.via, MatrixHit::Via(index)),
            ("curve", row.curve, MatrixHit::Curve(index)),
            ("invert", row.invert, MatrixHit::Invert(index)),
            ("bypass", row.bypass, MatrixHit::Bypass(index)),
            ("remove", row.remove, MatrixHit::Remove(index)),
        ];
        let mut right = row.frame.x - 0.01;
        for (name, rect, hit) in cells {
            assert!(!rect.is_empty(), "row {index}: no {name}");
            assert!(
                rect.x >= right - 0.01,
                "row {index}: {name} at {rect:?} overlaps the cell before it, which ends at {right}"
            );
            assert!(
                rect.y >= row.frame.y - 0.01 && rect.bottom() <= row.frame.bottom() + 0.01,
                "row {index}: {name} is outside its row"
            );
            let (x, y) = centre(rect);
            assert_eq!(matrix_hit(&layout, x, y), Some(hit), "row {index}: {name}");
            right = rect.right();
        }
        assert!(
            right <= row.frame.right() + 0.01,
            "row {index} runs past its frame"
        );
        assert!(
            row.source.width >= 60.0 && row.destination.width >= row.source.width,
            "the names have room: {:?} {:?}",
            row.source,
            row.destination
        );
    }
    // The header: a head over each column, the two that sort hit by name,
    // and the `+` at the right end.
    let head = &layout.matrix_header;
    for (name, rect) in [
        ("source", head.source),
        ("destination", head.destination),
        ("depth", head.depth),
        ("via", head.via),
        ("curve", head.curve),
        ("invert", head.invert),
        ("bypass", head.bypass),
        ("add", head.add),
    ] {
        assert!(!rect.is_empty(), "no {name} head");
        assert!(
            rect.y >= layout.matrix.y - 0.01
                && rect.bottom() <= layout.matrix.y + CARD_HEADER + 0.01,
            "{name} head is not in the header"
        );
    }
    assert!(
        (head.source.x - layout.routes[0].source.x).abs() < 0.01,
        "the head is over its column"
    );
    assert!((head.via.x - layout.routes[0].via.x).abs() < 0.01);
    let (x, y) = centre(head.source);
    assert_eq!(matrix_hit(&layout, x, y), Some(MatrixHit::SortSource));
    let (x, y) = centre(head.destination);
    assert_eq!(matrix_hit(&layout, x, y), Some(MatrixHit::SortDestination));
    let (x, y) = centre(head.add);
    assert_eq!(matrix_hit(&layout, x, y), Some(MatrixHit::Add));
    assert!(head.add.right() <= layout.matrix.right() + 0.01);
    // Between rows is nothing.
    assert_eq!(
        matrix_hit(
            &layout,
            layout.matrix.x + 2.0,
            layout.routes[0].frame.bottom() + 0.5
        ),
        None
    );
}

/// A row dragged by its grip lands on the row under the pointer, or after
/// the last one below them all — `route_landing` says which, from the
/// pointer's height alone, so a drag past the last row still has an answer.
#[test]
fn a_row_dragged_by_its_grip_lands_on_the_row_under_the_pointer() {
    use fontelle_ui::canvas::route_landing;
    let theme = Theme::dark_default();
    let mut view = a_view_on(FlopsynthPage::Modulation);
    view.cards.clear();
    view.routes = (0..5)
        .map(|i| FlopsynthRoute {
            source: "LFO 1".into(),
            destination: format!("dest {i}"),
            depth: 0.5,
            ..Default::default()
        })
        .collect();
    let layout = flopsynth_layout(BODY, &theme.metrics, &view);
    let row = |i: usize| layout.routes[i].frame;
    assert_eq!(route_landing(&layout, row(0).y + 2.0), Some(0));
    assert_eq!(
        route_landing(&layout, row(3).y + row(3).height / 2.0),
        Some(3)
    );
    // Below the last row, still inside the panel: after the last.
    assert_eq!(route_landing(&layout, row(4).bottom() + 10.0), Some(5));
    // Above the first row is the first; outside the panel is nowhere.
    assert_eq!(route_landing(&layout, row(0).y - 2.0), Some(0));
    assert_eq!(route_landing(&layout, layout.matrix.y - 10.0), None);
    assert_eq!(route_landing(&layout, layout.matrix.bottom() + 10.0), None);
}

// ------------------------------------------------- the editors (§3.4, step 7)

/// The envelope editor: each bent stage is drawn bent, the hold is a
/// plateau at the top, the loop is a span across the stages it names, and
/// every corner and every bend is a handle.
#[test]
fn an_envelope_picture_bends_holds_and_loops_and_its_bends_are_handles() {
    use fontelle_ui::canvas::{EnvelopePicture, env_corners, env_loop_span, env_node_position};
    let rect = Rect::new(100.0, 200.0, 400.0, 100.0);
    let straight = EnvelopePicture {
        attack: 0.3,
        decay: 0.4,
        sustain: 0.6,
        release: 0.5,
        ..Default::default()
    };
    // No hold, no bends: the five corners, as ever.
    let points = env_curve_points(rect, &straight);
    assert_eq!(points.len(), 5);
    let corners = env_corners(rect, &straight);
    assert_eq!(corners.start, points[0]);
    assert_eq!(corners.attack_end, points[1]);
    assert_eq!(corners.hold_end, corners.attack_end, "no hold, no plateau");
    assert_eq!(corners.decay_end, points[2]);
    assert_eq!(corners.sustain_end, points[3]);
    assert_eq!(corners.release_end, points[4]);
    assert_eq!(env_loop_span(rect, &straight), None);

    // A hold: a plateau at the top between the attack's end and the decay's
    // start, and a handle at its end.
    let held = EnvelopePicture {
        hold: 0.3,
        ..straight.clone()
    };
    let corners = env_corners(rect, &held);
    assert!(
        corners.hold_end.0 > corners.attack_end.0 + 5.0,
        "the plateau has width"
    );
    assert!(
        (corners.hold_end.1 - corners.attack_end.1).abs() < 0.01,
        "and is flat"
    );
    assert!((corners.hold_end.1 - rect.y).abs() < 0.51, "at the top");
    let at = env_node_position(rect, &held, EnvNode::Hold).unwrap();
    assert_eq!(env_node_at(rect, &held, at.0, at.1), Some(EnvNode::Hold));

    // A bend: the attack drawn with a positive shape sags under the straight
    // line — slow to leave, then fast — and the handle sits on the curve at
    // the stage's middle, where a drag upward straightens it.
    let bent = EnvelopePicture {
        attack_shape: 0.8,
        ..straight.clone()
    };
    let points = env_curve_points(rect, &bent);
    assert!(points.len() > 5, "a bent stage is drawn as a curve");
    let corners = env_corners(rect, &bent);
    let mid_x = (corners.start.0 + corners.attack_end.0) / 2.0;
    let on_curve = points
        .iter()
        .min_by(|a, b| (a.0 - mid_x).abs().total_cmp(&(b.0 - mid_x).abs()))
        .unwrap();
    let straight_y = (corners.start.1 + corners.attack_end.1) / 2.0;
    assert!(
        on_curve.1 > straight_y + 5.0,
        "sags: {} vs {straight_y}",
        on_curve.1
    );
    let handle = env_node_position(rect, &bent, EnvNode::AttackBend).unwrap();
    assert!((handle.0 - mid_x).abs() < 2.0 && (handle.1 - on_curve.1).abs() < 3.0);
    assert_eq!(
        env_node_at(rect, &bent, handle.0, handle.1),
        Some(EnvNode::AttackBend)
    );
    // The corners still win where they are.
    assert_eq!(
        env_node_at(rect, &bent, corners.attack_end.0, corners.attack_end.1),
        Some(EnvNode::Attack)
    );
    // Dragging a bend: up is a lower shape (the curve rises toward the
    // straight line and past it), a full height is the whole range, and it
    // is the shape's normalised value the picture's control takes.
    let from = (0.8f32 + 1.0) / 2.0;
    let up = env_node_drag(rect, EnvNode::AttackBend, from, 0.0, -rect.height / 4.0);
    assert!((up - (from - 0.25)).abs() < 1e-4, "{up}");
    assert_eq!(
        env_node_drag(rect, EnvNode::DecayBend, 0.5, 0.0, 10_000.0),
        1.0,
        "down is all the way bent"
    );
    assert_eq!(EnvNode::AttackBend.stage(), "attack_shape");
    assert_eq!(EnvNode::Hold.stage(), "hold");

    // The loop: a span from the start of the first stage it names to the
    // end of the second. Stages are numbered as `EnvStage` is: delay 0,
    // attack 1, hold 2, decay 3, sustain 4, release 5.
    let looped = EnvelopePicture {
        loop_stages: Some((1, 3)),
        ..held.clone()
    };
    let corners = env_corners(rect, &looped);
    let (from, to) = env_loop_span(rect, &looped).expect("a loop");
    assert!(
        (from - corners.start.0).abs() < 0.01,
        "from the attack's start"
    );
    assert!(
        (to - corners.decay_end.0).abs() < 0.01,
        "to the decay's end"
    );
}

/// The LFO's shape editor: the curve is read off the shape's own `value`
/// — the function the voice will play — its points are handles, a drag
/// snaps to the grid, the middle of a segment is its tension, and a
/// double-click's place is a point.
#[test]
fn an_lfo_shape_is_drawn_from_its_own_value_and_edited_by_its_handles() {
    use fontelle_types::{LfoPoint, LfoShape, LfoShapeMode};
    use fontelle_ui::canvas::{
        LfoShapeHit, lfo_point_add, lfo_point_drag, lfo_shape_curve_points, lfo_shape_hit,
        lfo_shape_point_at, lfo_tension_drag,
    };
    let rect = Rect::new(100.0, 200.0, 400.0, 100.0);
    let pt = |x: f32, y: f32| LfoPoint { x, y, tension: 0.0 };
    let shape = LfoShape {
        points: vec![pt(0.0, -1.0), pt(0.5, 1.0), pt(0.75, 0.0)],
        grid: 4,
        mode: LfoShapeMode::Smooth,
    };
    let curve = lfo_shape_curve_points(rect, &shape);
    assert!(curve.len() >= 64);
    for (x, y) in &curve {
        let phase = (x - rect.x) / rect.width;
        let expected = rect.y + rect.height * (1.0 - (shape.value(phase) + 1.0) / 2.0);
        assert!((y - expected).abs() < 0.6, "at {phase}: {y} vs {expected}");
    }
    // The points, where the picture puts them.
    let at = |i: usize| lfo_shape_point_at(rect, &shape.points[i]);
    assert!((at(1).0 - (rect.x + rect.width * 0.5)).abs() < 0.01);
    assert!((at(1).1 - rect.y).abs() < 0.01, "y = 1 is the top");
    assert_eq!(
        lfo_shape_hit(rect, &shape, at(1).0 + 2.0, at(1).1 + 2.0),
        Some(LfoShapeHit::Point(1))
    );
    // Between two points is the segment before the second.
    let mid = ((at(0).0 + at(1).0) / 2.0, (at(0).1 + at(1).1) / 2.0);
    assert_eq!(
        lfo_shape_hit(rect, &shape, mid.0, mid.1),
        Some(LfoShapeHit::Segment(0))
    );
    // Past the last point is the segment that closes the cycle.
    assert_eq!(
        lfo_shape_hit(
            rect,
            &shape,
            rect.x + rect.width * 0.9,
            rect.y + rect.height / 2.0
        ),
        Some(LfoShapeHit::Segment(2))
    );
    assert_eq!(lfo_shape_hit(rect, &shape, rect.x - 20.0, rect.y), None);
    // A point dragged lands where the pointer is, snapped to the grid in x
    // and clamped to the picture; the first point keeps its x, a point
    // cannot pass its neighbours.
    let (x, y) = lfo_point_drag(
        rect,
        &shape,
        1,
        rect.x + rect.width * 0.28,
        rect.y + rect.height * 0.25,
    );
    assert!((x - 0.25).abs() < 1e-4, "snapped: {x}");
    assert!((y - 0.5).abs() < 1e-4, "{y}");
    let (x, _) = lfo_point_drag(rect, &shape, 0, rect.x + 100.0, rect.y);
    assert_eq!(x, 0.0, "the first point is the cycle's start");
    let (x, _) = lfo_point_drag(rect, &shape, 1, rect.x + rect.width * 0.9, rect.y);
    assert!(x <= 0.75, "not past the next point: {x}");
    let free = LfoShape {
        grid: 0,
        ..shape.clone()
    };
    let (x, _) = lfo_point_drag(rect, &free, 1, rect.x + rect.width * 0.28, rect.y);
    assert!((x - 0.28).abs() < 1e-4, "no grid, no snap: {x}");
    // A tension drag: up bends the segment toward its end sooner (negative
    // tension), a full height is the whole range.
    assert!((lfo_tension_drag(rect, 0.0, -rect.height / 4.0) - (-0.5)).abs() < 1e-4);
    assert_eq!(lfo_tension_drag(rect, 0.0, 10_000.0), 1.0);
    // A point added at the pointer, snapped like a dragged one.
    let (x, y) = lfo_point_add(rect, &shape, rect.x + rect.width * 0.6, rect.bottom());
    assert!((x - 0.5).abs() < 1e-4 || (x - 0.75).abs() < 1e-4, "{x}");
    assert!((y + 1.0).abs() < 1e-4);
}

/// The inspected card's picture is tall: an editor, not a read-out.
#[test]
fn the_inspected_cards_picture_is_tall_enough_to_edit_in() {
    use fontelle_ui::canvas::INSPECTOR_ROW;
    let body = real_body(fontelle_ui::layout::FLOPSYNTH_SIZE);
    let mut view = synth_page();
    view.sources = twenty_one_sources();
    view.cards.push(sized(
        card(
            "LFO 1",
            INSPECTOR_ROW,
            false,
            8,
            FlopsynthPicture::Lfo {
                points: vec![0.0; 64],
                phase: 0.0,
            },
            vec![knob("patch/lfo[0]/rate", "RATE", 0.5)],
        ),
        "L",
    ));
    view.inspector = Some(4);
    let index = view.cards.len() - 1;
    let layout = flopsynth_layout(body, &metrics(), &view);
    let picture = layout.cards[index].picture;
    assert!(picture.height >= 110.0, "{picture:?}");
    assert!(picture.width >= 1000.0, "{picture:?}");
    assert!(layout.inspector.bottom() <= layout.strip.y + 0.01);
}
