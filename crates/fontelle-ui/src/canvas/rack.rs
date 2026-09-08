//! The channel rack (TDD §16.1, item 9 of `docs/first-usable-plan.md`).
//!
//! What instruments the project has, which one the piano roll is looking at,
//! and the two switches — mute and solo — that make a part balanceable without
//! opening a mixer. Everything else about a channel lives in the mixer panel or
//! the sampler editor; this is the list you work from.
//!
//! Geometry and hit-testing only, and both pure, for the reason §2.5 gives.
//! **Virtualised like the roll** (§16.4): a project with two hundred channels
//! builds a screenful of rectangles, not two hundred.

use crate::document::RackTab;
use crate::layout::Rect;
use crate::theme::Metrics;

/// One channel's row, and the controls inside it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RackRow {
    /// Which channel this is, counted from the top of the project's list — not
    /// from the top of the panel, which is what makes scrolling work.
    pub index: usize,
    pub frame: Rect,
    /// Where the name is written, and the part you click to select the channel.
    pub name: Rect,
    pub mute: Rect,
    pub solo: Rect,
    /// Opens this channel's instrument in the editor column. The answer to
    /// "why can't I open up the VST options" — there was nothing to click.
    pub edit: Rect,
    /// Where this channel's audio goes, as the mixer's own number — see
    /// [`route_label`]. Pressing it opens [`route_menu_layout`]'s menu.
    ///
    /// A channel does not own a mixer track any more (see
    /// `fontelle_model::Channel::mixer_track`), so "where does this go" became
    /// a question the rack has to be able to both ask and answer. Two
    /// characters wide, because a soundfont's name is the longest thing on the
    /// row and it is what the row is for.
    pub route: Rect,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RackLayout {
    pub body: Rect,
    /// The strip that switches between this list and the prefabs. Shared with
    /// [`crate::canvas::prefab_layout`] — see [`tab_strip`].
    pub tabs: Vec<(RackTab, Rect)>,
    /// Where the rows go — the panel above the add button. Its own rectangle
    /// because hit-testing asks "is the pointer in the list" before it asks
    /// "which row", and because the renderer clips to it.
    pub list: Rect,
    pub rows: Vec<RackRow>,
    /// "Add an instrument". Pinned to the bottom of the panel rather than
    /// placed after the last row: with no channels at all it is the only thing
    /// worth clicking, and with two hundred it must not have scrolled away.
    pub add: Rect,
    /// How many channels there are in total, so a scrollbar can be drawn and a
    /// scroll offset clamped.
    pub total: usize,
    pub scroll: usize,
    /// How many rows the list has **room** for, whatever it is showing.
    ///
    /// Not `rows.len()`, which is what the current offset happens to reach:
    /// scrolled to the end of a long list those two disagree, and the one a
    /// scroll offset has to be clamped against is this one. Clamping to
    /// `total - 1` instead is what leaves a full panel showing a single row.
    pub capacity: usize,
}

/// How wide the mute and solo squares are.
const SWITCH: f32 = 18.0;

/// The strip of tabs across the top of the panel, and what is left under it.
///
/// > *"the prefab tab should be where the channel rack is can be tabbed
/// > between instruments and prefabs."*
///
/// Here rather than in either list, because both lists have to agree about it
/// exactly: a tab strip that moved by a pixel when you pressed it would be a
/// strip you could not press twice.
pub fn tab_strip(body: Rect, metrics: &Metrics) -> (Vec<(RackTab, Rect)>, Rect) {
    let height = metrics.row_height.min(body.height.max(0.0));
    if body.is_empty() || height <= 0.0 {
        return (Vec::new(), body);
    }
    let count = RackTab::ALL.len() as f32;
    let width = body.width / count;
    let tabs = RackTab::ALL
        .into_iter()
        .enumerate()
        .map(|(index, tab)| {
            (
                tab,
                Rect::new(body.x + width * index as f32, body.y, width, height).clamped(),
            )
        })
        .collect();
    let rest = Rect::new(
        body.x,
        body.y + height,
        body.width,
        (body.height - height).max(0.0),
    )
    .clamped();
    (tabs, rest)
}

/// Which tab is under the pointer, if any.
pub fn tab_at(tabs: &[(RackTab, Rect)], x: f32, y: f32) -> Option<RackTab> {
    tabs.iter()
        .find(|(_, rect)| rect.contains(x, y))
        .map(|(tab, _)| *tab)
}

/// And the route chip, which holds two digits.
const ROUTE: f32 = 22.0;

/// Lays out `count` channels in `body`, starting from `scroll`.
pub fn rack_layout(body: Rect, metrics: &Metrics, count: usize, scroll: usize) -> RackLayout {
    let (tabs, body) = tab_strip(body, metrics);
    let add_height = metrics.row_height.min(body.height.max(0.0));
    let add = Rect::new(
        body.x,
        (body.bottom() - add_height).max(body.y),
        body.width,
        add_height,
    )
    .clamped();

    // What is left above the button is what the list gets, rounded down to a
    // whole number of rows for the reason `browser::browser_layout` gives.
    let room = (add.y - body.y - 2.0).max(0.0);
    let height = if metrics.row_height > 0.0 {
        (room / metrics.row_height).floor() * metrics.row_height
    } else {
        room
    };
    let list = Rect::new(body.x, body.y, body.width, height).clamped();

    // Whole rows only, for the reason `browser::rows` sets out: a row clipped
    // to a few pixels still draws its caption centred inside those pixels, on
    // top of the row above it.
    let mut rows = Vec::new();
    let mut capacity = 0;
    if metrics.row_height > 0.0 && !list.is_empty() {
        let visible = (list.height / metrics.row_height).floor() as usize;
        capacity = visible;
        let scroll = scroll.min(count.saturating_sub(1));
        for (slot, index) in (scroll..count).take(visible).enumerate() {
            let frame = Rect::new(
                list.x,
                list.y + slot as f32 * metrics.row_height,
                list.width,
                metrics.row_height,
            );
            // The switches sit at the right-hand end, solo then mute, so the
            // name has the whole of the rest — soundfont names are long.
            let mute = Rect::new(
                frame.right() - SWITCH - 2.0,
                frame.y + 2.0,
                SWITCH,
                (frame.height - 4.0).max(0.0),
            )
            .clamped();
            let solo = Rect::new(mute.x - SWITCH - 2.0, mute.y, SWITCH, mute.height).clamped();
            let edit = Rect::new(solo.x - SWITCH - 2.0, solo.y, SWITCH, solo.height).clamped();
            // The route chip sits left of the three buttons: it is a read-out
            // first and a control second, so it reads with the name rather
            // than with the switches.
            let route = Rect::new(edit.x - ROUTE - 2.0, edit.y, ROUTE, edit.height).clamped();
            let name = Rect::new(
                frame.x,
                frame.y,
                (route.x - frame.x - 2.0).max(0.0),
                frame.height,
            )
            .clamped();
            rows.push(RackRow {
                index,
                frame,
                name,
                mute,
                solo,
                edit,
                route,
            });
        }
    }

    RackLayout {
        body,
        tabs,
        list,
        rows,
        add,
        total: count,
        scroll,
        capacity,
    }
}

/// What is under the pointer in the rack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RackHit {
    /// Select this channel — the roll follows it.
    Row(usize),
    Mute(usize),
    Solo(usize),
    /// Open this channel's instrument in the editor column.
    Edit(usize),
    /// Open the menu of everywhere this channel could play through.
    Route(usize),
    Add,
    /// Show the panel's other list.
    Tab(RackTab),
    Nothing,
}

impl RackHit {
    /// What a hover tip says (see [`crate::tooltip`]).
    ///
    /// `None` for a row, which carries the channel's own name and needs no
    /// gloss.
    pub fn tip(self) -> Option<&'static str> {
        Some(match self {
            Self::Mute(_) => "Silence this channel",
            Self::Solo(_) => "Hear only this channel",
            Self::Edit(_) => "Open this channel's sound",
            Self::Route(_) => "Which mixer track this channel plays through",
            Self::Add => "Add a channel",
            Self::Tab(tab) => match tab {
                RackTab::Instruments => "The instruments in this project",
                RackTab::Prefabs => "Content you can draw in more than one place",
            },
            Self::Row(_) | Self::Nothing => return None,
        })
    }
}

pub fn rack_hit(layout: &RackLayout, x: f32, y: f32) -> RackHit {
    // The strip first: it is inside the panel, and a list that claimed the
    // whole of it would swallow the only way out.
    if let Some(tab) = tab_at(&layout.tabs, x, y) {
        return RackHit::Tab(tab);
    }
    if layout.add.contains(x, y) {
        return RackHit::Add;
    }
    if !layout.list.contains(x, y) {
        return RackHit::Nothing;
    }
    for row in &layout.rows {
        if !row.frame.contains(x, y) {
            continue;
        }
        // The switches first: they are inside the row, and a row that claimed
        // the whole width would swallow them.
        if row.mute.contains(x, y) {
            return RackHit::Mute(row.index);
        }
        if row.solo.contains(x, y) {
            return RackHit::Solo(row.index);
        }
        if row.edit.contains(x, y) {
            return RackHit::Edit(row.index);
        }
        if row.route.contains(x, y) {
            return RackHit::Route(row.index);
        }
        return RackHit::Row(row.index);
    }
    RackHit::Nothing
}

/// The scroll offset that keeps `index` on screen, given where the list is now.
///
/// Called when the selected channel changes by something other than a click —
/// a channel added, or one removed from under the selection — so the panel
/// follows the selection rather than the user having to go and find it.
pub fn scroll_to_show(layout: &RackLayout, index: usize) -> usize {
    if layout.rows.is_empty() {
        return index;
    }
    let first = layout.rows[0].index;
    // The last row may be half off the bottom; the one before it is the last
    // that is really readable.
    let last = layout.rows[layout.rows.len().saturating_sub(2)].index;
    if index < first {
        index
    } else if index > last {
        layout.scroll + (index - last)
    } else {
        layout.scroll
    }
}

// ------------------------------------------------------------- routing ---

/// Where a channel could be sent.
///
/// A choice rather than an index, because "the master" and "a track somebody
/// made" are different answers — `None` is the master in the document (see
/// `fontelle_model::Channel::mixer_track`), and one of the rows is not a
/// destination at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteChoice {
    /// Straight out, which is where a new channel goes.
    Master,
    /// A track somebody made, by its index in the host's `route_names` —
    /// **excluding** the master, which is always the last of those.
    Track(usize),
    /// Make one, and send this channel to it. The row that turns "I need a
    /// drum bus" into one gesture instead of three.
    New,
    /// **Nowhere.** A mixer track whose output is disconnected — see
    /// `fontelle_model::MixerTrack::output_on`.
    ///
    /// Only ever offered by [`output_menu_layout`]: a channel plays somewhere
    /// or it is not playing, and a *send* to nowhere is a send that should be
    /// deleted rather than pointed at nothing.
    Off,
}

/// The menu the route chip drops.
#[derive(Debug, Clone, PartialEq)]
pub struct RouteMenu {
    /// The whole panel, for the background and for "did the click miss".
    pub frame: Rect,
    pub items: Vec<(RouteChoice, Rect)>,
}

impl RouteMenu {
    /// What a row says. `names` is the host's `route_names`, master last.
    pub fn label(&self, choice: RouteChoice, names: &[String]) -> String {
        match choice {
            RouteChoice::Master => names
                .last()
                .cloned()
                .unwrap_or_else(|| "Master".to_string()),
            RouteChoice::Track(index) => names
                .get(index)
                .cloned()
                .unwrap_or_else(|| format!("Track {}", index + 1)),
            RouteChoice::New => NEW_TRACK.to_string(),
            RouteChoice::Off => NO_OUTPUT.to_string(),
        }
    }
}

/// The caption on the output menu's "nowhere" row, in one place so the window
/// can shape exactly the string the renderer will look for.
///
/// It names the *state* rather than the action — the row beside it says
/// "Master", not "route to master" — and the em dash is what the output chip
/// itself shows when it is off, so the menu and the chip read as one thing.
pub const NO_OUTPUT: &str = "\u{2014} none";

/// The caption on the menu's last row, in one place so the window can shape
/// exactly the string the renderer will look for.
pub const NEW_TRACK: &str = "+ New track";

/// A little air around the rows. The same as the lane menu's, because they are
/// the same kind of object and two menus with different padding read as a bug.
const MENU_PAD: f32 = 3.0;

/// Wide enough for a track name, and never narrower than the chip it hangs
/// from.
const MENU_WIDTH: f32 = 132.0;

/// Drops the route menu from `chip`, kept inside `bounds`.
///
/// `names` is the host's `route_names` — every strip, master **last**, the
/// same order the mixer panel lays them out in.
pub fn route_menu_layout(
    chip: Rect,
    bounds: Rect,
    metrics: &Metrics,
    names: &[String],
) -> RouteMenu {
    route_menu_layout_excluding(chip, bounds, metrics, names, None)
}

/// The same, leaving one track out of the list.
///
/// For the track-options column's output row (§13.2): a track routed into
/// itself is the shortest possible feedback loop and the easiest one to click
/// by accident in a menu that lists every track. Longer loops — A into B into
/// A — are refused by `SetTrackOutput` and reported, because whether one
/// exists is a question about the whole routing graph rather than about this
/// menu's own row.
pub fn route_menu_layout_excluding(
    chip: Rect,
    bounds: Rect,
    metrics: &Metrics,
    names: &[String],
    exclude: Option<usize>,
) -> RouteMenu {
    menu_of(destinations(names, exclude), chip, bounds, metrics)
}

/// The **mixer track's** output menu: the same destinations, plus nowhere.
///
/// > *"if i chose to not route it to master, i wont be hearing my own input
/// > but it will still be recording the audio clip."*
///
/// Its own function rather than a flag on the one above, because the extra row
/// is not a variation on a destination — it is the absence of one, and the two
/// other menus over this same list (a channel's route, a send's target) would
/// each mean something wrong by it.
pub fn output_menu_layout(
    chip: Rect,
    bounds: Rect,
    metrics: &Metrics,
    names: &[String],
    exclude: Option<usize>,
) -> RouteMenu {
    let mut choices = destinations(names, exclude);
    // Last, under the rule: it is the one answer you do not reach for by
    // accident, and a first row meaning "off" in a menu of places is a click
    // away from silence.
    choices.push(RouteChoice::Off);
    menu_of(choices, chip, bounds, metrics)
}

/// Master, every track somebody made, and the row that makes one.
fn destinations(names: &[String], exclude: Option<usize>) -> Vec<RouteChoice> {
    let mut choices = vec![RouteChoice::Master];
    choices.extend(
        (0..names.len().saturating_sub(1))
            .filter(|index| Some(*index) != exclude)
            .map(RouteChoice::Track),
    );
    choices.push(RouteChoice::New);
    choices
}

/// Lays `choices` out under `chip`, kept inside `bounds`.
fn menu_of(choices: Vec<RouteChoice>, chip: Rect, bounds: Rect, metrics: &Metrics) -> RouteMenu {
    let row = metrics.row_height.max(1.0);
    let width = MENU_WIDTH.max(chip.width).min(bounds.width);
    let height = row * choices.len() as f32 + MENU_PAD * 2.0;
    if bounds.is_empty() || width <= 0.0 || height > bounds.height {
        // Nowhere to put it. An empty menu hit-tests as absent and draws as
        // nothing, which is better than a sliver listing two of six things.
        return RouteMenu {
            frame: Rect::ZERO,
            items: Vec::new(),
        };
    }

    // Below the chip by preference — a menu over the control it belongs to
    // hides what it is doing — and above it when there is no room below.
    let below = chip.bottom();
    let y = if below + height <= bounds.bottom() {
        below
    } else {
        (chip.y - height).max(bounds.y)
    };
    // Right-aligned on the chip rather than left: the chip is near the right
    // edge of a narrow sidebar, and a menu hanging off its left corner is one
    // that has to be pushed back in every time.
    let x = (chip.right() - width).clamp(bounds.x, (bounds.right() - width).max(bounds.x));
    let frame = Rect::new(x, y, width, height).intersection(&bounds);

    let items = choices
        .into_iter()
        .enumerate()
        .map(|(index, choice)| {
            (
                choice,
                Rect::new(
                    frame.x + MENU_PAD,
                    frame.y + MENU_PAD + row * index as f32,
                    (frame.width - MENU_PAD * 2.0).max(0.0),
                    row,
                ),
            )
        })
        .collect();
    RouteMenu { frame, items }
}

/// Which row is under the pointer, if any.
pub fn route_menu_hit(menu: &RouteMenu, x: f32, y: f32) -> Option<RouteChoice> {
    menu.items
        .iter()
        .find(|(_, rect)| rect.contains(x, y))
        .map(|(choice, _)| *choice)
}

/// What the chip says: the mixer's own number for where the channel goes.
///
/// **The master is 0**, and every track somebody made counts up from one —
/// which is the numbering every mixer uses and what makes "send the drums to
/// 3" a sentence. `strips` is how many there are in total, master included, so
/// that a route naming the master's own strip reads the same as `None`.
pub fn route_label(route: Option<usize>, strips: usize) -> String {
    match route {
        // The master is the last strip, so naming it by index is the same
        // answer as naming it by absence.
        Some(index) if index + 1 < strips => (index + 1).to_string(),
        _ => "0".to_string(),
    }
}
