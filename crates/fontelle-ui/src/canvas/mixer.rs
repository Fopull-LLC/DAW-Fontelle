//! The mixer panel: a strip per mixer track (TDD §13, §16.1).
//!
//! The last thing outstanding under item 9 of `docs/first-usable-plan.md`, and
//! the last clause of the §3 gate sentence with nothing on screen behind it —
//! *"balance parts with per-channel gain/pan/mute"*. The rack has carried mute
//! and solo since it was written; a level and a place in the stereo field had
//! nowhere at all to be set from.
//!
//! Geometry, hit-testing and the fader's own arithmetic, all pure, per §2.5 of
//! the plan. Nothing here knows what a `Project` is: a strip is a
//! [`MixerStrip`](crate::document::MixerStrip), which is a flattened view, for
//! the reason INVARIANT 2 gives.
//!
//! **Virtualised like the roll** (§16.4): a project with two hundred tracks
//! builds a screenful of rectangles, not two hundred.

use crate::document::MixerStrip;
use crate::layout::Rect;
use crate::theme::Metrics;

/// How wide one channel strip is.
///
/// Fixed rather than shared out across the panel, because a mixer is a thing
/// you learn the shape of: a strip that changes width when a track is added is
/// one whose fader is somewhere new every time you look.
pub const STRIP_WIDTH: f32 = 76.0;

/// Between strips, and inside one.
const GAP: f32 = 4.0;

/// How tall one insert row is. Small on purpose: it carries three or four
/// letters and a switch, and every pixel it takes is a pixel off the fader.
const INSERT_ROW_HEIGHT: f32 = 12.0;

/// The bypass switch at the left-hand end of an insert row.
const BYPASS_WIDTH: f32 = 10.0;

/// How much fader a strip keeps whatever else it is asked to show.
///
/// A fader with no travel is not a control, and a rack that grew into one
/// would take away the mixer's one essential gesture the moment somebody used
/// its newest feature.
const MIN_FADER_HEIGHT: f32 = 44.0;

/// How tall the pan control is.
const PAN_HEIGHT: f32 = 12.0;

/// How wide the meter beside the fader is, at most.
const METER_WIDTH: f32 = 14.0;

/// The quietest the fader goes. Not negative infinity: a fader that reaches
/// silence has a whole region of travel where every position sounds the same,
/// which is worse to use than one whose bottom is inaudible anyway.
pub const MIN_FADER_DB: f32 = -60.0;

/// And the loudest. Six decibels of headroom above unity is the console
/// convention and is as much as a mix bus can usually take.
pub const MAX_FADER_DB: f32 = 6.0;

/// How close to unity a press has to land before it snaps there, in pixels.
///
/// Getting exactly 0.0 dB back by hand on a curved fader is otherwise
/// impossible, and "nearly unity" is a mix that drifts every time it is
/// touched.
pub const FADER_DETENT_PX: f32 = 4.0;

/// The same, for the pan's centre.
pub const PAN_DETENT_PX: f32 = 6.0;

/// The fader's taper, as `(fraction of the travel from the bottom, decibels)`.
///
/// A fader linear in decibels spends most of itself on levels nobody mixes at:
/// half its throw would be -60 to -30 dB. This gives the top four-tenths to
/// -10..+6 dB, which is the range an actual balance decision lives in, and
/// compresses the quiet end where the only question is "is it off".
///
/// Piecewise linear rather than a curve so that it is exactly invertible —
/// [`fader_y_of_db`] draws the handle where [`fader_db_at`] will read it back,
/// and a fader that jumps the moment it is grabbed is unusable.
const FADER_SCALE: [(f32, f32); 4] = [
    (0.0, MIN_FADER_DB),
    (0.25, -30.0),
    (0.60, -10.0),
    (1.0, MAX_FADER_DB),
];

/// One track's strip, and every control on it.
#[derive(Debug, Clone, PartialEq)]
pub struct MixerStripLayout {
    /// Which track this is, counted from the top of the panel's own list — not
    /// from the left of the panel, which is what makes scrolling work.
    pub index: usize,
    pub frame: Rect,
    /// The track's name, and the part you click to select it.
    pub name: Rect,
    /// The pan control: a horizontal strip with a centre detent.
    pub pan: Rect,
    /// The fader's whole travel — the groove the handle slides in, and the
    /// rectangle [`fader_db_at`] measures against.
    pub fader: Rect,
    /// Where the handle is right now, inside `fader`.
    pub handle: Rect,
    /// The level meter beside the fader.
    pub meter: Rect,
    pub mute: Rect,
    pub solo: Rect,
    /// The gain read-out, in decibels. A label, not a control.
    pub value: Rect,
    /// A row per insert, top to bottom in chain order — which is the order the
    /// sound goes through them, and the only order a rack may draw them in
    /// without lying about the signal path.
    ///
    /// Fewer rows than the track has inserts when the panel is too short for
    /// them: the fader is what a mixer is *for*, so it is the rack that gives
    /// way. See `MIN_FADER_HEIGHT`.
    pub inserts: Vec<Rect>,
    /// The row that adds one. Empty when there is no room for it.
    pub add: Rect,
}

/// Where every strip is.
#[derive(Debug, Clone, PartialEq)]
pub struct MixerLayout {
    pub body: Rect,
    /// Where the scrolling strips go. Its own rectangle because hit-testing
    /// asks "is the pointer in the list" before it asks "which strip", and
    /// because the renderer clips to it.
    pub list: Rect,
    pub strips: Vec<MixerStripLayout>,
    /// The master strip, pinned to the right-hand end and never scrolled.
    ///
    /// It is where everything arrives, not one of the things arriving. A
    /// master fader you have to scroll to find is one you cannot use to set
    /// the level of the thing you are listening to.
    pub master: Option<MixerStripLayout>,
    /// How many tracks there are in total, master included, so a scrollbar can
    /// be drawn and a scroll offset clamped.
    pub total: usize,
    pub scroll: usize,
}

/// Lays `strips` out in `body`, starting the scrolling half from `scroll`.
///
/// The master is recognised by [`MixerStrip::is_master`] rather than by its
/// position, and a set with no master simply gets no master column — a
/// document without one is damaged (see `RealiseError::NoMaster`), and a panel
/// is not the place to discover that.
pub fn mixer_layout(
    body: Rect,
    metrics: &Metrics,
    strips: &[MixerStrip],
    scroll: usize,
) -> MixerLayout {
    let master_index = strips.iter().position(|s| s.is_master);

    if body.is_empty() {
        return MixerLayout {
            body,
            list: body,
            strips: Vec::new(),
            master: None,
            total: strips.len(),
            scroll,
        };
    }

    // The master's column comes off the right first, and everything else
    // shares what is left — the opposite way round from the rack's add button
    // only because a mixer is read left to right.
    let (list, master) = match master_index {
        Some(index) => {
            let x = (body.right() - STRIP_WIDTH).max(body.x);
            let frame = Rect::new(x, body.y, (body.right() - x).max(0.0), body.height).clamped();
            let list = Rect::new(
                body.x,
                body.y,
                (frame.x - GAP * 2.0 - body.x).max(0.0),
                body.height,
            )
            .clamped();
            (
                list,
                Some(strip_layout(index, frame, metrics, &strips[index])),
            )
        }
        None => (body, None),
    };

    // Whole strips only, for the reason `rack_layout` gives about rows: a
    // strip clipped to a few pixels still draws its fader inside those pixels,
    // on top of the strip beside it.
    let mut out = Vec::new();
    if !list.is_empty() && STRIP_WIDTH > 0.0 {
        let visible = ((list.width + GAP) / (STRIP_WIDTH + GAP)).floor() as usize;
        let ordinary: Vec<usize> = (0..strips.len())
            .filter(|index| Some(*index) != master_index)
            .collect();
        let scroll = scroll.min(ordinary.len().saturating_sub(1));
        for (slot, index) in ordinary.into_iter().skip(scroll).take(visible).enumerate() {
            let frame = Rect::new(
                list.x + slot as f32 * (STRIP_WIDTH + GAP),
                list.y,
                STRIP_WIDTH,
                list.height,
            );
            out.push(strip_layout(index, frame, metrics, &strips[index]));
        }
    }

    MixerLayout {
        body,
        list,
        strips: out,
        master,
        total: strips.len(),
        scroll,
    }
}

/// One strip's insides, top to bottom: the name, the pan, the fader and its
/// meter, the two switches, and the level read-out.
///
/// The switches and the read-out are pinned to the **bottom** rather than
/// placed after the fader, so that they line up across every strip whatever
/// the panel's height is — a row of mute buttons at different heights is a row
/// you have to aim at.
fn strip_layout(
    index: usize,
    frame: Rect,
    metrics: &Metrics,
    strip: &MixerStrip,
) -> MixerStripLayout {
    let inner = frame.inset(GAP * 0.75);
    let row = metrics.row_height.min(inner.height.max(0.0));

    let (name, rest) = inner.split_top(row);

    let value_y = (inner.bottom() - row).max(rest.y);
    let value = Rect::new(
        inner.x,
        value_y,
        inner.width,
        (inner.bottom() - value_y).max(0.0),
    )
    .clamped();

    let switches_y = (value.y - row).max(rest.y);
    let switch_width = ((inner.width - GAP) / 2.0).max(0.0);
    let switch_height = (value.y - switches_y).max(0.0);
    let mute = Rect::new(inner.x, switches_y, switch_width, switch_height).clamped();
    let solo = Rect::new(
        inner.x + switch_width + GAP,
        switches_y,
        switch_width,
        switch_height,
    )
    .clamped();

    let pan_height = PAN_HEIGHT.min((switches_y - rest.y).max(0.0));
    let pan = Rect::new(inner.x, rest.y, inner.width, pan_height).clamped();

    let middle_y = pan.bottom() + GAP;
    let middle = Rect::new(
        inner.x,
        middle_y.min(switches_y),
        inner.width,
        (switches_y - GAP - middle_y).max(0.0),
    )
    .clamped();
    // The rack comes off the top of the fader's space, and only as much of it
    // as leaves a fader worth dragging.
    let wanted = strip.inserts.len() + 1;
    let room = ((middle.height - MIN_FADER_HEIGHT) / INSERT_ROW_HEIGHT)
        .floor()
        .max(0.0) as usize;
    let rows = wanted.min(room);
    let mut inserts = Vec::new();
    let mut add = Rect::new(middle.x, middle.y, 0.0, 0.0);
    for row in 0..rows {
        let rect = Rect::new(
            middle.x,
            middle.y + row as f32 * INSERT_ROW_HEIGHT,
            middle.width,
            INSERT_ROW_HEIGHT,
        )
        .clamped();
        // The add row is last, after every insert that fitted — so a rack that
        // cannot show everything shows the effects rather than the button.
        if row < strip.inserts.len() {
            inserts.push(rect);
        } else {
            add = rect;
        }
    }
    let rack_height = rows as f32 * INSERT_ROW_HEIGHT;
    let middle = Rect::new(
        middle.x,
        middle.y + rack_height,
        middle.width,
        (middle.height - rack_height).max(0.0),
    )
    .clamped();

    let meter_width = METER_WIDTH.min((middle.width - GAP) / 2.0).max(0.0);
    let fader = Rect::new(
        middle.x,
        middle.y,
        (middle.width - meter_width - GAP).max(0.0),
        middle.height,
    )
    .clamped();
    let meter = Rect::new(
        (middle.right() - meter_width)
            .max(fader.right() + GAP)
            .min(middle.right()),
        middle.y,
        meter_width,
        middle.height,
    )
    .clamped();

    // The handle is centred on the level and then clamped into the groove, so
    // a fader at either end still draws a whole handle rather than half of one
    // outside its own strip.
    let handle_height = (row * 0.6).min(fader.height);
    let centre = fader_y_of_db(fader, strip.gain_db);
    let handle_y = (centre - handle_height / 2.0)
        .clamp(fader.y, (fader.bottom() - handle_height).max(fader.y));
    let handle = Rect::new(fader.x, handle_y, fader.width, handle_height).clamped();

    MixerStripLayout {
        index,
        frame,
        name,
        pan,
        fader,
        handle,
        meter,
        mute,
        solo,
        value,
        inserts,
        add,
    }
}

/// What is under the pointer in the mixer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MixerHit {
    /// Select the channel this track carries — the rack and the roll follow.
    Name(usize),
    Fader(usize),
    Pan(usize),
    Mute(usize),
    Solo(usize),
    /// Open the effect in slot `.1` of strip `.0`.
    Insert(usize, usize),
    /// Switch that insert out of the chain, or back in — reachable without
    /// opening it, because comparing with and without is the reason to have
    /// the switch at all.
    BypassInsert(usize, usize),
    AddInsert(usize),
    Nothing,
}

pub fn mixer_hit(layout: &MixerLayout, x: f32, y: f32) -> MixerHit {
    // The master first: it is outside the list, and a list that claimed the
    // whole body would swallow it.
    for strip in layout.master.iter().chain(layout.strips.iter()) {
        if !strip.frame.contains(x, y) {
            continue;
        }
        // The fader before the name, because the groove is the tallest thing
        // in the strip and the name is a single row at the top of it.
        // The rack before anything else in the strip's middle: its rows are
        // small, and a fader that claimed them would make them unclickable.
        for (slot, row) in strip.inserts.iter().enumerate() {
            if row.contains(x, y) {
                return if x < row.x + BYPASS_WIDTH {
                    MixerHit::BypassInsert(strip.index, slot)
                } else {
                    MixerHit::Insert(strip.index, slot)
                };
            }
        }
        if strip.add.contains(x, y) {
            return MixerHit::AddInsert(strip.index);
        }
        if strip.mute.contains(x, y) {
            return MixerHit::Mute(strip.index);
        }
        if strip.solo.contains(x, y) {
            return MixerHit::Solo(strip.index);
        }
        if strip.pan.contains(x, y) {
            return MixerHit::Pan(strip.index);
        }
        if strip.fader.contains(x, y) {
            return MixerHit::Fader(strip.index);
        }
        if strip.name.contains(x, y) {
            return MixerHit::Name(strip.index);
        }
        return MixerHit::Nothing;
    }
    MixerHit::Nothing
}

/// Where unity sits on the fader, as a fraction of the travel from the bottom.
///
/// Public because it is the one number that says what the taper *is*, and a
/// test that reads it is checking the shape rather than restating the table.
pub fn unity_fraction() -> f32 {
    fraction_of_db(0.0)
}

/// The level a press at `y` on the groove `fader` is asking for.
///
/// Clamped to the fader's ends, so a drag that runs off the panel pins the
/// level rather than losing it.
pub fn fader_db_at(fader: Rect, y: f32) -> f32 {
    if fader.height <= 0.0 {
        return 0.0;
    }
    // The detent is measured in pixels against where the handle would be
    // drawn, not in decibels, so it feels the same on a short strip and a tall
    // one.
    if (y - fader_y_of_db(fader, 0.0)).abs() <= FADER_DETENT_PX {
        return 0.0;
    }
    let fraction = 1.0 - ((y - fader.y) / fader.height).clamp(0.0, 1.0);
    db_of_fraction(fraction)
}

/// The other direction: where the handle for `db` goes on the groove.
pub fn fader_y_of_db(fader: Rect, db: f32) -> f32 {
    fader.bottom() - fader.height * fraction_of_db(db)
}

fn db_of_fraction(fraction: f32) -> f32 {
    let fraction = fraction.clamp(0.0, 1.0);
    for pair in FADER_SCALE.windows(2) {
        let (low_t, low_db) = pair[0];
        let (high_t, high_db) = pair[1];
        if fraction <= high_t {
            let span = high_t - low_t;
            let along = if span > 0.0 {
                (fraction - low_t) / span
            } else {
                0.0
            };
            return low_db + (high_db - low_db) * along;
        }
    }
    MAX_FADER_DB
}

fn fraction_of_db(db: f32) -> f32 {
    let db = db.clamp(MIN_FADER_DB, MAX_FADER_DB);
    for pair in FADER_SCALE.windows(2) {
        let (low_t, low_db) = pair[0];
        let (high_t, high_db) = pair[1];
        if db <= high_db {
            let span = high_db - low_db;
            let along = if span > 0.0 {
                (db - low_db) / span
            } else {
                0.0
            };
            return low_t + (high_t - low_t) * along;
        }
    }
    1.0
}

/// The pan a press at `x` on the strip `pan` is asking for, -1.0 to +1.0.
pub fn pan_at(pan: Rect, x: f32) -> f32 {
    if pan.width <= 0.0 {
        return 0.0;
    }
    if (x - (pan.x + pan.width / 2.0)).abs() <= PAN_DETENT_PX {
        return 0.0;
    }
    let fraction = ((x - pan.x) / pan.width).clamp(0.0, 1.0);
    // Rounded to the hundredth the read-out shows, for the reason
    // `transport::round_tempo` gives: a value the panel cannot display exactly
    // is a number on screen that is not the number in the document.
    ((fraction * 2.0 - 1.0) * 100.0).round() / 100.0
}

/// Where the marker for `value` goes on the strip.
pub fn pan_x_of(pan: Rect, value: f32) -> f32 {
    pan.x + pan.width * (value.clamp(-1.0, 1.0) + 1.0) / 2.0
}

/// What the level read-out says.
///
/// The bottom of the travel reads as off rather than as `-60.0`: what it means
/// is "this track is not in the mix", and a number there invites arithmetic
/// nobody wants to do.
pub fn format_gain_db(db: f32) -> String {
    if db <= MIN_FADER_DB {
        return "off".to_string();
    }
    format!("{db:+.1}")
}

/// And the pan read-out. `C` for centre, then how far over it is, the way
/// every console labels it.
pub fn format_pan(pan: f32) -> String {
    let percent = (pan.clamp(-1.0, 1.0) * 100.0).round() as i32;
    match percent {
        0 => "C".to_string(),
        p if p < 0 => format!("L{}", -p),
        p => format!("R{p}"),
    }
}
