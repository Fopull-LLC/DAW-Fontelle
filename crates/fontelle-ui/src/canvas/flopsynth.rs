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
/// One control's cell inside a card, at the size the window opens at.
///
/// Smaller than the generic grid's 92 × 76: that cell is right for a panel of
/// nine controls and far too coarse for a page of a hundred and thirty. The
/// knob is drawn at [`FLOP_KNOB`] inside it, with the caption above and the
/// read-out below, both at the small label size (`text::SMALL_LABEL`).
///
/// **Design sizes.** The layout shrinks every cell together when the page
/// would not fit ([`CELL_FLOOR`]), so what a card is drawn at is read off its
/// cells and never off these.
pub const FLOP_CELL_W: f32 = 52.0;
pub const FLOP_CELL_H: f32 = 54.0;
/// The knob itself, inside its cell at the design size.
pub const FLOP_KNOB: f32 = 24.0;
/// How tall a card's picture is when there is room for it, and the floor it
/// shrinks to before anything else gives (§8.8).
pub const PICTURE_HEIGHT: f32 = 48.0;
pub const PICTURE_FLOOR: f32 = 26.0;
/// The picture gives this much before the cells start to, and the rest only
/// after the cells have reached their own floor: a page of knobs you can still
/// turn with pictures a little smaller reads better than the other way round.
const PICTURE_SOFT_FLOOR: f32 = 36.0;
/// The least a cell shrinks to, as a share of its design size, before the
/// page is allowed to run off the bottom. Below this the captions no longer
/// fit their cells, and a window that small is not one anybody is using.
pub const CELL_FLOOR: f32 = 0.8;
/// How long a chooser's longest option may be, in characters, before the
/// chooser takes two cells rather than one. "NES Pulse 12.5" in a cell fifty
/// pixels wide is "NES Pu", and a chooser whose value cannot be read is one
/// nobody can set on purpose.
pub const WIDE_CHOICE: usize = 9;

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
            Self::Modulation => "Modulation",
            Self::Effects => "Effects",
            Self::Presets => "Presets",
        }
    }
}

/// One row of the matrix, as the window shows it.
///
/// Labels rather than the matrix's own types, for INVARIANT 4's reason: this
/// crate may not see a `ModSource`. The host turns them into words and back.
#[derive(Debug, Clone, PartialEq)]
pub struct FlopsynthRoute {
    pub source: String,
    pub destination: String,
    /// Bipolar, -1..=1.
    pub depth: f32,
}

/// Where one matrix row's parts are.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MatrixRow {
    pub frame: Rect,
    /// The source and destination captions, left to right.
    pub source: Rect,
    pub destination: Rect,
    /// A **slider**, not a knob: the row is 22 pixels tall, which is the audio
    /// editor's argument for the same shape.
    pub depth: Rect,
    pub remove: Rect,
}

/// What a press in the matrix landed on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatrixHit {
    Depth(usize),
    Remove(usize),
}

/// How tall one matrix row is.
pub const MATRIX_ROW: f32 = 22.0;
/// The fewest rows the matrix shows before the cards above it give: the
/// list scrolls for the rest, and a list three rows tall is still a list.
pub const MATRIX_ROWS_LEAST: usize = 3;
/// How tall the tab strip is.
pub const TAB_HEIGHT: f32 = 22.0;
/// The least and the most of the window the canopy takes.
///
/// The least is a slit a sky can still be seen through; the most is what a
/// window taller than the consoles need gives up to it rather than to air
/// between the cards. The cards shrink for the least (`fit_cards`, in its
/// own order) and never for more.
pub const CANOPY_MIN: f32 = 84.0;
pub const CANOPY_MAX: f32 = 240.0;
/// And how wide one tab is. Fixed, so the strip does not shuffle when a page's
/// name changes length — `editor_tabs`' rule.
pub const TAB_WIDTH: f32 = 96.0;
/// One source badge.
pub const BADGE_W: f32 = 74.0;
pub const BADGE_H: f32 = 20.0;

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
    /// Every modulation source, in the order the badge row shows them. Drawn
    /// on the Modulation page only.
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
            fx_room: false,
        }
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
    /// its window; as tall as the cards leave it, within
    /// [`CANOPY_MIN`]..[`CANOPY_MAX`].
    pub canopy: Rect,
    pub cards: Vec<CardLayout>,
    /// The Modulation page's source badges, one per `FlopsynthView::sources`.
    /// Empty on every other page.
    pub badges: Vec<Rect>,
    /// The panel the matrix rows are in. Present on the Modulation page even
    /// with no routes, so it can say there are none — an empty area where a
    /// list should be reads as a bug.
    pub matrix: Rect,
    /// One per `FlopsynthView::routes`, **index for index**, so a hit is the
    /// route it names whatever the scroll. A row scrolled out of the panel
    /// has an empty frame and is neither drawn nor pressed.
    pub routes: Vec<MatrixRow>,
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
            cards: Vec::new(),
            badges: Vec::new(),
            matrix: Rect::ZERO,
            routes: Vec::new(),
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
fn columns_for(card: &FlopsynthCard) -> usize {
    let cells: usize = card
        .group
        .params
        .iter()
        .filter(|param| !is_nameplate_control(param))
        .map(cell_span)
        .sum();
    if cells <= WIDE_CARD_CELLS {
        cells.clamp(1, 5)
    } else {
        cells.div_ceil(WIDE_CARD_ROWS).clamp(5, MAX_COLUMNS)
    }
}

/// Past this many cells a count-sized card widens rather than deepens, and
/// this is how deep it may go — see [`columns_for`].
const WIDE_CARD_CELLS: usize = 25;
const WIDE_CARD_ROWS: usize = 6;
/// The widest a card can be: sixteen cells is 844 pixels, which the least
/// window still holds.
const MAX_COLUMNS: usize = 16;

/// How many cells a control takes across its row: two when something about it
/// would not fit in one ([`WIDE_CHOICE`]), one for everything else.
///
/// Two things can overflow, and both do. A **chooser's options** are the
/// obvious one — "NES Pulse 12.5" in a fifty-pixel cell is "NES Pu". The
/// other is the **caption**, which is drawn over every control and not only
/// over choosers: "Natural vibrato" over a knob reads "Natural vi", and a
/// knob you cannot name is one you set by counting along the row.
///
/// The caption rule is what `docs/tune-plan.md` §7.2 asks for in so many
/// words — "a chooser with a name over `WIDE_CHOICE` characters spans two
/// cells" — and the corrector is where it began to matter, because its
/// parameters are named for a panel with wider rows (§4.3) and its console
/// draws them at 52 pixels.
pub fn cell_span(param: &InstrumentParam) -> usize {
    // One character less room than an option gets: the caption is drawn over
    // the control with the cell's own padding either side, where an option
    // sits inside a chooser that fills the cell. "MIDI bend" is the nine that
    // proves it — it reads "MIDI benc" in one cell.
    if param.label.chars().count() >= WIDE_CHOICE {
        return 2;
    }
    match &param.kind {
        ParamKind::Choice(options)
            if options
                .iter()
                .any(|option| option.chars().count() > WIDE_CHOICE) =>
        {
            2
        }
        _ => 1,
    }
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

    // The tab strip, along the top, and everything else in what is left. The
    // strip is chrome: a card drawn across a tab is a tab that cannot be
    // pressed, so the cards start below it and never at the body's own top.
    let tabs: Vec<(FlopsynthPage, Rect)> = FlopsynthPage::ALL
        .iter()
        .enumerate()
        .map(|(index, page)| {
            let x = body.x + index as f32 * (TAB_WIDTH + 2.0);
            (
                *page,
                Rect::new(x, body.y, TAB_WIDTH, TAB_HEIGHT).intersection(&body),
            )
        })
        .collect();
    let mut body = Rect::new(
        body.x,
        body.y + TAB_HEIGHT + CARD_GAP,
        body.width,
        (body.height - TAB_HEIGHT - CARD_GAP).max(0.0),
    );

    // The Modulation page's two extra pieces — the badge row across the top
    // and the matrix along the bottom — measured **before the canopy is
    // sized**, because the canopy takes what the page leaves and they are on
    // the page. The first build measured the cards alone, and on the Grand
    // Piano's eleven routes the matrix was drawn under ENV 3 and ENV 4 with
    // its first two rows hidden (`docs/flopsynth-next.md` §1.4(2)).
    let (badge_rows, badges_used, matrix_wanted) = match view.page {
        FlopsynthPage::Modulation => {
            let per_row = ((body.width + CARD_GAP) / (BADGE_W + CARD_GAP)).max(1.0) as usize;
            let rows = view.sources.len().div_ceil(per_row.max(1));
            let used = match rows {
                0 => 0.0,
                rows => rows as f32 * (BADGE_H + 2.0) + CARD_GAP,
            };
            let matrix =
                CARD_HEADER + CARD_PAD * 2.0 + (view.routes.len().max(1) as f32) * MATRIX_ROW;
            (per_row, used, matrix + CARD_GAP)
        }
        _ => (1, 0.0, 0.0),
    };
    let extras = badges_used + matrix_wanted;

    // The canopy, under the tabs and over everything else: the least of the
    // window it can have, or whatever the cards at their full size leave —
    // a taller window is more sky, not more air between consoles. The
    // Presets page keeps its list and gives the canopy the least.
    let wanted = match view.page {
        FlopsynthPage::Presets => 0.0,
        _ => {
            let natural = place(body, &view.cards, PICTURE_HEIGHT, 1.0);
            natural
                .iter()
                .map(|c| c.frame.bottom())
                .fold(body.y, f32::max)
                - body.y
                + extras
        }
    };
    // The sky gives before the controls do: in a window too small for both
    // the consoles at their floor and the least canopy, the canopy is what
    // shrinks, down to nothing.
    let cards_floor = match view.page {
        FlopsynthPage::Presets => 0.0,
        _ => {
            let floor = place(body, &view.cards, PICTURE_FLOOR, CELL_FLOOR);
            floor
                .iter()
                .map(|c| c.frame.bottom())
                .fold(body.y, f32::max)
                - body.y
        }
    };
    // The least the page can be: the cards at their floor, the badges, and
    // the matrix showing [`MATRIX_ROWS_LEAST`] rows — it scrolls for the
    // rest.
    let matrix_least = if matrix_wanted > 0.0 {
        (CARD_HEADER + CARD_PAD * 2.0 + MATRIX_ROWS_LEAST as f32 * MATRIX_ROW + CARD_GAP)
            .min(matrix_wanted)
    } else {
        0.0
    };
    let at_floor = cards_floor + badges_used + matrix_least;
    let canopy_height = (body.height - wanted - CARD_GAP)
        .clamp(CANOPY_MIN, CANOPY_MAX)
        .min((body.height - at_floor - CARD_GAP).max(0.0));
    let canopy = Rect::new(body.x, body.y, body.width, canopy_height).intersection(&body);
    body = Rect::new(
        body.x,
        body.y + canopy_height + CARD_GAP,
        body.width,
        (body.height - canopy_height - CARD_GAP).max(0.0),
    );

    // The Presets page is the bank and nothing else: whatever cards the host
    // put in the view are not drawn on it.
    if view.page == FlopsynthPage::Presets {
        return FlopsynthLayout {
            body,
            whole,
            tabs,
            canopy,
            cards: empty_cards(view),
            presets: presets_layout(body, view),
            ..Default::default()
        };
    }

    // The Modulation page's two extra pieces: the badge row across the top of
    // what is left, and the matrix along the bottom. Both are taken out of the
    // body *before* the cards are placed, so the cards cannot run under them.
    let (badges, matrix) = match view.page {
        FlopsynthPage::Modulation => {
            let per_row = badge_rows;
            let badges: Vec<Rect> = view
                .sources
                .iter()
                .enumerate()
                .map(|(index, _)| {
                    let (column, row) = (index % per_row, index / per_row);
                    Rect::new(
                        body.x + column as f32 * (BADGE_W + CARD_GAP),
                        body.y + row as f32 * (BADGE_H + 2.0),
                        BADGE_W,
                        BADGE_H,
                    )
                    .intersection(&body)
                })
                .collect();
            body = Rect::new(
                body.x,
                body.y + badges_used,
                body.width,
                (body.height - badges_used).max(0.0),
            );

            // The matrix takes the room its rows need — a matrix with two
            // routes in it should not take a third of the window — up to
            // what the cards **at their full size** leave it, and never less
            // than [`MATRIX_ROWS_LEAST`] rows. It scrolls for the rest. It
            // used to take four tenths of the body whatever the cards
            // needed, and the cards shrank to their floor to make room: at
            // the size the window opens at, ENV 3 and ENV 4's captions read
            // "release a shaped shaper shape" (§1.4(8)). A card's captions
            // cannot scroll; a list can.
            let wanted = matrix_wanted - CARD_GAP;
            let natural = place(body, &view.cards, PICTURE_HEIGHT, 1.0)
                .iter()
                .map(|c| c.frame.bottom())
                .fold(body.y, f32::max)
                - body.y;
            let least = (matrix_least - CARD_GAP).max(0.0);
            let height = wanted
                .min((body.height - natural - CARD_GAP).max(least))
                .min(body.height);
            let matrix =
                Rect::new(body.x, body.bottom() - height, body.width, height).intersection(&body);
            body = Rect::new(
                body.x,
                body.y,
                body.width,
                (body.height - height - CARD_GAP).max(0.0),
            );
            (badges, matrix)
        }
        _ => (Vec::new(), Rect::ZERO),
    };
    if view.cards.is_empty() {
        let (routes, matrix_max_scroll, matrix_scrollbar) =
            matrix_rows(matrix, view.routes.len(), view.matrix_scroll);
        return FlopsynthLayout {
            body,
            whole,
            tabs,
            canopy,
            cards: empty_cards(view),
            badges,
            matrix,
            routes,
            matrix_max_scroll,
            matrix_scrollbar,
            add_effect: add_effect_button(body, view, &[]),
            ..Default::default()
        };
    }

    // Place, and if the result is taller than the body give something up and
    // go again — in the order the doc comment states. A further pass would be
    // a scrollbar, and this window does not have one.
    let cards = fit_cards(body, &view.cards);
    // The matrix was given the least it needs before the cards were placed;
    // now that they are, it takes everything under them. Pinned to the
    // bottom with the cards at the top, the page had a dead band across its
    // middle and a matrix two rows tall.
    let matrix = if matrix.is_empty() {
        matrix
    } else {
        let cards_bottom = cards
            .iter()
            .map(|c| c.frame.bottom())
            .fold(body.y, f32::max);
        let top = (cards_bottom + CARD_GAP).min(matrix.y);
        Rect::new(
            matrix.x,
            top,
            matrix.width,
            (matrix.bottom() - top).max(0.0),
        )
    };
    let (routes, matrix_max_scroll, matrix_scrollbar) =
        matrix_rows(matrix, view.routes.len(), view.matrix_scroll);
    let add_effect = add_effect_button(body, view, &cards);
    FlopsynthLayout {
        body,
        whole,
        tabs,
        canopy,
        cards,
        badges,
        matrix,
        routes,
        matrix_max_scroll,
        matrix_scrollbar,
        presets: PresetsLayout::default(),
        add_effect,
    }
}

/// Places `cards` in `body`, giving something up and going again until they
/// fit — the air first, then the pictures to their soft floor, then every cell
/// together down to [`CELL_FLOOR`], and last the pictures to [`PICTURE_FLOOR`].
///
/// Its own function so a **second** window can be built out of the same
/// cards: `docs/tune-plan.md` §7.2 says the corrector's console uses this
/// grid, this shrink order and these floors, and two copies of that would be
/// two windows that stopped agreeing about what a knob is.
pub fn fit_cards(body: Rect, cards: &[FlopsynthCard]) -> Vec<CardLayout> {
    let mut picture_height = PICTURE_HEIGHT;
    let mut scale = 1.0f32;
    let mut placed;
    loop {
        placed = place(body, cards, picture_height, scale);
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

fn wanted(card: &FlopsynthCard, picture_height: f32, scale: f32) -> Wanted {
    let (cell_w, cell_h) = (FLOP_CELL_W * scale, FLOP_CELL_H * scale);
    let columns = match card.columns {
        0 => columns_for(card),
        n => n,
    }
    .max(1);
    let picture = if card.picture.is_none() {
        0.0
    } else {
        picture_height + CARD_PAD
    };
    let top = CARD_HEADER + CARD_PAD + picture;

    // The cells flow across the rows, and a double cell that would not fit
    // at the end of a row starts the next one rather than hanging off the
    // card. The card's **kind** chooser is not in the flow at all: it sits
    // on the nameplate, at the right end of the header — see
    // [`is_nameplate_control`].
    let mut cells = Vec::with_capacity(card.group.params.len());
    let (mut column, mut row) = (0usize, 0usize);
    let width = CARD_PAD * 2.0 + columns as f32 * cell_w;
    for (index, param) in card.group.params.iter().enumerate() {
        if is_nameplate_control(param) {
            let chip_w = (NAMEPLATE_CHIP_W * scale).min(width - CARD_PAD * 2.0);
            cells.push((
                index,
                Rect::new(width - CARD_PAD - chip_w, 0.0, chip_w, CARD_HEADER),
            ));
            continue;
        }
        let span = cell_span(param).min(columns);
        if column + span > columns {
            column = 0;
            row += 1;
        }
        cells.push((
            index,
            Rect::new(
                CARD_PAD + column as f32 * cell_w,
                top + row as f32 * cell_h,
                cell_w * span as f32,
                cell_h,
            ),
        ));
        column += span;
    }
    let rows = if card.group.params.iter().all(is_nameplate_control) {
        0
    } else {
        row + 1
    };
    Wanted {
        width,
        height: top + rows as f32 * cell_h + CARD_PAD,
        cells,
        picture: if card.picture.is_none() {
            0.0
        } else {
            picture_height
        },
        removable: card.removable,
    }
}

/// Puts a wanted card at `(x, y)`.
fn placed(frame_x: f32, frame_y: f32, width: f32, want: &Wanted) -> CardLayout {
    let frame = Rect::new(frame_x, frame_y, width, want.height);
    let header = Rect::new(frame.x, frame.y, frame.width, CARD_HEADER);
    let picture = if want.picture <= 0.0 {
        Rect::ZERO
    } else {
        Rect::new(
            frame.x + CARD_PAD,
            frame.y + CARD_HEADER + CARD_PAD,
            (frame.width - CARD_PAD * 2.0).max(0.0),
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
            header.right() - CARD_PAD - REMOVE_SIZE,
            header.y + (header.height - REMOVE_SIZE) / 2.0,
            REMOVE_SIZE,
            REMOVE_SIZE,
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
    picture_height: f32,
    scale: f32,
) -> Vec<CardLayout> {
    let wants: Vec<Wanted> = cards_in
        .iter()
        .map(|card| wanted(card, picture_height, scale))
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
            cards[*index] = Some(placed(body.right() - aside_width, y, aside_width, want));
            y += want.height + CARD_GAP;
            aside_bottom = y - CARD_GAP;
        }
    }
    // The room the bands have at a given height: short of the column while
    // the column is beside them, the whole body below it.
    let right_at = |y: f32| {
        if !aside.is_empty() && y < aside_bottom - 0.01 {
            body.right() - aside_width - CARD_GAP
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
                y += row_height + CARD_GAP;
                row_height = 0.0;
            }
        }
        // Wrap when this card would run off the right — but never on the first
        // card of a row, or a card wider than the panel would loop for ever.
        if x > body.x && x + want.width > right_at(y) + 0.01 {
            x = body.x;
            y += row_height + CARD_GAP;
            row_height = 0.0;
        }
        let width = want.width.min((right_at(y) - x).max(0.0)).min(body.width);
        cards[index] = Some(placed(x, y, width, want));
        row_height = row_height.max(want.height);
        x += width + CARD_GAP;
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
/// `scroll` (clamped to what is hidden); with how far it could scroll and
/// the thumb that says where it is.
///
/// A row is drawn **whole or not at all**: the list under the heading is a
/// whole number of rows, a row scrolled above it or below the panel's foot
/// is an empty rect, and the thumb's length is the share of the rows on
/// show. Half a row at either end was tried and read as a row cut off.
fn matrix_rows(panel: Rect, count: usize, scroll: f32) -> (Vec<MatrixRow>, f32, Rect) {
    if panel.is_empty() {
        return (Vec::new(), 0.0, Rect::ZERO);
    }
    // The four columns, as shares of the row: the two names take most of it,
    // the depth slider is fixed because a slider that changed length would
    // change what a pixel is worth, and the ✕ is square.
    const REMOVE: f32 = 18.0;
    const DEPTH: f32 = 110.0;
    let list_top = panel.y + CARD_HEADER + CARD_PAD;
    let list_height = (panel.bottom() - CARD_PAD - list_top).max(0.0);
    let fit = ((list_height + 0.01) / MATRIX_ROW).floor().max(0.0) as usize;
    let hidden = count.saturating_sub(fit);
    let max_scroll = hidden as f32 * MATRIX_ROW;
    // Whole rows: a scroll between rows is rounded to the nearer one.
    let scroll = (scroll.clamp(0.0, max_scroll) / MATRIX_ROW).round() * MATRIX_ROW;
    let first = (scroll / MATRIX_ROW).round() as usize;
    // The thumb, down the right edge of the list, in a lane the rows stop
    // short of so it covers no row's ✕.
    let lane = if hidden == 0 { 0.0 } else { 8.0 };
    let scrollbar = if hidden == 0 || count == 0 {
        Rect::ZERO
    } else {
        let track = Rect::new(panel.right() - CARD_PAD - 4.0, list_top, 4.0, list_height);
        let length = (track.height * fit as f32 / count as f32).max(12.0);
        let travel = (track.height - length).max(0.0);
        let at = if max_scroll > 0.0 {
            travel * (scroll / max_scroll)
        } else {
            0.0
        };
        Rect::new(track.x, track.y + at, track.width, length).intersection(&panel)
    };
    let rows = (0..count)
        .map(|index| {
            let frame = if index < first || index >= first + fit {
                Rect::ZERO
            } else {
                Rect::new(
                    panel.x + CARD_PAD,
                    list_top + (index - first) as f32 * MATRIX_ROW,
                    (panel.width - CARD_PAD * 2.0 - lane).max(0.0),
                    MATRIX_ROW,
                )
                .intersection(&panel)
            };
            let remove = Rect::new(
                frame.right() - REMOVE,
                frame.y + (frame.height - REMOVE) / 2.0,
                REMOVE,
                REMOVE,
            )
            .intersection(&frame);
            let depth = Rect::new(
                (remove.x - CARD_GAP - DEPTH).max(frame.x),
                frame.y + 4.0,
                DEPTH.min((remove.x - CARD_GAP - frame.x).max(0.0)),
                (frame.height - 8.0).max(0.0),
            )
            .intersection(&frame);
            let names = (depth.x - frame.x - CARD_GAP).max(0.0);
            let source = Rect::new(frame.x, frame.y, names * 0.4, frame.height);
            let destination = Rect::new(frame.x + names * 0.4, frame.y, names * 0.6, frame.height);
            MatrixRow {
                frame,
                source,
                destination,
                depth,
                remove,
            }
        })
        .collect();
    (rows, max_scroll, scrollbar)
}

/// Which page's tab is under `(x, y)`.
pub fn flopsynth_tab_at(layout: &FlopsynthLayout, x: f32, y: f32) -> Option<FlopsynthPage> {
    layout
        .tabs
        .iter()
        .find(|(_, rect)| !rect.is_empty() && rect.contains(x, y))
        .map(|(page, _)| *page)
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
    for (index, row) in layout.routes.iter().enumerate() {
        if !row.remove.is_empty() && row.remove.contains(x, y) {
            return Some(MatrixHit::Remove(index));
        }
        if !row.depth.is_empty() && row.depth.contains(x, y) {
            return Some(MatrixHit::Depth(index));
        }
    }
    None
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
    for (card, placed) in layout.cards.iter().enumerate() {
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

/// Whether `(x, y)` is on this control's modulation ring.
pub fn ring_hit(cell: Rect, x: f32, y: f32) -> bool {
    let knob = flop_knob_rect(cell);
    if knob.is_empty() {
        return false;
    }
    let (cx, cy) = (knob.x + knob.width / 2.0, knob.y + knob.height / 2.0);
    let distance = ((x - cx).powi(2) + (y - cy).powi(2)).sqrt();
    let inner = knob.width / 2.0 + RING_GAP;
    (inner..=inner + RING_BAND).contains(&distance)
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
