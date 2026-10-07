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

pub const PAGE_ROW_HEIGHT: f32 = 76.0;

/// The page over `window`, showing `tab` with `count` rows scrolled by
/// `scroll` pixels.
pub fn voicebank_page_layout(
    window: Rect,
    tab: VoicebankTab,
    count: usize,
    scroll: f32,
) -> VoicebankPageLayout {
    // Both tabs are laid out alike; the tab only changes what the rows say.
    let _ = tab;
    let width = (window.width - 80.0).clamp(320.0, 860.0).min(window.width);
    let height = (window.height - 80.0)
        .clamp(240.0, 680.0)
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
    let tab_y = title.bottom() + 8.0;
    let tabs = [
        (
            VoicebankTab::Installed,
            Rect::new(frame.x + pad, tab_y, 110.0, 26.0),
        ),
        (
            VoicebankTab::Find,
            Rect::new(frame.x + pad + 116.0, tab_y, 120.0, 26.0),
        ),
    ];
    let install = Rect::new(frame.right() - pad - 300.0, tab_y, 170.0, 26.0);
    let folder = Rect::new(frame.right() - pad - 124.0, tab_y, 124.0, 26.0);
    let list = Rect::new(
        frame.x + pad,
        tab_y + 26.0 + 10.0,
        frame.width - 2.0 * pad,
        (frame.bottom() - pad - (tab_y + 36.0)).max(0.0),
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
        let row = Rect::new(list.x, y, list.width, PAGE_ROW_HEIGHT - 6.0);
        let portrait = Rect::new(
            row.x + 6.0,
            row.y + 6.0,
            row.height - 12.0,
            row.height - 12.0,
        );
        let action = Rect::new(
            row.right() - 112.0,
            row.y + (row.height - 26.0) / 2.0,
            104.0,
            26.0,
        );
        let terms = Rect::new(action.x - 126.0, action.y, 118.0, 26.0);
        let text = Rect::new(
            portrait.right() + 10.0,
            row.y + 4.0,
            (terms.x - portrait.right() - 18.0).max(0.0),
            row.height - 8.0,
        );
        rows.push(PageRow {
            index,
            frame: row,
            portrait,
            text,
            terms,
            action,
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
            if row.frame.contains(x, y) {
                return Some(PageHit::Row(row.index));
            }
        }
    }
    Some(PageHit::Page)
}
