//! The soundfont browser (TDD §17.5, item 9 of `docs/first-usable-plan.md`).
//!
//! The panel that makes Fontelle self-sufficient: a search box over the bank —
//! the folders the user drops `.sf2` files into — over the presets inside
//! whichever file is selected. Choosing one puts it on a channel, and nothing
//! about that involves a file path typed on a command line.
//!
//! The bank itself, the scan and the fuzzy search all live in `fontelle-app`
//! (`bank.rs`), because this crate may not read files. What is here is where
//! the rows go and which one was clicked — pure, and tested without a window.
//!
//! **Virtualised** (§16.4's rule, applied to a list): a collection of a hundred
//! thousand soundfonts costs a screenful of rectangles.

use crate::layout::Rect;
use crate::theme::Metrics;

/// Where everything in the browser is.
///
/// The row vectors carry the *index into the caller's list* alongside each
/// rectangle, which is what lets the list be a filtered, re-ordered search
/// result rather than the bank in its own order.
#[derive(Debug, Clone, PartialEq)]
pub struct BrowserLayout {
    pub body: Rect,
    /// The search field. §17.5's instant fuzzy search is the feature that makes
    /// a large collection usable, so it is the first thing in the panel.
    pub search: Rect,
    pub files: Rect,
    pub file_rows: Vec<(usize, Rect)>,
    /// The presets inside the selected file.
    pub presets: Rect,
    pub preset_rows: Vec<(usize, Rect)>,
    /// One line saying where the bank is, or what just went wrong.
    pub status: Rect,
    /// Shows the bank folder in the desktop's file manager. Pinned to the
    /// bottom: on a first run the bank is empty, and "where do I put them" is
    /// the only question the panel has to answer.
    pub open_folder: Rect,
    /// Picks a different folder.
    pub choose_folder: Rect,
    pub file_count: usize,
    pub preset_count: usize,
    pub file_scroll: usize,
    pub preset_scroll: usize,
}

/// How much of the list area goes to the files, the rest to their presets.
///
/// Slightly in the files' favour: you scan a collection to find a file, and
/// then read a shortish list of presets inside it.
const FILE_SHARE: f32 = 0.55;

pub fn browser_layout(
    body: Rect,
    metrics: &Metrics,
    file_count: usize,
    preset_count: usize,
    file_scroll: usize,
    preset_scroll: usize,
) -> BrowserLayout {
    let search_height = metrics.row_height.min(body.height.max(0.0));
    let (search, under) = body.split_top(search_height);
    // A hair of air under the box so it reads as a field rather than as the
    // first row of the list.
    let (_gap, rest) = under.split_top(GAP.min(under.height.max(0.0)));

    // The footer comes off the bottom first, so a long list can never push the
    // buttons off the panel — which is the one thing they must not do, since
    // on a first run they are all there is to click.
    let buttons_height = metrics.row_height.min(rest.height.max(0.0));
    let buttons = Rect::new(
        rest.x,
        (rest.bottom() - buttons_height).max(rest.y),
        rest.width,
        buttons_height,
    )
    .clamped();
    let status_height = metrics.row_height.min((buttons.y - rest.y).max(0.0));
    let status = Rect::new(
        rest.x,
        (buttons.y - status_height).max(rest.y),
        rest.width,
        status_height,
    )
    .clamped();

    // Slightly wider for "Open folder", which is both the longer caption and
    // the one somebody reaches for on a first run.
    let open_width = (buttons.width - GAP).max(0.0) * OPEN_SHARE;
    let open_folder = Rect::new(buttons.x, buttons.y, open_width, buttons.height).clamped();
    let choose_folder = Rect::new(
        open_folder.right() + GAP,
        buttons.y,
        buttons.right() - open_folder.right() - GAP,
        buttons.height,
    )
    .clamped();

    let lists = Rect::new(rest.x, rest.y, rest.width, (status.y - rest.y).max(0.0)).clamped();
    let files_height = (lists.height * FILE_SHARE).max(0.0);
    let (files, under_files) = lists.split_top(files_height);
    let (_gap, presets) = under_files.split_top(GAP.min(under_files.height.max(0.0)));

    BrowserLayout {
        body,
        search,
        file_rows: rows(files, metrics, file_count, file_scroll),
        files,
        preset_rows: rows(presets, metrics, preset_count, preset_scroll),
        presets,
        status,
        open_folder,
        choose_folder,
        file_count,
        preset_count,
        file_scroll,
        preset_scroll,
    }
}

/// Between the browser's stacked parts.
const GAP: f32 = 4.0;

/// How much of the footer row "Open folder" gets, the rest going to "Change".
const OPEN_SHARE: f32 = 0.58;

/// The rows of one list, built for the visible window and nothing else.
fn rows(area: Rect, metrics: &Metrics, count: usize, scroll: usize) -> Vec<(usize, Rect)> {
    if area.is_empty() || metrics.row_height <= 0.0 || count == 0 {
        return Vec::new();
    }
    let visible = (area.height / metrics.row_height).ceil() as usize + 1;
    let scroll = scroll.min(count.saturating_sub(1));
    let mut rows = Vec::new();
    for (slot, index) in (scroll..count).take(visible).enumerate() {
        let y = area.y + slot as f32 * metrics.row_height;
        if y >= area.bottom() {
            break;
        }
        rows.push((
            index,
            // Clipped to the list, so the last row is a half row rather than
            // one hanging over the panel below it.
            Rect::new(area.x, y, area.width, metrics.row_height).intersection(&area),
        ));
    }
    rows
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserHit {
    /// Give the search box the keyboard.
    Search,
    /// A file in the bank, by its index in the list the caller passed.
    File(usize),
    Preset(usize),
    /// Show the bank folder in the desktop's file manager.
    OpenFolder,
    /// Pick a different bank folder.
    ChooseFolder,
    Nothing,
}

pub fn browser_hit(layout: &BrowserLayout, x: f32, y: f32) -> BrowserHit {
    if layout.search.contains(x, y) {
        return BrowserHit::Search;
    }
    // The footer before the lists: it is drawn over the bottom of them when the
    // panel is short, and what is on top is what was clicked.
    if layout.open_folder.contains(x, y) {
        return BrowserHit::OpenFolder;
    }
    if layout.choose_folder.contains(x, y) {
        return BrowserHit::ChooseFolder;
    }
    for (index, rect) in &layout.file_rows {
        if rect.contains(x, y) {
            return BrowserHit::File(*index);
        }
    }
    for (index, rect) in &layout.preset_rows {
        if rect.contains(x, y) {
            return BrowserHit::Preset(*index);
        }
    }
    BrowserHit::Nothing
}

/// A scroll offset moved by `by` rows and kept inside a list of `count`.
///
/// One function rather than the same `saturating_sub`/`min` pair written out at
/// every wheel event, because getting it wrong scrolls a list past its own end
/// and leaves a panel that looks empty.
pub fn scrolled(scroll: usize, by: i32, count: usize) -> usize {
    let last = count.saturating_sub(1);
    if by < 0 {
        scroll.saturating_sub((-by) as usize)
    } else {
        (scroll + by as usize).min(last)
    }
}
