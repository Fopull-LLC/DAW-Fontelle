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
    /// The two mode tabs, across the top. See [`BrowserMode`].
    pub sounds_tab: Rect,
    pub projects_tab: Rect,
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
    /// Makes a project in the projects folder. **Empty in
    /// [`BrowserMode::Sounds`]** — a control that does nothing in the mode you
    /// are in is worse than one that is not there.
    pub new_project: Rect,
    /// Bounces the open project to a WAV. Beside "new project", because both
    /// are things you do to a **project** rather than to the notes in it —
    /// which is what this tab is for. Empty in [`BrowserMode::Sounds`].
    pub export: Rect,
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

/// What the browser panel is showing.
///
/// The panel grows a second mode rather than the window growing a second
/// panel, and the reason is the shape of a session: you reach for a *project*
/// at the start and the end and for a *soundfont* all the way through, so the
/// two are never wanted at once — and a window that changes shape depending on
/// what you are doing is one you have to re-learn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BrowserMode {
    /// The soundfont bank (TDD §17.5): files, and the presets inside one.
    #[default]
    Sounds,
    /// The projects folder (TDD §17.3): one list, and a way to make one.
    Projects,
}

impl BrowserMode {
    /// What the tab says.
    pub fn label(self) -> &'static str {
        match self {
            Self::Sounds => "Sounds",
            Self::Projects => "Projects",
        }
    }
}

/// [`browser_layout_for`] in [`BrowserMode::Sounds`], which is what every
/// caller that predates the projects list wants.
pub fn browser_layout(
    body: Rect,
    metrics: &Metrics,
    file_count: usize,
    preset_count: usize,
    file_scroll: usize,
    preset_scroll: usize,
) -> BrowserLayout {
    browser_layout_for(
        body,
        metrics,
        BrowserMode::Sounds,
        file_count,
        preset_count,
        file_scroll,
        preset_scroll,
    )
}

pub fn browser_layout_for(
    body: Rect,
    metrics: &Metrics,
    mode: BrowserMode,
    file_count: usize,
    preset_count: usize,
    file_scroll: usize,
    preset_scroll: usize,
) -> BrowserLayout {
    // The mode switch first, above the search box: the search filters
    // whichever list is showing, so it belongs *under* the thing that decides
    // which list that is.
    // Kept whole: `BrowserLayout::body` is the panel, and everything in it
    // has to be inside that — including the tabs, which are split off below.
    let panel = body;
    let tabs_height = metrics.row_height.min(body.height.max(0.0));
    let (tabs, body_below) = body.split_top(tabs_height);
    let half = (tabs.width - GAP).max(0.0) / 2.0;
    let sounds_tab = Rect::new(tabs.x, tabs.y, half, tabs.height).clamped();
    let projects_tab = Rect::new(sounds_tab.right() + GAP, tabs.y, half, tabs.height).clamped();
    let (_gap, body) = body_below.split_top(GAP.min(body_below.height.max(0.0)));

    let search_height = metrics.row_height.min(body.height.max(0.0));
    let (search, under) = body.split_top(search_height);
    // A hair of air under the box so it reads as a field rather than as the
    // first row of the list.
    let (_gap, rest) = under.split_top(GAP.min(under.height.max(0.0)));

    // The footer comes off the bottom first, so a long list can never push the
    // buttons off the panel — which is the one thing they must not do, since
    // on a first run they are all there is to click.
    //
    // **One row at a time, each above the last.** The version of this that
    // measured every footer row from `buttons.y` put the status line and the
    // new-project row in the same pixels: "no projects folder yet — ..." was
    // drawn straight across "New" and "Export...", which is what a screenshot
    // of the Projects tab showed. `take_row` is the fix and the guard — a row
    // can only come off what is left, so two of them cannot occupy one place
    // however many are added later.
    let mut floor = rest.bottom();
    let mut take_row = |wanted: f32| {
        let height = wanted.min((floor - rest.y).max(0.0));
        let row = Rect::new(rest.x, (floor - height).max(rest.y), rest.width, height).clamped();
        floor = row.y;
        row
    };

    let buttons = take_row(metrics.row_height);

    // "New project" sits above the two folder buttons, in the mode that has
    // one: it is the thing somebody opens this tab for on a first run, and the
    // two folder buttons stay where they are in both modes so neither moves
    // when you switch.
    let (new_project, export) = if mode == BrowserMode::Projects {
        let row = take_row(metrics.row_height);
        // Side by side on one row, so the two folder buttons under them stay
        // where they are when the mode changes.
        let half = (row.width - GAP).max(0.0) / 2.0;
        (
            Rect::new(row.x, row.y, half, row.height).clamped(),
            Rect::new(row.x + half + GAP, row.y, half, row.height).clamped(),
        )
    } else {
        (Rect::ZERO, Rect::ZERO)
    };

    // Above both, so the line that says what went wrong is never underneath
    // the button that caused it.
    let status = take_row(metrics.row_height);

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

    // Everything above the footer. The status line is the topmost row of it
    // whichever mode this is, so it is the only bound the lists need.
    let lists = Rect::new(rest.x, rest.y, rest.width, (status.y - rest.y).max(0.0)).clamped();
    // **Both lists are a whole number of rows tall.** Rows are whole rows (see
    // `rows`), so a list whose height is not a multiple of one would carry a
    // dead band along its bottom edge that looks like part of the list and
    // hit-tests as nothing. Rounding the *lists* instead moves those pixels
    // somewhere they are visibly a gap.
    let whole = |height: f32| {
        if metrics.row_height <= 0.0 {
            return height.max(0.0);
        }
        (height.max(0.0) / metrics.row_height).floor() * metrics.row_height
    };
    // A project has no presets inside it, so in that mode the one list takes
    // the whole area rather than half of it being left empty.
    let (files, presets) = match mode {
        BrowserMode::Projects => (
            Rect::new(lists.x, lists.y, lists.width, whole(lists.height)).clamped(),
            Rect::ZERO,
        ),
        BrowserMode::Sounds => {
            let files_height = whole(lists.height * FILE_SHARE);
            let (files, under_files) = lists.split_top(files_height);
            let (_gap, rest_of_lists) = under_files.split_top(GAP.min(under_files.height.max(0.0)));
            (
                files,
                Rect::new(
                    rest_of_lists.x,
                    rest_of_lists.y,
                    rest_of_lists.width,
                    whole(rest_of_lists.height),
                )
                .clamped(),
            )
        }
    };
    let preset_count = match mode {
        BrowserMode::Projects => 0,
        BrowserMode::Sounds => preset_count,
    };

    BrowserLayout {
        body: panel,
        sounds_tab,
        projects_tab,
        search,
        file_rows: rows(files, metrics, file_count, file_scroll),
        files,
        preset_rows: rows(presets, metrics, preset_count, preset_scroll),
        presets,
        status,
        open_folder,
        choose_folder,
        new_project,
        export,
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
///
/// **Whole rows only.** The version of this that took one row more than fits
/// and clipped its *rectangle* is what made the panel look broken: a row three
/// pixels tall still had its caption drawn centred inside those three pixels,
/// which put it on top of the row above, and the same over-long rectangle
/// reached across the boundary into the list below so a click at the top of the
/// preset list landed on a soundfont. A row that does not fit is not shown, and
/// the few pixels left over at the bottom are list background.
pub(crate) fn rows(
    area: Rect,
    metrics: &Metrics,
    count: usize,
    scroll: usize,
) -> Vec<(usize, Rect)> {
    if area.is_empty() || metrics.row_height <= 0.0 || count == 0 {
        return Vec::new();
    }
    let visible = (area.height / metrics.row_height).floor() as usize;
    if visible == 0 {
        return Vec::new();
    }
    let scroll = scroll.min(count.saturating_sub(1));
    (scroll..count)
        .take(visible)
        .enumerate()
        .map(|(slot, index)| {
            (
                index,
                Rect::new(
                    area.x,
                    area.y + slot as f32 * metrics.row_height,
                    area.width,
                    metrics.row_height,
                ),
            )
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserHit {
    /// Switch the panel to this mode.
    Mode(BrowserMode),
    /// Give the search box the keyboard.
    Search,
    /// A file in the bank, by its index in the list the caller passed.
    File(usize),
    Preset(usize),
    /// Show the bank folder in the desktop's file manager.
    OpenFolder,
    /// Pick a different bank folder.
    ChooseFolder,
    /// Make a project in the projects folder.
    NewProject,
    /// Bounce the open project to a WAV.
    Export,
    Nothing,
}

impl BrowserHit {
    /// What a hover tip says (see [`crate::tooltip`]).
    ///
    /// `None` for the rows: a row explains itself by being a row with a name
    /// on it, and a box following the pointer down a list is in the way of the
    /// list.
    pub fn tip(self) -> Option<&'static str> {
        Some(match self {
            Self::Mode(BrowserMode::Sounds) => "The soundfonts you have",
            Self::Mode(BrowserMode::Projects) => "Your projects folder",
            Self::Search => "Search every soundfont by name",
            Self::OpenFolder => "Show this folder in your file manager",
            Self::ChooseFolder => "Use a different folder",
            Self::NewProject => "Start a new project",
            Self::Export => "Bounce this project to a WAV",
            Self::File(_) | Self::Preset(_) | Self::Nothing => return None,
        })
    }
}

pub fn browser_hit(layout: &BrowserLayout, x: f32, y: f32) -> BrowserHit {
    if layout.sounds_tab.contains(x, y) {
        return BrowserHit::Mode(BrowserMode::Sounds);
    }
    if layout.projects_tab.contains(x, y) {
        return BrowserHit::Mode(BrowserMode::Projects);
    }
    if layout.new_project.contains(x, y) {
        return BrowserHit::NewProject;
    }
    if layout.export.contains(x, y) {
        return BrowserHit::Export;
    }
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
    // **Which list first, then which row.** A row belongs to its own list and
    // to nothing else, so a pointer in the presets can never be answered with a
    // soundfont however the rows happen to have been laid out.
    if layout.files.contains(x, y) {
        return row_at(&layout.file_rows, x, y).map_or(BrowserHit::Nothing, BrowserHit::File);
    }
    if layout.presets.contains(x, y) {
        return row_at(&layout.preset_rows, x, y).map_or(BrowserHit::Nothing, BrowserHit::Preset);
    }
    BrowserHit::Nothing
}

/// The index of the row under `(x, y)`, if there is one.
pub(crate) fn row_at(rows: &[(usize, Rect)], x: f32, y: f32) -> Option<usize> {
    rows.iter()
        .find(|(_, rect)| rect.contains(x, y))
        .map(|(index, _)| *index)
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
