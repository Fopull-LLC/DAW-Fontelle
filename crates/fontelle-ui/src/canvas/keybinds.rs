//! The keyboard shortcuts page: every key the window answers, on one sheet.
//!
//! > *"a dedicated keybinds page that shows every single keybind in the entire
//! > app organized cleanly sectioned to be easily understandable and grouped.
//! > should be accessible from the start screen with a new ? icon, and should
//! > also be accessible within a project by a new small ? icon next to the
//! > tempo indicator"*
//!
//! Two halves. The **catalogue**, [`KEYBIND_SECTIONS`], is the one place the
//! bindings are written down for a person: the window's `key` and `global_key`
//! match on them and the tooltips name a few, but until this there was no way
//! to find out that `A` toggles a slide or that Alt frees a drag from the grid
//! except by reading the source. `tests/keybinds.rs` holds the catalogue to
//! the bindings the window actually answers, so the two cannot drift apart
//! silently. The **sheet**, [`keybinds_layout`], is the card the catalogue is
//! drawn on: sections in as many columns as the window has room for, the key
//! in a chip on the left of each line and what it does on the right, with a
//! close button and a scroll — geometry only, like every canvas here
//! (INVARIANT 2).
//!
//! The catalogue is data rather than derived from the key handlers because the
//! handlers are spread over a dozen functions and a few of the bindings are
//! *conditions* — `M` mutes a strip only while the mixer is showing, Ctrl+L is
//! legato with notes in hand and the play mode without — that no enum of
//! actions would say in the words a person needs. Where a binding is a
//! modifier held during a mouse gesture it is listed under the mouse, because
//! that is where somebody looking for "how do I place this off the grid" will
//! look.

use crate::layout::Rect;
use crate::theme::Metrics;

use super::keymap::{Action, Keymap};

/// One line of the catalogue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeybindEntry {
    /// A rebindable action: its key is whatever the keymap says, and a press
    /// on its row starts listening for a new one.
    Action(Action),
    /// A binding that is not the keymap's to change — a mouse gesture, a
    /// text field's keys, the arrows, Esc — written down as it is.
    Fixed {
        /// As written on the sheet: `Shift+drag`, `Home / End`. A slash or a
        /// comma separates alternatives.
        keys: &'static str,
        /// In a person's words, present tense, no full stop.
        does: &'static str,
    },
}

impl KeybindEntry {
    /// What the key column says, given the map as it stands.
    pub fn keys(&self, keymap: &Keymap) -> String {
        match self {
            Self::Action(action) => keymap.label(*action),
            Self::Fixed { keys, .. } => (*keys).to_string(),
        }
    }

    pub fn does(&self) -> &'static str {
        match self {
            Self::Action(action) => action.does(),
            Self::Fixed { does, .. } => does,
        }
    }

    /// The action, when this row can be rebound.
    pub fn action(&self) -> Option<Action> {
        match self {
            Self::Action(action) => Some(*action),
            Self::Fixed { .. } => None,
        }
    }
}

/// A heading and the bindings under it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeybindSection {
    pub title: &'static str,
    pub binds: &'static [KeybindEntry],
}

const fn act(action: Action) -> KeybindEntry {
    KeybindEntry::Action(action)
}

const fn fixed(keys: &'static str, does: &'static str) -> KeybindEntry {
    KeybindEntry::Fixed { keys, does }
}

/// What the sheet is called.
pub const KEYBINDS_TITLE: &str = "Keyboard shortcuts";
/// The line under the title.
pub const KEYBINDS_HINT: &str =
    "Click a shortcut to change it. Esc closes this page; Ctrl keys work everywhere.";
/// The line under the title while a row is listening.
pub const KEYBINDS_LISTENING: &str = "Press the new shortcut \u{2014} hold Ctrl, Shift or Alt with it if you like. Esc keeps the old one.";
/// What a listening row's chip says.
pub const KEYBINDS_PRESS: &str = "Press keys\u{2026}";
/// The close button's glyph.
pub const KEYBINDS_CLOSE: &str = "\u{2715}";
/// The button that puts every default back.
pub const KEYBINDS_RESET: &str = "Reset to defaults";

/// Every binding, grouped the way somebody would look for them.
pub const KEYBIND_SECTIONS: &[KeybindSection] = &[
    KeybindSection {
        title: "Transport",
        binds: &[
            act(Action::Play),
            act(Action::Stop),
            act(Action::Metronome),
            act(Action::LegatoOrPlayMode),
        ],
    },
    KeybindSection {
        title: "Project",
        binds: &[
            act(Action::Save),
            act(Action::Undo),
            act(Action::Redo),
            act(Action::ExportWav),
            act(Action::ExportMidi),
            act(Action::Help),
            fixed(
                "Esc",
                "Shut a menu, a window or this page; else drop the selection",
            ),
        ],
    },
    KeybindSection {
        title: "Panels and views",
        binds: &[
            act(Action::ShowRoll),
            act(Action::ShowMixer),
            act(Action::RackTab),
            act(Action::SwapSplit),
            act(Action::ToggleTimeline),
            act(Action::ToolsPanel),
            act(Action::Search),
            act(Action::ZoomIn),
            act(Action::ZoomOut),
        ],
    },
    KeybindSection {
        title: "Tools",
        binds: &[
            act(Action::DrawTool),
            act(Action::PaintTool),
            act(Action::SelectTool),
            act(Action::DeleteTool),
            act(Action::SliceTool),
            act(Action::SnapOrStretch),
            act(Action::Slide),
            act(Action::LaneProperty),
            act(Action::Ghosts),
        ],
    },
    KeybindSection {
        title: "Editing notes and clips",
        binds: &[
            act(Action::SelectAll),
            act(Action::Copy),
            act(Action::Cut),
            act(Action::Paste),
            act(Action::Duplicate),
            act(Action::DeleteSelection),
            act(Action::MuteClips),
            fixed("\u{2190} \u{2192}", "Move one snap step"),
            fixed(
                "\u{2191} \u{2193}",
                "Move a semitone (notes) or a row (clips)",
            ),
            fixed("Ctrl+\u{2190} \u{2192}", "Move a bar"),
            fixed("Ctrl+\u{2191} \u{2193}", "Move an octave"),
            fixed("Shift+\u{2190} \u{2192}", "Change the length"),
            fixed("Shift+\u{2191} \u{2193}", "Nudge the property lane's value"),
            fixed("Ctrl+Shift+arrows", "The same, by a bar / by ten"),
        ],
    },
    KeybindSection {
        title: "Mixer",
        binds: &[act(Action::MuteTrack), act(Action::SoloTrack)],
    },
    KeybindSection {
        title: "Lists and menus",
        binds: &[
            fixed(
                "\u{2191} \u{2193}",
                "Move through a menu, the presets list or a settings row",
            ),
            fixed("Enter", "Choose the row"),
            fixed("Type", "Filter a menu or the plugin picker as you type"),
            fixed(
                "\u{2190} \u{2192} / \u{2191} \u{2193}",
                "Nudge a focused settings value by one",
            ),
            fixed("Esc", "Let go"),
        ],
    },
    KeybindSection {
        title: "Typing in a field",
        binds: &[
            fixed("Ctrl+A", "Select all"),
            fixed("Ctrl+C / Ctrl+X / Ctrl+V", "Copy, cut, paste"),
            fixed("Home / End", "Start or end of the text"),
            fixed("Shift+arrows", "Extend the selection"),
            fixed("Ctrl+\u{2190} \u{2192}", "A word at a time"),
            fixed("Enter", "Done"),
            fixed("Esc", "Leave the field"),
        ],
    },
    KeybindSection {
        title: "Editor windows",
        binds: &[
            fixed("Esc", "Close the window"),
            act(Action::RemoveBand),
            fixed(
                "Space, Ctrl+Z, Ctrl+S\u{2026}",
                "The transport and project keys work here too",
            ),
        ],
    },
    KeybindSection {
        title: "Mouse: held keys",
        binds: &[
            fixed(
                "Shift",
                "Fine drag on a knob, fader or the tempo; a note along one axis",
            ),
            fixed("Alt", "Drag free of the grid"),
            fixed(
                "Ctrl",
                "Marquee select with any tool; add a preset as a new channel",
            ),
            fixed("Ctrl / Shift", "Preview a sound an octave down / up"),
            fixed("Ctrl+wheel", "Zoom in time"),
            fixed("Ctrl+Shift+wheel / Alt+wheel", "Zoom vertically"),
            fixed("Shift+wheel", "Scroll sideways"),
        ],
    },
    KeybindSection {
        title: "Mouse: clicks",
        binds: &[
            fixed("Right-click a note", "Delete it, whatever the tool"),
            fixed("Right-click a control", "Automate it"),
            fixed("Click the tempo", "Type a tempo; drag to slide it"),
            fixed("Double-click the arrangement", "A new blank clip"),
            fixed(
                "Shift+click the arrangement",
                "A copy of the last clip you picked",
            ),
            fixed("Shift+drag a clip's right edge", "Make it loop"),
            fixed("Double-click an audio clip", "Open its editor"),
            fixed(
                "Click a sound",
                "Preview it; double-click to choose or import",
            ),
            fixed("Ctrl+click an EQ band", "Remove the band"),
            fixed("Ctrl+click a chooser", "Step it backwards"),
        ],
    },
];

/// The card at its widest; a narrower window gets a narrower card.
const CARD_WIDTH: f32 = 1040.0;
/// Between the card and the window's edge.
const MARGIN: f32 = 24.0;
/// Inside the card's edge.
const PADDING: f32 = 20.0;
/// Between the columns.
const GUTTER: f32 = 32.0;
/// A column narrower than this is a column whose descriptions are cut off
/// — nothing here wraps — so the sheet drops to one column instead. The
/// longest description in the catalogue fits beside its chip at this width.
const COLUMN_MIN_WIDTH: f32 = 470.0;
/// The most columns the sheet lays sections in.
const MAX_COLUMNS: usize = 2;
/// The key chip's column, and the gap to the description.
const KEYS_WIDTH: f32 = 160.0;
const KEYS_GAP: f32 = 12.0;
/// Air above a heading, so sections read as sections.
const SECTION_GAP: f32 = 14.0;
/// The reset button's width.
const RESET_WIDTH: f32 = 132.0;

/// One line of the sheet.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum KeybindRow {
    /// A section's title.
    Heading { section: usize, rect: Rect },
    /// One binding: the key chip and, beside it, what it does. `action` is
    /// `Some` for a row a press can rebind.
    Bind {
        section: usize,
        index: usize,
        action: Option<Action>,
        keys: Rect,
        does: Rect,
    },
}

/// Where everything on the sheet is.
#[derive(Debug, Clone, PartialEq)]
pub struct KeybindsLayout {
    /// The card. A press outside it is a dismissal.
    pub frame: Rect,
    pub title: Rect,
    /// The line under the title.
    pub hint: Rect,
    /// The × in the top right corner.
    pub close: Rect,
    /// *Reset to defaults*, beside the ×.
    pub reset: Rect,
    /// The scrolling list. Rows are laid out only where they intersect it.
    pub body: Rect,
    /// The rows that are at least partly in the body, at their scrolled
    /// positions. Clipped to the body by the renderer, not here, so a row
    /// half off the top still draws its visible half.
    pub rows: Vec<KeybindRow>,
    /// How tall the whole catalogue is at this width, scrolled or not. What
    /// [`keybinds_scroll_max`] is measured from.
    pub content_height: f32,
    /// The scroll this was laid out at, clamped.
    pub scroll: f32,
}

/// Lays the sheet out in `window`, scrolled `scroll` points down the list.
pub fn keybinds_layout(window: Rect, metrics: &Metrics, scroll: f32) -> KeybindsLayout {
    // Whole points, so rows that abut are laid out abutting rather than a
    // float's width apart or over one another.
    let row = metrics.row_height.round().max(1.0);
    let heading = (row * 1.35).round().max(1.0);
    let width = (window.width - 2.0 * MARGIN).clamp(0.0, CARD_WIDTH);
    let height = (window.height - 2.0 * MARGIN).max(0.0);
    let frame = Rect::new(
        window.x + (window.width - width) / 2.0,
        window.y + (window.height - height) / 2.0,
        width,
        height,
    )
    .clamped();
    let inner = frame.inset(PADDING).clamped();

    let close = Rect::new(inner.right() - row, inner.y, row, row)
        .intersection(&inner)
        .clamped();
    let reset = Rect::new(
        close.x - KEYS_GAP - RESET_WIDTH,
        inner.y,
        RESET_WIDTH.min((close.x - KEYS_GAP - inner.x).max(0.0)),
        row,
    )
    .intersection(&inner)
    .clamped();
    let title = Rect::new(
        inner.x,
        inner.y,
        (reset.x - KEYS_GAP - inner.x).max(0.0),
        row,
    )
    .intersection(&inner)
    .clamped();
    let hint = Rect::new(inner.x, title.bottom(), inner.width, row)
        .intersection(&inner)
        .clamped();
    let body_top = hint.bottom() + SECTION_GAP;
    let body = Rect::new(
        inner.x,
        body_top,
        inner.width,
        (inner.bottom() - body_top).max(0.0),
    )
    .intersection(&inner)
    .clamped();

    // How many columns fit, and how wide each is.
    let columns = (((body.width + GUTTER) / (COLUMN_MIN_WIDTH + GUTTER)).floor() as usize)
        .clamp(1, MAX_COLUMNS);
    let column_width = ((body.width - GUTTER * (columns as f32 - 1.0)) / columns as f32).max(0.0);
    let keys_width = KEYS_WIDTH.min(column_width * 0.45).max(0.0);
    let does_x = keys_width + KEYS_GAP;
    let does_width = (column_width - does_x).max(0.0);

    // Each section goes into the shortest column so far, unscrolled; the
    // rows are then shifted by the scroll and kept only where they meet the
    // body. Sections are never split across columns — a heading with its
    // list in the next column is a list you have to hunt for.
    let mut heights = vec![0.0_f32; columns];
    let mut placed: Vec<KeybindRow> = Vec::new();
    for (index, section) in KEYBIND_SECTIONS.iter().enumerate() {
        let (column, _) = heights
            .iter()
            .enumerate()
            .min_by(|a, b| a.1.total_cmp(b.1))
            .expect("at least one column");
        let x = body.x + column as f32 * (column_width + GUTTER);
        let mut y = heights[column];
        if y > 0.0 {
            y += SECTION_GAP;
        }
        placed.push(KeybindRow::Heading {
            section: index,
            rect: Rect::new(x, y, column_width, heading),
        });
        y += heading;
        for (bind_index, entry) in section.binds.iter().enumerate() {
            placed.push(KeybindRow::Bind {
                section: index,
                index: bind_index,
                action: entry.action(),
                keys: Rect::new(x, y, keys_width, row),
                does: Rect::new(x + does_x, y, does_width, row),
            });
            y += row;
        }
        heights[column] = y;
    }
    let content_height = heights.iter().copied().fold(0.0_f32, f32::max);
    let scroll = scroll.clamp(0.0, (content_height - body.height).max(0.0));

    let shift =
        |r: Rect| Rect::new(r.x, (r.y + body.y - scroll).round(), r.width, r.height).clamped();
    let rows = placed
        .into_iter()
        .filter_map(|row| match row {
            KeybindRow::Heading { section, rect } => {
                let rect = shift(rect);
                rect.intersects(&body)
                    .then_some(KeybindRow::Heading { section, rect })
            }
            KeybindRow::Bind {
                section,
                index,
                action,
                keys,
                does,
            } => {
                let (keys, does) = (shift(keys), shift(does));
                (keys.intersects(&body) || does.intersects(&body)).then_some(KeybindRow::Bind {
                    section,
                    index,
                    action,
                    keys,
                    does,
                })
            }
        })
        .collect();

    KeybindsLayout {
        frame,
        title,
        hint,
        close,
        reset,
        body,
        rows,
        content_height,
        scroll,
    }
}

/// How far the list can be scrolled: what is off the bottom of the body.
pub fn keybinds_scroll_max(layout: &KeybindsLayout) -> f32 {
    (layout.content_height - layout.body.height).max(0.0)
}

/// How far one wheel notch moves the list.
const SCROLL_STEP: f32 = 48.0;

/// The scroll `notches` wheel notches from `scroll`, clamped. Up is positive,
/// the way the wheel reports it, so a notch up scrolls toward the top.
pub fn keybinds_scrolled(layout: &KeybindsLayout, scroll: f32, notches: f32) -> f32 {
    (scroll - notches * SCROLL_STEP).clamp(0.0, keybinds_scroll_max(layout))
}

/// What a press means while the sheet is up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeybindsHit {
    /// The × — close the sheet.
    Close,
    /// *Reset to defaults*.
    Reset,
    /// A rebindable row — start listening for its new shortcut.
    Row(Action),
    /// Somewhere else on the card — nothing; the sheet stays.
    Card,
    /// Off the card — close it, the way a click away from a menu shuts it.
    Outside,
}

pub fn keybinds_hit(layout: &KeybindsLayout, x: f32, y: f32) -> KeybindsHit {
    if layout.close.contains(x, y) {
        return KeybindsHit::Close;
    }
    if layout.reset.contains(x, y) {
        return KeybindsHit::Reset;
    }
    if layout.body.contains(x, y) {
        // The whole line, chip and words: a target the width of the chip
        // alone is one you have to aim at.
        for row in &layout.rows {
            if let KeybindRow::Bind {
                action: Some(action),
                keys,
                does,
                ..
            } = row
                && (keys.contains(x, y) || does.contains(x, y))
            {
                return KeybindsHit::Row(*action);
            }
        }
    }
    if layout.frame.contains(x, y) {
        KeybindsHit::Card
    } else {
        KeybindsHit::Outside
    }
}
