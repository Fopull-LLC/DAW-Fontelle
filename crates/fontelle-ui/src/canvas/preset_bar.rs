//! The preset bar, drawn into the header of **every** editor window
//! (`docs/flopsynth-plan.md` §P.7).
//!
//! ```text
//! │ ◀ ▶ │ Choir Ahh*  ▾ │ Choir & Vocal │ ★ │ Save │ Save as… │
//! ```
//!
//! One widget for the knob grid, the Flopsynth window, the EQ's curve, the
//! effect grid and a hosted plugin's panel, because a preset is a property of
//! *a device* and not of any one of those. That is the whole of §P: what a
//! device contributes is nothing but what its state is, and everything else —
//! the file, the name, the star, the undo — is the same code however the
//! device draws itself.
//!
//! Pure geometry, like every other module here. What the bar *says* is worked
//! out by the host from the bank (`Session::preset_state`, §P.6); what is
//! decided here is where the seven controls go, which of them fit, and which
//! of them a press can land on.
//!
//! # Two rules worth stating
//!
//! - **A control that is not drawn cannot be pressed.** A rectangle that did
//!   not fit is [`Rect::ZERO`], the renderer skips it and [`preset_bar_hit`]
//!   cannot land on it — the same one rule `ContextMenu::rows` uses for a row
//!   scrolled out of sight, and for the same reason: what is drawn and what is
//!   clickable must not be two lists that can disagree.
//! - **Disabled is drawn.** *Save* on a factory preset is there and does
//!   nothing, because hiding it teaches nobody why it is not there. The hit
//!   test knows it is dead; the renderer draws it muted.

use fontelle_types::PresetOrigin;

use super::menu::MenuEntry;
use crate::layout::Rect;
use crate::theme::Metrics;

/// Which device a bar, a menu or a save is about.
///
/// The two shapes an editor window can be open on. An insert carries the two
/// numbers the effect window already addresses its controls with — the mixer
/// strip and the slot — because the host is stateless about which insert is
/// open and the window is not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresetDevice {
    /// The selected channel's instrument.
    Instrument,
    Insert {
        strip: usize,
        slot: usize,
    },
}

/// What the bar shows, for one device.
///
/// Built by the host, which is the only layer that can see both the device's
/// current state and the bank's copy of the file it came from.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PresetBarView {
    /// `None` is a device that came from no preset at all — a real state, and
    /// the one every channel starts in.
    pub name: Option<String>,
    pub category: String,
    pub origin: Option<PresetOrigin>,
    /// Whether the device has drifted from its file (§P.6). **Never stored**:
    /// recognised by comparing the state with the bank's, so an undo makes it
    /// go out with nothing to remember.
    pub dirty: bool,
    pub favourite: bool,
    /// Whether *Save* can write. False on a factory preset (read-only) and on
    /// a device with no ref (there is no file to write over).
    pub can_save: bool,
}

/// What a device with no preset is called.
pub const NO_PRESET: &str = "\u{2014} no preset \u{2014}";

/// The name the bar draws, `*` and all.
///
/// The `*` goes on the *name*, including on "— no preset —": a device somebody
/// has been turning knobs on has unsaved work whether or not it ever had a
/// name, and the bar saying so is what makes "Save as…" look like the thing to
/// press.
pub fn preset_bar_name(view: &PresetBarView) -> String {
    let name = view.name.as_deref().unwrap_or(NO_PRESET);
    match view.dirty {
        true => format!("{name}*"),
        false => name.to_string(),
    }
}

/// Where the bar's controls are.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PresetBarLayout {
    /// Everything together — for the background, and for "did the press miss".
    pub frame: Rect,
    pub previous: Rect,
    pub next: Rect,
    /// The name and its `▾`, which is one control: the whole box opens the
    /// drop-down.
    pub name: Rect,
    pub category: Rect,
    pub star: Rect,
    pub save: Rect,
    pub save_as: Rect,
}

impl Default for PresetBarLayout {
    /// A bar with no room for even its name: every rectangle empty, which the
    /// renderer skips and the hit test cannot land on.
    fn default() -> Self {
        Self {
            frame: Rect::ZERO,
            previous: Rect::ZERO,
            next: Rect::ZERO,
            name: Rect::ZERO,
            category: Rect::ZERO,
            star: Rect::ZERO,
            save: Rect::ZERO,
            save_as: Rect::ZERO,
        }
    }
}

impl PresetBarLayout {
    /// Every control with its name, for the tests that hold "nothing overlaps"
    /// and "nothing escapes the header".
    pub fn controls(&self) -> [(&'static str, Rect); 7] {
        [
            ("previous", self.previous),
            ("next", self.next),
            ("name", self.name),
            ("category", self.category),
            ("star", self.star),
            ("save", self.save),
            ("save as", self.save_as),
        ]
    }
}

/// What was pressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresetBarHit {
    /// The previous entry of the device's bank, wrapping.
    Previous,
    Next,
    /// The name: opens the drop-down.
    Name,
    /// The category, which opens the same drop-down. It is a second target for
    /// one menu rather than a control of its own, because "show me the others
    /// like this" and "show me the list" are the same list.
    Category,
    Star,
    Save,
    SaveAs,
}

// The widths. Fixed rather than measured, for `editor_tabs`' reason: a header
// whose controls moved as the preset's name changed length would be a row of
// buttons that will not hold still under the pointer.
const ARROW: f32 = 22.0;
const NAME_WIDTH: f32 = 168.0;
/// The least the name box shrinks to before the bar gives up altogether.
const NAME_MIN: f32 = 96.0;
const CATEGORY_WIDTH: f32 = 132.0;
const STAR: f32 = 24.0;
const SAVE_WIDTH: f32 = 52.0;
const SAVE_AS_WIDTH: f32 = 82.0;
const GAP: f32 = 3.0;
/// A wider gap where the drawing shows a `│`: between the arrows and the name,
/// and between the star and the two save buttons.
const GROUP_GAP: f32 = 9.0;

/// Lays the bar out at the right-hand end of `header`.
///
/// `reserved_left` is how much of the header the window's own title took. A
/// canvas has no font and cannot measure text, so the host — which laid the
/// title out — passes the answer in. The same arrangement every other pure
/// layout function in this crate has with text.
///
/// **What drops out first when there is no room** is stated here rather than
/// discovered: the category, then the star, then *Save as…*, then *Save*, then
/// the two arrows. The name never drops, because it is the one thing on the
/// bar with no other route to it — every button is a gesture that exists
/// somewhere else too.
pub fn preset_bar_layout(
    header: Rect,
    reserved_left: f32,
    view: &PresetBarView,
    metrics: &Metrics,
) -> PresetBarLayout {
    let inset = (header.height * 0.15).min(4.0);
    let height = (header.height - inset * 2.0).max(0.0);
    let y = header.y + inset;
    let right = header.right() - metrics.panel_padding.min(header.width);
    let left = header.x + reserved_left;
    let room = (right - left).max(0.0);

    // Every control, in the order they are given up, with what dropping it
    // gives back — the width and the gap that went with it.
    let (mut category, mut star, mut save, mut save_as, mut arrows) =
        (true, true, true, true, true);
    let mut others = ARROW * 2.0
        + GAP
        + GROUP_GAP
        + GAP
        + CATEGORY_WIDTH
        + GROUP_GAP
        + STAR
        + GROUP_GAP
        + SAVE_WIDTH
        + GAP
        + SAVE_AS_WIDTH;
    for (fits, cost) in [
        (&mut category, CATEGORY_WIDTH + GAP),
        (&mut star, STAR + GROUP_GAP),
        (&mut save_as, SAVE_AS_WIDTH + GAP),
        (&mut save, SAVE_WIDTH + GROUP_GAP),
        (&mut arrows, ARROW * 2.0 + GAP + GROUP_GAP),
    ] {
        if others + NAME_MIN <= room {
            break;
        }
        *fits = false;
        others -= cost;
    }
    // The name takes whatever is left, between the width it reads best at and
    // the least it can say anything in. **It flexes and the buttons do not**,
    // because a button that changed width would move under the pointer while a
    // name that does is still a name.
    let name_width = (room - others).clamp(NAME_MIN, NAME_WIDTH);
    let needed = others + name_width;
    // Even the name may not fit, and then there is no bar at all rather than a
    // sliver of one.
    if needed > room || height <= 0.0 {
        return PresetBarLayout::default();
    }

    let mut x = right - needed;
    let frame = Rect::new(x, y, needed, height);
    let mut take = |width: f32, gap: f32| {
        x += gap;
        let rect = Rect::new(x, y, width, height).intersection(&header);
        x += width;
        rect
    };
    let (previous, next) = match arrows {
        true => (take(ARROW, 0.0), take(ARROW, GAP)),
        false => (Rect::ZERO, Rect::ZERO),
    };
    let name = take(name_width, if arrows { GROUP_GAP } else { 0.0 });
    let category = match category {
        true => take(CATEGORY_WIDTH, GAP),
        false => Rect::ZERO,
    };
    let star = match star {
        true => take(STAR, GROUP_GAP),
        false => Rect::ZERO,
    };
    let save = match save {
        true => take(SAVE_WIDTH, GROUP_GAP),
        false => Rect::ZERO,
    };
    let save_as = match save_as {
        true => take(SAVE_AS_WIDTH, GAP),
        false => Rect::ZERO,
    };
    let _ = view;
    PresetBarLayout {
        frame,
        previous,
        next,
        name,
        category,
        star,
        save,
        save_as,
    }
}

/// What a press at `(x, y)` lands on.
///
/// Takes the view as well as the layout because two of the controls are dead
/// in states the geometry cannot see: *Save* on a read-only preset, and the
/// arrows on a device whose bank is empty — a press on either must land on
/// nothing rather than on the control beside it.
pub fn preset_bar_hit(
    layout: &PresetBarLayout,
    view: &PresetBarView,
    x: f32,
    y: f32,
) -> Option<PresetBarHit> {
    for (hit, rect) in [
        (PresetBarHit::Previous, layout.previous),
        (PresetBarHit::Next, layout.next),
        (PresetBarHit::Name, layout.name),
        (PresetBarHit::Category, layout.category),
        (PresetBarHit::Star, layout.star),
        (PresetBarHit::Save, layout.save),
        (PresetBarHit::SaveAs, layout.save_as),
    ] {
        if rect.is_empty() || !rect.contains(x, y) {
            continue;
        }
        if hit == PresetBarHit::Save && !view.can_save {
            return None;
        }
        return Some(hit);
    }
    None
}

/// One preset the drop-down can offer.
///
/// The host's list, in the bank's own order — factory then user, category then
/// name — because that is the order the menu shows and re-sorting it here
/// would be a second opinion about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresetChoice {
    pub name: String,
    pub category: String,
    pub origin: PresetOrigin,
    pub favourite: bool,
}

/// What a row of the drop-down means.
///
/// A pair with the entries rather than a list of strings whose meaning is an
/// index into a second list — `favorites.rs`'s shape, and its reason: a menu
/// that has to be counted is a menu that breaks the day a heading is added.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresetMenuRow {
    /// A caption — the *Favorites* heading or a category's. Does nothing.
    Heading,
    /// One preset, by its position in the `choices` the menu was built from.
    Preset(usize),
}

/// A mark at the right end of a row that came from the user's own bank.
///
/// So a preset named like a factory one is *tellable* from it rather than
/// merely present twice (§P.3).
pub const USER_MARK: &str = "  \u{00b7} mine";

/// What the drop-down's first row says.
///
/// The plugin picker's rule, for the same reason: a hundred and twenty-eight
/// presets is a list to be *searched*, not only scrolled, and the row that
/// shows what has been typed is what makes the narrowing visible.
pub const PRESET_MENU_HEADING: &str = "Presets";

/// The drop-down's rows, grouped by category with favourites first, narrowed
/// to what matches `query` — a plain substring of the name, ignoring case
/// ([`menu_matches`](super::menu_matches)).
///
/// A category with nothing left in it after the narrowing is not shown, and
/// neither is a Favorites section with no favourite in it: a heading over
/// nothing is a heading that says the search failed where it did not.
pub fn preset_menu(choices: &[PresetChoice], query: &str) -> (Vec<MenuEntry>, Vec<PresetMenuRow>) {
    let mut entries = Vec::new();
    let mut rows = Vec::new();

    if choices.is_empty() {
        entries.push(MenuEntry::disabled("No presets"));
        rows.push(PresetMenuRow::Heading);
        return (entries, rows);
    }

    let query = query.trim();
    entries.push(MenuEntry::disabled(if query.is_empty() {
        format!("{PRESET_MENU_HEADING} \u{2014} type to filter")
    } else {
        format!("{PRESET_MENU_HEADING} \u{2014} {query}")
    }));
    rows.push(PresetMenuRow::Heading);

    let matching: Vec<(usize, &PresetChoice)> = choices
        .iter()
        .enumerate()
        .filter(|(_, choice)| super::menu_matches(&choice.name, query))
        .collect();
    if matching.is_empty() {
        entries.push(MenuEntry::disabled("nothing matches"));
        rows.push(PresetMenuRow::Heading);
        return (entries, rows);
    }

    let push = |entries: &mut Vec<MenuEntry>,
                rows: &mut Vec<PresetMenuRow>,
                index: usize,
                choice: &PresetChoice| {
        let label = match choice.origin {
            PresetOrigin::User => format!("{}{USER_MARK}", choice.name),
            PresetOrigin::Factory => choice.name.clone(),
        };
        entries.push(MenuEntry::new(label).starred(choice.favourite));
        rows.push(PresetMenuRow::Preset(index));
    };

    // The rule every menu here follows: if anything is starred, a Favorites
    // section goes first and holds all of them.
    if matching.iter().any(|(_, choice)| choice.favourite) {
        entries.push(MenuEntry::disabled(super::favorites::FAVORITES_HEADING));
        rows.push(PresetMenuRow::Heading);
        for (index, choice) in matching.iter().filter(|(_, c)| c.favourite) {
            push(&mut entries, &mut rows, *index, choice);
        }
    }

    let mut heading: Option<&str> = None;
    for (index, choice) in &matching {
        if heading != Some(choice.category.as_str()) {
            let mut entry = MenuEntry::disabled(choice.category.clone());
            // A rule above every category but the first, so the sections read
            // as sections rather than as one long list with bold lines in it.
            entry.separator = entries.len() > 1;
            entries.push(entry);
            rows.push(PresetMenuRow::Heading);
            heading = Some(choice.category.as_str());
        }
        push(&mut entries, &mut rows, *index, choice);
    }
    (entries, rows)
}
