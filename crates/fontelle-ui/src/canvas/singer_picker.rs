//! The singer picker: a card over the window with a grid of singers — each
//! one's portrait, name, kind (UTAU, DiffSinger, Engine), languages (a chip
//! each, which chooses that language), whether its terms are still to be
//! read and whether it is research-only. The Fontelle Singer's voicebanks
//! are a group of their own above the engines'. A search field narrows it
//! (name, kind, language by tag or name, engine); the arrow keys move
//! between cards as they are laid out, Enter chooses. Below: No singer,
//! Install a voicebank…, Voicebanks & Find singers…, and any engine that
//! did not load. Geometry, filtering and hits; drawn by `render::vocal`,
//! driven by the window.

use crate::document::SingerChoice;
use crate::layout::Rect;

/// The two groups, in the order they are shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerGroup {
    /// The Fontelle Singer's free voicebanks.
    Voicebanks,
    /// External singer engines.
    Engines,
}

/// One singer's card.
#[derive(Debug, Clone, PartialEq)]
pub struct PickerCard {
    /// Into the singer list the picker was laid out for.
    pub singer: usize,
    pub frame: Rect,
    pub portrait: Rect,
    /// A chip per language (its tag), which chooses that language.
    pub languages: Vec<(String, Rect)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SingerPickerLayout {
    pub frame: Rect,
    pub title: Rect,
    pub close: Rect,
    pub search: Rect,
    /// Where the groups and cards scroll.
    pub list: Rect,
    /// Each group shown and its heading.
    pub groups: Vec<(PickerGroup, Rect)>,
    /// Every card, in order (also those scrolled out of sight: the keys
    /// move by them).
    pub cards: Vec<PickerCard>,
    pub nobody: Rect,
    pub install: Rect,
    pub manage: Rect,
    /// A line for each engine that did not load.
    pub problems: Vec<Rect>,
    pub content_height: f32,
}

/// What a press on the picker lands on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PickerHit {
    /// A singer's card: sing with it (in the language the track has, if it
    /// sings that, else its first).
    Card(usize),
    /// One of its language chips.
    Language(usize, String),
    Search,
    Nobody,
    Install,
    Manage,
    Close,
    /// On the picker but on nothing.
    Page,
    /// Off it: closes it.
    Outside,
}

/// A key that moves the focused card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerMove {
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
}

pub const CARD_WIDTH: f32 = 200.0;
pub const CARD_HEIGHT: f32 = 128.0;
pub const CARD_GAP: f32 = 10.0;
pub const PORTRAIT_SIZE: f32 = 64.0;
const GROUP_HEIGHT: f32 = 24.0;
const CHIP_WIDTH: f32 = 30.0;
const CHIP_HEIGHT: f32 = 18.0;
const PROBLEM_HEIGHT: f32 = 18.0;

/// Which singers match `query`, voicebanks first then engines, each in the
/// order given.
pub fn picker_filter(singers: &[SingerChoice], query: &str) -> Vec<usize> {
    let q = query.trim().to_lowercase();
    let matches = |s: &SingerChoice| {
        if q.is_empty() {
            return true;
        }
        let mut hay = vec![s.name.to_lowercase(), s.kind.label().to_lowercase()];
        if !s.kind.is_voicebank() {
            hay.push(s.engine_name.to_lowercase());
            hay.push(s.engine.to_lowercase());
        }
        for l in &s.languages {
            hay.push(l.to_lowercase());
            hay.push(super::language_name(l).to_lowercase());
        }
        if s.research_only {
            hay.push("research".into());
        }
        hay.iter().any(|h| h.contains(&q))
    };
    let banks = (0..singers.len()).filter(|&i| singers[i].kind.is_voicebank());
    let engines = (0..singers.len()).filter(|&i| !singers[i].kind.is_voicebank());
    banks
        .chain(engines)
        .filter(|&i| matches(&singers[i]))
        .collect()
}

/// The picker over `window` showing `shown` (from [`picker_filter`]),
/// scrolled by `scroll` pixels, with `problems` lines of engines that did
/// not load.
pub fn singer_picker_layout(
    window: Rect,
    singers: &[SingerChoice],
    shown: &[usize],
    scroll: f32,
    problems: usize,
) -> SingerPickerLayout {
    let width = (window.width - 80.0).clamp(360.0, 980.0).min(window.width);
    let height = (window.height - 80.0)
        .clamp(300.0, 720.0)
        .min(window.height);
    let frame = Rect::new(
        window.x + (window.width - width) / 2.0,
        window.y + (window.height - height) / 2.0,
        width,
        height,
    );
    let pad = 14.0;
    let title = Rect::new(
        frame.x + pad,
        frame.y + pad,
        frame.width - 2.0 * pad - 30.0,
        24.0,
    );
    let close = Rect::new(frame.right() - pad - 24.0, frame.y + pad, 24.0, 24.0);
    let search = Rect::new(
        frame.x + pad,
        title.bottom() + 8.0,
        (frame.width - 2.0 * pad).min(360.0),
        28.0,
    );
    let footer_y = frame.bottom() - pad - 28.0;
    let nobody = Rect::new(frame.x + pad, footer_y, 110.0, 28.0);
    let install = Rect::new(nobody.right() + 8.0, footer_y, 180.0, 28.0);
    let manage = Rect::new(install.right() + 8.0, footer_y, 220.0, 28.0);
    let problems_top = footer_y - 6.0 - problems as f32 * PROBLEM_HEIGHT;
    let problem_rects = (0..problems)
        .map(|i| {
            Rect::new(
                frame.x + pad,
                problems_top + i as f32 * PROBLEM_HEIGHT,
                frame.width - 2.0 * pad,
                PROBLEM_HEIGHT,
            )
        })
        .collect();
    let list_top = search.bottom() + 10.0;
    let list = Rect::new(
        frame.x + pad,
        list_top,
        frame.width - 2.0 * pad,
        (problems_top - 6.0 - list_top).max(0.0),
    );
    let columns = (((list.width + CARD_GAP) / (CARD_WIDTH + CARD_GAP)).floor() as usize).max(1);
    let card_width = (list.width - CARD_GAP * (columns - 1) as f32) / columns as f32;
    // First where everything goes in content coordinates; then scrolled.
    let mut y = 0.0f32;
    let mut groups: Vec<(PickerGroup, f32)> = Vec::new();
    let mut cards: Vec<(usize, f32, f32)> = Vec::new();
    for group in [PickerGroup::Voicebanks, PickerGroup::Engines] {
        let members: Vec<usize> = shown
            .iter()
            .copied()
            .filter(|&i| {
                singers
                    .get(i)
                    .is_some_and(|s| s.kind.is_voicebank() == (group == PickerGroup::Voicebanks))
            })
            .collect();
        if members.is_empty() {
            continue;
        }
        groups.push((group, y));
        y += GROUP_HEIGHT;
        for (k, singer) in members.iter().enumerate() {
            let column = k % columns;
            let row = k / columns;
            cards.push((
                *singer,
                list.x + column as f32 * (card_width + CARD_GAP),
                y + row as f32 * (CARD_HEIGHT + CARD_GAP),
            ));
        }
        let rows = members.len().div_ceil(columns);
        y += rows as f32 * (CARD_HEIGHT + CARD_GAP) + 4.0;
    }
    let content_height = y;
    let scroll = scroll.clamp(0.0, (content_height - list.height).max(0.0));
    let at = |cy: f32| list.y + cy - scroll;
    let groups = groups
        .into_iter()
        .map(|(g, gy)| (g, Rect::new(list.x, at(gy), list.width, GROUP_HEIGHT)))
        .collect();
    let cards = cards
        .into_iter()
        .map(|(singer, x, cy)| {
            let frame = Rect::new(x, at(cy), card_width, CARD_HEIGHT);
            let portrait = Rect::new(frame.x + 10.0, frame.y + 10.0, PORTRAIT_SIZE, PORTRAIT_SIZE);
            let mut chip_x = frame.x + 10.0;
            let chip_y = frame.bottom() - 10.0 - CHIP_HEIGHT;
            let languages = singers
                .get(singer)
                .map(|s| s.languages.as_slice())
                .unwrap_or_default()
                .iter()
                .filter_map(|l| {
                    if chip_x + CHIP_WIDTH > frame.right() - 6.0 {
                        return None;
                    }
                    let r = Rect::new(chip_x, chip_y, CHIP_WIDTH, CHIP_HEIGHT);
                    chip_x += CHIP_WIDTH + 4.0;
                    Some((l.clone(), r))
                })
                .collect();
            PickerCard {
                singer,
                frame,
                portrait,
                languages,
            }
        })
        .collect();
    SingerPickerLayout {
        frame,
        title,
        close,
        search,
        list,
        groups,
        cards,
        nobody,
        install,
        manage,
        problems: problem_rects,
        content_height,
    }
}

pub fn singer_picker_hit(layout: &SingerPickerLayout, x: f32, y: f32) -> Option<PickerHit> {
    let l = layout;
    if !l.frame.contains(x, y) {
        return Some(PickerHit::Outside);
    }
    if l.close.contains(x, y) {
        return Some(PickerHit::Close);
    }
    if l.search.contains(x, y) {
        return Some(PickerHit::Search);
    }
    if l.nobody.contains(x, y) {
        return Some(PickerHit::Nobody);
    }
    if l.install.contains(x, y) {
        return Some(PickerHit::Install);
    }
    if l.manage.contains(x, y) {
        return Some(PickerHit::Manage);
    }
    if l.list.contains(x, y) {
        for card in &l.cards {
            if !card.frame.contains(x, y) {
                continue;
            }
            for (language, r) in &card.languages {
                if r.contains(x, y) {
                    return Some(PickerHit::Language(card.singer, language.clone()));
                }
            }
            return Some(PickerHit::Card(card.singer));
        }
    }
    Some(PickerHit::Page)
}

/// The card focused after `step` from `focus` (an index into the layout's
/// cards), moving by where the cards are.
pub fn picker_move(
    layout: &SingerPickerLayout,
    focus: Option<usize>,
    step: PickerMove,
) -> Option<usize> {
    let n = layout.cards.len();
    if n == 0 {
        return None;
    }
    let Some(at) = focus.filter(|f| *f < n) else {
        return Some(0);
    };
    let here = layout.cards[at].frame;
    let centre = |r: Rect| r.x + r.width / 2.0;
    let row = |above: bool| {
        // The nearest row above or below, then the card in it nearest across.
        let ys = layout.cards.iter().map(|c| c.frame.y).filter(|y| {
            if above {
                *y < here.y - 1.0
            } else {
                *y > here.y + 1.0
            }
        });
        let target = if above {
            ys.fold(f32::NEG_INFINITY, f32::max)
        } else {
            ys.fold(f32::INFINITY, f32::min)
        };
        if !target.is_finite() {
            return at;
        }
        (0..n)
            .filter(|&i| (layout.cards[i].frame.y - target).abs() < 1.0)
            .min_by(|&a, &b| {
                let da = (centre(layout.cards[a].frame) - centre(here)).abs();
                let db = (centre(layout.cards[b].frame) - centre(here)).abs();
                da.total_cmp(&db)
            })
            .unwrap_or(at)
    };
    Some(match step {
        PickerMove::Left => at.saturating_sub(1),
        PickerMove::Right => (at + 1).min(n - 1),
        PickerMove::Up => row(true),
        PickerMove::Down => row(false),
        PickerMove::Home => 0,
        PickerMove::End => n - 1,
    })
}
