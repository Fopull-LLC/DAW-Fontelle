//! What a row carried out of the browser is about to land on — and what that
//! looks like before the button comes up.
//!
//! > *"i cant see any visuals of the thing being dragged when i click and drag
//! > something for example an audio clip from the import section im trying to
//! > drag into the channel rack or playlist to turn into an instrument or clip.
//! > please also ensure that it shows a visual of where its about to go so you
//! > know youre actually placing it right / that is a legal action before you
//! > do it. right now theres virtually no feedback until you actually finish
//! > dragging it."*
//!
//! The drag existed; the *answer* to "where would this go" was computed inside
//! the release handler and nowhere else, so there was nothing to draw with.
//! [`carry_target`] is that answer as a pure function of the geometry, asked
//! once per pointer move to draw the marks and once more on release to make
//! the edit — which is the property that makes the highlight honest rather
//! than decorative: **the drawing and the drop read the same function**.

use fontelle_types::{FolderKind, PPQN};
use fontelle_ui::canvas::{
    BrowserMode, Carried, CarryOscillator, CarryRack, CarryScene, CarryTarget, CarryTimeline,
    SnapDivision, TimelineView, browser_row_carries, carry_chip, carry_note, carry_target,
    rack_layout, timeline_layout,
};
use fontelle_ui::document::{LibraryEntry, LibraryKind};
use fontelle_ui::layout::Rect;
use fontelle_ui::theme::{Metrics, Theme};

fn metrics() -> Metrics {
    Theme::dark_default().metrics
}

/// The window this all happens in: a rack down the left, the arrangement
/// across the top right, the browser under the rack.
fn window() -> Rect {
    Rect::new(0.0, 0.0, 1400.0, 820.0)
}

fn rack_body() -> Rect {
    Rect::new(8.0, 40.0, 240.0, 320.0)
}

fn browser_frame() -> Rect {
    Rect::new(8.0, 368.0, 240.0, 440.0)
}

fn timeline_frame() -> Rect {
    Rect::new(256.0, 40.0, 1136.0, 300.0)
}

fn view() -> TimelineView {
    TimelineView {
        scroll_tick: 0,
        top_lane: 0,
        // A bar is 96 pixels, which is about what an arrangement is read at.
        pixels_per_tick: 0.025,
        lane_height: 34.0,
        snap: SnapDivision::Bar,
    }
}

fn channels() -> Vec<String> {
    vec!["Kick".to_string(), "Snare".to_string(), "Bass".to_string()]
}

/// The whole scene, with everything in it — which is what the window hands
/// [`carry_target`] while the pointer is over the studio itself.
fn scene<'a>(
    carried: Carried,
    rack: &'a fontelle_ui::canvas::RackLayout,
    timeline: &'a fontelle_ui::canvas::TimelineLayout,
    view: &'a TimelineView,
) -> CarryScene<'a> {
    CarryScene {
        carried,
        rack: Some(CarryRack {
            frame: rack_body(),
            layout: rack,
        }),
        panel: Some(browser_frame()),
        timeline: Some(CarryTimeline {
            layout: timeline,
            view,
            beats_per_bar: 4,
            lanes: 2,
        }),
        name: None,
        oscillators: &[],
    }
}

fn mid(rect: Rect) -> (f32, f32) {
    (rect.x + rect.width / 2.0, rect.y + rect.height / 2.0)
}

// ------------------------------------------------------------- the rack ---

#[test]
fn a_sound_over_a_channel_lands_on_that_channel() {
    let rack = rack_layout(rack_body(), &metrics(), 3, 0);
    let timeline = timeline_layout(timeline_frame(), &metrics());
    let v = view();
    let row = rack.rows[1];
    let (x, y) = mid(row.name);
    let target = carry_target(&scene(Carried::Audio, &rack, &timeline, &v), x, y);
    assert_eq!(
        target,
        CarryTarget::Channel {
            index: 1,
            rect: row.frame
        },
        "a sound let go on a row means that row"
    );
    assert!(target.lands(), "and letting go there does something");
    assert_eq!(
        target.mark(),
        Some(row.frame),
        "the whole row lights up, because the whole row is the target"
    );
}

#[test]
fn every_part_of_a_row_is_that_row() {
    // The rule the right-click menu already follows: aiming at a caption is
    // not a thing anybody should have to do. The mute square, the solo
    // square, the edit button and the route chip are all *on* a channel.
    let rack = rack_layout(rack_body(), &metrics(), 3, 0);
    let timeline = timeline_layout(timeline_frame(), &metrics());
    let v = view();
    let row = rack.rows[2];
    for part in [row.name, row.mute, row.solo, row.edit, row.route] {
        let (x, y) = mid(part);
        assert_eq!(
            carry_target(&scene(Carried::Preset, &rack, &timeline, &v), x, y),
            CarryTarget::Channel {
                index: 2,
                rect: row.frame
            },
            "{part:?} is part of channel 2"
        );
    }
}

#[test]
fn the_rack_under_its_rows_makes_a_channel_of_its_own() {
    let rack = rack_layout(rack_body(), &metrics(), 2, 0);
    let timeline = timeline_layout(timeline_frame(), &metrics());
    let v = view();
    // Below the second row, in the empty part of the list.
    let empty = rack.rows[1].frame.bottom() + 20.0;
    let target = carry_target(
        &scene(Carried::Audio, &rack, &timeline, &v),
        rack.list.x + rack.list.width / 2.0,
        empty,
    );
    let CarryTarget::NewChannel { rect } = target else {
        panic!("the empty rack is a new channel, got {target:?}");
    };
    assert!(target.lands());
    assert!(
        rack.list.contains(rect.x + 1.0, rect.y + 1.0)
            && rect.bottom() <= rack.list.bottom() + 0.01,
        "the band showing where the new row goes has to be inside the list: {rect:?} against {:?}",
        rack.list
    );
    assert!(
        rect.y >= rack.rows[1].frame.bottom() - 0.01,
        "and after the rows that are already there"
    );
}

#[test]
fn an_empty_rack_still_takes_the_first_channel() {
    // The case a new project is in: no rows at all, and the drop that makes
    // the first channel is the one that most needs to say it will work.
    let rack = rack_layout(rack_body(), &metrics(), 0, 0);
    let timeline = timeline_layout(timeline_frame(), &metrics());
    let v = view();
    let (x, y) = mid(rack.list);
    let target = carry_target(&scene(Carried::Audio, &rack, &timeline, &v), x, y);
    assert!(
        matches!(target, CarryTarget::NewChannel { .. }),
        "got {target:?}"
    );
    assert!(target.mark().is_some_and(|rect| !rect.is_empty()));
}

// ------------------------------------------------------ the arrangement ---

#[test]
fn a_sound_over_the_arrangement_is_a_clip_at_the_bar_under_the_pointer() {
    let rack = rack_layout(rack_body(), &metrics(), 3, 0);
    let timeline = timeline_layout(timeline_frame(), &metrics());
    let v = view();
    let grid = timeline.grid;
    // Bar 5 is four bars in: 4 * 4 beats * PPQN.
    let bar = PPQN * 4;
    let x = grid.x + (bar * 4) as f32 * v.pixels_per_tick + 7.0;
    let target = carry_target(
        &scene(Carried::Audio, &rack, &timeline, &v),
        x,
        grid.y + grid.height / 2.0,
    );
    let CarryTarget::Clip {
        row,
        at,
        tick,
        lane,
    } = target
    else {
        panic!("a sound let go on the arrangement is a clip, got {target:?}");
    };
    assert!(target.lands());
    assert_eq!(tick, bar * 4, "snapped to the bar it was let go over");
    // The pointer is at mid-grid, which is empty space past the two lanes this
    // scene has — so this is still the make-a-new-row case.
    assert_eq!(
        lane, None,
        "a drop in the empty space past the rows makes a new one"
    );
    assert!(
        (at - (grid.x + (bar * 4) as f32 * v.pixels_per_tick)).abs() < 0.01,
        "the caret is drawn at the tick it will land on, not where the pointer is"
    );
    assert!(
        (row.height - v.lane_height).abs() < 0.01 && grid.contains(row.x + 1.0, row.y + 1.0),
        "the new row is one lane tall, inside the arrangement: {row:?} against {grid:?}"
    );
    assert_eq!(target.mark(), Some(row));
}

#[test]
fn a_sound_dropped_over_an_existing_row_lands_on_that_row() {
    // The report's whole complaint: a drop over a row that is there should land
    // *there*, not always on a new row at the bottom.
    let rack = rack_layout(rack_body(), &metrics(), 3, 0);
    let timeline = timeline_layout(timeline_frame(), &metrics());
    let v = view();
    let grid = timeline.grid;
    // Aim at the middle of row 0 (the scene has two rows).
    let y = fontelle_ui::canvas::lane_to_y(&v, grid, 0) + v.lane_height / 2.0;
    let target = carry_target(
        &scene(Carried::Audio, &rack, &timeline, &v),
        grid.x + 130.0,
        y,
    );
    let CarryTarget::Clip { row, lane, .. } = target else {
        panic!("got {target:?}");
    };
    assert_eq!(
        lane,
        Some(0),
        "the drop lands on the row the pointer is over"
    );
    let row0 = fontelle_ui::canvas::lane_to_y(&v, grid, 0);
    assert!(
        (row.y - row0).abs() < 0.01,
        "the mark is row 0's band, where the clip will go: {row:?}"
    );
}

#[test]
fn the_new_row_is_drawn_under_the_rows_that_are_already_there() {
    // `AddAudioClip` puts the clip on a **new lane past the bottom of the
    // stack**, so that is where the mark goes: the row after the last one.
    // Drawn at the foot of the *grid* instead — which is what the recording
    // band does — it would sit six rows below the arrangement in a project
    // with two lanes and point at nothing.
    let rack = rack_layout(rack_body(), &metrics(), 3, 0);
    let timeline = timeline_layout(timeline_frame(), &metrics());
    let v = view();
    let target = carry_target(
        &scene(Carried::Audio, &rack, &timeline, &v),
        mid(timeline.grid).0,
        mid(timeline.grid).1,
    );
    let CarryTarget::Clip { row, .. } = target else {
        panic!("got {target:?}");
    };
    let after = fontelle_ui::canvas::lane_to_y(&v, timeline.grid, 2);
    assert!(
        (row.y - after).abs() < 0.01,
        "the mark should be lane 2's row (two lanes in the project): {row:?}"
    );
}

#[test]
fn a_full_arrangement_lands_the_drop_on_the_visible_row_under_the_pointer() {
    // A project with more lanes than the grid can show: every point in the grid
    // is over a row that exists, so the drop lands on the one under the pointer
    // rather than on a new row off the bottom.
    let rack = rack_layout(rack_body(), &metrics(), 3, 0);
    let timeline = timeline_layout(timeline_frame(), &metrics());
    let v = view();
    let mut scene = scene(Carried::Audio, &rack, &timeline, &v);
    scene.timeline = Some(CarryTimeline {
        layout: &timeline,
        view: &v,
        beats_per_bar: 4,
        lanes: 40,
    });
    let grid = timeline.grid;
    // The middle of row 3.
    let y = fontelle_ui::canvas::lane_to_y(&v, grid, 3) + v.lane_height / 2.0;
    let target = carry_target(&scene, mid(grid).0, y);
    let CarryTarget::Clip { row, lane, .. } = target else {
        panic!("got {target:?}");
    };
    assert_eq!(
        lane,
        Some(3),
        "the drop lands on the visible row it is over"
    );
    assert!(!row.is_empty() && grid.contains(row.x + 1.0, row.y + 1.0));
}

#[test]
fn the_bar_a_clip_lands_on_is_the_one_the_arrangement_snaps_to() {
    // The snap the arrangement is set to, not a bar always: the mark has to
    // agree with where the clip actually goes, or it is a lie drawn in the
    // accent colour.
    let rack = rack_layout(rack_body(), &metrics(), 3, 0);
    let timeline = timeline_layout(timeline_frame(), &metrics());
    let mut v = view();
    v.snap = SnapDivision::None;
    let grid = timeline.grid;
    let x = grid.x + 130.0;
    let target = carry_target(&scene(Carried::Audio, &rack, &timeline, &v), x, mid(grid).1);
    let CarryTarget::Clip { tick, .. } = target else {
        panic!("got {target:?}");
    };
    let free = fontelle_ui::canvas::timeline_x_to_tick(&v, grid, x);
    assert_eq!(tick, free, "with the snap off it lands where it was let go");
}

#[test]
fn an_instrument_is_not_a_clip() {
    // A preset is a sound for a channel to play; the arrangement holds
    // stretches of song. There is nothing for a soundfont preset to become
    // there, and saying so before the button comes up is the whole point.
    let rack = rack_layout(rack_body(), &metrics(), 3, 0);
    let timeline = timeline_layout(timeline_frame(), &metrics());
    let v = view();
    let target = carry_target(
        &scene(Carried::Preset, &rack, &timeline, &v),
        mid(timeline.grid).0,
        mid(timeline.grid).1,
    );
    assert_eq!(target, CarryTarget::Nowhere);
    assert!(!target.lands());
    assert!(
        target.refuses(),
        "and it says so, rather than saying nothing"
    );
    assert_eq!(target.mark(), None);
}

#[test]
fn the_ruler_and_the_lane_names_are_not_the_grid() {
    // Everything above and left of the grid is chrome: a clip cannot start on
    // the ruler, so a drop there must not claim it can.
    let rack = rack_layout(rack_body(), &metrics(), 3, 0);
    let timeline = timeline_layout(timeline_frame(), &metrics());
    let v = view();
    for part in [timeline.ruler, timeline.headers, timeline.toolbar] {
        let (x, y) = mid(part);
        assert!(
            carry_target(&scene(Carried::Audio, &rack, &timeline, &v), x, y).refuses(),
            "{part:?} took a clip"
        );
    }
}

// ----------------------------------------------------- home, and nowhere ---

#[test]
fn the_panel_it_came_out_of_is_neither_a_drop_nor_a_refusal() {
    // A press that never leaves the list is a **click**, and the click is what
    // the release does — see `press_browser`. So the panel is not a target,
    // and it must not be drawn as a refusal either: the row is exactly where
    // it started and nothing is wrong.
    let rack = rack_layout(rack_body(), &metrics(), 3, 0);
    let timeline = timeline_layout(timeline_frame(), &metrics());
    let v = view();
    let (x, y) = mid(browser_frame());
    let target = carry_target(&scene(Carried::Audio, &rack, &timeline, &v), x, y);
    assert_eq!(target, CarryTarget::Panel);
    assert!(!target.lands());
    assert!(!target.refuses());
    assert_eq!(target.mark(), None);
}

#[test]
fn dead_space_refuses() {
    // Between the panels, under the arrangement, over the transport bar: the
    // places a drop does nothing at all. It said nothing before this change,
    // which is the report.
    let rack = rack_layout(rack_body(), &metrics(), 3, 0);
    let timeline = timeline_layout(timeline_frame(), &metrics());
    let v = view();
    for (x, y) in [
        (700.0, 700.0),
        (252.0, 400.0),
        (1000.0, 10.0),
        (-40.0, -40.0),
    ] {
        let target = carry_target(&scene(Carried::Audio, &rack, &timeline, &v), x, y);
        assert!(target.refuses(), "({x}, {y}) claimed a drop: {target:?}");
        assert_eq!(target.mark(), None);
    }
}

#[test]
fn a_window_with_no_studio_in_it_refuses_everything() {
    // Every panel absent — no document, so no rack, no arrangement, and
    // nothing that could take a sound.
    let empty = CarryScene {
        carried: Carried::Audio,
        rack: None,
        panel: None,
        timeline: None,
        name: None,
        oscillators: &[],
    };
    assert!(carry_target(&empty, 100.0, 100.0).refuses());
}

// ------------------------------------------- the instrument's own window ---

#[test]
fn the_instrument_windows_name_stands_for_the_channel_it_has_open() {
    // *"i should be able to drag soundfonts into it from the soundfonts window
    // to also assign a soundfont."* That window shows one channel, so its
    // name field is that channel — and the pointer being in another window is
    // why nothing else in the scene is filled in: those coordinates belong to
    // the studio, not to this window.
    let field = Rect::new(12.0, 30.0, 260.0, 26.0);
    let over = CarryScene {
        carried: Carried::Preset,
        rack: None,
        panel: None,
        timeline: None,
        name: Some((2, field)),
        oscillators: &[],
    };
    let (x, y) = mid(field);
    let target = carry_target(&over, x, y);
    assert_eq!(
        target,
        CarryTarget::Instrument {
            channel: 2,
            rect: field
        }
    );
    assert!(target.lands());
    assert_eq!(target.mark(), Some(field));
    // And the rest of that window is not the name field.
    assert!(carry_target(&over, x, field.bottom() + 60.0).refuses());
}

/// A **sound** carried over one of Flopsynth's oscillator cards becomes that
/// oscillator's sound — which the file-from-the-desktop drop already did and
/// the browser's own drag did not, so a file dragged out of the Import tab
/// onto OSC A did nothing at all.
///
/// > *"i tried doing this from the audio import tab and dragging an audio
/// > file into flopsynth over one of my oscilator waveforms right now and it
/// > didnt do anything unfortunately"*
#[test]
fn a_sound_over_an_oscillator_card_becomes_that_oscillators_sound() {
    let field = Rect::new(12.0, 30.0, 260.0, 26.0);
    let cards = [
        CarryOscillator {
            layer: 0,
            frame: Rect::new(10.0, 70.0, 300.0, 200.0),
            name: "OSC A".to_string(),
        },
        CarryOscillator {
            layer: 3,
            frame: Rect::new(330.0, 70.0, 150.0, 200.0),
            name: "SUB".to_string(),
        },
    ];
    let over = CarryScene {
        carried: Carried::Audio,
        rack: None,
        panel: None,
        timeline: None,
        name: Some((2, field)),
        oscillators: &cards,
    };
    let (x, y) = mid(cards[1].frame);
    let target = carry_target(&over, x, y);
    assert_eq!(
        target,
        CarryTarget::Oscillator {
            layer: 3,
            card: 1,
            rect: cards[1].frame,
        }
    );
    assert!(target.lands());
    assert_eq!(target.mark(), Some(cards[1].frame));
    // The name field still stands for the channel, above the cards.
    let (x, y) = mid(field);
    assert!(matches!(
        carry_target(&over, x, y),
        CarryTarget::Instrument { channel: 2, .. }
    ));
    // And the window between the cards is nowhere.
    assert!(carry_target(&over, 320.0, 150.0).refuses());
}

/// A **preset** is not a sound: over an oscillator card it has nowhere to
/// go, and the chip says so rather than lighting the card up.
#[test]
fn a_preset_over_an_oscillator_card_is_refused() {
    let cards = [CarryOscillator {
        layer: 0,
        frame: Rect::new(10.0, 70.0, 300.0, 200.0),
        name: "OSC A".to_string(),
    }];
    let over = CarryScene {
        carried: Carried::Preset,
        rack: None,
        panel: None,
        timeline: None,
        name: None,
        oscillators: &cards,
    };
    let (x, y) = mid(cards[0].frame);
    assert!(carry_target(&over, x, y).refuses());
}

// ------------------------------------------------------- what it says ---

#[test]
fn the_chip_says_what_letting_go_would_do() {
    let rack = rack_layout(rack_body(), &metrics(), 3, 0);
    let names = channels();
    assert_eq!(
        carry_note(
            &CarryTarget::Channel {
                index: 1,
                rect: rack.rows[1].frame
            },
            &names,
            &[],
            4
        ),
        "Onto Snare",
        "which channel, by name — a row number is not something you can see"
    );
    assert_eq!(
        carry_note(&CarryTarget::NewChannel { rect: rack.list }, &names, &[], 4),
        "A new channel"
    );
    assert_eq!(
        carry_note(
            &CarryTarget::Instrument {
                channel: 0,
                rect: Rect::new(0.0, 0.0, 10.0, 10.0)
            },
            &names,
            &[],
            4
        ),
        "Onto Kick"
    );
    assert_eq!(
        carry_note(
            &CarryTarget::Clip {
                row: Rect::new(0.0, 0.0, 10.0, 10.0),
                at: 0.0,
                tick: PPQN * 4 * 8,
                lane: None,
            },
            &names,
            &[],
            4
        ),
        "A new row at bar 9",
        "the bar it will start on, counted the way the transport counts"
    );
    assert_eq!(
        carry_note(
            &CarryTarget::Oscillator {
                layer: 0,
                card: 1,
                rect: Rect::new(0.0, 0.0, 10.0, 10.0)
            },
            &names,
            &["OSC A".to_string(), "OSC B".to_string()],
            4
        ),
        "As OSC B\u{2019}s sound",
        "which oscillator, by the name on its card"
    );
    assert_eq!(
        carry_note(&CarryTarget::Nowhere, &names, &[], 4),
        "Nowhere to put this"
    );
    assert_eq!(
        carry_note(&CarryTarget::Panel, &names, &[], 4),
        "",
        "nothing to say about the list it came from"
    );
}

#[test]
fn a_clip_that_does_not_start_on_a_bar_line_says_the_beat_too() {
    // With the snap off a clip can land inside a bar, and "bar 9" for
    // something that starts on the third beat of it is wrong.
    let names = channels();
    let target = CarryTarget::Clip {
        row: Rect::new(0.0, 0.0, 10.0, 10.0),
        at: 0.0,
        tick: PPQN * 4 * 8 + PPQN * 2,
        lane: None,
    };
    assert_eq!(carry_note(&target, &names, &[], 4), "A new row at bar 9.3");
    // And onto an existing row, the label names the row rather than a new one.
    let onto = CarryTarget::Clip {
        row: Rect::new(0.0, 0.0, 10.0, 10.0),
        at: 0.0,
        tick: PPQN * 4 * 8,
        lane: Some(2),
    };
    assert_eq!(carry_note(&onto, &names, &[], 4), "Onto row 3 at bar 9");
}

#[test]
fn a_channel_nobody_has_named_is_still_named_in_the_chip() {
    // A short list and a target past the end of it: the note must still read
    // as a sentence rather than panicking or going blank.
    assert_eq!(
        carry_note(
            &CarryTarget::Channel {
                index: 7,
                rect: Rect::ZERO
            },
            &channels(),
            &[],
            4
        ),
        "Onto channel 8"
    );
}

// -------------------------------------------------------- where it draws ---

#[test]
fn the_chip_follows_the_pointer_without_hiding_it() {
    let chip = carry_chip((140.0, 34.0), (400.0, 300.0), window());
    assert!(!chip.is_empty());
    assert!(
        !chip.contains(400.0, 300.0),
        "a label drawn under the cursor covers the thing you are aiming at"
    );
    assert!(
        chip.x >= 400.0 && chip.y >= 300.0,
        "and it hangs down-right of the pointer, the way every desktop's does"
    );
}

#[test]
fn the_chip_never_leaves_the_window() {
    let w = window();
    for (x, y) in [
        (w.right() - 2.0, w.bottom() - 2.0),
        (w.x + 1.0, w.bottom() - 1.0),
        (w.right() - 1.0, w.y + 1.0),
    ] {
        let chip = carry_chip((220.0, 40.0), (x, y), w);
        assert!(!chip.is_empty(), "no chip at ({x}, {y})");
        assert!(
            chip.x >= w.x - 0.01
                && chip.y >= w.y - 0.01
                && chip.right() <= w.right() + 0.01
                && chip.bottom() <= w.bottom() + 0.01,
            "{chip:?} runs outside {w:?}"
        );
        assert!(!chip.contains(x, y), "{chip:?} is under the pointer");
    }
}

#[test]
fn a_chip_bigger_than_the_window_is_not_drawn() {
    assert!(carry_chip((4000.0, 40.0), (10.0, 10.0), window()).is_empty());
}

// --------------------------------------------- and then you can see it ---

#[test]
fn the_arrangement_scrolls_to_the_row_the_clip_landed_on() {
    // The other half of "so you know youre actually placing it right": the
    // lane an imported sound arrives on is a **new** one, past the bottom of
    // the stack, so in any project with a screenful of lanes the clip lands
    // where nobody can see it and the drop looks like it did nothing at all.
    use fontelle_ui::canvas::lane_scroll_to_show;
    let v = view(); // 34-pixel lanes
    let grid = Rect::new(120.0, 24.0, 900.0, 204.0); // six whole rows
    assert_eq!(
        lane_scroll_to_show(&v, grid, 3),
        0,
        "a lane already on screen must not move the view at all"
    );
    assert_eq!(
        lane_scroll_to_show(&v, grid, 9),
        4,
        "and one below it comes into view on the bottom row"
    );
    let mut scrolled = view();
    scrolled.top_lane = 8;
    assert_eq!(
        lane_scroll_to_show(&scrolled, grid, 2),
        2,
        "a lane above the view comes in on the top row"
    );
}

#[test]
fn a_view_with_no_room_for_a_lane_is_left_where_it_is() {
    use fontelle_ui::canvas::lane_scroll_to_show;
    let v = view();
    let flat = Rect::new(120.0, 24.0, 900.0, 0.0);
    assert_eq!(lane_scroll_to_show(&v, flat, 40), v.top_lane);
}

// ------------------------------------------------- what can be picked up ---

#[test]
fn only_a_sound_is_carried_out_of_the_import_tab() {
    // A `.mid` is not a sampler and not a clip at a bar — it is a file that
    // makes tracks of its own — so it is opened by a click and never carried.
    // The doc comment on `Drag::BrowserRow` has said "only audio can be
    // carried" since it was written; the function it is written on did not
    // check, so a MIDI row armed a drag that could only ever fail.
    let rows = vec![
        LibraryEntry::file("CoolBreak.wav", "1.2 MB"),
        LibraryEntry {
            name: "Beat.mid".to_string(),
            detail: String::new(),
            kind: LibraryKind::File,
        },
    ];
    assert!(browser_row_carries(
        &rows,
        BrowserMode::Import,
        FolderKind::Audio,
        0
    ));
    for kind in [FolderKind::Midi, FolderKind::Scores] {
        assert!(
            !browser_row_carries(&rows, BrowserMode::Import, kind, 1),
            "{kind:?} rows are opened, not carried"
        );
    }
}

// ------------------------------------------------- letting go outside ---
//
// > *"i also still cant drag audio clips into an osc to sample it from my
// > import tab when i try to drag it out it gets stuck inside the main daw
// > window."* — Ty, 2026-09-16
//
// A press grabs the pointer for the window it happened in, and on Wayland
// the grab holds until the button comes up: the studio hears every move and
// the release, the synth window hears nothing until after. So a row let go
// over the synth window is, to the studio, a row let go *outside itself* —
// and that used to mean "nowhere", which is the row getting stuck at the
// edge. Now it means the row is **held**: it stays on the pointer, and the
// next click puts it down, in whichever window that click is.

use fontelle_ui::canvas::{CarryRelease, carry_release, held_note};

#[test]
fn a_release_inside_the_studio_drops_where_it_is() {
    let studio = window();
    assert_eq!(
        carry_release(studio, (300.0, 200.0), false, true),
        CarryRelease::Drop
    );
    // Whether or not a floating window is open.
    assert_eq!(
        carry_release(studio, (300.0, 200.0), false, false),
        CarryRelease::Drop
    );
}

#[test]
fn a_release_in_a_floating_window_drops_there() {
    // The pointer is in the synth window (a compositor that hands the
    // pointer over mid-gesture): its coordinates are that window's, and the
    // release lands as it always did.
    assert_eq!(
        carry_release(window(), (40.0, 40.0), true, true),
        CarryRelease::Drop
    );
}

#[test]
fn a_release_outside_the_studio_holds_the_row_when_there_is_a_window_to_take_it() {
    let studio = window();
    for pointer in [
        (-20.0, 200.0),
        (1500.0, 200.0),
        (300.0, -5.0),
        (300.0, 900.0),
        // Where the studio puts the pointer once the compositor says it has
        // left — the shape a Wayland release outside the window arrives in.
        (f32::MIN, f32::MIN),
    ] {
        assert_eq!(
            carry_release(studio, pointer, false, true),
            CarryRelease::Hold,
            "{pointer:?}"
        );
    }
}

#[test]
fn a_release_outside_the_studio_with_nothing_open_lets_go() {
    // Nowhere for it to go: not held, because a chip that follows the pointer
    // around a window that cannot take it is a row that cannot be put down.
    assert_eq!(
        carry_release(window(), (-20.0, 200.0), false, false),
        CarryRelease::Cancel
    );
}

#[test]
fn a_held_row_says_how_to_put_it_down() {
    let names = vec!["OSC A".to_string()];
    let over = CarryTarget::Oscillator {
        card: 0,
        layer: 0,
        rect: Rect::new(0.0, 0.0, 10.0, 10.0),
    };
    // Over something that takes it, the note is the landing, as it was.
    let landing = carry_note(&over, &[], &names, 4);
    assert_eq!(held_note(&over, &landing), landing);
    // Over nothing, it says what to do — the row is not stuck, it is waiting.
    let nowhere = held_note(
        &CarryTarget::Nowhere,
        &carry_note(&CarryTarget::Nowhere, &[], &names, 4),
    );
    assert!(nowhere.to_lowercase().contains("click"), "{nowhere}");
    assert!(nowhere.contains("Esc"), "{nowhere}");
}
