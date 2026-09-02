//! Where things go, as arithmetic.
//!
//! `docs/first-usable-plan.md` §2.5: everything that can be a pure function is
//! one, and the tests live there. Window geometry is the easiest thing in a GUI
//! to get subtly wrong and the easiest to test, so none of it happens inside an
//! event loop.
//!
//! Everything here is in **logical** pixels. The surface is in physical ones;
//! [`Rect::scale`] is the single place that conversion happens.

use crate::theme::Metrics;

/// An axis-aligned rectangle.
///
/// Half-open: a point on the right or bottom edge is *outside*. That is what
/// lets two abutting rectangles tile a surface without both claiming the seam,
/// which matters because dirty regions are unioned and redrawn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    /// A rectangle with nothing in it. The identity of [`union`](Rect::union).
    pub const ZERO: Rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 0.0,
        height: 0.0,
    };

    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn right(&self) -> f32 {
        self.x + self.width
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.height
    }

    /// True when there is nothing to draw in it. A rectangle with a negative
    /// dimension counts as empty rather than as an error — see
    /// [`Rect::clamped`].
    pub fn is_empty(&self) -> bool {
        self.width <= 0.0 || self.height <= 0.0
    }

    pub fn contains(&self, x: f32, y: f32) -> bool {
        !self.is_empty() && x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }

    pub fn intersects(&self, other: &Rect) -> bool {
        !self.is_empty()
            && !other.is_empty()
            && self.x < other.right()
            && other.x < self.right()
            && self.y < other.bottom()
            && other.y < self.bottom()
    }

    /// The smallest rectangle covering both. An empty operand is ignored, so
    /// unioning into `ZERO` accumulates rather than dragging the result back to
    /// the origin.
    pub fn union(&self, other: &Rect) -> Rect {
        if self.is_empty() {
            return *other;
        }
        if other.is_empty() {
            return *self;
        }
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        Rect::new(
            x,
            y,
            self.right().max(other.right()) - x,
            self.bottom().max(other.bottom()) - y,
        )
    }

    /// The overlap, or an empty rectangle when there is none.
    pub fn intersection(&self, other: &Rect) -> Rect {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        Rect::new(
            x,
            y,
            (self.right().min(other.right()) - x).max(0.0),
            (self.bottom().min(other.bottom()) - y).max(0.0),
        )
    }

    /// Shrunk by `by` on every side, never past nothing.
    pub fn inset(&self, by: f32) -> Rect {
        Rect::new(
            self.x + by,
            self.y + by,
            self.width - 2.0 * by,
            self.height - 2.0 * by,
        )
        .clamped()
    }

    /// Logical pixels to physical ones.
    pub fn scale(&self, factor: f32) -> Rect {
        Rect::new(
            self.x * factor,
            self.y * factor,
            self.width * factor,
            self.height * factor,
        )
    }

    /// Replaces a negative dimension with zero.
    ///
    /// A user dragging a window edge past the chrome is ordinary; a negative
    /// width reaching the GPU is a panic or a garbage draw. Every constructor
    /// here that can subtract goes through this.
    pub fn clamped(&self) -> Rect {
        Rect::new(self.x, self.y, self.width.max(0.0), self.height.max(0.0))
    }

    /// The top `height` of this rectangle, and what is left below it.
    pub fn split_top(&self, height: f32) -> (Rect, Rect) {
        let height = height.clamp(0.0, self.height.max(0.0));
        (
            Rect::new(self.x, self.y, self.width, height).clamped(),
            Rect::new(self.x, self.y + height, self.width, self.height - height).clamped(),
        )
    }
}

/// One docked panel: its frame, the strip carrying its name, and the area its
/// contents get.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PanelLayout {
    /// The whole panel, header included — what gets the background and border.
    pub frame: Rect,
    pub header: Rect,
    /// Where the panel's contents draw, already inset by the padding.
    pub body: Rect,
}

/// The window, and what is in it (TDD §16.1).
///
/// A transport bar across the top, a sidebar down the left carrying the channel
/// rack over the soundfont browser, and the piano roll taking everything that
/// is left. Item 9 of `docs/first-usable-plan.md`; §16.1's resizable, saveable
/// dock layouts are what this grows into, and the shape is already the shape
/// they will need — every panel is a `PanelLayout` computed from one rectangle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WindowLayout {
    pub window: Rect,
    /// The strip carrying play/stop, the playhead and the master meter. See
    /// [`crate::transport::transport_bar_layout`] for what is inside it.
    pub transport: Rect,
    /// The channel rack: what is in the project and what it is playing.
    pub rack: PanelLayout,
    /// The soundfont bank (TDD §17.5).
    pub browser: PanelLayout,
    /// The arrangement: clips as blocks on lanes, across the top of the editor
    /// column. Empty when it is hidden, and then the editor has the room.
    pub timeline: PanelLayout,
    /// The seam between the arrangement and the editor — the thing you drag to
    /// decide which of them gets the room. Empty when the arrangement is
    /// hidden; there is no seam.
    pub divider: Rect,
    /// The seam down the right of the sidebar: drag it to decide how much of
    /// the window the rack and browser get. Empty when the window is too
    /// narrow to have a sidebar at all — a seam with nothing on one side of it
    /// is a strip that eats clicks.
    pub sidebar_seam: Rect,
    /// And the seam across the sidebar, between the rack and the browser.
    pub sidebar_split: Rect,
    /// The editor: the piano roll, or the instrument the rack has open.
    pub panel: PanelLayout,
}

/// How tall the arrangement is until somebody drags it.
pub const DEFAULT_TIMELINE_HEIGHT: f32 = 200.0;

/// The shortest the arrangement may be dragged before it is simply hidden.
pub const MIN_TIMELINE_HEIGHT: f32 = 80.0;

/// The editor is what the window is for; the arrangement may not squeeze it
/// past this.
pub const MIN_EDITOR_HEIGHT: f32 = 220.0;

/// How tall the drag target on the seam is.
const DIVIDER: f32 = 6.0;

/// The arrangement height a drag of the divider to `y` is asking for.
///
/// Its own function, and pure, for the reason
/// [`crate::canvas::lane_height_at`] is: "the arrangement cannot swallow the
/// editor and cannot be dragged negative" is exactly the rule that gets written
/// once inside an event handler and then not held.
pub fn timeline_height_at(layout: &WindowLayout, y: f32) -> f32 {
    let column = layout
        .timeline
        .frame
        .union(&layout.divider)
        .union(&layout.panel.frame);
    if column.is_empty() {
        return 0.0;
    }
    let ceiling = (column.height - MIN_EDITOR_HEIGHT - DIVIDER).max(0.0);
    let wanted = (y - column.y).max(0.0);
    // Below the minimum it snaps shut rather than becoming a sliver that shows
    // nothing and cannot be read.
    if wanted < MIN_TIMELINE_HEIGHT / 2.0 {
        return 0.0;
    }
    wanted.clamp(MIN_TIMELINE_HEIGHT.min(ceiling), ceiling)
}

/// How much of the sidebar's height the channel rack gets **until somebody
/// drags the seam**.
///
/// The rack is a list of names; the browser is a search box over two lists, and
/// wants the room. Below this the browser stops being usable before the rack
/// does, which is why the split is not even — and why it is only a default.
pub const RACK_SHARE: f32 = 0.42;

/// The narrowest the sidebar is allowed to squeeze the roll to before it gives
/// up its own width instead.
///
/// The roll is what the window is *for*. A sidebar that keeps its width on a
/// small screen leaves a piano roll two bars wide, which is worse for everyone
/// than a browser that has to be scrolled sideways.
pub const MIN_ROLL_WIDTH: f32 = 320.0;

/// The narrowest the sidebar may be dragged.
///
/// A soundfont's name is the widest thing in it, and a column too narrow to
/// read one in is not a column — it is a strip you have to widen again before
/// you can use it.
pub const MIN_SIDEBAR_WIDTH: f32 = 160.0;

/// The shortest the channel rack may be dragged: its header and a row.
pub const MIN_RACK_HEIGHT: f32 = 64.0;

/// And the browser, which needs its header, its search box and something to
/// search through before it is a browser rather than a caption.
pub const MIN_BROWSER_HEIGHT: f32 = 120.0;

/// How big the window's panels are, as far as the user has said.
///
/// Reported from using the window: *"I want to be able to make the soundfonts
/// bigger than the channels, but the program doesn't let me right now."* It
/// did not, because all three of these were constants. They are still the
/// defaults — `None` means "whatever the theme and this file say" — and none of
/// them is a decision this program gets to keep making for somebody who is
/// looking at it all day.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Docks {
    /// How tall the arrangement is. **Zero hides it.**
    pub timeline_height: f32,
    /// How wide the sidebar is. `None` until it is dragged, and then the
    /// theme's `sidebar_width`.
    pub sidebar_width: Option<f32>,
    /// How much of the sidebar the rack gets, 0..=1. `None` until it is
    /// dragged, and then [`RACK_SHARE`].
    pub rack_share: Option<f32>,
}

impl Default for Docks {
    fn default() -> Self {
        Self {
            timeline_height: DEFAULT_TIMELINE_HEIGHT,
            sidebar_width: None,
            rack_share: None,
        }
    }
}

/// Lays out a window of `width` x `height` logical pixels, giving the
/// arrangement `timeline_height` of the editor column. **Zero hides it.**
///
/// The sidebar keeps the theme's width and the rack keeps [`RACK_SHARE`]; see
/// [`window_layout_with`] for the form a window that has been dragged about
/// uses.
pub fn window_layout(
    width: f32,
    height: f32,
    metrics: &Metrics,
    timeline_height: f32,
) -> WindowLayout {
    window_layout_with(
        width,
        height,
        metrics,
        &Docks {
            timeline_height,
            ..Docks::default()
        },
    )
}

/// The width a drag of the sidebar's seam to `x` is asking for, clamped to
/// what a sidebar may be.
///
/// Its own function, and pure, for the reason [`timeline_height_at`] is: the
/// rule that the sidebar can neither be dragged shut nor eat the piano roll is
/// exactly the kind that gets written once inside an event handler and then not
/// held.
pub fn sidebar_width_at(layout: &WindowLayout, x: f32) -> f32 {
    let column = layout.rack.frame.union(&layout.browser.frame);
    let editor = layout
        .timeline
        .frame
        .union(&layout.divider)
        .union(&layout.panel.frame);
    if column.is_empty() {
        return MIN_SIDEBAR_WIDTH;
    }
    // Everything the sidebar, the seam and the editor share, which is what
    // decides how much of it the sidebar may have.
    let across = editor.right() - column.x;
    let seam = layout.sidebar_seam.width;
    let ceiling = (across - MIN_ROLL_WIDTH - seam).max(0.0);
    (x - column.x).clamp(MIN_SIDEBAR_WIDTH.min(ceiling), ceiling)
}

/// The share of the sidebar a drag of its split to `y` gives the rack, 0..=1.
///
/// A share rather than a height so that resizing the window keeps the
/// proportion somebody chose instead of quietly giving one of them everything.
pub fn rack_share_at(layout: &WindowLayout, y: f32) -> f32 {
    let column = layout
        .rack
        .frame
        .union(&layout.sidebar_split)
        .union(&layout.browser.frame);
    let available = column.height - layout.sidebar_split.height;
    if available <= 0.0 {
        return RACK_SHARE;
    }
    clamped_rack_height(y - column.y, available) / available
}

/// The rack height `wanted` becomes once neither panel is allowed to disappear.
///
/// Shared by the layout and by [`rack_share_at`] so that a drag round-trips:
/// the share the seam reports is the share the layout then produces, which is
/// what stops the seam drifting away from the pointer at the ends of its
/// travel.
fn clamped_rack_height(wanted: f32, available: f32) -> f32 {
    let ceiling = (available - MIN_BROWSER_HEIGHT).max(0.0);
    let floor = MIN_RACK_HEIGHT.min(ceiling);
    wanted.clamp(floor, ceiling.max(floor))
}

/// [`window_layout`], with every panel size the user has chosen.
pub fn window_layout_with(
    width: f32,
    height: f32,
    metrics: &Metrics,
    docks: &Docks,
) -> WindowLayout {
    let timeline_height = docks.timeline_height;
    let window = Rect::new(0.0, 0.0, width, height).clamped();
    let content = window.inset(metrics.panel_margin);

    let (transport, below_bar) = content.split_top(metrics.transport_bar_height);
    // The same gap between the bar and the panels as between them and the
    // window edge, so the chrome reads as evenly spaced rather than as a bar
    // with panels stuck to it.
    let (_gap, rest) = below_bar.split_top(metrics.panel_margin);

    // The sidebar yields before the roll does, and disappears entirely rather
    // than becoming a column too narrow to read.
    let room_for_sidebar = (rest.width - MIN_ROLL_WIDTH - metrics.panel_margin).max(0.0);
    let wanted = docks.sidebar_width.unwrap_or(metrics.sidebar_width);
    let sidebar_width = if rest.width <= metrics.panel_margin || room_for_sidebar <= 0.0 {
        0.0
    } else {
        wanted
            .max(MIN_SIDEBAR_WIDTH.min(room_for_sidebar))
            .min(room_for_sidebar)
            .max(0.0)
    };

    let sidebar = Rect::new(rest.x, rest.y, sidebar_width, rest.height).clamped();
    let roll_x = if sidebar.is_empty() {
        rest.x
    } else {
        sidebar.right() + metrics.panel_margin
    };
    // The gap the margin already left, named so it can be dragged. Nothing
    // moves to make room for it, which is why it can be added to a layout
    // people are already used to without anything shifting under them.
    let sidebar_seam = if sidebar.is_empty() {
        Rect::ZERO
    } else {
        Rect::new(
            sidebar.right(),
            sidebar.y,
            metrics.panel_margin,
            sidebar.height,
        )
        .clamped()
    };
    let column = Rect::new(roll_x, rest.y, rest.right() - roll_x, rest.height).clamped();

    // The arrangement comes off the top of the editor column, and never at the
    // cost of the editor having somewhere to be.
    let wanted = if timeline_height > 0.0 {
        timeline_height
            .max(MIN_TIMELINE_HEIGHT)
            .min((column.height - MIN_EDITOR_HEIGHT - DIVIDER).max(0.0))
    } else {
        0.0
    };
    let (timeline_frame, under) = column.split_top(wanted);
    let (divider, roll) = if timeline_frame.is_empty() {
        (Rect::ZERO, under)
    } else {
        under.split_top(DIVIDER.min(under.height.max(0.0)))
    };

    // The rack over the browser, with the same margin between them — and that
    // margin is the second seam.
    let available = (sidebar.height - metrics.panel_margin).max(0.0);
    let share = docks.rack_share.unwrap_or(RACK_SHARE).clamp(0.0, 1.0);
    let rack_height = clamped_rack_height(available * share, available);
    let (rack_frame, under) = sidebar.split_top(rack_height);
    let (gap, browser_frame) = under.split_top(metrics.panel_margin);
    let sidebar_split = if sidebar.is_empty() || browser_frame.is_empty() {
        Rect::ZERO
    } else {
        gap
    };

    WindowLayout {
        window,
        transport,
        rack: panel(rack_frame, metrics),
        browser: panel(browser_frame, metrics),
        timeline: panel(timeline_frame, metrics),
        divider,
        sidebar_seam,
        sidebar_split,
        panel: panel(roll, metrics),
    }
}

/// A frame split into its header and the body under it.
fn panel(frame: Rect, metrics: &Metrics) -> PanelLayout {
    let (header, below) = frame.split_top(metrics.panel_header_height);
    PanelLayout {
        frame,
        header,
        body: below.inset(metrics.panel_padding),
    }
}

/// Which of the editor column's two views is showing.
///
/// **Two, and only two.** An instrument, an effect and an automation curve
/// were all tabs here once, and that was wrong for a reason that has nothing
/// to do with taste: §7.5 hosts third-party plugins, and a VST or a CLAP
/// editor expects a *window* — it is handed a parent handle and draws into it.
/// A thing that can only exist as one of this column's tabs is a thing the
/// plugin path can never be built on. So the two views that are genuinely
/// *the document* stay here, and everything that edits one device opens as a
/// window of its own. See [`EditorKind`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorTab {
    /// The piano roll: the notes of one clip.
    Roll,
    /// The mixer: a fader, a pan and two switches per mixer track (TDD §13).
    Mixer,
}

impl EditorTab {
    /// What a hover tip says (see [`crate::tooltip`]).
    pub fn tip(self) -> Option<&'static str> {
        Some(match self {
            Self::Roll => "The notes of the open clip",
            Self::Mixer => "Levels, effects and routing",
        })
    }
}

/// What a floating editor window has open (TDD §7.2, §12, §13.4).
///
/// One window each, and at most one of each open at a time — which is not a
/// simplification but the shape of what is behind them: the host answers
/// "the selected channel's instrument", "the open insert's parameters" and
/// "the open automation clip", each of which is one thing. A second window of
/// the same kind would be a second view of the same state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EditorKind {
    /// The parameters of one channel's soundfont player (TDD §7.2) — the
    /// window a third-party instrument will one day draw into instead.
    Instrument,
    /// One insert's parameters: the EQ's curve, the compressor's knobs
    /// (TDD §13.4).
    Effect,
    /// An automation clip's points (TDD §12).
    Automation,
}

impl EditorKind {
    /// What its title bar says, before the name of whatever it has open.
    pub fn title(self) -> &'static str {
        match self {
            Self::Instrument => "Instrument",
            Self::Effect => "Effect",
            Self::Automation => "Automation",
        }
    }

    /// How big it opens, in logical pixels.
    ///
    /// Wide enough for the panel each one draws and no wider: a window that
    /// opens larger than its contents is one you have to resize before you can
    /// see the thing beside it, which is the whole reason these are windows.
    pub fn default_size(self) -> (u32, u32) {
        match self {
            // A grid of knobs. **Tall**, because it is a column of sections —
            // channel, voice, both filters, the envelopes — and a window that
            // opens shorter than its own contents is one you have to resize
            // before you can read it. Narrow for the same reason a plugin
            // editor is: it goes beside the thing it is editing.
            Self::Instrument => (620, 760),
            // A curve needs width far more than it needs height.
            Self::Effect => (720, 420),
            Self::Automation => (720, 360),
        }
    }

    /// The smallest it may be dragged to. Under this the panel inside has no
    /// room for a control, and a window you can shrink into uselessness is one
    /// somebody will.
    pub fn minimum_size(self) -> (u32, u32) {
        (320, 200)
    }
}

/// The insides of a floating editor window: a header carrying its name, and
/// the body the panel is drawn in.
///
/// The same shape as a docked panel's, deliberately — the panels drawn into
/// these windows are the ones that were drawn into the editor column, and they
/// take a body rectangle either way.
pub fn editor_window_layout(width: f32, height: f32, metrics: &Metrics) -> PanelLayout {
    panel(Rect::new(0.0, 0.0, width.max(0.0), height.max(0.0)), metrics)
}

/// Where the editor column's tabs are, in its panel header.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EditorTabs {
    pub roll: Rect,
    pub mixer: Rect,
}

/// How wide each tab is. Fixed rather than measured, so the header does not
/// jump about as the selected channel's name changes length.
const TAB_WIDTH: f32 = 96.0;
const TAB_GAP: f32 = 2.0;

/// Lays the two tabs out at the **right-hand end** of the header.
///
/// The left-hand end already carries the panel's title, which is the name of
/// the project; putting the tabs against it would read as one long caption.
pub fn editor_tabs(header: Rect, metrics: &Metrics) -> EditorTabs {
    let inset = (header.height * 0.15).min(4.0);
    let height = (header.height - inset * 2.0).max(0.0);
    let y = header.y + inset;
    let right = header.right() - metrics.panel_padding.min(header.width);

    // Laid out from the right, in the order they are *used*: the roll first
    // because it is where the notes are, then the mixer that balances them.
    let mut x = right;
    let mut take = || {
        x -= TAB_WIDTH;
        let tab = Rect::new(x, y, TAB_WIDTH, height).intersection(&header);
        x -= TAB_GAP;
        tab
    };
    let mixer = take();
    let roll = take();
    EditorTabs { roll, mixer }
}

/// Which tab is under the pointer.
pub fn editor_tab_at(tabs: &EditorTabs, x: f32, y: f32) -> Option<EditorTab> {
    if tabs.roll.contains(x, y) {
        return Some(EditorTab::Roll);
    }
    if tabs.mixer.contains(x, y) {
        return Some(EditorTab::Mixer);
    }
    None
}
