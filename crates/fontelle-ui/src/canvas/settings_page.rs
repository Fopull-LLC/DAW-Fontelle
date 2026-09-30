//! The settings page: a full page over the studio, opened by the gear.
//!
//! > *"formatted weird like everything looks like a button even when things
//! > are just labels, some things just have no or little feedback"*
//!
//! Ty chose a full page over the sidebar tab it replaces
//! (`docs/ux-routing-and-learning-plan.md` §6): a list of sections down the
//! left, and on the right the chosen section's rows — each a name, a line
//! under it saying what it is for, and a control sized as a control rather
//! than a whole row that looks pressable.
//!
//! The rows are the host's flat list (`StudioHost::settings`,
//! `setting_controls`, `setting_help`), unchanged in meaning: a heading row
//! starts a section, and every other row is addressed by its index in the
//! list, so the presses, drags and drop-downs the window already routed by
//! index route the same way from here. Geometry only (INVARIANT 2).

use std::ops::Range;

use super::SettingControl;
use crate::layout::Rect;
use crate::theme::Metrics;

/// What the page is called.
pub const SETTINGS_TITLE: &str = "Settings";
/// The close button's glyph.
pub const SETTINGS_CLOSE: &str = "\u{2715}";
/// What the section list says of a section that has nothing in it yet.
pub const SETTINGS_EMPTY: &str = "Nothing to set here yet";

/// One section: the heading row that names it, and the rows under it up to
/// the next heading.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsSection {
    /// The heading's index in the host's list. Rows before the first heading,
    /// if any, make a section whose heading is their own first row's index.
    pub heading: usize,
    pub rows: Range<usize>,
}

/// Splits the host's flat list into sections at its headings.
///
/// A section with no rows is kept: the section list is how somebody learns
/// what the page covers, and a heading that vanished while empty (the
/// Extensions heading before its catalogue fills it) would teach them it is
/// not there.
pub fn settings_sections(controls: &[SettingControl]) -> Vec<SettingsSection> {
    let mut sections: Vec<SettingsSection> = Vec::new();
    for (index, control) in controls.iter().enumerate() {
        if *control == SettingControl::Heading {
            sections.push(SettingsSection {
                heading: index,
                rows: index + 1..index + 1,
            });
        } else if let Some(last) = sections.last_mut() {
            last.rows.end = index + 1;
        } else {
            sections.push(SettingsSection {
                heading: index,
                rows: index..index + 1,
            });
        }
    }
    sections
}

/// The card at its widest.
const CARD_WIDTH: f32 = 980.0;
/// Between the card and the window's edge.
const MARGIN: f32 = 24.0;
/// Inside the card's edge.
const PADDING: f32 = 20.0;
/// The section list's width, and the gap between it and the rows.
const NAV_WIDTH: f32 = 190.0;
const NAV_GAP: f32 = 24.0;
/// A control's width at most; a narrow card gives it a share of the row.
const CONTROL_WIDTH: f32 = 300.0;
const CONTROL_SHARE: f32 = 0.45;
/// Inside a row's edge, and between its words and its control.
const ROW_PAD: f32 = 12.0;
/// Between rows.
const ROW_GAP: f32 = 6.0;
/// Under the header, before the body.
const HEADER_GAP: f32 = 14.0;

/// One row of the chosen section, where it is drawn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SettingsPageRow {
    /// Its index in the host's list — what a press on it is routed by.
    pub index: usize,
    /// The whole row, its hover plate.
    pub rect: Rect,
    /// The name, top left.
    pub label: Rect,
    /// The line saying what it is for, under the name.
    pub help: Rect,
    /// The thing you touch, on the right.
    pub control: Rect,
}

/// Where everything on the page is.
#[derive(Debug, Clone, PartialEq)]
pub struct SettingsPageLayout {
    /// The card. A press outside it closes the page.
    pub frame: Rect,
    pub title: Rect,
    /// The line under the title: where the settings file is, or what went
    /// wrong writing it.
    pub status: Rect,
    /// The × in the top right corner.
    pub close: Rect,
    /// One entry per section, top to bottom, in the host's order.
    pub nav: Vec<Rect>,
    /// The section being shown, clamped to one that exists.
    pub section: usize,
    /// The scrolling rows' area.
    pub body: Rect,
    /// The chosen section's rows that meet the body, at their scrolled places.
    pub rows: Vec<SettingsPageRow>,
    /// How tall the section's rows are in all, scrolled or not.
    pub content_height: f32,
    /// The scroll this was laid out at, clamped.
    pub scroll: f32,
}

/// How tall one row is: a name and a help line, with air round them.
pub fn settings_page_row_height(metrics: &Metrics) -> f32 {
    (metrics.row_height.round().max(1.0) * 2.0 + 2.0 * ROW_PAD * 0.75).round()
}

/// Lays the page out in `window`, showing `section`, scrolled `scroll`
/// points down its rows.
pub fn settings_page_layout(
    window: Rect,
    metrics: &Metrics,
    sections: &[SettingsSection],
    section: usize,
    scroll: f32,
) -> SettingsPageLayout {
    let row = metrics.row_height.round().max(1.0);
    let width = (window.width - 2.0 * MARGIN).clamp(0.0, CARD_WIDTH);
    let height = (window.height - 2.0 * MARGIN).max(0.0);
    let frame = Rect::new(
        (window.x + (window.width - width) / 2.0).round(),
        (window.y + (window.height - height) / 2.0).round(),
        width,
        height,
    )
    .clamped();
    let inner = frame.inset(PADDING).clamped();

    let title_h = (row * 1.35).round();
    let close = Rect::new(inner.right() - title_h, inner.y, title_h, title_h)
        .intersection(&inner)
        .clamped();
    let title = Rect::new(
        inner.x,
        inner.y,
        (close.x - ROW_PAD - inner.x).max(0.0),
        title_h,
    )
    .intersection(&inner)
    .clamped();
    let status = Rect::new(inner.x, title.bottom(), inner.width, row)
        .intersection(&inner)
        .clamped();
    let top = status.bottom() + HEADER_GAP;
    let below = (inner.bottom() - top).max(0.0);

    let nav_width = NAV_WIDTH.min(inner.width * 0.3).max(0.0).round();
    let nav_h = (row * 1.6).round();
    let nav = (0..sections.len())
        .map(|i| {
            Rect::new(inner.x, top + i as f32 * nav_h, nav_width, nav_h)
                .intersection(&inner)
                .clamped()
        })
        .collect();

    let body_x = inner.x + nav_width + NAV_GAP;
    let body = Rect::new(body_x, top, (inner.right() - body_x).max(0.0), below)
        .intersection(&inner)
        .clamped();

    let section = if section < sections.len() { section } else { 0 };
    let row_h = settings_page_row_height(metrics);
    let count = sections.get(section).map_or(0, |s| s.rows.len());
    let content_height = if count == 0 {
        0.0
    } else {
        count as f32 * row_h + (count - 1) as f32 * ROW_GAP
    };
    let scroll = scroll.clamp(0.0, (content_height - body.height).max(0.0));

    let control_w = CONTROL_WIDTH
        .min(body.width * CONTROL_SHARE)
        .max(0.0)
        .round();
    let control_h = (row * 1.4).round().min(row_h);
    let rows = sections
        .get(section)
        .map(|s| s.rows.clone())
        .unwrap_or(0..0)
        .enumerate()
        .filter_map(|(at, index)| {
            let y = (body.y + at as f32 * (row_h + ROW_GAP) - scroll).round();
            let rect = Rect::new(body.x, y, body.width, row_h);
            if !rect.intersects(&body) {
                return None;
            }
            let control = Rect::new(
                rect.right() - ROW_PAD - control_w,
                rect.y + ((row_h - control_h) / 2.0).round(),
                control_w,
                control_h,
            )
            .intersection(&rect)
            .clamped();
            let words_w = (control.x - ROW_PAD - (rect.x + ROW_PAD)).max(0.0);
            let words_y = rect.y + ((row_h - 2.0 * row) / 2.0).round();
            let label = Rect::new(rect.x + ROW_PAD, words_y, words_w, row)
                .intersection(&rect)
                .clamped();
            let help = Rect::new(rect.x + ROW_PAD, label.bottom(), words_w, row)
                .intersection(&rect)
                .clamped();
            Some(SettingsPageRow {
                index,
                rect,
                label,
                help,
                control,
            })
        })
        .collect();

    SettingsPageLayout {
        frame,
        title,
        status,
        close,
        nav,
        section,
        body,
        rows,
        content_height,
        scroll,
    }
}

/// How far the rows can be scrolled.
pub fn settings_page_scroll_max(layout: &SettingsPageLayout) -> f32 {
    (layout.content_height - layout.body.height).max(0.0)
}

/// How far one wheel notch moves the rows.
const SCROLL_STEP: f32 = 48.0;

/// The scroll `notches` wheel notches from `scroll`, clamped. Up is positive,
/// the way the wheel reports it.
pub fn settings_page_scrolled(layout: &SettingsPageLayout, scroll: f32, notches: f32) -> f32 {
    (scroll - notches * SCROLL_STEP).clamp(0.0, settings_page_scroll_max(layout))
}

/// What a press lands on while the page is up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsPageHit {
    /// The × — close the page.
    Close,
    /// An entry in the section list, by its place in it.
    Section(usize),
    /// A row, by its index in the host's list; `on_control` when the press is
    /// on the control itself rather than the row's words.
    Row { index: usize, on_control: bool },
    /// Elsewhere on the card — nothing.
    Card,
    /// Off the card — close it, the way a click away from a menu shuts it.
    Outside,
}

pub fn settings_page_hit(layout: &SettingsPageLayout, x: f32, y: f32) -> SettingsPageHit {
    if layout.close.contains(x, y) {
        return SettingsPageHit::Close;
    }
    if let Some(at) = layout.nav.iter().position(|r| r.contains(x, y)) {
        return SettingsPageHit::Section(at);
    }
    if layout.body.contains(x, y)
        && let Some(row) = layout.rows.iter().find(|r| r.rect.contains(x, y))
    {
        return SettingsPageHit::Row {
            index: row.index,
            on_control: row.control.contains(x, y),
        };
    }
    if layout.frame.contains(x, y) {
        SettingsPageHit::Card
    } else {
        SettingsPageHit::Outside
    }
}
