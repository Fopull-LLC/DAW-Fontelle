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
    /// Which list this geometry was built for.
    ///
    /// Carried rather than left to the caller because the two folder buttons
    /// mean a *different folder* in each mode, and a caller that has to
    /// remember which one it asked for is a caller that forgets: the Projects
    /// tab's "Change..." replaced the soundfont bank for exactly that reason.
    /// See [`BrowserHit::ChooseFolder`].
    pub mode: BrowserMode,
    /// The four mode tabs, across the top. See [`BrowserMode`].
    pub sounds_tab: Rect,
    pub projects_tab: Rect,
    pub import_tab: Rect,
    pub settings_tab: Rect,
    /// The two buttons inside the Import tab that say which kind of file is
    /// being browsed. **Empty in every other mode** — a control that does
    /// nothing in the mode you are in is worse than one that is not there.
    pub midi_kind: Rect,
    pub score_kind: Rect,
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
    /// The files you import from: `.mid` files, or FL Studio's `.fsc` scores.
    ///
    /// A tab rather than a list inside the roll's Tools panel, because
    /// everything a browser of files needs is already here — folders you can
    /// walk into, a `..` row back out, a search across the whole collection,
    /// and a virtualised list. A second implementation of all of that, eleven
    /// rows tall and hanging off a chip, would be worse at every one of them.
    ///
    /// **One tab for both kinds**, with a pair of buttons inside it saying
    /// which: five tabs across a 248-pixel sidebar is a row of abbreviations.
    Import,
    /// What Fontelle is set to (TDD §14.3, §18): one list of names and values,
    /// each of which a click changes.
    ///
    /// A third mode rather than a fourth panel or a dialog, for the reason the
    /// second one was added: it is reached for occasionally and never at the
    /// same time as the other two.
    Settings,
}

impl BrowserMode {
    /// Every mode, in the order the tabs are drawn.
    ///
    /// A list rather than four call sites writing the same four names out:
    /// the window shapes a caption per tab and the renderer draws one per tab,
    /// and a mode missing from either is a tab with **no words on it** — which
    /// is exactly what the Import tab was on the first frame it ever drew.
    pub const ALL: [Self; 4] = [Self::Sounds, Self::Projects, Self::Import, Self::Settings];

    /// What the tab says.
    pub fn label(self) -> &'static str {
        match self {
            Self::Sounds => "Sounds",
            Self::Projects => "Projects",
            Self::Import => "Import",
            Self::Settings => "Settings",
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
    // Four across, each the same width. Laid out from a running left edge
    // rather than each from its own multiple, so the rounding that a width of
    // 248 divided four ways produces lands in one place instead of opening a
    // gap between every pair.
    let quarter = (tabs.width - GAP * 3.0).max(0.0) / 4.0;
    let tab_at = |index: usize| {
        Rect::new(
            tabs.x + (quarter + GAP) * index as f32,
            tabs.y,
            quarter,
            tabs.height,
        )
        .clamped()
    };
    let sounds_tab = tab_at(0);
    let projects_tab = tab_at(1);
    let import_tab = tab_at(2);
    let settings_tab = tab_at(3);
    let (_gap, body) = body_below.split_top(GAP.min(body_below.height.max(0.0)));

    // **No search box over the settings.** It filters whichever list is
    // showing, and a settings list is short enough to read whole. A field
    // that does nothing is worse than no field — it takes the keyboard when
    // you click it and then says nothing back — so the row it would have
    // taken goes to the list instead.
    let (search, rest) = if mode == BrowserMode::Settings {
        (Rect::ZERO, body)
    } else {
        let search_height = metrics.row_height.min(body.height.max(0.0));
        let (search, under) = body.split_top(search_height);
        // A hair of air under the box so it reads as a field rather than as
        // the first row of the list.
        let (_gap, rest) = under.split_top(GAP.min(under.height.max(0.0)));
        (search, rest)
    };

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
    // The two kind buttons take the row "New project" takes in the Projects
    // tab — same place, same shape, so nothing under them moves when the tab
    // changes.
    let (midi_kind, score_kind) = if mode == BrowserMode::Import {
        let row = take_row(metrics.row_height);
        let half = (row.width - GAP).max(0.0) / 2.0;
        (
            Rect::new(row.x, row.y, half, row.height).clamped(),
            Rect::new(row.x + half + GAP, row.y, half, row.height).clamped(),
        )
    } else {
        (Rect::ZERO, Rect::ZERO)
    };

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

    // "Change..." has no meaning in the settings tab — there is no folder
    // being browsed there — so it is not drawn, and the whole row goes to the
    // one button that does mean something: the folder the settings live in.
    let (open_folder, choose_folder) = if mode == BrowserMode::Settings {
        (buttons, Rect::ZERO)
    } else {
        // Slightly wider for "Open folder", which is both the longer caption
        // and the one somebody reaches for on a first run.
        let open_width = (buttons.width - GAP).max(0.0) * OPEN_SHARE;
        let open = Rect::new(buttons.x, buttons.y, open_width, buttons.height).clamped();
        let choose = Rect::new(
            open.right() + GAP,
            buttons.y,
            buttons.right() - open.right() - GAP,
            buttons.height,
        )
        .clamped();
        (open, choose)
    };

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
        BrowserMode::Projects | BrowserMode::Settings | BrowserMode::Import => (
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
        BrowserMode::Projects | BrowserMode::Settings | BrowserMode::Import => 0,
        BrowserMode::Sounds => preset_count,
    };

    BrowserLayout {
        body: panel,
        mode,
        sounds_tab,
        projects_tab,
        import_tab,
        settings_tab,
        midi_kind,
        score_kind,
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
    /// Give the search box the keyboard. It filters whichever list is
    /// showing, so it says which one that is.
    Search(BrowserMode),
    /// A file in the bank, by its index in the list the caller passed.
    File(usize),
    Preset(usize),
    /// Show a folder in the desktop's file manager — **which** folder is the
    /// mode it carries.
    OpenFolder(BrowserMode),
    /// Pick a different folder, for whichever list is showing.
    ///
    /// The mode is part of the hit and not something the handler works out
    /// again from its own state. That is the whole fix for *"if I select
    /// change to set my projects folder ... it actually just changes my
    /// soundfonts folder"*: the click handler branched on the mode for
    /// [`OpenFolder`](Self::OpenFolder) and not for this one, so the two
    /// folders were one folder as far as this button was concerned.
    ChooseFolder(BrowserMode),
    /// Show this kind of file in the Import tab.
    Kind(fontelle_types::FolderKind),
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
            Self::Mode(BrowserMode::Import) => "MIDI files and FL scores to bring in",
            Self::Mode(BrowserMode::Settings) => "How Fontelle is set up",
            Self::Search(BrowserMode::Import) => "Search every file in the folder by name",
            Self::OpenFolder(BrowserMode::Import) => "Show the import folder in your file manager",
            Self::ChooseFolder(BrowserMode::Import) => "Choose the folder to import from",
            Self::Kind(fontelle_types::FolderKind::Midi) => "Browse your .mid files",
            Self::Kind(fontelle_types::FolderKind::Scores) => "Browse FL Studio .fsc scores",
            Self::Search(BrowserMode::Sounds) => "Search every soundfont by name",
            Self::Search(BrowserMode::Projects) => "Search your projects by name",
            // Never drawn in the settings tab: there is no box there. A hit is
            // an enum, though, and every case of one has to be a sentence.
            Self::Search(BrowserMode::Settings) => "Search your settings by name",
            Self::OpenFolder(BrowserMode::Sounds) => {
                "Show your soundfont folder in your file manager"
            }
            Self::OpenFolder(BrowserMode::Projects) => {
                "Show your projects folder in your file manager"
            }
            Self::OpenFolder(BrowserMode::Settings) => {
                "Show the folder Fontelle keeps its settings in"
            }
            Self::ChooseFolder(BrowserMode::Sounds) => "Use a different soundfont folder",
            Self::ChooseFolder(BrowserMode::Projects) => "Use a different projects folder",
            Self::ChooseFolder(BrowserMode::Settings) => "Use a different folder",
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
    if layout.import_tab.contains(x, y) {
        return BrowserHit::Mode(BrowserMode::Import);
    }
    if layout.settings_tab.contains(x, y) {
        return BrowserHit::Mode(BrowserMode::Settings);
    }
    if layout.midi_kind.contains(x, y) {
        return BrowserHit::Kind(fontelle_types::FolderKind::Midi);
    }
    if layout.score_kind.contains(x, y) {
        return BrowserHit::Kind(fontelle_types::FolderKind::Scores);
    }
    if layout.new_project.contains(x, y) {
        return BrowserHit::NewProject;
    }
    if layout.export.contains(x, y) {
        return BrowserHit::Export;
    }
    if layout.search.contains(x, y) {
        return BrowserHit::Search(layout.mode);
    }
    // The footer before the lists: it is drawn over the bottom of them when the
    // panel is short, and what is on top is what was clicked.
    if layout.open_folder.contains(x, y) {
        return BrowserHit::OpenFolder(layout.mode);
    }
    if layout.choose_folder.contains(x, y) {
        return BrowserHit::ChooseFolder(layout.mode);
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

/// The row of the browser's main list under `(x, y)`, if the pointer is on
/// one.
///
/// [`browser_hit`] answers the same question and four others; this is for the
/// caller — the wheel — that only wants the row and does not want a button
/// press for an answer.
pub fn row_under(layout: &BrowserLayout, x: f32, y: f32) -> Option<usize> {
    layout
        .files
        .contains(x, y)
        .then(|| row_at(&layout.file_rows, x, y))
        .flatten()
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
