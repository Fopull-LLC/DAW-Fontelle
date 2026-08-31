//! The arrangement canvas (TDD §16.4; the half of item 9 of
//! `docs/first-usable-plan.md` that was outstanding).
//!
//! Clips as blocks on lanes: move them, size them, duplicate them, mute them,
//! delete them, and click one to open it in the piano roll. It is the piano
//! roll's shape at a coarser grain and it obeys the same two rules —
//!
//! - **Virtualisation is mandatory** (§16.4). [`visible_lanes`] and
//!   [`visible_ticks`](crate::canvas::visible_ticks)'s counterpart here bound
//!   every loop, so a project with two hundred lanes costs a screenful of
//!   rectangles.
//! - **The canvas is a view, never a mutator** (INVARIANT 2). It reads
//!   [`ClipInfo`] and returns [`ArrangeEdit`] values; turning those into
//!   `Command`s is `fontelle-app`'s job.
//!
//! Direct-draw, not a widget tree, for the reason §16.4 gives: grid, clips and
//! playhead are separate layers with independent invalidation, so a moving
//! playhead never redirties clip geometry.

use std::ops::Range;

use fontelle_types::{ClipId, PPQN, Tick};

use crate::canvas::piano_roll::{SnapDivision, snap_tick, snap_unit};
use crate::canvas::{Modifiers, MouseButton, clamp_to_grid};
use crate::document::ClipInfo;
use crate::layout::Rect;
use crate::theme::Metrics;

/// How wide the grab handle on a clip's right-hand edge is.
const HANDLE_PX: f32 = 7.0;

/// Zoom limits. A pixels-per-tick of zero is a division by zero in every
/// conversion here; a lane taller than the panel is not a zoom.
pub const MIN_TIMELINE_PPT: f32 = 0.0005;
pub const MAX_TIMELINE_PPT: f32 = 0.5;
pub const MIN_LANE_ROW: f32 = 14.0;
pub const MAX_LANE_ROW: f32 = 96.0;

/// Where the arrangement is looking, and how closely.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimelineView {
    /// The tick at the left edge of the grid.
    pub scroll_tick: Tick,
    /// The lane in the top row. Lanes read downwards, unlike the roll's keys —
    /// a track list is a list, not a keyboard.
    pub top_lane: usize,
    pub pixels_per_tick: f32,
    pub lane_height: f32,
    pub snap: SnapDivision,
}

impl Default for TimelineView {
    fn default() -> Self {
        Self {
            scroll_tick: 0,
            top_lane: 0,
            // About a hundred pixels to the bar: enough of a piece on screen to
            // be an arrangement rather than a magnified clip.
            pixels_per_tick: 0.025,
            lane_height: 34.0,
            // Bars, because that is what an arrangement is built out of.
            snap: SnapDivision::Bar,
        }
    }
}

/// The arrangement's parts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimelineLayout {
    pub frame: Rect,
    /// The controls across the top: the snap chip, repeat, the clipboard,
    /// mute. Added because the arrangement had none at all — see
    /// [`TimelineControl`].
    pub toolbar: Rect,
    /// The bar ruler, under the toolbar. Clicking it moves the time marker.
    pub ruler: Rect,
    /// The lane names down the left, beside the grid.
    pub headers: Rect,
    /// Where the clips are.
    pub grid: Rect,
}

/// Wide enough for a lane name and narrow enough not to eat the arrangement.
const HEADER_WIDTH: f32 = 120.0;

pub fn timeline_layout(frame: Rect, metrics: &Metrics) -> TimelineLayout {
    let row = metrics.row_height.min(frame.height.max(0.0));
    let (toolbar, under_toolbar) = frame.split_top(row);
    let (ruler, below) =
        under_toolbar.split_top(metrics.row_height.min(under_toolbar.height.max(0.0)));
    let header_width = HEADER_WIDTH.min(below.width.max(0.0));
    let headers = Rect::new(below.x, below.y, header_width, below.height).clamped();
    let grid = Rect::new(
        headers.right(),
        below.y,
        below.width - headers.width,
        below.height,
    )
    .clamped();
    TimelineLayout {
        frame,
        toolbar,
        ruler,
        headers,
        grid,
    }
}

/// One control on the arrangement's toolbar.
///
/// The arrangement had none. `TimelineView` has carried a `snap` since it was
/// written and `duplicate` has existed since the arrangement did; neither had
/// anywhere to appear, so both were rumours rather than controls — which is
/// exactly how they were reported (*"I can only change the length and move
/// them around"*, *"I don't see snap controls"*).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimelineControl {
    /// Draw: a press on empty grid makes a clip.
    Draw,
    /// Select: a press on empty grid marquees, the way it always did.
    Select,
    /// Cycles the arrangement's snap; the chip says which division is on.
    Snap,
    /// The selection again, after itself — the answer to *"I made this drum
    /// loop but I can't repeat it"*.
    ///
    /// A **copy**: new clips with notes of their own, which edit
    /// independently. [`Loop`](Self::Loop) is the other reading of the same
    /// sentence and they are deliberately two buttons, because they are two
    /// features and conflating them is what the arrangement used to do.
    Repeat,
    /// Makes the selection repeat its own content, or stops it.
    ///
    /// One clip, one set of notes, played again every period — so editing bar
    /// one changes every repeat. Shift-dragging a clip's right-hand grip is
    /// the same thing without coming up here for it.
    Loop,
    Cut,
    Copy,
    Paste,
    /// Mutes the selection, or unmutes it when all of it is muted.
    Mute,
    ZoomOut,
    ZoomIn,
}

impl TimelineControl {
    /// What the button says. The snap chip is the exception — it says its
    /// division, because the state is the useful half.
    pub fn label(self) -> &'static str {
        match self {
            Self::Draw => "Draw",
            Self::Select => "Sel",
            Self::Snap => "snap",
            Self::Repeat => "Repeat",
            Self::Loop => "Loop",
            Self::Cut => "Cut",
            Self::Copy => "Copy",
            Self::Paste => "Paste",
            Self::Mute => "Mute",
            Self::ZoomOut => "-",
            Self::ZoomIn => "+",
        }
    }

    /// The glyph this control draws, or `None` when it draws its own value.
    /// The same rule the roll's toolbar follows — see [`crate::canvas::RollControl::icon`].
    pub fn icon(self) -> Option<crate::icon::Icon> {
        use crate::icon::Icon;
        Some(match self {
            Self::Draw => Icon::Pencil,
            Self::Select => Icon::Marquee,
            // One clip coming round again, against copies laid after it. Two
            // features, two pictures.
            Self::Loop => Icon::Loop,
            Self::Repeat => Icon::Repeat,
            Self::Cut => Icon::Cut,
            Self::Copy => Icon::Copy,
            Self::Paste => Icon::Paste,
            Self::Mute => Icon::Mute,
            Self::ZoomOut => Icon::ZoomOut,
            Self::ZoomIn => Icon::ZoomIn,
            // The snap chip says its division, which is a value.
            Self::Snap => return None,
        })
    }

    /// What a hover tip says (see [`crate::tooltip`]).
    ///
    /// Beside `label` and `icon` rather than in a table somewhere else, so
    /// that a control added without an explanation is a hole in this match
    /// rather than a silent miss.
    ///
    /// The shortcut is **not** repeated here — the renderer appends
    /// [`shortcut`](Self::shortcut) to the tip it draws, so the two cannot
    /// drift.
    pub fn tip(self) -> Option<&'static str> {
        Some(match self {
            Self::Draw => "Draw clips on empty bars",
            Self::Select => "Select clips; drag on empty bars to marquee",
            Self::Snap => "What clips snap to \u{2014} click to cycle",
            // The two readings of "repeat this" are two buttons on purpose,
            // and the tips are where the difference is said out loud.
            Self::Repeat => "Copy the selection after itself \u{2014} edits apart",
            Self::Loop => "Repeat one clip's own notes \u{2014} edits together",
            Self::Cut => "Cut the selected clips",
            Self::Copy => "Copy the selected clips",
            Self::Paste => "Paste at the marker",
            Self::Mute => "Mute the selected clips",
            Self::ZoomOut => "Zoom out",
            Self::ZoomIn => "Zoom in",
        })
    }

    /// The keyboard shortcut worth writing down, if there is one.
    pub fn shortcut(self) -> Option<&'static str> {
        match self {
            Self::Draw => Some("P"),
            Self::Select => Some("E"),
            Self::Repeat => Some("Ctrl+B"),
            Self::Loop => Some("Shift+drag"),
            Self::Cut => Some("Ctrl+X"),
            Self::Copy => Some("Ctrl+C"),
            Self::Paste => Some("Ctrl+V"),
            Self::Mute => Some("Ctrl+M"),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TimelineToolbar {
    pub items: Vec<(TimelineControl, Rect)>,
}

/// The controls, left to right: what the grid is, then what to do to a clip,
/// then how close you are looking. The same order as the roll's toolbar, for
/// the same reason — the two panels are read the same way.
const TIMELINE_TOOLBAR: [(TimelineControl, f32); 11] = [
    // The tools first: they decide what every other press on the grid means,
    // and the roll's toolbar is read the same way round.
    (TimelineControl::Draw, 26.0),
    (TimelineControl::Select, 26.0),
    (TimelineControl::Snap, 52.0),
    // Loop next to Repeat, deliberately: they are the two readings of "play
    // this again" and seeing them side by side is what teaches the
    // difference. One makes copies you edit separately; the other makes one
    // clip come round again.
    (TimelineControl::Loop, 26.0),
    (TimelineControl::Repeat, 26.0),
    (TimelineControl::Cut, 26.0),
    (TimelineControl::Copy, 26.0),
    (TimelineControl::Paste, 26.0),
    (TimelineControl::Mute, 26.0),
    (TimelineControl::ZoomOut, 26.0),
    (TimelineControl::ZoomIn, 26.0),
];

const TOOLBAR_PAD: f32 = 4.0;
const TOOLBAR_GAP: f32 = 4.0;

/// Lays the arrangement's controls out inside `toolbar`.
///
/// A control that will not fit is **left out** rather than squeezed or drawn
/// off the end — the same rule the roll's toolbar follows, and the reason a
/// narrow panel degrades to the controls it has room for instead of a row of
/// overlapping boxes.
pub fn timeline_toolbar_layout(toolbar: Rect, metrics: &Metrics) -> TimelineToolbar {
    let mut items = Vec::with_capacity(TIMELINE_TOOLBAR.len());
    if toolbar.is_empty() {
        return TimelineToolbar { items };
    }
    // Never taller than a row, so a toolbar in a stretched panel stays a
    // toolbar rather than a band of tall buttons.
    let height = (toolbar.height - TOOLBAR_PAD).clamp(0.0, metrics.row_height);
    let mut x = toolbar.x + TOOLBAR_PAD;
    for (control, width) in TIMELINE_TOOLBAR {
        if x + width > toolbar.right() - TOOLBAR_PAD {
            continue;
        }
        items.push((
            control,
            Rect::new(x, toolbar.y + TOOLBAR_PAD / 2.0, width, height).clamped(),
        ));
        x += width + TOOLBAR_GAP;
    }
    TimelineToolbar { items }
}

/// Which control `(x, y)` is on, if any.
pub fn timeline_toolbar_hit(bar: &TimelineToolbar, x: f32, y: f32) -> Option<TimelineControl> {
    bar.items
        .iter()
        .find(|(_, rect)| rect.contains(x, y))
        .map(|(control, _)| *control)
}

// ------------------------------------------------------------- geometry ---

pub fn timeline_tick_to_x(view: &TimelineView, grid: Rect, tick: Tick) -> f32 {
    grid.x + (tick - view.scroll_tick) as f32 * view.pixels_per_tick
}

/// Clamped at zero: left of bar 1 is the start of the song, not a negative tick
/// every conversion downstream would have to defend against.
pub fn timeline_x_to_tick(view: &TimelineView, grid: Rect, x: f32) -> Tick {
    if view.pixels_per_tick <= 0.0 {
        return view.scroll_tick.max(0);
    }
    let tick = view.scroll_tick as f32 + (x - grid.x) / view.pixels_per_tick;
    (tick.round() as Tick).max(0)
}

pub fn lane_to_y(view: &TimelineView, grid: Rect, lane: usize) -> f32 {
    grid.y + (lane as f32 - view.top_lane as f32) * view.lane_height
}

pub fn y_to_lane(view: &TimelineView, grid: Rect, y: f32) -> usize {
    if view.lane_height <= 0.0 {
        return view.top_lane;
    }
    let rows = ((y - grid.y) / view.lane_height).floor() as i64;
    (view.top_lane as i64 + rows).max(0) as usize
}

/// The tick range the grid can show, inclusive of the partly visible column on
/// the right — build geometry for this and nothing else (§16.4).
pub fn timeline_visible_ticks(view: &TimelineView, grid: Rect) -> Range<Tick> {
    if view.pixels_per_tick <= 0.0 || grid.width <= 0.0 {
        return view.scroll_tick..view.scroll_tick;
    }
    let span = (grid.width / view.pixels_per_tick).ceil() as Tick;
    view.scroll_tick..view.scroll_tick + span + 1
}

/// The lanes the grid can show, clamped to how many the project has.
pub fn visible_lanes(view: &TimelineView, grid: Rect, lane_count: usize) -> Range<usize> {
    if view.lane_height <= 0.0 || grid.height <= 0.0 || lane_count == 0 {
        return 0..0;
    }
    let rows = (grid.height / view.lane_height).ceil() as usize + 1;
    let top = view.top_lane.min(lane_count.saturating_sub(1));
    top..(top + rows).min(lane_count)
}

/// The block a clip draws as.
///
/// Always at least a pixel wide, for the same reason a note is: a clip too
/// short to see is still a clip, and one that vanishes cannot be clicked to
/// find out why.
pub fn clip_rect(view: &TimelineView, grid: Rect, clip: &ClipInfo) -> Rect {
    let x0 = timeline_tick_to_x(view, grid, clip.start);
    let x1 = timeline_tick_to_x(view, grid, clip.start + clip.length);
    Rect::new(
        x0,
        lane_to_y(view, grid, clip.lane),
        (x1 - x0).max(1.0),
        view.lane_height,
    )
}

/// Zooms time about `anchor_x`, so whatever is under the pointer stays under
/// it — [`crate::canvas::zoom_x`]'s counterpart, and the same arithmetic.
/// How much air a curve keeps between itself and the block's own edges.
///
/// A point at 0.0 or 1.0 is drawn as a dot, and a dot centred on the edge is
/// half a dot — and half of it is over the lane next door.
const CURVE_INSET: f32 = 3.0;

/// A [`ClipKind::Automation`](crate::document::ClipKind) block's curve, in
/// screen points.
///
/// `curve` is `(tick from the clip's start, value 0..1)` in time order —
/// exactly what [`ClipInfo::curve`](crate::document::ClipInfo::curve) carries.
///
/// **One is up.** A curve drawn upside down is a lie about the value and the
/// mistake is invisible until you put it beside the editor, so it is worth a
/// test of its own.
///
/// Everything is clamped into `block`: nothing should produce a point outside
/// it, and one that escaped would be drawn over the lane above.
pub fn automation_polyline(block: Rect, length: Tick, curve: &[(Tick, f64)]) -> Vec<(f32, f32)> {
    if block.is_empty() || curve.is_empty() {
        return Vec::new();
    }
    let inner = block.inset(CURVE_INSET);
    // A block narrower than twice the inset has no inside; drawing down its
    // middle is better than drawing nothing.
    let (top, height) = if inner.height > 0.0 {
        (inner.y, inner.height)
    } else {
        (block.y + block.height / 2.0, 0.0)
    };
    curve
        .iter()
        .map(|(tick, value)| {
            // A clip of no length is one instant: everything in it is at its
            // left-hand edge, rather than a division by zero.
            let along = if length > 0 {
                (*tick as f32 / length as f32).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let up = value.clamp(0.0, 1.0) as f32;
            (
                (block.x + block.width * along).clamp(block.x, block.right()),
                (top + height * (1.0 - up)).clamp(block.y, block.bottom()),
            )
        })
        .collect()
}

pub fn timeline_zoom_x(view: &mut TimelineView, grid: Rect, anchor_x: f32, factor: f32) {
    if view.pixels_per_tick <= 0.0 || !factor.is_finite() || factor <= 0.0 {
        return;
    }
    let offset = f64::from(anchor_x - grid.x);
    let anchor_tick = view.scroll_tick as f64 + offset / f64::from(view.pixels_per_tick);
    view.pixels_per_tick =
        (view.pixels_per_tick * factor).clamp(MIN_TIMELINE_PPT, MAX_TIMELINE_PPT);
    let scroll = anchor_tick - offset / f64::from(view.pixels_per_tick);
    view.scroll_tick = (scroll.round() as Tick).max(0);
}

pub fn timeline_zoom_y(view: &mut TimelineView, factor: f32) {
    if !factor.is_finite() || factor <= 0.0 {
        return;
    }
    view.lane_height = (view.lane_height * factor).clamp(MIN_LANE_ROW, MAX_LANE_ROW);
}

// ---------------------------------------------------------- hit-testing ---

/// Which bit of a clip is under the pointer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipPart {
    Body,
    RightEdge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimelineHit {
    Clip(ClipId, ClipPart),
    /// Bare grid. The tick is **unsnapped**, for the reason
    /// [`crate::canvas::RollHit::Empty`] gives.
    Empty {
        tick: Tick,
        lane: usize,
    },
    /// The bar ruler, at this tick — where the time marker is set.
    Ruler(Tick),
    /// A lane's name column.
    Lane(usize),
    Outside,
}

pub fn timeline_hit(
    view: &TimelineView,
    layout: &TimelineLayout,
    clips: &[ClipInfo],
    x: f32,
    y: f32,
) -> TimelineHit {
    if layout.ruler.contains(x, y) {
        // Measured against the *grid*, not the ruler: the ruler runs the whole
        // width of the panel and the grid starts after the header column, so
        // taking the ruler's own x would put every bar line in the wrong place.
        return TimelineHit::Ruler(timeline_x_to_tick(view, layout.grid, x.max(layout.grid.x)));
    }
    if layout.headers.contains(x, y) {
        return TimelineHit::Lane(y_to_lane(view, layout.grid, y));
    }
    if !layout.grid.contains(x, y) {
        return TimelineHit::Outside;
    }

    // Last match wins: later clips are drawn over earlier ones, so the one on
    // top is the one that was clicked.
    let mut found = None;
    for clip in clips {
        let block = clip_rect(view, layout.grid, clip);
        if !block.contains(x, y) {
            continue;
        }
        // A block narrower than three handles has none: moving a clip is more
        // common than sizing one, and a clip you cannot grab is worse than one
        // you cannot size without zooming in.
        let handle = HANDLE_PX.min(block.width / 3.0);
        found = Some(TimelineHit::Clip(
            clip.id,
            if x >= block.right() - handle {
                ClipPart::RightEdge
            } else {
                ClipPart::Body
            },
        ));
    }
    found.unwrap_or(TimelineHit::Empty {
        tick: timeline_x_to_tick(view, layout.grid, x),
        lane: y_to_lane(view, layout.grid, y),
    })
}

// -------------------------------------------------------------- editing ---

/// What the arrangement wants done to the document.
///
/// Values, not commands, for the reason [`crate::canvas::RollEdit`] gives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArrangeEdit {
    /// Deltas, **relative to the previous step of the same drag** — which is
    /// what `MoveClip` takes and what lets `merge_with` coalesce a drag into
    /// one history entry.
    Move {
        ids: Vec<ClipId>,
        tick_delta: Tick,
        lane_delta: i32,
    },
    Resize {
        ids: Vec<ClipId>,
        tick_delta: Tick,
    },
    Duplicate {
        ids: Vec<ClipId>,
        tick_offset: Tick,
    },
    Remove(Vec<ClipId>),
    SetMuted {
        ids: Vec<ClipId>,
        muted: bool,
    },
    /// Keep these clips, so a later [`Paste`](Self::Paste) can put them down
    /// again.
    ///
    /// The clips themselves are **the host's** to hold, not this canvas's. A
    /// [`ClipInfo`] is a flattened view for drawing — no notes, no channel —
    /// so a canvas holding copied clips would be holding something INVARIANT 2
    /// says it may not see. It is also what makes cut work: paste has to put
    /// something down after the clip it copied from has been deleted.
    Copy(Vec<ClipId>),
    /// Put whatever was copied down, earliest clip at `at`.
    Paste {
        at: Tick,
    },
    /// Make a clip on `lane`, starting at `start` — the draw tool.
    ///
    /// The one thing the arrangement could not do: it could move, resize,
    /// duplicate, delete, copy, cut, paste, mute and loop clips, and a press
    /// on empty grid always started a marquee. What channel the clip plays and
    /// how long it is are the host's to decide — a canvas may not see a
    /// `Project` (INVARIANT 2), and "the channel already on this lane" is a
    /// question only the document can answer.
    Add {
        lane: usize,
        start: Tick,
    },
    /// Make these clips repeat their content every `loop_length` ticks, or
    /// stop them repeating.
    ///
    /// **Not a duplicate.** A duplicate makes new clips with notes of their
    /// own; this is one clip whose one set of notes plays again every period,
    /// so editing bar 1 changes every repeat. See `fontelle_model::Clip`'s
    /// `loop_length`.
    SetLoop {
        ids: Vec<ClipId>,
        loop_length: Option<Tick>,
    },
}

/// How far a move may go before it would take a clip somewhere the document
/// would refuse, **measured once when the gesture started**.
///
/// The same type and the same reason as the roll's `MoveLimits`: a drag emits
/// deltas relative to its own previous step, so a clamp recomputed from the
/// live clips — which the drag is changing, and which the window refreshes
/// between events — chases the thing it is clamping. The clip reaches bar one,
/// the clamp becomes "cannot move at all", `wanted` snaps to zero against an
/// `applied` of minus four bars, and the clip is asked to jump four bars
/// forward. Then back. See `tests/arrange_gestures.rs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct MoveLimits {
    /// The furthest back in time the selection may go.
    min_tick: Tick,
    /// And the furthest up the lanes.
    min_lane: i32,
}

impl MoveLimits {
    fn of(selection: &[ClipId], clips: &[ClipInfo]) -> Self {
        let mut earliest = Tick::MAX;
        let mut highest = i32::MAX;
        for clip in clips.iter().filter(|c| selection.contains(&c.id)) {
            earliest = earliest.min(clip.start);
            highest = highest.min(clip.lane as i32);
        }
        if earliest == Tick::MAX {
            return Self::default();
        }
        Self {
            min_tick: -earliest,
            min_lane: -highest,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Gesture {
    None,
    Moving {
        applied_tick: Tick,
        applied_lane: i32,
        limits: MoveLimits,
    },
    Resizing {
        applied_tick: Tick,
        /// The shortest clip in the selection when the gesture started — same
        /// reason as [`MoveLimits`].
        shortest: Tick,
        /// Shift was held when the grip was taken, so this drag is making the
        /// clip **loop** rather than merely making it longer.
        ///
        /// Decided at the press and kept, not read live: a modifier let go
        /// halfway through a drag must not turn a loop back into a stretch
        /// under the pointer. The same rule the roll's gestures follow.
        looping: bool,
        /// The period to set, measured **when the drag started**: the length
        /// the clip already loops at, or the length it was. Kept for the same
        /// reason `shortest` is — the drag is changing the clip it would
        /// otherwise be reading.
        period: Tick,
    },
    Marquee {
        from: (f32, f32),
        to: (f32, f32),
    },
    /// Rubbing clips out: the right button held down. Stateless, for the same
    /// reason the roll's `Erasing` is — it asks the current clips what is
    /// under the pointer, so one already removed is simply not found again.
    Erasing,
}

/// What a press on empty grid means.
///
/// FL Studio's two, and the roll next door already works this way — so it is
/// one idea rather than two. **Draw is the default**, because the first thing
/// anybody wants from an empty arrangement is a clip, and a marquee over
/// nothing selects nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TimelineTool {
    /// A press on empty grid makes a clip.
    #[default]
    Draw,
    /// A press on empty grid marquees.
    Select,
}

/// The arrangement's own state: where it is looking, what is selected, what the
/// mouse is doing.
pub struct Timeline {
    pub view: TimelineView,
    tool: TimelineTool,
    selection: Vec<ClipId>,
    gesture: Gesture,
    /// What is held down. The window pushes these in rather than the canvas
    /// asking, because only the window sees a `ModifiersChanged` — the same
    /// shape `PianoRoll::set_modifiers` has.
    modifiers: Modifiers,
    /// Where the pointer was when the gesture started, in document units.
    origin: (Tick, usize),
    /// A clip the canvas wants the roll to open, waiting to be collected.
    ///
    /// Handed over rather than acted on, because opening a clip is the
    /// *document's* business (it changes which channel is selected) and this
    /// canvas may not touch the document (INVARIANT 2).
    open: Option<ClipId>,
}

impl Timeline {
    pub fn new(view: TimelineView) -> Self {
        Self {
            view,
            tool: TimelineTool::default(),
            selection: Vec::new(),
            gesture: Gesture::None,
            modifiers: Modifiers::default(),
            origin: (0, 0),
            open: None,
        }
    }

    pub fn selection(&self) -> &[ClipId] {
        &self.selection
    }

    pub fn select(&mut self, ids: Vec<ClipId>) {
        self.selection = ids;
    }

    pub fn clear_selection(&mut self) {
        self.selection.clear();
    }

    /// One step of an erase: whatever `hit` found, gone. Shared by the press
    /// and the drag so the rule is written once.
    fn erase_at(&mut self, hit: TimelineHit) -> Vec<ArrangeEdit> {
        match hit {
            TimelineHit::Clip(id, _) => {
                self.selection.retain(|s| *s != id);
                vec![ArrangeEdit::Remove(vec![id])]
            }
            _ => Vec::new(),
        }
    }

    /// The clip the arrangement wants opened in the roll, once.
    pub fn take_open(&mut self) -> Option<ClipId> {
        self.open.take()
    }

    /// The selection box being dragged, for drawing.
    pub fn marquee(&self) -> Option<Rect> {
        match self.gesture {
            Gesture::Marquee { from, to } => Some(box_between(from, to)),
            _ => None,
        }
    }

    pub fn press(
        &mut self,
        button: MouseButton,
        x: f32,
        y: f32,
        layout: &TimelineLayout,
        clips: &[ClipInfo],
        beats_per_bar: u32,
    ) -> Vec<ArrangeEdit> {
        let hit = timeline_hit(&self.view, layout, clips, x, y);

        // Right-click deletes, as it does in the roll — and keeps deleting
        // while the button is down. Note what it deliberately does *not* do:
        // set `self.open`. A clip being rubbed out is not a clip being opened
        // for editing, and a sweep across a row would otherwise leave the
        // piano roll showing the last thing it destroyed.
        if button == MouseButton::Right {
            self.gesture = Gesture::Erasing;
            return self.erase_at(hit);
        }

        match hit {
            TimelineHit::Clip(id, part) => {
                if !self.selection.contains(&id) {
                    self.selection = vec![id];
                }
                // Clicking a clip opens it: the roll and the arrangement are
                // two views of the same piece, and having to find the channel
                // in the rack to edit the clip you just pointed at is the
                // difference between two panels and one workflow.
                self.open = Some(id);
                self.origin = (
                    timeline_x_to_tick(&self.view, layout.grid, x),
                    y_to_lane(&self.view, layout.grid, y),
                );
                self.gesture = match part {
                    ClipPart::RightEdge => Gesture::Resizing {
                        applied_tick: 0,
                        shortest: self.shortest_selected(clips),
                        // Shift on the edge grip means **loop**, and it is
                        // decided here rather than read live: a modifier let
                        // go halfway through a drag must not turn a loop back
                        // into a stretch under the pointer.
                        looping: self.modifiers.shift,
                        period: self.loop_period(clips),
                    },
                    ClipPart::Body => Gesture::Moving {
                        applied_tick: 0,
                        applied_lane: 0,
                        limits: MoveLimits::of(&self.selection, clips),
                    },
                };
                Vec::new()
            }
            TimelineHit::Empty { tick, lane } => {
                self.selection.clear();
                // Ctrl is the marquee whichever tool is on — the same
                // modifier the roll uses for the same thing, so selecting a
                // few clips does not cost two trips to the toolbar.
                if self.tool == TimelineTool::Select || self.modifiers.ctrl {
                    self.gesture = Gesture::Marquee {
                        from: (x, y),
                        to: (x, y),
                    };
                    return Vec::new();
                }
                self.gesture = Gesture::None;
                // On the grid, not where the pointer was: a clip half a beat
                // off the bar is one somebody has to nudge before they can use
                // it, and the snap is right there in the toolbar saying what
                // it should have been.
                let start = timeline_snap(&self.view, tick, beats_per_bar).max(0);
                vec![ArrangeEdit::Add { lane, start }]
            }
            _ => {
                self.gesture = Gesture::None;
                let _ = beats_per_bar;
                Vec::new()
            }
        }
    }

    pub fn drag(
        &mut self,
        x: f32,
        y: f32,
        layout: &TimelineLayout,
        // Read by exactly one gesture, and it is worth saying which. A
        // *move* or *resize* must not look here: its limits are the ones its
        // press measured (see `MoveLimits`), and reaching for the live clips
        // — which the drag is itself changing — is the bug this canvas was
        // fixed for. An **erase** is the opposite case and the reason the
        // parameter was kept: it holds no state, so the live clips are the
        // only thing that can tell it what is still under the pointer.
        clips: &[ClipInfo],
        beats_per_bar: u32,
    ) -> Vec<ArrangeEdit> {
        let grid = layout.grid;
        // Clamped for the reason the roll clamps: a drag that strays off the
        // canvas means the nearest cell inside it, not a teleport.
        let (cx, cy) = clamp_to_grid(grid, x, y);
        let tick = timeline_x_to_tick(&self.view, grid, cx);
        let lane = y_to_lane(&self.view, grid, cy);

        match self.gesture.clone() {
            Gesture::None => Vec::new(),

            // The raw pointer, not the clamped one: an erase dragged off the
            // grid must find nothing rather than rubbing out whatever sits at
            // the edge it slid past. Same rule as the roll's.
            Gesture::Erasing => self.erase_at(timeline_hit(&self.view, layout, clips, x, y)),

            Gesture::Marquee { from, .. } => {
                self.gesture = Gesture::Marquee { from, to: (x, y) };
                Vec::new()
            }

            Gesture::Moving {
                applied_tick,
                applied_lane,
                limits,
            } => {
                if self.selection.is_empty() {
                    return Vec::new();
                }
                let unit = snap_unit(self.view.snap, beats_per_bar);
                let raw = tick - self.origin.0;
                let wanted_tick = if unit > 0 {
                    (raw as f64 / unit as f64).round() as Tick * unit
                } else {
                    raw
                };
                // Never before the start of the song, and never above the
                // first lane — against the limits this gesture *started* with.
                let wanted_tick = wanted_tick.max(limits.min_tick);
                let wanted_lane = (lane as i32 - self.origin.1 as i32).max(limits.min_lane);

                let d_tick = wanted_tick - applied_tick;
                let d_lane = wanted_lane - applied_lane;
                if d_tick == 0 && d_lane == 0 {
                    // No zero-delta edits: a sub-bar mouse move is not a
                    // history entry and not a timeline recompile.
                    return Vec::new();
                }
                self.gesture = Gesture::Moving {
                    applied_tick: wanted_tick,
                    applied_lane: wanted_lane,
                    limits,
                };
                vec![ArrangeEdit::Move {
                    ids: self.selection.clone(),
                    tick_delta: d_tick,
                    lane_delta: d_lane,
                }]
            }

            Gesture::Resizing {
                applied_tick,
                shortest,
                looping,
                period,
            } => {
                if self.selection.is_empty() {
                    return Vec::new();
                }
                let unit = snap_unit(self.view.snap, beats_per_bar);
                let raw = tick - self.origin.0;
                let wanted = if unit > 0 {
                    (raw as f64 / unit as f64).round() as Tick * unit
                } else {
                    raw
                };
                // Never past nothing: the shortest a clip may become is one
                // snap unit, measured against the length it had when the drag
                // started.
                let wanted = wanted.max(-(shortest - unit.max(1)).max(0));
                let delta = wanted - applied_tick;
                if delta == 0 {
                    return Vec::new();
                }
                let first = applied_tick == 0;
                self.gesture = Gesture::Resizing {
                    applied_tick: wanted,
                    shortest,
                    looping,
                    period,
                };
                let mut edits = Vec::new();
                // The loop is set once, on the first step of the drag, and the
                // clip then simply grows. Setting it every step would work and
                // would also mean a history entry per pixel that says nothing
                // new; `SetClipLoop::merge_with` would swallow them, but a
                // command that is only ever a no-op is a command not to send.
                if looping && first && period > 0 {
                    edits.push(ArrangeEdit::SetLoop {
                        ids: self.selection.clone(),
                        loop_length: Some(period),
                    });
                }
                edits.push(ArrangeEdit::Resize {
                    ids: self.selection.clone(),
                    tick_delta: delta,
                });
                edits
            }
        }
    }

    pub fn release(&mut self) {
        self.gesture = Gesture::None;
    }

    /// [`release`](Self::release), knowing where the button came up — which a
    /// marquee needs, because that is when it decides what it caught.
    pub fn release_over(&mut self, x: f32, y: f32, layout: &TimelineLayout, clips: &[ClipInfo]) {
        if let Gesture::Marquee { from, .. } = self.gesture {
            let box_ = box_between(from, (x, y));
            self.selection = clips
                .iter()
                .filter(|clip| clip_rect(&self.view, layout.grid, clip).intersects(&taut(box_)))
                .map(|clip| clip.id)
                .collect();
        }
        self.gesture = Gesture::None;
    }

    /// The shortest selected clip, which is what a resize is floored against.
    fn shortest_selected(&self, clips: &[ClipInfo]) -> Tick {
        self.selected(clips).map(|c| c.length).min().unwrap_or(1)
    }

    /// The period a Shift-drag would set: the one the selection **already**
    /// loops at, or the length it has.
    ///
    /// Already-looping wins, which is the whole of the rule people get wrong:
    /// dragging a one-bar loop out to eight bars must not make it an eight-bar
    /// loop. The period is the content, and stretching the window does not
    /// change the content.
    fn loop_period(&self, clips: &[ClipInfo]) -> Tick {
        self.selected(clips)
            .map(|clip| clip.loop_length.unwrap_or(clip.length))
            .min()
            .unwrap_or(0)
    }

    /// What is held down, from the window.
    pub fn set_modifiers(&mut self, modifiers: Modifiers) {
        self.modifiers = modifiers;
    }

    pub fn tool(&self) -> TimelineTool {
        self.tool
    }

    pub fn set_tool(&mut self, tool: TimelineTool) {
        self.tool = tool;
    }

    // ---------------------------------------------------- from the keyboard ---

    /// Moves the selection, the way a drag does but by a known amount.
    ///
    /// Clamped exactly as the drag is, and empty when there is nothing to move
    /// or nowhere left to move it — an arrow key at the end of its travel is
    /// not an undo entry. The roll's [`PianoRoll::nudge`] in every respect
    /// except that clips have lanes where notes have keys.
    ///
    /// [`PianoRoll::nudge`]: crate::canvas::PianoRoll::nudge
    pub fn nudge(
        &mut self,
        clips: &[ClipInfo],
        tick_delta: Tick,
        lane_delta: i32,
    ) -> Vec<ArrangeEdit> {
        if self.selection.is_empty() {
            return Vec::new();
        }
        let limits = MoveLimits::of(&self.selection, clips);
        let tick_delta = tick_delta.max(limits.min_tick);
        let lane_delta = lane_delta.max(limits.min_lane);
        if tick_delta == 0 && lane_delta == 0 {
            return Vec::new();
        }
        vec![ArrangeEdit::Move {
            ids: self.selection.clone(),
            tick_delta,
            lane_delta,
        }]
    }

    /// Lengthens or shortens the selection, floored at its shortest clip.
    pub fn resize_selection(&mut self, clips: &[ClipInfo], tick_delta: Tick) -> Vec<ArrangeEdit> {
        if self.selection.is_empty() {
            return Vec::new();
        }
        let tick_delta = tick_delta.max(-(self.shortest_selected(clips) - 1).max(0));
        if tick_delta == 0 {
            return Vec::new();
        }
        vec![ArrangeEdit::Resize {
            ids: self.selection.clone(),
            tick_delta,
        }]
    }

    /// What `Delete` should do.
    pub fn delete_selection(&mut self) -> Vec<ArrangeEdit> {
        if self.selection.is_empty() {
            // Not an empty removal — that would still be a history entry, and
            // an undo that puts nothing back is worse than a key that did
            // nothing.
            return Vec::new();
        }
        vec![ArrangeEdit::Remove(std::mem::take(&mut self.selection))]
    }

    /// `Ctrl+B`: the selection again, starting where it ends.
    ///
    /// Rounded up to the next bar, so duplicating a phrase produces a phrase
    /// twice as long rather than an overlap nobody asked for — the same rule
    /// the roll's duplicate follows.
    pub fn duplicate(&mut self, clips: &[ClipInfo], beats_per_bar: u32) -> Vec<ArrangeEdit> {
        if self.selection.is_empty() {
            return Vec::new();
        }
        let earliest = self.selected(clips).map(|c| c.start).min().unwrap_or(0);
        let end = self
            .selected(clips)
            .map(|c| c.start + c.length)
            .max()
            .unwrap_or(earliest);
        let bar = PPQN * Tick::from(beats_per_bar.max(1));
        let landing = (end + bar - 1) / bar * bar;
        let offset = (landing - earliest).max(bar);
        vec![ArrangeEdit::Duplicate {
            ids: self.selection.clone(),
            tick_offset: offset,
        }]
    }

    /// Steps the arrangement's snap to the next division.
    ///
    /// Its own, not the roll's: the two are different grids and a person sets
    /// them separately — bars on the arrangement while writing sixteenths in
    /// the roll is the ordinary case, not an edge one.
    pub fn cycle_snap(&mut self) {
        self.view.snap = self.view.snap.next();
    }

    /// The clips a duplicate or a paste just created, as the host reports
    /// them — they become the selection.
    ///
    /// The same handshake `PianoRoll::notes_inserted` has, and for a sharper
    /// reason: a duplicate's offset is measured from the selection, so leaving
    /// the selection on the *original* makes pressing the key twice put two
    /// copies at the same offset, on top of each other. Selecting the copy is
    /// what turns "duplicate" into "repeat".
    ///
    /// An empty list leaves the selection alone: a duplicate that created
    /// nothing must not clear it.
    pub fn clips_inserted(&mut self, ids: Vec<ClipId>) {
        if ids.is_empty() {
            return;
        }
        self.selection = ids;
    }

    /// `Ctrl+C`: keep the selection. See [`ArrangeEdit::Copy`] for why the
    /// clips go to the host rather than into this canvas.
    pub fn copy(&mut self) -> Vec<ArrangeEdit> {
        if self.selection.is_empty() {
            return Vec::new();
        }
        vec![ArrangeEdit::Copy(self.selection.clone())]
    }

    /// `Ctrl+X`: keep it, then take it away.
    ///
    /// In that order, and the order is the whole of it — removing first copies
    /// a clip that is already gone.
    pub fn cut(&mut self) -> Vec<ArrangeEdit> {
        if self.selection.is_empty() {
            return Vec::new();
        }
        let ids = std::mem::take(&mut self.selection);
        vec![ArrangeEdit::Copy(ids.clone()), ArrangeEdit::Remove(ids)]
    }

    /// `Ctrl+V`: the clipboard down at `at`, **snapped**.
    ///
    /// Snapped here for the reason `PianoRoll::paste` snaps there: `at` comes
    /// from the playhead or from the pointer, and neither of those is ever on
    /// a bar line by itself.
    pub fn paste(&mut self, at: Tick, beats_per_bar: u32) -> Vec<ArrangeEdit> {
        let at = timeline_snap(&self.view, at.max(0), beats_per_bar).max(0);
        vec![ArrangeEdit::Paste { at }]
    }

    /// The selection again, `times` more, each copy starting where the last
    /// one ends.
    ///
    /// Reported as *"I made this drum loop but I can't repeat it"*. One edit
    /// per copy rather than one edit meaning "n copies", because each copy's
    /// offset is different and a single edit carrying a count would have to
    /// re-derive them somewhere that cannot see the clips.
    pub fn repeat(
        &mut self,
        clips: &[ClipInfo],
        times: usize,
        beats_per_bar: u32,
    ) -> Vec<ArrangeEdit> {
        if self.selection.is_empty() || times == 0 {
            return Vec::new();
        }
        let earliest = self.selected(clips).map(|c| c.start).min().unwrap_or(0);
        let end = self
            .selected(clips)
            .map(|c| c.start + c.length)
            .max()
            .unwrap_or(earliest);
        // The span the selection occupies, rounded up to a bar so a loop that
        // is not quite a whole number of bars still repeats on the beat.
        let bar = PPQN * Tick::from(beats_per_bar.max(1));
        let span = (end - earliest).max(1);
        let stride = (span + bar - 1) / bar * bar;
        (1..=times as Tick)
            .map(|n| ArrangeEdit::Duplicate {
                ids: self.selection.clone(),
                tick_offset: stride * n,
            })
            .collect()
    }

    /// Mutes the selection, or unmutes it when all of it is already muted —
    /// one key for both, which is what a person means by "mute this".
    pub fn toggle_mute(&mut self, clips: &[ClipInfo]) -> Vec<ArrangeEdit> {
        if self.selection.is_empty() {
            return Vec::new();
        }
        let all_muted = self.selected(clips).all(|c| c.muted);
        vec![ArrangeEdit::SetMuted {
            ids: self.selection.clone(),
            muted: !all_muted,
        }]
    }

    fn selected<'a>(&'a self, clips: &'a [ClipInfo]) -> impl Iterator<Item = &'a ClipInfo> + 'a {
        clips.iter().filter(|c| self.selection.contains(&c.id))
    }
}

/// The rectangle two corners describe, whichever way round they came.
fn box_between(a: (f32, f32), b: (f32, f32)) -> Rect {
    Rect::new(
        a.0.min(b.0),
        a.1.min(b.1),
        (a.0 - b.0).abs(),
        (a.1 - b.1).abs(),
    )
}

/// A selection box with no zero dimension: a box dragged along one axis is a
/// line, and a line through a row of clips is an ordinary way to select them.
fn taut(box_: Rect) -> Rect {
    Rect::new(box_.x, box_.y, box_.width.max(1.0), box_.height.max(1.0))
}

/// Snaps a tick to the arrangement's grid — re-exported here so callers that
/// have a `TimelineView` do not have to reach into the roll for it.
pub fn timeline_snap(view: &TimelineView, tick: Tick, beats_per_bar: u32) -> Tick {
    snap_tick(tick, view.snap, beats_per_bar)
}

// -------------------------------------------------------- loop seams ---

/// How few pixels a repeat may be before the seams stop being drawn.
///
/// At one pixel per repeat a mark per seam is a filled rectangle, which reads
/// as a solid block — the exact opposite of what the marks are for. Below this
/// the clip is drawn plain and the loop is a fact you find by zooming in.
const MIN_REPEAT_PX: f32 = 6.0;

/// Where the seams between a looped clip's repeats fall, in screen x.
///
/// **Between** them, not around them: a line at the start and the end is a
/// border, and a block with a border is exactly the copy-and-paste picture
/// looping is meant to stop looking like.
///
/// Empty for a clip that does not loop, and for one zoomed out past legibility.
pub fn loop_marks(view: &TimelineView, grid: Rect, clip: &ClipInfo) -> Vec<f32> {
    let Some(period) = clip.loop_length.filter(|p| *p > 0) else {
        return Vec::new();
    };
    let step = period as f32 * view.pixels_per_tick;
    if step < MIN_REPEAT_PX {
        return Vec::new();
    }
    let block = clip_rect(view, grid, clip);
    if block.is_empty() {
        return Vec::new();
    }

    let mut marks = Vec::new();
    let mut tick = period;
    while tick < clip.length {
        let x = block.x + tick as f32 * view.pixels_per_tick;
        // Clipped to the block rather than to the grid: a seam scrolled off
        // the left is not drawn, and the renderer clips the rest.
        if x > block.x && x < block.right() {
            marks.push(x);
        }
        tick += period;
    }
    marks
}
