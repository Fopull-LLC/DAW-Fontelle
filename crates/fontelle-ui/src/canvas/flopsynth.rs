//! Flopsynth's window (`docs/flopsynth-plan.md` §8).
//!
//! > *"we need to ensure that this has a really polished, clean, and pretty
//! > user interface. it should be organized, have actual design, flow cleanly
//! > for the users eyes while also having tons of complexity to allow for
//! > massive configuration."* — Ty, 2026-09-06
//!
//! # Why this is not the generic knob grid
//!
//! [`instrument_layout`](super::instrument_layout) draws a **list**: headed
//! rows of cells, in order, wrapping. That is right for a soundfont player,
//! whose twenty controls have no shape between them. A synthesiser is a
//! **picture of a signal path** — sources at the top, what they go through
//! under them, what moves those under that — and a list cannot say so.
//!
//! So Flopsynth's window is cards. Each card is one box of the block diagram,
//! laid out left to right and top to bottom in the order the sound is made
//! (§8.1 rule 1), and most of them carry a **picture** computed from the same
//! numbers the voice reads (rule 5): a picture that lies is believed.
//!
//! # Everything here is pure
//!
//! Geometry and arithmetic, no drawing and no state, for the reason §2.5
//! gives — so what appears on screen is decided by functions a test can call,
//! and the renderer has nothing left to get wrong but colours.

use fontelle_types::LfoWave;

use crate::layout::Rect;
use crate::theme::Metrics;

use super::instrument::{InstrumentGroup, InstrumentParam, ParamKind};
use super::preset_bar::PresetChoice;

/// The strip across the top of a card carrying its name.
pub const CARD_HEADER: f32 = 18.0;
/// Between a card's frame and what is inside it.
pub const CARD_PAD: f32 = 6.0;
/// Between one card and the next.
pub const CARD_GAP: f32 = 8.0;
/// One control's cell inside a card, at the size the window opens at
/// (`docs/flopsynth-next.md` §3.1, principle 11: three sizes of everything).
///
/// A **full** cell holds a Large or Medium knob with its caption above and
/// its read-out below; a **half** cell — [`FLOP_CELL_HALF`] tall, two to a
/// column — holds a Small knob, a chooser or a switch, which is what lets a
/// card with seventeen controls stay three rows tall with 44-pixel knobs
/// in it. The layout stacks halves two to a column and places the full
/// cells first (`wanted`), so the knob a player reaches for first is the
/// first thing on the card.
///
/// **Design sizes.** Multiplied by the window's scale
/// (`FlopsynthView::scale`) and by nothing else: the shrink cascade of
/// v0.9.0 is gone, and a window too small for the page at its scale is one
/// the window refuses (`layout::flopsynth_window_size`). What a card is
/// drawn at is read off its cells, never off these.
pub const FLOP_CELL_W: f32 = 56.0;
pub const FLOP_CELL_H: f32 = 72.0;
pub const FLOP_CELL_HALF: f32 = 36.0;
/// The three knobs, at the design size (§3.1). Large is forty rather than
/// §3.1's forty-four: with a caption over it, a read-out under it and the
/// modulation ring clear of both, forty is what a 72-pixel cell holds
/// (`cell_anatomy`).
pub const KNOB_LARGE: f32 = 40.0;
pub const KNOB_MEDIUM: f32 = 32.0;
pub const KNOB_SMALL: f32 = 20.0;
/// How tall a card's picture is. Sixty rather than §3.1's seventy-two,
/// because the fit decided: two bands of cards, the canopy and the strip at
/// 1180×840 leave sixty (`the_whole_synth_page_fits_at_every_scale`).
pub const PICTURE_HEIGHT: f32 = 60.0;
/// The canopy — the instrument's eyes (§3.2) — on every page, at the
/// design size. Fixed: a taller window is more air under the consoles, not
/// a taller sky, so the scope and the spectrum are always the same picture.
pub const CANOPY_HEIGHT: f32 = 120.0;
/// The scales the window offers (§3.2). One of these multiplies every
/// design size above; the window opens at `FLOPSYNTH_SIZE` times it.
pub const SCALES: [f32; 4] = [0.75, 1.0, 1.25, 1.5];

/// How big a knob is drawn, declared per control by the card
/// (`FlopsynthCard::sizes`) — a fact about what the control *is*: the one
/// knob per card a player reaches for first is Large, everything continuous
/// is Medium, and the fine adjustments (fine, phase, pan of a sub) are
/// Small and take half a cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum KnobSize {
    Large,
    #[default]
    Medium,
    Small,
}

impl KnobSize {
    pub fn pixels(self) -> f32 {
        match self {
            Self::Large => KNOB_LARGE,
            Self::Medium => KNOB_MEDIUM,
            Self::Small => KNOB_SMALL,
        }
    }
}

/// The grid a window's cards are laid out on: the cell, whether it has a
/// half, the picture and its floor. Two windows share the card machinery
/// — Flopsynth's and the corrector's console (`docs/tune-plan.md` §7.2) —
/// and the corrector keeps the old grid and its shrink cascade: it was
/// measured at those sizes and nothing in the plan for the synth's window
/// is about it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Grid {
    pub cell_w: f32,
    pub cell_h: f32,
    /// A half cell's height, or `None` for a grid where every control takes
    /// a whole cell.
    pub half_h: Option<f32>,
    pub picture: f32,
}

/// Flopsynth's window.
pub const FLOP_GRID: Grid = Grid {
    cell_w: FLOP_CELL_W,
    cell_h: FLOP_CELL_H,
    half_h: Some(FLOP_CELL_HALF),
    picture: PICTURE_HEIGHT,
};

/// The corrector's console: the grid the window had before §3.1, whole
/// cells only, shrunk by [`fit_cards`] when the page will not fit.
pub const TUNE_GRID: Grid = Grid {
    cell_w: 52.0,
    cell_h: 54.0,
    half_h: None,
    picture: 48.0,
};
/// The corrector's floors (`fit_cards`): the picture gives to its soft
/// floor first, then every cell together down to [`CELL_FLOOR`], then the
/// picture to [`PICTURE_FLOOR`].
pub const PICTURE_FLOOR: f32 = 26.0;
const PICTURE_SOFT_FLOOR: f32 = 36.0;
pub const CELL_FLOOR: f32 = 0.8;

/// What a card draws above its controls.
///
/// Computed from the same numbers the voice reads, never from a second
/// description of them — `effect.rs`'s argument for the EQ curve, which is
/// that a picture that lies is believed.
#[derive(Debug, Clone, PartialEq)]
pub enum FlopsynthPicture {
    /// Nothing: the card is controls alone. The macros' card and the voice's
    /// are like this, and they get the space back.
    None,
    /// One cycle of the oscillator's **current frame**, in −1..=1, and where
    /// the position knob sits. Dragged sideways to move the position, so the
    /// picture changes under the hand.
    Wave { points: Vec<f32>, position: f32 },
    /// The filter's magnitude response in dB over the EQ's log axis, plus
    /// where its corner and its resonance are, as 0..1.
    Response {
        points: Vec<f32>,
        cutoff: f32,
        resonance: f32,
    },
    /// An envelope, as its four normalised stages — drawn on a square-root
    /// time axis so a 5 ms attack and a 2 s release are both visible.
    Envelope {
        attack: f32,
        decay: f32,
        sustain: f32,
        release: f32,
    },
    /// A **recording** — a sample source's sound, whole: its shape as one
    /// `(low, high)` pair per column, in −1..=1, where the note starts in it
    /// (the position knob, 0..1) and the loop when there is one, as 0..1
    /// fractions. `peaks` is empty when nothing has been dropped on the
    /// oscillator yet, and `name` then says what to do about it.
    ///
    /// Dragged sideways like a wave, to move the start.
    Sound {
        peaks: Vec<(f32, f32)>,
        start: f32,
        loop_region: Option<(f32, f32)>,
        name: String,
    },
    /// A **string** — its partials as bars, each at `(x, height)`: `x` is
    /// the partial's frequency as a multiple of the fundamental over
    /// `0..=harmonics`, so a stiff string's bars stand visibly sharp of the
    /// harmonic grid and further sharp going up, and `height` is its level
    /// at the strike, 0..1. The picture is the one thing about a string a
    /// table cannot be, drawn.
    ///
    /// Dragged sideways to move the brightness.
    Partials {
        bars: Vec<(f32, f32)>,
        harmonics: usize,
    },
    /// One cycle of an LFO's shape, in −1..=1, and where the **newest voice**
    /// is in it, 0..1.
    ///
    /// The shape says what the LFO is; the dot going round says what it is
    /// doing. The newest voice's, because the LFOs are per voice and
    /// retriggered per note — a chord has four of each at four offsets, and
    /// the newest is the one somebody just played.
    Lfo { points: Vec<f32>, phase: f32 },
}

impl FlopsynthPicture {
    pub fn is_none(&self) -> bool {
        matches!(self, Self::None)
    }
}

/// Which page of the window is showing (§8.3–§8.6).
///
/// Four rather than one long scroll, because §8.1 rule 7 says the window does
/// not scroll: what does not fit is on another page. The split is by *what the
/// controls are about* — making the sound, moving it, the chain after it, and
/// the bank — which is also the order somebody works in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FlopsynthPage {
    #[default]
    Synth,
    Modulation,
    Effects,
    Presets,
}

impl FlopsynthPage {
    pub const ALL: [Self; 4] = [Self::Synth, Self::Modulation, Self::Effects, Self::Presets];

    pub fn label(self) -> &'static str {
        match self {
            Self::Synth => "Synth",
            // The Matrix page (§3.4): the inspector and the full table. The
            // variant keeps its name — the window's state and the tests know
            // the page by it.
            Self::Modulation => "Matrix",
            Self::Effects => "Effects",
            Self::Presets => "Presets",
        }
    }
}

/// One row of the matrix, as the window shows it.
///
/// Labels rather than the matrix's own types, for INVARIANT 4's reason: this
/// crate may not see a `ModSource`. The host turns them into words and back.
#[derive(Default, Debug, Clone, PartialEq)]
pub struct FlopsynthRoute {
    pub source: String,
    pub destination: String,
    /// Bipolar, -1..=1.
    pub depth: f32,
    /// The source scaling this route's depth, if one does (§3.4 "via").
    pub via: Option<String>,
    /// The curve's name, one of the view's `curves`.
    pub curve: String,
    /// `1 - source` in place of the source.
    pub invert: bool,
    /// Kept and not heard.
    pub bypass: bool,
}

/// Where one matrix row's parts are.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MatrixRow {
    pub frame: Rect,
    /// The grip at the left end, to drag the row by (§3.4).
    pub grip: Rect,
    /// The source and destination choosers, left to right.
    pub source: Rect,
    pub destination: Rect,
    /// A **slider**, not a knob: the row is 22 pixels tall, which is the audio
    /// editor's argument for the same shape.
    pub depth: Rect,
    /// The via chooser, the curve chooser, the invert and on/off switches.
    pub via: Rect,
    pub curve: Rect,
    pub invert: Rect,
    pub bypass: Rect,
    pub remove: Rect,
}

/// The table's header: a head over each column — the source's and the
/// destination's sort the rows — and the `+` that adds one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MatrixHeader {
    pub source: Rect,
    pub destination: Rect,
    pub depth: Rect,
    pub via: Rect,
    pub curve: Rect,
    pub invert: Rect,
    pub bypass: Rect,
    pub add: Rect,
}

impl Default for MatrixHeader {
    fn default() -> Self {
        Self {
            source: Rect::ZERO,
            destination: Rect::ZERO,
            depth: Rect::ZERO,
            via: Rect::ZERO,
            curve: Rect::ZERO,
            invert: Rect::ZERO,
            bypass: Rect::ZERO,
            add: Rect::ZERO,
        }
    }
}

/// What a press in the matrix landed on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatrixHit {
    Grip(usize),
    Source(usize),
    Destination(usize),
    Depth(usize),
    Via(usize),
    Curve(usize),
    Invert(usize),
    Bypass(usize),
    Remove(usize),
    /// The `+` on the header.
    Add,
    /// The source head: sort the rows by source.
    SortSource,
    /// The destination head: sort by destination.
    SortDestination,
}

/// The table's columns at scale 1 (§3.4), left to right: what does not
/// flex. The destination takes what is left, because it has the longest
/// names ("Env 1 release time").
const MATRIX_GRIP: f32 = 12.0;
const MATRIX_SOURCE: f32 = 128.0;
const MATRIX_DEPTH: f32 = 110.0;
const MATRIX_VIA: f32 = 110.0;
const MATRIX_CURVE: f32 = 104.0;
const MATRIX_SWITCH: f32 = 40.0;
const MATRIX_REMOVE: f32 = 18.0;
const MATRIX_COL_GAP: f32 = 6.0;
/// The `+` on the header: a round chip over the ✕ column, no wider, so
/// the ON head beside it keeps its word.
const MATRIX_ADD_W: f32 = 22.0;
/// What a via cell says, and the via chooser's first row, when nothing
/// scales the route.
pub const NO_VIA: &str = "none";
/// What the `+` on the table's header says.
pub const ADD_ROUTE: &str = "+";
/// The column heads.
pub const MATRIX_HEADS: [&str; 7] = [
    "SOURCE",
    "DESTINATION",
    "DEPTH",
    "VIA",
    "CURVE",
    "INV",
    "ON",
];

/// How tall one matrix row is.
pub const MATRIX_ROW: f32 = 22.0;
/// The fewest rows the matrix shows before the cards above it give: the
/// list scrolls for the rest, and a list three rows tall is still a list.
pub const MATRIX_ROWS_LEAST: usize = 3;
/// How tall the tab strip is.
pub const TAB_HEIGHT: f32 = 22.0;
/// And how wide one tab is. Fixed, so the strip does not shuffle when a page's
/// name changes length — `editor_tabs`' rule.
pub const TAB_WIDTH: f32 = 96.0;
/// The scale chooser's chip on the tab strip, and how far it stands in from
/// the right edge — room for the voice read-out beside it.
pub const SCALE_CHIP_W: f32 = 52.0;
const SCALE_CHIP_RIGHT: f32 = 92.0;
/// One source badge — on the strip now (§3.4), where it carries the
/// source's name and a thumbnail of it.
pub const BADGE_W: f32 = 52.0;
pub const BADGE_H: f32 = 44.0;
/// The mod strip: the band across the foot of every page (§3.4).
pub const STRIP_HEIGHT: f32 = 56.0;
/// A card in the inspector (§3.4) says so with this row, and the layout
/// places it in the drawer over the page rather than in a band.
pub const INSPECTOR_ROW: usize = usize::MAX;

/// One box of the block diagram: a heading, a picture, and its controls.
#[derive(Debug, Clone, PartialEq)]
pub struct FlopsynthCard {
    pub group: InstrumentGroup,
    pub picture: FlopsynthPicture,
    /// Which of the patch's oscillators this card is, if it is one — the
    /// layer's own index, as `fontelle_core::flopsynth::layer_role` numbers
    /// them.
    ///
    /// It is here so that **a sound dropped on a card lands on the right
    /// oscillator**: the window can see where the pointer is and nothing
    /// else, and which layer a card stands for is a fact about the patch
    /// (INVARIANT 2). `None` for every card that is not an oscillator, which
    /// is where a dropped sound has nowhere to go.
    pub oscillator: Option<usize>,
    /// Which band of the window this card belongs to — 0 for the sources, 1
    /// for what they go through, 2 for what moves those.
    ///
    /// **The row is said rather than worked out.** Wrapping when a row fills
    /// up is what a list does, and a list is exactly what §8.1's first rule
    /// forbids: on a 1180-pixel window the filter fits beside the third
    /// oscillator, and a window that put it there would read as a heap of
    /// boxes rather than as a signal path. So whoever builds the view — which
    /// is the layer that knows what each card *is* — declares the band, and
    /// the layout draws it, **in band order whatever order the cards are
    /// listed in**. The first build placed them in list order, which put the
    /// channel and the voice above the oscillators.
    ///
    /// Cards still wrap **within** a band when a narrow window cannot fit
    /// them, because the alternative is running off the edge.
    pub row: usize,
    /// Set aside: stacked down the window's right-hand edge beside the bands
    /// rather than in one of them.
    ///
    /// The sub and the noise are sources like the oscillators and half their
    /// size; put in the sources' band they wrap under it, and the page is a
    /// band taller than the window. In a column of their own they stand
    /// beside the oscillators *and* the filters, which is the shape §8.3
    /// draws. The bands wrap in the room to the column's left for as long as
    /// the column is beside them, and take the whole width below it.
    pub aside: bool,
    /// How many cells across the card is, or `0` to let the layout choose
    /// from the count. Declared for the same reason the band is: an
    /// oscillator's seventeen controls go six across so three oscillators
    /// share a row, and the same seventeen on the sub go three across so it
    /// can stand aside — a fact about what the card is, not how many knobs
    /// it has.
    pub columns: usize,
    /// Whether the card can be taken off the window — an effect slot can,
    /// and nothing else can. Draws the ✕ in its header.
    pub removable: bool,
    /// How big each control's knob is, index for index with the group's
    /// params; a control past the end is Medium. Declared by whoever builds
    /// the view, like the band and the columns, because which knob a player
    /// reaches for first is a fact about the instrument (§3.1).
    pub sizes: Vec<KnobSize>,
}

impl FlopsynthCard {
    /// The size of control `index`.
    pub fn size_of(&self, index: usize) -> KnobSize {
        self.sizes.get(index).copied().unwrap_or_default()
    }

    /// Whether control `index` takes half a cell: a Small knob, a chooser
    /// or a switch. A chooser's chip and a switch's pill are shorter than
    /// any knob, so they stack two to a column whatever the card says.
    pub fn is_half(&self, index: usize) -> bool {
        match self.group.params.get(index) {
            Some(param) => {
                matches!(param.kind, ParamKind::Choice(_) | ParamKind::Switch)
                    || self.size_of(index) == KnobSize::Small
            }
            None => false,
        }
    }
}

/// Everything Flopsynth's window shows.
///
/// Deliberately the same shape as [`InstrumentView`](super::InstrumentView) —
/// groups of normalised values with stable addresses — plus the pictures. The
/// window knows nothing about a `Patch`, and turning an address and a 0..1
/// into a change to one is `fontelle-app`'s job on the other side of
/// `StudioHost`, exactly as it is for every other panel.
#[derive(Debug, Clone, PartialEq)]
pub struct FlopsynthView {
    /// What the channel is playing — the preset's name, when it has one.
    pub title: String,
    pub cards: Vec<FlopsynthCard>,
    /// Which page is showing. The **cards are already filtered to it** by
    /// whoever built the view: the page is a fact about what this window is
    /// looking at, and the layout draws what it is given.
    pub page: FlopsynthPage,
    /// Every modulation source, in the order the strip's badges show them —
    /// on every page (§3.4).
    pub sources: Vec<String>,
    /// The matrix, as rows.
    pub routes: Vec<FlopsynthRoute>,
    /// How many voices are sounding right now (§11, phase 6).
    ///
    /// Off the **audio thread's own state** rather than off the document: a
    /// count of live voices is not a fact a document has. Polyphony is a
    /// number somebody sets, and the only way to know whether sixteen is
    /// enough for what they are playing is to watch this.
    pub voices: usize,
    /// The device's whole bank, for the Presets page (§8.6) — and empty on
    /// every other page, because a hundred and twenty-eight rows built for a
    /// page that is not showing is work nobody sees.
    pub bank: Vec<PresetChoice>,
    /// How the Presets page is being looked at: which shelf, what has been
    /// typed, how far it is scrolled. Window state, set by the window.
    pub browse: PresetBrowse,
    /// How far down the matrix is scrolled, in pixels. Window state, like
    /// `browse.scroll`, and clamped by the layout for the same reason: only
    /// the layout knows how many rows fit. The **table** scrolls; the page
    /// still does not (§8.1 rule 7) — a patch can have more routes than any
    /// window has rows, and the Grand Piano's nineteen do not fit the size
    /// the window opens at.
    pub matrix_scroll: f32,
    /// The window's scale (§3.2): one of [`SCALES`], multiplying every
    /// design size. Window state, read off the settings by whoever builds
    /// the view.
    pub scale: f32,
    /// A thumbnail per source, index for index with `sources`, for the
    /// strip's badges (§3.4): an envelope's curve, an LFO's cycle, a macro's
    /// value as one sample; empty for a source with no picture.
    pub source_shapes: Vec<Vec<f32>>,
    /// Which family each source is, index for index with `sources` — the
    /// ink its badge and its rings wear (§3.3). The host's to say: the
    /// window never sees a `ModSource` (INVARIANT 4), and a name is not a
    /// family.
    pub source_families: Vec<crate::document::SourceFamily>,
    /// Every destination a route could reach, by label, in the order the
    /// table's destination chooser lists them (§3.4). On the Matrix page.
    pub destinations: Vec<String>,
    /// The curves a route can have, by name, in the curve chooser's order.
    pub curves: Vec<String>,
    /// The source whose editor the inspector shows, if it is open — window
    /// state; the card that edits it is in `cards`, marked `INSPECTOR_ROW`.
    pub inspector: Option<usize>,
    /// A picture per option, for the choosers whose options are shapes —
    /// a wavetable's first frame, an LFO wave's cycle — by the chooser's
    /// address (§3.3). Behind an `Arc`, because the bank's forty tables are
    /// the same forty pictures every revision.
    pub thumbnails: Vec<(fontelle_types::ParamAddress, std::sync::Arc<[Vec<f32>]>)>,
    /// Whether the chain has room for another effect (§8.5) — what draws the
    /// `+ effect` button on the Effects page. The host knows the limit; the
    /// window only needs to know whether it has been reached.
    pub fx_room: bool,
}

impl Default for FlopsynthView {
    fn default() -> Self {
        Self {
            title: String::new(),
            cards: Vec::new(),
            page: FlopsynthPage::Synth,
            sources: Vec::new(),
            routes: Vec::new(),
            voices: 0,
            bank: Vec::new(),
            browse: PresetBrowse::default(),
            matrix_scroll: 0.0,
            scale: 1.0,
            source_shapes: Vec::new(),
            source_families: Vec::new(),
            destinations: Vec::new(),
            curves: Vec::new(),
            inspector: None,
            thumbnails: Vec::new(),
            fx_room: false,
        }
    }
}

impl FlopsynthView {
    /// The pictures for the chooser at `address`, one per option, or `None`
    /// for a chooser of words.
    pub fn thumbnails_for(&self, address: &fontelle_types::ParamAddress) -> Option<&[Vec<f32>]> {
        self.thumbnails
            .iter()
            .find(|(at, _)| at == address)
            .map(|(_, shapes)| &shapes[..])
    }
}

/// Where one card ended up.
#[derive(Debug, Clone, PartialEq)]
pub struct CardLayout {
    pub frame: Rect,
    pub header: Rect,
    /// Empty when the card has no picture, or when the panel is too small to
    /// give it one.
    pub picture: Rect,
    /// `(param index, cell)`, in the order the group lists them.
    pub cells: Vec<(usize, Rect)>,
    /// The ✕ at the right end of the header, for a card that can be taken
    /// off. Empty for every other card.
    pub remove: Rect,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FlopsynthLayout {
    /// The room the cards were placed in: under the tabs and the canopy.
    pub body: Rect,
    /// The whole of what the window was given, tabs and canopy included —
    /// what the bridge's hull and the sky are drawn across.
    pub whole: Rect,
    /// The page tabs, along the top of the body.
    pub tabs: Vec<(FlopsynthPage, Rect)>,
    /// The **canopy**: the bridge's window onto the sky, between the tab
    /// strip and the consoles. On every page, because a bridge always has
    /// its window; [`CANOPY_HEIGHT`] at the window's scale.
    pub canopy: Rect,
    /// The scale chooser's chip, at the right end of the tab strip (§3.2).
    pub scale_chip: Rect,
    pub cards: Vec<CardLayout>,
    /// The strip across the foot of the page (§3.4), on every page.
    pub strip: Rect,
    /// The strip's source badges, one per `FlopsynthView::sources`.
    pub badges: Vec<Rect>,
    /// The inspector's drawer over the page, above the strip, when a source
    /// is being inspected; empty otherwise. The card in it is the one in
    /// `cards` marked `INSPECTOR_ROW`.
    pub inspector: Rect,
    pub inspector_close: Rect,
    /// The panel the matrix rows are in. Present on the Modulation page even
    /// with no routes, so it can say there are none — an empty area where a
    /// list should be reads as a bug.
    pub matrix: Rect,
    /// One per `FlopsynthView::routes`, **index for index**, so a hit is the
    /// route it names whatever the scroll. A row scrolled out of the panel
    /// has an empty frame and is neither drawn nor pressed.
    pub routes: Vec<MatrixRow>,
    /// The table's header: the column heads and the `+` (§3.4).
    pub matrix_header: MatrixHeader,
    /// The furthest `FlopsynthView::matrix_scroll` can go: the rows that do
    /// not fit, in pixels; zero when they all do.
    pub matrix_max_scroll: f32,
    /// The thumb down the panel's right edge, saying where the list is.
    /// Empty when nothing is hidden.
    pub matrix_scrollbar: Rect,
    /// The Presets page (§8.6). Empty on every other page.
    pub presets: PresetsLayout,
    /// The Effects page's `+ effect` button (§8.5): after the last card, or
    /// first on the page when there is none. Empty on every other page, and
    /// when the chain is full.
    pub add_effect: Rect,
}

impl Default for FlopsynthLayout {
    /// A window with nothing in it: every rectangle empty, which the renderer
    /// skips and every hit test declines. What the host holds before it has
    /// been given a view.
    fn default() -> Self {
        Self {
            body: Rect::ZERO,
            whole: Rect::ZERO,
            tabs: Vec::new(),
            canopy: Rect::ZERO,
            scale_chip: Rect::ZERO,
            cards: Vec::new(),
            strip: Rect::ZERO,
            badges: Vec::new(),
            inspector: Rect::ZERO,
            inspector_close: Rect::ZERO,
            matrix: Rect::ZERO,
            routes: Vec::new(),
            matrix_header: MatrixHeader::default(),
            matrix_max_scroll: 0.0,
            matrix_scrollbar: Rect::ZERO,
            presets: PresetsLayout::default(),
            add_effect: Rect::ZERO,
        }
    }
}

/// What is under a point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlopsynthHit {
    /// One control, by its card and its place in that card's group.
    Control { card: usize, param: usize },
    /// A card's picture — a different gesture from turning a knob (§8.7): a
    /// wave is dragged sideways, a response is dragged in both directions.
    Picture { card: usize },
    /// A card's header strip, which is what a drag reorders an effect slot by.
    Header { card: usize },
    /// The ✕ on an effect card: take the slot off the chain.
    Remove { card: usize },
    /// The Effects page's `+ effect` button.
    AddEffect,
    /// The scale chooser on the tab strip.
    Scale,
    /// The inspector's ✕.
    InspectorClose,
}

/// What the `+ effect` button says.
pub const ADD_EFFECT: &str = "+ effect";
/// And how big it is.
const ADD_EFFECT_W: f32 = 108.0;
const ADD_EFFECT_H: f32 = 26.0;
/// The ✕ on a removable card's header.
const REMOVE_SIZE: f32 = 14.0;

/// Whether a control sits on the card's **nameplate** rather than in its
/// grid: the oscillator's kind chooser, which says what the module *is* —
/// the label on the module — and which cost every oscillator a row of the
/// window while it was a cell.
pub fn is_nameplate_control(param: &InstrumentParam) -> bool {
    param.address.as_str().ends_with("/synth/kind")
}

/// How wide the nameplate's chooser is, at the design size.
pub const NAMEPLATE_CHIP_W: f32 = 66.0;

/// How wide a card is, in cells, when the host did not say.
///
/// Sized by **what is in it** rather than by a column count, so a chorus's
/// eight controls and a macro row's four do not both get a third of the
/// window. Five across is the widest a card gets on its own: past that the
/// eye stops reading it as a group, and the cards that want six say so.
///
/// Unless five across would make it **taller than the window**. The patch
/// chain's EQ is fifty controls, most of them two cells wide by
/// [`cell_span`]'s caption rule — ninety cells, sixteen rows, an 894-pixel
/// card on a 798-pixel body (`docs/flopsynth-next.md` §1.4(2), found by the
/// Effects fit test). A card that runs off the window is worse than a wide
/// one, so past [`WIDE_CARD_CELLS`] the card goes as wide as it must to stay
/// within [`WIDE_CARD_ROWS`], up to [`MAX_COLUMNS`]. The rack of §3.6 is the
/// design answer; this is what keeps the page honest until it lands.
fn columns_for(card: &FlopsynthCard, measure: Measure<'_>) -> usize {
    let (mut full, mut half) = (0usize, 0usize);
    for (index, param) in card.group.params.iter().enumerate() {
        if is_nameplate_control(param) {
            continue;
        }
        let span = cell_span_measured(param, measure);
        if card.is_half(index) {
            half += span;
        } else {
            full += span;
        }
    }
    // Two halves to a column.
    let cells = full + half.div_ceil(2);
    if cells <= WIDE_CARD_CELLS {
        cells.clamp(1, 5)
    } else {
        cells.div_ceil(WIDE_CARD_ROWS).clamp(5, MAX_COLUMNS)
    }
}

/// Past this many cells a count-sized card widens rather than deepens, and
/// this is how deep it may go — see [`columns_for`].
const WIDE_CARD_CELLS: usize = 25;
const WIDE_CARD_ROWS: usize = 4;
/// The widest a card can be: sixteen cells is 908 pixels, which the window
/// holds beside its margins.
const MAX_COLUMNS: usize = 16;

/// How wide a string is, in pixels, at the size the captions are drawn.
///
/// The window hands the layout the shaper's own answer ([`Labels`]'s small
/// form — `flopsynth_layout_with`); the fixture tests, and a layout asked
/// before anything is shaped, get [`estimated_width`].
pub type Measure<'a> = &'a dyn Fn(&str) -> f32;

/// A width from a character count, for when nothing has been shaped:
/// the mean advance of the caption face at its size, measured over the
/// bank's captions (5.0–6.5 px a glyph; "Hardness" 6.1, "division" 4.9).
pub fn estimated_width(text: &str) -> f32 {
    text.chars().count() as f32 * ESTIMATED_CHAR_W
}

const ESTIMATED_CHAR_W: f32 = 5.9;

/// The room a caption has: its cell. A caption is drawn centred over the
/// control and may fill the cell edge to edge — "mod from" is 51.6 px in a
/// 52 px cell and reads.
pub const CAPTION_ROOM: f32 = FLOP_CELL_W;
/// The room a chooser's chip gives its text in a single cell: the cell less
/// the chip's inset either side, the text's own indent, and the chevron
/// with its gap. The renderer draws the chip from the same three numbers.
///
/// Trimmed with the counting rule's fall: the chip used to spend twenty-one
/// of its cell's fifty-two pixels on itself, and "Bypass", "chorus" and
/// "Grains" — the words the reports named — are thirty-three to thirty-six
/// wide in this face. Thirty-seven fits them in one cell; "Reverse",
/// "Quantise" and "Formant" are wider still and take two.
pub const CHIP_TEXT_ROOM: f32 = FLOP_CELL_W - CHIP_INSET * 2.0 - CHIP_TEXT_INDENT - CHIP_CHEVRON;
pub const CHIP_INSET: f32 = 1.0;
pub const CHIP_TEXT_INDENT: f32 = 3.0;
/// The chevron's wedge, its margin from the edge, and its gap from the text.
pub const CHIP_CHEVRON: f32 = 10.0;

/// How many cells a control takes across its row: two when something about it
/// would not fit in one, one for everything else.
///
/// Two things can overflow, and both do. A **chooser's options** are the
/// obvious one — "NES Pulse 12.5" in a fifty-pixel cell is "NES Pu". The
/// other is the **caption**, which is drawn over every control and not only
/// over choosers: "Natural vibrato" over a knob reads "Natural vi", and a
/// knob you cannot name is one you set by counting along the row.
///
/// **Measured, not counted.** The rule used to be nine characters, and by
/// it "Bypass" fitted and read "Bypas", "Reverse" read "Revers", "chorus"
/// "choru" (`docs/flopsynth-next.md` §1.4(7), open since v0.7.0): a
/// chooser's text has a third of its cell taken by the chip's own chrome,
/// and six letters of this face are wider than that. The caption is held to
/// [`CAPTION_ROOM`] and every option to [`CHIP_TEXT_ROOM`], by the width
/// `measure` gives.
pub fn cell_span_measured(param: &InstrumentParam, measure: Measure<'_>) -> usize {
    if measure(&param.label) > CAPTION_ROOM + 0.01 {
        return 2;
    }
    match &param.kind {
        ParamKind::Choice(options)
            if options
                .iter()
                .any(|option| measure(option) > CHIP_TEXT_ROOM + 0.01) =>
        {
            2
        }
        _ => 1,
    }
}

/// [`cell_span_measured`] with the count-based estimate.
pub fn cell_span(param: &InstrumentParam) -> usize {
    cell_span_measured(param, &estimated_width)
}

/// Lays the cards out as the signal flows — the bands top to bottom, each
/// band left to right — with the aside cards in a column down the right.
///
/// **The window does not scroll** (§8.1 rule 7). What does not fit is on
/// another page — and when the body is short, the *air* goes first, then the
/// pictures down to a soft floor, then every cell shrinks together down to
/// [`CELL_FLOOR`], and last the pictures down to [`PICTURE_FLOOR`]. The
/// controls give last because they are the thing being used, and they give
/// *together* because a page of knobs in two sizes is a page that looks
/// broken.
pub fn flopsynth_layout(body: Rect, metrics: &Metrics, view: &FlopsynthView) -> FlopsynthLayout {
    flopsynth_layout_with(body, metrics, view, &estimated_width)
}

/// [`flopsynth_layout`], measuring captions and options with `measure` —
/// the window's shaper — to decide which controls take a double cell
/// ([`cell_span_measured`]).
pub fn flopsynth_layout_with(
    body: Rect,
    metrics: &Metrics,
    view: &FlopsynthView,
    measure: Measure<'_>,
) -> FlopsynthLayout {
    let whole = body;
    let empty_cards = |view: &FlopsynthView| {
        view.cards
            .iter()
            .map(|_| CardLayout {
                frame: Rect::ZERO,
                header: Rect::ZERO,
                picture: Rect::ZERO,
                cells: Vec::new(),
                remove: Rect::ZERO,
            })
            .collect::<Vec<_>>()
    };
    if body.is_empty() {
        return FlopsynthLayout {
            body,
            whole,
            tabs: Vec::new(),
            canopy: Rect::ZERO,
            cards: empty_cards(view),
            ..Default::default()
        };
    }
    let _ = metrics;
    // Every design size below is multiplied by this and by nothing else.
    let scale = if view.scale.is_finite() && view.scale > 0.0 {
        view.scale
    } else {
        1.0
    };
    let gap = CARD_GAP * scale;

    // The tab strip, along the top, and everything else in what is left. The
    // strip is chrome: a card drawn across a tab is a tab that cannot be
    // pressed, so the cards start below it and never at the body's own top.
    let tab_h = TAB_HEIGHT * scale;
    let tab_w = TAB_WIDTH * scale;
    let tabs: Vec<(FlopsynthPage, Rect)> = FlopsynthPage::ALL
        .iter()
        .enumerate()
        .map(|(index, page)| {
            let x = body.x + index as f32 * (tab_w + 2.0);
            (
                *page,
                Rect::new(x, body.y, tab_w, tab_h).intersection(&body),
            )
        })
        .collect();
    // The scale chooser, at the strip's right end, short of the voice
    // read-out the renderer puts in the corner.
    let chip_w = SCALE_CHIP_W * scale;
    let scale_chip = Rect::new(
        body.right() - SCALE_CHIP_RIGHT * scale - chip_w,
        body.y,
        chip_w,
        tab_h,
    )
    .intersection(&body);
    let mut body = Rect::new(
        body.x,
        body.y + tab_h + gap,
        body.width,
        (body.height - tab_h - gap).max(0.0),
    );

    // The canopy, under the tabs and over everything else, the same height
    // on every page (§3.2): the instrument's eyes are the same picture
    // whatever is being edited under them. It used to take what the cards
    // left, which on the Presets page was nothing and on the Modulation
    // page a slit.
    let canopy_height = (CANOPY_HEIGHT * scale).min(body.height);
    let canopy = Rect::new(body.x, body.y, body.width, canopy_height).intersection(&body);
    body = Rect::new(
        body.x,
        body.y + canopy_height + gap,
        body.width,
        (body.height - canopy_height - gap).max(0.0),
    );

    // The strip across the foot of every page (§3.4), and its badges: one
    // per source, left to right, as wide as the strip lets that many be.
    let strip_h = (STRIP_HEIGHT * scale).min(body.height);
    let strip = Rect::new(body.x, body.bottom() - strip_h, body.width, strip_h).intersection(&body);
    let badges = strip_badges(strip, view.sources.len(), scale);
    body = Rect::new(
        body.x,
        body.y,
        body.width,
        (body.height - strip_h - gap).max(0.0),
    );
    // What the layout hands back as its body: the page, between the canopy
    // and the strip. `body` goes on being cut — by the drawer, by the
    // matrix — and what is left of it is where the cards went, which on a
    // page whose table took all of it is nothing at all; and a window whose
    // body is nothing draws nothing (2026-09-18: the first click on a badge
    // on the Matrix page).
    let page_body = body;

    // The inspector's drawer (§3.4): the card marked for it, full width,
    // laid over the page just above the strip, as tall as the card and its
    // header need. The page's own cards are placed as if it were not there
    // — it is a drawer, and what is under it comes back when it shuts.
    let inspected: Vec<usize> = view
        .cards
        .iter()
        .enumerate()
        .filter(|(_, card)| card.row == INSPECTOR_ROW)
        .map(|(index, _)| index)
        .collect();
    let page_cards: Vec<FlopsynthCard> = view
        .cards
        .iter()
        .filter(|card| card.row != INSPECTOR_ROW)
        .cloned()
        .collect();
    let (inspector, inspector_close, inspector_cards) =
        if view.inspector.is_some() && !inspected.is_empty() {
            let pad = CARD_PAD * scale;
            // As wide as the drawer, whatever columns the host declared: the
            // editor of one source has the whole width to itself, which is
            // what the LFO's and the envelope's pictures want.
            let inner_w = body.width - pad * 2.0;
            let columns = ((inner_w - pad * 2.0) / (FLOP_GRID.cell_w * scale)).floor() as usize;
            let inspected_cards: Vec<FlopsynthCard> = inspected
                .iter()
                .map(|i| {
                    let mut card = view.cards[*i].clone();
                    card.columns = columns.max(1);
                    card
                })
                .collect();
            let probe = Rect::new(body.x + pad, body.y, body.width - pad * 2.0, f32::MAX / 4.0);
            let natural = place(probe, &inspected_cards, FLOP_GRID, scale, measure);
            let tall = natural
                .iter()
                .map(|c| c.frame.bottom())
                .fold(probe.y, f32::max)
                - probe.y;
            let height = (tall + pad * 2.0).min(body.height);
            // Over the page's foot on every page but the Matrix, where the
            // drawer is the top of the page and the table takes the rest —
            // §3.4's "the inspector plus the full table".
            let drawer = if view.page == FlopsynthPage::Modulation {
                let drawer = Rect::new(body.x, body.y, body.width, height);
                body = Rect::new(
                    body.x,
                    body.y + height + gap,
                    body.width,
                    (body.height - height - gap).max(0.0),
                );
                drawer
            } else {
                Rect::new(body.x, body.bottom() - height, body.width, height)
            };
            let inner = Rect::new(
                drawer.x + pad,
                drawer.y + pad,
                drawer.width - pad * 2.0,
                (drawer.height - pad * 2.0).max(0.0),
            );
            let placed = place(inner, &inspected_cards, FLOP_GRID, scale, measure);
            // The ✕ sits at the right end of the card's own header: the
            // drawer has no header of its own, because the card's nameplate
            // already says what is in it and a band saying it again over
            // the band that says it was two headers for one thing.
            let close = placed.first().map_or(Rect::ZERO, |card| {
                let size = REMOVE_SIZE * scale;
                Rect::new(
                    card.header.right() - pad - size,
                    card.header.y + (card.header.height - size) / 2.0,
                    size,
                    size,
                )
            });
            (drawer, close, placed)
        } else {
            (Rect::ZERO, Rect::ZERO, Vec::new())
        };
    // Stitches a layout of the page's cards and of the inspected ones back
    // into the view's own order, so a card's index is the same everywhere.
    let stitch = |page: Vec<CardLayout>, inspector: Vec<CardLayout>| -> Vec<CardLayout> {
        let (mut page, mut inspector) = (page.into_iter(), inspector.into_iter());
        view.cards
            .iter()
            .map(|card| {
                if card.row == INSPECTOR_ROW {
                    inspector.next()
                } else {
                    page.next()
                }
                .unwrap_or(CardLayout {
                    frame: Rect::ZERO,
                    header: Rect::ZERO,
                    picture: Rect::ZERO,
                    cells: Vec::new(),
                    remove: Rect::ZERO,
                })
            })
            .collect()
    };

    // The Presets page is the bank and nothing else: whatever cards the host
    // put in the view are not drawn on it.
    if view.page == FlopsynthPage::Presets {
        return FlopsynthLayout {
            body: page_body,
            whole,
            tabs,
            canopy,
            scale_chip,
            strip,
            badges,
            inspector,
            inspector_close,
            cards: stitch(empty_cards(view), inspector_cards),
            presets: presets_layout(body, view),
            ..Default::default()
        };
    }

    // The Modulation page's matrix, along the bottom of what is left. Taken
    // out of the body *before* the cards are placed, so the cards cannot
    // run under it.
    let matrix = match view.page {
        FlopsynthPage::Modulation => {
            // The matrix takes the room its rows need — a matrix with two
            // routes in it should not take a third of the window — up to
            // what the cards leave it, and never less than
            // [`MATRIX_ROWS_LEAST`] rows. It scrolls for the rest: a card's
            // captions cannot scroll, a list can (§1.4(8)).
            let wanted =
                CARD_HEADER + CARD_PAD * 2.0 + (view.routes.len().max(1) as f32) * MATRIX_ROW;
            let least = CARD_HEADER + CARD_PAD * 2.0 + MATRIX_ROWS_LEAST as f32 * MATRIX_ROW;
            let natural = place(body, &page_cards, FLOP_GRID, scale, measure)
                .iter()
                .map(|c| c.frame.bottom())
                .fold(body.y, f32::max)
                - body.y;
            let height = wanted
                .min((body.height - natural - gap).max(least.min(wanted)))
                .min(body.height);
            // A page with no cards of its own — the Matrix page since §3.4
            // — is the table, from the canopy (or the drawer) down: a list
            // floating at the foot of an empty page reads as one that lost
            // something.
            let matrix = if page_cards.is_empty() {
                body
            } else {
                Rect::new(body.x, body.bottom() - height, body.width, height).intersection(&body)
            };
            let height = matrix.height;
            body = Rect::new(
                body.x,
                body.y,
                body.width,
                (body.height - height - gap).max(0.0),
            );
            matrix
        }
        _ => Rect::ZERO,
    };
    if page_cards.is_empty() {
        let (routes, matrix_header, matrix_max_scroll, matrix_scrollbar) =
            matrix_rows(matrix, view.routes.len(), view.matrix_scroll, scale);
        return FlopsynthLayout {
            body: page_body,
            whole,
            tabs,
            canopy,
            scale_chip,
            strip,
            badges,
            inspector,
            inspector_close,
            cards: stitch(Vec::new(), inspector_cards),
            matrix,
            routes,
            matrix_header,
            matrix_max_scroll,
            matrix_scrollbar,
            add_effect: add_effect_button(body, view, &[]),
            ..Default::default()
        };
    }

    // Placed once, at the window's scale. Nothing shrinks: a page that would
    // not fit is a window smaller than the page's size at its scale, which
    // the window refuses (`layout::flopsynth_window_size`).
    let cards = place(body, &page_cards, FLOP_GRID, scale, measure);
    // The matrix was given the least it needs before the cards were placed;
    // now that they are, it takes everything under them.
    let matrix = if matrix.is_empty() {
        matrix
    } else {
        let cards_bottom = cards
            .iter()
            .map(|c| c.frame.bottom())
            .fold(body.y, f32::max);
        let top = (cards_bottom + gap).min(matrix.y);
        Rect::new(
            matrix.x,
            top,
            matrix.width,
            (matrix.bottom() - top).max(0.0),
        )
    };
    let (routes, matrix_header, matrix_max_scroll, matrix_scrollbar) =
        matrix_rows(matrix, view.routes.len(), view.matrix_scroll, scale);
    let add_effect = add_effect_button(body, view, &cards);
    FlopsynthLayout {
        body: page_body,
        whole,
        tabs,
        canopy,
        scale_chip,
        cards: stitch(cards, inspector_cards),
        strip,
        badges,
        inspector,
        inspector_close,
        matrix,
        routes,
        matrix_header,
        matrix_max_scroll,
        matrix_scrollbar,
        presets: PresetsLayout::default(),
        add_effect,
    }
}

/// Places `cards` in `body` on the corrector's grid, giving something up and
/// going again until they fit — the air first, then the pictures to their
/// soft floor, then every cell together down to [`CELL_FLOOR`], and last the
/// pictures to [`PICTURE_FLOOR`].
///
/// The corrector's console (`docs/tune-plan.md` §7.2) is the one window
/// still laid out this way: it was measured at [`TUNE_GRID`] and shrinks
/// rather than refuses. Flopsynth's window places once at its scale and
/// never shrinks (`flopsynth_layout_with`).
pub fn fit_cards(body: Rect, cards: &[FlopsynthCard], measure: Measure<'_>) -> Vec<CardLayout> {
    let mut picture_height = TUNE_GRID.picture;
    let mut scale = 1.0f32;
    let mut placed;
    loop {
        let grid = Grid {
            picture: picture_height / scale,
            ..TUNE_GRID
        };
        placed = place(body, cards, grid, scale, measure);
        let bottom = placed.iter().map(|c| c.frame.bottom()).fold(0.0, f32::max);
        if bottom <= body.bottom() + 0.01 {
            break;
        }
        if picture_height > PICTURE_SOFT_FLOOR {
            picture_height = (picture_height - 4.0).max(PICTURE_SOFT_FLOOR);
        } else if scale > CELL_FLOOR + 0.001 {
            scale = (scale - 0.05).max(CELL_FLOOR);
        } else if picture_height > PICTURE_FLOOR {
            picture_height = (picture_height - 4.0).max(PICTURE_FLOOR);
        } else {
            break;
        }
    }
    placed
}

/// Where the `+ effect` button goes: after the last card on the row it ends,
/// or on a row of its own when that would run off the right, or first on
/// the page when there is no card. Nowhere on any other page, and nowhere
/// when the chain is full.
fn add_effect_button(body: Rect, view: &FlopsynthView, cards: &[CardLayout]) -> Rect {
    if view.page != FlopsynthPage::Effects || !view.fx_room || body.is_empty() {
        return Rect::ZERO;
    }
    let last = cards.iter().filter(|c| !c.frame.is_empty()).max_by(|a, b| {
        (a.frame.y, a.frame.x)
            .partial_cmp(&(b.frame.y, b.frame.x))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let (x, y) = match last {
        None => (body.x, body.y),
        Some(card) if card.frame.right() + CARD_GAP + ADD_EFFECT_W <= body.right() + 0.01 => {
            (card.frame.right() + CARD_GAP, card.frame.y)
        }
        Some(card) => (body.x, card.frame.bottom() + CARD_GAP),
    };
    Rect::new(x, y, ADD_EFFECT_W, ADD_EFFECT_H).intersection(&body)
}

/// What one card wants, before anything is placed: its width and height, and
/// where its cells go relative to its own top-left.
struct Wanted {
    width: f32,
    height: f32,
    /// `(param index, cell)`, with the cell relative to the card's frame.
    cells: Vec<(usize, Rect)>,
    picture: f32,
    removable: bool,
}

/// Lays the cards out as the signal flows — the bands top to bottom, each
/// band left to right — with the aside cards in a column down the right.
///
/// **The window does not scroll** (§8.1 rule 7) and **nothing shrinks**
/// (§3.1): what does not fit is on another page, and a window too small for
/// a page at its scale is a window that is not opened.
///
/// Inside a card the cells go in **two passes**. The full-height controls —
/// Large and Medium knobs — flow first, left to right, wrapping; then the
/// half-height ones (Small knobs, choosers, switches) fill the half-cells,
/// two to a column, in reading order — the top half of every free column
/// on a row, then the bottom half — taking the first slot that fits. So the
/// knob a player reaches for first is the first thing on the card, and a
/// seventeen-control oscillator is three rows with a 44-pixel knob in it.
/// A grid with no half (`TUNE_GRID`) is one pass, in listing order.
fn wanted(card: &FlopsynthCard, grid: Grid, scale: f32, measure: Measure<'_>) -> Wanted {
    let (cell_w, cell_h) = (grid.cell_w * scale, grid.cell_h * scale);
    let pad = CARD_PAD * scale;
    let header = CARD_HEADER * scale;
    let columns = match card.columns {
        0 => columns_for(card, measure),
        n => n,
    }
    .max(1);
    let picture = if card.picture.is_none() {
        0.0
    } else {
        grid.picture * scale + pad
    };
    let top = header + pad + picture;
    let width = pad * 2.0 + columns as f32 * cell_w;

    let mut cells = Vec::with_capacity(card.group.params.len());
    // Which columns of which row are taken, and by what: `Full` both
    // halves, `Top`/`Bottom` one. Grown as rows are needed.
    #[derive(Clone, Copy, PartialEq)]
    enum Slot {
        Free,
        Top,
        Bottom,
        Full,
    }
    let mut rows: Vec<Vec<Slot>> = Vec::new();
    let new_row = |rows: &mut Vec<Vec<Slot>>| rows.push(vec![Slot::Free; columns]);

    // The card's **kind** chooser is not in the flow at all: it sits on
    // the nameplate, at the right end of the header — see
    // [`is_nameplate_control`].
    for (index, param) in card.group.params.iter().enumerate() {
        if is_nameplate_control(param) {
            let chip_w = (NAMEPLATE_CHIP_W * scale).min(width - pad * 2.0);
            cells.push((index, Rect::new(width - pad - chip_w, 0.0, chip_w, header)));
        }
    }

    // Pass one: the full cells.
    let (mut column, mut row) = (0usize, 0usize);
    new_row(&mut rows);
    for (index, param) in card.group.params.iter().enumerate() {
        if is_nameplate_control(param) || (grid.half_h.is_some() && card.is_half(index)) {
            continue;
        }
        let span = cell_span_measured(param, measure).min(columns);
        if column + span > columns {
            column = 0;
            row += 1;
            new_row(&mut rows);
        }
        for slot in &mut rows[row][column..column + span] {
            *slot = Slot::Full;
        }
        cells.push((
            index,
            Rect::new(
                pad + column as f32 * cell_w,
                top + row as f32 * cell_h,
                cell_w * span as f32,
                cell_h,
            ),
        ));
        column += span;
    }

    // Pass two: the half cells, into the first slot that fits — the top
    // half of a row's free columns before the bottom half, rows in order.
    if let Some(half) = grid.half_h {
        let half_h = half * scale;
        for (index, param) in card.group.params.iter().enumerate() {
            if is_nameplate_control(param) || !card.is_half(index) {
                continue;
            }
            let span = cell_span_measured(param, measure).min(columns);
            let slot_for = |rows: &[Vec<Slot>]| {
                rows.iter().enumerate().find_map(|(r, slots)| {
                    [Slot::Top, Slot::Bottom].into_iter().find_map(|tier| {
                        (0..=columns.saturating_sub(span))
                            .find(|c| {
                                (*c..*c + span).all(|k| {
                                    slots[k] == Slot::Free
                                        || (tier == Slot::Bottom && slots[k] == Slot::Top)
                                        || (tier == Slot::Top && slots[k] == Slot::Bottom)
                                })
                            })
                            .map(|c| (r, tier, c))
                    })
                })
            };
            let (r, tier, c) = loop {
                if let Some(found) = slot_for(&rows) {
                    break found;
                }
                new_row(&mut rows);
            };
            for slot in &mut rows[r][c..c + span] {
                *slot = match (*slot, tier) {
                    (Slot::Free, t) => t,
                    _ => Slot::Full,
                };
            }
            let y = top + r as f32 * cell_h + if tier == Slot::Bottom { half_h } else { 0.0 };
            cells.push((
                index,
                Rect::new(pad + c as f32 * cell_w, y, cell_w * span as f32, half_h),
            ));
        }
    }

    // Every row is a whole cell tall, so a card is as tall as its rows —
    // except a card with nothing but its nameplate, which is its header.
    let used_rows = if rows
        .iter()
        .all(|slots| slots.iter().all(|s| *s == Slot::Free))
    {
        0
    } else {
        rows.len()
    };
    Wanted {
        width,
        height: top + used_rows as f32 * cell_h + pad,
        cells,
        picture: if card.picture.is_none() {
            0.0
        } else {
            grid.picture * scale
        },
        removable: card.removable,
    }
}

/// Puts a wanted card at `(x, y)`.
fn placed(frame_x: f32, frame_y: f32, width: f32, want: &Wanted, scale: f32) -> CardLayout {
    let (header_h, pad, remove_size) = (CARD_HEADER * scale, CARD_PAD * scale, REMOVE_SIZE * scale);
    let frame = Rect::new(frame_x, frame_y, width, want.height);
    let header = Rect::new(frame.x, frame.y, frame.width, header_h);
    let picture = if want.picture <= 0.0 {
        Rect::ZERO
    } else {
        Rect::new(
            frame.x + pad,
            frame.y + header_h + pad,
            (frame.width - pad * 2.0).max(0.0),
            want.picture,
        )
    };
    let cells = want
        .cells
        .iter()
        .map(|(index, cell)| {
            (
                *index,
                Rect::new(frame.x + cell.x, frame.y + cell.y, cell.width, cell.height),
            )
        })
        .collect();
    let remove = if want.removable {
        Rect::new(
            header.right() - pad - remove_size,
            header.y + (header.height - remove_size) / 2.0,
            remove_size,
            remove_size,
        )
        .intersection(&header)
    } else {
        Rect::ZERO
    };
    CardLayout {
        frame,
        header,
        picture,
        cells,
        remove,
    }
}

fn place(
    body: Rect,
    cards_in: &[FlopsynthCard],
    grid: Grid,
    scale: f32,
    measure: Measure<'_>,
) -> Vec<CardLayout> {
    let gap = CARD_GAP * scale;
    let wants: Vec<Wanted> = cards_in
        .iter()
        .map(|card| wanted(card, grid, scale, measure))
        .collect();
    let mut cards: Vec<Option<CardLayout>> = vec![None; cards_in.len()];

    // The aside column first, so the bands know how much room they have. As
    // wide as the widest card in it, against the right-hand edge, stacked.
    let aside: Vec<usize> = (0..cards_in.len())
        .filter(|index| cards_in[*index].aside)
        .collect();
    let aside_width = aside
        .iter()
        .map(|index| wants[*index].width)
        .fold(0.0f32, f32::max)
        .min(body.width);
    let mut aside_bottom = body.y;
    {
        let mut y = body.y;
        for index in &aside {
            let want = &wants[*index];
            cards[*index] = Some(placed(
                body.right() - aside_width,
                y,
                aside_width,
                want,
                scale,
            ));
            y += want.height + gap;
            aside_bottom = y - gap;
        }
    }
    // The room the bands have at a given height: short of the column while
    // the column is beside them, the whole body below it.
    let right_at = |y: f32| {
        if !aside.is_empty() && y < aside_bottom - 0.01 {
            body.right() - aside_width - gap
        } else {
            body.right()
        }
    };

    // The bands, in band order — a stable sort, so the listed order holds
    // within a band.
    let mut order: Vec<usize> = (0..cards_in.len())
        .filter(|index| !cards_in[*index].aside)
        .collect();
    order.sort_by_key(|index| cards_in[*index].row);

    let (mut x, mut y) = (body.x, body.y);
    let mut row_height = 0.0f32;
    let mut band = order.first().map_or(0, |index| cards_in[*index].row);
    for index in order {
        let card = &cards_in[index];
        let want = &wants[index];
        // A new band always starts a new row: that is what makes the window a
        // signal path rather than a heap.
        if card.row != band {
            band = card.row;
            if x > body.x {
                x = body.x;
                y += row_height + gap;
                row_height = 0.0;
            }
        }
        // Wrap when this card would run off the right — but never on the first
        // card of a row, or a card wider than the panel would loop for ever.
        if x > body.x && x + want.width > right_at(y) + 0.01 {
            x = body.x;
            y += row_height + gap;
            row_height = 0.0;
        }
        let width = want.width.min((right_at(y) - x).max(0.0)).min(body.width);
        cards[index] = Some(placed(x, y, width, want, scale));
        row_height = row_height.max(want.height);
        x += width + gap;
    }

    cards
        .into_iter()
        .map(|card| {
            card.unwrap_or(CardLayout {
                frame: Rect::ZERO,
                header: Rect::ZERO,
                picture: Rect::ZERO,
                cells: Vec::new(),
                remove: Rect::ZERO,
            })
        })
        .collect()
}

/// The rows inside the matrix panel, under its heading, scrolled by
/// `scroll` (clamped to what is hidden); with the header's heads, how far it
/// could scroll and the thumb that says where it is.
fn matrix_rows(
    panel: Rect,
    count: usize,
    scroll: f32,
    scale: f32,
) -> (Vec<MatrixRow>, MatrixHeader, f32, Rect) {
    if panel.is_empty() {
        return (Vec::new(), MatrixHeader::default(), 0.0, Rect::ZERO);
    }
    let row_h = MATRIX_ROW * scale;
    let pad = CARD_PAD * scale;
    let list_top = panel.y + CARD_HEADER * scale + pad;
    let list_height = (panel.bottom() - pad - list_top).max(0.0);
    let fit = ((list_height + 0.01) / row_h).floor().max(0.0) as usize;
    let hidden = count.saturating_sub(fit);
    let max_scroll = hidden as f32 * row_h;
    // Whole rows: a scroll between rows is rounded to the nearer one.
    let scroll = (scroll.clamp(0.0, max_scroll) / row_h).round() * row_h;
    let first = (scroll / row_h).round() as usize;
    // The thumb, down the right edge of the list, in a lane the rows stop
    // short of so it covers no row's ✕.
    let lane = if hidden == 0 { 0.0 } else { 8.0 * scale };
    let scrollbar = if hidden == 0 || count == 0 {
        Rect::ZERO
    } else {
        let track = Rect::new(panel.right() - pad - 4.0, list_top, 4.0, list_height);
        let length = (track.height * fit as f32 / count as f32).max(12.0);
        let travel = (track.height - length).max(0.0);
        let at = if max_scroll > 0.0 {
            travel * (scroll / max_scroll)
        } else {
            0.0
        };
        Rect::new(track.x, track.y + at, track.width, length).intersection(&panel)
    };
    // The columns, cut once for every row from the width a row has: the
    // fixed ones from each end, the destination what is left.
    let width = (panel.width - pad * 2.0 - lane).max(0.0);
    let columns = matrix_columns(panel.x + pad, width, scale);
    let rows = (0..count)
        .map(|index| {
            let frame = if index < first || index >= first + fit {
                Rect::ZERO
            } else {
                Rect::new(
                    panel.x + pad,
                    list_top + (index - first) as f32 * row_h,
                    width,
                    row_h,
                )
                .intersection(&panel)
            };
            let cell = |(x, w): (f32, f32), inset: f32| {
                if frame.is_empty() {
                    Rect::ZERO
                } else {
                    Rect::new(x, frame.y + inset, w, (frame.height - inset * 2.0).max(0.0))
                        .intersection(&frame)
                }
            };
            let switch = 3.0 * scale;
            MatrixRow {
                frame,
                grip: cell(columns.grip, 0.0),
                source: cell(columns.source, 0.0),
                destination: cell(columns.destination, 0.0),
                depth: cell(columns.depth, 4.0 * scale),
                via: cell(columns.via, 0.0),
                curve: cell(columns.curve, 0.0),
                invert: cell(columns.invert, switch),
                bypass: cell(columns.bypass, switch),
                remove: {
                    let size = MATRIX_REMOVE * scale;
                    cell(columns.remove, ((row_h - size) / 2.0).max(0.0))
                },
            }
        })
        .collect();
    // The heads, in the header band over the same columns; the `+` at the
    // right end, over the ✕ column and what it borrows from the switches.
    let header_h = CARD_HEADER * scale;
    let head = |(x, w): (f32, f32)| Rect::new(x, panel.y, w, header_h).intersection(&panel);
    let add_w = MATRIX_ADD_W * scale;
    let header = MatrixHeader {
        source: head(columns.source),
        destination: head(columns.destination),
        depth: head(columns.depth),
        via: head(columns.via),
        curve: head(columns.curve),
        invert: head(columns.invert),
        bypass: head(columns.bypass),
        add: Rect::new(
            columns.remove.0 + columns.remove.1 - add_w,
            panel.y + 2.0 * scale,
            add_w,
            (header_h - 4.0 * scale).max(0.0),
        )
        .intersection(&panel),
    };
    (rows, header, max_scroll, scrollbar)
}

/// Where each column of the table starts and how wide it is, across a row
/// `width` wide from `x`.
struct MatrixColumns {
    grip: (f32, f32),
    source: (f32, f32),
    destination: (f32, f32),
    depth: (f32, f32),
    via: (f32, f32),
    curve: (f32, f32),
    invert: (f32, f32),
    bypass: (f32, f32),
    remove: (f32, f32),
}

fn matrix_columns(x: f32, width: f32, scale: f32) -> MatrixColumns {
    let gap = MATRIX_COL_GAP * scale;
    let fixed = [
        MATRIX_GRIP,
        MATRIX_SOURCE,
        MATRIX_DEPTH,
        MATRIX_VIA,
        MATRIX_CURVE,
        MATRIX_SWITCH,
        MATRIX_SWITCH,
        MATRIX_REMOVE,
    ]
    .iter()
    .map(|w| w * scale)
    .sum::<f32>()
        + gap * 8.0;
    // The destination flexes; when the row is too narrow even for the
    // fixed columns, every column shrinks alike rather than the last ones
    // falling off the end.
    let squeeze = if fixed > width && fixed > 0.0 {
        width / fixed
    } else {
        1.0
    };
    let w = |px: f32| px * scale * squeeze;
    let destination_w = (width - fixed).max(0.0);
    let mut at = x;
    let mut take = |width: f32| {
        let cell = (at, width);
        at += width + gap * squeeze;
        cell
    };
    MatrixColumns {
        grip: take(w(MATRIX_GRIP)),
        source: take(w(MATRIX_SOURCE)),
        destination: take(destination_w),
        depth: take(w(MATRIX_DEPTH)),
        via: take(w(MATRIX_VIA)),
        curve: take(w(MATRIX_CURVE)),
        invert: take(w(MATRIX_SWITCH)),
        bypass: take(w(MATRIX_SWITCH)),
        remove: take(w(MATRIX_REMOVE)),
    }
}

/// The strip's badges: `count` of them left to right, [`BADGE_W`] wide at
/// `scale` or narrower when that many will not fit, centred in the strip's
/// height with a little air.
fn strip_badges(strip: Rect, count: usize, scale: f32) -> Vec<Rect> {
    if strip.is_empty() || count == 0 {
        return Vec::new();
    }
    let pad = CARD_PAD * scale;
    let gap = 3.0 * scale;
    let room = strip.width - pad * 2.0;
    let width = ((room - gap * (count as f32 - 1.0)) / count as f32).min(BADGE_W * scale);
    let height = (BADGE_H * scale).min(strip.height - pad);
    (0..count)
        .map(|i| {
            Rect::new(
                strip.x + pad + i as f32 * (width + gap),
                strip.y + (strip.height - height) / 2.0,
                width,
                height,
            )
            .intersection(&strip)
        })
        .collect()
}

/// The three bands of a badge (§3.4): the source's name across the top, its
/// picture under it, and a hairline along the foot that reads its level.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BadgeAnatomy {
    pub name: Rect,
    pub picture: Rect,
    pub level: Rect,
}

/// Where a badge's name, picture and level go, at the window's scale.
pub fn badge_anatomy(badge: Rect, scale: f32) -> BadgeAnatomy {
    let text = CELL_TEXT_H * scale;
    let level_h = 2.0 * scale;
    let inset = 3.0 * scale;
    let name = Rect::new(
        badge.x + inset,
        badge.y + 1.0,
        badge.width - inset * 2.0,
        text,
    );
    let level = Rect::new(
        badge.x + inset,
        badge.bottom() - inset - level_h,
        badge.width - inset * 2.0,
        level_h,
    );
    let picture = Rect::new(
        badge.x + inset,
        name.bottom() + 1.0,
        badge.width - inset * 2.0,
        (level.y - 2.0 * scale - name.bottom() - 1.0).max(0.0),
    );
    BadgeAnatomy {
        name,
        picture,
        level,
    }
}

/// A source's name as its badge says it: whole if it fits the name band,
/// else cut to what does with an ellipsis — "Bright…" for a macro called
/// Brightness. The window shapes each candidate it tries (the measure is
/// the shaper's), so the renderer, asking the same question of the same
/// labels, gets the same answer.
pub fn badge_caption(name: &str, room: f32, measure: Measure<'_>) -> String {
    if measure(name) <= room {
        return name.to_string();
    }
    let chars: Vec<char> = name.chars().collect();
    for n in (1..chars.len()).rev() {
        let head: String = chars[..n].iter().collect();
        let candidate = format!("{}\u{2026}", head.trim_end());
        if measure(&candidate) <= room {
            return candidate;
        }
    }
    format!("{}\u{2026}", chars.first().copied().unwrap_or(' '))
}

/// A source's picture fitted to its badge: a shape that never dips under
/// zero — an envelope, a macro — fills the picture from its floor; one that
/// swings both ways is centred on it, the way [`super::thumbnail_points`]
/// draws a wave.
pub fn badge_points(picture: Rect, shape: &[f32]) -> Vec<(f32, f32)> {
    if shape.iter().any(|s| *s < 0.0) {
        return super::thumbnail_points(picture, shape);
    }
    let last = shape.len().saturating_sub(1).max(1) as f32;
    let inset = 1.0;
    shape
        .iter()
        .enumerate()
        .map(|(i, s)| {
            (
                picture.x + picture.width * i as f32 / last,
                picture.bottom() - inset - (picture.height - inset * 2.0) * s.clamp(0.0, 1.0),
            )
        })
        .collect()
}

/// Which page's tab is under `(x, y)`.
pub fn flopsynth_tab_at(layout: &FlopsynthLayout, x: f32, y: f32) -> Option<FlopsynthPage> {
    layout
        .tabs
        .iter()
        .find(|(_, rect)| !rect.is_empty() && rect.contains(x, y))
        .map(|(page, _)| *page)
}

/// Which **effect** card is under `(x, y)` — where a slot dragged by its
/// header lands (§8.5). A card that cannot be taken off the chain is not a
/// slot, so an oscillator answers `None` and a drag let go over it is called
/// off.
pub fn effect_card_at(
    layout: &FlopsynthLayout,
    view: &FlopsynthView,
    x: f32,
    y: f32,
) -> Option<usize> {
    layout.cards.iter().enumerate().position(|(index, placed)| {
        view.cards.get(index).is_some_and(|card| card.removable)
            && !placed.frame.is_empty()
            && placed.frame.contains(x, y)
    })
}

/// Which source badge is under `(x, y)` — where a drag-to-assign starts.
pub fn badge_at(layout: &FlopsynthLayout, x: f32, y: f32) -> Option<usize> {
    layout
        .badges
        .iter()
        .position(|rect| !rect.is_empty() && rect.contains(x, y))
}

/// What a press in the matrix landed on.
pub fn matrix_hit(layout: &FlopsynthLayout, x: f32, y: f32) -> Option<MatrixHit> {
    let inside = |rect: Rect| !rect.is_empty() && rect.contains(x, y);
    let head = &layout.matrix_header;
    if inside(head.add) {
        return Some(MatrixHit::Add);
    }
    if inside(head.source) {
        return Some(MatrixHit::SortSource);
    }
    if inside(head.destination) {
        return Some(MatrixHit::SortDestination);
    }
    for (index, row) in layout.routes.iter().enumerate() {
        if row.frame.is_empty() || !row.frame.contains(x, y) {
            continue;
        }
        let cells = [
            (row.remove, MatrixHit::Remove(index)),
            (row.depth, MatrixHit::Depth(index)),
            (row.grip, MatrixHit::Grip(index)),
            (row.source, MatrixHit::Source(index)),
            (row.destination, MatrixHit::Destination(index)),
            (row.via, MatrixHit::Via(index)),
            (row.curve, MatrixHit::Curve(index)),
            (row.invert, MatrixHit::Invert(index)),
            (row.bypass, MatrixHit::Bypass(index)),
        ];
        return cells
            .iter()
            .find(|(rect, _)| inside(*rect))
            .map(|(_, hit)| *hit);
    }
    None
}

/// Where a row dragged by its grip would land if let go with the pointer
/// at `y` (§3.4): the index of the row under it, one past the last for a
/// pointer below them all, `None` for one outside the table.
pub fn route_landing(layout: &FlopsynthLayout, y: f32) -> Option<usize> {
    if layout.matrix.is_empty() || y < layout.matrix.y || y > layout.matrix.bottom() {
        return None;
    }
    let shown: Vec<(usize, Rect)> = layout
        .routes
        .iter()
        .enumerate()
        .filter(|(_, row)| !row.frame.is_empty())
        .map(|(index, row)| (index, row.frame))
        .collect();
    let Some((first, top)) = shown.first().copied() else {
        return Some(0);
    };
    if y < top.y {
        return Some(first);
    }
    for (index, frame) in &shown {
        if y <= frame.bottom() {
            return Some(*index);
        }
    }
    shown.last().map(|(index, _)| index + 1)
}

/// The depth a press at `x` on a row's slider means, -1..=1.
///
/// **Bipolar with nothing in the middle**, because a modulation depth is: the
/// centre is a route that does nothing rather than a route that is not there.
pub fn matrix_depth_at(slider: Rect, x: f32) -> f32 {
    if slider.width <= 0.0 {
        return 0.0;
    }
    (((x - slider.x) / slider.width) * 2.0 - 1.0).clamp(-1.0, 1.0)
}

/// What is under `(x, y)`.
///
/// Controls first, then the picture, then the header: the smallest thing under
/// the pointer wins, which is what makes a knob inside a card reachable.
pub fn flopsynth_hit(layout: &FlopsynthLayout, x: f32, y: f32) -> Option<FlopsynthHit> {
    if !layout.add_effect.is_empty() && layout.add_effect.contains(x, y) {
        return Some(FlopsynthHit::AddEffect);
    }
    if !layout.scale_chip.is_empty() && layout.scale_chip.contains(x, y) {
        return Some(FlopsynthHit::Scale);
    }
    if !layout.inspector_close.is_empty() && layout.inspector_close.contains(x, y) {
        return Some(FlopsynthHit::InspectorClose);
    }
    // In reverse: the last card drawn is the first one under the pointer,
    // which is what puts the inspector's drawer over the page.
    for (card, placed) in layout.cards.iter().enumerate().rev() {
        for (param, cell) in &placed.cells {
            if cell.contains(x, y) {
                return Some(FlopsynthHit::Control {
                    card,
                    param: *param,
                });
            }
        }
        if !placed.picture.is_empty() && placed.picture.contains(x, y) {
            return Some(FlopsynthHit::Picture { card });
        }
        if !placed.remove.is_empty() && placed.remove.contains(x, y) {
            return Some(FlopsynthHit::Remove { card });
        }
        if placed.header.contains(x, y) {
            return Some(FlopsynthHit::Header { card });
        }
    }
    None
}

/// Where the knob sits inside its cell — the caption goes above it and the
/// read-out below, which is why the cell is taller than the knob.
///
/// **Read off the cell**, not off [`FLOP_KNOB`]: the layout shrinks the cells
/// when a page would not fit, and a knob that kept its design size in a
/// smaller cell would sit over its own read-out.
pub fn flop_knob_rect(cell: Rect) -> Rect {
    if cell.is_empty() {
        return Rect::ZERO;
    }
    let size = (cell.height * 0.42).min(cell.width * 0.6).max(8.0);
    Rect::new(
        cell.x + (cell.width - size) * 0.5,
        // Far enough below the caption that the **modulation ring** clears it.
        // A bipolar arc grows from straight up, which is the topmost point of
        // the circle, and the ring sits `RING_GAP + RING_BAND` outside the
        // groove — so a knob placed tight under its caption had its arc
        // striking through the word naming it.
        cell.y + cell.height * 0.33,
        size,
        size,
    )
}

// ------------------------------------------------------------- gestures ---

/// The canopy's eyes (`docs/flopsynth-next.md` §3.2): where the
/// oscilloscope, the spectrum and the voice lamps go inside the canopy,
/// left to right, with the sky behind them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Eyes {
    pub scope: Rect,
    pub spectrum: Rect,
    pub lamps: Rect,
}

/// The lamps: a grid of `LAMP_COLUMNS` across, at `LAMP_PITCH` apart.
pub const LAMP_COLUMNS: usize = 8;
pub const LAMP_PITCH: f32 = 11.0;
const EYES_INSET: f32 = 14.0;
const EYES_GAP: f32 = 12.0;

/// Lays the eyes out in `canopy` at `scale`: the lamps' column at the
/// right, then the scope with two fifths of the rest and the spectrum
/// with three — the spectrum is the picture with the most in it.
pub fn canopy_eyes(canopy: Rect, scale: f32) -> Eyes {
    if canopy.is_empty() {
        return Eyes {
            scope: Rect::ZERO,
            spectrum: Rect::ZERO,
            lamps: Rect::ZERO,
        };
    }
    let inset = EYES_INSET * scale;
    let gap = EYES_GAP * scale;
    let inner = canopy.inset(inset);
    let lamps_w = (LAMP_COLUMNS as f32 * LAMP_PITCH * scale).min(inner.width / 3.0);
    let lamps = Rect::new(inner.right() - lamps_w, inner.y, lamps_w, inner.height);
    let room = (inner.width - lamps_w - gap * 2.0).max(0.0);
    let scope = Rect::new(inner.x, inner.y, room * 0.4, inner.height);
    let spectrum = Rect::new(scope.right() + gap, inner.y, room * 0.6, inner.height);
    Eyes {
        scope: scope.intersection(&canopy),
        spectrum: spectrum.intersection(&canopy),
        lamps: lamps.intersection(&canopy),
    }
}

/// The spectrum's floor: sixty decibels down is nothing to see.
pub const SPECTRUM_FLOOR_DB: f32 = -60.0;

/// One bar per band across `rect`, standing on its floor, as tall as the
/// band is above [`SPECTRUM_FLOOR_DB`]. The bands are already a log axis
/// (`canvas::spectrum_band_hz`), so evenly across is the octaves evenly
/// across.
pub fn spectrum_bars(rect: Rect, bands_db: &[f32]) -> Vec<Rect> {
    if rect.is_empty() || bands_db.is_empty() {
        return Vec::new();
    }
    let width = rect.width / bands_db.len() as f32;
    bands_db
        .iter()
        .enumerate()
        .map(|(i, db)| {
            let share = ((db - SPECTRUM_FLOOR_DB) / -SPECTRUM_FLOOR_DB).clamp(0.0, 1.0);
            let height = rect.height * share;
            Rect::new(
                rect.x + i as f32 * width,
                rect.bottom() - height,
                (width - 1.0).max(0.5),
                height,
            )
        })
        .collect()
}

/// One dot per voice of `polyphony`, in rows of [`LAMP_COLUMNS`] from the
/// top-left of `rect`; the renderer lights the first `sounding` of them.
pub fn lamp_dots(rect: Rect, polyphony: usize, scale: f32) -> Vec<Rect> {
    if rect.is_empty() {
        return Vec::new();
    }
    let pitch = LAMP_PITCH * scale;
    let size = (pitch * 0.55).max(2.0);
    let rows = polyphony.div_ceil(LAMP_COLUMNS).max(1);
    let top = rect.y + ((rect.height - rows as f32 * pitch) / 2.0).max(0.0);
    (0..polyphony)
        .map(|i| {
            let (column, row) = (i % LAMP_COLUMNS, i / LAMP_COLUMNS);
            Rect::new(
                rect.x + column as f32 * pitch + (pitch - size) / 2.0,
                top + row as f32 * pitch + (pitch - size) / 2.0,
                size,
                size,
            )
            .intersection(&rect)
        })
        .collect()
}

/// Where a cell's three parts go — the caption, the control, the read-out
/// — for a control of `size` and `kind` in `cell`, at `scale`.
///
/// A **full** cell is the three bands top to bottom, the knob at its size
/// in the middle. A **half** cell (`FLOP_CELL_HALF` tall) holds a Small
/// knob on the left with its caption and read-out stacked beside it, or a
/// chooser's chip and a switch's pill under their caption with no read-out
/// (the chip carries its value; the pill says on or off). The renderer
/// draws from these rectangles and the hit tests read them, so the knob
/// under the pointer is the knob that is drawn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CellAnatomy {
    pub caption: Rect,
    pub control: Rect,
    /// Empty when the cell has no read-out of its own.
    pub readout: Rect,
}

pub fn cell_anatomy(cell: Rect, size: KnobSize, kind: &ParamKind, scale: f32) -> CellAnatomy {
    if cell.is_empty() {
        return CellAnatomy {
            caption: Rect::ZERO,
            control: Rect::ZERO,
            readout: Rect::ZERO,
        };
    }
    let text_h = CELL_TEXT_H * scale;
    let half = cell.height < FLOP_CELL_H * scale - 0.5;
    match (half, kind) {
        // A chooser or a switch, in a half cell: the caption, then the chip
        // or pill filling what is left.
        (true, ParamKind::Choice(_) | ParamKind::Switch) => CellAnatomy {
            caption: Rect::new(cell.x, cell.y, cell.width, text_h),
            control: Rect::new(
                cell.x,
                cell.y + text_h,
                cell.width,
                (cell.height - text_h - 2.0 * scale).max(0.0),
            ),
            readout: Rect::ZERO,
        },
        // A Small knob: the caption across the top — "character" and "loop
        // out" are Small knobs' captions and fit nothing narrower — then the
        // knob at the left with its read-out beside it, right-aligned, and
        // allowed a couple of pixels past the cell's edge: "centre" is
        // thirty-three pixels and the room beside a twenty-pixel knob is
        // thirty-one.
        (true, _) => {
            let knob = KNOB_SMALL * scale;
            let inset = 3.0 * scale;
            let control = Rect::new(
                cell.x + inset,
                cell.y + text_h + (cell.height - text_h - knob) / 2.0,
                knob,
                knob,
            );
            let words_x = control.right() + scale;
            CellAnatomy {
                caption: Rect::new(cell.x, cell.y + scale, cell.width, text_h),
                control,
                readout: Rect::new(
                    words_x,
                    control.y,
                    (cell.right() + 2.0 * scale - words_x).max(0.0),
                    knob,
                ),
            }
        }
        // A chooser or a switch that was given a whole cell (a grid with no
        // halves): the caption and the control in the top half of it.
        (false, ParamKind::Choice(_) | ParamKind::Switch) => {
            let control_h = (FLOP_CELL_HALF * scale - text_h - 2.0 * scale).max(0.0);
            CellAnatomy {
                caption: Rect::new(cell.x, cell.y + 2.0 * scale, cell.width, text_h),
                control: Rect::new(cell.x, cell.y + text_h + 4.0 * scale, cell.width, control_h),
                readout: Rect::ZERO,
            }
        }
        // A whole cell, the three bands: the knob at its size, centred in
        // the room between the caption and the read-out — and never nearer
        // the caption than the **modulation ring** needs: a bipolar arc grows
        // from straight up, and a knob tight under its caption had its arc
        // striking through the word naming it.
        (false, _) => {
            let knob = (size.pixels() * scale).min(cell.width - 4.0 * scale);
            let caption = Rect::new(cell.x, cell.y + scale, cell.width, text_h);
            let readout = Rect::new(cell.x, cell.bottom() - text_h - scale, cell.width, text_h);
            let room = readout.y - caption.bottom();
            CellAnatomy {
                caption,
                control: Rect::new(
                    cell.x + (cell.width - knob) / 2.0,
                    caption.bottom() + ((room - knob) / 2.0).max(RING_GAP + RING_BAND),
                    knob,
                    knob,
                ),
                readout,
            }
        }
    }
}

/// One line of caption or read-out, at the design size: the small label's
/// line.
pub const CELL_TEXT_H: f32 = 12.0;

/// The position a drag across a wave picture lands on, 0..=1.
///
/// Clamped rather than extrapolated: past either end of the picture is that
/// end of the table, because there is no table past it.
pub fn wave_position_at(picture: Rect, x: f32) -> f32 {
    if picture.width <= 0.0 {
        return 0.0;
    }
    ((x - picture.x) / picture.width).clamp(0.0, 1.0)
}

/// The cutoff and resonance a drag on a filter's response lands on, both
/// 0..=1.
///
/// The EQ handle's gesture, borrowed rather than reinvented: sideways is the
/// corner and **up is more**, which is the direction a resonant peak grows on
/// the picture it is being dragged over.
pub fn filter_xy_at(picture: Rect, x: f32, y: f32) -> (f32, f32) {
    if picture.width <= 0.0 || picture.height <= 0.0 {
        return (0.0, 0.0);
    }
    let cutoff = ((x - picture.x) / picture.width).clamp(0.0, 1.0);
    let resonance = 1.0 - ((y - picture.y) / picture.height).clamp(0.0, 1.0);
    (cutoff, resonance)
}

/// The polyline of an envelope, in the picture's own pixels.
///
/// # The time axis is a square root
///
/// An envelope's stages span three orders of magnitude — a 5 ms attack and a
/// 2 s release are both ordinary — and on a linear axis the attack is less
/// than one pixel wide. A square root gives the short stages room without
/// making the long ones useless, which is what makes the curve something you
/// can aim at.
///
/// `y` grows downward, so the top of the rectangle is full level.
pub fn env_curve_points(
    rect: Rect,
    attack: f32,
    decay: f32,
    sustain: f32,
    release: f32,
) -> Vec<(f32, f32)> {
    if rect.is_empty() {
        return Vec::new();
    }
    let level = |v: f32| rect.bottom() - rect.height * v.clamp(0.0, 1.0);
    // The sustain is drawn as a stage of its own so the shape reads as an
    // envelope rather than as three lines: a fixed share of the width, which
    // is what every synthesiser's envelope display does.
    const SUSTAIN_SHARE: f32 = 0.22;
    let span = |v: f32| v.clamp(0.0, 1.0).sqrt();
    let (a, d, r) = (span(attack), span(decay), span(release));
    let total = (a + d + r).max(1e-4);
    let usable = rect.width * (1.0 - SUSTAIN_SHARE);
    let width = |v: f32| usable * v / total;

    let mut x = rect.x;
    let mut points = vec![(x, level(0.0))];
    x += width(a);
    points.push((x, level(1.0)));
    x += width(d);
    points.push((x, level(sustain)));
    x += rect.width * SUSTAIN_SHARE;
    points.push((x, level(sustain)));
    x += width(r);
    points.push((x.min(rect.right()), level(0.0)));
    points
}

/// One cycle of an LFO's shape, in the picture's own pixels.
///
/// Drawn from [`LfoWave::value`] — the **same function the voice plays** — so
/// the shape on screen is the shape in the sound (catalogue rule 3, one level
/// up). Sample & hold has no closed form, so it is drawn from a fixed seed:
/// its picture is stable, which is what a picture has to be.
pub fn lfo_curve_points(rect: Rect, wave: LfoWave, phase: f32) -> Vec<(f32, f32)> {
    if rect.is_empty() {
        return Vec::new();
    }
    const STEPS: usize = 64;
    (0..=STEPS)
        .map(|i| {
            let t = i as f32 / STEPS as f32;
            let at = (t + phase).rem_euclid(1.0);
            let value = match wave {
                // Eight held steps across the cycle, from a fixed table: what
                // sample & hold looks like, without pretending to know which
                // numbers this voice happens to have drawn.
                LfoWave::SampleHold => {
                    const HELD: [f32; 8] = [0.4, -0.7, 0.9, -0.2, 0.6, -0.9, 0.1, -0.5];
                    HELD[((at * HELD.len() as f32) as usize).min(HELD.len() - 1)]
                }
                other => other.value(at),
            };
            (
                rect.x + rect.width * t,
                rect.y + rect.height * (0.5 - value.clamp(-1.0, 1.0) * 0.5),
            )
        })
        .collect()
}

/// A wave picture's polyline, from its samples.
///
/// The samples are the oscillator's **current frame** — the one it is actually
/// reading — so moving the position knob moves the picture, which is what
/// makes the picture worth having.
pub fn wave_curve_points(rect: Rect, samples: &[f32]) -> Vec<(f32, f32)> {
    if rect.is_empty() || samples.is_empty() {
        return Vec::new();
    }
    samples
        .iter()
        .enumerate()
        .map(|(i, value)| {
            let t = i as f32 / (samples.len() - 1).max(1) as f32;
            (
                rect.x + rect.width * t,
                rect.y + rect.height * (0.5 - value.clamp(-1.0, 1.0) * 0.5),
            )
        })
        .collect()
}

/// A run of points, as every picture here hands the renderer.
pub type Polyline = Vec<(f32, f32)>;

/// A recording's shape, as two polylines: its highs along the top and its
/// lows along the bottom, one point per column of `peaks`, in `rect`.
///
/// Two lines rather than a bar per column, so the shape can be filled
/// between them and stroked along them — what makes it read as a sound and
/// not as a histogram.
pub fn sound_outline_points(rect: Rect, peaks: &[(f32, f32)]) -> (Polyline, Polyline) {
    if peaks.is_empty() || rect.is_empty() {
        return (Vec::new(), Vec::new());
    }
    let mid = rect.y + rect.height * 0.5;
    let half = rect.height * 0.5;
    let step = if peaks.len() > 1 {
        rect.width / (peaks.len() - 1) as f32
    } else {
        0.0
    };
    let mut top = Vec::with_capacity(peaks.len());
    let mut bottom = Vec::with_capacity(peaks.len());
    for (i, (low, high)) in peaks.iter().enumerate() {
        let x = rect.x + step * i as f32;
        top.push((x, mid - half * high.clamp(-1.0, 1.0)));
        bottom.push((x, mid - half * low.clamp(-1.0, 1.0)));
    }
    (top, bottom)
}

/// A filter response's polyline, from its magnitudes in dB.
///
/// The span is the EQ's, so a filter's curve and an EQ band's curve are read
/// against the same vertical scale — two pictures of the same kind of thing
/// with two different scales is two pictures nobody can compare.
pub const RESPONSE_TOP_DB: f32 = 18.0;
pub const RESPONSE_BOTTOM_DB: f32 = -42.0;

pub fn response_curve_points(rect: Rect, db: &[f32]) -> Vec<(f32, f32)> {
    if rect.is_empty() || db.is_empty() {
        return Vec::new();
    }
    let span = RESPONSE_TOP_DB - RESPONSE_BOTTOM_DB;
    db.iter()
        .enumerate()
        .map(|(i, value)| {
            let t = i as f32 / (db.len() - 1).max(1) as f32;
            let up = ((value - RESPONSE_BOTTOM_DB) / span).clamp(0.0, 1.0);
            (rect.x + rect.width * t, rect.bottom() - rect.height * up)
        })
        .collect()
}

// ------------------------------------------------- the envelope's nodes ---

/// One corner of the envelope curve, as a thing to drag.
///
/// Four rather than five: the first point is where the note started, and an
/// attack that began late is not an envelope any synthesiser has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvNode {
    Attack,
    Decay,
    /// The one node that moves **up and down**: the others are times and this
    /// is a level.
    Sustain,
    Release,
}

impl EnvNode {
    /// Which stage of the patch this node is, as the address's tail.
    pub fn stage(self) -> &'static str {
        match self {
            Self::Attack => "attack",
            Self::Decay => "decay",
            Self::Sustain => "sustain",
            Self::Release => "release",
        }
    }
}

/// How near a node a press has to land, in pixels.
///
/// Generous, because the nodes are four pixels wide and a pointer is not: the
/// same argument the EQ's handles make, and the radius they use.
pub const NODE_GRAB: f32 = 7.0;

/// Which node is under `(x, y)`, if any.
///
/// Read off [`env_curve_points`] rather than computed a second way. Two
/// answers to "where is the decay node" is exactly the defect that leaves a
/// handle you can see and cannot grab, and it is invisible until somebody
/// tries.
pub fn env_node_at(
    rect: Rect,
    attack: f32,
    decay: f32,
    sustain: f32,
    release: f32,
    x: f32,
    y: f32,
) -> Option<EnvNode> {
    let points = env_curve_points(rect, attack, decay, sustain, release);
    // Skipping point 0, which is the note-on and not a handle. The sustain's
    // handle is the *end* of its plateau (point 3), because that is the corner
    // the release leaves from and the one the eye reads as the level.
    let nodes = [
        (EnvNode::Attack, points.get(1)),
        (EnvNode::Decay, points.get(2)),
        (EnvNode::Sustain, points.get(3)),
        (EnvNode::Release, points.get(4)),
    ];
    let mut best: Option<(f32, EnvNode)> = None;
    for (node, at) in nodes {
        let Some((px, py)) = at else { continue };
        let distance = ((px - x).powi(2) + (py - y).powi(2)).sqrt();
        if distance <= NODE_GRAB && best.is_none_or(|(near, _)| distance < near) {
            best = Some((distance, node));
        }
    }
    best.map(|(_, node)| node)
}

/// Where a node lands after a drag of `(dx, dy)` pixels from `from`.
///
/// The three time stages move **sideways** and the sustain moves **up**,
/// because that is what each one is; a node that answered to both would be
/// two controls under one finger. The gain is the picture's own size, so a
/// drag across the whole picture is the whole range — the rule every drag in
/// this program follows.
pub fn env_node_drag(rect: Rect, node: EnvNode, from: f32, dx: f32, dy: f32) -> f32 {
    let moved = match node {
        EnvNode::Sustain => {
            let span = rect.height.max(1.0);
            -dy / span
        }
        _ => {
            // Across the *usable* part, which is what the stages share — see
            // `env_curve_points`' sustain plateau.
            let span = (rect.width * 0.78).max(1.0);
            dx / span
        }
    };
    // The time axis is a square root on the picture, so a drag is one there
    // too: a pixel near zero is a millisecond and a pixel near the end is a
    // tenth of a second, which is what makes both aimable.
    let value = match node {
        EnvNode::Sustain => from + moved,
        _ => (from.clamp(0.0, 1.0).sqrt() + moved).max(0.0).powi(2),
    };
    value.clamp(0.0, 1.0)
}

// ------------------------------------------------- the modulation ring ---

/// How far outside the knob's groove the modulation ring sits, in pixels.
///
/// Outside rather than on it, so a knob with a route on it can still be
/// **turned**: the value and the depth are two controls in one place, and the
/// one you get has to be the one you aimed at.
pub const RING_GAP: f32 = 2.0;

/// How wide the band is.
pub const RING_BAND: f32 = 4.0;

/// The ring's gap from the groove and its band, for a knob of this size: a
/// Small knob in a half cell has no room for the full ring — it would strike
/// through the caption over it and the cell under it — so it wears a thin
/// one close in.
pub fn ring_band(knob: Rect) -> (f32, f32) {
    if knob.width < KNOB_MEDIUM - 0.5 {
        (1.0, 3.0)
    } else {
        (RING_GAP, RING_BAND)
    }
}

/// The bands of a knob with `count` routes on it (§3.3): `(inner radius,
/// thickness)` each, the oldest route nearest the groove and the rest
/// stacked outward with a pixel between. Thinner than the single ring was
/// — three of them have to fit under a caption — and thinner still on a
/// Small knob.
pub fn ring_bands(knob: Rect, count: usize) -> Vec<(f32, f32)> {
    let (gap, band) = ring_band(knob);
    let band = (band * 0.65).max(1.5);
    let radius = knob.width / 2.0;
    (0..count)
        .map(|i| (radius + gap + i as f32 * (band + 1.0), band))
        .collect()
}

/// The stretch of the travel a route covers (§3.3: the band's arc is the
/// route's range): from the knob's own `value` out by `depth` — a unipolar
/// source pushes one way, a bipolar one both — clamped to the travel.
pub fn ring_range(value: f32, depth: f32, bipolar: bool) -> (f32, f32) {
    let (lo, hi) = if bipolar {
        (value - depth.abs(), value + depth.abs())
    } else if depth >= 0.0 {
        (value, value + depth)
    } else {
        (value + depth, value)
    };
    (lo.clamp(0.0, 1.0), hi.clamp(0.0, 1.0))
}

/// Where a source at `source` (−1..=1 for a bipolar one, 0..=1 for a
/// unipolar one) puts the knob right now: its value plus the depth's share
/// of the source, on the travel.
pub fn ring_live(value: f32, depth: f32, source: f32) -> f32 {
    (value + depth * source).clamp(0.0, 1.0)
}

/// Which of `count` bands `(x, y)` is on, if any — the route a press on the
/// ring edits.
pub fn ring_hit_index(knob: Rect, x: f32, y: f32, count: usize) -> Option<usize> {
    if knob.is_empty() {
        return None;
    }
    let (cx, cy) = (knob.x + knob.width / 2.0, knob.y + knob.height / 2.0);
    let distance = ((x - cx).powi(2) + (y - cy).powi(2)).sqrt();
    ring_bands(knob, count)
        .into_iter()
        .position(|(inner, band)| (inner..=inner + band).contains(&distance))
}

/// The point on a band at `t` of the knob's own sweep — 0 at seven o'clock,
/// 0.5 straight up, 1 at five — the same sweep the value arc is drawn on,
/// so a ring and the needle agree about where a value is.
pub fn ring_dot(knob: Rect, band: (f32, f32), t: f32) -> (f32, f32) {
    let (inner, thick) = band;
    let radius = inner + thick / 2.0;
    let (cx, cy) = (knob.x + knob.width / 2.0, knob.y + knob.height / 2.0);
    let angle = (-0.75 + 1.5 * t.clamp(0.0, 1.0)) * std::f32::consts::PI;
    (cx + radius * angle.sin(), cy - radius * angle.cos())
}

/// Whether `(x, y)` is on this control's modulation ring.
pub fn ring_hit(knob: Rect, x: f32, y: f32) -> bool {
    if knob.is_empty() {
        return false;
    }
    let (gap, band) = ring_band(knob);
    let (cx, cy) = (knob.x + knob.width / 2.0, knob.y + knob.height / 2.0);
    let distance = ((x - cx).powi(2) + (y - cy).powi(2)).sqrt();
    let inner = knob.width / 2.0 + gap;
    (inner..=inner + band).contains(&distance)
}

/// The depth a drag of `dy` pixels from `from` lands on, −1..=1.
///
/// **Bipolar**, because a modulation depth is: up adds, down subtracts, and
/// the middle is a route that does nothing rather than a route that is not
/// there. A route at zero still shows in the matrix, which is what lets
/// somebody set one up and then find the amount.
pub fn ring_depth(from: f32, dy: f32) -> f32 {
    /// Pixels for the whole of one direction. The knob drag's own gain
    /// (`knob_value`), so the two gestures in the same place move at the same
    /// speed.
    const SPAN: f32 = 160.0;
    (from - dy / SPAN).clamp(-1.0, 1.0)
}

/// Which of a card's controls a picture gesture moves, by the tail of its
/// address — `"position"`, `"cutoff"`, `"attack"`.
///
/// **The picture moves a control that is already there**, and this is the join
/// between the two. §8.7's whole table is second routes to controls the panel
/// already draws: a wave dragged sideways is the position knob, an envelope
/// node dragged is one of the four stage knobs. So the gesture looks the
/// control up rather than carrying an address of its own, and a control the
/// card does not have is a gesture that does nothing — which is what an LFO's
/// picture is, and why it is not dragged.
pub fn picture_control(card: &FlopsynthCard, tail: &str) -> Option<usize> {
    card.group.params.iter().position(|param| {
        let address = param.address.as_str();
        address.ends_with(tail)
            && address.len() > tail.len()
            && address.as_bytes()[address.len() - tail.len() - 1] == b'/'
    })
}

/// The control at `hit`, if it names one.
pub fn control_at(view: &FlopsynthView, hit: FlopsynthHit) -> Option<&InstrumentParam> {
    match hit {
        FlopsynthHit::Control { card, param } => view.cards.get(card)?.group.params.get(param),
        _ => None,
    }
}

// -------------------------------------------------------- the Presets page
//
// §8.6: a view over the bank filtered to this device, so a person does not
// have to leave the synth to browse. Shelves down the left, the presets in
// the middle under a search box, and the one that is loaded described on the
// right. It invents no mechanism: a row is `ApplyPreset`, a star is the
// favourite the bar's star is, and the search is the menu's own rule.

/// One shelf of the bank: what the left-hand column lists.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum PresetShelf {
    /// Every starred preset. Listed only when there is one.
    Favourites,
    #[default]
    All,
    Category(String),
    /// The user's own presets. Listed only when there is one.
    Mine,
}

impl PresetShelf {
    pub fn label(&self) -> String {
        match self {
            Self::Favourites => "\u{2605} Favourites".to_string(),
            Self::All => "All".to_string(),
            Self::Category(name) => name.clone(),
            Self::Mine => "Mine".to_string(),
        }
    }
}

/// How the Presets page is being looked at. Window state, not document state.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PresetBrowse {
    pub shelf: PresetShelf,
    pub query: String,
    /// How far down the list is scrolled, in pixels. Clamped by the layout,
    /// which is the only thing that knows how long the list came out.
    pub scroll: f32,
}

/// The shelves a bank has, in the order the column lists them.
pub fn preset_shelves(bank: &[PresetChoice]) -> Vec<PresetShelf> {
    let mut shelves = Vec::new();
    if bank.iter().any(|p| p.favourite) {
        shelves.push(PresetShelf::Favourites);
    }
    shelves.push(PresetShelf::All);
    for preset in bank {
        let shelf = PresetShelf::Category(preset.category.clone());
        if !shelves.contains(&shelf) {
            shelves.push(shelf);
        }
    }
    if bank
        .iter()
        .any(|p| p.origin == fontelle_types::PresetOrigin::User)
    {
        shelves.push(PresetShelf::Mine);
    }
    shelves
}

/// Which presets a shelf shows once `query` has narrowed it, by their place
/// in the bank and in the bank's order.
pub fn preset_page_rows(bank: &[PresetChoice], shelf: &PresetShelf, query: &str) -> Vec<usize> {
    bank.iter()
        .enumerate()
        .filter(|(_, preset)| match shelf {
            PresetShelf::Favourites => preset.favourite,
            PresetShelf::All => true,
            PresetShelf::Category(name) => preset.category == *name,
            PresetShelf::Mine => preset.origin == fontelle_types::PresetOrigin::User,
        })
        .filter(|(_, preset)| super::menu_matches(&preset.name, query))
        .map(|(index, _)| index)
        .collect()
}

/// Where the Presets page's pieces are.
#[derive(Debug, Clone, PartialEq)]
pub struct PresetsLayout {
    /// The left-hand column, and each shelf's row in it.
    pub column: Rect,
    pub shelves: Vec<(PresetShelf, Rect)>,
    /// The search box, across the top of the list.
    pub search: Rect,
    /// The panel the rows scroll in.
    pub list: Rect,
    /// `(place in the bank, row)`, in the shelf's order. A row scrolled out of
    /// sight is **empty** — the menu's rule, for the menu's reason.
    pub rows: Vec<(usize, Rect)>,
    /// The right-hand column describing the loaded preset.
    pub about: Rect,
    /// How tall every row together is, so the scroll can be clamped.
    pub content_height: f32,
}

impl Default for PresetsLayout {
    /// Nothing laid out: every rectangle empty, which the renderer skips and
    /// [`presets_hit`] cannot land on. What every page but Presets has.
    fn default() -> Self {
        Self {
            column: Rect::ZERO,
            shelves: Vec::new(),
            search: Rect::ZERO,
            list: Rect::ZERO,
            rows: Vec::new(),
            about: Rect::ZERO,
            content_height: 0.0,
        }
    }
}

impl PresetsLayout {
    /// The furthest the list scrolls: the last row resting on the bottom.
    pub fn max_scroll(&self) -> f32 {
        (self.content_height - self.list.height + LIST_PAD * 2.0).max(0.0)
    }
}

/// What a press on the Presets page landed on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresetsHit {
    /// A shelf, by its place in [`PresetsLayout::shelves`].
    Shelf(usize),
    Search,
    /// A preset, by its place in the **bank**.
    Row(usize),
    /// A preset's star, by its place in the bank.
    Star(usize),
}

/// One row of the list or the shelf column.
pub const PRESET_ROW: f32 = 22.0;
/// The search box.
const SEARCH_HEIGHT: f32 = 26.0;
const SHELF_WIDTH: f32 = 168.0;
const ABOUT_WIDTH: f32 = 250.0;
/// Air inside the three panels.
const LIST_PAD: f32 = 4.0;

fn presets_layout(body: Rect, view: &FlopsynthView) -> PresetsLayout {
    if body.is_empty() {
        return PresetsLayout::default();
    }
    let shelf_width = SHELF_WIDTH.min(body.width * 0.25);
    // The about column goes before the list does: a list you cannot read is
    // the page not working, and a description you cannot read is a nicety.
    let about_width = if body.width >= 720.0 {
        ABOUT_WIDTH
    } else {
        0.0
    };
    let column = Rect::new(body.x, body.y, shelf_width, body.height);
    let about = if about_width > 0.0 {
        Rect::new(body.right() - about_width, body.y, about_width, body.height)
    } else {
        Rect::ZERO
    };
    let list_x = column.right() + CARD_GAP;
    let list_right = if about.is_empty() {
        body.right()
    } else {
        about.x - CARD_GAP
    };
    let panel = Rect::new(list_x, body.y, (list_right - list_x).max(0.0), body.height);
    let search = Rect::new(
        panel.x + LIST_PAD,
        panel.y + LIST_PAD,
        (panel.width - LIST_PAD * 2.0).max(0.0),
        SEARCH_HEIGHT,
    )
    .intersection(&panel);
    let list = Rect::new(
        panel.x,
        search.bottom() + LIST_PAD,
        panel.width,
        (panel.bottom() - search.bottom() - LIST_PAD).max(0.0),
    )
    .intersection(&panel);

    let shelves = preset_shelves(&view.bank)
        .into_iter()
        .enumerate()
        .map(|(index, shelf)| {
            let rect = Rect::new(
                column.x + LIST_PAD,
                column.y + LIST_PAD + index as f32 * PRESET_ROW,
                (column.width - LIST_PAD * 2.0).max(0.0),
                PRESET_ROW,
            )
            .intersection(&column);
            (shelf, rect)
        })
        .collect();

    let which = preset_page_rows(&view.bank, &view.browse.shelf, &view.browse.query);
    let content_height = which.len() as f32 * PRESET_ROW;
    let max_scroll = (content_height - list.height + LIST_PAD * 2.0).max(0.0);
    let scroll = view.browse.scroll.clamp(0.0, max_scroll);
    let top = list.y + LIST_PAD;
    let room = (list.height - LIST_PAD * 2.0).max(0.0);
    let rows = which
        .into_iter()
        .enumerate()
        .map(|(slot, index)| {
            let y = top + slot as f32 * PRESET_ROW - scroll;
            // Whole rows only: half a name sliding under the edge is a row
            // you cannot read and should not be able to press.
            let rect = if y < top - 0.01 || y + PRESET_ROW > top + room + 0.01 {
                Rect::ZERO
            } else {
                Rect::new(
                    list.x + LIST_PAD,
                    y,
                    (list.width - LIST_PAD * 2.0).max(0.0),
                    PRESET_ROW,
                )
                .clamped()
            };
            (index, rect)
        })
        .collect();

    PresetsLayout {
        column,
        shelves,
        search,
        list,
        rows,
        about,
        content_height,
    }
}

/// What a press at `(x, y)` on the Presets page landed on.
pub fn presets_hit(layout: &FlopsynthLayout, x: f32, y: f32) -> Option<PresetsHit> {
    let page = &layout.presets;
    for (index, (_, rect)) in page.shelves.iter().enumerate() {
        if !rect.is_empty() && rect.contains(x, y) {
            return Some(PresetsHit::Shelf(index));
        }
    }
    if !page.search.is_empty() && page.search.contains(x, y) {
        return Some(PresetsHit::Search);
    }
    for (which, rect) in &page.rows {
        if rect.is_empty() || !rect.contains(x, y) {
            continue;
        }
        // The right-hand end of a row is its star — the menu's own width, so
        // the two feel like one control.
        let star = super::menu::STAR_WIDTH.min(rect.width);
        return Some(if x >= rect.right() - star {
            PresetsHit::Star(*which)
        } else {
            PresetsHit::Row(*which)
        });
    }
    None
}

/// The About column's lines, from the bar's view of the loaded preset and
/// how many rows the shelf is showing.
///
/// Generated rather than stored, so it is never stale — and short, because a
/// column of prose beside a list is a column nobody reads.
pub fn preset_about(bar: &super::PresetBarView, showing: usize, total: usize) -> Vec<String> {
    let mut lines = vec![super::preset_bar_name(bar)];
    if !bar.category.is_empty() {
        lines.push(bar.category.clone());
    }
    match bar.origin {
        Some(fontelle_types::PresetOrigin::Factory) => lines.push("factory preset".to_string()),
        Some(fontelle_types::PresetOrigin::User) => lines.push("your preset".to_string()),
        None => {}
    }
    if bar.favourite {
        lines.push("\u{2605} favourite".to_string());
    }
    if bar.dirty {
        lines.push("edited since it was loaded".to_string());
    }
    lines.push(String::new());
    lines.push(match (showing, total) {
        (s, t) if s == t => format!("{t} presets"),
        (s, t) => format!("{s} of {t} presets"),
    });
    lines
}
