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

/// How wide the track-options column is.
///
/// Wider than a strip because what it carries is *words* — an effect's name, a
/// route's destination — where a strip carries controls. Three letters and a
/// switch is all a 76-pixel rack can say, and "is that the EQ or the
/// expander" is the question the column exists to answer.
pub const OPTIONS_WIDTH: f32 = 168.0;

/// How tall one row in the options column is. A row you can read and aim at,
/// unlike `INSERT_ROW_HEIGHT`, which is what a strip can spare.
const OPTION_ROW_HEIGHT: f32 = 20.0;

/// How much of a send row goes to naming its destination, the rest to the
/// level. Under half, because the level is what gets dragged and a groove you
/// cannot aim at is not a control.
const SEND_TARGET_SHARE: f32 = 0.38;

/// How wide the pre/post switch at the head of a send row is.
///
/// Wider than the bypass dot beside it, because this one carries a *word*.
/// At the bypass switch's sixteen pixels it read as "po", which is worse than
/// no label — a control that says half of something is one you have to guess
/// at twice.
const SEND_TAP_WIDTH: f32 = 30.0;

/// A send's travel, in decibels. The same ends as the mixer's own fader, so
/// "off" means the same thing in both places.
pub const MIN_SEND_DB: f32 = MIN_FADER_DB;
pub const MAX_SEND_DB: f32 = MAX_FADER_DB;

/// The bypass switch at the left of an options row, and the grip and the
/// delete at its right.
const OPTION_BYPASS_WIDTH: f32 = 16.0;
const OPTION_GRIP_WIDTH: f32 = 12.0;
const OPTION_REMOVE_WIDTH: f32 = 16.0;

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

/// One insert, as the options column draws it: a switch, a name you can press
/// to open it, something to drag it by, and something to throw it away with.
#[derive(Debug, Clone, PartialEq)]
pub struct InsertRowLayout {
    /// Its place in the chain, which is also the order the sound goes through
    /// it.
    pub slot: usize,
    pub frame: Rect,
    pub bypass: Rect,
    /// Press to open the effect's editor.
    pub name: Rect,
    /// Drag to reorder. Its own target rather than "drag the row", because the
    /// row's main job is a press that opens the editor, and one rectangle
    /// cannot mean both without a timeout nobody can see.
    pub grip: Rect,
    pub remove: Rect,
    /// Wet/dry: how much of this insert's output is the effect and how much is
    /// the signal that went into it. A short groove dragged like the send's
    /// level, and on the row for the same reason — it is set by ear against
    /// the rest of the chain, and a control behind a window is one nobody
    /// touches twice.
    pub mix: Rect,
}

/// One send, as the options column draws it: where it goes, how much of the
/// track goes there, whether it is taken before the fader, and a way to throw
/// it away.
#[derive(Debug, Clone, PartialEq)]
pub struct SendRowLayout {
    /// Its place in the track's send list.
    pub index: usize,
    pub frame: Rect,
    /// Pre-fader or post. A switch, not a menu: there are two answers.
    pub tap: Rect,
    /// Where it goes. Press to send it somewhere else.
    pub target: Rect,
    /// How much goes. A horizontal groove, dragged like a fader on its side —
    /// the widest thing in the row, because it is the one you actually move.
    pub level: Rect,
    pub remove: Rect,
}

/// The track-options column (TDD §13.2, §13.4): everything about the selected
/// track that a 76-pixel strip has no room for.
///
/// Anchored between the last strip and the master. It belongs with the mixer
/// rather than in the editor column because what it edits is whichever strip
/// you just clicked — see `fontelle-ui/tests/track_options.rs`.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackOptionsLayout {
    /// Which strip this is about, by its index in the caller's list.
    pub track: usize,
    pub frame: Rect,
    /// The track's name, and where a rename starts.
    pub title: Rect,
    /// Which audio input this track records from (TDD §15.4). Press to choose
    /// another.
    ///
    /// *"i click a input button that lets my select my mic input to feed to
    /// that mixer track."* Beside the output row, because they are the same
    /// question pointed two ways — where the sound comes from and where it
    /// goes — and a column that reads top to bottom as a signal path is one you
    /// do not have to hunt in.
    pub input: Rect,
    /// Where the track's output goes. Press to choose another (§13.2).
    pub output: Rect,
    /// The heading over the chain.
    pub inserts_title: Rect,
    /// A row per insert, in chain order. Fewer than the track has when the
    /// column is too short — the *front* of the chain is kept, because that is
    /// where the signal arrives.
    pub inserts: Vec<InsertRowLayout>,
    /// Puts another on the end. Empty when there is no room for it.
    pub add_insert: Rect,
    /// The heading over the sends.
    pub sends_title: Rect,
    /// A row per send (§13.2).
    ///
    /// Dropped before the insert chain is when the column is short: the chain
    /// is what changes the sound of *this* track, and a send is what it gives
    /// to something else.
    pub sends: Vec<SendRowLayout>,
    pub add_send: Rect,
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
    /// The master strip, pinned to the left-hand end and never scrolled.
    ///
    /// It is where everything arrives, not one of the things arriving. A
    /// master fader you have to scroll to find is one you cannot use to set
    /// the level of the thing you are listening to.
    pub master: Option<MixerStripLayout>,
    /// The column past the last strip that adds another track.
    ///
    /// *"in the mixer track next to the end of the empty track columns, there
    /// will be a plus where you can add a new track there, then the plus
    /// button moves to the next empty space"* — so its place is a consequence
    /// of how many tracks there are, and adding one leaves the pointer over it
    /// again. Empty when the strips have filled the panel: a button drawn over
    /// a fader is worse than one that is not there.
    pub add_track: Rect,
    /// The selected track's options, between the last strip and the master.
    /// `None` on a panel too narrow to carry it — the strips are what a mixer
    /// is, and they are what a narrow window keeps.
    pub options: Option<TrackOptionsLayout>,
    /// How many tracks there are in total, master included, so a scrollbar can
    /// be drawn and a scroll offset clamped.
    pub total: usize,
    pub scroll: usize,
}

impl MixerLayout {
    /// The options column's rectangle, or an empty one. For callers — chiefly
    /// tests and the renderer's overlap checks — that want to reason about
    /// where it is without unwrapping it first.
    pub fn options_frame(&self) -> Rect {
        self.options.as_ref().map_or(Rect::ZERO, |o| o.frame)
    }
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
    mixer_layout_for(body, metrics, strips, scroll, None)
}

/// The same, told which strip the options column is about.
///
/// `selected` is an index into `strips`. Out of range — and `None`, which is
/// what every caller predating the column passes — falls back to the first
/// track: a blank column that reserves the width and shows nothing is the
/// worst of both.
pub fn mixer_layout_for(
    body: Rect,
    metrics: &Metrics,
    strips: &[MixerStrip],
    scroll: usize,
    selected: Option<usize>,
) -> MixerLayout {
    let master_index = strips.iter().position(|s| s.is_master);

    if body.is_empty() {
        return MixerLayout {
            body,
            list: body,
            strips: Vec::new(),
            master: None,
            add_track: Rect::ZERO,
            options: None,
            total: strips.len(),
            scroll,
        };
    }

    // The master's column comes off the **left** first, and everything else
    // shares what is left of the panel.
    //
    // The left-hand end because that is where a panel starts: the rack and the
    // browser both begin at their own left edge, and a master pinned to the
    // right made the mixer the one panel in the window read from the other
    // side. It is still the thing that never scrolls — where everything
    // arrives, rather than one of the things arriving — which is the property
    // that matters and is independent of which edge it is pinned to.
    let (mut rest, master) = match master_index {
        Some(index) => {
            let width = STRIP_WIDTH.min(body.width.max(0.0));
            let frame = Rect::new(body.x, body.y, width, body.height).clamped();
            let rest = Rect::new(
                frame.right() + GAP * 2.0,
                body.y,
                (body.right() - frame.right() - GAP * 2.0).max(0.0),
                body.height,
            )
            .clamped();
            (
                rest,
                Some(strip_layout(index, frame, metrics, &strips[index])),
            )
        }
        None => (body, None),
    };

    // Then the options column, off the right of what is left — but only when
    // the strips can still spare it. A mixer is its faders; a window dragged
    // narrow loses the column, not them.
    let wants_options = !strips.is_empty();
    let room_for_options = rest.width >= OPTIONS_WIDTH + GAP * 2.0 + STRIP_WIDTH * 2.0;
    let options_frame = if wants_options && room_for_options {
        let x = (rest.right() - OPTIONS_WIDTH).max(rest.x);
        let frame = Rect::new(x, rest.y, (rest.right() - x).max(0.0), rest.height).clamped();
        rest = Rect::new(
            rest.x,
            rest.y,
            (frame.x - GAP * 2.0 - rest.x).max(0.0),
            rest.height,
        )
        .clamped();
        Some(frame)
    } else {
        None
    };
    let list = rest;

    // Whole strips only, for the reason `rack_layout` gives about rows: a
    // strip clipped to a few pixels still draws its fader inside those pixels,
    // on top of the strip beside it.
    let mut out = Vec::new();
    let mut add_track = Rect::ZERO;
    if !list.is_empty() && STRIP_WIDTH > 0.0 {
        let columns = ((list.width + GAP) / (STRIP_WIDTH + GAP)).floor() as usize;
        let ordinary: Vec<usize> = (0..strips.len())
            .filter(|index| Some(*index) != master_index)
            .collect();
        let scroll = scroll.min(ordinary.len().saturating_sub(1));
        let shown = ordinary.len().saturating_sub(scroll).min(columns);
        let column = |slot: usize| {
            Rect::new(
                list.x + slot as f32 * (STRIP_WIDTH + GAP),
                list.y,
                STRIP_WIDTH,
                list.height,
            )
        };
        for (slot, index) in ordinary.into_iter().skip(scroll).take(shown).enumerate() {
            out.push(strip_layout(index, column(slot), metrics, &strips[index]));
        }
        // The `+` takes the next column along, if there is one. It is the last
        // thing to be given room: a panel full of strips shows the strips.
        if shown < columns {
            add_track = column(shown).clamped();
        }
    }

    let options = options_frame.map(|frame| {
        // Out of range, and `None`, fall back to a track that is there.
        let track = selected
            .filter(|index| *index < strips.len())
            .unwrap_or(0)
            .min(strips.len().saturating_sub(1));
        options_layout(track, frame, metrics, &strips[track])
    });

    MixerLayout {
        body,
        list,
        strips: out,
        master,
        add_track,
        options,
        total: strips.len(),
        scroll,
    }
}

/// The options column's insides, top to bottom: which track, where it goes,
/// and what is on it.
fn options_layout(
    track: usize,
    frame: Rect,
    metrics: &Metrics,
    strip: &MixerStrip,
) -> TrackOptionsLayout {
    let inner = frame.inset(GAP);
    let row = metrics.row_height.min(inner.height.max(0.0));

    // Each row comes off what is left, never measured from a shared edge —
    // the arithmetic that put two of the browser's footer rows in the same
    // pixels. See `browser_layout_for`.
    //
    // A function taking `&mut top` rather than a closure capturing it, so the
    // remaining height is still readable between the calls: how many insert
    // rows fit is a question about what is left.
    fn take(top: &mut f32, inner: Rect, wanted: f32) -> Rect {
        let height = wanted.min((inner.bottom() - *top).max(0.0));
        let taken = Rect::new(inner.x, *top, inner.width, height).clamped();
        *top = taken.bottom();
        taken
    }

    let mut top = inner.y;
    let title = take(&mut top, inner, row);
    take(&mut top, inner, GAP);
    let input = take(&mut top, inner, row);
    let output = take(&mut top, inner, row);
    take(&mut top, inner, GAP);
    let inserts_title = take(&mut top, inner, row);

    // As many rows as fit, then the add row. The **front** of the chain is
    // what a short column keeps: that is where the signal arrives, and a rack
    // that dropped its first effect to show its last would be describing a
    // signal path that does not exist.
    let mut inserts = Vec::new();
    for slot in 0..strip.inserts.len() {
        // One row's worth has to be left for the add button as well, or a full
        // chain has no way to grow.
        if inner.bottom() - top < OPTION_ROW_HEIGHT * 2.0 {
            break;
        }
        let frame = take(&mut top, inner, OPTION_ROW_HEIGHT);
        if frame.is_empty() {
            break;
        }
        inserts.push(insert_row(slot, frame));
    }
    let add_insert = if inner.bottom() - top >= OPTION_ROW_HEIGHT {
        take(&mut top, inner, OPTION_ROW_HEIGHT)
    } else {
        Rect::ZERO
    };

    // Then the sends, under the chain rather than among it: they are two
    // different things about a track, and a column that interleaved them would
    // read as one list.
    let mut sends_title = Rect::ZERO;
    let mut sends = Vec::new();
    let mut add_send = Rect::ZERO;
    if inner.bottom() - top >= row + OPTION_ROW_HEIGHT {
        take(&mut top, inner, GAP);
        sends_title = take(&mut top, inner, row);
        for index in 0..strip.sends.len() {
            // A row's worth left for the add button as well, or a track with
            // sends has no way to make another.
            if inner.bottom() - top < OPTION_ROW_HEIGHT * 2.0 {
                break;
            }
            let frame = take(&mut top, inner, OPTION_ROW_HEIGHT);
            if frame.is_empty() {
                break;
            }
            sends.push(send_row(index, frame));
        }
        if inner.bottom() - top >= OPTION_ROW_HEIGHT {
            add_send = take(&mut top, inner, OPTION_ROW_HEIGHT);
        }
    }

    TrackOptionsLayout {
        track,
        frame,
        title,
        input,
        output,
        inserts_title,
        inserts,
        add_insert,
        sends_title,
        sends,
        add_send,
    }
}

/// One send row: the tap switch, where it goes, how much, and a delete.
fn send_row(index: usize, frame: Rect) -> SendRowLayout {
    let tap = Rect::new(
        frame.x,
        frame.y,
        SEND_TAP_WIDTH.min(frame.width),
        frame.height,
    )
    .clamped();
    let remove = Rect::new(
        (frame.right() - OPTION_REMOVE_WIDTH).max(tap.right()),
        frame.y,
        OPTION_REMOVE_WIDTH.min((frame.right() - tap.right()).max(0.0)),
        frame.height,
    )
    .clamped();
    // The destination gets a name's worth, and the level takes the rest —
    // it is the thing you actually drag.
    let middle = (remove.x - tap.right()).max(0.0);
    let target = Rect::new(
        tap.right(),
        frame.y,
        (middle * SEND_TARGET_SHARE).max(0.0),
        frame.height,
    )
    .clamped();
    let level = Rect::new(
        target.right(),
        frame.y,
        (remove.x - target.right()).max(0.0),
        frame.height,
    )
    .clamped();
    SendRowLayout {
        index,
        frame,
        tap,
        target,
        level,
        remove,
    }
}

/// One insert row: switch, name, grip, delete.
/// How wide the wet/dry groove on an insert row is.
///
/// Wide enough for the dial and the number beside it, and no wider: the row is
/// 168 pixels and already carries a bypass, a name, a grip and a delete. An
/// effect's name is three or four letters, so the room comes out of the name.
const OPTION_MIX_WIDTH: f32 = 52.0;

fn insert_row(slot: usize, frame: Rect) -> InsertRowLayout {
    let bypass = Rect::new(
        frame.x,
        frame.y,
        OPTION_BYPASS_WIDTH.min(frame.width),
        frame.height,
    )
    .clamped();
    let remove = Rect::new(
        (frame.right() - OPTION_REMOVE_WIDTH).max(bypass.right()),
        frame.y,
        OPTION_REMOVE_WIDTH.min((frame.right() - bypass.right()).max(0.0)),
        frame.height,
    )
    .clamped();
    let grip = Rect::new(
        (remove.x - OPTION_GRIP_WIDTH).max(bypass.right()),
        frame.y,
        OPTION_GRIP_WIDTH.min((remove.x - bypass.right()).max(0.0)),
        frame.height,
    )
    .clamped();
    // Between the name and the grip. Full row height, because the dial wants
    // to be as round as the row is tall.
    let mix = Rect::new(
        (grip.x - OPTION_MIX_WIDTH).max(bypass.right()),
        frame.y + 1.0,
        OPTION_MIX_WIDTH.min((grip.x - bypass.right()).max(0.0)),
        (frame.height - 2.0).max(0.0),
    )
    .clamped();
    let name = Rect::new(
        bypass.right(),
        frame.y,
        (mix.x - bypass.right()).max(0.0),
        frame.height,
    )
    .clamped();
    InsertRowLayout {
        slot,
        frame,
        bypass,
        name,
        grip,
        remove,
        mix,
    }
}

/// What a wet/dry read-out says.
///
/// `dry` and `wet` at the ends rather than 0% and 100%, because those are the
/// two settings that mean something on their own: one is the effect switched
/// out of the sound, the other is the effect.
pub fn format_mix(mix: f32) -> String {
    let percent = (mix.clamp(0.0, 1.0) * 100.0).round() as i32;
    match percent {
        0 => "dry".to_string(),
        100 => "wet".to_string(),
        other => format!("{other}%"),
    }
}

/// Where the dial is drawn inside an insert row's wet/dry box.
///
/// A square at the leading end, with the number beside it. Its own function so
/// the renderer and the hit-test cannot disagree about where the knob is — the
/// same rule every other geometry decision in this crate follows.
///
/// The **box** is what you grab, not the circle: an 18-pixel dial is a small
/// target, and the number next to it is part of the same control.
pub fn insert_mix_dial(mix: Rect) -> Rect {
    let side = mix.height.min(mix.width);
    Rect::new(mix.x, mix.y, side, side).clamped()
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

/// What is under the pointer in the track-options column.
///
/// Its own enum rather than seven more variants on [`MixerHit`], because every
/// one of these is about *the selected track* and none of them carries a strip
/// index: the column is only ever about one track, and threading that index
/// through each variant would be inviting the two to disagree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptionsHit {
    /// The track's name, where a rename starts.
    Rename,
    /// Which input this track records from (TDD §15.4).
    Input,
    /// Where the track's output goes (§13.2).
    Output,
    /// Open the effect in this slot.
    Insert(usize),
    /// Switch it out of the chain, or back in.
    Bypass(usize),
    Remove(usize),
    /// How much of that insert is heard against the signal that went into it.
    /// Dragged.
    InsertMix(usize),
    /// Pick the row up to reorder it.
    Grip(usize),
    AddInsert,
    /// Where a send goes — press to change it (§13.2).
    Send(usize),
    /// Take it before the fader instead of after, or back.
    SendTap(usize),
    /// How much of the track goes down it. Dragged.
    SendLevel(usize),
    SendRemove(usize),
    AddSend,
}

/// What is under the pointer in the mixer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MixerHit {
    /// The track's name — select it. The rack, the roll and the options column
    /// all follow the selection.
    Name(usize),
    /// The strip's own body, away from every control. Also a select: a strip
    /// you have to aim at a 22-pixel caption to choose is a strip nobody
    /// realises they can choose at all.
    Strip(usize),
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
    /// The `+` column past the last strip.
    AddTrack,
    /// Something in the track-options column.
    Options(OptionsHit),
    Nothing,
}

impl OptionsHit {
    /// What a hover tip says (see [`crate::tooltip`]).
    pub fn tip(self) -> &'static str {
        match self {
            Self::Rename => "This is the track the options below are about",
            Self::Input => "Which input this track records from",
            Self::Output => "Where this track's sound goes",
            Self::Insert(_) => "Open this effect's controls",
            Self::Bypass(_) => "Switch this effect out, keeping its settings",
            Self::Remove(_) => "Take this effect off the track",
            Self::Grip(_) => "Drag to reorder \u{2014} order changes the sound",
            Self::InsertMix(_) => {
                "Wet/dry: drag to blend this effect with the sound that went into it"
            }
            Self::AddInsert => "Put another effect on the end of the chain",
            Self::Send(_) => "Where this send goes",
            Self::SendTap(_) => "Take the send before the fader, or after it",
            Self::SendLevel(_) => "How much of this track goes down the send",
            Self::SendRemove(_) => "Take this send off the track",
            Self::AddSend => "Send a copy of this track to another one",
        }
    }
}

impl MixerHit {
    /// What a hover tip says (see [`crate::tooltip`]).
    ///
    /// `None` where there is nothing to explain: a fader with a decibel
    /// read-out under it already says what it is.
    pub fn tip(self) -> Option<&'static str> {
        Some(match self {
            Self::Name(_) | Self::Strip(_) => "Select this track",
            Self::Fader(_) => "Level \u{2014} drag; double the detent snaps to unity",
            Self::Pan(_) => "Balance \u{2014} drag; the centre has a detent",
            Self::Mute(_) => "Silence this track",
            Self::Solo(_) => "Hear only this track and what feeds it",
            Self::Insert(_, _) => "Open this effect's controls",
            Self::BypassInsert(_, _) => "Switch this effect out, keeping its settings",
            Self::AddInsert(_) => "Put an effect on this track",
            Self::AddTrack => "Add a mixer track",
            Self::Options(what) => what.tip(),
            Self::Nothing => return None,
        })
    }
}

pub fn mixer_hit(layout: &MixerLayout, x: f32, y: f32) -> MixerHit {
    // The options column first. It is over the mixer's own body, and it is the
    // only thing here whose rows are small enough that anything else claiming
    // them would make them unreachable.
    if let Some(options) = &layout.options
        && options.frame.contains(x, y)
    {
        return options_hit(options, x, y);
    }
    if layout.add_track.contains(x, y) {
        return MixerHit::AddTrack;
    }
    // Then the master: it is outside the list, and a list that claimed the
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
        // Everything else on the strip — the read-out, the gaps between the
        // controls — selects it. Last, so it can never shadow a control.
        return MixerHit::Strip(strip.index);
    }
    MixerHit::Nothing
}

fn options_hit(options: &TrackOptionsLayout, x: f32, y: f32) -> MixerHit {
    for row in &options.inserts {
        if !row.frame.contains(x, y) {
            continue;
        }
        // Right to left: the two narrow targets at the end of the row, then
        // the name, which is everything left over.
        if row.remove.contains(x, y) {
            return MixerHit::Options(OptionsHit::Remove(row.slot));
        }
        if row.grip.contains(x, y) {
            return MixerHit::Options(OptionsHit::Grip(row.slot));
        }
        if row.bypass.contains(x, y) {
            return MixerHit::Options(OptionsHit::Bypass(row.slot));
        }
        if row.mix.contains(x, y) {
            return MixerHit::Options(OptionsHit::InsertMix(row.slot));
        }
        return MixerHit::Options(OptionsHit::Insert(row.slot));
    }
    for row in &options.sends {
        if !row.frame.contains(x, y) {
            continue;
        }
        if row.remove.contains(x, y) {
            return MixerHit::Options(OptionsHit::SendRemove(row.index));
        }
        if row.tap.contains(x, y) {
            return MixerHit::Options(OptionsHit::SendTap(row.index));
        }
        if row.target.contains(x, y) {
            return MixerHit::Options(OptionsHit::Send(row.index));
        }
        // The groove is everything left over, so a press just off the mark
        // still moves the level rather than doing nothing.
        return MixerHit::Options(OptionsHit::SendLevel(row.index));
    }
    if options.add_insert.contains(x, y) {
        return MixerHit::Options(OptionsHit::AddInsert);
    }
    if options.add_send.contains(x, y) {
        return MixerHit::Options(OptionsHit::AddSend);
    }
    if options.input.contains(x, y) {
        return MixerHit::Options(OptionsHit::Input);
    }
    if options.output.contains(x, y) {
        return MixerHit::Options(OptionsHit::Output);
    }
    if options.title.contains(x, y) {
        return MixerHit::Options(OptionsHit::Rename);
    }
    // Empty space in the column. Deliberately **not** a fall-through to the
    // strips underneath: a press that reached a fader you cannot see would
    // move a level for no visible reason.
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

/// The send level a press at `x` on the groove `level` is asking for.
///
/// **Linear in decibels**, unlike the strip's fader: a send has a shorter
/// travel and is usually set by ear against the dry signal rather than read
/// off a scale, and the fader's taper on a hundred-pixel groove would spend
/// most of it below -30 dB.
///
/// Clamped to the ends, so a drag that runs off the panel pins the level
/// rather than losing it.
pub fn send_level_at(level: Rect, x: f32) -> f32 {
    if level.width <= 0.0 {
        return MIN_SEND_DB;
    }
    let along = ((x - level.x) / level.width).clamp(0.0, 1.0);
    MIN_SEND_DB + (MAX_SEND_DB - MIN_SEND_DB) * along
}

/// The other direction: where the mark for `db` goes on the groove.
pub fn send_x_of_level(level: Rect, db: f32) -> f32 {
    let db = db.clamp(MIN_SEND_DB, MAX_SEND_DB);
    let along = (db - MIN_SEND_DB) / (MAX_SEND_DB - MIN_SEND_DB);
    level.x + level.width * along
}

/// What a send's level read-out says. `off` at the bottom, for the reason
/// [`format_gain_db`] gives.
pub fn format_send_db(db: f32) -> String {
    if db <= MIN_SEND_DB {
        return "off".to_string();
    }
    format!("{db:+.0}")
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
