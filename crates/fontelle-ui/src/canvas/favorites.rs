//! The menus that list things you can star, and where the starred ones go.
//!
//! > *"favorite (star) plugins, instruments, effects, etc. so that the
//! > favorites are always the most visible (highlighted appearance) and there
//! > should be a favorites section at the top of every dropdown basically
//! > that includes all of your applicable favorites if there are any."*
//!
//! Three menus offer those things: the mixer's *+ fx*, the rack's *New
//! instrument* / *Change instrument*, and the plugin picker both of them open.
//! Each is one function here that returns the rows **and what each row
//! means**, as a pair — so the window builds the menu from one half and
//! answers a press from the other, and the two cannot come to disagree about
//! which row was the reverb. That is the same shape `lane_menu` and the
//! transport's record menu already have, and the reason: a menu of strings
//! whose meaning is an index into a second list is a menu that breaks the day
//! a row is added above it.
//!
//! The rule all three follow: **if anything in the list is starred, a
//! *Favorites* section goes first**, under the heading and above a rule, and
//! holds every applicable favourite — an effect, a kind, or a plugin that is
//! installed here. The full list follows unchanged, and a favourite is lit in
//! it too, so a thing is highlighted wherever it appears. A starred plugin is
//! *applicable* to the *+ fx* and *New instrument* menus as well as to the
//! picker: a favourite is the promise that it is one press away, not two.

use fontelle_types::{EffectKind, Favorite, InstrumentKind, PluginKey};

use super::menu::{ASKS_MORE, CHOSEN_MARK, MenuEntry, menu_matches};
use crate::document::PluginListing;

/// What the favourites section is called.
pub const FAVORITES_HEADING: &str = "Favorites";

/// The plugin picker's last row.
pub const RESCAN_PLUGINS: &str = "Rescan plugin folders";

/// What a row of the *+ fx* menu means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectRow {
    /// A caption: the menu's own, or the favourites'. Does nothing.
    Heading,
    /// One of the effects that ship in this binary.
    Builtin(EffectKind),
    /// A starred plugin effect, by its position in the listing the menu was
    /// built from — see `StudioHost::add_plugin_insert`.
    Plugin(usize),
    /// *"Plugin…"* — the last row, which opens the list of what is installed
    /// on the machine (TDD §8.4).
    ///
    /// A second menu rather than the plugins listed here, because they are
    /// different lists: the built-ins are known at compile time and the same
    /// on every machine; the plugins are however many somebody has installed.
    /// One menu of both would be a menu whose length depends on the computer.
    /// The favourites are the exception, and they are the person's own list.
    PluginPicker,
}

/// What a row of the *New instrument* / *Change instrument* menu means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstrumentRow {
    Heading,
    /// One of the kinds — `Plugin` included, which opens the picker.
    Kind(InstrumentKind),
    /// A starred plugin instrument, by position in the listing.
    Plugin(usize),
}

/// What a row of the plugin picker means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerRow {
    Heading,
    /// The greyed "none found" / "nothing matches" row.
    Nothing,
    /// A plugin, by position in the listing the picker was built from.
    Plugin(usize),
    Rescan,
}

fn is_starred(favorites: &[Favorite], favorite: &Favorite) -> bool {
    favorites.contains(favorite)
}

fn plugin_starred(favorites: &[Favorite], key: &PluginKey) -> bool {
    favorites
        .iter()
        .any(|favorite| matches!(favorite, Favorite::Plugin(starred) if starred == key))
}

/// What a plugin's row says: its name, and who wrote it when it said.
fn plugin_label(listing: &PluginListing) -> String {
    if listing.vendor.is_empty() {
        listing.name.clone()
    } else {
        format!("{} — {}", listing.name, listing.vendor)
    }
}

/// Puts a rule above the first row of `rows`, if there is one.
fn rule_above_first<R>(rows: &mut [(MenuEntry, R)]) {
    if let Some((entry, _)) = rows.first_mut() {
        entry.separator = true;
    }
}

/// The *+ fx* menu: a heading, the favourites if there are any, every
/// built-in effect from `EffectKind::ALL`, and the row that opens the picker.
///
/// One list rather than two: a new effect appears in this menu by *existing*,
/// not by somebody remembering to add it in a second place. That is the same
/// rule the roll's lane properties follow and the reason they have a `const`
/// list too.
pub fn effect_menu_rows(
    favorites: &[Favorite],
    plugins: &[PluginListing],
) -> Vec<(MenuEntry, EffectRow)> {
    let mut rows = vec![(MenuEntry::disabled("Add effect"), EffectRow::Heading)];

    let mut starred: Vec<(MenuEntry, EffectRow)> = EffectKind::ALL
        .into_iter()
        .filter(|kind| is_starred(favorites, &Favorite::Effect(*kind)))
        .map(|kind| {
            (
                MenuEntry::new(kind.full_label()).starred(true),
                EffectRow::Builtin(kind),
            )
        })
        .collect();
    starred.extend(
        plugins
            .iter()
            .enumerate()
            .filter(|(_, listing)| plugin_starred(favorites, &listing.key))
            .map(|(which, listing)| {
                (
                    MenuEntry::new(plugin_label(listing)).starred(true),
                    EffectRow::Plugin(which),
                )
            }),
    );

    let mut rest: Vec<(MenuEntry, EffectRow)> = EffectKind::ALL
        .into_iter()
        .map(|kind| {
            let lit = is_starred(favorites, &Favorite::Effect(kind));
            (
                MenuEntry::new(kind.full_label()).starred(lit),
                EffectRow::Builtin(kind),
            )
        })
        .collect();

    if !starred.is_empty() {
        rows.push((MenuEntry::disabled(FAVORITES_HEADING), EffectRow::Heading));
        rows.extend(starred);
        rule_above_first(&mut rest);
    }
    rows.extend(rest);
    rows.push((
        MenuEntry::new(format!("Plugin{ASKS_MORE}")),
        EffectRow::PluginPicker,
    ));
    rows
}

/// One kind's row.
///
/// `current` is the kind the channel already is, or `None` for the *New
/// instrument* menu, which is not changing anything.
///
/// **The kind you already have is marked and greyed**, because choosing it
/// would hand you a fresh instrument and throw away whatever you had edited —
/// see `StudioHost::set_channel_kind`, which refuses that edit anyway.
///
/// **Except `Plugin`, which is always live.** It is not an instrument the way
/// the other four are: it is a promise to name one, and the row opens a second
/// menu to ask which. Greying it on a channel that already plays a plugin was
/// the same mistake the record button's menu made — *"i pressed the record
/// button again but i was locked out of the audio option"*, and here
/// *"currently cant replace a plugin instrument with another plugin
/// instrument"*. A row that means *choose* must not be disabled by what was
/// chosen last time. And for the same reason it has no star: it is a door,
/// not a thing.
fn kind_entry(kind: InstrumentKind, current: Option<InstrumentKind>, lit: bool) -> MenuEntry {
    let asks_more = kind == InstrumentKind::Plugin;
    let label = format!(
        "{}{}{}",
        if current == Some(kind) {
            CHOSEN_MARK
        } else {
            ""
        },
        kind.label(),
        if asks_more { ASKS_MORE } else { "" },
    );
    let entry = if current == Some(kind) && !asks_more {
        MenuEntry::disabled(label)
    } else {
        MenuEntry::new(label)
    };
    if asks_more { entry } else { entry.starred(lit) }
}

/// The *New instrument* / *Change instrument* menu: a heading, the favourites
/// if there are any — starred kinds, then starred plugin instruments that are
/// installed here — and every kind in `InstrumentKind::ALL`'s order.
///
/// One list, so the two places that offer the kinds cannot come to disagree
/// about what there is.
pub fn instrument_menu_rows(
    current: Option<InstrumentKind>,
    favorites: &[Favorite],
    plugins: &[PluginListing],
) -> Vec<(MenuEntry, InstrumentRow)> {
    let mut rows = vec![(
        MenuEntry::disabled("Change instrument"),
        InstrumentRow::Heading,
    )];

    let mut starred: Vec<(MenuEntry, InstrumentRow)> = InstrumentKind::ALL
        .into_iter()
        .filter(|kind| {
            *kind != InstrumentKind::Plugin && is_starred(favorites, &Favorite::Instrument(*kind))
        })
        .map(|kind| (kind_entry(kind, current, true), InstrumentRow::Kind(kind)))
        .collect();
    starred.extend(
        plugins
            .iter()
            .enumerate()
            .filter(|(_, listing)| plugin_starred(favorites, &listing.key))
            .map(|(which, listing)| {
                (
                    MenuEntry::new(plugin_label(listing)).starred(true),
                    InstrumentRow::Plugin(which),
                )
            }),
    );

    let mut rest: Vec<(MenuEntry, InstrumentRow)> = InstrumentKind::ALL
        .into_iter()
        .map(|kind| {
            let lit = is_starred(favorites, &Favorite::Instrument(kind));
            (kind_entry(kind, current, lit), InstrumentRow::Kind(kind))
        })
        .collect();

    if !starred.is_empty() {
        rows.push((
            MenuEntry::disabled(FAVORITES_HEADING),
            InstrumentRow::Heading,
        ));
        rows.extend(starred);
        rule_above_first(&mut rest);
    }
    rows.extend(rest);
    rows
}

/// The plugin picker: what has been typed narrows the list, the favourites
/// that survive it go first, and the rescan row is last.
///
/// The heading carries the filter, because a filtered menu that does not show
/// its filter is a menu that has lost your plugins. With nothing left, one
/// greyed row says which of the two things happened — nothing installed (or
/// nothing looked for yet), or nothing matching what was typed — because a
/// menu with nothing in it but a heading looks broken.
pub fn plugin_picker_rows(
    heading: &str,
    filter: &str,
    favorites: &[Favorite],
    listings: &[PluginListing],
) -> Vec<(MenuEntry, PickerRow)> {
    let filter = filter.trim();
    let mut rows = vec![(
        MenuEntry::disabled(if filter.is_empty() {
            format!("{heading} — type to filter")
        } else {
            format!("{heading} — {filter}")
        }),
        PickerRow::Heading,
    )];

    let matching: Vec<(usize, &PluginListing)> = listings
        .iter()
        .enumerate()
        .filter(|(_, listing)| {
            menu_matches(&listing.name, filter) || menu_matches(&listing.vendor, filter)
        })
        .collect();
    if matching.is_empty() {
        rows.push((
            MenuEntry::disabled(if filter.is_empty() {
                "none found"
            } else {
                "nothing matches"
            }),
            PickerRow::Nothing,
        ));
    }

    let starred: Vec<(MenuEntry, PickerRow)> = matching
        .iter()
        .filter(|(_, listing)| plugin_starred(favorites, &listing.key))
        .map(|(which, listing)| {
            (
                MenuEntry::new(plugin_label(listing)).starred(true),
                PickerRow::Plugin(*which),
            )
        })
        .collect();
    let mut rest: Vec<(MenuEntry, PickerRow)> = matching
        .iter()
        .map(|(which, listing)| {
            let lit = plugin_starred(favorites, &listing.key);
            (
                MenuEntry::new(plugin_label(listing)).starred(lit),
                PickerRow::Plugin(*which),
            )
        })
        .collect();

    if !starred.is_empty() {
        rows.push((MenuEntry::disabled(FAVORITES_HEADING), PickerRow::Heading));
        rows.extend(starred);
        rule_above_first(&mut rest);
    }
    rows.extend(rest);
    rows.push((
        MenuEntry::new(RESCAN_PLUGINS).after_rule(),
        PickerRow::Rescan,
    ));
    rows
}
