//! The start menu: the panel that helps you get where you need to go.
//!
//! > *"you open it it checks for updates and you have the option to upgrade
//! > if theres an update, or open an existing project from your recent
//! > projects or make a new project. this will be the start menu of the
//! > software which is just the panel that helps you get where you need to
//! > go, has the logo, and should also have a section somewhere marking it
//! > as a open source product of Fopull LLC"*
//!
//! One card, centred in the window, drawn over the studio until a choice is
//! made. Two columns: on the left the logo and the name, the version and
//! what the update check found, and the two ways to begin; on the right the
//! projects this machine was last in. Along the bottom, who made it and
//! where to find them.
//!
//! Like every canvas here it is a pure view-model (INVARIANT 2): a layout
//! from a rectangle and a count, a hit from a point, a [`WelcomeHit`] handed
//! back to the window, which asks the host to act. The check itself, the
//! files, the browser — none of that is here.

use crate::document::UpdateStatus;
use crate::layout::Rect;
use crate::theme::Metrics;

/// The company's site — the footer's first link.
pub const WEBSITE_URL: &str = "https://fopull.com";
/// The source — the footer's second link.
pub const REPOSITORY_URL: &str = "https://github.com/Fopull-LLC/DAW-Fontelle";

/// The footer's wording. One string, so the layout and the renderer agree
/// on what is being said.
pub const FOOTER_TEXT: &str = "Open source software by Fopull LLC";
pub const WEBSITE_LABEL: &str = "fopull.com";
pub const REPOSITORY_LABEL: &str = "Source on GitHub";
/// The footer's third link: the folder the session logs and crash reports
/// are in (`fontelle_app::logs`), for attaching to a bug report.
pub const LOGS_LABEL: &str = "Logs folder";
pub const NEW_PROJECT_LABEL: &str = "New project";
pub const OPEN_PROJECT_LABEL: &str = "Open a project\u{2026}";
pub const RECENT_HEADING: &str = "Recent projects";
pub const NOTHING_RECENT: &str = "Nothing yet \u{2014} make one, or open one.";

/// The card at its widest and tallest; smaller windows get a smaller card.
const CARD_WIDTH: f32 = 880.0;
const CARD_HEIGHT: f32 = 560.0;
/// Between the card and the window's edge.
const MARGIN: f32 = 24.0;
/// Inside the card's edge.
const PADDING: f32 = 24.0;
/// Between the two columns.
const GUTTER: f32 = 32.0;
/// How much of the inner width the left column takes.
const LEFT_SHARE: f32 = 0.45;
/// The logo is a square this big, with the name beside it.
const LOGO: f32 = 96.0;
/// Between the logo and the name.
const LOGO_GAP: f32 = 16.0;
/// The name is drawn at twice the chrome's size; this is the room for it.
pub const TITLE_HEIGHT: f32 = 40.0;
/// A button's height, and the gap between stacked ones.
const BUTTON_HEIGHT: f32 = 32.0;
const BUTTON_GAP: f32 = 8.0;
/// The update button is smaller: it is an offer, not the way in.
const UPDATE_BUTTON_HEIGHT: f32 = 26.0;
const UPDATE_BUTTON_WIDTH: f32 = 150.0;
/// A recent row: the name on one line and the path, small, on the next.
const ROW_HEIGHT: f32 = 40.0;
/// Between things stacked in a column.
const STACK_GAP: f32 = 12.0;
/// The footer's links are fixed-width cells the text is drawn inside.
const WEBSITE_WIDTH: f32 = 80.0;
const REPOSITORY_WIDTH: f32 = 120.0;
const LOGS_WIDTH: f32 = 80.0;
const LINK_GAP: f32 = 16.0;

/// One row of the recent list.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RecentRow {
    /// The whole row — pressing it opens the project.
    pub frame: Rect,
    /// The × at its right-hand end — pressing it forgets the project.
    pub forget: Rect,
}

/// Where everything on the card is.
#[derive(Debug, Clone, PartialEq)]
pub struct WelcomeLayout {
    /// The card itself.
    pub frame: Rect,
    pub logo: Rect,
    /// The name, beside the logo.
    pub title: Rect,
    /// "Version 0.1.0", under the name.
    pub version: Rect,
    /// What the update check has to say. Two rows, because "could not check
    /// for updates — no connection" is a sentence and the column is narrow.
    pub update: Rect,
    /// The offer, when there is one: install it, or go to the release page.
    /// While a download is under way the same slot holds the progress bar
    /// — [`update_progress`] says when — so the card does not reflow
    /// between the press and the bar.
    pub update_button: Option<Rect>,
    /// Two rows above the buttons for what went wrong — a bundle that would
    /// not open, a picker that is not installed. Empty most of the time.
    pub message: Rect,
    pub new_button: Rect,
    pub open_button: Rect,
    pub recent_heading: Rect,
    /// As many as were asked for and fit; the rest are not drawn.
    pub rows: Vec<RecentRow>,
    /// The line that stands in for an empty list.
    pub empty_recent: Option<Rect>,
    /// The bottom strip: the company line and the two links.
    pub footer: Rect,
    pub website: Rect,
    pub repository: Rect,
    /// The logs folder — "where get", answered.
    pub logs: Rect,
    /// The `?` in the top right corner: the keyboard shortcuts page.
    pub help: Rect,
}

/// Lays the card out in `window`, with `recent` rows wanted and an update
/// button or not.
pub fn welcome_layout(
    window: Rect,
    metrics: &Metrics,
    recent: usize,
    update_button: bool,
) -> WelcomeLayout {
    let row = metrics.row_height;
    let width = (window.width - 2.0 * MARGIN).clamp(0.0, CARD_WIDTH);
    let height = (window.height - 2.0 * MARGIN).clamp(0.0, CARD_HEIGHT);
    let frame = Rect::new(
        window.x + (window.width - width) / 2.0,
        window.y + (window.height - height) / 2.0,
        width,
        height,
    );
    let inner = frame.inset(PADDING);

    // The `?`, in the corner the card has nothing else in: the logo is top
    // left and the recent list starts a heading's height down. A row square,
    // so it reads as a button and not a stray glyph.
    let help = Rect::new(inner.right() - row, inner.y, row, row).clamped();

    // The footer first, because everything else stops above it.
    let footer = Rect::new(inner.x, inner.y + inner.height - row, inner.width, row);
    let repository = Rect::new(
        footer.x + footer.width - REPOSITORY_WIDTH,
        footer.y,
        REPOSITORY_WIDTH,
        row,
    );
    let website = Rect::new(
        repository.x - LINK_GAP - WEBSITE_WIDTH,
        footer.y,
        WEBSITE_WIDTH,
        row,
    );
    let logs = Rect::new(website.x - LINK_GAP - LOGS_WIDTH, footer.y, LOGS_WIDTH, row);
    let above_footer = footer.y - STACK_GAP;

    // The left column, top down: the logo with the name and version beside
    // it, the update line and its offer; and bottom up: the two ways in.
    let left_width = ((inner.width - GUTTER) * LEFT_SHARE).max(0.0);
    let left = Rect::new(inner.x, inner.y, left_width, above_footer - inner.y);
    let logo = Rect::new(left.x, left.y, LOGO, LOGO);
    let beside = Rect::new(
        logo.x + LOGO + LOGO_GAP,
        logo.y,
        (left.width - LOGO - LOGO_GAP).max(0.0),
        LOGO,
    );
    // The name and the version are stacked and centred on the logo.
    let block = TITLE_HEIGHT + row;
    let title = Rect::new(
        beside.x,
        beside.y + (LOGO - block) / 2.0,
        beside.width,
        TITLE_HEIGHT,
    );
    let version = Rect::new(beside.x, title.y + TITLE_HEIGHT, beside.width, row);
    let update = Rect::new(left.x, logo.y + LOGO + STACK_GAP, left.width, 2.0 * row);
    let update_button = update_button.then(|| {
        Rect::new(
            left.x,
            update.y + update.height + BUTTON_GAP,
            UPDATE_BUTTON_WIDTH.min(left.width),
            UPDATE_BUTTON_HEIGHT,
        )
    });
    let open_button = Rect::new(
        left.x,
        above_footer - BUTTON_HEIGHT,
        left.width,
        BUTTON_HEIGHT,
    );
    let new_button = Rect::new(
        left.x,
        open_button.y - BUTTON_GAP - BUTTON_HEIGHT,
        left.width,
        BUTTON_HEIGHT,
    );
    let message = Rect::new(
        left.x,
        new_button.y - STACK_GAP - 2.0 * row,
        left.width,
        2.0 * row,
    );

    // The right column: a heading, then rows until they would reach the
    // footer.
    let right_x = left.x + left.width + GUTTER;
    let right_width = (inner.x + inner.width - right_x).max(0.0);
    // The heading stops short of the help button beside it.
    let recent_heading = Rect::new(
        right_x,
        inner.y,
        (right_width - row - BUTTON_GAP).max(0.0),
        row,
    );
    let rows_top = recent_heading.y + row + BUTTON_GAP;
    let room = (above_footer - rows_top).max(0.0);
    let fit = (room / ROW_HEIGHT).floor() as usize;
    let rows: Vec<RecentRow> = (0..recent.min(fit))
        .map(|i| {
            let frame = Rect::new(
                right_x,
                rows_top + i as f32 * ROW_HEIGHT,
                right_width,
                ROW_HEIGHT,
            );
            let forget = Rect::new(
                frame.x + frame.width - row,
                frame.y + (ROW_HEIGHT - row) / 2.0,
                row,
                row,
            );
            RecentRow { frame, forget }
        })
        .collect();
    let empty_recent = (recent == 0).then(|| Rect::new(right_x, rows_top, right_width, row));

    WelcomeLayout {
        frame,
        logo,
        title,
        version,
        update,
        update_button,
        message,
        new_button,
        open_button,
        recent_heading,
        rows,
        empty_recent,
        footer,
        website,
        repository,
        logs,
        help,
    }
}

/// What a press on the card means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WelcomeHit {
    NewProject,
    OpenProject,
    /// The `n`th recent row: open it.
    Recent(usize),
    /// The × on the `n`th recent row: forget it.
    Forget(usize),
    /// The update offer — install, or the release page, whichever
    /// [`update_line`] offered.
    Update,
    Website,
    Repository,
    /// The logs folder, shown in the file manager.
    Logs,
    /// The `?`: the keyboard shortcuts page.
    Help,
}

/// What is under `(x, y)`, if anything is.
pub fn welcome_hit(layout: &WelcomeLayout, x: f32, y: f32) -> Option<WelcomeHit> {
    for (i, row) in layout.rows.iter().enumerate() {
        if row.forget.contains(x, y) {
            return Some(WelcomeHit::Forget(i));
        }
        if row.frame.contains(x, y) {
            return Some(WelcomeHit::Recent(i));
        }
    }
    if layout.new_button.contains(x, y) {
        return Some(WelcomeHit::NewProject);
    }
    if layout.open_button.contains(x, y) {
        return Some(WelcomeHit::OpenProject);
    }
    if layout.update_button.is_some_and(|b| b.contains(x, y)) {
        return Some(WelcomeHit::Update);
    }
    if layout.website.contains(x, y) {
        return Some(WelcomeHit::Website);
    }
    if layout.repository.contains(x, y) {
        return Some(WelcomeHit::Repository);
    }
    if layout.logs.contains(x, y) {
        return Some(WelcomeHit::Logs);
    }
    if layout.help.contains(x, y) {
        return Some(WelcomeHit::Help);
    }
    None
}

/// The update line's words, and the button's, if there is one to press.
///
/// The button is an *offer*: when a release can be installed it says so,
/// and when the check or the install failed it offers the page instead,
/// because a person who was told "no route to host" still wants a way to
/// get the release by hand. Everything in between — checking, downloading,
/// installed — is a state to read, not one to act on.
pub fn update_line(status: &UpdateStatus, current: &str) -> (String, Option<&'static str>) {
    match status {
        UpdateStatus::Unchecked => (String::new(), None),
        UpdateStatus::Off => (
            "Update check is off \u{2014} see Settings".to_string(),
            None,
        ),
        UpdateStatus::Checking => ("Checking for updates\u{2026}".to_string(), None),
        UpdateStatus::UpToDate => (format!("Up to date \u{2014} {current} is the latest"), None),
        UpdateStatus::Available { version } => (
            format!("Fontelle {version} is available"),
            Some("Install update"),
        ),
        UpdateStatus::Downloading {
            version,
            done,
            total,
        } => (
            format!(
                "Downloading Fontelle {version}\u{2026} {}",
                transfer_text(*done, *total)
            ),
            None,
        ),
        UpdateStatus::Installed { version } => (
            format!("Fontelle {version} is installed \u{2014} restart to use it"),
            None,
        ),
        // Already a sentence: the host says which half failed and why.
        UpdateStatus::Failed(why) => (why.clone(), Some("Release page")),
    }
}

/// How far the download is, for the bar: `None` when there is no download,
/// `Some(None)` while one runs whose size the server did not say (an
/// indeterminate bar), `Some(Some(fraction))` otherwise.
pub fn update_progress(status: &UpdateStatus) -> Option<Option<f32>> {
    match status {
        UpdateStatus::Downloading { done, total, .. } => Some(transfer_fraction(*done, *total)),
        _ => None,
    }
}

/// `3.1 of 12.4 MB`, or `3.1 MB` when the size is not known.
pub fn transfer_text(done: u64, total: Option<u64>) -> String {
    let mb = |bytes: u64| bytes as f64 / 1_000_000.0;
    match total {
        Some(total) => format!("{:.1} of {:.1} MB", mb(done), mb(total)),
        None => format!("{:.1} MB", mb(done)),
    }
}

/// The fraction done, or `None` when the size is not known.
pub fn transfer_fraction(done: u64, total: Option<u64>) -> Option<f32> {
    total
        .filter(|total| *total > 0)
        .map(|total| (done as f64 / total as f64).clamp(0.0, 1.0) as f32)
}
