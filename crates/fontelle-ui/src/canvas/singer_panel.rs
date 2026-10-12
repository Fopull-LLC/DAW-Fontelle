//! The Fontelle Singer's instrument panel: a vocal track's instrument,
//! opened.
//!
//! > *"theres no instrument visuals for each yet, please go and create the
//! > generic vocal plugin instrument fully ... it should have its own
//! > visuals and knobs and stuff in the instrument that people would
//! > expect"* — Ty
//!
//! Two parts, one above the other. **The stage**: who sings — a big
//! portrait, the name, the instrument and kind, the author, a chip per
//! language (choosing it), the terms, and *Change singer…*, *▶ Sample*,
//! *Open <engine>*. **The cards**: one per section of controls (Voice,
//! Expression, Performance, Output — whatever groups the host's
//! [`InstrumentView`] has), flowing into two columns when there is room,
//! the Expression card with a picture of the vibrato and glide it gives,
//! and a card whose controls the voice ignores saying why under them.
//!
//! The controls are an ordinary [`InstrumentLayout`] — the same cells the
//! knob grid has — so every gesture the instrument window knows (a drag, a
//! fine drag, Alt-click, a typed value, a drop-down, the right-click menu)
//! works here unchanged; only *where* the cells are is this module's.
//!
//! Geometry and hit-testing only, and both pure, for the reason §2.5 gives.

use crate::document::SingerPanelView;
use crate::layout::Rect;
use crate::theme::Metrics;

use super::{CELL_HEIGHT, CELL_WIDTH, InstrumentLayout, InstrumentView};

/// Around the panel, and between the stage and the cards and between cards.
pub const SINGER_PANEL_PAD: f32 = 14.0;
/// The portrait's side.
pub const SINGER_PORTRAIT: f32 = 132.0;
/// Inside the stage, around the portrait and the words.
const STAGE_PAD: f32 = 16.0;
/// A card's nameplate.
pub const CARD_HEADING: f32 = 30.0;
/// Inside a card, either side of its cells.
const CARD_PAD: f32 = 12.0;
/// Between cells.
const CELL_GAP: f32 = 6.0;
/// The picture of the vibrato and glide on the Expression card.
pub const SINGER_PICTURE_HEIGHT: f32 = 64.0;
/// The line under a card saying why its greyed controls are greyed.
pub const FOOTNOTE_HEIGHT: f32 = 22.0;
/// The stage's text lines and chips.
const KICKER: f32 = 18.0;
const NAME: f32 = 30.0;
const DETAILS: f32 = 20.0;
const CHIP: f32 = 26.0;
const BUTTON: f32 = 30.0;
const BUTTON_GAP: f32 = 8.0;
/// The size the instrument window opens at for a vocal track: two columns
/// of cards under the stage, every control at its design size.
pub const SINGER_PANEL_SIZE: (u32, u32) = (980, 780);

/// A button on the stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SingerPanelButton {
    /// Opens the singer picker for this track's instrument.
    ChangeSinger,
    /// Plays the bank's own sample.
    Sample,
    /// Opens the engine's own window.
    OpenEditor,
    /// The Voicebanks page: install, find, uninstall.
    Voicebanks,
    /// The Voicebanks page's Find singers tab: offered instead when there
    /// is nothing installed to choose, or the track's singer is missing.
    FindSingers,
}

impl SingerPanelButton {
    /// What it says. `instrument` names the engine whose window it opens.
    pub fn label(self, instrument: &str) -> String {
        match self {
            Self::ChangeSinger => "Change singer\u{2026}".to_string(),
            Self::Sample => "\u{25b6} Sample".to_string(),
            Self::OpenEditor => format!("Open {instrument}"),
            Self::Voicebanks => "Voicebanks\u{2026}".to_string(),
            Self::FindSingers => "Find singers\u{2026}".to_string(),
        }
    }

    /// How wide it is drawn: room for its word at the stage's text size.
    fn width(self, instrument: &str) -> f32 {
        let chars = self.label(instrument).chars().count() as f32;
        (chars * 7.6 + 32.0).clamp(96.0, 240.0)
    }
}

/// What a press on the panel's own chrome landed on. A press on a control
/// is not one of these: it is [`super::instrument_hit`]'s, on
/// [`SingerPanelLayout::controls`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SingerPanelHit {
    Button(SingerPanelButton),
    /// The singer's language chip `n` (into `SingerChoice::languages`).
    Language(usize),
    /// The terms chip: read them.
    Terms,
    /// The portrait: the picker, like Change singer.
    Portrait,
}

/// Where everything on the panel is.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SingerPanelLayout {
    pub body: Rect,
    /// The stage, across the top.
    pub header: Rect,
    pub portrait: Rect,
    /// "FONTELLE SINGER · UTAU", over the name.
    pub kicker: Rect,
    pub name: Rect,
    /// The author and the bank's folder.
    pub details: Rect,
    /// A chip per language the singer sings, in its order.
    pub languages: Vec<(usize, Rect)>,
    /// The terms chip, when the singer has terms.
    pub terms: Option<Rect>,
    pub buttons: Vec<(SingerPanelButton, Rect)>,
    /// A card per group of the controls, by group index.
    pub cards: Vec<(usize, Rect)>,
    /// The picture of the vibrato and glide, on the card that has them.
    pub picture: Option<Rect>,
    /// The why-line under a card with a greyed control, by group.
    pub footnotes: Vec<(usize, Rect)>,
    /// Which controls are greyed, as `(group, param)`.
    pub greyed: Vec<(usize, usize)>,
    /// The cells, laid out in the cards: the instrument window's gestures
    /// read this exactly as they read the knob grid's.
    pub controls: InstrumentLayout,
    /// How tall all of it is, unscrolled — for the scroll's limit.
    pub content_height: f32,
    /// The scroll the layout was made at, after clamping.
    pub scroll: f32,
}

/// Which group is the one that gets the picture: the one holding the
/// vibrato's depth.
fn picture_group(controls: &InstrumentView) -> Option<usize> {
    controls.groups.iter().position(|g| {
        g.params
            .iter()
            .any(|p| p.address.as_str() == "vocal/vibrato/depth")
    })
}

/// Cells across a card `width` wide, at least one.
fn per_row(width: f32) -> usize {
    (((width - 2.0 * CARD_PAD + CELL_GAP) / (CELL_WIDTH + CELL_GAP)).floor() as usize).max(1)
}

/// How many cells wide a control is: two for one whose caption or longest
/// option would be cut off in one ("Pitch expressiveness", "Automatic
/// (Japanese VCV)").
pub fn cell_span(param: &super::InstrumentParam) -> usize {
    let longest = match &param.kind {
        super::ParamKind::Choice(options) => {
            options.iter().map(|o| o.chars().count()).max().unwrap_or(0)
        }
        _ => param.display.chars().count(),
    };
    if param.label.chars().count() > 13 || longest > 13 {
        2
    } else {
        1
    }
}

/// Where each control goes in a card `across` cells wide: `(column, row,
/// span)`, in order, a double cell starting a new row when it would not fit.
fn place(params: &[super::InstrumentParam], across: usize) -> Vec<(usize, usize, usize)> {
    let mut out = Vec::new();
    let (mut col, mut row) = (0, 0);
    for p in params {
        let span = cell_span(p).min(across);
        if col + span > across {
            col = 0;
            row += 1;
        }
        out.push((col, row, span));
        col += span;
    }
    out
}

/// How many rows `params` take `across` cells wide.
fn rows_of(params: &[super::InstrumentParam], across: usize) -> usize {
    place(params, across)
        .last()
        .map_or(0, |(_, row, _)| row + 1)
}

/// How tall a card of `params` is at `width`.
fn card_height(
    width: f32,
    params: &[super::InstrumentParam],
    picture: bool,
    footnote: bool,
) -> f32 {
    let rows = rows_of(params, per_row(width)) as f32;
    CARD_HEADING
        + if picture {
            SINGER_PICTURE_HEIGHT + CELL_GAP
        } else {
            0.0
        }
        + rows * (CELL_HEIGHT + CELL_GAP)
        + if footnote { FOOTNOTE_HEIGHT } else { 0.0 }
        + CARD_PAD / 2.0
}

/// The stage's buttons for `view`.
fn buttons_for(view: &SingerPanelView) -> Vec<SingerPanelButton> {
    let mut out = vec![SingerPanelButton::ChangeSinger];
    if view.sample {
        out.push(SingerPanelButton::Sample);
    }
    if view.has_editor {
        out.push(SingerPanelButton::OpenEditor);
    }
    if view.engine.is_none() {
        out.push(if view.find_first {
            SingerPanelButton::FindSingers
        } else {
            SingerPanelButton::Voicebanks
        });
    }
    out
}

/// How wide a language chip is.
pub fn language_chip_width(tag: &str) -> f32 {
    (tag.chars().count() as f32 * 9.0 + 22.0).max(40.0)
}

/// How wide the terms chip is.
pub const TERMS_CHIP_WIDTH: f32 = 128.0;

/// Lays the panel out in `body`, scrolled down by `scroll` (clamped to what
/// there is).
pub fn singer_panel_layout(
    body: Rect,
    _metrics: &Metrics,
    view: &SingerPanelView,
    controls: &InstrumentView,
    scroll: f32,
) -> SingerPanelLayout {
    if body.is_empty() {
        return SingerPanelLayout {
            body,
            ..Default::default()
        };
    }
    // Laid out from the top unscrolled first, then moved: the clamp needs
    // the height, and the height needs the layout.
    let mut out = lay_out(body, view, controls);
    let room = (out.content_height - body.height).max(0.0);
    let scroll = if scroll.is_finite() {
        scroll.clamp(0.0, room)
    } else {
        0.0
    };
    if scroll > 0.0 {
        let up = |r: &mut Rect| {
            if !r.is_empty() {
                r.y -= scroll;
            }
        };
        up(&mut out.header);
        up(&mut out.portrait);
        up(&mut out.kicker);
        up(&mut out.name);
        up(&mut out.details);
        out.languages.iter_mut().for_each(|(_, r)| up(r));
        if let Some(r) = &mut out.terms {
            up(r);
        }
        out.buttons.iter_mut().for_each(|(_, r)| up(r));
        out.cards.iter_mut().for_each(|(_, r)| up(r));
        if let Some(r) = &mut out.picture {
            up(r);
        }
        out.footnotes.iter_mut().for_each(|(_, r)| up(r));
        out.controls.headings.iter_mut().for_each(|(_, r)| up(r));
        out.controls.cells.iter_mut().for_each(|(_, _, r)| up(r));
    }
    out.scroll = scroll;
    out
}

fn lay_out(body: Rect, view: &SingerPanelView, controls: &InstrumentView) -> SingerPanelLayout {
    let inner = Rect::new(
        body.x + SINGER_PANEL_PAD,
        body.y + SINGER_PANEL_PAD,
        (body.width - 2.0 * SINGER_PANEL_PAD).max(0.0),
        body.height,
    );

    // --- the stage
    let portrait = Rect::new(
        inner.x + STAGE_PAD,
        inner.y + STAGE_PAD,
        SINGER_PORTRAIT,
        SINGER_PORTRAIT,
    );
    let text_x = portrait.right() + 18.0;
    let text_w = (inner.right() - STAGE_PAD - text_x).max(0.0);
    let mut y = portrait.y;
    let kicker = Rect::new(text_x, y, text_w, KICKER);
    y += KICKER + 2.0;
    let name = Rect::new(text_x, y, text_w, NAME);
    y += NAME + 2.0;
    let details = Rect::new(text_x, y, text_w, DETAILS);
    y += DETAILS + 8.0;
    // Chips: a language each, then the terms — wrapping inside the column.
    let mut languages = Vec::new();
    let mut x = text_x;
    let languages_of: &[String] = view.singer.as_ref().map_or(&[], |s| s.languages.as_slice());
    let mut row_used = false;
    for (n, tag) in languages_of.iter().enumerate() {
        let w = language_chip_width(tag);
        if row_used && x + w > text_x + text_w {
            x = text_x;
            y += CHIP + 6.0;
        }
        languages.push((n, Rect::new(x, y, w, CHIP)));
        x += w + 6.0;
        row_used = true;
    }
    let terms = view.singer.as_ref().filter(|s| s.terms.is_some()).map(|_| {
        if row_used && x + 4.0 + TERMS_CHIP_WIDTH > text_x + text_w {
            x = text_x;
            y += CHIP + 6.0;
        } else if row_used {
            x += 4.0;
        }
        let r = Rect::new(x, y, TERMS_CHIP_WIDTH, CHIP);
        row_used = true;
        r
    });
    if row_used {
        y += CHIP + 10.0;
    }
    // Buttons, wrapping the same way.
    let mut buttons = Vec::new();
    let mut x = text_x;
    let mut first = true;
    for b in buttons_for(view) {
        let w = b.width(&view.instrument).min(text_w.max(1.0));
        if !first && x + w > text_x + text_w {
            x = text_x;
            y += BUTTON + BUTTON_GAP;
        }
        buttons.push((b, Rect::new(x, y, w, BUTTON)));
        x += w + BUTTON_GAP;
        first = false;
    }
    y += BUTTON;
    let stage_bottom = (portrait.bottom()).max(y) + STAGE_PAD;
    let header = Rect::new(inner.x, inner.y, inner.width, stage_bottom - inner.y);

    // --- the cards
    let picture_in = picture_group(controls);
    let greyed: Vec<(usize, usize)> = controls
        .groups
        .iter()
        .enumerate()
        .flat_map(|(g, group)| {
            group
                .params
                .iter()
                .enumerate()
                .filter(|(_, p)| view.greyed.iter().any(|(a, _)| a == p.address.as_str()))
                .map(move |(n, _)| (g, n))
        })
        .collect();
    let has_footnote = |g: usize| greyed.iter().any(|(gg, _)| *gg == g);
    let min_card = 2.0 * CARD_PAD + 4.0 * CELL_WIDTH + 3.0 * CELL_GAP;
    let columns = if inner.width >= 2.0 * min_card + SINGER_PANEL_PAD {
        2
    } else {
        1
    };
    let card_w =
        ((inner.width - (columns - 1) as f32 * SINGER_PANEL_PAD) / columns as f32).max(0.0);
    let top = header.bottom() + SINGER_PANEL_PAD;
    let mut bottoms = vec![top; columns];
    let mut cards = Vec::new();
    let mut picture = None;
    let mut footnotes = Vec::new();
    let mut headings = Vec::new();
    let mut cells = Vec::new();
    for (g, group) in controls.groups.iter().enumerate() {
        // The shortest column takes the next card (the leftmost on a tie).
        let column = (0..columns)
            .min_by(|a, b| bottoms[*a].total_cmp(&bottoms[*b]))
            .unwrap_or(0);
        let x = inner.x + column as f32 * (card_w + SINGER_PANEL_PAD);
        let y = bottoms[column];
        let with_picture = picture_in == Some(g);
        let h = card_height(card_w, &group.params, with_picture, has_footnote(g));
        let card = Rect::new(x, y, card_w, h);
        cards.push((g, card));
        headings.push((g, Rect::new(x, y, card_w, CARD_HEADING)));
        let mut cy = y + CARD_HEADING;
        if with_picture {
            picture = Some(Rect::new(
                x + CARD_PAD,
                cy,
                (card_w - 2.0 * CARD_PAD).max(0.0),
                SINGER_PICTURE_HEIGHT,
            ));
            cy += SINGER_PICTURE_HEIGHT + CELL_GAP;
        }
        let across = per_row(card_w);
        let placed = place(&group.params, across);
        // The cells centred in the card, so a card wider than its cells is
        // not all on one side.
        let widest = placed
            .iter()
            .map(|(col, _, span)| col + span)
            .max()
            .unwrap_or(1);
        let used = widest as f32 * (CELL_WIDTH + CELL_GAP) - CELL_GAP;
        let left = x + CARD_PAD + ((card_w - 2.0 * CARD_PAD - used) / 2.0).max(0.0);
        for (n, (col, row, span)) in placed.iter().enumerate() {
            let cx = left + *col as f32 * (CELL_WIDTH + CELL_GAP);
            let w = *span as f32 * (CELL_WIDTH + CELL_GAP) - CELL_GAP;
            let cell = Rect::new(
                cx,
                cy + *row as f32 * (CELL_HEIGHT + CELL_GAP),
                w.min((card.right() - CARD_PAD - cx).max(0.0)),
                CELL_HEIGHT,
            );
            cells.push((g, n, cell));
        }
        let rows = rows_of(&group.params, across) as f32;
        cy += rows * (CELL_HEIGHT + CELL_GAP);
        if has_footnote(g) {
            footnotes.push((
                g,
                Rect::new(
                    x + CARD_PAD,
                    cy,
                    (card_w - 2.0 * CARD_PAD).max(0.0),
                    FOOTNOTE_HEIGHT,
                ),
            ));
        }
        bottoms[column] = card.bottom() + SINGER_PANEL_PAD;
    }
    let bottom = bottoms.iter().copied().fold(header.bottom(), f32::max);
    let content_height = (bottom - body.y).max(0.0);
    SingerPanelLayout {
        body,
        header,
        portrait,
        kicker,
        name,
        details,
        languages,
        terms,
        buttons,
        cards,
        picture,
        footnotes,
        greyed,
        controls: InstrumentLayout {
            body,
            name: Rect::ZERO,
            keys: Vec::new(),
            headings,
            cells,
            content_height,
        },
        content_height,
        scroll: 0.0,
    }
}

/// What a press at `(x, y)` lands on among the stage's chrome, if anything.
/// Nothing outside the panel's body (scrolled away) is pressed.
pub fn singer_panel_hit(layout: &SingerPanelLayout, x: f32, y: f32) -> Option<SingerPanelHit> {
    if !layout.body.contains(x, y) {
        return None;
    }
    if let Some((b, _)) = layout.buttons.iter().find(|(_, r)| r.contains(x, y)) {
        return Some(SingerPanelHit::Button(*b));
    }
    if let Some((n, _)) = layout.languages.iter().find(|(_, r)| r.contains(x, y)) {
        return Some(SingerPanelHit::Language(*n));
    }
    if layout.terms.is_some_and(|r| r.contains(x, y)) {
        return Some(SingerPanelHit::Terms);
    }
    if layout.portrait.contains(x, y) {
        return Some(SingerPanelHit::Portrait);
    }
    None
}

/// What the line under card `group` says: why its greyed controls are
/// greyed — "重音テト doesn't use gender (formant), tension" when they share
/// a reason's start, each reason in turn otherwise. `None` with nothing
/// greyed.
pub fn singer_footnote(
    view: &SingerPanelView,
    controls: &InstrumentView,
    group: usize,
) -> Option<String> {
    let mut whys: Vec<&str> = Vec::new();
    for param in &controls.groups.get(group)?.params {
        if let Some((_, why)) = view
            .greyed
            .iter()
            .find(|(a, _)| a == param.address.as_str())
            && !whys.contains(&why.as_str())
        {
            whys.push(why);
        }
    }
    let first = *whys.first()?;
    const JOIN: &str = " doesn't use ";
    if let Some(at) = first.find(JOIN) {
        let head = &first[..at + JOIN.len()];
        if whys.iter().all(|w| w.starts_with(head)) {
            let tails: Vec<&str> = whys.iter().map(|w| &w[head.len()..]).collect();
            return Some(format!("{head}{}", tails.join(", ")));
        }
    }
    Some(whys.join("; "))
}
