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

use fontelle_model::{Arena, Note};
use fontelle_types::{NoteId, PPQN, Tick};

use crate::layout::Rect;
use crate::theme::Metrics;

/// The lowest and highest MIDI keys, and the count between them.
const KEY_COUNT: i32 = 128;

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
    /// The label strip beside the velocity lane, lined up with the keyboard.
    pub velocity_keys: Rect,
    /// The velocity lane (§16.5's first note property lane). Empty when it is
    /// hidden, and then the grid has the room instead.
    pub velocity: Rect,
}

/// Wide enough for a key name and narrow enough not to eat the song.
const KEYBOARD_WIDTH: f32 = 56.0;

/// How tall the velocity lane is.
const VELOCITY_HEIGHT: f32 = 78.0;

pub fn roll_layout(frame: Rect, metrics: &Metrics, velocity_lane: bool) -> RollLayout {
    let (toolbar, under_toolbar) = frame.split_top(metrics.row_height.min(frame.height.max(0.0)));

    let ruler_height = metrics.row_height;
    let keys_width = KEYBOARD_WIDTH.min(under_toolbar.width.max(0.0));

    let (ruler, below) = under_toolbar.split_top(ruler_height);

    // The lane comes out of the bottom before the grid is measured, so the
    // grid never has to be shrunk after the fact — which is the version of
    // this that leaves a one-pixel seam.
    let lane_height = if velocity_lane {
        VELOCITY_HEIGHT.min((below.height - metrics.row_height).max(0.0))
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

    RollLayout {
        frame,
        toolbar,
        keys,
        ruler,
        grid,
        velocity_keys,
        velocity,
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

    view.key_height = (view.key_height * factor).clamp(MIN_KEY_HEIGHT, MAX_KEY_HEIGHT);

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

/// The velocity a point in the lane means: loud at the top, quiet at the
/// bottom, and **never zero** — a note-on with velocity zero is a note-off in
/// MIDI, so a lane that can produce one can silently delete a note.
pub fn velocity_of_y(lane: Rect, y: f32) -> u8 {
    if lane.height <= 0.0 {
        return 100;
    }
    let t = ((y - lane.y) / lane.height).clamp(0.0, 1.0);
    let value = (127.0 - t * 126.0).round() as i32;
    value.clamp(1, 127) as u8
}

/// Where a note's velocity bar reaches in the lane.
pub fn velocity_to_y(lane: Rect, velocity: u8) -> f32 {
    if lane.height <= 0.0 {
        return lane.y;
    }
    let t = (127.0 - f32::from(velocity.max(1))) / 126.0;
    lane.y + t.clamp(0.0, 1.0) * lane.height
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
    Add {
        tick: Tick,
        key: u8,
        length: Tick,
        velocity: u8,
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
    /// One value for every named note — the velocity lane's whole vocabulary.
    SetVelocity {
        ids: Vec<NoteId>,
        velocity: u8,
    },
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
    /// Shows and hides the velocity lane.
    Velocity,
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
            Self::Velocity => "vel",
        }
    }

    /// The keyboard shortcut worth writing in a tooltip, if there is one.
    pub fn shortcut(self) -> Option<&'static str> {
        match self {
            Self::Tool(Tool::Draw) => Some("P"),
            Self::Tool(Tool::Paint) => Some("B"),
            Self::Tool(Tool::Select) => Some("E"),
            Self::Tool(Tool::Delete) => Some("D"),
            Self::Snap => Some("S"),
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
const TOOLBAR: [(RollControl, f32); 10] = [
    (RollControl::Tool(Tool::Draw), 40.0),
    (RollControl::Tool(Tool::Paint), 42.0),
    (RollControl::Tool(Tool::Select), 34.0),
    (RollControl::Tool(Tool::Delete), 34.0),
    (RollControl::Snap, 52.0),
    (RollControl::ZoomOutX, 24.0),
    (RollControl::ZoomInX, 24.0),
    (RollControl::ZoomOutY, 24.0),
    (RollControl::ZoomInY, 24.0),
    (RollControl::Velocity, 34.0),
];

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

// ------------------------------------------------------------- gestures ---

/// What the pointer is in the middle of doing.
#[derive(Debug, Clone, PartialEq)]
enum Gesture {
    None,
    /// A note has been asked for and its id is not known yet. The next
    /// [`PianoRoll::note_added`] turns this into a resize of it.
    PendingAdd {
        /// Where the new note's right edge is, which is what the resize
        /// measures from.
        end: Tick,
        key: u8,
    },
    /// Dragging the selection around. `applied` is how far the drag has been
    /// committed so far, so each step can emit only the difference.
    Moving {
        applied_tick: Tick,
        applied_key: i16,
    },
    /// Dragging a note's right edge.
    Resizing {
        applied_tick: Tick,
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
    /// Dragging in the velocity lane.
    Velocity {
        ids: Vec<NoteId>,
    },
}

/// The roll's own state: where it is looking, what is selected, what the mouse
/// is doing.
pub struct PianoRoll {
    pub view: RollView,
    pub tool: Tool,
    /// Whether the velocity lane is showing.
    pub velocity_lane: bool,
    /// The velocity a newly drawn note gets.
    pub default_velocity: u8,
    /// How long a newly drawn note is, when snap is off.
    pub default_length: Tick,
    selection: Vec<NoteId>,
    gesture: Gesture,
    /// Where the pointer was when the gesture started, in document units.
    origin: (Tick, u8),
    modifiers: Modifiers,
    /// The phrase Ctrl+C put there, normalised so its earliest note starts at
    /// tick zero — which is what lets a paste land anywhere.
    clipboard: Vec<Note>,
}

impl PianoRoll {
    pub fn new(view: RollView) -> Self {
        Self {
            view,
            tool: Tool::Draw,
            velocity_lane: true,
            default_velocity: 100,
            default_length: PPQN / 4,
            selection: Vec::new(),
            gesture: Gesture::None,
            origin: (0, 0),
            modifiers: Modifiers::default(),
            clipboard: Vec::new(),
        }
    }

    pub fn selection(&self) -> &[NoteId] {
        &self.selection
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
        let Gesture::PendingAdd { end, key } = self.gesture else {
            return;
        };
        self.selection = vec![id];
        self.origin = (end, key);
        self.gesture = Gesture::Resizing { applied_tick: 0 };
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

        // Right-click deletes in any tool (§16.5).
        if button == MouseButton::Right || self.tool == Tool::Delete {
            return match hit {
                RollHit::Note(id, _) => {
                    self.selection.retain(|s| *s != id);
                    vec![RollEdit::Remove(vec![id])]
                }
                _ => Vec::new(),
            };
        }

        match hit {
            RollHit::Note(id, part) => {
                // Pressing a note that is already part of the selection keeps
                // it — otherwise dragging a chord would collapse it to
                // whichever note you happened to grab.
                if !self.selection.contains(&id) {
                    self.selection = vec![id];
                }
                self.origin = (
                    x_to_tick(&self.view, grid, x),
                    y_to_key(&self.view, grid, y),
                );
                self.gesture = match part {
                    NotePart::RightEdge => Gesture::Resizing { applied_tick: 0 },
                    _ => Gesture::Moving {
                        applied_tick: 0,
                        applied_key: 0,
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
                let unit = snap_unit(snap, beats_per_bar);
                let length = if unit > 0 { unit } else { self.default_length };
                let start = snap_tick(tick, snap, beats_per_bar);
                self.gesture = if self.tool == Tool::Paint {
                    Gesture::Painting { last: (start, key) }
                } else {
                    Gesture::PendingAdd {
                        end: start + length,
                        key,
                    }
                };
                vec![RollEdit::Add {
                    tick: start,
                    key,
                    length,
                    velocity: self.default_velocity,
                }]
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
        let tick = x_to_tick(&self.view, grid, x);
        let key = y_to_key(&self.view, grid, y);

        match self.gesture.clone() {
            Gesture::None | Gesture::PendingAdd { .. } | Gesture::Velocity { .. } => Vec::new(),

            Gesture::Marquee { from, .. } => {
                self.gesture = Gesture::Marquee { from, to: (x, y) };
                Vec::new()
            }

            Gesture::Painting { last } => {
                let snap = self.live_snap();
                let unit = snap_unit(snap, beats_per_bar);
                let length = if unit > 0 { unit } else { self.default_length };
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
                vec![RollEdit::Add {
                    tick: start,
                    key,
                    length,
                    velocity: self.default_velocity,
                }]
            }

            Gesture::Moving {
                applied_tick,
                applied_key,
            } => {
                if self.selection.is_empty() {
                    return Vec::new();
                }
                // Snap the *destination*, not the delta: a note that started
                // off the grid should land on it, which is what dragging a
                // sloppily-placed note onto the beat is for.
                let mut wanted_tick = self.snapped_delta(tick, beats_per_bar, notes);
                let mut wanted_key = i16::from(key) - i16::from(self.origin.1);

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
                self.gesture = Gesture::Moving {
                    applied_tick: wanted_tick,
                    applied_key: wanted_key,
                };
                vec![RollEdit::Move {
                    ids: self.selection.clone(),
                    tick_delta: d_tick,
                    key_delta: d_key,
                }]
            }

            Gesture::Resizing { applied_tick } => {
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
                // snap unit, or one tick with snap off.
                let floor = self.shortest_selected(notes);
                let wanted = wanted.max(-(floor - unit.max(1)));
                let delta = wanted - applied_tick;
                if delta == 0 {
                    return Vec::new();
                }
                self.gesture = Gesture::Resizing {
                    applied_tick: wanted,
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
    pub fn release_over(&mut self, x: f32, y: f32, grid: Rect, notes: &Arena<NoteId, Note>) {
        if let Gesture::Marquee { from, .. } = self.gesture {
            let box_ = box_between(from, (x, y));
            self.selection = notes_in(&self.view, grid, notes, box_);
        }
        self.gesture = Gesture::None;
    }

    // ------------------------------------------------------ velocity lane ---

    /// Pressing in the velocity lane. Sets the note under the column, or the
    /// whole selection when the column is part of it.
    pub fn press_velocity(
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
        self.gesture = Gesture::Velocity { ids: ids.clone() };
        vec![RollEdit::SetVelocity {
            ids,
            velocity: velocity_of_y(lane, y),
        }]
    }

    pub fn drag_velocity(
        &mut self,
        _x: f32,
        y: f32,
        lane: Rect,
        _grid: Rect,
        _notes: &Arena<NoteId, Note>,
    ) -> Vec<RollEdit> {
        let Gesture::Velocity { ids } = &self.gesture else {
            return Vec::new();
        };
        // The notes are the ones the press caught, not whatever is under the
        // pointer now: dragging sideways across the lane while setting a
        // velocity would otherwise rewrite the whole bar.
        vec![RollEdit::SetVelocity {
            ids: ids.clone(),
            velocity: velocity_of_y(lane, y),
        }]
    }

    /// Whether a drag in progress belongs to the velocity lane.
    pub fn is_editing_velocity(&self) -> bool {
        matches!(self.gesture, Gesture::Velocity { .. })
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

    /// Puts the clipboard down with its earliest note at `at`.
    pub fn paste(&mut self, at: Tick) -> Vec<RollEdit> {
        if self.clipboard.is_empty() {
            return Vec::new();
        }
        let at = at.max(0);
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
    fn snapped_delta(
        &self,
        pointer_tick: Tick,
        beats_per_bar: u32,
        notes: &Arena<NoteId, Note>,
    ) -> Tick {
        let unit = snap_unit(self.live_snap(), beats_per_bar);
        let raw = pointer_tick - self.origin.0;
        let wanted = if unit > 0 {
            (raw as f64 / unit as f64).round() as Tick * unit
        } else {
            raw
        };
        let earliest = self
            .selection
            .iter()
            .filter_map(|id| notes.get(*id))
            .map(|n| n.start)
            .min()
            .unwrap_or(0);
        wanted.max(-earliest)
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
