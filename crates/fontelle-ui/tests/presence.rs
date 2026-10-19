//! Everybody's cursor, drawn where it points in the song.
//!
//! Ty, 2026-10-18: a cursor sent as a place on the screen lands somewhere
//! else on a screen zoomed or scrolled differently. So a pointer goes across
//! as a place in the song — a tick and a lane on the arrangement, a tick and
//! a key in a clip — and each studio draws it through its own view. These
//! are the conversions, both ways, and what following somebody adopts.

use fontelle_model::Command;
use fontelle_model::wire::{Pointer, Seen, SeenEditor};
use fontelle_types::{LaneId, PPQN};
use fontelle_ui::canvas::{
    RollView, Spot, TimelineView, Toward, arrangement_pointer, arrangement_spot, follow_roll,
    follow_timeline, roll_pointer, roll_spot, seen_roll, seen_timeline,
};
use fontelle_ui::layout::Rect;

fn lanes(n: usize) -> Vec<LaneId> {
    let mut song = fontelle_model::Project::new("presence");
    for i in 0..n {
        fontelle_model::AddLane::new(format!("Lane {i}"))
            .apply(&mut song)
            .unwrap();
    }
    song.lane_ids()
}

const GRID: Rect = Rect {
    x: 120.0,
    y: 60.0,
    width: 800.0,
    height: 400.0,
};

fn alice() -> TimelineView {
    TimelineView {
        scroll_tick: PPQN * 4,
        top_lane: 1,
        lane_offset: 0.25,
        pixels_per_tick: 0.05,
        lane_height: 30.0,
        ..TimelineView::default()
    }
}

/// Bob's window is narrower, zoomed in, scrolled elsewhere, rows taller.
fn bob() -> TimelineView {
    TimelineView {
        scroll_tick: PPQN * 6,
        top_lane: 0,
        lane_offset: 0.0,
        pixels_per_tick: 0.2,
        lane_height: 50.0,
        ..TimelineView::default()
    }
}

const BOB_GRID: Rect = Rect {
    x: 140.0,
    y: 80.0,
    width: 600.0,
    height: 500.0,
};

#[test]
fn a_point_on_the_arrangement_goes_there_and_back() {
    let lanes = lanes(8);
    let (x, y) = (GRID.x + 333.0, GRID.y + 77.0);
    let pointer = arrangement_pointer(&alice(), GRID, &lanes, x, y).expect("over a lane");
    let Some(Spot::Here { x: bx, y: by }) = spot_of(&alice(), GRID, &lanes, &pointer) else {
        panic!("not drawn where it came from");
    };
    assert!((bx - x).abs() < 0.01 && (by - y).abs() < 0.01, "{bx},{by}");
}

fn spot_of(view: &TimelineView, grid: Rect, lanes: &[LaneId], pointer: &Pointer) -> Option<Spot> {
    let Pointer::Arrangement { tick, lane, down } = pointer else {
        panic!("not an arrangement pointer");
    };
    arrangement_spot(view, grid, lanes, *tick, *lane, *down)
}

/// The heart of it: Alice points at a beat on a lane; Bob, zoomed and
/// scrolled differently, sees the cursor on that beat of that lane.
#[test]
fn the_same_place_on_a_screen_zoomed_and_scrolled_differently() {
    let lanes = lanes(8);
    let tick = (PPQN * 9) as f32;
    let x = GRID.x + (tick - (PPQN * 4) as f32) * 0.05;
    // Halfway down lane 3: Alice's top row is lane 1, a quarter scrolled away.
    let y = GRID.y + (3.0 - 1.0 - 0.25 + 0.5) * 30.0;
    let pointer = arrangement_pointer(&alice(), GRID, &lanes, x, y).unwrap();
    let Pointer::Arrangement {
        tick: sent,
        lane,
        down,
    } = pointer
    else {
        panic!();
    };
    assert!((sent - tick as f64).abs() < 0.5, "{sent}");
    assert_eq!(lane, lanes[3]);
    assert!((down - 0.5).abs() < 0.01, "{down}");
    let Some(Spot::Here { x: bx, y: by }) =
        arrangement_spot(&bob(), BOB_GRID, &lanes, sent, lane, down)
    else {
        panic!("Bob cannot see it");
    };
    assert!((bx - (BOB_GRID.x + (tick - (PPQN * 6) as f32) * 0.2)).abs() < 0.1);
    assert!((by - (BOB_GRID.y + 3.5 * 50.0)).abs() < 0.1);
}

/// Off Bob's screen is an arrow at the edge, pointing the way to go.
#[test]
fn a_place_off_screen_is_an_arrow_at_the_edge() {
    let lanes = lanes(40);
    let far_right = (PPQN * 400) as f64;
    match arrangement_spot(&bob(), BOB_GRID, &lanes, far_right, lanes[2], 0.5) {
        Some(Spot::Off { toward, x, .. }) => {
            assert_eq!(toward, Toward::Right);
            assert!(x <= BOB_GRID.right() && x > BOB_GRID.x);
        }
        other => panic!("{other:?}"),
    }
    let visible = (PPQN * 7) as f64;
    match arrangement_spot(&bob(), BOB_GRID, &lanes, visible, lanes[39], 0.5) {
        Some(Spot::Off { toward, y, .. }) => {
            assert_eq!(toward, Toward::Down);
            assert!(y <= BOB_GRID.bottom() && y > BOB_GRID.y);
        }
        other => panic!("{other:?}"),
    }
}

/// A lane this studio has not got (yet) is nowhere, not somewhere wrong.
#[test]
fn a_lane_not_here_is_not_drawn() {
    let lanes = lanes(4);
    let elsewhere = self::lanes(6)[5];
    assert!(arrangement_spot(&bob(), BOB_GRID, &lanes[..2], 0.0, elsewhere, 0.0).is_none());
    assert!(arrangement_spot(&bob(), BOB_GRID, &lanes, 0.0, lanes[3], 0.0).is_some());
}

fn roll(top_key: u8, key_offset: f32, key_height: f32, ppt: f32, scroll: i64) -> RollView {
    RollView {
        scroll_tick: scroll,
        top_key,
        key_offset,
        pixels_per_tick: ppt,
        key_height,
        ..RollView::default()
    }
}

#[test]
fn a_point_in_a_clip_lands_on_the_same_key_and_beat() {
    let clip = fontelle_types::ClipId::default();
    let a = roll(84, 0.3, 16.0, 0.125, PPQN * 2);
    let b = roll(72, 0.0, 22.0, 0.15, PPQN);
    // In the middle of key 60's row, on beat 4.
    let tick = PPQN * 4;
    let x = GRID.x + (tick - PPQN * 2) as f32 * 0.125;
    let y = GRID.y + ((84 - 60) as f32 + 0.3) * 16.0 + 8.0;
    let pointer = roll_pointer(&a, GRID, clip, x, y).unwrap();
    let Pointer::Roll {
        tick: sent, key, ..
    } = pointer
    else {
        panic!();
    };
    assert!((sent - tick as f64).abs() < 0.5);
    assert!((key - 60.5).abs() < 0.01, "{key}");
    let Spot::Here { x: bx, y: by } = roll_spot(&b, BOB_GRID, sent, key) else {
        panic!("off Bob's screen");
    };
    assert!((bx - (BOB_GRID.x + (tick - PPQN) as f32 * 0.15)).abs() < 0.1);
    assert!(
        (by - (BOB_GRID.y + (72 - 60) as f32 * 22.0 + 11.0)).abs() < 0.1,
        "{by}"
    );
}

/// Following somebody shows the stretch of song they see, top row and all,
/// fitted to this window.
#[test]
fn following_shows_the_same_stretch_of_song() {
    let lanes = lanes(8);
    let seen = Seen {
        editor: SeenEditor::Other,
        ..seen_timeline(&alice(), GRID, &lanes)
    };
    let mut mine = bob();
    follow_timeline(&mut mine, BOB_GRID, &lanes, &seen);
    assert_eq!(mine.scroll_tick, alice().scroll_tick);
    assert_eq!(mine.top_lane, alice().top_lane);
    assert!((mine.lane_offset - alice().lane_offset).abs() < 1e-4);
    let alice_ticks = GRID.width / alice().pixels_per_tick;
    let my_ticks = BOB_GRID.width / mine.pixels_per_tick;
    assert!(
        (alice_ticks - my_ticks).abs() < 1.0,
        "{alice_ticks} {my_ticks}"
    );

    let clip = fontelle_types::ClipId::default();
    let a = roll(84, 0.3, 16.0, 0.125, PPQN * 2);
    let editor = seen_roll(&a, GRID, clip);
    let mut b = roll(72, 0.0, 22.0, 0.3, PPQN);
    follow_roll(&mut b, BOB_GRID, &editor);
    assert_eq!(b.scroll_tick, a.scroll_tick);
    assert_eq!(b.top_key, 84);
    assert!((b.key_offset - 0.3).abs() < 1e-4);
    assert!((GRID.width / a.pixels_per_tick - BOB_GRID.width / b.pixels_per_tick).abs() < 1.0);
}

fn person(presence: fontelle_model::wire::Presence) -> fontelle_ui::document::PersonHere {
    fontelle_ui::document::PersonHere {
        peer: 1,
        name: "Bob".into(),
        colour: 1,
        presence,
    }
}

/// A cursor in a clip this studio does not have open is not drawn on the
/// clip it has; the time marker shows only where the arrangement is in view.
#[test]
fn marks_only_where_this_studio_can_see_them() {
    use fontelle_ui::canvas::{Places, person_marks};
    let lanes = lanes(4);
    let clips = slotmap_ids();
    let (open, elsewhere) = (clips[0], clips[1]);
    let view = alice();
    let rollview = roll(84, 0.0, 16.0, 0.125, 0);
    let places = Places {
        timeline: Some((&view, GRID)),
        lanes: &lanes,
        roll: Some((&rollview, BOB_GRID, open)),
        mixer: None,
    };
    let in_other = person(fontelle_model::wire::Presence {
        pointer: Some(Pointer::Roll {
            clip: elsewhere,
            tick: 10.0,
            key: 60.0,
        }),
        playhead: (PPQN * 8) as f64,
        ..Default::default()
    });
    let marks = person_marks(&in_other, &places, false);
    assert!(marks.cursor.is_none(), "drawn on a clip they are not in");
    let x = marks.playhead.expect("the marker is in view");
    assert!((x - (GRID.x + (PPQN * 4) as f32 * 0.05)).abs() < 0.1);

    let in_mine = person(fontelle_model::wire::Presence {
        pointer: Some(Pointer::Roll {
            clip: open,
            tick: 10.0,
            key: 60.0,
        }),
        playhead: (PPQN * 4000) as f64,
        ..Default::default()
    });
    let marks = person_marks(&in_mine, &places, false);
    assert!(
        matches!(marks.cursor, Some(Spot::Here { .. })),
        "{:?}",
        marks.cursor
    );
    assert!(
        marks.playhead.is_none(),
        "the marker is far off to the right"
    );
}

/// Two distinct clip ids, made the way the song makes them.
fn slotmap_ids() -> Vec<fontelle_types::ClipId> {
    let mut song = fontelle_model::Project::new("ids");
    fontelle_model::AddLane::new("Lane")
        .apply(&mut song)
        .unwrap();
    let lane = song.lane_ids()[0];
    fontelle_model::AddChannel::new("Keys", None)
        .apply(&mut song)
        .unwrap();
    let channel = song.channels.keys().next().unwrap();
    for at in [0, 4] {
        fontelle_model::AddClip::new(fontelle_model::Clip {
            name: None,
            lane,
            start: PPQN * at,
            length: PPQN * 4,
            source: fontelle_model::ClipSource::Notes(fontelle_model::NoteData {
                channel,
                notes: fontelle_model::Arena::default(),
                vocal: None,
            }),
            prefab_link: None,
            color: None,
            muted: false,
            loop_length: None,
        })
        .apply(&mut song)
        .unwrap();
    }
    song.clips.keys().collect()
}
