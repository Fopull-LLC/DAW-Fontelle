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
    RackHit, RackLayout, TimelineLayout, TimelineView, lane_to_y, rack_hit, timeline_snap,
    timeline_tick_to_x, timeline_x_to_tick, y_to_lane,
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
    /// One of Flopsynth's oscillator cards: the sound becomes that
    /// oscillator's, as the recording it plays. `layer` is the patch's own
    /// index for it and `card` is which of the scene's
    /// [`CarryScene::oscillators`] it was, for the chip's name.
    ///
    /// > *"i tried doing this from the audio import tab and dragging an
    /// > audio file into flopsynth over one of my oscilator waveforms right
    /// > now and it didnt do anything unfortunately"*
    ///
    /// The desktop drop knew about the cards and the browser's own drag did
    /// not, which is exactly the kind of gap the one-function rule exists
    /// to close: the card lights up and the release loads it, off the same
    /// answer.
    Oscillator {
        layer: usize,
        card: usize,
        rect: Rect,
    },
    /// The arrangement: a clip starting at `tick`, on the row highlighted by
    /// `row`, whose left edge is at `at`.
    ///
    /// `lane` is the row the pointer is over — `Some(index)` into the
    /// arrangement's stack when it is over one that exists, so the clip lands
    /// *there* rather than on a row of its own; `None` when the pointer is in
    /// the empty space past the last row, which still makes a new row at the
    /// foot (`fontelle_model::AddAudioClip`). `row` is the band that lights up
    /// for whichever it is.
    Clip {
        row: Rect,
        at: f32,
        tick: Tick,
        lane: Option<usize>,
    },
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
            | Self::Instrument { rect, .. }
            | Self::Oscillator { rect, .. } => Some(*rect),
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
    /// Flopsynth's oscillator cards, when that is the window the pointer is
    /// in: each one takes a sound. Empty for every other window.
    pub oscillators: &'a [CarryOscillator],
}

/// One oscillator card a sound can be dropped on.
#[derive(Debug, Clone, PartialEq)]
pub struct CarryOscillator {
    /// The patch's index for the oscillator, which is what the load takes.
    pub layer: usize,
    pub frame: Rect,
    /// What the card is called, for the chip.
    pub name: String,
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
    // An oscillator card takes a **sound** and nothing else: a preset is a
    // whole instrument, and there is nothing for one to become on one
    // oscillator of another.
    if scene.carried == Carried::Audio
        && let Some((card, osc)) = scene
            .oscillators
            .iter()
            .enumerate()
            .find(|(_, osc)| !osc.frame.is_empty() && osc.frame.contains(x, y))
    {
        return CarryTarget::Oscillator {
            layer: osc.layer,
            card,
            rect: osc.frame,
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
        // Snap to whatever grid the arrangement is on, so a dropped clip lines
        // up with the bars and beats already there rather than landing a few
        // pixels off — the "snapping to my current grid" the report asks for.
        let tick = timeline_snap(timeline.view, loose, timeline.beats_per_bar).max(0);
        let height = timeline.view.lane_height.min(grid.height);
        // **The row under the pointer**, not always a new one at the foot: a
        // drop over an existing row lands on it (`Some(index)`), and only a
        // drop into the empty space past the last row makes a row of its own
        // (`None`). This is the "put it where I'm dragging it, nearest lane"
        // the report asks for; the old always-a-new-row behaviour was the
        // thing it was asking to be rid of.
        let under = y_to_lane(timeline.view, grid, y);
        let (lane, row_lane) = if under < timeline.lanes {
            (Some(under), under)
        } else {
            // Past the bottom: a new row, drawn where `AddAudioClip` will make
            // it, and held at the foot of the grid when that is off-screen so
            // the drop still shows feedback.
            (None, timeline.lanes)
        };
        let y = lane_to_y(timeline.view, grid, row_lane)
            .clamp(grid.y, (grid.bottom() - height).max(grid.y));
        let row = Rect::new(grid.x, y, grid.width, height).intersection(&grid);
        return CarryTarget::Clip {
            row,
            at: timeline_tick_to_x(timeline.view, grid, tick),
            tick,
            lane,
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
pub fn carry_note(
    target: &CarryTarget,
    channels: &[String],
    oscillators: &[String],
    beats_per_bar: u32,
) -> String {
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
        CarryTarget::Oscillator { card, .. } => match oscillators.get(*card) {
            Some(name) if !name.is_empty() => format!("As {name}\u{2019}s sound"),
            _ => "As this oscillator\u{2019}s sound".to_string(),
        },
        CarryTarget::NewChannel { .. } => "A new channel".to_string(),
        CarryTarget::Clip { tick, lane, .. } => match lane {
            Some(index) => format!(
                "Onto row {} at bar {}",
                index + 1,
                bar_label(*tick, beats_per_bar)
            ),
            None => format!("A new row at bar {}", bar_label(*tick, beats_per_bar)),
        },
        CarryTarget::Nowhere => "Nowhere to put this".to_string(),
        CarryTarget::Panel => String::new(),
    }
}

/// What the chip says while the row is **held** (see [`carry_release`]):
/// the landing, when there is one under the pointer, and otherwise how to
/// put the row down — because a chip that follows the pointer with nothing
/// under it has to say it is waiting, not stuck.
pub fn held_note(target: &CarryTarget, note: &str) -> String {
    if target.lands() {
        note.to_string()
    } else {
        "Click where it goes \u{b7} Esc lets go".to_string()
    }
}

/// What letting go of a carried row does, by where the pointer is.
///
/// > *"when i try to drag it out it gets stuck inside the main daw window."*
///
/// A press grabs the pointer for the window it happened in, and on Wayland
/// the grab holds until the button comes up: the studio hears every move
/// and the release, and the synth window hears nothing until after. So a
/// row let go over the synth window is, to the studio, a row let go
/// **outside its own bounds** — which used to be "nowhere", and was the
/// row stuck at the edge. Now:
///
/// - inside the studio, or in a floating window that did get the pointer,
///   it **drops** where it is, as it always did;
/// - outside the studio with a floating window open, it is **held**: the
///   chip stays on the pointer and the next click puts it down, in
///   whichever window that click lands (or Esc lets go);
/// - outside with nothing open, it is let go — there is nowhere for it.
///
/// `pointer` is in the studio's coordinates unless `in_floating` says the
/// pointer is in a floating window, where it is that window's. A pointer
/// the compositor has taken away arrives as `f32::MIN`, which is outside.
pub fn carry_release(
    studio: Rect,
    pointer: (f32, f32),
    in_floating: bool,
    floating_open: bool,
) -> CarryRelease {
    if in_floating || studio.contains(pointer.0, pointer.1) {
        CarryRelease::Drop
    } else if floating_open {
        CarryRelease::Hold
    } else {
        CarryRelease::Cancel
    }
}

/// [`carry_release`]'s answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CarryRelease {
    /// Act on where the pointer is.
    Drop,
    /// Keep the row on the pointer until a click puts it down.
    Hold,
    /// Nothing, and nothing carried any more.
    Cancel,
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
