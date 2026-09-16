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
    /// The mode tabs, across the top, in [`BrowserMode::ALL`]'s order.
    ///
    /// **One list rather than a field per mode**, which is the same lesson
    /// `BrowserMode::ALL` records one level up: a mode missing from a field
    /// list is a tab with no words on it, and adding the fifth (Presets, §P.8)
    /// is now a variant and nothing else.
    pub tabs: Vec<(BrowserMode, Rect)>,
    /// A button per kind inside the Import tab, saying which one is being
    /// browsed, in `FolderKind::ALL`'s order. **Empty in every other mode** —
    /// a control that does nothing in the mode you are in is worse than one
    /// that is not there.
    ///
    /// A list rather than one field per kind: adding audio to `FolderKind` with
    /// two named fields here drew two buttons over three kinds, which is the
    /// same class of bug as the fourth browser tab that was drawn with no words
    /// on it. What is laid out is what the enum says there is.
    pub kinds: Vec<(fontelle_types::FolderKind, Rect)>,
    /// The search field. §17.5's instant fuzzy search is the feature that makes
    /// a large collection usable, so it is the first thing in the panel.
    pub search: Rect,
    pub files: Rect,
    pub file_rows: Vec<(usize, Rect)>,
    /// The presets inside the selected file.
    /// The grab strip between the two lists — see [`browser_file_share_at`].
    ///
    /// Empty in every mode but [`BrowserMode::Sounds`], which is the only one
    /// with two lists to divide. *"give it a knob i can drag in the middle to
    /// make whichever one i want bigger whenever i want."*
    pub seam: Rect,
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

/// How few rows either list may be squeezed to by the seam.
///
/// Not zero: a list dragged away to nothing takes the seam with it, and then
/// there is nothing left to grab to bring it back. Two rows is enough to see
/// that something is there and enough to aim at.
const MIN_LIST_ROWS: f32 = 2.0;

/// How tall the grab strip between the lists is.
const SEAM_PX: f32 = 6.0;

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
    /// Every preset on this machine, for every device
    /// (`docs/flopsynth-plan.md` §P.8).
    ///
    /// A fifth mode rather than a list inside one of the others, because a
    /// preset is something you go looking for the way you go looking for a
    /// soundfont — and everything a browser of those needs is already here:
    /// two lists, a search across the whole collection, a star on every row.
    Presets,
}

impl BrowserMode {
    /// Every mode, in the order the tabs are drawn.
    ///
    /// A list rather than four call sites writing the same four names out:
    /// the window shapes a caption per tab and the renderer draws one per tab,
    /// and a mode missing from either is a tab with **no words on it** — which
    /// is exactly what the Import tab was on the first frame it ever drew.
    pub const ALL: [Self; 5] = [
        Self::Sounds,
        Self::Presets,
        Self::Projects,
        Self::Import,
        Self::Settings,
    ];

    /// What the tab says.
    pub fn label(self) -> &'static str {
        match self {
            Self::Sounds => "Sounds",
            Self::Projects => "Projects",
            Self::Import => "Import",
            Self::Settings => "Settings",
            Self::Presets => "Presets",
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
    browser_layout_split(
        body,
        metrics,
        mode,
        file_count,
        preset_count,
        file_scroll,
        preset_scroll,
        None,
    )
}

/// [`browser_layout_for`], with the seam between the two lists dragged to
/// `file_share` of the room they share.
///
/// `None` is [`FILE_SHARE`], which is where it sits until somebody moves it.
#[allow(clippy::too_many_arguments)]
pub fn browser_layout_split(
    body: Rect,
    metrics: &Metrics,
    mode: BrowserMode,
    file_count: usize,
    preset_count: usize,
    file_scroll: usize,
    preset_scroll: usize,
    file_share: Option<f32>,
) -> BrowserLayout {
    // The mode switch first, above the search box: the search filters
    // whichever list is showing, so it belongs *under* the thing that decides
    // which list that is.
    // Kept whole: `BrowserLayout::body` is the panel, and everything in it
    // has to be inside that — including the tabs, which are split off below.
    let panel = body;
    let tabs_height = metrics.row_height.min(body.height.max(0.0));
    let (tabs, body_below) = body.split_top(tabs_height);
    // One across per mode, each the same width. Laid out from a running left
    // edge rather than each from its own multiple, so the rounding a sidebar
    // width divided five ways produces lands in one place instead of opening a
    // gap between every pair.
    let count = BrowserMode::ALL.len();
    let share = (tabs.width - GAP * (count.saturating_sub(1)) as f32).max(0.0) / count as f32;
    let tabs: Vec<(BrowserMode, Rect)> = BrowserMode::ALL
        .into_iter()
        .enumerate()
        .map(|(index, mode)| {
            (
                mode,
                // Clipped to the strip, not merely clamped: five shares of a
                // sidebar's width do not add back up to it exactly, and a tab
                // a hundred-thousandth of a pixel past the edge is still a tab
                // outside the panel.
                Rect::new(
                    tabs.x + (share + GAP) * index as f32,
                    tabs.y,
                    share,
                    tabs.height,
                )
                .intersection(&tabs)
                .clamped(),
            )
        })
        .collect();
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
    let kinds: Vec<(fontelle_types::FolderKind, Rect)> = if mode == BrowserMode::Import {
        let row = take_row(metrics.row_height);
        let count = fontelle_types::FolderKind::ALL.len() as f32;
        let each = ((row.width - GAP * (count - 1.0)) / count).max(0.0);
        fontelle_types::FolderKind::ALL
            .iter()
            .enumerate()
            .map(|(index, kind)| {
                let x = row.x + (each + GAP) * index as f32;
                (*kind, Rect::new(x, row.y, each, row.height).clamped())
            })
            .collect()
    } else {
        Vec::new()
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
    let (files, seam, presets) = match mode {
        BrowserMode::Projects | BrowserMode::Settings | BrowserMode::Import => (
            Rect::new(lists.x, lists.y, lists.width, whole(lists.height)).clamped(),
            Rect::ZERO,
            Rect::ZERO,
        ),
        // Two lists, and the same two: devices above, their presets below
        // (§P.8). Reusing the shape rather than inventing one is most of why
        // the Presets tab is a variant and not a panel.
        BrowserMode::Sounds | BrowserMode::Presets => {
            // What is left for the two lists once the seam has taken its
            // strip: the seam is *between* them rather than over either, so a
            // press on it can never also be a press on a row.
            let seam_height = SEAM_PX.min(lists.height.max(0.0));
            let shared = (lists.height - seam_height).max(0.0);
            // Both floors measured against what there is, so a panel too short
            // for four rows gives what it can instead of going negative.
            let floor = (metrics.row_height * MIN_LIST_ROWS).min(shared / 2.0);
            let wanted = shared * file_share.unwrap_or(FILE_SHARE).clamp(0.0, 1.0);
            let files_height = whole(wanted.clamp(floor, (shared - floor).max(floor)));
            let (files, under_files) = lists.split_top(files_height);
            let (seam, rest_of_lists) =
                under_files.split_top(seam_height.min(under_files.height.max(0.0)));
            (
                files,
                seam,
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
        BrowserMode::Sounds | BrowserMode::Presets => preset_count,
    };

    BrowserLayout {
        body: panel,
        mode,
        tabs,
        kinds,
        search,
        file_rows: rows(files, metrics, file_count, file_scroll),
        files,
        preset_rows: rows(presets, metrics, preset_count, preset_scroll),
        seam,
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
    /// The strip between the bank and its presets, which drags to divide them.
    Seam,
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
            Self::Mode(BrowserMode::Import) => "Sounds, MIDI files and FL scores to bring in",
            Self::Mode(BrowserMode::Settings) => "How Fontelle is set up",
            Self::Mode(BrowserMode::Presets) => "Every preset, for every device",
            Self::Search(BrowserMode::Presets) => "Search every preset by name",
            Self::OpenFolder(BrowserMode::Presets) => {
                "Show your own preset folder in your file manager"
            }
            Self::ChooseFolder(BrowserMode::Presets) => "Use a different preset folder",
            Self::Search(BrowserMode::Import) => "Search every file in the folder by name",
            Self::OpenFolder(BrowserMode::Import) => "Show the import folder in your file manager",
            Self::ChooseFolder(BrowserMode::Import) => "Choose the folder to import from",
            Self::Kind(fontelle_types::FolderKind::Midi) => "Browse your .mid files",
            Self::Kind(fontelle_types::FolderKind::Scores) => "Browse FL Studio .fsc scores",
            Self::Kind(fontelle_types::FolderKind::Audio) => "Browse your sounds and loops",
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
            Self::Seam => "Drag to divide the bank and its presets",
            Self::File(_) | Self::Preset(_) | Self::Nothing => return None,
        })
    }

    /// The keymap action that does the same, for the tip: the search box
    /// has a key, and the WAV bounce has one.
    pub fn action(self) -> Option<super::keymap::Action> {
        Some(match self {
            Self::Search(_) => super::keymap::Action::Search,
            Self::Export => super::keymap::Action::ExportWav,
            _ => return None,
        })
    }
}

impl BrowserLayout {
    /// Where one mode's tab is, or [`Rect::ZERO`] if it did not fit.
    ///
    /// Named rather than indexed, so a caller says which tab it means and
    /// cannot be off by one when a mode is added between two others.
    pub fn tab(&self, mode: BrowserMode) -> Rect {
        self.tabs
            .iter()
            .find(|(which, _)| *which == mode)
            .map(|(_, rect)| *rect)
            .unwrap_or(Rect::ZERO)
    }
}

pub fn browser_hit(layout: &BrowserLayout, x: f32, y: f32) -> BrowserHit {
    for (mode, rect) in &layout.tabs {
        if rect.contains(x, y) {
            return BrowserHit::Mode(*mode);
        }
    }
    for (kind, rect) in &layout.kinds {
        if rect.contains(x, y) {
            return BrowserHit::Kind(*kind);
        }
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
    // Before the lists, because it sits in the gap between them and a strip
    // that lost to a row would be a strip nobody can grab.
    if !layout.seam.is_empty() && layout.seam.contains(x, y) {
        return BrowserHit::Seam;
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

/// What a settings row is drawn and driven as.
///
/// The host answers this per row (see `StudioHost::setting_controls`), and the
/// panel draws a real control and routes a press accordingly — a drag along a
/// groove, a flip of a switch, a drop-down of choices — instead of the old
/// click that stepped a value one place. The window still knows nothing about
/// what any row *means* (INVARIANT 2): a `Slider` is a fraction and a value
/// string, a `Choice` is a list of labels, and which is which is the host's.
#[derive(Debug, Clone, PartialEq)]
pub enum SettingControl {
    /// A section title — nothing to touch.
    Heading,
    /// A press acts (a folder picker, a rescan, an install). The row's value
    /// column is a caption, not a value, so nothing extra is drawn.
    Button,
    /// A number the user drags along a groove or nudges with the arrow keys.
    /// `fraction` (0..=1) is where the handle sits; the value text is the row's
    /// own `detail`.
    Slider { fraction: f32 },
    /// One of a fixed list, opened as a drop-down. `chosen` indexes `options`.
    Choice { options: Vec<String>, chosen: usize },
    /// An on/off switch a press flips.
    Switch { on: bool },
}

/// How much of a settings row's width its control takes, on the right. The left
/// is the row's name.
const CONTROL_SHARE: f32 = 0.46;

/// Room at the right of a slider's control for its value to be written, clear
/// of the groove — so "+12 st" is not drawn over the handle.
const VALUE_GUTTER: f32 = 42.0;

/// Where a settings row's control sits — the right of the row, the name filling
/// the left. What a switch's pill, a choice's caret and a slider's groove are
/// laid out inside, and the target a press on the control is tested against.
pub fn setting_control_rect(row: Rect, _metrics: &Metrics) -> Rect {
    if row.is_empty() {
        return Rect::ZERO;
    }
    let width = (row.width * CONTROL_SHARE).clamp(0.0, row.width);
    Rect::new(row.right() - width, row.y, width, row.height)
        .intersection(&row)
        .clamped()
}

/// How wide the draggable groove is inside a control — the control less the
/// gutter its value is written in. One place, so the fill and the drag agree.
fn slider_span(control: Rect) -> f32 {
    (control.width - VALUE_GUTTER.min(control.width * 0.5)).max(0.0)
}

/// Where along a slider's groove a fraction sits, in pixels.
pub fn setting_slider_x_of(control: Rect, fraction: f32) -> f32 {
    control.x + slider_span(control) * fraction.clamp(0.0, 1.0)
}

/// The other way: what fraction a press at `x` is, held inside 0..=1 so a drag
/// past either end of the groove does not set a value past the row's ends.
pub fn setting_slider_at(control: Rect, x: f32) -> f32 {
    let span = slider_span(control);
    if span <= 0.0 {
        return 0.0;
    }
    ((x - control.x) / span).clamp(0.0, 1.0)
}

/// The thin groove a slider row draws its fill along, inside its control.
pub fn setting_slider_groove(control: Rect, metrics: &Metrics) -> Rect {
    if control.is_empty() {
        return Rect::ZERO;
    }
    let height = (metrics.row_height * 0.26).clamp(2.0, control.height);
    Rect::new(
        control.x,
        control.y + (control.height - height) / 2.0,
        slider_span(control),
        height,
    )
    .intersection(&control)
    .clamped()
}

/// What share of the two lists the bank should get for a seam dragged to `y`.
///
/// The browser's own [`crate::layout::rack_share_at`]: pure, so "it cannot be
/// dragged until one list has no rows left" is a test rather than something to
/// find out by doing it. Clamped to 0..=1; the layout applies the row floors,
/// since only it knows how tall a row is.
pub fn browser_file_share_at(layout: &BrowserLayout, y: f32) -> f32 {
    let top = layout.files.y;
    let bottom = layout.presets.bottom().max(layout.files.bottom());
    let span = bottom - top;
    if span <= 0.0 {
        return FILE_SHARE;
    }
    ((y - top) / span).clamp(0.0, 1.0)
}

/// Where the keyboard focus in one of the browser's lists should go from
/// `from`, stepping by `delta` rows.
///
/// *"make it easy to also go through the selected instruments with arrow keys
/// after it being clicked on to focus it."* Pure, because the two rules that
/// make it usable are both easy to get wrong in an event handler and easy to
/// state here:
///
/// - **A heading is stepped over.** A search across the collection puts one
///   over every run of hits ([`crate::document::LibraryKind::Group`]), and a
///   focus that lands on them makes the down arrow appear to do nothing every
///   few presses.
/// - **The ends hold.** Wrapping from the bottom back to the top loses your
///   place without saying so.
///
/// `None` when the list has nothing that can be chosen in it at all.
pub fn browser_focus_step(
    rows: &[crate::document::LibraryEntry],
    from: Option<usize>,
    delta: i32,
) -> Option<usize> {
    use crate::document::LibraryKind;
    let choosable = |index: usize| {
        rows.get(index)
            .is_some_and(|row| row.kind != LibraryKind::Group)
    };
    if rows.is_empty() {
        return None;
    }
    // A list rebuilt by a search keystroke can be shorter than the focus that
    // was in it, so the starting point is brought inside before anything else.
    let start = match from {
        Some(index) => index.min(rows.len() - 1) as i32,
        // Nothing focused: the first row that can be chosen, whichever way the
        // arrow was pointing.
        None => {
            return (0..rows.len()).find(|index| choosable(*index));
        }
    };
    let step = if delta == 0 { 1 } else { delta.signum() };
    // A step of nothing keeps a row that can be chosen and moves off one that
    // cannot, which is what a rebuilt list needs to settle.
    if delta == 0 && choosable(start as usize) {
        return Some(start as usize);
    }
    let mut at = start + if delta == 0 { 0 } else { step };
    while at >= 0 && (at as usize) < rows.len() {
        if choosable(at as usize) {
            return Some(at as usize);
        }
        at += step;
    }
    // Off the end: hold where we were, if that is somewhere we may be.
    if choosable(start as usize) {
        return Some(start as usize);
    }
    // The row we started on cannot be chosen either — a list of headings, or
    // one rebuilt under us. Anything choosable will do; nothing means nothing.
    (0..rows.len()).find(|index| choosable(*index))
}

/// Whether row `index` can be **carried out** of the panel — dragged onto the
/// rack, or onto a channel already on it, to become a sampler.
///
/// > *"i currently cannot drag audio files from the import audio tab. i want to
/// > be able to click and drag them into the sampler or into the channel rack
/// > to make it have a sampler with that clip sampled."*
///
/// The press that arms that drag used to ask **the soundfont list** whether
/// the row under the pointer was a folder, while the rows on screen came from
/// the import list. Two different lists, of two different lengths, so the
/// answer was whatever the other panel happened to hold at that index: usually
/// a folder, which armed nothing at all, and past its end `None`, which armed
/// a drag on the `..` row. One function, handed the list the rows were drawn
/// from, so there is nowhere left for the two to disagree.
///
/// Only in the Import tab, and only a file: a folder is a place, the `..` row
/// is a move, a heading is a label, and a soundfont preset is carried by a
/// different gesture that means something else (see [`BrowserHit::Preset`]).
///
/// And only while that tab is showing **sounds**. `Drag::BrowserRow` has said
/// "only audio can be carried: a MIDI file dropped on the rack is not an
/// instrument" since it was written, and this function — the one place that
/// decision lives — did not ask, so a `.mid` row armed a drag whose every
/// landing could only fail (`only an audio file can become a sampler`, said
/// after the release rather than before it). A file that makes tracks of its
/// own is opened by a click; what is carried is a sound.
pub fn browser_row_carries(
    rows: &[crate::document::LibraryEntry],
    mode: BrowserMode,
    kind: fontelle_types::FolderKind,
    index: usize,
) -> bool {
    mode == BrowserMode::Import
        && kind == fontelle_types::FolderKind::Audio
        && rows.get(index).map(|row| row.kind) == Some(crate::document::LibraryKind::File)
}
