//! What the *Change instrument* menu offers, and what it refuses.

use fontelle_types::InstrumentKind;
use fontelle_ui::canvas::{CHOSEN_MARK, instrument_menu_entries};

/// The kind a channel already is is marked and greyed: choosing it would hand
/// you a fresh instrument and throw away whatever you had edited.
#[test]
fn the_kind_a_channel_already_is_is_said_rather_than_offered() {
    let entries = instrument_menu_entries(Some(InstrumentKind::Osc3));
    let row = entries
        .iter()
        .find(|entry| entry.label.contains("3OSC"))
        .expect("3OSC is in the list");
    assert!(!row.enabled, "{:?}", row.label);
    assert!(row.label.starts_with(CHOSEN_MARK), "{:?}", row.label);
}

/// **Plugin is the exception**, and it has to be.
///
/// > *"currently cant replace a plugin instrument with another plugin
/// > instrument"*
///
/// "Plugin" is not an instrument the way the other four are — it is a promise
/// to name one, and the row opens a second menu to ask which. Greying it on a
/// channel that already plays a plugin is the same mistake the record button's
/// menu made (*"i was locked out of the audio option"*): it reads as "this is
/// the one you have" to whoever wrote it and as "you cannot have this" to
/// whoever is trying to swap a synth for another synth.
#[test]
fn a_plugin_channel_can_still_be_given_a_different_plugin() {
    let entries = instrument_menu_entries(Some(InstrumentKind::Plugin));
    let row = entries
        .iter()
        .find(|entry| entry.label.contains("Plugin"))
        .expect("Plugin is in the list");
    assert!(row.enabled, "{:?}", row.label);
}

/// And the row says that choosing it asks a second question.
#[test]
fn the_plugin_row_says_it_asks_which() {
    for current in [
        None,
        Some(InstrumentKind::Plugin),
        Some(InstrumentKind::Sampler),
    ] {
        let entries = instrument_menu_entries(current);
        assert!(
            entries
                .iter()
                .any(|entry| entry.label.ends_with('\u{2026}')),
            "{current:?}"
        );
    }
}

/// Every kind is offered, once, in `InstrumentKind::ALL`'s order, under a
/// heading — the same list the *New instrument* menu shows.
#[test]
fn every_kind_is_offered_once_in_one_order() {
    let entries = instrument_menu_entries(None);
    assert_eq!(entries.len(), InstrumentKind::ALL.len() + 1);
    assert!(!entries[0].enabled, "the heading");
    for (row, kind) in entries[1..].iter().zip(InstrumentKind::ALL) {
        assert!(row.label.contains(kind.label()), "{:?}", row.label);
        assert!(row.enabled, "nothing is greyed when the kind is unknown");
    }
}

/// What an instrument window says with nothing to show depends on what the
/// channel is: an empty sampler told to "pick a soundfont from the browser"
/// was the report — *"it will be saying stuff for soundfonts while u have a
/// sampler and its confusing."*
#[test]
fn an_empty_instrument_says_what_it_is_waiting_for() {
    use fontelle_types::InstrumentKind;
    use fontelle_ui::render::no_instrument_text;
    for kind in InstrumentKind::ALL {
        let said = no_instrument_text(Some(kind)).to_lowercase();
        assert!(!said.is_empty());
        let soundfont = said.contains("soundfont");
        assert_eq!(
            soundfont,
            kind == InstrumentKind::SoundFont,
            "{kind:?}: {said}"
        );
    }
    assert!(
        no_instrument_text(Some(InstrumentKind::Sampler))
            .to_lowercase()
            .contains("import")
    );
    assert!(
        no_instrument_text(Some(InstrumentKind::Plugin))
            .to_lowercase()
            .contains("plugin")
    );
}
