//! The Voicebanks page: a card over the window with two tabs — what is
//! **installed** (each bank's portrait, name, kind, its terms and whether
//! they were agreed to, Uninstall) and **Find singers** (the catalogue: who,
//! what kind, the author's terms in brief and what they allow, Install or
//! Get it). Geometry and hits; drawn by `render::vocal`, driven by the
//! window.

use crate::layout::Rect;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VoicebankTab {
    #[default]
    Installed,
    Find,
}

/// One row of the list.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageRow {
    /// Into the list the page was laid out for.
    pub index: usize,
    pub frame: Rect,
    pub portrait: Rect,
    /// Name, kind, languages, author.
    pub text: Rect,
    /// Read the terms / agree.
    pub terms: Rect,
    /// Uninstall, Install, Get it.
    pub action: Rect,
    /// Installed only: show the bank's folder.
    pub reveal: Rect,
    /// Installed only: play the sample its author named (drawn only when
    /// there is one).
    pub sample: Rect,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VoicebankPageLayout {
    pub frame: Rect,
    pub title: Rect,
    pub tabs: [(VoicebankTab, Rect); 2],
    pub close: Rect,
    /// "Install from file…".
    pub install: Rect,
    /// "Open folder".
    pub folder: Rect,
    /// Where the rows scroll.
    pub list: Rect,
    /// Only the rows that can be seen.
    pub rows: Vec<PageRow>,
    /// How tall all the rows are, for the scroll.
    pub content_height: f32,
}

/// A row: a portrait as big as the singer picker's (the same bank looks
/// the same in both), its name, its folder, what it is.
pub const PAGE_ROW_HEIGHT: f32 = 152.0;
/// The portrait on a row, as on a picker card.
pub const PAGE_PORTRAIT: f32 = super::singer_picker::PORTRAIT_SIZE;
const BUTTON_HEIGHT: f32 = 32.0;

/// The page over `window`, showing `tab` with `count` rows scrolled by
/// `scroll` pixels.
pub fn voicebank_page_layout(
    window: Rect,
    tab: VoicebankTab,
    count: usize,
    scroll: f32,
) -> VoicebankPageLayout {
    // Both tabs are laid out alike but for the installed rows' folder and
    // sample buttons; the tab otherwise only changes what the rows say.
    let width = (window.width - 48.0).clamp(320.0, 1180.0).min(window.width);
    let height = (window.height - 48.0)
        .clamp(240.0, 980.0)
        .min(window.height);
    let frame = Rect::new(
        window.x + (window.width - width) / 2.0,
        window.y + (window.height - height) / 2.0,
        width,
        height,
    );
    let pad = 20.0;
    let title = Rect::new(
        frame.x + pad,
        frame.y + pad,
        frame.width - 2.0 * pad - 40.0,
        30.0,
    );
    let close = Rect::new(frame.right() - pad - 32.0, frame.y + pad, 32.0, 32.0);
    let tab_y = title.bottom() + 10.0;
    let tabs = [
        (
            VoicebankTab::Installed,
            Rect::new(frame.x + pad, tab_y, 130.0, BUTTON_HEIGHT),
        ),
        (
            VoicebankTab::Find,
            Rect::new(frame.x + pad + 138.0, tab_y, 150.0, BUTTON_HEIGHT),
        ),
    ];
    let folder = Rect::new(frame.right() - pad - 140.0, tab_y, 140.0, BUTTON_HEIGHT);
    let install = Rect::new(folder.x - 8.0 - 200.0, tab_y, 200.0, BUTTON_HEIGHT);
    let list_top = tab_y + BUTTON_HEIGHT + 14.0;
    let list = Rect::new(
        frame.x + pad,
        list_top,
        frame.width - 2.0 * pad,
        (frame.bottom() - pad - list_top).max(0.0),
    );
    let content_height = count as f32 * PAGE_ROW_HEIGHT;
    let max_scroll = (content_height - list.height).max(0.0);
    let scroll = scroll.clamp(0.0, max_scroll);
    let mut rows = Vec::new();
    for index in 0..count {
        let y = list.y + index as f32 * PAGE_ROW_HEIGHT - scroll;
        if y + PAGE_ROW_HEIGHT <= list.y || y >= list.bottom() {
            continue;
        }
        let row = Rect::new(list.x, y, list.width, PAGE_ROW_HEIGHT - 8.0);
        let inset = (row.height - PAGE_PORTRAIT) / 2.0;
        let portrait = Rect::new(row.x + inset, row.y + inset, PAGE_PORTRAIT, PAGE_PORTRAIT);
        // The buttons beside the words; on a narrow page (a small window,
        // a large UI scale) along the row's foot, under them, so the words
        // keep the row's width.
        let buttons = if tab == VoicebankTab::Installed {
            120.0 + 148.0 + 92.0 + 108.0
        } else {
            120.0 + 148.0
        };
        let narrow = row.width - PAGE_PORTRAIT - inset - 28.0 - buttons < 260.0;
        let button_y = if narrow {
            row.bottom() - inset - BUTTON_HEIGHT
        } else {
            row.y + (row.height - BUTTON_HEIGHT) / 2.0
        };
        let action = Rect::new(row.right() - 12.0 - 120.0, button_y, 120.0, BUTTON_HEIGHT);
        let terms = Rect::new(action.x - 148.0, action.y, 140.0, BUTTON_HEIGHT);
        let (reveal, sample) = if tab == VoicebankTab::Installed {
            let reveal = Rect::new(terms.x - 92.0, action.y, 84.0, BUTTON_HEIGHT);
            let sample = Rect::new(reveal.x - 108.0, action.y, 100.0, BUTTON_HEIGHT);
            (reveal, sample)
        } else {
            (Rect::default(), Rect::default())
        };
        let words_end = if narrow {
            row.right() - 12.0
        } else if tab == VoicebankTab::Installed {
            sample.x - 12.0
        } else {
            terms.x - 12.0
        };
        let text_bottom = if narrow {
            button_y - 4.0
        } else {
            row.y + inset + PAGE_PORTRAIT
        };
        let text = Rect::new(
            portrait.right() + 16.0,
            row.y + inset,
            (words_end - portrait.right() - 16.0).max(0.0),
            (text_bottom - row.y - inset).max(0.0),
        );
        rows.push(PageRow {
            index,
            frame: row,
            portrait,
            text,
            terms,
            action,
            reveal,
            sample,
        });
    }
    VoicebankPageLayout {
        frame,
        title,
        tabs,
        close,
        install,
        folder,
        list,
        rows,
        content_height,
    }
}

/// What a press on the page lands on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageHit {
    Tab(VoicebankTab),
    Close,
    InstallFromFile,
    OpenFolder,
    Action(usize),
    Terms(usize),
    /// An installed bank's folder.
    Reveal(usize),
    /// An installed bank's sample.
    Sample(usize),
    Row(usize),
    /// On the page but on nothing.
    Page,
    /// Off the page: closes it.
    Outside,
}

pub fn voicebank_page_hit(page: &VoicebankPageLayout, x: f32, y: f32) -> Option<PageHit> {
    if !page.frame.contains(x, y) {
        return Some(PageHit::Outside);
    }
    if page.close.contains(x, y) {
        return Some(PageHit::Close);
    }
    for (tab, rect) in &page.tabs {
        if rect.contains(x, y) {
            return Some(PageHit::Tab(*tab));
        }
    }
    if page.install.contains(x, y) {
        return Some(PageHit::InstallFromFile);
    }
    if page.folder.contains(x, y) {
        return Some(PageHit::OpenFolder);
    }
    if page.list.contains(x, y) {
        for row in &page.rows {
            if row.action.contains(x, y) {
                return Some(PageHit::Action(row.index));
            }
            if row.terms.contains(x, y) {
                return Some(PageHit::Terms(row.index));
            }
            if !row.reveal.is_empty() && row.reveal.contains(x, y) {
                return Some(PageHit::Reveal(row.index));
            }
            if !row.sample.is_empty() && row.sample.contains(x, y) {
                return Some(PageHit::Sample(row.index));
            }
            if row.frame.contains(x, y) {
                return Some(PageHit::Row(row.index));
            }
        }
    }
    Some(PageHit::Page)
}
