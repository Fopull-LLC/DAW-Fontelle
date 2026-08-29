//! The piano roll (TDD §16.4, §16.5; item 8 of `docs/first-usable-plan.md`).
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

use std::ops::Range;

use fontelle_model::{Arena, Note};
use fontelle_types::{NoteId, PPQN, Tick};

use crate::layout::Rect;
use crate::theme::Metrics;

/// The lowest and highest MIDI keys, and the count between them.
const KEY_COUNT: i32 = 128;

/// How wide a note's resize handle is, in pixels.
const HANDLE_PX: f32 = 6.0;

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
    /// Row height.
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
            key_height: 12.0,
            snap: SnapDivision::Step,
        }
    }
}

/// The roll's parts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RollLayout {
    pub frame: Rect,
    /// The keyboard down the left, beside the grid.
    pub keys: Rect,
    /// The bar ruler across the top, above the grid.
    pub ruler: Rect,
    /// Where the notes are.
    pub grid: Rect,
}

/// Wide enough for a key name and narrow enough not to eat the song.
const KEYBOARD_WIDTH: f32 = 56.0;

pub fn roll_layout(frame: Rect, metrics: &Metrics) -> RollLayout {
    let ruler_height = metrics.row_height;
    let keys_width = KEYBOARD_WIDTH.min(frame.width.max(0.0));

    let ruler = Rect::new(
        frame.x,
        frame.y,
        frame.width,
        ruler_height.min(frame.height.max(0.0)),
    )
    .clamped();
    let below = Rect::new(
        frame.x,
        ruler.bottom(),
        frame.width,
        frame.height - ruler.height,
    )
    .clamped();

    let keys = Rect::new(below.x, below.y, keys_width, below.height).clamped();
    let grid = Rect::new(
        keys.right(),
        below.y,
        below.width - keys.width,
        below.height,
    )
    .clamped();

    RollLayout {
        frame,
        keys,
        ruler,
        grid,
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

// -------------------------------------------------------------- editing ---

/// What the roll wants done to the document.
///
/// Values, not commands: this crate cannot see `fontelle_model::Command` being
/// applied and should not want to. `fontelle-app` turns each of these into the
/// matching command and puts it through `History`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RollEdit {
    Add {
        tick: Tick,
        key: u8,
        length: Tick,
        velocity: u8,
    },
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
}

/// The tools §16.5 names. The gate needs the first three.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Draw,
    Paint,
    Delete,
    Select,
    Slice,
    Mute,
    Slip,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
}

/// What the pointer is in the middle of doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Gesture {
    None,
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
}

/// The roll's own state: where it is looking, what is selected, what the mouse
/// is doing.
pub struct PianoRoll {
    pub view: RollView,
    pub tool: Tool,
    /// The velocity a newly drawn note gets.
    pub default_velocity: u8,
    /// How long a newly drawn note is, when snap is off.
    pub default_length: Tick,
    selection: Vec<NoteId>,
    gesture: Gesture,
    /// Where the pointer was when the gesture started, in document units.
    origin: (Tick, u8),
}

impl PianoRoll {
    pub fn new(view: RollView) -> Self {
        Self {
            view,
            tool: Tool::Draw,
            default_velocity: 100,
            default_length: PPQN / 4,
            selection: Vec::new(),
            gesture: Gesture::None,
            origin: (0, 0),
        }
    }

    pub fn selection(&self) -> &[NoteId] {
        &self.selection
    }

    pub fn select_all(&mut self, notes: &Arena<NoteId, Note>) {
        self.selection = notes.keys().collect();
    }

    pub fn clear_selection(&mut self) {
        self.selection.clear();
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
                if self.tool != Tool::Draw {
                    return Vec::new();
                }
                let unit = snap_unit(self.view.snap, beats_per_bar);
                vec![RollEdit::Add {
                    tick: snap_tick(tick, self.view.snap, beats_per_bar),
                    key,
                    length: if unit > 0 { unit } else { self.default_length },
                    velocity: self.default_velocity,
                }]
            }
            RollHit::Outside => Vec::new(),
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
        if self.selection.is_empty() {
            return Vec::new();
        }
        let tick = x_to_tick(&self.view, grid, x);
        let key = y_to_key(&self.view, grid, y);

        match self.gesture {
            Gesture::None => Vec::new(),

            Gesture::Moving {
                applied_tick,
                applied_key,
            } => {
                // Snap the *destination*, not the delta: a note that started
                // off the grid should land on it, which is what dragging a
                // sloppily-placed note onto the beat is for.
                let wanted_tick = self.snapped_delta(tick, beats_per_bar, notes, grid);
                let wanted_key = i16::from(key) - i16::from(self.origin.1);

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
                let unit = snap_unit(self.view.snap, beats_per_bar);
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

    /// The tick delta a move wants, snapped and clamped so the earliest
    /// selected note cannot be dragged before the start of the clip.
    fn snapped_delta(
        &self,
        pointer_tick: Tick,
        beats_per_bar: u32,
        notes: &Arena<NoteId, Note>,
        _grid: Rect,
    ) -> Tick {
        let unit = snap_unit(self.view.snap, beats_per_bar);
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
