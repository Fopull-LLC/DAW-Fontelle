//! The preset drop-down at the top of a plugin's own window.
//!
//! > *"when clicking the preset dropdown on the top of the plugin window it
//! > didnt drop down any of those presets for me to select there or hit
//! > random preset to get a random one, it just opened the preset tab on the
//! > left."*
//!
//! The strip is a picture the studio draws into the plugin's window
//! (`fontelle_host::PluginWindow::set_header`), and so is this: a second
//! picture under the strip's name, over the plugin's area
//! (`PluginWindow::show_overlay`), which has the pointer and the keyboard
//! while it is up. The window reports what was done to it in its own pixels;
//! this works out what that means — purely, like every other menu here — and
//! the studio does it.
//!
//! The rows are the studio's own preset drop-down's
//! ([`preset_menu_marking`]) — the user's presets first, then the plugin's
//! library by category, the one playing marked, Random — with Previous and
//! Next, which leave the menu up so a bank can be walked by ear, and *Show in
//! browser* at the foot for the tab the name used to open.

use super::menu::{
    ContextMenu, MenuEntry, context_menu_hit, context_menu_layout, context_menu_star_hit,
};
use super::preset_bar::{
    PRESET_MENU_HEADING, PluginHeaderView, PresetBarHit, PresetChoice, PresetDevice, PresetMenuRow,
    plugin_header_hit, plugin_header_layout, preset_menu_marking, random_preset_row,
};
use crate::layout::Rect;
use crate::theme::Metrics;

/// What a row of the plugin window's drop-down does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginMenuRow {
    /// The search line, a category: nothing.
    Heading,
    /// The preset at this place in the device's choices.
    Preset(usize),
    /// One of the presets the menu is showing, by chance.
    Random,
    Previous,
    Next,
    /// The Presets tab, on this device — what the name used to open.
    ShowInBrowser,
}

pub const PREVIOUS_PRESET: &str = "\u{2039} Previous preset";
pub const NEXT_PRESET: &str = "Next preset \u{203a}";
pub const SHOW_IN_BROWSER: &str = "Show in browser";

/// The drop-down's rows: the studio's preset menu, with the walk and the way
/// to the browser added.
pub fn plugin_preset_menu(
    choices: &[PresetChoice],
    query: &str,
    current: Option<usize>,
) -> (Vec<MenuEntry>, Vec<PluginMenuRow>) {
    let (mut entries, marked) = preset_menu_marking(choices, query, current);
    let mut rows: Vec<PluginMenuRow> = marked
        .into_iter()
        .map(|row| match row {
            PresetMenuRow::Heading => PluginMenuRow::Heading,
            PresetMenuRow::Preset(which) => PluginMenuRow::Preset(which),
            PresetMenuRow::Random => PluginMenuRow::Random,
        })
        .collect();
    // Under Random, with it: the three that leave the menu up.
    if let Some(random) = rows.iter().position(|row| *row == PluginMenuRow::Random) {
        entries.insert(random + 1, MenuEntry::new(PREVIOUS_PRESET));
        rows.insert(random + 1, PluginMenuRow::Previous);
        entries.insert(random + 2, MenuEntry::new(NEXT_PRESET));
        rows.insert(random + 2, PluginMenuRow::Next);
    }
    entries.push(MenuEntry::new(SHOW_IN_BROWSER).after_rule());
    rows.push(PluginMenuRow::ShowInBrowser);
    (entries, rows)
}

/// Something done to the drop-down, in the plugin window's own pixels.
#[derive(Debug, Clone, PartialEq)]
pub enum PluginMenuInput {
    /// A press anywhere in the window — on the menu, the strip or the
    /// plugin.
    Press(f32, f32),
    Pointer(f32, f32),
    /// The wheel, in notches; positive is down.
    Scroll(i32),
    Key(PluginMenuKey),
    /// The window lost the pointer to something else.
    Lost,
}

/// A key typed while the drop-down is up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginMenuKey {
    Text(String),
    Backspace,
    Enter,
    Escape,
    Up,
    Down,
    PageUp,
    PageDown,
    Home,
    End,
}

/// What the studio is to do about an input.
#[derive(Debug, Clone, PartialEq)]
pub enum PluginMenuOutcome {
    /// Nothing changed.
    Nothing,
    /// The menu looks different; draw it again.
    Redraw,
    /// Take it down.
    Close,
    /// Load the preset at this place in the choices, and take it down.
    Choose(usize),
    /// Load this one, chosen by chance — and leave the menu up.
    Random(usize),
    /// Step through the bank, and leave the menu up.
    Step(i32),
    /// Star or unstar the preset at this place in the choices.
    Star(usize),
    /// Take it down and open the Presets tab on the device.
    ShowInBrowser,
    /// Take it down; the press was on the strip at `(x, y)`, and is the
    /// strip's.
    PassToStrip(f32, f32),
}

/// The drop-down while it is up: its rows laid out in the plugin's window,
/// what has been typed, and which row is lit.
#[derive(Debug, Clone, PartialEq)]
pub struct PluginPresetMenu {
    pub device: PresetDevice,
    /// Laid out in the window's **logical** units — its pixels over
    /// [`scale`](Self::scale).
    pub menu: ContextMenu,
    pub rows: Vec<PluginMenuRow>,
    query: String,
    hover: Option<usize>,
    scale: f32,
    /// The window it was laid out in: `(width, strip, area)`, its pixels.
    window: (u32, u32, u32),
    /// The search line, pinned above the list, and what it says — the
    /// list's own first row once, which scrolled away with the rest.
    search: Rect,
    search_label: String,
}

impl PluginPresetMenu {
    /// Drops the menu from the name on `header`'s strip. `None` when the
    /// window has no room for a row.
    pub fn open(
        header: &PluginHeaderView,
        choices: &[PresetChoice],
        current: Option<usize>,
        metrics: &Metrics,
        font_size: f32,
    ) -> Option<Self> {
        let mut this = Self {
            device: header.device,
            menu: ContextMenu::default(),
            rows: Vec::new(),
            query: String::new(),
            hover: None,
            scale: header.scale.max(0.25),
            window: (0, 0, 0),
            search: Rect::ZERO,
            search_label: String::new(),
        };
        this.lay_out(header, choices, current, metrics, font_size, None);
        if this.menu.is_empty() {
            return None;
        }
        // Opened on what is playing: a bank of hundreds may have it far down.
        if let Some(index) = current.and_then(|which| {
            this.rows
                .iter()
                .rposition(|row| *row == PluginMenuRow::Preset(which))
        }) {
            this.menu.scroll_to(index);
        }
        Some(this)
    }

    /// What has been typed into its search.
    pub fn query(&self) -> &str {
        &self.query
    }

    /// The row lit by the pointer or the arrow keys.
    pub fn hover(&self) -> Option<usize> {
        self.hover
    }

    /// The window's pixels per logical one.
    pub fn scale(&self) -> f32 {
        self.scale
    }

    /// Whether `header` is a window of another size than the one it was laid
    /// out in — resized while it was up.
    pub fn is_stale(&self, header: &PluginHeaderView) -> bool {
        self.window != (header.width, header.height, header.area)
            || (self.scale - header.scale.max(0.25)).abs() > f32::EPSILON
    }

    /// The search line over the list, in logical units: empty when the
    /// device has no presets to search.
    pub fn search(&self) -> Rect {
        self.search
    }

    /// What the search line says: what has been typed, or how to.
    pub fn search_label(&self) -> &str {
        &self.search_label
    }

    /// The whole of it — the search line and the list under it — in
    /// logical units.
    pub fn frame(&self) -> Rect {
        if self.search.is_empty() {
            return self.menu.frame;
        }
        let top = self.search.y;
        Rect::new(
            self.menu.frame.x,
            top,
            self.menu.frame.width,
            self.menu.frame.bottom() - top,
        )
    }

    /// Where it goes in the window: `(x, y, width, height)` in its pixels.
    pub fn pixel_rect(&self) -> (i32, i32, u32, u32) {
        let frame = self.frame();
        let s = self.scale;
        (
            (frame.x * s).round() as i32,
            (frame.y * s).round() as i32,
            (frame.width * s).ceil().max(1.0) as u32,
            (frame.height * s).ceil().max(1.0) as u32,
        )
    }

    /// Lays it out again after something was loaded with it up — the mark
    /// moves — keeping where the list was scrolled.
    pub fn refresh(
        &mut self,
        header: &PluginHeaderView,
        choices: &[PresetChoice],
        current: Option<usize>,
        metrics: &Metrics,
        font_size: f32,
    ) {
        let scroll = self.menu.scroll();
        self.lay_out(header, choices, current, metrics, font_size, Some(scroll));
    }

    fn lay_out(
        &mut self,
        header: &PluginHeaderView,
        choices: &[PresetChoice],
        current: Option<usize>,
        metrics: &Metrics,
        font_size: f32,
        scroll: Option<f32>,
    ) {
        self.scale = header.scale.max(0.25);
        self.window = (header.width, header.height, header.area);
        let s = self.scale;
        let (width, strip, area) = (
            header.width as f32 / s,
            header.height as f32 / s,
            header.area as f32 / s,
        );
        let name = plugin_header_layout(width, &header.bar, metrics).name;
        let (mut entries, mut rows) = plugin_preset_menu(choices, &self.query, current);
        // The search line comes out of the list and is pinned over it, so
        // it is there however far down a bank of hundreds is scrolled.
        let row = metrics.row_height.max(1.0);
        let searchable = rows.first() == Some(&PluginMenuRow::Heading)
            && entries
                .first()
                .is_some_and(|entry| entry.label.starts_with(PRESET_MENU_HEADING));
        let top = if searchable {
            let heading = entries.remove(0);
            rows.remove(0);
            self.search_label = heading.label;
            strip + row
        } else {
            self.search_label.clear();
            strip
        };
        let bounds = Rect::new(0.0, top, width, (area - (top - strip)).max(0.0));
        self.menu = context_menu_layout((name.x, top), bounds, metrics, font_size, entries);
        self.search = if searchable && !self.menu.is_empty() {
            Rect::new(self.menu.frame.x, strip, self.menu.frame.width, row)
        } else {
            Rect::ZERO
        };
        self.rows = rows;
        if let Some(scroll) = scroll {
            self.menu.scroll_by(scroll);
        }
        self.hover = self
            .hover
            .filter(|&at| at < self.rows.len() && self.selectable(at));
    }

    fn selectable(&self, index: usize) -> bool {
        !matches!(self.rows.get(index), None | Some(PluginMenuRow::Heading))
            && self.menu.entries.get(index).is_some_and(|e| e.enabled)
    }

    /// What row `index` does when it is pressed.
    fn act(&self, index: usize, seed: u64) -> PluginMenuOutcome {
        match self.rows.get(index) {
            Some(PluginMenuRow::Preset(which)) => PluginMenuOutcome::Choose(*which),
            Some(PluginMenuRow::Random) => {
                let shown: Vec<PresetMenuRow> = self
                    .rows
                    .iter()
                    .filter_map(|row| match row {
                        PluginMenuRow::Preset(which) => Some(PresetMenuRow::Preset(*which)),
                        _ => None,
                    })
                    .collect();
                random_preset_row(&shown, seed)
                    .map_or(PluginMenuOutcome::Nothing, PluginMenuOutcome::Random)
            }
            Some(PluginMenuRow::Previous) => PluginMenuOutcome::Step(-1),
            Some(PluginMenuRow::Next) => PluginMenuOutcome::Step(1),
            Some(PluginMenuRow::ShowInBrowser) => PluginMenuOutcome::ShowInBrowser,
            Some(PluginMenuRow::Heading) | None => PluginMenuOutcome::Nothing,
        }
    }

    /// Lights row `index` and brings it into sight.
    fn light(&mut self, index: usize) -> PluginMenuOutcome {
        if self.hover == Some(index) {
            return PluginMenuOutcome::Nothing;
        }
        self.hover = Some(index);
        self.menu.scroll_to(index);
        PluginMenuOutcome::Redraw
    }

    /// What `input` does. `header`, `choices` and `current` are the device's
    /// as they are now — typing lays the list out again — and `seed` is what
    /// Random deals from.
    #[allow(clippy::too_many_arguments)]
    pub fn input(
        &mut self,
        input: PluginMenuInput,
        header: &PluginHeaderView,
        choices: &[PresetChoice],
        current: Option<usize>,
        metrics: &Metrics,
        font_size: f32,
        seed: u64,
    ) -> PluginMenuOutcome {
        let s = self.scale;
        match input {
            PluginMenuInput::Lost => PluginMenuOutcome::Close,
            PluginMenuInput::Press(px, py) => {
                let (x, y) = (px / s, py / s);
                if self.search.contains(x, y) {
                    return PluginMenuOutcome::Nothing;
                }
                if self.menu.frame.contains(x, y) {
                    if let Some(star) = context_menu_star_hit(&self.menu, x, y)
                        && let Some(PluginMenuRow::Preset(which)) = self.rows.get(star)
                    {
                        return PluginMenuOutcome::Star(*which);
                    }
                    return context_menu_hit(&self.menu, x, y)
                        .map_or(PluginMenuOutcome::Nothing, |index| self.act(index, seed));
                }
                let strip = header.height as f32 / s;
                if y < strip {
                    let width = header.width as f32 / s;
                    return match plugin_header_hit(width, &header.bar, metrics, x, y) {
                        // The name again: shut — a drop-down is a toggle.
                        Some(PresetBarHit::Name | PresetBarHit::Category) | None => {
                            PluginMenuOutcome::Close
                        }
                        Some(_) => PluginMenuOutcome::PassToStrip(px, py),
                    };
                }
                PluginMenuOutcome::Close
            }
            PluginMenuInput::Pointer(px, py) => {
                let hit = context_menu_hit(&self.menu, px / s, py / s)
                    .filter(|&index| self.selectable(index));
                if hit == self.hover || hit.is_none() {
                    return PluginMenuOutcome::Nothing;
                }
                self.hover = hit;
                PluginMenuOutcome::Redraw
            }
            PluginMenuInput::Scroll(notches) => {
                let before = self.menu.scroll();
                self.menu
                    .scroll_by(notches as f32 * metrics.row_height.max(1.0) * 3.0);
                if self.menu.scroll() == before {
                    PluginMenuOutcome::Nothing
                } else {
                    PluginMenuOutcome::Redraw
                }
            }
            PluginMenuInput::Key(key) => {
                self.key(key, header, choices, current, metrics, font_size, seed)
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn key(
        &mut self,
        key: PluginMenuKey,
        header: &PluginHeaderView,
        choices: &[PresetChoice],
        current: Option<usize>,
        metrics: &Metrics,
        font_size: f32,
        seed: u64,
    ) -> PluginMenuOutcome {
        let selectable: Vec<usize> = (0..self.rows.len())
            .filter(|&index| self.selectable(index))
            .collect();
        match key {
            PluginMenuKey::Escape => PluginMenuOutcome::Close,
            PluginMenuKey::Text(text) => {
                self.query.push_str(&text);
                self.hover = None;
                self.lay_out(header, choices, current, metrics, font_size, None);
                PluginMenuOutcome::Redraw
            }
            PluginMenuKey::Backspace => {
                if self.query.pop().is_none() {
                    return PluginMenuOutcome::Nothing;
                }
                self.hover = None;
                self.lay_out(header, choices, current, metrics, font_size, None);
                PluginMenuOutcome::Redraw
            }
            PluginMenuKey::Enter => {
                // The row lit, or else the first preset the search left.
                let index = self.hover.or_else(|| {
                    self.rows
                        .iter()
                        .position(|row| matches!(row, PluginMenuRow::Preset(_)))
                });
                index.map_or(PluginMenuOutcome::Nothing, |index| self.act(index, seed))
            }
            PluginMenuKey::Down | PluginMenuKey::Up => {
                let at = self
                    .hover
                    .and_then(|lit| selectable.iter().position(|&index| index == lit));
                let down = key == PluginMenuKey::Down;
                let next = match at {
                    None if down => selectable.first(),
                    None => selectable.last(),
                    Some(at) if down => selectable.get(at + 1).or(selectable.last()),
                    Some(at) => selectable.get(at.saturating_sub(1)),
                };
                next.copied()
                    .map_or(PluginMenuOutcome::Nothing, |index| self.light(index))
            }
            PluginMenuKey::Home => selectable
                .first()
                .copied()
                .map_or(PluginMenuOutcome::Nothing, |index| self.light(index)),
            PluginMenuKey::End => selectable
                .last()
                .copied()
                .map_or(PluginMenuOutcome::Nothing, |index| self.light(index)),
            PluginMenuKey::PageDown | PluginMenuKey::PageUp => {
                let page = (self.menu.frame.height - metrics.row_height).max(metrics.row_height);
                let before = self.menu.scroll();
                self.menu.scroll_by(match key {
                    PluginMenuKey::PageDown => page,
                    _ => -page,
                });
                if self.menu.scroll() == before {
                    PluginMenuOutcome::Nothing
                } else {
                    PluginMenuOutcome::Redraw
                }
            }
        }
    }
}
