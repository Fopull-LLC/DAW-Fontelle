//! What a row carried out of the browser would land on, and what that looks
//! like while it is still in the air.
//!
//! > *"i cant see any visuals of the thing being dragged when i click and drag
//! > something for example an audio clip from the import section im trying to
//! > drag into the channel rack or playlist to turn into an instrument or clip.
//! > please also ensure that it shows a visual of where its about to go so you
//! > know youre actually placing it right / that is a legal action before you
//! > do it. right now theres virtually no feedback until you actually finish
//! > dragging it."*
//!
//! The drag was already there — a press on a row armed it and the release
//! acted on it (`Drag::BrowserRow`) — but the question *"where would this
//! go?"* was answered inside the release handler and nowhere else, so there
//! was nothing for a frame in the middle of the drag to draw. Everything
//! about the gesture was therefore invisible until it was over.
//!
//! [`carry_target`] is that answer, lifted out as a pure function of the
//! geometry. It is asked twice: once per pointer move, to light up what would
//! take the row and to write the chip under the cursor, and once on release,
//! to make the edit. That is the property worth having and the reason this is
//! a module rather than a highlight painted beside the old code — **the mark
//! and the drop read the same function**, so a highlight cannot promise
//! something the release will not do.
//!
//! Pure, per §2.5 of `docs/first-usable-plan.md`: no document, no window, and
//! tested in `fontelle-ui/tests/carry.rs`.

use fontelle_types::{PPQN, Tick};

use crate::canvas::{
    RackHit, RackLayout, TimelineLayout, TimelineView, rack_hit, timeline_snap, timeline_tick_to_x,
    timeline_x_to_tick,
};
use crate::layout::Rect;

/// What is in the air.
///
/// Two kinds, because two kinds of row can be picked up and they mean
/// different things. A preset is **what a channel plays** and nothing else. A
/// sound out of the Import tab can be that *or* a stretch of the song, which
/// is why only one of them has a landing on the arrangement. Nothing else is
/// carried — see [`super::browser_row_carries`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Carried {
    /// A preset of the open soundfont or device.
    Preset,
    /// An audio file out of the Import tab.
    Audio,
}

/// Where a carried row would land if the button came up now.
///
/// Each variant carries the rectangle to light up, because the *thing that
/// would change* is what has to be drawn: a row, the band a new row would
/// appear in, a name field, the row a clip would be made on. A variant with
/// no rectangle would need a second function to work one out, and the two
/// would eventually disagree.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CarryTarget {
    /// Channel `index` of the rack: it plays this instead of what it had.
    Channel { index: usize, rect: Rect },
    /// The rack itself: a channel of its own, in the band `rect`.
    NewChannel { rect: Rect },
    /// The open instrument window's name field, which stands for the channel
    /// that window is showing.
    Instrument { channel: usize, rect: Rect },
    /// The arrangement: a clip starting at `tick`, on a new row along `row`,
    /// whose left edge is at `at`.
    ///
    /// The row is at the **foot** of the grid because that is where
    /// `fontelle_model::AddAudioClip` puts the row it makes, and a mark drawn
    /// on the lane under the pointer would be a picture of something else
    /// happening.
    Clip { row: Rect, at: f32, tick: Tick },
    /// The panel it came out of. Not a drop and not a mistake either: a press
    /// that never leaves the list is a **click**, and the click is what the
    /// release does.
    Panel,
    /// Nowhere. Letting go here does nothing at all.
    Nowhere,
}

impl CarryTarget {
    /// Whether letting go here actually does something.
    pub fn lands(&self) -> bool {
        !matches!(self, Self::Panel | Self::Nowhere)
    }

    /// Whether this is a place that has to be *refused* — drawn so that the
    /// gesture visibly will not work.
    ///
    /// [`Panel`](Self::Panel) is neither: the row is exactly where it started
    /// and nothing is wrong, so marking it up in the warning ink would be
    /// telling somebody off for holding the mouse still.
    pub fn refuses(&self) -> bool {
        matches!(self, Self::Nowhere)
    }

    /// The rectangle that lights up, if there is one.
    pub fn mark(&self) -> Option<Rect> {
        match self {
            Self::Channel { rect, .. }
            | Self::NewChannel { rect }
            | Self::Instrument { rect, .. } => Some(*rect),
            Self::Clip { row, .. } => Some(*row),
            Self::Panel | Self::Nowhere => None,
        }
    }
}

/// The channel rack, when its list is the one the panel is showing.
///
/// `frame` is the whole panel and `layout` is the list inside it: any part of
/// the panel that is not a row means *the rack*, which is what makes the empty
/// space under the rows into "a channel of its own".
pub struct CarryRack<'a> {
    pub frame: Rect,
    pub layout: &'a RackLayout,
}

/// The arrangement, when it is showing.
///
/// The **grid** is the target and the rest of the panel is chrome — a clip
/// cannot start on the bar ruler — so unlike [`CarryRack`] there is no frame
/// here: the layout already says where the grid is.
pub struct CarryTimeline<'a> {
    pub layout: &'a TimelineLayout,
    pub view: &'a TimelineView,
    pub beats_per_bar: u32,
    /// How many lanes the project has, because an imported sound arrives on a
    /// **new** one after the last of them — see [`CarryTarget::Clip`].
    pub lanes: usize,
}

/// Everything the decision needs, borrowed from the window.
///
/// A wide struct rather than eight arguments, the same arrangement
/// [`crate::pointer::PointerScene`] has and for the same reason: every field
/// is geometry the window has computed anyway, and naming them at the call
/// site is what stops two of them being passed the wrong way round.
///
/// Every panel is optional, and that is how a pointer in **another window**
/// is described: an editor window's coordinates are its own, so while the
/// pointer is over one the studio's panels are not in the scene at all and
/// only [`name`](Self::name) is filled in.
pub struct CarryScene<'a> {
    pub carried: Carried,
    pub rack: Option<CarryRack<'a>>,
    /// The panel the row was dragged out of.
    pub panel: Option<Rect>,
    pub timeline: Option<CarryTimeline<'a>>,
    /// The instrument window's name field and the channel it stands for.
    pub name: Option<(usize, Rect)>,
}

/// Where the row would land, with the pointer at `(x, y)`.
///
/// The order the places are asked in is the order they are drawn in, which is
/// the rule `press` already follows: a floating window is above the studio,
/// and a panel is above the gaps between panels.
pub fn carry_target(scene: &CarryScene<'_>, x: f32, y: f32) -> CarryTarget {
    // The instrument window first: while the pointer is in it, nothing else in
    // the scene is even in the same coordinate space.
    if let Some((channel, field)) = scene.name
        && !field.is_empty()
        && field.contains(x, y)
    {
        return CarryTarget::Instrument {
            channel,
            rect: field,
        };
    }

    if let Some(rack) = &scene.rack
        && rack.frame.contains(x, y)
    {
        // Any part of a row is that row — the rule the right-click menu
        // already follows, because aiming at a caption is not a thing anybody
        // should have to do.
        if let RackHit::Row(index)
        | RackHit::Mute(index)
        | RackHit::Solo(index)
        | RackHit::Edit(index)
        | RackHit::Route(index) = rack_hit(rack.layout, x, y)
        {
            let rect = rack
                .layout
                .rows
                .iter()
                .find(|row| row.index == index)
                .map_or(Rect::ZERO, |row| row.frame);
            return CarryTarget::Channel { index, rect };
        }
        return CarryTarget::NewChannel {
            rect: new_channel_band(rack.layout),
        };
    }

    if let Some(panel) = scene.panel
        && panel.contains(x, y)
    {
        return CarryTarget::Panel;
    }

    // The arrangement takes a **sound** and nothing else: a preset is what a
    // channel plays, and there is nothing for one to become on a row of song.
    if let Some(timeline) = &scene.timeline
        && scene.carried == Carried::Audio
        && timeline.layout.grid.contains(x, y)
    {
        let grid = timeline.layout.grid;
        let loose = timeline_x_to_tick(timeline.view, grid, x);
        let tick = timeline_snap(timeline.view, loose, timeline.beats_per_bar).max(0);
        let height = timeline.view.lane_height.min(grid.height);
        // The row **after the ones that are there**, which is where
        // `fontelle_model::AddAudioClip` puts the lane it makes. Held at the
        // foot of the grid when that row is off the bottom of it: a project
        // with forty lanes is exactly the one where a mark that quietly
        // disappeared would leave the drop with no feedback at all.
        let y = crate::canvas::lane_to_y(timeline.view, grid, timeline.lanes)
            .clamp(grid.y, (grid.bottom() - height).max(grid.y));
        let row = Rect::new(grid.x, y, grid.width, height).intersection(&grid);
        return CarryTarget::Clip {
            row,
            at: timeline_tick_to_x(timeline.view, grid, tick),
            tick,
        };
    }

    CarryTarget::Nowhere
}

/// The band a new channel would appear in: one row tall, after the rows that
/// are already there, and never outside the list.
///
/// Drawn rather than left to "the whole list lights up", because *where in the
/// list* is the question a drop on the rack raises — a new channel goes on the
/// end, and a mark over the rows that are already there reads as replacing
/// one.
fn new_channel_band(rack: &RackLayout) -> Rect {
    let list = rack.list;
    if list.is_empty() {
        return Rect::ZERO;
    }
    let after = rack
        .rows
        .last()
        .map_or(list.y, |row| row.frame.bottom())
        .clamp(list.y, list.bottom());
    let height = rack
        .rows
        .first()
        .map_or(list.height, |row| row.frame.height)
        .min(list.height);
    // Off the bottom of a full list, the band is the last row's worth of space
    // rather than nothing: a mark that vanishes once the panel fills up is a
    // mark you cannot trust.
    let y = after.min((list.bottom() - height).max(list.y));
    Rect::new(list.x, y, list.width, height).intersection(&list)
}

/// What the chip under the pointer says on its second line: what letting go
/// here would do, in words.
///
/// A function rather than a `format!` at the call site, for the reason
/// [`crate::render::voice_count_label`] gives — the window shapes its text
/// ahead of drawing it, and the two halves have to ask for the same string.
///
/// An empty string means there is nothing to say, and draws as one line.
pub fn carry_note(target: &CarryTarget, channels: &[String], beats_per_bar: u32) -> String {
    let name = |index: usize| match channels.get(index) {
        Some(name) if !name.is_empty() => format!("Onto {name}"),
        // A channel past the end of the list somebody handed us. Still a
        // sentence: this is drawn under the pointer mid-gesture, and a blank
        // line there reads as the drag having broken.
        _ => format!("Onto channel {}", index + 1),
    };
    match target {
        CarryTarget::Channel { index, .. } => name(*index),
        CarryTarget::Instrument { channel, .. } => name(*channel),
        CarryTarget::NewChannel { .. } => "A new channel".to_string(),
        CarryTarget::Clip { tick, .. } => {
            format!("A new row at bar {}", bar_label(*tick, beats_per_bar))
        }
        CarryTarget::Nowhere => "Nowhere to put this".to_string(),
        CarryTarget::Panel => String::new(),
    }
}

/// A tick as a bar number, with the beat after it when it is not on the bar
/// line — which it can be, since the arrangement's snap can be off.
///
/// One-based, like [`crate::transport::format_bars_beats`], because a musician
/// counts from one and the transport read-out three inches away already does.
fn bar_label(tick: Tick, beats_per_bar: u32) -> String {
    let beats = Tick::from(beats_per_bar.max(1));
    let bar = PPQN * beats;
    let tick = tick.max(0);
    let number = tick / bar + 1;
    let beat = (tick % bar) / PPQN;
    if beat == 0 {
        format!("{number}")
    } else {
        format!("{number}.{}", beat + 1)
    }
}

/// Air between the chip's words and its edge.
pub const CARRY_PAD: f32 = 6.0;

/// How far the chip clears the pointer.
///
/// Enough to be out from under a normal cursor, which is about sixteen pixels
/// tall on every desktop this runs on — the same judgement
/// [`crate::tooltip`]'s clearance is.
const CARRY_CLEARANCE: f32 = 14.0;

/// Where the chip of `size` goes with the pointer at `pointer`, inside
/// `bounds`.
///
/// Down and to the right of the pointer, which is where every desktop hangs
/// the thing it is carrying, and flipped rather than slid when there is no
/// room that way: a chip pinned against the right edge would sit **under** the
/// cursor, hiding the row it is about to be dropped on.
///
/// Empty when the window cannot hold the chip at all, which draws as nothing.
pub fn carry_chip(size: (f32, f32), pointer: (f32, f32), bounds: Rect) -> Rect {
    let (width, height) = size;
    if bounds.is_empty() || width > bounds.width || height > bounds.height {
        return Rect::ZERO;
    }
    let (px, py) = pointer;
    let right = px + CARRY_CLEARANCE;
    let x = if right + width <= bounds.right() {
        right
    } else {
        (px - CARRY_CLEARANCE - width).clamp(bounds.x, (bounds.right() - width).max(bounds.x))
    };
    let below = py + CARRY_CLEARANCE;
    let y = if below + height <= bounds.bottom() {
        below
    } else {
        (py - CARRY_CLEARANCE - height).clamp(bounds.y, (bounds.bottom() - height).max(bounds.y))
    };
    Rect::new(x, y, width, height).intersection(&bounds)
}
