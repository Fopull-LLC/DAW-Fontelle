//! The gestures on a Flopsynth knob, as arithmetic (`docs/flopsynth-next.md`
//! §3.3): a drag at three precisions, a nudge by keys or the wheel, and a
//! typed value read into a number the host can place. Pure, so the window
//! only dispatches (plan §13's thirty-line rule).

use fontelle_ui::canvas::{
    NUDGE, NUDGE_FINE, ParamKind, Precision, Typed, knob_drag, nudged, parse_typed, wheel_nudge,
};

#[test]
fn a_drag_has_three_precisions_and_ctrl_is_the_finest() {
    // A hundred and fifty pixels is the whole travel; Shift six times that;
    // Ctrl twenty times — a knob you can set to the cent.
    assert!((knob_drag(0.5, -150.0, Precision::Coarse) - 1.0).abs() < 1e-6);
    assert!((knob_drag(0.5, -150.0, Precision::Fine) - (0.5 + 1.0 / 6.0)).abs() < 1e-5);
    assert!((knob_drag(0.5, -150.0, Precision::Finer) - (0.5 + 1.0 / 20.0)).abs() < 1e-5);
    assert_eq!(knob_drag(0.5, 400.0, Precision::Coarse), 0.0, "clamped");
    // Ctrl wins over Shift: the finer of the two is what a held pair means.
    assert_eq!(Precision::from_modifiers(false, false), Precision::Coarse);
    assert_eq!(Precision::from_modifiers(true, false), Precision::Fine);
    assert_eq!(Precision::from_modifiers(false, true), Precision::Finer);
    assert_eq!(Precision::from_modifiers(true, true), Precision::Finer);
}

#[test]
fn a_nudge_is_a_hundredth_and_a_fine_nudge_a_thousandth() {
    assert!((nudged(0.5, 1, Precision::Coarse) - (0.5 + NUDGE)).abs() < 1e-6);
    assert!((nudged(0.5, -3, Precision::Coarse) - (0.5 - 3.0 * NUDGE)).abs() < 1e-6);
    assert!((nudged(0.5, 1, Precision::Fine) - (0.5 + NUDGE_FINE)).abs() < 1e-6);
    assert!((nudged(0.5, 1, Precision::Finer) - (0.5 + NUDGE_FINE)).abs() < 1e-6);
    assert_eq!(nudged(0.999, 5, Precision::Coarse), 1.0);
    assert_eq!(nudged(0.0, -1, Precision::Coarse), 0.0);
}

#[test]
fn the_wheel_with_ctrl_nudges_a_knob_steps_a_chooser_and_flips_a_switch() {
    let knob = ParamKind::Knob;
    assert!((wheel_nudge(0.5, 1.0, &knob) - (0.5 + NUDGE)).abs() < 1e-6);
    assert!((wheel_nudge(0.5, -2.0, &knob) - (0.5 - 2.0 * NUDGE)).abs() < 1e-6);
    // A chooser of four: one option per notch, no wrap — the wheel stops at
    // the ends, where a menu would.
    let chooser = ParamKind::Choice(vec!["a".into(), "b".into(), "c".into(), "d".into()]);
    let step = 1.0 / 3.0;
    assert!((wheel_nudge(0.0, 1.0, &chooser) - step).abs() < 1e-5);
    assert!((wheel_nudge(step, 1.0, &chooser) - 2.0 * step).abs() < 1e-5);
    assert!(
        (wheel_nudge(1.0, 1.0, &chooser) - 1.0).abs() < 1e-5,
        "no wrap"
    );
    assert!((wheel_nudge(0.0, -1.0, &chooser)).abs() < 1e-5);
    // A switch flips either way.
    assert_eq!(wheel_nudge(1.0, 1.0, &ParamKind::Switch), 0.0);
    assert_eq!(wheel_nudge(0.0, -1.0, &ParamKind::Switch), 1.0);
}

#[test]
fn a_typed_value_reads_its_number_its_unit_and_its_prefix() {
    // The four spellings §3.3 names, and the ones a read-out shows.
    assert_eq!(parse_typed("2.4k"), Some(Typed::new(2400.0, "")));
    assert_eq!(parse_typed("-12"), Some(Typed::new(-12.0, "")));
    assert_eq!(parse_typed("37%"), Some(Typed::percent(0.37)));
    assert_eq!(parse_typed("37 %"), Some(Typed::percent(0.37)));
    assert_eq!(parse_typed("1/8"), Some(Typed::fraction(0.125)));
    assert_eq!(parse_typed("9.00 kHz"), Some(Typed::new(9000.0, "hz")));
    assert_eq!(parse_typed("900 ms"), Some(Typed::new(900.0, "ms")));
    assert_eq!(parse_typed("+0.0 dB"), Some(Typed::new(0.0, "db")));
    assert_eq!(parse_typed("-11.2dB"), Some(Typed::new(-11.2, "db")));
    assert_eq!(parse_typed("15 c"), Some(Typed::new(15.0, "c")));
    assert_eq!(parse_typed("+0 st"), Some(Typed::new(0.0, "st")));
    assert_eq!(parse_typed("55L"), Some(Typed::new(55.0, "l")));
    assert_eq!(parse_typed("  1.5 s "), Some(Typed::new(1.5, "s")));
    assert_eq!(parse_typed("2.4 kHz"), Some(Typed::new(2400.0, "hz")));
    assert_eq!(parse_typed("centre"), None, "a word is not a number");
    assert_eq!(parse_typed(""), None);
    assert_eq!(parse_typed("1/0"), None);
    // A display and a typed entry agree when their numbers meet in the same
    // unit, or the entry names none.
    let display = parse_typed("9.00 kHz").unwrap();
    assert!(display.matches_unit(&Typed::new(9000.0, "")));
    assert!(display.matches_unit(&Typed::new(9000.0, "hz")));
    assert!(!display.matches_unit(&Typed::new(9000.0, "ms")));
}

/// The knob's right-click menu (§3.3): what Serum's does, as one list whose
/// rows the window dispatches on. Built from what is true of the knob — a
/// preset to go back to, routes on it, whether it can be modulated, a
/// value on the clipboard — so a row that would do nothing is greyed
/// rather than missing, and a chooser is not offered a typed number's
/// gestures it cannot take.
#[test]
fn the_knob_menu_offers_what_is_true_of_the_knob() {
    use fontelle_ui::canvas::{FlopKnobMenu, FlopKnobMenuItem as Item, flop_knob_menu};
    let full = FlopKnobMenu {
        name: "CUTOFF",
        kind: &ParamKind::Knob,
        has_preset: true,
        routes: &["ENV 2".to_string(), "LFO 1".to_string()],
        is_destination: true,
        clipboard: true,
    };
    let rows = flop_knob_menu(&full);
    let items: Vec<Item> = rows.iter().map(|(_, item)| *item).collect();
    assert_eq!(
        items,
        [
            Item::Heading,
            Item::ResetPreset,
            Item::ResetDefault,
            Item::TypeValue,
            Item::ModulateFrom,
            Item::RemoveRoute(0),
            Item::RemoveRoute(1),
            Item::AssignMacro,
            Item::CreateAutomation,
            Item::CopyValue,
            Item::PasteValue,
        ]
    );
    assert_eq!(rows[0].0.label, "CUTOFF");
    assert!(!rows[0].0.enabled, "the heading is not a row to press");
    assert_eq!(rows[5].0.label, "Remove ENV 2");
    assert_eq!(rows[6].0.label, "Remove LFO 1");
    assert!(rows[4].0.separator && rows[8].0.separator, "three groups");
    assert!(
        rows.iter()
            .all(|(entry, item)| entry.enabled || *item == Item::Heading)
    );

    // A knob from no preset, with nothing on it, nothing to paste, that
    // cannot be modulated: the rows are there and say no.
    let bare = FlopKnobMenu {
        name: "VOLUME",
        kind: &ParamKind::Knob,
        has_preset: false,
        routes: &[],
        is_destination: false,
        clipboard: false,
    };
    let rows = flop_knob_menu(&bare);
    let enabled = |item: Item| {
        rows.iter()
            .find(|(_, i)| *i == item)
            .map(|(e, _)| e.enabled)
    };
    assert_eq!(enabled(Item::ResetPreset), Some(false));
    assert_eq!(enabled(Item::ResetDefault), Some(true));
    assert_eq!(enabled(Item::ModulateFrom), Some(false));
    assert_eq!(enabled(Item::AssignMacro), Some(false));
    assert_eq!(enabled(Item::PasteValue), Some(false));
    assert!(!rows.iter().any(|(_, i)| matches!(i, Item::RemoveRoute(_))));

    // A chooser takes a typed name but not a number's copy and paste; a
    // switch takes neither.
    let chooser = FlopKnobMenu {
        kind: &ParamKind::Choice(vec!["a".into()]),
        ..full.clone()
    };
    let rows = flop_knob_menu(&chooser);
    assert!(rows.iter().any(|(_, i)| *i == Item::TypeValue));
    assert!(
        !rows
            .iter()
            .any(|(_, i)| matches!(i, Item::CopyValue | Item::PasteValue))
    );
    let switch = FlopKnobMenu {
        kind: &ParamKind::Switch,
        ..full.clone()
    };
    let rows = flop_knob_menu(&switch);
    assert!(
        !rows
            .iter()
            .any(|(_, i)| matches!(i, Item::TypeValue | Item::CopyValue))
    );
}
