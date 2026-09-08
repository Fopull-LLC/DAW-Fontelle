//! Starred things, and where a menu puts them.
//!
//! > *"make it so that i can favorite (star) plugins, instruments, effects,
//! > etc. so that the favorites are always the most visible (highlighted
//! > appearance) and there should be a favorites section at the top of every
//! > dropdown basically that includes all of your applicable favorites if
//! > there are any."*
//!
//! Three menus offer the things that can be starred — the mixer's *+ fx*, the
//! rack's *New instrument* / *Change instrument*, and the plugin picker both
//! of those open — and each is built by one pure function here, so what goes
//! at the top, what is starred and what a press on a star means are all
//! checkable without a window.

use fontelle_types::{EffectKind, Favorite, InstrumentKind, PluginKey};
use fontelle_ui::PluginListing;
use fontelle_ui::canvas::{
    CHOSEN_MARK, EffectRow, FAVORITES_HEADING, InstrumentRow, MenuEntry, PickerRow,
    context_menu_hit, context_menu_layout, context_menu_star_hit, effect_menu_rows,
    instrument_menu_rows, plugin_picker_rows,
};
use fontelle_ui::icon::{EVERY_ICON, Icon};
use fontelle_ui::layout::Rect;
use fontelle_ui::theme::{Metrics, Theme};

fn metrics() -> Metrics {
    Theme::dark_default().metrics
}

fn font_size() -> f32 {
    Theme::dark_default().font.size
}

fn bounds() -> Rect {
    Rect::new(0.0, 0.0, 800.0, 600.0)
}

fn listing(name: &str, id: &str) -> PluginListing {
    PluginListing {
        name: name.to_string(),
        vendor: "Vendor".to_string(),
        key: PluginKey::clap(id),
    }
}

fn plugins() -> Vec<PluginListing> {
    vec![
        listing("Alpha", "com.example.alpha"),
        listing("Beta", "com.example.beta"),
        listing("Gamma", "com.example.gamma"),
    ]
}

fn favourite_plugin(id: &str) -> Favorite {
    Favorite::Plugin(PluginKey::clap(id))
}

/// The labels of the rows between the first heading and the first rule —
/// which is where a favourites section lives, if there is one.
fn top_section<R>(rows: &[(MenuEntry, R)]) -> Vec<String> {
    rows.iter()
        .skip(1)
        .take_while(|(entry, _)| !entry.separator)
        .map(|(entry, _)| entry.label.clone())
        .collect()
}

// --------------------------------------------------------- the entries ---

#[test]
fn an_entry_can_carry_a_star_and_says_whether_it_is_lit() {
    let plain = MenuEntry::new("Delete");
    assert!(plain.star.is_none(), "an action row has no star");
    assert!(!plain.is_favorite());

    let unlit = MenuEntry::new("Reverb").starred(false);
    assert_eq!(unlit.star, Some(false));
    assert!(!unlit.is_favorite());

    let lit = MenuEntry::new("Reverb").starred(true);
    assert_eq!(lit.star, Some(true));
    assert!(
        lit.is_favorite(),
        "a lit star is what a highlighted row means"
    );
}

// ---------------------------------------------------------- the fx menu ---

#[test]
fn with_nothing_starred_the_fx_menu_is_a_heading_every_effect_and_the_plugin_row() {
    let rows = effect_menu_rows(&[], &[]);
    assert!(!rows[0].0.enabled, "the heading");
    assert_eq!(rows[0].1, EffectRow::Heading);
    // No favourites section: nothing between the heading and the first
    // built-in, and no "Favorites" caption anywhere.
    assert!(
        rows.iter()
            .all(|(entry, _)| entry.label != FAVORITES_HEADING),
        "{rows:?}"
    );
    let builtins: Vec<EffectKind> = rows
        .iter()
        .filter_map(|(_, row)| match row {
            EffectRow::Builtin(kind) => Some(*kind),
            _ => None,
        })
        .collect();
    assert_eq!(
        builtins,
        EffectKind::ALL.to_vec(),
        "every effect, once, in order"
    );
    assert_eq!(
        rows.last().map(|(_, row)| *row),
        Some(EffectRow::PluginPicker)
    );
    assert!(
        rows.last().unwrap().0.label.ends_with('\u{2026}'),
        "the plugin row says it asks which"
    );
}

#[test]
fn every_built_in_effect_can_be_starred_and_the_plugin_row_cannot() {
    let rows = effect_menu_rows(&[], &[]);
    for (entry, row) in &rows {
        match row {
            EffectRow::Builtin(_) => assert_eq!(entry.star, Some(false), "{:?}", entry.label),
            EffectRow::PluginPicker | EffectRow::Heading => {
                assert!(entry.star.is_none(), "{:?}", entry.label)
            }
            EffectRow::Plugin(_) => unreachable!("no plugins were given"),
        }
    }
}

#[test]
fn starred_effects_come_first_under_a_favorites_heading_and_stay_lit_in_the_list() {
    let favourites = [
        Favorite::Effect(EffectKind::Reverb),
        Favorite::Effect(EffectKind::Eq),
    ];
    let rows = effect_menu_rows(&favourites, &[]);
    assert_eq!(rows[1].0.label, FAVORITES_HEADING);
    assert!(!rows[1].0.enabled, "a heading, not a row to press");
    let section = top_section(&rows);
    assert_eq!(
        section,
        vec![
            FAVORITES_HEADING.to_string(),
            EffectKind::Eq.label().to_string(),
            EffectKind::Reverb.label().to_string(),
        ],
        "the favourites, in the list's own order"
    );
    for (entry, row) in &rows[2..4] {
        assert!(entry.is_favorite(), "{:?}", entry.label);
        assert!(matches!(row, EffectRow::Builtin(_)));
    }
    // Then the rule, and then the whole list — in which the same two are
    // starred, so a favourite is lit wherever it appears.
    let rest = &rows[4..];
    assert!(
        rest[0].0.separator,
        "a rule between the favourites and the rest"
    );
    let lit: Vec<&str> = rest
        .iter()
        .filter(|(entry, _)| entry.is_favorite())
        .map(|(entry, _)| entry.label.as_str())
        .collect();
    assert_eq!(
        lit,
        vec![EffectKind::Eq.label(), EffectKind::Reverb.label()]
    );
    let builtins = rest
        .iter()
        .filter(|(_, row)| matches!(row, EffectRow::Builtin(_)))
        .count();
    assert_eq!(
        builtins,
        EffectKind::ALL.len(),
        "the full list is still the full list"
    );
}

#[test]
fn a_starred_plugin_effect_is_offered_straight_from_the_fx_menu() {
    // Without a favourite, a plugin effect is two menus away: *Plugin…* and
    // then the picker. A favourite is the promise that it is one press away.
    let favourites = [favourite_plugin("com.example.beta")];
    let rows = effect_menu_rows(&favourites, &plugins());
    let section = top_section(&rows);
    assert_eq!(section.len(), 2, "{section:?}");
    assert!(section[1].starts_with("Beta"), "{section:?}");
    assert_eq!(
        rows[2].1,
        EffectRow::Plugin(1),
        "by its position in the listing"
    );
    assert!(rows[2].0.is_favorite());
    // And the plugins that are not favourites are not in this menu at all:
    // they still live behind *Plugin…*.
    assert!(
        rows.iter()
            .all(|(_, row)| !matches!(row, EffectRow::Plugin(0) | EffectRow::Plugin(2))),
        "{rows:?}"
    );
}

#[test]
fn a_starred_plugin_that_is_not_installed_here_is_left_out_rather_than_offered() {
    // A favourite names the plugin permanently; the list is what this machine
    // has. One that is not here is not applicable, and a row that would do
    // nothing is worse than no row.
    let favourites = [favourite_plugin("com.example.missing")];
    let rows = effect_menu_rows(&favourites, &plugins());
    assert!(
        rows.iter()
            .all(|(entry, _)| entry.label != FAVORITES_HEADING),
        "{rows:?}"
    );
}

// -------------------------------------------------- the instrument menu ---

#[test]
fn with_nothing_starred_the_instrument_menu_is_what_it_always_was() {
    let rows = instrument_menu_rows(None, &[], &[]);
    assert_eq!(rows.len(), InstrumentKind::ALL.len() + 1);
    assert_eq!(rows[0].1, InstrumentRow::Heading);
    for ((entry, row), kind) in rows[1..].iter().zip(InstrumentKind::ALL) {
        assert_eq!(*row, InstrumentRow::Kind(kind));
        assert!(entry.label.contains(kind.label()));
        // The plugin row opens a second menu; it is a door, not a thing to
        // star. The other four can be.
        if kind == InstrumentKind::Plugin {
            assert!(entry.star.is_none(), "{:?}", entry.label);
        } else {
            assert_eq!(entry.star, Some(false), "{:?}", entry.label);
        }
    }
}

#[test]
fn starred_kinds_and_starred_plugin_instruments_share_the_favorites_section() {
    let favourites = [
        favourite_plugin("com.example.gamma"),
        Favorite::Instrument(InstrumentKind::DrumMachine),
    ];
    let rows = instrument_menu_rows(None, &favourites, &plugins());
    let section = top_section(&rows);
    assert_eq!(section[0], FAVORITES_HEADING);
    assert_eq!(section.len(), 3, "{section:?}");
    // The kinds first (they are the built-ins), then the plugins.
    assert_eq!(rows[2].1, InstrumentRow::Kind(InstrumentKind::DrumMachine));
    assert_eq!(rows[3].1, InstrumentRow::Plugin(2));
    assert!(rows[3].0.label.starts_with("Gamma"));
    assert!(rows[2].0.is_favorite() && rows[3].0.is_favorite());
    // The full list follows, with the drum machine lit there too.
    let drum_again = rows[4..]
        .iter()
        .find(|(_, row)| *row == InstrumentRow::Kind(InstrumentKind::DrumMachine))
        .expect("still in the full list");
    assert!(drum_again.0.is_favorite());
    assert!(rows[4].0.separator);
}

#[test]
fn the_kind_a_channel_already_is_is_still_said_rather_than_offered() {
    // The rule `instrument_menu_entries` had, kept: a favourite you already
    // have is marked and greyed in both places it appears.
    let favourites = [Favorite::Instrument(InstrumentKind::Osc3)];
    let rows = instrument_menu_rows(Some(InstrumentKind::Osc3), &favourites, &[]);
    let said: Vec<&(MenuEntry, InstrumentRow)> = rows
        .iter()
        .filter(|(_, row)| *row == InstrumentRow::Kind(InstrumentKind::Osc3))
        .collect();
    assert_eq!(said.len(), 2, "once in the favourites, once in the list");
    for (entry, _) in said {
        assert!(!entry.enabled, "{:?}", entry.label);
        assert!(entry.label.starts_with(CHOSEN_MARK), "{:?}", entry.label);
    }
}

// ------------------------------------------------------ the plugin picker ---

#[test]
fn the_picker_puts_starred_plugins_first_and_still_lists_them_below() {
    let favourites = [favourite_plugin("com.example.gamma")];
    let rows = plugin_picker_rows("Plugin effects", "", &favourites, &plugins());
    assert_eq!(rows[0].1, PickerRow::Heading);
    assert_eq!(rows[1].0.label, FAVORITES_HEADING);
    assert_eq!(rows[2].1, PickerRow::Plugin(2));
    assert!(rows[2].0.is_favorite());
    assert!(rows[3].0.separator, "a rule under the favourites");
    let all: Vec<PickerRow> = rows[3..].iter().map(|(_, row)| *row).collect();
    assert_eq!(
        all,
        vec![
            PickerRow::Plugin(0),
            PickerRow::Plugin(1),
            PickerRow::Plugin(2),
            PickerRow::Rescan
        ]
    );
    assert!(rows[5].0.is_favorite(), "lit in the full list too");
    assert!(!rows[3].0.is_favorite());
    assert!(rows[6].0.star.is_none(), "the rescan row is not a plugin");
}

#[test]
fn the_picker_filters_the_favorites_too() {
    let favourites = [
        favourite_plugin("com.example.gamma"),
        favourite_plugin("com.example.alpha"),
    ];
    let rows = plugin_picker_rows("Plugin effects", "alp", &favourites, &plugins());
    let section = top_section(&rows);
    assert_eq!(
        section,
        vec![FAVORITES_HEADING.to_string(), "Alpha — Vendor".to_string()]
    );
    let listed: Vec<PickerRow> = rows
        .iter()
        .filter_map(|(_, row)| match row {
            PickerRow::Plugin(which) => Some(PickerRow::Plugin(*which)),
            _ => None,
        })
        .collect();
    assert_eq!(listed, vec![PickerRow::Plugin(0), PickerRow::Plugin(0)]);
    // And a filter that matches no favourite shows no section at all.
    let rows = plugin_picker_rows("Plugin effects", "bet", &favourites, &plugins());
    assert!(
        rows.iter()
            .all(|(entry, _)| entry.label != FAVORITES_HEADING)
    );
}

#[test]
fn an_empty_picker_still_says_why() {
    let rows = plugin_picker_rows("Plugin instruments", "", &[], &[]);
    assert_eq!(rows[1].1, PickerRow::Nothing);
    assert!(!rows[1].0.enabled);
    assert!(
        rows[1].0.label.contains("none found"),
        "{:?}",
        rows[1].0.label
    );
    let rows = plugin_picker_rows("Plugin instruments", "zzz", &[], &plugins());
    assert_eq!(rows[1].1, PickerRow::Nothing);
    assert!(
        rows[1].0.label.contains("nothing matches"),
        "{:?}",
        rows[1].0.label
    );
}

// -------------------------------------------------------- the star itself ---

#[test]
fn the_right_end_of_a_starred_row_is_its_star_and_not_the_row() {
    let entries = vec![
        MenuEntry::disabled("Add effect"),
        MenuEntry::new("Reverb").starred(false),
        MenuEntry::new("Plugin…"),
    ];
    let menu = context_menu_layout((100.0, 100.0), bounds(), &metrics(), font_size(), entries);
    let row = menu.rows[1];
    let star = menu.star_rect(1);
    assert!(!star.is_empty(), "a starred row has a star to press");
    assert!(
        star.right() <= row.right() + 0.01 && star.x >= row.x,
        "inside the row"
    );
    assert!(
        star.width < row.width / 2.0,
        "a corner of the row, not half of it"
    );
    let on_star = (star.x + star.width / 2.0, star.y + star.height / 2.0);
    assert_eq!(context_menu_star_hit(&menu, on_star.0, on_star.1), Some(1));
    assert_eq!(
        context_menu_hit(&menu, on_star.0, on_star.1),
        None,
        "a press on the star is not a press on the row"
    );
    let on_caption = (row.x + 4.0, row.y + row.height / 2.0);
    assert_eq!(context_menu_hit(&menu, on_caption.0, on_caption.1), Some(1));
    assert_eq!(
        context_menu_star_hit(&menu, on_caption.0, on_caption.1),
        None
    );
}

#[test]
fn a_row_with_no_star_has_no_star_to_press() {
    let entries = vec![MenuEntry::new("Delete"), MenuEntry::new("Rename")];
    let menu = context_menu_layout((100.0, 100.0), bounds(), &metrics(), font_size(), entries);
    let row = menu.rows[0];
    assert!(menu.star_rect(0).is_empty());
    let right_end = (row.right() - 4.0, row.y + row.height / 2.0);
    assert_eq!(context_menu_star_hit(&menu, right_end.0, right_end.1), None);
    assert_eq!(context_menu_hit(&menu, right_end.0, right_end.1), Some(0));
}

#[test]
fn a_menu_with_stars_is_wider_than_the_same_captions_without_them() {
    // The star sits after the caption, so the room for it has to be added or
    // a long name runs under its own star.
    let plain = context_menu_layout(
        (0.0, 0.0),
        bounds(),
        &metrics(),
        font_size(),
        vec![MenuEntry::new("Create automation clip for this control")],
    );
    let starred = context_menu_layout(
        (0.0, 0.0),
        bounds(),
        &metrics(),
        font_size(),
        vec![MenuEntry::new("Create automation clip for this control").starred(false)],
    );
    assert!(starred.frame.width > plain.frame.width);
}

#[test]
fn a_star_scrolled_out_of_sight_cannot_be_pressed() {
    // Nine hundred, not eighty: a list of eighty is laid out in columns now
    // and every row of it shows. This one is too long even for that, so it
    // scrolls, and the last row's star is nowhere until it is scrolled to.
    let entries: Vec<MenuEntry> = (0..900)
        .map(|i| MenuEntry::new(format!("Plugin {i}")).starred(false))
        .collect();
    let menu = context_menu_layout((0.0, 0.0), bounds(), &metrics(), font_size(), entries);
    assert!(menu.scrolls());
    assert!(menu.star_rect(899).is_empty());
}

#[test]
fn the_star_is_an_icon_the_chrome_draws() {
    assert!(EVERY_ICON.contains(&Icon::Star));
    assert!(EVERY_ICON.contains(&Icon::StarFilled));
}
