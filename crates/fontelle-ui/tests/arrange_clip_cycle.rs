//! A second click on the clip that is already open steps the rack through the
//! instruments playing in it.
//!
//! > *"please make it work more like the user would expect where it picks
//! > either the most relevant instrument (or just a different one than the
//! > one thats currently selected, maybe cycle through?) in the case of
//! > multiple instruments in one clip"*
//!
//! Which channel comes next is the document's to say
//! (`fontelle_model::ClipChannels::cycled`); the canvas only says that a
//! **click** — down and up without the clip moving — landed on the block that
//! was already open, and hands it over the way `take_open` does. A drag is
//! not a click: picking the open clip up to move it must not change the
//! instrument you are writing.

use fontelle_model::Arena;
use fontelle_types::{ClipId, PPQN, Tick};
use fontelle_ui::canvas::{
    Modifiers, MouseButton, Timeline, TimelineLayout, TimelineView, lane_to_y, timeline_layout,
    timeline_tick_to_x,
};
use fontelle_ui::document::{ClipInfo, ClipKind};
use fontelle_ui::layout::Rect;
use fontelle_ui::theme::Theme;

const BAR: Tick = PPQN * 4;

fn layout() -> TimelineLayout {
    timeline_layout(
        Rect::new(0.0, 0.0, 1600.0, 400.0),
        &Theme::dark_default().metrics,
    )
}

fn a_clip(open: bool, kind: ClipKind) -> ClipInfo {
    let mut arena: Arena<ClipId, ()> = Arena::default();
    ClipInfo {
        id: arena.insert(()),
        lane: 1,
        start: BAR,
        length: BAR * 2,
        name: "Drums +1".to_string(),
        muted: false,
        open,
        color: [0x4f, 0x8f, 0xd0, 0xff],
        loop_length: None,
        kind,
        curve: Vec::new(),
        notes: Vec::new(),
        audio: Default::default(),
        prefab: None,
    }
}

fn timeline() -> Timeline {
    Timeline::new(TimelineView {
        pixels_per_tick: 0.05,
        ..TimelineView::default()
    })
}

fn at(timeline: &Timeline, tick: Tick, lane: usize) -> (f32, f32) {
    let l = layout();
    (
        timeline_tick_to_x(&timeline.view, l.grid, tick),
        lane_to_y(&timeline.view, l.grid, lane) + timeline.view.lane_height * 0.6,
    )
}

/// Down on the clip's body, through `travel` ticks, and up again.
fn click(timeline: &mut Timeline, clips: &[ClipInfo], travel: Tick) {
    let l = layout();
    let (x, y) = at(timeline, BAR + PPQN * 2, 1);
    timeline.press(MouseButton::Left, x, y, &l, clips);
    let (x2, y2) = at(timeline, BAR + PPQN * 2 + travel, 1);
    if travel != 0 {
        timeline.drag(x2, y2, &l, clips);
    }
    timeline.release_over(x2, y2, &l, clips);
}

#[test]
fn clicking_the_open_clip_again_asks_for_the_next_instrument() {
    let clips = vec![a_clip(true, ClipKind::Notes)];
    let mut timeline = timeline();
    click(&mut timeline, &clips, 0);
    assert_eq!(timeline.take_cycle(), Some(clips[0].id));
    assert_eq!(timeline.take_cycle(), None, "handed over once");
}

#[test]
fn the_first_click_on_a_clip_only_opens_it() {
    let clips = vec![a_clip(false, ClipKind::Notes)];
    let mut timeline = timeline();
    click(&mut timeline, &clips, 0);
    assert_eq!(timeline.take_open(), Some(clips[0].id));
    assert_eq!(timeline.take_cycle(), None);
}

#[test]
fn moving_the_open_clip_is_not_a_click() {
    let clips = vec![a_clip(true, ClipKind::Notes)];
    let mut timeline = timeline();
    click(&mut timeline, &clips, BAR * 2);
    assert_eq!(timeline.take_cycle(), None);
}

#[test]
fn a_click_with_a_modifier_is_some_other_gesture() {
    let clips = vec![a_clip(true, ClipKind::Notes)];
    for modifiers in [
        Modifiers {
            shift: true,
            ..Modifiers::default()
        },
        Modifiers {
            ctrl: true,
            ..Modifiers::default()
        },
    ] {
        let mut timeline = timeline();
        timeline.set_modifiers(modifiers);
        click(&mut timeline, &clips, 0);
        assert_eq!(timeline.take_cycle(), None, "{modifiers:?}");
    }
}

#[test]
fn only_a_note_clip_has_instruments_to_cycle() {
    for kind in [ClipKind::Audio, ClipKind::Automation] {
        let clips = vec![a_clip(true, kind)];
        let mut timeline = timeline();
        click(&mut timeline, &clips, 0);
        assert_eq!(timeline.take_cycle(), None, "{kind:?}");
    }
}
