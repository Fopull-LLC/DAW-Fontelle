//! The piano roll's third pass: the things that made it unpleasant to *use*
//! once there was enough of it to write a bar of music in.
//!
//! Every one of these is a report from somebody actually using the window, and
//! every one of them is arithmetic that was wrong at an edge:
//!
//! - **Dragging near bar 1 was jittery.** The pointer leaving the grid — which
//!   is what happens the moment you drag a note towards the start, because the
//!   keyboard is right there — was converted as if it were still inside it.
//!   Left of the grid clamped the *absolute* tick to zero and above the grid
//!   ran the key up to 127, so a drag that strayed a few pixels teleported.
//! - **A new note ignored the last one.** In FL Studio the note you draw is a
//!   copy of the last note you drew or clicked — its length, its velocity, its
//!   pan, all of it. Here every note came out one snap unit long at velocity
//!   100 for ever.
//! - **The lane under the grid only ever showed velocity, and was 78 pixels
//!   tall whatever you thought of that.**
//!
//! All pure functions and one state machine over them, per §2.5 of
//! `docs/first-usable-plan.md`.

use fontelle_model::{Arena, Note};
use fontelle_types::{NoteId, PPQN, Tick};
use fontelle_ui::canvas::{
    LaneProperty, MouseButton, PianoRoll, RollControl, RollEdit, RollView, SnapDivision,
    clamp_to_grid, edge_scroll, key_to_y, lane_height_at, lane_value_of_y, lane_y_of_value,
    roll_layout, tick_to_x, toolbar_hit, toolbar_layout, velocity_of_y,
};
use fontelle_ui::layout::Rect;
use fontelle_ui::theme::{Metrics, Theme};

fn metrics() -> Metrics {
    Theme::dark_default().metrics
}

fn view() -> RollView {
    RollView {
        scroll_tick: 0,
        top_key: 72,
        pixels_per_tick: 0.25,
        key_height: 12.0,
        snap: SnapDivision::Step,
    }
}

fn grid() -> Rect {
    Rect::new(60.0, 30.0, 800.0, 400.0)
}

fn note(start: Tick, length: Tick, key: u8) -> Note {
    Note {
        start,
        length,
        key,
        velocity: 100,
        pan: 0,
        fine_pitch: 0,
        release: 0,
        mod_x: 0,
        mod_y: 0,
        slide: false,
    }
}

fn notes(items: &[(Tick, Tick, u8)]) -> Arena<NoteId, Note> {
    let mut arena = Arena::default();
    for (start, length, key) in items {
        arena.insert(note(*start, *length, *key));
    }
    arena
}

fn roll() -> PianoRoll {
    PianoRoll::new(view())
}

fn at(view: &RollView, tick: Tick, key: u8) -> (f32, f32) {
    (
        tick_to_x(view, grid(), tick) + 1.0,
        key_to_y(view, grid(), key) + 1.0,
    )
}

// ------------------------------------------ a drag that strays off the grid ---

#[test]
fn a_pointer_outside_the_grid_clamps_to_the_nearest_point_inside_it() {
    let g = grid();
    // Inside is left alone, to the pixel.
    assert_eq!(clamp_to_grid(g, 100.0, 100.0), (100.0, 100.0));
    // Outside comes back to the edge, never past it — and never *on* the far
    // edge, which is outside by `Rect`'s half-open rule.
    let (x, y) = clamp_to_grid(g, g.x - 500.0, g.y - 500.0);
    assert!(x >= g.x && x < g.right(), "x={x}");
    assert!(y >= g.y && y < g.bottom(), "y={y}");
    let (x, y) = clamp_to_grid(g, g.right() + 500.0, g.bottom() + 500.0);
    assert!(x >= g.x && x < g.right(), "x={x}");
    assert!(y >= g.y && y < g.bottom(), "y={y}");
}

#[test]
fn dragging_a_note_off_the_left_of_the_grid_does_not_teleport_it() {
    let mut roll = roll();
    // In view: at this zoom the grid shows a little over three beats, and a
    // note the pointer cannot reach is not a note this test is about.
    let arena = notes(&[(PPQN * 2, PPQN, 60)]);
    let id = arena.keys().next().unwrap();

    let (x, y) = at(&roll.view, PPQN * 2, 60);
    roll.press(MouseButton::Left, x + 20.0, y, grid(), &arena, 4);

    // Straight off the left-hand end of the grid, and up above the ruler: the
    // exact gesture that used to fling the note to key 127 at tick 0.
    let edits = roll.drag(grid().x - 300.0, grid().y - 200.0, grid(), &arena, 4);
    let [
        RollEdit::Move {
            ids,
            tick_delta,
            key_delta,
        },
    ] = &edits[..]
    else {
        panic!("expected one move, got {edits:?}");
    };
    assert_eq!(ids, &vec![id]);
    // The clamp puts the pointer at the grid's top-left corner, which is
    // tick 0 of `top_key` — a real cell, a bounded distance from where the
    // note was.
    assert_eq!(
        *tick_delta,
        -(PPQN * 2),
        "the clamp puts the pointer on the grid's left-hand edge, which is \
         tick 0 — a bounded distance from where the note was, not a flattened \
         one"
    );
    assert_eq!(
        *key_delta,
        i16::from(roll.view.top_key) - 60,
        "off the top of the grid means the top row of the grid, not key 127"
    );
}

#[test]
fn a_drag_held_off_the_edge_scrolls_the_view_towards_it() {
    let v = view();
    let g = grid();

    assert_eq!(
        edge_scroll(&v, g, g.x + 10.0, g.y + 10.0),
        (0, 0),
        "a pointer inside the grid scrolls nothing"
    );

    let (ticks, keys) = edge_scroll(&v, g, g.x - 40.0, g.y + 10.0);
    assert!(ticks < 0, "off the left scrolls back towards bar 1");
    assert_eq!(keys, 0);

    let (ticks, _) = edge_scroll(&v, g, g.right() + 40.0, g.y + 10.0);
    assert!(ticks > 0, "off the right scrolls forwards");

    let (_, keys) = edge_scroll(&v, g, g.x + 10.0, g.y - 40.0);
    assert!(keys > 0, "off the top scrolls up the keyboard");
    let (_, keys) = edge_scroll(&v, g, g.x + 10.0, g.bottom() + 40.0);
    assert!(keys < 0, "off the bottom scrolls down it");

    // Further out is faster, but bounded — a flick of the mouse must not throw
    // the view a thousand bars.
    let (near, _) = edge_scroll(&v, g, g.x - 10.0, g.y + 10.0);
    let (far, _) = edge_scroll(&v, g, g.x - 2000.0, g.y + 10.0);
    assert!(far < near, "further out scrolls faster");
    assert!(
        far.abs() < (g.width / v.pixels_per_tick) as Tick,
        "and never more than a screenful in one event, got {far}"
    );
}

// ------------------------------------------------ the last note is a stencil ---

#[test]
fn a_new_note_is_a_copy_of_the_template() {
    let mut roll = roll();
    let arena = notes(&[]);
    roll.view.snap = SnapDivision::None;
    roll.set_template(Note {
        start: 0,
        length: PPQN * 2,
        key: 0,
        velocity: 42,
        pan: -30,
        fine_pitch: 17,
        release: 9,
        mod_x: 3,
        mod_y: 4,
        slide: false,
    });

    let (x, y) = at(&roll.view, PPQN, 64);
    let edits = roll.press(MouseButton::Left, x, y, grid(), &arena, 4);
    let [RollEdit::Add { note }] = &edits[..] else {
        panic!("expected one add, got {edits:?}");
    };
    assert_eq!(note.key, 64, "the pitch comes from the pointer");
    assert_eq!(note.length, PPQN * 2, "everything else comes from the last");
    assert_eq!(note.velocity, 42);
    assert_eq!(note.pan, -30);
    assert_eq!(note.fine_pitch, 17);
    assert_eq!(note.release, 9);
    assert_eq!((note.mod_x, note.mod_y), (3, 4));
}

#[test]
fn clicking_a_note_makes_it_the_template() {
    let mut roll = roll();
    let mut arena = notes(&[]);
    let long = arena.insert(Note {
        velocity: 20,
        pan: 55,
        ..note(PPQN, PPQN * 3, 60)
    });

    let (x, y) = at(&roll.view, PPQN + PPQN / 2, 60);
    roll.press(MouseButton::Left, x, y, grid(), &arena, 4);
    assert_eq!(roll.selection(), &[long]);

    let template = roll.template();
    assert_eq!(template.length, PPQN * 3, "its length");
    assert_eq!(template.velocity, 20, "its velocity");
    assert_eq!(template.pan, 55, "and everything else it carries");

    // And the next note drawn is that shape, with the snap off so the length
    // is the template's rather than the grid's.
    roll.release();
    roll.view.snap = SnapDivision::None;
    let (x, y) = at(&roll.view, 0, 70);
    let edits = roll.press(MouseButton::Left, x, y, grid(), &arena, 4);
    let [RollEdit::Add { note }] = &edits[..] else {
        panic!("expected one add, got {edits:?}");
    };
    assert_eq!(note.length, PPQN * 3);
    assert_eq!(note.velocity, 20);
    assert_eq!(note.pan, 55);
}

#[test]
fn sizing_a_note_updates_the_template_length() {
    let mut roll = roll();
    let mut arena = notes(&[]);
    let id = arena.insert(note(0, PPQN / 4, 60));
    roll.select(vec![id]);

    // Stretch it to a whole bar, then let go over the grid.
    arena.get_mut(id).unwrap().length = PPQN * 4;
    let (x, y) = at(&roll.view, PPQN * 4, 60);
    roll.release_over(x, y, grid(), &arena);

    assert_eq!(
        roll.template().length,
        PPQN * 4,
        "letting go of a resize leaves the roll drawing notes that long — \
         that is the whole point of the template"
    );
}

// ----------------------------------------------------- the property lane ---

#[test]
fn the_lane_can_show_a_property_other_than_velocity() {
    let mut roll = roll();
    assert_eq!(roll.lane_property, LaneProperty::Velocity);
    let mut seen = vec![roll.lane_property];
    loop {
        roll.lane_property = roll.lane_property.next();
        if roll.lane_property == LaneProperty::Velocity {
            break;
        }
        seen.push(roll.lane_property);
        assert!(
            seen.len() < 16,
            "the chip never comes back round to velocity: {seen:?}"
        );
    }
    assert!(
        seen.contains(&LaneProperty::Pan),
        "pan is the one people ask for first, got {seen:?}"
    );
    // Every property names itself for the chip, and no two share a caption.
    for property in &seen {
        assert!(!property.label().is_empty());
    }
    let mut captions: Vec<&str> = seen.iter().map(|p| p.label()).collect();
    captions.sort_unstable();
    let count = captions.len();
    captions.dedup();
    assert_eq!(captions.len(), count, "two properties share a caption");
}

#[test]
fn a_bipolar_lane_puts_zero_in_the_middle() {
    let lane = Rect::new(0.0, 100.0, 400.0, 80.0);

    assert_eq!(
        lane_value_of_y(LaneProperty::Pan, lane, lane.y + lane.height / 2.0),
        0,
        "the middle of a pan lane is centre, not one off it"
    );
    let (min, max) = LaneProperty::Pan.range();
    assert!(min < 0 && max > 0, "pan is signed, or none of this holds");
    assert_eq!(lane_value_of_y(LaneProperty::Pan, lane, lane.y), max);
    assert_eq!(
        lane_value_of_y(LaneProperty::Pan, lane, lane.bottom()),
        min,
        "clamped, and the ends of the range are reachable"
    );
    // The two directions agree.
    for value in [min, min / 3, 0, max / 2, max] {
        let y = lane_y_of_value(LaneProperty::Pan, lane, value);
        let back = lane_value_of_y(LaneProperty::Pan, lane, y);
        assert!(
            (back - value).abs() <= 1,
            "{value} came back as {back} through y={y}"
        );
    }

    // Velocity is unipolar and unchanged — the old lane is a special case of
    // the new one, not a different one.
    assert_eq!(
        lane_value_of_y(LaneProperty::Velocity, lane, lane.y),
        i32::from(velocity_of_y(lane, lane.y))
    );
}

#[test]
fn dragging_the_lane_edits_whichever_property_it_is_showing() {
    let mut roll = roll();
    let arena = notes(&[(0, PPQN, 60)]);
    let id = arena.keys().next().unwrap();
    let lane = Rect::new(grid().x, 500.0, grid().width, 80.0);
    roll.lane_property = LaneProperty::Pan;

    let x = tick_to_x(&roll.view, grid(), PPQN / 2);
    let edits = roll.press_lane(x, lane.y + 4.0, lane, grid(), &arena);
    let [
        RollEdit::SetProperty {
            ids,
            property,
            value,
        },
    ] = &edits[..]
    else {
        panic!("expected one property edit, got {edits:?}");
    };
    assert_eq!(ids, &vec![id]);
    assert_eq!(*property, LaneProperty::Pan);
    assert!(
        *value > LaneProperty::Pan.range().1 / 2,
        "near the top of a pan lane is hard right, got {value}"
    );

    // And the value the lane just set becomes the template's, so the next note
    // drawn is panned the same way.
    assert_eq!(i32::from(roll.template().pan), *value);
}

// ------------------------------------------------------- resizing the lane ---

#[test]
fn the_lane_has_a_grip_that_resizes_it() {
    let m = metrics();
    let frame = Rect::new(0.0, 0.0, 900.0, 520.0);
    let l = roll_layout(frame, &m, 78.0);

    assert!(!l.lane_grip.is_empty(), "there is something to grab");
    assert!(
        l.lane_grip.bottom() <= l.velocity.y + 0.001 && l.lane_grip.y < l.velocity.y,
        "the grip is the seam above the lane, {:?} against {:?}",
        l.lane_grip,
        l.velocity
    );

    // Dragging it up makes the lane taller and the grid shorter, by the same
    // amount — nothing is lost between them.
    let wanted = lane_height_at(&l, l.lane_grip.y - 60.0);
    assert!(wanted > 78.0);
    let taller = roll_layout(frame, &m, wanted);
    assert!(taller.velocity.height > l.velocity.height);
    assert!(taller.grid.height < l.grid.height);
    assert!((taller.velocity.bottom() - frame.bottom()).abs() < 0.001);

    // And it cannot be dragged past either end.
    let squashed = lane_height_at(&l, frame.bottom() + 400.0);
    assert!(squashed > 0.0, "a lane cannot be dragged out of existence");
    let huge = lane_height_at(&l, frame.y - 400.0);
    let huge_layout = roll_layout(frame, &m, huge);
    assert!(
        !huge_layout.grid.is_empty(),
        "and the grid cannot be squeezed to nothing to make room for it"
    );
}

#[test]
fn the_toolbar_carries_the_lane_property_chip() {
    let m = metrics();
    let l = roll_layout(Rect::new(0.0, 0.0, 900.0, 520.0), &m, 78.0);
    let bar = toolbar_layout(l.toolbar, &m);
    let controls: Vec<RollControl> = bar.items.iter().map(|(c, _)| *c).collect();
    assert!(
        controls.contains(&RollControl::Lane),
        "there is no way to reach pan without one, got {controls:?}"
    );
    let rect = bar
        .items
        .iter()
        .find(|(c, _)| *c == RollControl::Lane)
        .map(|(_, r)| *r)
        .unwrap();
    assert_eq!(
        toolbar_hit(&bar, rect.x + rect.width / 2.0, rect.y + rect.height / 2.0),
        Some(RollControl::Lane)
    );
}

// ------------------------------------------------------ hearing a click ---
//
// Reported as *"a short flicker of static"*. The engine path is fine —
// `fontelle-app/tests/audition.rs` holds a note for sixty-four blocks — so the
// blip was the window's: an audition lasted exactly as long as the mouse
// button was down, and a click is thirty milliseconds. Thirty milliseconds of
// a chiptune noise channel *is* static.

#[test]
fn an_audition_lasts_long_enough_to_be_a_note() {
    use fontelle_ui::transport::{MIN_AUDITION, audition_release};
    use std::time::{Duration, Instant};

    let started = Instant::now();
    // A quick click: the button came up almost immediately, and the note is
    // held on until it has been a note.
    let quick = audition_release(started, started + Duration::from_millis(20));
    assert_eq!(quick, started + MIN_AUDITION);
    assert!(MIN_AUDITION >= Duration::from_millis(100), "still a blip");

    // A held key is released when it is released — the minimum is a floor, not
    // a length.
    let held = started + Duration::from_secs(3);
    assert_eq!(audition_release(started, held), held);
}

// ---------------------------------------------------------- onion skins ---
//
// Reported: *"I should have options for seeing onion skins of the other notes
// that match up in the timeline, that way I can easily go back and forth
// between chord and melody on different instruments, and be able to change the
// filter of the skinning to see what I want."*

#[test]
fn the_ghost_filter_walks_off_then_everything_then_one_channel_at_a_time() {
    use fontelle_ui::document::GhostFilter;

    let mut filter = GhostFilter::Off;
    let mut seen = vec![filter];
    for _ in 0..8 {
        filter = filter.next(3);
        seen.push(filter);
        if filter == GhostFilter::Off && seen.len() > 1 {
            break;
        }
    }
    assert_eq!(
        seen,
        vec![
            GhostFilter::Off,
            GhostFilter::All,
            GhostFilter::Channel(0),
            GhostFilter::Channel(1),
            GhostFilter::Channel(2),
            GhostFilter::Off,
        ],
        "off, then everything, then one instrument at a time, then off again"
    );
}

#[test]
fn a_project_with_nothing_to_skin_against_leaves_the_chip_off() {
    use fontelle_ui::document::GhostFilter;
    // One channel means there is no *other* instrument, so every setting shows
    // the same empty picture. Stepping through three of them would be a chip
    // that looks broken rather than one that is honestly inapplicable.
    assert_eq!(GhostFilter::Off.next(1), GhostFilter::Off);
    assert_eq!(GhostFilter::Off.next(0), GhostFilter::Off);
    // Two channels: off, all, the first, the second, off.
    assert_eq!(GhostFilter::Off.next(2), GhostFilter::All);
    assert_eq!(GhostFilter::All.next(2), GhostFilter::Channel(0));
    assert_eq!(GhostFilter::Channel(1).next(2), GhostFilter::Off);
    // A filter naming a channel that has since been deleted falls back rather
    // than showing nothing for ever.
    assert_eq!(GhostFilter::Channel(9).next(2), GhostFilter::Off);
}

#[test]
fn the_toolbar_carries_the_onion_skin_chip() {
    let m = metrics();
    let l = roll_layout(Rect::new(0.0, 0.0, 1100.0, 520.0), &m, 78.0);
    let bar = toolbar_layout(l.toolbar, &m);
    let controls: Vec<RollControl> = bar.items.iter().map(|(c, _)| *c).collect();
    assert!(
        controls.contains(&RollControl::Ghost),
        "there is no way to reach the onion skins, got {controls:?}"
    );
    let rect = bar
        .items
        .iter()
        .find(|(c, _)| *c == RollControl::Ghost)
        .map(|(_, r)| *r)
        .expect("the chip");
    assert_eq!(
        toolbar_hit(&bar, rect.x + rect.width / 2.0, rect.y + rect.height / 2.0),
        Some(RollControl::Ghost)
    );
}

#[test]
fn the_roll_starts_with_the_onion_skins_off() {
    use fontelle_ui::document::GhostFilter;
    // They are a reference, not a default: a roll that opens showing every
    // other instrument's notes is a roll you cannot read.
    assert_eq!(roll().ghosts, GhostFilter::Off);
}

#[test]
fn the_lane_seam_is_thick_enough_to_actually_grab() {
    // Reported from using the roll: *"the velocity / pan etc. section at the
    // bottom does have a knob to drag it but i am unable to drag it right
    // now."* The seam was five logical pixels tall — which on a 4K screen at
    // 150% is three physical rows of the mark you are aiming at, and a target
    // that thin is one you miss and then decide is not a control.
    //
    // The room comes from the *grid* side, never the lane's: a grip drawn over
    // the top row of bars would eat the clicks that set a velocity to its
    // loudest, which is the value people reach for most.
    let l = roll_layout(Rect::new(0.0, 0.0, 900.0, 520.0), &metrics(), 78.0);
    assert!(
        l.lane_grip.height >= 8.0,
        "the seam is {} pixels tall, which is not a control",
        l.lane_grip.height
    );
    assert!(
        l.lane_grip.bottom() <= l.velocity.y + 0.001,
        "the grip must stay out of the lane"
    );
}
