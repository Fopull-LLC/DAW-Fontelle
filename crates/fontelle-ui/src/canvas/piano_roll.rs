//! The piano roll (TDD §16.4, §16.5; items 8 and 9 of
//! `docs/first-usable-plan.md`).
//!
//! A direct-draw canvas, not a widget tree. Almost all of it is here, as pure
//! functions and one small state machine, because §2.5 says so and because the
//! alternative — geometry and drag logic tangled into an event handler — is the
//! part of a GUI that is impossible to test and easy to get subtly wrong.
//!
//! **Virtualisation is mandatory** (§16.4): [`visible_ticks`] and
//! [`visible_keys`] depend on the viewport and the zoom and on nothing else, so
//! a project with 100 000 notes costs the same per frame as a project with ten.
//!
//! **The roll is a view, never a mutator** (INVARIANT 2). It reads notes and
//! returns [`RollEdit`]s; turning those into `Command`s and applying them is
//! `fontelle-app`'s job. That is why this file can be tested with an `Arena`
//! and no document at all.
//!
//! # The shape of a gesture
//!
//! One thing here is not obvious and is load-bearing. Drawing a note and
//! sizing it is **one** mouse gesture, the way it is in FL Studio: press on
//! empty grid, and the same drag that follows sets the length. But the roll
//! does not own the document, so at the moment of the press it does not know
//! the id of the note it just asked for. So the press leaves a *pending* add,
//! the host applies the edit and calls [`PianoRoll::note_added`] with the id it
//! minted, and the gesture becomes a resize of that note. Without that
//! handshake the drag has nothing to resize, which is exactly what the first
//! version of this file did and exactly why it felt wrong to use.

use std::ops::Range;

use fontelle_model::{Arena, Note, NoteProperty};
use fontelle_types::{NoteId, PPQN, Tick};

use crate::layout::Rect;
use crate::theme::Metrics;

/// The lowest and highest MIDI keys, and the count between them.
const KEY_COUNT: i32 = 128;

/// What an arrow key moves a note by when the snap is off — a sixty-fourth.
///
/// Free positioning turns the *grid* off, not the keyboard.
const FINE_STEP: Tick = PPQN / 16;

/// How wide a note's resize handle is, in pixels.
///
/// Eight rather than six: this is the control people reach for most often after
/// the note body itself, and a six-pixel target at a normal zoom is a target
/// you miss. [`hit_test`] still shrinks it for notes too short to spare it.
const HANDLE_PX: f32 = 8.0;

/// Zoom limits. Both ends matter: `pixels_per_tick` at zero is a division by
/// zero in every conversion here, and a key row taller than the panel is not a
/// zoom, it is a broken window.
pub const MIN_PIXELS_PER_TICK: f32 = 0.002;
pub const MAX_PIXELS_PER_TICK: f32 = 4.0;
pub const MIN_KEY_HEIGHT: f32 = 5.0;
pub const MAX_KEY_HEIGHT: f32 = 40.0;

/// Which modifier keys are down.
///
/// Pushed in by the window rather than passed to every method: §16.5 gives Alt
/// and Shift meanings that apply to whatever gesture is in progress, and
/// threading two booleans through nine entry points is how one of them ends up
/// forgotten.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub ctrl: bool,
    pub shift: bool,
    /// §16.5: free positioning — the snap is bypassed for as long as it is
    /// held.
    pub alt: bool,
}

/// Where the roll is looking, and how closely.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RollView {
    /// The tick at the left edge of the grid.
    pub scroll_tick: Tick,
    /// The MIDI key whose row is at the *top* of the grid. Pitch increases
    /// upwards, which is the one thing everybody gets backwards once.
    pub top_key: u8,
    /// Horizontal zoom.
    pub pixels_per_tick: f32,
    /// Row height — the vertical zoom. §16.4: continuous, and independent of
    /// the horizontal one.
    pub key_height: f32,
    pub snap: SnapDivision,
}

impl Default for RollView {
    fn default() -> Self {
        Self {
            scroll_tick: 0,
            // C6 at the top puts middle C comfortably in view on a normal
            // window without anyone having to scroll to find it.
            top_key: 84,
            pixels_per_tick: 0.125,
            // Tall enough for a key's name to fit beside it and for a note to
            // be an easy target. Twelve was neither.
            key_height: 16.0,
            snap: SnapDivision::Step,
        }
    }
}

/// The roll's parts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RollLayout {
    pub frame: Rect,
    /// The tools, the snap chip and the zoom buttons — the answer to "I cannot
    /// see what this can do".
    pub toolbar: Rect,
    /// The bar ruler across the top, above the grid.
    pub ruler: Rect,
    /// The keyboard down the left, beside the grid.
    pub keys: Rect,
    /// Where the notes are.
    pub grid: Rect,
    /// The label strip beside the property lane, lined up with the keyboard.
    pub velocity_keys: Rect,
    /// The property lane (§16.5's note property lanes — velocity, pan, and the
    /// rest, one at a time). Empty when it is hidden, and then the grid has the
    /// room instead.
    pub velocity: Rect,
    /// The seam between the grid and the lane, which is the thing you drag to
    /// make the lane taller. Empty when the lane is hidden — there is no seam.
    pub lane_grip: Rect,
}

/// How the strip down the side of the roll is drawn (TDD §16.4).
///
/// Asked for from using the roll: *"there should also be view options to switch
/// between a piano visual view or just a plain list of names... this would be
/// useful for drums since like right now if a drum sound if on a black key i
/// cant even read it."*
///
/// The two are not decoration. A piano is the right picture for a melodic
/// instrument, where the pattern of blacks and whites is how you find your
/// place; it is the wrong one for a kit, where the keys are a list of sounds
/// and the black ones are the ones you cannot read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum KeyStyle {
    /// Naturals and accidentals, drawn as a keyboard — but **on even rows**,
    /// which is the fix for "the E key is smaller": a real keyboard's whites
    /// are not all the same height and a grid's rows are.
    #[default]
    Piano,
    /// One even band per key, each carrying the name of what is on it. No
    /// black keys, because there is nothing to find your place by in a kit.
    Names,
}

impl KeyStyle {
    /// What the toolbar chip says.
    pub fn label(self) -> &'static str {
        match self {
            Self::Piano => "keys",
            Self::Names => "list",
        }
    }

    /// The other one. There are two, so a press is a toggle.
    pub fn next(self) -> Self {
        match self {
            Self::Piano => Self::Names,
            Self::Names => Self::Piano,
        }
    }

    pub fn tip(self) -> &'static str {
        match self {
            Self::Piano => "The keyboard down the side. Click for a list of names instead",
            Self::Names => "A name per key. Click for the keyboard instead",
        }
    }
}

/// Wide enough for a key name and narrow enough not to eat the song.
pub const KEYBOARD_WIDTH: f32 = 56.0;

/// And the width for an instrument whose keys have names of their own — a drum
/// kit, where the row is not "C2" but "Kick".
///
/// A second width rather than one width that fits everything, because the
/// widest thing the strip ever has to hold is the *rare* case. Making every
/// session pay 76 pixels of song for a label only a kit has would be the
/// familiar mistake of designing the common case around the uncommon one.
/// Only a key map with names asks for this — see [`keyboard_width`].
pub const NAMED_KEYBOARD_WIDTH: f32 = 132.0;

/// How wide the key strip should be for `map`.
///
/// A pure function of the key map so the decision is made once and tested
/// without a window, rather than being a comparison the window does twice —
/// once to lay out and once to draw — and gets subtly different answers from.
pub fn keyboard_width(map: &crate::document::KeyMap) -> f32 {
    keyboard_width_for(map, KeyStyle::Piano)
}

/// The same, for a strip drawn in a given style.
///
/// The list view always takes the wider strip: a column of names on 56 pixels
/// is a column of first syllables, and the whole reason to switch to it is to
/// read them.
pub fn keyboard_width_for(map: &crate::document::KeyMap, style: KeyStyle) -> f32 {
    if map.is_named() || style == KeyStyle::Names {
        NAMED_KEYBOARD_WIDTH
    } else {
        KEYBOARD_WIDTH
    }
}

/// How tall the property lane is until somebody drags it.
pub const DEFAULT_LANE_HEIGHT: f32 = 78.0;

/// The shortest a lane may be dragged. Below this the bars are too short to
/// aim at, which makes the lane a decoration rather than a control.
pub const MIN_LANE_HEIGHT: f32 = 32.0;

/// The most of the space under the ruler the lane may take. The grid is what
/// the panel is for; a lane that can eat it is a lane that will.
pub const MAX_LANE_FRACTION: f32 = 0.7;

/// How tall the drag target on the seam is.
///
/// Reported as *"the velocity / pan etc. section at the bottom does have a
/// knob to drag it but i am unable to drag it"*. It was five, which is three
/// physical rows on a scaled display — thin enough to miss every time and then
/// conclude the thing is not draggable at all. The extra pixels come out of
/// the **grid**, never the lane: a grip over the top of the bars would eat the
/// clicks that set a velocity to its loudest.
const LANE_GRIP: f32 = 9.0;

/// The lane height a drag of the grip to `y` is asking for, clamped to what a
/// lane may be.
///
/// Its own function, and pure, because "the lane cannot be dragged shut and
/// cannot swallow the grid" is exactly the kind of rule that gets written once
/// in an event handler and then not held.
pub fn lane_height_at(layout: &RollLayout, y: f32) -> f32 {
    let below = (layout.frame.bottom() - layout.grid.y).max(0.0);
    let ceiling = (below * MAX_LANE_FRACTION).max(0.0);
    (layout.frame.bottom() - y).clamp(MIN_LANE_HEIGHT.min(ceiling), ceiling)
}

/// Lays the roll out with a property lane `lane_height` pixels tall. **Zero
/// hides the lane**, and then the grid has the room instead.
///
/// The keyboard is [`KEYBOARD_WIDTH`]. See [`roll_layout_with_keys`] for the
/// wider strip a named key map asks for.
pub fn roll_layout(frame: Rect, metrics: &Metrics, lane_height: f32) -> RollLayout {
    roll_layout_with_keys(frame, metrics, lane_height, KEYBOARD_WIDTH)
}

/// [`roll_layout`], with the key strip a given width.
///
/// The width is a parameter rather than a constant because it depends on the
/// instrument: a drum kit's rows carry names and a piano's do not (see
/// [`keyboard_width`]). It comes out of the **grid's** room and nothing
/// else's — the ruler, the toolbar and the property lane are all where they
/// were — which is what keeps widening the strip from reflowing the panel.
pub fn roll_layout_with_keys(
    frame: Rect,
    metrics: &Metrics,
    lane_height: f32,
    keys_width: f32,
) -> RollLayout {
    let (toolbar, under_toolbar) = frame.split_top(metrics.row_height.min(frame.height.max(0.0)));

    let ruler_height = metrics.row_height;
    // Clamped to the panel: a strip wider than the window is a window with no
    // grid in it, which is not a keyboard, it is a bug.
    let keys_width = keys_width.max(0.0).min(under_toolbar.width.max(0.0));

    let (ruler, below) = under_toolbar.split_top(ruler_height);

    // The lane comes out of the bottom before the grid is measured, so the
    // grid never has to be shrunk after the fact — which is the version of
    // this that leaves a one-pixel seam.
    let lane_height = if lane_height > 0.0 && below.height > 0.0 {
        lane_height
            .max(MIN_LANE_HEIGHT)
            .min((below.height * MAX_LANE_FRACTION).max(0.0))
    } else {
        0.0
    };
    let grid_area = Rect::new(
        below.x,
        below.y,
        below.width,
        (below.height - lane_height).max(0.0),
    )
    .clamped();
    let lane_area = Rect::new(
        below.x,
        grid_area.bottom(),
        below.width,
        below.bottom() - grid_area.bottom(),
    )
    .clamped();

    let keys = Rect::new(grid_area.x, grid_area.y, keys_width, grid_area.height).clamped();
    let grid = Rect::new(
        keys.right(),
        grid_area.y,
        grid_area.width - keys.width,
        grid_area.height,
    )
    .clamped();

    let velocity_keys = Rect::new(lane_area.x, lane_area.y, keys_width, lane_area.height).clamped();
    let velocity = Rect::new(
        velocity_keys.right(),
        lane_area.y,
        lane_area.width - velocity_keys.width,
        lane_area.height,
    )
    .clamped();

    // Above the lane, never inside it: the seam belongs to the boundary, and a
    // grip drawn over the top row of bars is a grip that eats clicks meant for
    // them.
    let lane_grip = if lane_area.is_empty() {
        Rect::ZERO
    } else {
        Rect::new(
            below.x,
            (lane_area.y - LANE_GRIP).max(below.y),
            below.width,
            LANE_GRIP.min((lane_area.y - below.y).max(0.0)),
        )
        .clamped()
    };

    RollLayout {
        frame,
        toolbar,
        keys,
        ruler,
        grid,
        velocity_keys,
        velocity,
        lane_grip,
    }
}

// ------------------------------------------------------------- geometry ---

pub fn tick_to_x(view: &RollView, grid: Rect, tick: Tick) -> f32 {
    grid.x + (tick - view.scroll_tick) as f32 * view.pixels_per_tick
}

/// Clamped at zero: scrubbing left of bar 1 is the start of the song, not a
/// negative tick that every downstream conversion would have to defend against.
pub fn x_to_tick(view: &RollView, grid: Rect, x: f32) -> Tick {
    if view.pixels_per_tick <= 0.0 {
        return view.scroll_tick.max(0);
    }
    let tick = view.scroll_tick as f32 + (x - grid.x) / view.pixels_per_tick;
    (tick.round() as Tick).max(0)
}

/// The top of `key`'s row.
pub fn key_to_y(view: &RollView, grid: Rect, key: u8) -> f32 {
    grid.y + (i32::from(view.top_key) - i32::from(key)) as f32 * view.key_height
}

/// The rectangle one key's row occupies, **snapped to whole pixels**.
///
/// This is what the keyboard, the grid's rows and the notes in them are all
/// drawn against, so none of the three can disagree with the others by a
/// pixel. Two things make the rows even:
///
/// - the top is rounded, so a grid that starts on half a pixel does not put
///   every row on half a pixel; and
/// - the height is [`RollView::key_height`], which [`zoom_y`] keeps a whole
///   number — so every row is the same height rather than alternating between
///   two of them as a fractional zoom is rounded off.
///
/// Reported as *"inconsistant sizing on the notes in the piano roll"*, which
/// is what a row height of 23.04 looks like once it has been rasterised.
pub fn key_row(view: &RollView, grid: Rect, key: u8) -> Rect {
    let height = view.key_height.round().max(1.0);
    Rect::new(grid.x, key_to_y(view, grid, key).round(), grid.width, height)
}

pub fn y_to_key(view: &RollView, grid: Rect, y: f32) -> u8 {
    if view.key_height <= 0.0 {
        return view.top_key;
    }
    let rows = ((y - grid.y) / view.key_height).floor() as i32;
    (i32::from(view.top_key) - rows).clamp(0, KEY_COUNT - 1) as u8
}

/// The tick range the grid can show, inclusive of the partly-visible column on
/// the right — build geometry for this and for nothing else (§16.4).
pub fn visible_ticks(view: &RollView, grid: Rect) -> Range<Tick> {
    if view.pixels_per_tick <= 0.0 || grid.width <= 0.0 {
        return view.scroll_tick..view.scroll_tick;
    }
    let span = (grid.width / view.pixels_per_tick).ceil() as Tick;
    view.scroll_tick..view.scroll_tick + span + 1
}

/// The keys the grid can show, high to low, clamped to what MIDI has.
///
/// Returned as `i32` rather than `u8` so an exclusive end of 128 can be said at
/// all.
pub fn visible_keys(view: &RollView, grid: Rect) -> Range<i32> {
    if view.key_height <= 0.0 || grid.height <= 0.0 {
        return 0..0;
    }
    let rows = (grid.height / view.key_height).ceil() as i32;
    let top = i32::from(view.top_key);
    let bottom = top - rows + 1;
    bottom.max(0)..(top + 1).min(KEY_COUNT)
}

/// A pointer position brought back inside the grid.
///
/// **This is the fix for the roll feeling broken near bar 1.** Dragging a note
/// towards the start of a clip walks the pointer off the left of the grid and
/// onto the keyboard, and dragging one upwards walks it onto the ruler — both
/// of them one small movement away at any normal zoom. Converted unclamped,
/// left of the grid gave a negative tick that [`x_to_tick`] flattened to zero
/// and above the grid gave a negative row count that ran the key up to 127, so
/// a gesture that strayed a few pixels teleported the note. Clamping first
/// means a drag that leaves the grid keeps meaning the nearest cell inside it,
/// which is what it looks like it means.
///
/// The far edges are half-open, like every other [`Rect`] test here, so the
/// clamp lands a hair inside them rather than on a row that is outside.
pub fn clamp_to_grid(grid: Rect, x: f32, y: f32) -> (f32, f32) {
    if grid.is_empty() {
        return (grid.x, grid.y);
    }
    const INSIDE: f32 = 0.5;
    (
        x.clamp(grid.x, grid.right() - INSIDE),
        y.clamp(grid.y, grid.bottom() - INSIDE),
    )
}

/// The most one event may scroll the view, in pixels.
const EDGE_SCROLL_MAX_PX: f32 = 48.0;

/// How far the view should move because a drag is being held outside the grid,
/// as `(ticks, key rows)`.
///
/// The other half of [`clamp_to_grid`]: clamping alone means a drag towards
/// bar 1 stops at the left edge and stays there. Every roll worth using scrolls
/// instead, and the speed rises with how far out the pointer is so a nudge is a
/// nudge and a shove is a shove — bounded, because an unbounded one turns a
/// flick of the mouse into a thousand bars of travel.
///
/// Positive ticks are forwards in time; positive rows are *up* the keyboard,
/// matching [`RollView::top_key`].
pub fn edge_scroll(view: &RollView, grid: Rect, x: f32, y: f32) -> (Tick, i32) {
    if grid.is_empty() {
        return (0, 0);
    }
    let speed = |over: f32| (over / 3.0).clamp(1.0, EDGE_SCROLL_MAX_PX);
    let per_tick = view.pixels_per_tick.max(f32::EPSILON);
    let ticks = if x < grid.x {
        -((speed(grid.x - x) / per_tick) as Tick)
    } else if x >= grid.right() {
        (speed(x - grid.right()) / per_tick) as Tick
    } else {
        0
    };
    let per_row = view.key_height.max(f32::EPSILON);
    let rows = if y < grid.y {
        ((speed(grid.y - y) / per_row).ceil() as i32).max(1)
    } else if y >= grid.bottom() {
        -((speed(y - grid.bottom()) / per_row).ceil() as i32).max(1)
    } else {
        0
    };
    (ticks, rows)
}

// ----------------------------------------------------------------- zoom ---

/// Zooms time about `anchor_x`, so whatever is under the pointer stays under
/// it (§16.4).
///
/// Zooming about the left edge — which is what the first version did — throws
/// away where you were looking every time you turn the wheel, and is the
/// difference between navigating a song and hunting for your place in it.
///
/// The arithmetic is in `f64`: the anchor is recomputed from the live view on
/// every call, so the only error is `scroll_tick` being a whole tick, and that
/// is worth less than a pixel at any zoom this allows.
pub fn zoom_x(view: &mut RollView, grid: Rect, anchor_x: f32, factor: f32) {
    if view.pixels_per_tick <= 0.0 || !factor.is_finite() || factor <= 0.0 {
        return;
    }
    let offset = f64::from(anchor_x - grid.x);
    let anchor_tick = view.scroll_tick as f64 + offset / f64::from(view.pixels_per_tick);

    view.pixels_per_tick =
        (view.pixels_per_tick * factor).clamp(MIN_PIXELS_PER_TICK, MAX_PIXELS_PER_TICK);

    let scroll = anchor_tick - offset / f64::from(view.pixels_per_tick);
    view.scroll_tick = (scroll.round() as Tick).max(0);
}

/// Zooms pitch about `anchor_y`. See [`zoom_x`]; the only difference is that
/// `top_key` is a key rather than a fraction of one, so the anchor holds to
/// within a row.
pub fn zoom_y(view: &mut RollView, grid: Rect, anchor_y: f32, factor: f32) {
    if view.key_height <= 0.0 || !factor.is_finite() || factor <= 0.0 {
        return;
    }
    let offset = f64::from(anchor_y - grid.y);
    let anchor_key = f64::from(view.top_key) - offset / f64::from(view.key_height);

    // A **whole number** of pixels, always. A fractional row height draws some
    // rows a pixel taller than others once the rasteriser has rounded them,
    // which is the reported "inconsistant sizing" — see [`key_row`]. Rounded
    // away from where it started, so a zoom step at the small end still moves:
    // 5 × 1.2 is 6, and 6 / 1.2 rounds to 5 rather than back to 6.
    let wanted = view.key_height * factor;
    let stepped = if factor > 1.0 {
        wanted.ceil()
    } else {
        wanted.floor()
    };
    view.key_height = stepped.clamp(MIN_KEY_HEIGHT, MAX_KEY_HEIGHT);

    let top = anchor_key + offset / f64::from(view.key_height);
    view.top_key = (top.round() as i32).clamp(0, KEY_COUNT - 1) as u8;
}

// ----------------------------------------------------------------- snap ---

/// The standard divisions (§16.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapDivision {
    Bar,
    Beat,
    /// A sixteenth — the step sequencer's grid, hence the name.
    Step,
    /// One `n`th of a beat.
    Division(u8),
    Triplet,
    None,
}

impl SnapDivision {
    /// What the snap chip on the toolbar says.
    pub fn label(self) -> &'static str {
        match self {
            Self::Bar => "bar",
            Self::Beat => "beat",
            Self::Step => "1/16",
            Self::Division(2) => "1/8",
            Self::Division(8) => "1/32",
            Self::Division(_) => "1/n",
            Self::Triplet => "trip",
            Self::None => "none",
        }
    }

    /// The cycle `B` walks, in the order a person would want them.
    pub fn next(self) -> Self {
        match self {
            Self::Bar => Self::Beat,
            Self::Beat => Self::Division(2),
            Self::Division(2) => Self::Step,
            Self::Step => Self::Division(8),
            Self::Division(8) => Self::Triplet,
            Self::Triplet => Self::None,
            _ => Self::Bar,
        }
    }
}

/// How many ticks one snap line is from the next. **Zero means no snap.**
pub fn snap_unit(snap: SnapDivision, beats_per_bar: u32) -> Tick {
    match snap {
        SnapDivision::Bar => PPQN * Tick::from(beats_per_bar.max(1)),
        SnapDivision::Beat => PPQN,
        SnapDivision::Step => PPQN / 4,
        SnapDivision::Division(n) => PPQN / Tick::from(n.max(1)),
        SnapDivision::Triplet => PPQN / 3,
        SnapDivision::None => 0,
    }
}

/// The finest grid the roll *draws*, which is not the same thing as the snap.
///
/// Reported from using the window: *"it's kind of hard to tell the time right
/// now so ensure between bars or measures there's more dividers so I can see
/// the on and offbeats."* The roll drew its finest lines at the snap division,
/// so setting the snap to bars — or turning it off — removed every line
/// between the bars and left a bar-wide empty box to place notes in by eye.
///
/// The grid is the ruler you read the time off; the snap is where a note is
/// allowed to land. They are related and they are not the same, so this shows
/// the snap when the snap is fine enough to be useful as a ruler, and the
/// **eighths** otherwise. An eighth is the coarsest division that still shows
/// an offbeat, which is what the report is actually asking for.
///
/// The renderer drops any level whose lines would be a few pixels apart, so
/// this being fine at a very low zoom costs nothing.
pub fn subdivision_unit(snap: SnapDivision, beats_per_bar: u32) -> Tick {
    let eighth = PPQN / 2;
    let unit = snap_unit(snap, beats_per_bar);
    if unit > 0 && unit < eighth {
        unit
    } else {
        eighth
    }
}

/// Rounds to the **nearest** line, not the one before.
///
/// Nearest is what makes a grid feel like a magnet instead of a ratchet: a
/// note dropped a hair past a line should stay where it looks like it is.
pub fn snap_tick(tick: Tick, snap: SnapDivision, beats_per_bar: u32) -> Tick {
    let unit = snap_unit(snap, beats_per_bar);
    if unit <= 0 {
        return tick;
    }
    let tick = tick.max(0);
    ((tick + unit / 2) / unit * unit).max(0)
}

// ---------------------------------------------------------- hit-testing ---

/// Which bit of a note is under the pointer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotePart {
    Body,
    LeftEdge,
    RightEdge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RollHit {
    Note(NoteId, NotePart),
    /// Bare grid. The tick is **unsnapped** — snapping is the caller's
    /// decision, because Alt-drag bypasses it (§16.5) and a hit test that had
    /// already snapped could not offer that.
    Empty {
        tick: Tick,
        key: u8,
    },
    Outside,
}

pub fn hit_test(
    view: &RollView,
    grid: Rect,
    notes: &Arena<NoteId, Note>,
    x: f32,
    y: f32,
) -> RollHit {
    if !grid.contains(x, y) {
        return RollHit::Outside;
    }
    let key = y_to_key(view, grid, y);

    // Last match wins: later notes are drawn over earlier ones, so the one on
    // top is the one that was clicked.
    let mut found = None;
    for (id, note) in notes.iter() {
        if note.key != key {
            continue;
        }
        let left = tick_to_x(view, grid, note.start);
        let right = tick_to_x(view, grid, note.start + note.length);
        if x < left || x >= right {
            continue;
        }
        // A note narrower than two handles has none: dragging a note is more
        // common than resizing one, and a note you cannot grab is worse than
        // one you cannot resize without zooming in.
        let handle = HANDLE_PX.min((right - left) / 3.0);
        found = Some(RollHit::Note(
            id,
            if x >= right - handle {
                NotePart::RightEdge
            } else if x < left + handle {
                NotePart::LeftEdge
            } else {
                NotePart::Body
            },
        ));
    }

    found.unwrap_or(RollHit::Empty {
        tick: x_to_tick(view, grid, x),
        key,
    })
}

/// The note covering the column at `x`, whatever its pitch.
///
/// What the velocity lane hit-tests against: the lane has no pitch axis, so a
/// column is the whole of the question. The topmost note wins, the same rule
/// [`hit_test`] uses.
pub fn note_at_tick(
    view: &RollView,
    grid: Rect,
    notes: &Arena<NoteId, Note>,
    x: f32,
) -> Option<NoteId> {
    let tick = x_to_tick(view, grid, x);
    let mut found = None;
    for (id, note) in notes.iter() {
        if tick >= note.start && tick < note.start + note.length {
            found = Some(id);
        }
    }
    found
}

/// Which of a note's properties the lane under the grid is showing (§16.5's
/// note property lanes).
///
/// One lane rather than a stack of them, cycled by a chip on the toolbar: the
/// panel has room for one, and a stack of six would leave the grid a strip.
/// `Note` carries all of these already — the lane is the only thing that was
/// missing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaneProperty {
    Velocity,
    Pan,
    /// Cents off the note's own pitch, as `Note::fine_pitch` stores it.
    FinePitch,
    Release,
    ModX,
    ModY,
}

/// Every property the lane can show, in the order the menu lists them and the
/// chip cycles them.
///
/// One list rather than two, because two lists of the same six things is one
/// list to forget to update — and the menu existing at all is what fixed
/// *"I'm still not seeing panning options"*: the cycle could always reach pan,
/// and nothing on screen said so.
pub const LANE_PROPERTIES: [LaneProperty; 6] = [
    LaneProperty::Velocity,
    LaneProperty::Pan,
    LaneProperty::FinePitch,
    LaneProperty::Release,
    LaneProperty::ModX,
    LaneProperty::ModY,
];

impl LaneProperty {
    /// What the chip says, and the strip beside the lane.
    pub fn label(self) -> &'static str {
        match self {
            Self::Velocity => "vel",
            Self::Pan => "pan",
            Self::FinePitch => "tune",
            Self::Release => "rel",
            Self::ModX => "mod x",
            Self::ModY => "mod y",
        }
    }

    /// The cycle the chip walks, most-used first.
    pub fn next(self) -> Self {
        match self {
            Self::Velocity => Self::Pan,
            Self::Pan => Self::FinePitch,
            Self::FinePitch => Self::Release,
            Self::Release => Self::ModX,
            Self::ModX => Self::ModY,
            Self::ModY => Self::Velocity,
        }
    }

    /// The document's name for the same property.
    ///
    /// The lane is a view of a note, so what a note may *hold* is the model's
    /// to say — every range and clamp below comes from there rather than being
    /// written down twice and left to drift.
    pub fn property(self) -> NoteProperty {
        match self {
            Self::Velocity => NoteProperty::Velocity,
            Self::Pan => NoteProperty::Pan,
            Self::FinePitch => NoteProperty::FinePitch,
            Self::Release => NoteProperty::Release,
            Self::ModX => NoteProperty::ModX,
            Self::ModY => NoteProperty::ModY,
        }
    }

    /// The lowest and highest the property may be, in the units `Note` stores.
    pub fn range(self) -> (i32, i32) {
        self.property().range()
    }

    /// Whether the lane's zero is in the middle rather than at the bottom.
    ///
    /// A signed property reads as a deviation from centre; an unsigned one is a
    /// quantity and reads from the floor up. Asked of the range rather than
    /// listed here, so a property whose range changes cannot end up drawn one
    /// way and edited the other.
    pub fn bipolar(self) -> bool {
        self.range().0 < 0
    }

    /// What the property is worth on a lane with no room to read it.
    pub fn neutral(self) -> i32 {
        match self {
            Self::Velocity => 100,
            _ => 0,
        }
    }

    pub fn of(self, note: &Note) -> i32 {
        self.property().get(note)
    }

    /// Writes `value` onto `note`, clamped into [`range`](Self::range) — so a
    /// lane that has been dragged off its own end cannot produce a note the
    /// document would refuse.
    pub fn set(self, note: &mut Note, value: i32) {
        self.property().set(note, value);
    }
}

/// What a point in the lane means: the top of the range at the top, and for a
/// bipolar property zero exactly half-way down.
pub fn lane_value_of_y(property: LaneProperty, lane: Rect, y: f32) -> i32 {
    let (min, max) = property.range();
    if lane.height <= 0.0 {
        return property.neutral();
    }
    let value = if property.bipolar() {
        let half = lane.height / 2.0;
        let t = ((lane.y + half - y) / half).clamp(-1.0, 1.0);
        // Measured against whichever end of the range the pointer is on, so a
        // range that is not symmetric — pan is -127..=127 in a byte that
        // reaches -128 — still puts its zero in the middle.
        let span = if t >= 0.0 { max as f32 } else { -(min as f32) };
        (t * span).round() as i32
    } else {
        let t = ((y - lane.y) / lane.height).clamp(0.0, 1.0);
        (max as f32 - t * (max - min) as f32).round() as i32
    };
    value.clamp(min, max)
}

/// The other direction: where a value sits in the lane.
pub fn lane_y_of_value(property: LaneProperty, lane: Rect, value: i32) -> f32 {
    let (min, max) = property.range();
    let value = value.clamp(min, max);
    if lane.height <= 0.0 {
        return lane.y;
    }
    if property.bipolar() {
        let half = lane.height / 2.0;
        let span = if value >= 0 {
            max as f32
        } else {
            -(min as f32)
        };
        let t = if span > 0.0 { value as f32 / span } else { 0.0 };
        lane.y + half - t * half
    } else {
        let t = (max - value) as f32 / (max - min).max(1) as f32;
        lane.y + t.clamp(0.0, 1.0) * lane.height
    }
}

/// Where a bar in the lane grows *from* — the bottom for a quantity, the middle
/// for a deviation.
pub fn lane_baseline_y(property: LaneProperty, lane: Rect) -> f32 {
    if property.bipolar() {
        lane.y + lane.height / 2.0
    } else {
        lane.bottom()
    }
}

/// [`lane_value_of_y`] for velocity, which is the lane's default and the one
/// every other part of the roll already spoke in.
pub fn velocity_of_y(lane: Rect, y: f32) -> u8 {
    lane_value_of_y(LaneProperty::Velocity, lane, y) as u8
}

/// Where a note's velocity bar reaches in the lane.
pub fn velocity_to_y(lane: Rect, velocity: u8) -> f32 {
    lane_y_of_value(LaneProperty::Velocity, lane, i32::from(velocity))
}

// -------------------------------------------------------------- editing ---

/// What the roll wants done to the document.
///
/// Values, not commands: this crate cannot see `fontelle_model::Command` being
/// applied and should not want to. `fontelle-app` turns each of these into the
/// matching command and puts it through `History`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RollEdit {
    /// One note, drawn. Separate from [`RollEdit::Insert`] because the host
    /// hands the id back for it (see [`PianoRoll::note_added`]) and the drag
    /// that follows sizes it.
    ///
    /// A whole [`Note`] rather than four fields: what you draw is a copy of the
    /// last note you drew or clicked, all of its properties included, which is
    /// FL Studio's behaviour and the one thing people miss immediately when it
    /// is absent. See [`PianoRoll::template`].
    Add {
        note: Note,
    },
    /// Marks notes as slide notes, or unmarks them.
    SetSlide {
        ids: Vec<NoteId>,
        slide: bool,
    },
    /// Cuts notes in two, each at its own tick — the cut tool. See
    /// [`slice_cuts`] for where the ticks come from, and why they differ per
    /// note on a diagonal stroke.
    Slice {
        cuts: Vec<(NoteId, Tick)>,
    },
    /// A phrase, pasted or duplicated, already positioned.
    Insert(Vec<Note>),
    Remove(Vec<NoteId>),
    /// Deltas, and **relative to the previous step of the same drag** — that is
    /// what `MoveNotes` takes and what lets `merge_with` coalesce a drag into
    /// one history entry.
    Move {
        ids: Vec<NoteId>,
        tick_delta: Tick,
        key_delta: i16,
    },
    Resize {
        ids: Vec<NoteId>,
        tick_delta: Tick,
    },
    /// One value for every named note — the property lane's whole vocabulary.
    ///
    /// The property comes with the edit because the lane can be showing any of
    /// them (see [`LaneProperty`]), and a host that had to ask the roll which
    /// one would be reading state at a different moment from the one that
    /// produced the value.
    SetProperty {
        ids: Vec<NoteId>,
        property: LaneProperty,
        value: i32,
    },
}

/// A key the roll wants sounded, and how long the thing it came from is.
///
/// The length is the fix for *"clicking a note I'm still not hearing it
/// cleanly, just a flicker"*: the roll has always known how long the note under
/// the pointer was and was throwing it away at the door, so every audition —
/// a click on a whole note, a click on a thirty-second — got the same flat
/// minimum. What you click should sound for as long as it is.
///
/// **Ticks, not seconds.** The roll has no tempo and should not want one; the
/// window converts, because it is the half that can ask the document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Audition {
    pub key: u8,
    /// The note's length. Zero for an audition with no note behind it — the
    /// on-screen keyboard — which then gets the floor and nothing more.
    pub ticks: Tick,
}

/// What the drag *after* drawing a note does.
///
/// Reported from using the window: *"if I click to place a note, then my cursor
/// moves to drag it, it shouldn't change the note length it should move the
/// note."*
///
/// FL Studio's habit is [`Resize`](Self::Resize) — press on empty grid and the
/// same drag sets the length — and it is a real habit, but it has a sharp edge:
/// a press is never perfectly still, so *every* drawn note is a resize in
/// progress and the smallest wobble of the hand changes a length nobody meant
/// to change. [`Move`](Self::Move) is the default because that is what was
/// asked for and because the right edge resizes a note either way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DrawDrag {
    /// The drag carries the note you just drew, in both axes.
    #[default]
    Move,
    /// The drag sets the length of the note you just drew.
    Resize,
}

/// The tools §16.5 names. The gate needs the first four.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Draw,
    /// Draw, and keep drawing across every cell the pointer crosses.
    Paint,
    Delete,
    Select,
    Slice,
    Mute,
    Slip,
}

impl Tool {
    /// What a hover tip says about this tool (see [`crate::tooltip`]).
    pub fn tip(self) -> &'static str {
        match self {
            Self::Draw => "Draw notes; drag one out to length",
            Self::Paint => "Draw, and keep drawing as you sweep",
            Self::Delete => "Erase notes you press or sweep over",
            Self::Select => "Select notes; drag on empty grid to marquee",
            Self::Slice => "Cut notes in two where you drag across them",
            Self::Mute => "Mute notes you press",
            Self::Slip => "Slide a note's content without moving the note",
        }
    }

    /// The toolbar caption, and the key that picks it.
    pub fn label(self) -> &'static str {
        match self {
            Self::Draw => "Draw",
            Self::Paint => "Paint",
            Self::Delete => "Del",
            Self::Select => "Sel",
            Self::Slice => "Slice",
            Self::Mute => "Mute",
            Self::Slip => "Slip",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
}

// -------------------------------------------------------------- toolbar ---

/// Something on the roll's toolbar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RollControl {
    Tool(Tool),
    /// Cycles the snap division; the chip says which one is on.
    Snap,
    ZoomOutX,
    ZoomInX,
    ZoomOutY,
    ZoomInY,
    /// Shows and hides the property lane.
    Velocity,
    /// Opens the menu of properties the lane can show. The chip says which
    /// one is on **and carries a caret**, because a small box containing the
    /// word "vel" is a read-out and nothing about it says the other five are
    /// behind it — which is how a lane that could always draw pan came to be
    /// reported as not having it. See [`lane_caption`].
    Lane,
    /// Cycles the onion skin: which other instruments show through.
    Ghost,
    /// Makes the selection **slide notes**, or ordinary ones again — and sets
    /// what the next note you draw will be.
    ///
    /// A slide starts no voice: it bends whatever is already sounding to its
    /// pitch, over its own length (FL Studio's). See
    /// `fontelle_model::Note::slide`.
    Slide,
    /// Switches the strip down the side between the keyboard and a list of
    /// names. The chip says which one is on — see [`KeyStyle`].
    Keys,
}

impl RollControl {
    pub fn label(self) -> &'static str {
        match self {
            Self::Tool(tool) => tool.label(),
            Self::Snap => "snap",
            Self::ZoomOutX => "-",
            Self::ZoomInX => "+",
            Self::ZoomOutY => "\u{2193}",
            Self::ZoomInY => "\u{2191}",
            Self::Velocity => "lane",
            Self::Lane => "vel",
            Self::Ghost => "skin",
            Self::Slide => "slide",
            // The chip carries the view it will *give you*, like every other
            // read-out on this bar: what it says is what is on.
            Self::Keys => "keys",
        }
    }

    /// The glyph this control draws, or `None` when it draws its own value.
    ///
    /// The division is the whole rule for icons in this window: a **verb**
    /// gets a picture, because a picture of an action is quicker to find than
    /// its name; a **read-out** keeps its text, because the useful half of it
    /// is the value and no picture can say `1/16`.
    pub fn icon(self) -> Option<crate::icon::Icon> {
        use crate::icon::Icon;
        Some(match self {
            Self::Tool(Tool::Draw) => Icon::Pencil,
            Self::Tool(Tool::Paint) => Icon::Brush,
            Self::Tool(Tool::Select) => Icon::Marquee,
            Self::Tool(Tool::Delete) => Icon::Eraser,
            // The cut tool: the scissors, which is the one picture nobody
            // has to be taught.
            Self::Tool(Tool::Slice) => Icon::Cut,
            // Mute and Slip are on the tool list and not on the toolbar yet;
            // when they get there they get a glyph, and until then they keep
            // their caption rather than a wrong picture.
            Self::Tool(Tool::Mute | Tool::Slip) => return None,
            Self::ZoomOutX => Icon::Minus,
            Self::ZoomInX => Icon::Plus,
            Self::ZoomOutY => Icon::ArrowDown,
            Self::ZoomInY => Icon::ArrowUp,
            Self::Velocity => Icon::Sliders,
            // A slide is a *bend*: the glyph is the line that leaves one pitch
            // and arrives at another, which is what the note does.
            Self::Slide => Icon::Route,
            // Snap says its division, the lane chip says its property, and the
            // onion skin says which channel — all three are values.
            // Snap says its division, the lane chip says its property, the
            // onion skin says which channel, and the strip chip says which
            // view — all four are values.
            Self::Snap | Self::Lane | Self::Ghost | Self::Keys => return None,
        })
    }

    /// The keyboard shortcut worth writing in a tooltip, if there is one.
    /// What a hover tip says (see [`crate::tooltip`]).
    ///
    /// The shortcut is not repeated here — the renderer appends
    /// [`shortcut`](Self::shortcut) to what it draws, so the two cannot drift.
    pub fn tip(self) -> Option<&'static str> {
        Some(match self {
            Self::Tool(tool) => tool.tip(),
            Self::Snap => "What notes snap to \u{2014} click to cycle",
            Self::ZoomOutX => "Zoom out in time",
            Self::ZoomInX => "Zoom in in time",
            Self::ZoomOutY => "Shorter keys \u{2014} more of the keyboard",
            Self::ZoomInY => "Taller keys \u{2014} easier to aim at",
            Self::Velocity => "Show or hide the property lane",
            Self::Lane => "Which property the lane draws",
            Self::Ghost => "Show other instruments' notes behind these",
            Self::Slide => "Slide notes: bend what is sounding, start nothing",
            Self::Keys => "The strip down the side: a keyboard, or a list of names",
        })
    }

    pub fn shortcut(self) -> Option<&'static str> {
        match self {
            Self::Tool(Tool::Draw) => Some("P"),
            Self::Tool(Tool::Paint) => Some("B"),
            Self::Tool(Tool::Select) => Some("E"),
            Self::Tool(Tool::Delete) => Some("D"),
            Self::Tool(Tool::Slice) => Some("C"),
            Self::Snap => Some("S"),
            Self::Lane => Some("L"),
            Self::Ghost => Some("G"),
            Self::Slide => Some("A"),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolbarLayout {
    pub items: Vec<(RollControl, Rect)>,
}

/// The controls, left to right, in the order they are used: what the mouse
/// does, then what the grid is, then how close you are looking.
/// The controls, and how wide each is.
///
/// The ones that draw an **icon** ([`RollControl::icon`]) are square: a glyph
/// in a wide button is a glyph with a gap either side of it. The ones that are
/// a *read-out* keep their text and keep the room to say it — which division
/// is which is the same one the icons themselves are chosen by.
const TOOLBAR: [(RollControl, f32); 15] = [
    (RollControl::Tool(Tool::Draw), 26.0),
    (RollControl::Tool(Tool::Paint), 26.0),
    (RollControl::Tool(Tool::Select), 26.0),
    (RollControl::Tool(Tool::Delete), 26.0),
    (RollControl::Tool(Tool::Slice), 26.0),
    (RollControl::Snap, 52.0),
    (RollControl::ZoomOutX, 26.0),
    (RollControl::ZoomInX, 26.0),
    (RollControl::ZoomOutY, 26.0),
    (RollControl::ZoomInY, 26.0),
    (RollControl::Velocity, 26.0),
    (RollControl::Slide, 26.0),
    (RollControl::Lane, 62.0),
    (RollControl::Ghost, 40.0),
    (RollControl::Keys, 40.0),
];

/// What the lane chip says: the property, and a caret because it opens a menu.
///
/// Built rather than matched so the caret cannot end up on the chip and not in
/// the shaped-label set, which would draw the chip empty.
pub fn lane_caption(property: LaneProperty) -> String {
    format!("{} \u{25be}", property.label())
}

/// A little air at each end and between groups.
const TOOLBAR_PAD: f32 = 4.0;

pub fn toolbar_layout(toolbar: Rect, metrics: &Metrics) -> ToolbarLayout {
    let height = (toolbar.height - 2.0).max(0.0).min(metrics.row_height);
    let y = toolbar.y + (toolbar.height - height).max(0.0) / 2.0;

    let mut x = toolbar.x + TOOLBAR_PAD;
    let mut items = Vec::with_capacity(TOOLBAR.len());
    for (control, width) in TOOLBAR {
        // Clipped rather than dropped: a control that has run off the end of a
        // narrow panel is an empty rectangle, which hit-tests as absent and
        // draws as nothing, and the list stays the same length either way.
        items.push((
            control,
            Rect::new(x, y, width, height).intersection(&toolbar),
        ));
        x += width + TOOLBAR_PAD;
    }
    ToolbarLayout { items }
}

pub fn toolbar_hit(bar: &ToolbarLayout, x: f32, y: f32) -> Option<RollControl> {
    bar.items
        .iter()
        .find(|(_, rect)| rect.contains(x, y))
        .map(|(control, _)| *control)
}

/// The lane chip's drop-down, laid out.
#[derive(Debug, Clone, PartialEq)]
pub struct LaneMenu {
    /// The whole panel, for the background and for "did the click miss".
    pub frame: Rect,
    pub items: Vec<(LaneProperty, Rect)>,
}

/// A little air around the rows.
const MENU_PAD: f32 = 3.0;

/// Narrow enough to hang off a chip, wide enough for `mod x` and a tick.
const MENU_WIDTH: f32 = 84.0;

/// Drops the property menu from `chip`, kept inside `bounds`.
///
/// Pure, like every other piece of geometry here, which is what lets "it does
/// not hang out of the bottom of a short panel" be a test rather than
/// something to notice by looking at it once.
pub fn lane_menu_layout(chip: Rect, bounds: Rect, metrics: &Metrics) -> LaneMenu {
    let row = metrics.row_height.max(1.0);
    let width = MENU_WIDTH.max(chip.width).min(bounds.width);
    let height = row * LANE_PROPERTIES.len() as f32 + MENU_PAD * 2.0;
    if bounds.is_empty() || width <= 0.0 || height > bounds.height {
        // Nowhere to put it. An empty menu hit-tests as absent and draws as
        // nothing, which is better than a sliver listing two of six things.
        return LaneMenu {
            frame: Rect::ZERO,
            items: Vec::new(),
        };
    }

    // Below the chip by preference — a menu over the control it belongs to
    // hides what it is doing — and above it when there is no room below.
    let below = chip.bottom();
    let y = if below + height <= bounds.bottom() {
        below
    } else {
        (chip.y - height).max(bounds.y)
    };
    let x = chip
        .x
        .clamp(bounds.x, (bounds.right() - width).max(bounds.x));
    let frame = Rect::new(x, y, width, height).intersection(&bounds);

    let mut items = Vec::with_capacity(LANE_PROPERTIES.len());
    for (index, property) in LANE_PROPERTIES.iter().enumerate() {
        items.push((
            *property,
            Rect::new(
                frame.x + MENU_PAD,
                frame.y + MENU_PAD + row * index as f32,
                (frame.width - MENU_PAD * 2.0).max(0.0),
                row,
            ),
        ));
    }
    LaneMenu { frame, items }
}

/// Which row is under the pointer, if any.
pub fn lane_menu_hit(menu: &LaneMenu, x: f32, y: f32) -> Option<LaneProperty> {
    menu.items
        .iter()
        .find(|(_, rect)| rect.contains(x, y))
        .map(|(property, _)| *property)
}

// ------------------------------------------------------------- gestures ---

/// What the pointer is in the middle of doing.
#[derive(Debug, Clone, PartialEq)]
enum Gesture {
    None,
    /// A note has been asked for and its id is not known yet. The next
    /// [`PianoRoll::note_added`] turns this into a resize of it.
    PendingAdd {
        /// Where the note was put, which is what a *move* is measured from.
        start: Tick,
        /// Where its right edge is, which is what a *resize* measures from.
        end: Tick,
        key: u8,
        /// How long it was asked for, which is the resize's floor.
        length: Tick,
    },
    /// Dragging the selection around. `applied` is how far the drag has been
    /// committed so far, so each step can emit only the difference.
    Moving {
        applied_tick: Tick,
        applied_key: i16,
        /// The limits, **measured once when the gesture started**.
        ///
        /// This is the whole of the flicker fix. These used to be recomputed
        /// from the live document on every step — and the drag is what is
        /// changing the live document, so the moment a note reached bar 1 the
        /// clamp became "cannot move at all", `wanted` snapped to zero against
        /// an `applied` of minus three bars, and the note was asked to jump
        /// three bars *forward*. Then back. At mouse-move rate.
        limits: MoveLimits,
    },
    /// Dragging a note's right edge.
    Resizing {
        applied_tick: Tick,
        /// The shortest note in the selection **when the gesture started**.
        /// Same reason as [`Gesture::Moving`]'s limits: measured live, it
        /// shrinks as the drag shortens the note, and the clamp chases it.
        shortest: Tick,
    },
    /// Dragging a selection box. Corners in pixels, because that is what the
    /// box is drawn in and what makes direction irrelevant.
    Marquee {
        from: (f32, f32),
        to: (f32, f32),
    },
    /// Painting notes across cells as the pointer crosses them.
    Painting {
        last: (Tick, u8),
    },
    /// Dragging in the property lane.
    Lane {
        ids: Vec<NoteId>,
    },
    /// Dragging the seam between the grid and the lane. The lane's height is
    /// the window's to apply — the roll only says a drag is in progress, so a
    /// pointer that wanders over a note mid-drag does not start editing it.
    LaneResize,
    /// Rubbing notes out: the right button held down, or the Delete tool.
    ///
    /// It carries no state at all, and that is the point. Every other gesture
    /// here has to remember where it started, because it emits deltas from
    /// that origin; an erase asks the *current* document what is under the
    /// pointer and removes it, so a note already gone is simply not found
    /// again. That is what makes "held still, asks for nothing" true by
    /// construction rather than by a `last` field somebody has to maintain.
    Erasing,
    /// Drawing the cut tool's line.
    ///
    /// Kept in **screen** points rather than in ticks and keys: it is a
    /// straight line across the canvas, and which notes it crosses is a
    /// question about pixels — see [`slice_cuts`]. It emits its edit on
    /// **release**, because a cut half-drawn is not a cut.
    Slicing {
        from: (f32, f32),
        to: (f32, f32),
    },
}

/// How far a move may go before it would take a note somewhere the document
/// would refuse, measured when the gesture started.
///
/// Deltas, not absolutes: a move is expressed as a delta from where the drag
/// began, so its limits have to be in the same units or the two cannot be
/// compared.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct MoveLimits {
    /// The furthest back in time the selection may go — the negative of the
    /// earliest selected note's start, so nothing lands before the clip.
    min_tick: Tick,
    /// And the furthest down and up the keyboard, from the lowest and highest
    /// selected notes. Clamping rather than letting the document refuse
    /// matters for a *chord*: `MoveNotes` is all-or-nothing, so one note that
    /// would pass 127 stops the whole shape from moving at all.
    min_key: i16,
    max_key: i16,
}

impl MoveLimits {
    /// The limits for `selection` as the document has it now.
    fn of(selection: &[NoteId], notes: &Arena<NoteId, Note>) -> Self {
        let mut earliest = Tick::MAX;
        let mut lowest = i16::MAX;
        let mut highest = i16::MIN;
        for note in selection.iter().filter_map(|id| notes.get(*id)) {
            earliest = earliest.min(note.start);
            lowest = lowest.min(i16::from(note.key));
            highest = highest.max(i16::from(note.key));
        }
        if earliest == Tick::MAX {
            return Self::default();
        }
        Self {
            min_tick: -earliest,
            min_key: -lowest,
            max_key: (KEY_COUNT as i16 - 1) - highest,
        }
    }
}

/// The roll's own state: where it is looking, what is selected, what the mouse
/// is doing.
pub struct PianoRoll {
    pub view: RollView,
    pub tool: Tool,
    /// How tall the property lane is, in pixels. **Zero hides it**, and the
    /// grid takes the room back. Dragged by the seam above it — see
    /// [`lane_height_at`].
    pub lane_height: f32,
    /// Which property the lane is showing and editing.
    pub lane_property: LaneProperty,
    /// What the drag after drawing a note does.
    pub draw_drag: DrawDrag,
    /// Which other instruments show through behind the notes.
    pub ghosts: crate::document::GhostFilter,
    /// The note every newly drawn note is a copy of — see
    /// [`template`](Self::template).
    template: Note,
    selection: Vec<NoteId>,
    gesture: Gesture,
    /// Where the pointer was when the gesture started, in document units.
    origin: (Tick, u8),
    modifiers: Modifiers,
    /// A key the roll wants sounded, waiting to be collected.
    ///
    /// Handed over rather than played, because making a sound is the
    /// *document's* business — it owns the live path to the audio thread — and
    /// this canvas may not touch it (INVARIANT 2). Same shape as
    /// [`Timeline::take_open`](crate::canvas::Timeline::take_open).
    audition: Option<Audition>,
    /// The phrase Ctrl+C put there, normalised so its earliest note starts at
    /// tick zero — which is what lets a paste land anywhere.
    clipboard: Vec<Note>,
}

impl PianoRoll {
    pub fn new(view: RollView) -> Self {
        Self {
            view,
            tool: Tool::Draw,
            lane_height: DEFAULT_LANE_HEIGHT,
            lane_property: LaneProperty::Velocity,
            draw_drag: DrawDrag::default(),
            ghosts: crate::document::GhostFilter::Off,
            template: BLANK_TEMPLATE,
            selection: Vec::new(),
            gesture: Gesture::None,
            origin: (0, 0),
            modifiers: Modifiers::default(),
            audition: None,
            clipboard: Vec::new(),
        }
    }

    pub fn selection(&self) -> &[NoteId] {
        &self.selection
    }

    /// A key the roll wants sounded, once.
    ///
    /// **You hear what you touch**: drawing a note, clicking one, and dragging
    /// one to a new pitch all ask for the pitch they landed on. Deleting one
    /// does not, and neither does sliding a note along in time — that would
    /// machine-gun the same pitch for the length of the drag.
    pub fn take_audition(&mut self) -> Option<Audition> {
        self.audition.take()
    }

    /// The note the next drawn note is a copy of.
    ///
    /// **This is FL Studio's "the new note is the last note".** Draw a long
    /// quiet note and the next one you draw is long and quiet; click a short
    /// loud one and the next one is short and loud. It carries every property
    /// `Note` has — pan and the two free modulation values included — because
    /// the alternative is setting them again on every note of a phrase.
    ///
    /// `start` and `key` are ignored: those come from where you pressed.
    pub fn template(&self) -> Note {
        self.template
    }

    /// Replaces the template outright. For a host restoring a session, and for
    /// the tests.
    pub fn set_template(&mut self, note: Note) {
        self.template = note;
    }

    /// Takes `id`'s properties as the template, if it is still there.
    ///
    /// Called wherever a note becomes "the note you last touched": clicking
    /// one, and letting go of a drag that drew or resized one.
    fn adopt(&mut self, notes: &Arena<NoteId, Note>, id: NoteId) {
        if let Some(note) = notes.get(id) {
            self.template = *note;
        }
    }

    /// [`adopt`](Self::adopt) for whatever the selection starts with.
    fn adopt_selected(&mut self, notes: &Arena<NoteId, Note>) {
        if let Some(id) = self.selection.first().copied() {
            self.adopt(notes, id);
        }
    }

    /// The note a press on empty grid asks for.
    fn drawn_note(&self, start: Tick, key: u8, length: Tick) -> Note {
        Note {
            start,
            length: length.max(1),
            key,
            ..self.template
        }
    }

    pub fn set_modifiers(&mut self, modifiers: Modifiers) {
        self.modifiers = modifiers;
    }

    pub fn modifiers(&self) -> Modifiers {
        self.modifiers
    }

    /// The selection box being dragged, for drawing. `None` when there is not
    /// one.
    pub fn marquee(&self) -> Option<Rect> {
        match self.gesture {
            Gesture::Marquee { from, to } => Some(box_between(from, to)),
            _ => None,
        }
    }

    pub fn select_all(&mut self, notes: &Arena<NoteId, Note>) {
        self.selection = notes.keys().collect();
    }

    /// Whether the **next note drawn** will be a slide.
    ///
    /// The template's own flag, so a slide you draw is a slide the moment it
    /// exists rather than an ordinary note you then have to convert — which is
    /// the same rule every other property the template carries follows.
    pub fn drawing_slides(&self) -> bool {
        self.template.slide
    }

    /// Toggles the selection between slide and ordinary, and sets what the
    /// next drawn note will be.
    ///
    /// Off when all of the selection already slides, on otherwise — the same
    /// rule the arrangement's Mute button follows, so a mixed selection
    /// settles rather than flipping each note against the others.
    ///
    /// With **nothing selected** it only changes the template, which is what
    /// makes "turn slide on, then draw three of them" work.
    pub fn toggle_slide(&mut self, notes: &Arena<NoteId, Note>) -> Vec<RollEdit> {
        let selected: Vec<NoteId> = self
            .selection
            .iter()
            .copied()
            .filter(|id| notes.contains_key(*id))
            .collect();
        if selected.is_empty() {
            self.template.slide = !self.template.slide;
            return Vec::new();
        }
        let slide = !selected
            .iter()
            .all(|id| notes.get(*id).is_some_and(|note| note.slide));
        self.template.slide = slide;
        vec![RollEdit::SetSlide {
            ids: selected,
            slide,
        }]
    }

    pub fn clear_selection(&mut self) {
        self.selection.clear();
    }

    pub fn select(&mut self, ids: Vec<NoteId>) {
        self.selection = ids;
    }

    /// The snap in force *right now* — `None` while Alt is held (§16.5).
    fn live_snap(&self) -> SnapDivision {
        if self.modifiers.alt {
            SnapDivision::None
        } else {
            self.view.snap
        }
    }

    /// `Ctrl+A`'s counterpart: what `Delete` should do.
    /// One step of an erase: whatever `hit` found, gone.
    ///
    /// Shared by the press and the drag so that "what the right button
    /// deletes" is written once. Empty when there is nothing under the
    /// pointer, which is also what makes a stationary erase over a note it
    /// has already removed ask for nothing.
    fn erase_at(&mut self, hit: RollHit) -> Vec<RollEdit> {
        match hit {
            RollHit::Note(id, _) => {
                self.selection.retain(|s| *s != id);
                vec![RollEdit::Remove(vec![id])]
            }
            _ => Vec::new(),
        }
    }

    pub fn delete_selection(&mut self) -> Vec<RollEdit> {
        if self.selection.is_empty() {
            // Not an empty removal — that would still be a history entry, and
            // an undo that puts nothing back is worse than a key that did
            // nothing.
            return Vec::new();
        }
        vec![RollEdit::Remove(std::mem::take(&mut self.selection))]
    }

    /// Tells the roll the id of the note its last [`press`](Self::press) asked
    /// for, turning the press into a draw-and-size gesture.
    ///
    /// Called by the host right after it applies a [`RollEdit::Add`]. A press
    /// that produced no note leaves no pending add, so a stale one cannot
    /// capture the next note drawn somewhere else.
    pub fn note_added(&mut self, id: NoteId) {
        let Gesture::PendingAdd {
            start,
            end,
            key,
            length,
        } = self.gesture
        else {
            return;
        };
        self.selection = vec![id];
        // Everything either gesture needs comes out of the pending add rather
        // than out of the document. The host has applied the edit by now, so
        // it *could* be looked up — but reading a gesture's limits back out of
        // the thing the gesture is changing is the "measure it live" mistake
        // that made the drag oscillate, and it is no less a mistake for being
        // only the first read.
        match self.draw_drag {
            DrawDrag::Move => {
                self.gesture = Gesture::Moving {
                    applied_tick: 0,
                    applied_key: 0,
                    limits: MoveLimits {
                        min_tick: -start,
                        min_key: -i16::from(key),
                        max_key: (KEY_COUNT as i16 - 1) - i16::from(key),
                    },
                };
            }
            DrawDrag::Resize => {
                self.origin = (end, key);
                self.gesture = Gesture::Resizing {
                    applied_tick: 0,
                    shortest: length,
                };
            }
        }
    }

    /// The ids of notes the host created for a [`RollEdit::Insert`], so a
    /// pasted phrase arrives selected and can be dragged straight away.
    pub fn notes_inserted(&mut self, ids: Vec<NoteId>) {
        if !ids.is_empty() {
            self.selection = ids;
        }
    }

    pub fn press(
        &mut self,
        button: MouseButton,
        x: f32,
        y: f32,
        grid: Rect,
        notes: &Arena<NoteId, Note>,
        beats_per_bar: u32,
    ) -> Vec<RollEdit> {
        let hit = hit_test(&self.view, grid, notes, x, y);

        // Right-click deletes in any tool (§16.5), and **keeps deleting for as
        // long as the button is down** — see [`Gesture::Erasing`]. Starting
        // the gesture on empty grid matters as much as starting it on a note:
        // a sweep across a sparse bar begins wherever the pointer happened to
        // be, and leaving `self.gesture` at whatever the *previous* gesture
        // left behind is how a right press used to carry on marqueeing.
        if button == MouseButton::Right || self.tool == Tool::Delete {
            self.gesture = Gesture::Erasing;
            return self.erase_at(hit);
        }

        // The cut tool draws a line, and it draws it over **anything** — notes
        // included, which is the point. It is the one tool whose press does
        // not care what is under it.
        if self.tool == Tool::Slice {
            self.gesture = Gesture::Slicing {
                from: (x, y),
                to: (x, y),
            };
            return Vec::new();
        }

        match hit {
            RollHit::Note(id, part) => {
                // Pressing a note that is already part of the selection keeps
                // it — otherwise dragging a chord would collapse it to
                // whichever note you happened to grab.
                if !self.selection.contains(&id) {
                    self.selection = vec![id];
                }
                // The note you just clicked is the one the next one copies —
                // and the one you hear, so clicking a note tells you what it
                // is without having to play the song to find out.
                self.adopt(notes, id);
                self.audition = notes.get(id).map(|note| Audition {
                    key: note.key,
                    ticks: note.length,
                });
                self.origin = (
                    x_to_tick(&self.view, grid, x),
                    y_to_key(&self.view, grid, y),
                );
                self.gesture = match part {
                    NotePart::RightEdge => Gesture::Resizing {
                        applied_tick: 0,
                        shortest: self.shortest_selected(notes),
                    },
                    _ => Gesture::Moving {
                        applied_tick: 0,
                        applied_key: 0,
                        limits: MoveLimits::of(&self.selection, notes),
                    },
                };
                Vec::new()
            }
            RollHit::Empty { tick, key } => {
                self.selection.clear();
                // Ctrl on empty grid is a marquee whatever the tool, which is
                // the FL habit; the Select tool does it without one.
                if self.tool == Tool::Select || self.modifiers.ctrl {
                    self.gesture = Gesture::Marquee {
                        from: (x, y),
                        to: (x, y),
                    };
                    return Vec::new();
                }
                if !matches!(self.tool, Tool::Draw | Tool::Paint) {
                    self.gesture = Gesture::None;
                    return Vec::new();
                }
                let snap = self.live_snap();
                let start = snap_tick(tick, snap, beats_per_bar);
                let note = self.drawn_note(start, key, self.template.length);
                // Where the pointer *pressed*, not where the note landed: a
                // move is a delta from the press, and measuring it from the
                // snapped start would jump the note by up to half a step the
                // moment the drag began.
                self.origin = (tick, key);
                self.gesture = if self.tool == Tool::Paint {
                    Gesture::Painting { last: (start, key) }
                } else {
                    Gesture::PendingAdd {
                        start,
                        end: start + note.length,
                        key,
                        length: note.length,
                    }
                };
                self.audition = Some(Audition {
                    key,
                    ticks: note.length,
                });
                vec![RollEdit::Add { note }]
            }
            RollHit::Outside => {
                self.gesture = Gesture::None;
                Vec::new()
            }
        }
    }

    pub fn drag(
        &mut self,
        x: f32,
        y: f32,
        grid: Rect,
        notes: &Arena<NoteId, Note>,
        beats_per_bar: u32,
    ) -> Vec<RollEdit> {
        // **Before anything else.** A drag that has wandered off the grid —
        // onto the keyboard reaching for bar 1, onto the ruler reaching for a
        // higher note — means the nearest cell inside it, not a negative tick
        // flattened to zero and a row count that runs the key up to 127. See
        // [`clamp_to_grid`], and [`edge_scroll`] for the other half.
        // The **unclamped** pointer, kept for the erase alone. Clamping is
        // right for a move — a drag that strays onto the keyboard still means
        // the nearest cell — and wrong for a rub-out, where it would delete
        // whatever happens to sit at the edge you slid past on your way off
        // the canvas. An erase off the grid must find nothing.
        let (raw_x, raw_y) = (x, y);
        let (x, y) = clamp_to_grid(grid, x, y);
        let tick = x_to_tick(&self.view, grid, x);
        let key = y_to_key(&self.view, grid, y);

        match self.gesture.clone() {
            Gesture::None
            | Gesture::PendingAdd { .. }
            | Gesture::Lane { .. }
            | Gesture::LaneResize => Vec::new(),

            Gesture::Erasing => self.erase_at(hit_test(&self.view, grid, notes, raw_x, raw_y)),

            Gesture::Marquee { from, .. } => {
                self.gesture = Gesture::Marquee { from, to: (x, y) };
                Vec::new()
            }

            // The **unclamped** pointer, like the erase: a stroke that runs
            // off the top of the grid on its way past a chord should still
            // have crossed it, and clamping would bend the line.
            Gesture::Slicing { from, .. } => {
                self.gesture = Gesture::Slicing {
                    from,
                    to: (raw_x, raw_y),
                };
                Vec::new()
            }

            Gesture::Painting { last } => {
                let snap = self.live_snap();
                let start = snap_tick(tick, snap, beats_per_bar);
                if (start, key) == last {
                    return Vec::new();
                }
                // One note per cell crossed, and never on top of one that is
                // already there — painting over an existing note in FL leaves
                // one note, not a stack of them.
                if note_covers(notes, start, key) {
                    self.gesture = Gesture::Painting { last: (start, key) };
                    return Vec::new();
                }
                self.gesture = Gesture::Painting { last: (start, key) };
                self.audition = Some(Audition {
                    key,
                    ticks: self.template.length,
                });
                vec![RollEdit::Add {
                    note: self.drawn_note(start, key, self.template.length),
                }]
            }

            Gesture::Moving {
                applied_tick,
                applied_key,
                limits,
            } => {
                if self.selection.is_empty() {
                    return Vec::new();
                }
                // Snap the *destination*, not the delta: a note that started
                // off the grid should land on it, which is what dragging a
                // sloppily-placed note onto the beat is for.
                let mut wanted_tick = self.snapped_delta(tick, beats_per_bar, limits);
                let mut wanted_key = (i16::from(key) - i16::from(self.origin.1))
                    .clamp(limits.min_key, limits.max_key);

                // §16.5: Shift constrains to whichever axis the drag is mostly
                // along, measured in pixels so it is the axis it *looks* like.
                if self.modifiers.shift {
                    let dx = (wanted_tick as f32 * self.view.pixels_per_tick).abs();
                    let dy = (f32::from(wanted_key) * self.view.key_height).abs();
                    if dx >= dy {
                        wanted_key = 0;
                    } else {
                        wanted_tick = 0;
                    }
                }

                let d_tick = wanted_tick - applied_tick;
                let d_key = wanted_key - applied_key;
                if d_tick == 0 && d_key == 0 {
                    // No zero-delta edits: a sub-step mouse move is not a
                    // history entry, and it is not a timeline recompile either.
                    return Vec::new();
                }
                if d_key != 0 {
                    // The pitch it landed on, so a transpose can be done by ear
                    // rather than by counting rows. Only on a pitch change: a
                    // note slid along in time would otherwise machine-gun.
                    self.audition =
                        self.selection
                            .first()
                            .and_then(|id| notes.get(*id))
                            .map(|note| Audition {
                                key: (i16::from(note.key) + d_key).clamp(0, 127) as u8,
                                ticks: note.length,
                            });
                }
                self.gesture = Gesture::Moving {
                    applied_tick: wanted_tick,
                    applied_key: wanted_key,
                    limits,
                };
                vec![RollEdit::Move {
                    ids: self.selection.clone(),
                    tick_delta: d_tick,
                    key_delta: d_key,
                }]
            }

            Gesture::Resizing {
                applied_tick,
                shortest,
            } => {
                if self.selection.is_empty() {
                    return Vec::new();
                }
                let unit = snap_unit(self.live_snap(), beats_per_bar);
                let raw = tick - self.origin.0;
                let wanted = if unit > 0 {
                    (raw as f64 / unit as f64).round() as Tick * unit
                } else {
                    raw
                };
                // Never past nothing: the shortest a note may become is one
                // snap unit, or one tick with snap off. Measured against the
                // length the note had **when the drag started** — see
                // `Gesture::Resizing`.
                let wanted = wanted.max(-(shortest - unit.max(1)).max(0));
                let delta = wanted - applied_tick;
                if delta == 0 {
                    return Vec::new();
                }
                self.gesture = Gesture::Resizing {
                    applied_tick: wanted,
                    shortest,
                };
                vec![RollEdit::Resize {
                    ids: self.selection.clone(),
                    tick_delta: delta,
                }]
            }
        }
    }

    /// Ends the gesture. The caller breaks the history gesture on the same
    /// event, which is what makes one drag one undo entry.
    pub fn release(&mut self) {
        self.gesture = Gesture::None;
    }

    /// [`release`](Self::release), knowing where the button came up — which a
    /// marquee needs, because that is the moment it decides what it caught.
    /// Ends the gesture, and hands back whatever it produced on the way out.
    ///
    /// Most gestures emit as they go and return nothing here. The **cut tool**
    /// is the exception and the reason this returns anything at all: a line
    /// half-drawn is not a cut, so the whole edit lands on release.
    pub fn release_over(
        &mut self,
        x: f32,
        y: f32,
        grid: Rect,
        notes: &Arena<NoteId, Note>,
    ) -> Vec<RollEdit> {
        let mut edits = Vec::new();
        match self.gesture {
            Gesture::Marquee { from, .. } => {
                let box_ = box_between(from, (x, y));
                self.selection = notes_in(&self.view, grid, notes, box_);
            }
            Gesture::Slicing { from, to } => {
                let cuts = slice_cuts(&self.view, grid, notes, from, to);
                if !cuts.is_empty() {
                    edits.push(RollEdit::Slice { cuts });
                }
            }
            _ => {}
        }
        self.gesture = Gesture::None;
        // Whatever the gesture just left selected is the note the next one is
        // a copy of — which is what makes drawing a note to length change the
        // length of the *next* note too, the way FL Studio does.
        self.adopt_selected(notes);
        edits
    }

    /// The cut tool's line while it is being drawn, in screen points.
    ///
    /// The renderer draws it: a tool whose gesture leaves no mark is one you
    /// have to aim blind, which on a grid where rows are fourteen pixels apart
    /// is a guess.
    pub fn slice_stroke(&self) -> Option<((f32, f32), (f32, f32))> {
        match self.gesture {
            Gesture::Slicing { from, to } => Some((from, to)),
            _ => None,
        }
    }

    /// Whether a drag in progress is the lane's seam being moved.
    pub fn is_resizing_lane(&self) -> bool {
        matches!(self.gesture, Gesture::LaneResize)
    }

    /// Starts a drag of the seam above the lane.
    pub fn press_lane_grip(&mut self) {
        self.gesture = Gesture::LaneResize;
    }

    // ------------------------------------------------------ property lane ---

    /// Pressing in the property lane. Sets the note under the column, or the
    /// whole selection when the column is part of it.
    pub fn press_lane(
        &mut self,
        x: f32,
        y: f32,
        lane: Rect,
        grid: Rect,
        notes: &Arena<NoteId, Note>,
    ) -> Vec<RollEdit> {
        let Some(id) = note_at_tick(&self.view, grid, notes, x) else {
            self.gesture = Gesture::None;
            return Vec::new();
        };
        // Grabbing one of a selection sets the lot — that is what makes
        // flattening a chord one gesture rather than four.
        let ids = if self.selection.contains(&id) {
            self.selection.clone()
        } else {
            self.selection = vec![id];
            vec![id]
        };
        self.gesture = Gesture::Lane { ids: ids.clone() };
        self.lane_edit(ids, lane, y)
    }

    pub fn drag_lane(
        &mut self,
        _x: f32,
        y: f32,
        lane: Rect,
        _grid: Rect,
        _notes: &Arena<NoteId, Note>,
    ) -> Vec<RollEdit> {
        // The notes are the ones the press caught, not whatever is under the
        // pointer now: dragging sideways across the lane while setting a value
        // would otherwise rewrite the whole bar.
        let Gesture::Lane { ids } = &self.gesture else {
            return Vec::new();
        };
        let ids = ids.clone();
        self.lane_edit(ids, lane, y)
    }

    /// One value, for whichever property the lane is showing.
    ///
    /// The template takes the value too, so a phrase written after flattening
    /// a chord to pan-left comes out panned left rather than back at centre.
    fn lane_edit(&mut self, ids: Vec<NoteId>, lane: Rect, y: f32) -> Vec<RollEdit> {
        let property = self.lane_property;
        let value = lane_value_of_y(property, lane, y);
        property.set(&mut self.template, value);
        vec![RollEdit::SetProperty {
            ids,
            property,
            value,
        }]
    }

    /// Whether a drag in progress belongs to the property lane.
    pub fn is_editing_lane(&self) -> bool {
        matches!(self.gesture, Gesture::Lane { .. })
    }

    // ---------------------------------------------------- from the keyboard ---

    /// What one arrow key is worth in ticks: the snap division in force.
    ///
    /// Never zero. With free positioning on there is still a sensible smallest
    /// nudge, and an arrow key that does nothing because the snap chip says
    /// "none" is a keyboard that stops working when you turn a *drawing* aid
    /// off.
    pub fn step(&self, beats_per_bar: u32) -> Tick {
        match snap_unit(self.view.snap, beats_per_bar) {
            0 => FINE_STEP,
            unit => unit,
        }
    }

    /// Moves the selection, the way a drag does but by a known amount.
    ///
    /// Clamped exactly as a drag is, and for the same reason: `MoveNotes` is
    /// all-or-nothing, so a chord with one note near the top of the keyboard
    /// would refuse to transpose at all rather than transposing as far as it
    /// can. Empty when there is nothing to move or nowhere to move it — an
    /// arrow key at the end of its travel is not an undo entry.
    pub fn nudge(
        &mut self,
        notes: &Arena<NoteId, Note>,
        tick_delta: Tick,
        key_delta: i16,
    ) -> Vec<RollEdit> {
        if self.selection.is_empty() {
            return Vec::new();
        }
        let limits = MoveLimits::of(&self.selection, notes);
        let tick_delta = tick_delta.max(limits.min_tick);
        let key_delta = key_delta.clamp(limits.min_key, limits.max_key);
        if tick_delta == 0 && key_delta == 0 {
            return Vec::new();
        }
        if key_delta != 0 {
            // You hear what you touch, whether you touched it with the mouse
            // or not. Only on a pitch change, for the reason a drag along the
            // time axis is silent.
            self.audition = self
                .selection
                .first()
                .and_then(|id| notes.get(*id))
                .map(|note| Audition {
                    key: (i16::from(note.key) + key_delta).clamp(0, 127) as u8,
                    ticks: note.length,
                });
        }
        vec![RollEdit::Move {
            ids: self.selection.clone(),
            tick_delta,
            key_delta,
        }]
    }

    /// Lengthens or shortens the selection — Shift and an arrow key.
    ///
    /// Floored at the shortest selected note, so shrinking a chord stops when
    /// its shortest note reaches one tick rather than when the *document*
    /// refuses the whole edit.
    pub fn resize_selection(
        &mut self,
        notes: &Arena<NoteId, Note>,
        tick_delta: Tick,
    ) -> Vec<RollEdit> {
        if self.selection.is_empty() {
            return Vec::new();
        }
        let tick_delta = tick_delta.max(-(self.shortest_selected(notes) - 1).max(0));
        if tick_delta == 0 {
            return Vec::new();
        }
        vec![RollEdit::Resize {
            ids: self.selection.clone(),
            tick_delta,
        }]
    }

    /// Bumps whichever property the lane is showing, on every selected note.
    ///
    /// **One edit per note, each carrying that note's own new value.** A single
    /// `SetProperty` over the selection would flatten a phrase's dynamics to
    /// one number, which is what the lane's *drag* means and the opposite of
    /// what a bump means.
    pub fn nudge_property(&mut self, notes: &Arena<NoteId, Note>, by: i32) -> Vec<RollEdit> {
        let property = self.lane_property;
        let (min, max) = property.range();
        let mut edits = Vec::new();
        for id in &self.selection {
            let Some(note) = notes.get(*id) else { continue };
            let was = property.of(note);
            let value = (was + by).clamp(min, max);
            if value == was {
                continue;
            }
            edits.push(RollEdit::SetProperty {
                ids: vec![*id],
                property,
                value,
            });
        }
        // The template follows the first one, so the next note drawn carries
        // the value you just dialled in — the same rule the lane's drag has.
        if let Some(RollEdit::SetProperty { value, .. }) = edits.first() {
            property.set(&mut self.template, *value);
        }
        edits
    }

    // --------------------------------------------------------- clipboard ---

    /// Copies the selection, normalised so its earliest note starts at zero.
    /// Returns how many notes were taken; **zero leaves the clipboard alone**,
    /// so a stray Ctrl+C with nothing selected does not throw away what you had.
    pub fn copy(&mut self, notes: &Arena<NoteId, Note>) -> usize {
        let phrase = self.selected_phrase(notes);
        if phrase.is_empty() {
            return 0;
        }
        self.clipboard = phrase;
        self.clipboard.len()
    }

    pub fn cut(&mut self, notes: &Arena<NoteId, Note>) -> Vec<RollEdit> {
        if self.copy(notes) == 0 {
            return Vec::new();
        }
        vec![RollEdit::Remove(std::mem::take(&mut self.selection))]
    }

    /// Puts the clipboard down with its earliest note **on the grid** at or
    /// nearest `at`.
    ///
    /// Reported from using the window: *"when I paste notes in the piano roll
    /// they're off time instead of snapped."* The window works `at` out from
    /// the playhead, or from the raw pointer when the playhead is elsewhere in
    /// the song, and a pointer is never on a line — so a pasted phrase never
    /// was either.
    ///
    /// The snap happens here rather than at the call site because the roll is
    /// what owns the division, `Alt` included (see `live_snap`): a rule
    /// enforced by the caller is a rule the next caller forgets.
    pub fn paste(&mut self, at: Tick, beats_per_bar: u32) -> Vec<RollEdit> {
        if self.clipboard.is_empty() {
            return Vec::new();
        }
        let at = snap_tick(at.max(0), self.live_snap(), beats_per_bar).max(0);
        let notes: Vec<Note> = self
            .clipboard
            .iter()
            .map(|note| Note {
                start: note.start + at,
                ..*note
            })
            .collect();
        vec![RollEdit::Insert(notes)]
    }

    /// `Ctrl+B`: the selection again, starting where it ends.
    ///
    /// Rounded up to the next bar, which is what makes duplicating a phrase
    /// produce a phrase twice as long rather than an overlap nobody asked for.
    pub fn duplicate(&mut self, notes: &Arena<NoteId, Note>, beats_per_bar: u32) -> Vec<RollEdit> {
        let phrase = self.selected_phrase(notes);
        if phrase.is_empty() {
            return Vec::new();
        }
        let earliest = self
            .selection
            .iter()
            .filter_map(|id| notes.get(*id))
            .map(|n| n.start)
            .min()
            .unwrap_or(0);
        let end = self
            .selection
            .iter()
            .filter_map(|id| notes.get(*id))
            .map(|n| n.start + n.length)
            .max()
            .unwrap_or(earliest);

        let bar = PPQN * Tick::from(beats_per_bar.max(1));
        // `div_ceil` on a signed integer is still unstable, so this is it by
        // hand — and `end` is never negative, which is what makes it this short.
        let landing = (end + bar - 1) / bar * bar;
        let copies: Vec<Note> = phrase
            .into_iter()
            .map(|note| Note {
                start: note.start + landing,
                ..note
            })
            .collect();
        vec![RollEdit::Insert(copies)]
    }

    pub fn clipboard_len(&self) -> usize {
        self.clipboard.len()
    }

    /// The selection as notes, sorted and moved so the earliest starts at zero.
    fn selected_phrase(&self, notes: &Arena<NoteId, Note>) -> Vec<Note> {
        let mut phrase: Vec<Note> = self
            .selection
            .iter()
            .filter_map(|id| notes.get(*id).copied())
            .collect();
        if phrase.is_empty() {
            return phrase;
        }
        // Sorted so a paste is deterministic — the arena's own order is
        // whatever the ids happen to be, and a phrase that comes back in a
        // different order every time is one nobody can reason about.
        phrase.sort_by_key(|n| (n.start, n.key));
        let earliest = phrase.iter().map(|n| n.start).min().unwrap_or(0);
        for note in &mut phrase {
            note.start -= earliest;
        }
        phrase
    }

    /// The tick delta a move wants, snapped and clamped so the earliest
    /// selected note cannot be dragged before the start of the clip.
    ///
    /// `limits` is the gesture's, captured at the press — never the live
    /// document's. See [`MoveLimits`].
    fn snapped_delta(&self, pointer_tick: Tick, beats_per_bar: u32, limits: MoveLimits) -> Tick {
        let unit = snap_unit(self.live_snap(), beats_per_bar);
        let raw = pointer_tick - self.origin.0;
        let wanted = if unit > 0 {
            (raw as f64 / unit as f64).round() as Tick * unit
        } else {
            raw
        };
        wanted.max(limits.min_tick)
    }

    fn shortest_selected(&self, notes: &Arena<NoteId, Note>) -> Tick {
        self.selection
            .iter()
            .filter_map(|id| notes.get(*id))
            .map(|n| n.length)
            .min()
            .unwrap_or(1)
    }
}

/// What the roll draws with before anybody has drawn or clicked anything.
///
/// A sixteenth at velocity 100, dead centre — the note a person expects when
/// they press on an empty grid for the first time.
const BLANK_TEMPLATE: Note = Note {
    start: 0,
    length: PPQN / 4,
    key: 60,
    velocity: 100,
    pan: 0,
    fine_pitch: 0,
    release: 0,
    mod_x: 0,
    mod_y: 0,
    slide: false,
};

/// The rectangle two corners describe, whichever way round they came.
fn box_between(a: (f32, f32), b: (f32, f32)) -> Rect {
    let x = a.0.min(b.0);
    let y = a.1.min(b.1);
    Rect::new(x, y, (a.0 - b.0).abs(), (a.1 - b.1).abs())
}

/// Every note whose block overlaps `box_`, in pixels.
///
/// In pixels rather than in ticks and keys, because that is what the box is
/// drawn in: what it visibly covers is what it selects, at any zoom, dragged in
/// any direction.
fn notes_in(view: &RollView, grid: Rect, notes: &Arena<NoteId, Note>, box_: Rect) -> Vec<NoteId> {
    // Never empty: a box dragged along one axis is a line, and a line through a
    // row of notes is a perfectly ordinary way to select them.
    let box_ = Rect::new(box_.x, box_.y, box_.width.max(1.0), box_.height.max(1.0));
    notes
        .iter()
        .filter(|(_, note)| {
            let left = tick_to_x(view, grid, note.start);
            let right = tick_to_x(view, grid, note.start + note.length);
            let top = key_to_y(view, grid, note.key);
            let block = Rect::new(left, top, (right - left).max(1.0), view.key_height);
            block.intersects(&box_)
        })
        .map(|(id, _)| id)
        .collect()
}

/// Whether any note already covers this cell — what stops the Paint tool
/// stacking notes on top of each other.
fn note_covers(notes: &Arena<NoteId, Note>, tick: Tick, key: u8) -> bool {
    notes
        .values()
        .any(|n| n.key == key && tick >= n.start && tick < n.start + n.length)
}

// ------------------------------------------------------------ the cut tool ---

/// Where a line drawn from `from` to `to` cuts the notes it crosses.
///
/// **A note is cut where the line crosses the middle of its own row**, and
/// only if that lands strictly inside the note. Three things follow from that
/// one rule, and each is the reason for it:
///
/// - A **diagonal** stroke cuts a chord's notes at *different* times, because
///   it crosses each row at a different x. That is the gesture, not an
///   artefact — it is what a line is for, as against a click.
/// - A line drawn **along** a row never crosses its middle, so a horizontal
///   wiggle inside a long note cuts nothing rather than cutting it somewhere
///   nobody aimed at.
/// - A cut landing on a note's own **edge** is not offered. A zero-length note
///   is a note-on and a note-off at the same sample: silence you can neither
///   see nor select. (`SliceNotes` refuses one too — both is right, because
///   this is what stops the gesture *looking* like it did something.)
///
/// Each note appears at most once, so a stroke that wanders back over a row
/// does not offer two cuts for one note — the second would be measured against
/// a length the first has already changed.
pub fn slice_cuts(
    view: &RollView,
    grid: Rect,
    notes: &Arena<NoteId, Note>,
    from: (f32, f32),
    to: (f32, f32),
) -> Vec<(NoteId, Tick)> {
    let mut cuts = Vec::new();
    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
    // A press with no drag is somebody putting the pointer down, not a cut.
    if dx.abs() < f32::EPSILON && dy.abs() < f32::EPSILON {
        return cuts;
    }

    for (id, note) in notes.iter() {
        let row = key_to_y(view, grid, note.key) + view.key_height / 2.0;
        // Where the segment crosses this row, as a fraction along it. A line
        // that does not cross the row at all — including one lying exactly on
        // it — has no answer, and that is the horizontal case above.
        if dy.abs() < f32::EPSILON {
            continue;
        }
        let along = (row - from.1) / dy;
        if !(0.0..=1.0).contains(&along) {
            continue; // the row is beyond one end of the stroke
        }
        let x = from.0 + dx * along;
        let at = x_to_tick(view, grid, x);
        if at <= note.start || at >= note.start + note.length {
            continue;
        }
        cuts.push((id, at));
    }
    cuts
}
