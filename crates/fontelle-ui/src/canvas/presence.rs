//! Everybody's whereabouts, between the screen and the song.
//!
//! A pointer goes across a session as a place in the song — a tick, a lane
//! and how far down it on the arrangement; a tick and a key in a clip — and
//! never as pixels: Ty's *"if someone is zoomed in differently"* is exactly
//! the case a pixel gets wrong (2026-10-18). Each studio turns the place
//! back into a point through its own view, so the cursor is on the same beat
//! of the same lane whatever either window's zoom, scroll, size or row
//! height. Pure functions, both directions, and what following someone
//! adopts.

use fontelle_model::wire::{Pointer, Seen, SeenEditor};
use fontelle_types::{ClipId, LaneId, MixerTrackId};

use crate::document::PersonHere;

use super::mixer::MixerLayout;
use super::piano_roll::RollView;
use super::timeline::{MAX_TIMELINE_PPT, MIN_TIMELINE_PPT, TimelineView};
use crate::layout::Rect;

/// Where somebody's place in the song falls on this screen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Spot {
    /// On screen, at this point.
    Here { x: f32, y: f32 },
    /// Off it: an arrow at the edge, at this point, pointing `toward` it.
    Off { x: f32, y: f32, toward: Toward },
}

/// Which way an off-screen place lies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Toward {
    Left,
    Right,
    Up,
    Down,
}

/// How far in from the edge an off-screen arrow sits.
const EDGE: f32 = 10.0;

/// A point on the grid, or the arrow at its edge that points to it — left
/// and right first, since time is what a person scrolls most.
fn spot(grid: Rect, x: f32, y: f32) -> Spot {
    let clamp_x = |x: f32| x.clamp(grid.x + EDGE, (grid.right() - EDGE).max(grid.x + EDGE));
    let clamp_y = |y: f32| y.clamp(grid.y + EDGE, (grid.bottom() - EDGE).max(grid.y + EDGE));
    if x < grid.x {
        Spot::Off {
            x: clamp_x(x),
            y: clamp_y(y),
            toward: Toward::Left,
        }
    } else if x > grid.right() {
        Spot::Off {
            x: clamp_x(x),
            y: clamp_y(y),
            toward: Toward::Right,
        }
    } else if y < grid.y {
        Spot::Off {
            x: clamp_x(x),
            y: clamp_y(y),
            toward: Toward::Up,
        }
    } else if y > grid.bottom() {
        Spot::Off {
            x: clamp_x(x),
            y: clamp_y(y),
            toward: Toward::Down,
        }
    } else {
        Spot::Here { x, y }
    }
}

// ------------------------------------------------------------ arrangement

/// The place on the arrangement under (`x`, `y`): `None` off the grid or
/// below the last lane. `lanes` are the song's lanes, top row first.
pub fn arrangement_pointer(
    view: &TimelineView,
    grid: Rect,
    lanes: &[LaneId],
    x: f32,
    y: f32,
) -> Option<Pointer> {
    if !grid.contains(x, y) || view.pixels_per_tick <= 0.0 || view.lane_height <= 0.0 {
        return None;
    }
    let tick = view.scroll_tick as f64 + f64::from((x - grid.x) / view.pixels_per_tick);
    let rows = (y - grid.y) / view.lane_height + view.lane_offset;
    let index = view.top_lane as i64 + rows.floor() as i64;
    let lane = *lanes.get(usize::try_from(index).ok()?)?;
    Some(Pointer::Arrangement {
        tick: tick.max(0.0),
        lane,
        down: rows - rows.floor(),
    })
}

/// Where a place on the arrangement is drawn here: `None` when this song
/// has no such lane (somebody else's lane on its way, or gone).
pub fn arrangement_spot(
    view: &TimelineView,
    grid: Rect,
    lanes: &[LaneId],
    tick: f64,
    lane: LaneId,
    down: f32,
) -> Option<Spot> {
    let index = lanes.iter().position(|l| *l == lane)?;
    let x = grid.x + ((tick - view.scroll_tick as f64) as f32) * view.pixels_per_tick;
    let y =
        grid.y + (index as f32 - view.top_lane as f32 - view.lane_offset + down) * view.lane_height;
    Some(spot(grid, x, y))
}

/// Where somebody's time marker crosses the arrangement here, while it is
/// in view.
pub fn arrangement_playhead_x(view: &TimelineView, grid: Rect, tick: f64) -> Option<f32> {
    let x = grid.x + ((tick - view.scroll_tick as f64) as f32) * view.pixels_per_tick;
    (grid.x..=grid.right()).contains(&x).then_some(x)
}

/// What this arrangement shows, for somebody following.
pub fn seen_timeline(view: &TimelineView, grid: Rect, lanes: &[LaneId]) -> Seen {
    Seen {
        first_tick: view.scroll_tick as f64,
        ticks: if view.pixels_per_tick > 0.0 {
            f64::from(grid.width / view.pixels_per_tick)
        } else {
            0.0
        },
        top_lane: lanes.get(view.top_lane).copied(),
        lane_offset: view.lane_offset,
        editor: SeenEditor::Other,
    }
}

/// Shows here the stretch of arrangement somebody else sees: the same left
/// edge, as many ticks across this grid as across theirs, the same top row.
/// Row height is this studio's own.
pub fn follow_timeline(view: &mut TimelineView, grid: Rect, lanes: &[LaneId], seen: &Seen) {
    view.scroll_tick = seen.first_tick.round().max(0.0) as i64;
    if seen.ticks > 0.0 && grid.width > 0.0 {
        view.pixels_per_tick =
            (grid.width / seen.ticks as f32).clamp(MIN_TIMELINE_PPT, MAX_TIMELINE_PPT);
    }
    if let Some(top) = seen
        .top_lane
        .and_then(|top| lanes.iter().position(|l| *l == top))
    {
        view.top_lane = top;
        view.lane_offset = seen.lane_offset.clamp(0.0, 0.999);
    }
}

// ------------------------------------------------------------------ roll

/// The place in `clip`'s notes under (`x`, `y`). The key counts up from the
/// bottom of key 0's row: 60.5 is halfway up middle C's.
pub fn roll_pointer(view: &RollView, grid: Rect, clip: ClipId, x: f32, y: f32) -> Option<Pointer> {
    if !grid.contains(x, y) || view.pixels_per_tick <= 0.0 || view.key_height <= 0.0 {
        return None;
    }
    let tick = view.scroll_tick as f64 + f64::from((x - grid.x) / view.pixels_per_tick);
    let key = f32::from(view.top_key) + 1.0 + view.key_offset - (y - grid.y) / view.key_height;
    Some(Pointer::Roll {
        clip,
        tick: tick.max(0.0),
        key,
    })
}

/// Where a place in a clip's notes is drawn here — for a clip this studio
/// has open too.
pub fn roll_spot(view: &RollView, grid: Rect, tick: f64, key: f32) -> Spot {
    let x = grid.x + ((tick - view.scroll_tick as f64) as f32) * view.pixels_per_tick;
    let y = grid.y + (f32::from(view.top_key) + 1.0 + view.key_offset - key) * view.key_height;
    spot(grid, x, y)
}

/// What this roll shows, for somebody following.
pub fn seen_roll(view: &RollView, grid: Rect, clip: ClipId) -> SeenEditor {
    SeenEditor::Roll {
        clip,
        first_tick: view.scroll_tick as f64,
        ticks: if view.pixels_per_tick > 0.0 {
            f64::from(grid.width / view.pixels_per_tick)
        } else {
            0.0
        },
        top_key: f32::from(view.top_key) + view.key_offset,
    }
}

/// Shows here the stretch of a clip somebody else sees. Key height is this
/// studio's own.
pub fn follow_roll(view: &mut RollView, grid: Rect, seen: &SeenEditor) {
    let SeenEditor::Roll {
        first_tick,
        ticks,
        top_key,
        ..
    } = seen
    else {
        return;
    };
    view.scroll_tick = first_tick.round().max(0.0) as i64;
    if *ticks > 0.0 && grid.width > 0.0 {
        view.pixels_per_tick = grid.width / *ticks as f32;
    }
    let top = top_key.clamp(0.0, 127.0);
    view.top_key = top.floor() as u8;
    view.key_offset = top - top.floor();
}

// ----------------------------------------------------------------- marks

/// What this studio has on screen to draw somebody's whereabouts on: the
/// arrangement's view and grid, the song's lanes, the roll's view, grid and
/// open clip, and the mixer's strips with the track each one is. `None`
/// for what is not showing.
pub struct Places<'a> {
    pub timeline: Option<(&'a TimelineView, Rect)>,
    pub lanes: &'a [LaneId],
    pub roll: Option<(&'a RollView, Rect, ClipId)>,
    pub mixer: Option<(&'a MixerLayout, &'a [MixerTrackId])>,
}

/// Somebody's whereabouts as they are drawn here.
#[derive(Debug, Clone, PartialEq)]
pub struct Marks {
    pub peer: u16,
    pub name: String,
    pub colour: u8,
    /// Their pointer, on the arrangement or the open clip — or the arrow at
    /// the edge that points to it.
    pub cursor: Option<Spot>,
    /// Where their time marker crosses the arrangement, while in view.
    pub playhead: Option<f32>,
    /// The mixer strip they point at, lit.
    pub strip: Option<Rect>,
    /// Whether this studio is following them: their cursor is drawn larger.
    pub followed: bool,
    /// What they said a moment ago, shown by their pointer.
    pub said: Option<String>,
}

/// Where `person`'s whereabouts are drawn on what this studio shows.
pub fn person_marks(person: &PersonHere, places: &Places<'_>, followed: bool) -> Marks {
    let presence = &person.presence;
    let mut cursor = None;
    let mut strip = None;
    match &presence.pointer {
        Some(Pointer::Arrangement { tick, lane, down }) => {
            if let Some((view, grid)) = places.timeline {
                cursor = arrangement_spot(view, grid, places.lanes, *tick, *lane, *down);
            }
        }
        Some(Pointer::Roll { clip, tick, key }) => {
            if let Some((view, grid, open)) = places.roll
                && open == *clip
            {
                cursor = Some(roll_spot(view, grid, *tick, *key));
            }
        }
        Some(Pointer::Mixer { track }) => {
            if let Some((layout, tracks)) = places.mixer
                && let Some(index) = tracks.iter().position(|t| t == track)
            {
                strip = layout
                    .strips
                    .iter()
                    .chain(layout.master.iter())
                    .find(|s| s.index == index)
                    .map(|s| s.frame);
            }
        }
        None => {}
    }
    Marks {
        peer: person.peer,
        name: person.name.clone(),
        colour: person.colour,
        cursor,
        playhead: places
            .timeline
            .and_then(|(view, grid)| arrangement_playhead_x(view, grid, presence.playhead)),
        strip,
        followed,
        said: None,
    }
}

/// How long something said shows by the pointer of whoever said it.
pub const SAID_SECONDS: f32 = 6.0;

/// What something said shows as by a pointer: short, one line.
pub fn said_bubble(text: &str) -> String {
    const LONGEST: usize = 48;
    if text.chars().count() <= LONGEST {
        text.to_string()
    } else {
        format!(
            "{}\u{2026}",
            text.chars().take(LONGEST - 1).collect::<String>()
        )
    }
}

// ------------------------------------------------------------- smoothing

/// How quickly a glide closes on where it is going: about this many seconds
/// to cover two thirds of the way. Short enough that a cursor still feels
/// live; long enough to join fifteen messages a second into a line.
const CURSOR_EASE_S: f32 = 0.05;
const PLAYHEAD_EASE_S: f64 = 0.08;
/// A marker that moves further than this at once was put there (a click on
/// the ruler, a jump to the start), and is drawn there, not slid there.
const PLAYHEAD_SNAP_TICKS: f64 = fontelle_types::PPQN as f64 * 4.0;
/// How long a playing marker runs on past its last message before it waits
/// for the next: a lost message must not carry it off into the song.
const RUN_ON_S: f64 = 0.3;

/// Somebody's time marker as drawn: eased between the moments it is heard,
/// and run on at the pace it was heard moving while they play.
///
/// Ty, 2026-10-20: *"add smoothing to collaboration syncing on cursor and
/// time bar."* Whereabouts arrive fifteen times a second (`PRESENCE_EVERY`
/// in the app), and a marker drawn where each put it moved in steps. Time
/// is in seconds from any start the caller likes; ticks are song ticks.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PlayheadGlide {
    /// The last tick heard, and when.
    heard: Option<(f64, f64)>,
    playing: bool,
    /// Ticks a second, learned from the messages while playing.
    rate: f64,
    shown: Option<f64>,
}

impl PlayheadGlide {
    /// What the last message said. The same message read again (the window
    /// reads whereabouts every pass) changes nothing — not even when it was
    /// heard, which is what the pace is measured from.
    pub fn hear(&mut self, tick: f64, playing: bool, now: f64) {
        if let Some((was, at)) = self.heard {
            if was == tick && playing == self.playing {
                return;
            }
            if playing && self.playing && now > at {
                let pace = (tick - was) / (now - at);
                if pace > 0.0 {
                    self.rate = if self.rate > 0.0 {
                        self.rate * 0.6 + pace * 0.4
                    } else {
                        pace
                    };
                }
            }
        }
        if !playing {
            self.rate = 0.0;
        }
        self.heard = Some((tick, now));
        self.playing = playing;
    }

    /// Where it is drawn now, `dt` seconds after the last frame.
    pub fn advance(&mut self, now: f64, dt: f32) -> f64 {
        let Some((tick, at)) = self.heard else {
            return self.shown.unwrap_or(0.0);
        };
        let target = tick + self.rate * (now - at).clamp(0.0, RUN_ON_S);
        let shown = match self.shown {
            Some(shown) if (target - shown).abs() <= PLAYHEAD_SNAP_TICKS => {
                let dt = f64::from(dt.max(0.0));
                let moved = shown + self.rate * dt;
                let eased = moved + (target - moved) * (1.0 - (-dt / PLAYHEAD_EASE_S).exp());
                // Never backwards while it plays: a message a hair behind
                // the run-on is not the music going back.
                if self.playing {
                    eased.max(shown)
                } else {
                    eased
                }
            }
            _ => target,
        };
        self.shown = Some(shown);
        shown
    }

    /// Where it was drawn last, once it has been.
    pub fn shown(&self) -> Option<f64> {
        self.shown
    }

    /// Whether it has come to rest where it was put.
    pub fn settled(&self) -> bool {
        match (self.heard, self.shown) {
            (Some((tick, _)), Some(shown)) => !self.playing && (tick - shown).abs() < 0.5,
            _ => true,
        }
    }
}

/// A cursor `dt` seconds on from `from` towards `to`: eased while it stays
/// on the screen (or stays an arrow at the same edge), and straight there
/// when it changes from one to the other or comes or goes.
pub fn glide_spot(from: Option<Spot>, to: Option<Spot>, dt: f32) -> Option<Spot> {
    let k = 1.0 - (-dt.max(0.0) / CURSOR_EASE_S).exp();
    let ease = |a: f32, b: f32| {
        let at = a + (b - a) * k;
        if (b - at).abs() < 0.25 { b } else { at }
    };
    match (from, to) {
        (Some(Spot::Here { x, y }), Some(Spot::Here { x: tx, y: ty })) => Some(Spot::Here {
            x: ease(x, tx),
            y: ease(y, ty),
        }),
        (
            Some(Spot::Off { x, y, toward }),
            Some(Spot::Off {
                x: tx,
                y: ty,
                toward: towards,
            }),
        ) if toward == towards => Some(Spot::Off {
            x: ease(x, tx),
            y: ease(y, ty),
            toward,
        }),
        _ => to,
    }
}
