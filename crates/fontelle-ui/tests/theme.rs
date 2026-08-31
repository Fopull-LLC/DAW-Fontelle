//! The theme token set and its file format (TDD §16.6).
//!
//! A theme is data, and every one of these is a pure function of it — which is
//! the §2.5 rule in `docs/first-usable-plan.md` applied to the one part of the
//! GUI that has no pixels in it at all.

use fontelle_ui::theme::{Color, THEME_FORMAT_VERSION, Theme, ThemeError};

#[test]
fn the_dark_default_is_dark_and_the_light_default_is_light() {
    let dark = Theme::dark_default();
    let light = Theme::light_default();

    // The one thing that makes a dark theme dark: the window is darker than
    // the text on it, and in the light variant it is the other way round.
    assert!(
        luminance(dark.palette.window) < luminance(dark.palette.text),
        "dark theme window {:?} is not darker than its text {:?}",
        dark.palette.window,
        dark.palette.text
    );
    assert!(
        luminance(light.palette.window) > luminance(light.palette.text),
        "light theme window {:?} is not lighter than its text {:?}",
        light.palette.window,
        light.palette.text
    );
}

#[test]
fn every_token_in_the_default_themes_is_opaque_and_legible() {
    for theme in [Theme::dark_default(), Theme::light_default()] {
        // Text on its own panel has to be readable. WCAG's 4.5:1 is the
        // standard for body text; the chrome is small and dense, so this is
        // not a stylistic preference.
        let ratio = contrast(theme.palette.text, theme.palette.panel);
        assert!(
            ratio >= 4.5,
            "{}: text on panel is {ratio:.2}:1, below 4.5:1",
            theme.name
        );
        // Muted text is deliberately quieter, but it is still text.
        let muted = contrast(theme.palette.text_muted, theme.palette.panel);
        assert!(
            muted >= 3.0,
            "{}: muted text on panel is {muted:.2}:1, below 3:1",
            theme.name
        );
        // A grid line that cannot be seen is not a grid line.
        assert_ne!(
            theme.palette.grid_line, theme.palette.panel,
            "{}: the grid line is the panel colour",
            theme.name
        );
    }
}

#[test]
fn a_theme_survives_the_round_trip_through_its_file_format() {
    for original in [Theme::dark_default(), Theme::light_default()] {
        let text = original.to_json();
        let read = Theme::from_json(&text).expect("a theme we just wrote must read back");
        assert_eq!(read, original);
    }
}

#[test]
fn the_written_theme_stamps_its_format_version() {
    let json: serde_json::Value =
        serde_json::from_str(&Theme::dark_default().to_json()).expect("valid JSON");
    assert_eq!(
        json["format_version"],
        serde_json::json!(THEME_FORMAT_VERSION)
    );
}

#[test]
fn colours_are_written_as_hex_so_the_file_is_editable_by_hand() {
    // The whole reason §16.6 says "user themes are just files" is that a
    // person edits them. `[18, 18, 22, 255]` is not something a person edits.
    let json: serde_json::Value =
        serde_json::from_str(&Theme::dark_default().to_json()).expect("valid JSON");
    let window = json["palette"]["window"]
        .as_str()
        .expect("a colour must serialise as a string");
    assert!(
        window.starts_with('#') && (window.len() == 7 || window.len() == 9),
        "colour {window:?} is not #rrggbb or #rrggbbaa"
    );
}

#[test]
fn an_opaque_colour_round_trips_without_growing_an_alpha_pair() {
    assert_eq!(Color::rgb(0x4f, 0x8f, 0xd0).to_hex(), "#4f8fd0");
    assert_eq!(Color::rgba(0x4f, 0x8f, 0xd0, 0x80).to_hex(), "#4f8fd080");
    assert_eq!(
        Color::parse("#4f8fd0").expect("valid"),
        Color::rgb(0x4f, 0x8f, 0xd0)
    );
    assert_eq!(
        Color::parse("#4f8fd080").expect("valid"),
        Color::rgba(0x4f, 0x8f, 0xd0, 0x80)
    );
}

#[test]
fn a_theme_written_by_an_older_build_is_migrated_rather_than_refused() {
    // v0 had no `transport_bar_height` — the transport bar did not exist yet
    // (item 7 of `docs/first-usable-plan.md`). A theme somebody wrote against
    // v0 must still open, with the missing token filled in, because "your
    // theme file stopped working" is not an acceptable cost of adding a
    // panel. This is the migration chain's first real arm.
    let mut json: serde_json::Value =
        serde_json::from_str(&Theme::dark_default().to_json()).expect("valid JSON");
    json["format_version"] = serde_json::json!(0);
    json["metrics"]
        .as_object_mut()
        .expect("metrics is an object")
        .remove("transport_bar_height")
        .expect("v1 has the token v0 lacked");

    let migrated = Theme::from_json(&json.to_string()).expect("a v0 theme must still open");
    assert_eq!(
        migrated.metrics.transport_bar_height,
        Theme::dark_default().metrics.transport_bar_height
    );
    // Everything the old file *did* say is still its own.
    assert_eq!(migrated.palette, Theme::dark_default().palette);
    assert_eq!(migrated.format_version, THEME_FORMAT_VERSION);
}

#[test]
fn a_v0_theme_is_carried_through_every_arm_of_the_chain() {
    // The whole point of a migration chain: a file from two revisions back
    // goes through both steps, not just the last one. Strip everything each
    // revision added and check it all comes back.
    let mut json: serde_json::Value =
        serde_json::from_str(&Theme::dark_default().to_json()).expect("valid JSON");
    json["format_version"] = serde_json::json!(0);
    json["metrics"]
        .as_object_mut()
        .expect("metrics is an object")
        .remove("transport_bar_height")
        .expect("v1 added this");
    let palette = json["palette"]
        .as_object_mut()
        .expect("palette is an object");
    for added_in_v2 in [
        "note",
        "note_selected",
        "key_white",
        "key_black",
        "row_accidental",
    ] {
        palette.remove(added_in_v2).expect("v2 added this");
    }

    let migrated = Theme::from_json(&json.to_string()).expect("a v0 theme must still open");
    let default = Theme::dark_default();
    assert_eq!(migrated.format_version, THEME_FORMAT_VERSION);
    assert_eq!(
        migrated.metrics.transport_bar_height,
        default.metrics.transport_bar_height
    );
    assert_eq!(migrated.palette.note, default.palette.note);
    assert_eq!(migrated.palette.key_white, default.palette.key_white);
    // And nothing the old file actually said was overwritten on the way.
    assert_eq!(migrated.palette.window, default.palette.window);
    assert_eq!(migrated.name, default.name);
}

#[test]
fn a_migration_does_not_overwrite_what_the_old_file_did_say() {
    // The `or_insert` half of the contract. A v1 theme with a hand-picked
    // window colour must come out with *that* colour, not the default the
    // migration would have supplied for a missing one.
    let mut json: serde_json::Value =
        serde_json::from_str(&Theme::dark_default().to_json()).expect("valid JSON");
    json["format_version"] = serde_json::json!(1);
    json["palette"]["window"] = serde_json::json!("#123456");
    json["palette"]
        .as_object_mut()
        .expect("palette is an object")
        .remove("note")
        .expect("v2 added this");

    let migrated = Theme::from_json(&json.to_string()).expect("a v1 theme must still open");
    assert_eq!(migrated.palette.window, Color::rgb(0x12, 0x34, 0x56));
    assert_eq!(migrated.palette.note, Theme::dark_default().palette.note);
}

#[test]
fn a_theme_from_a_newer_build_is_refused_by_version_not_by_field() {
    let mut json: serde_json::Value =
        serde_json::from_str(&Theme::dark_default().to_json()).expect("valid JSON");
    json["format_version"] = serde_json::json!(THEME_FORMAT_VERSION + 9);
    // Break a field too: the version must be checked *before* the body, so
    // this reports the version rather than whichever field parsed first.
    json["palette"]["window"] = serde_json::json!("not a colour");

    match Theme::from_json(&json.to_string()) {
        Err(ThemeError::FromTheFuture { found, newest }) => {
            assert_eq!(found, THEME_FORMAT_VERSION + 9);
            assert_eq!(newest, THEME_FORMAT_VERSION);
        }
        other => panic!("expected FromTheFuture, got {other:?}"),
    }
}

#[test]
fn a_broken_theme_says_what_is_wrong_with_it() {
    let cases = [
        ("{", "truncated"),
        ("{}", "no format_version"),
        (
            &format!(r#"{{"format_version":{THEME_FORMAT_VERSION}}}"#),
            "no tokens",
        ),
    ];
    for (text, what) in cases {
        let err = Theme::from_json(text).expect_err(&format!("{what} must not load"));
        let message = err.to_string();
        assert!(
            !message.is_empty() && !message.contains("Error"),
            "{what}: unhelpful message {message:?}"
        );
    }
}

#[test]
fn a_colour_that_is_not_a_colour_is_rejected() {
    for bad in ["", "#", "#abc", "4f8fd0", "#gggggg", "#4f8fd0ff00"] {
        assert!(
            Color::parse(bad).is_err(),
            "{bad:?} should not parse as a colour"
        );
    }
}

#[test]
fn a_theme_loads_from_a_file_and_a_missing_file_says_so() {
    let dir = std::env::temp_dir().join(format!("fontelle-theme-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join("mine.json");
    std::fs::write(&path, Theme::light_default().to_json()).expect("write");

    let loaded = Theme::load_from_file(&path).expect("a theme file we just wrote must load");
    assert_eq!(loaded, Theme::light_default());

    let missing = dir.join("nope.json");
    match Theme::load_from_file(&missing) {
        Err(ThemeError::Io { path, .. }) => assert_eq!(path, missing),
        other => panic!("expected an Io error naming the path, got {other:?}"),
    }
    std::fs::remove_dir_all(&dir).ok();
}

/// Relative luminance, WCAG 2.x.
fn luminance(c: Color) -> f64 {
    let f = |v: u8| {
        let v = v as f64 / 255.0;
        if v <= 0.03928 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * f(c.0[0]) + 0.7152 * f(c.0[1]) + 0.0722 * f(c.0[2])
}

fn contrast(a: Color, b: Color) -> f64 {
    let (x, y) = (luminance(a), luminance(b));
    let (hi, lo) = if x > y { (x, y) } else { (y, x) };
    (hi + 0.05) / (lo + 0.05)
}

/// v4 added the key map's colours: the roll now says which keys the instrument
/// on the channel can actually play, and that needs ink of its own for a dead
/// row, a dead key and a note sitting on one.
///
/// The same contract as every arm before it — a file from before the feature
/// opens, with the new tokens filled in from the default and everything it did
/// say left alone.
#[test]
fn a_v3_theme_gains_the_key_maps_colours_and_keeps_its_own() {
    let mut json: serde_json::Value =
        serde_json::from_str(&Theme::dark_default().to_json()).expect("valid JSON");
    json["format_version"] = serde_json::json!(3);
    json["palette"]["note"] = serde_json::json!("#123456");
    let palette = json["palette"]
        .as_object_mut()
        .expect("palette is an object");
    for added_in_v4 in ["row_dead", "key_dead", "note_silent"] {
        palette.remove(added_in_v4).expect("v4 added this");
    }

    let migrated = Theme::from_json(&json.to_string()).expect("a v3 theme must still open");
    let default = Theme::dark_default();
    assert_eq!(migrated.format_version, THEME_FORMAT_VERSION);
    assert_eq!(migrated.palette.row_dead, default.palette.row_dead);
    assert_eq!(migrated.palette.key_dead, default.palette.key_dead);
    assert_eq!(migrated.palette.note_silent, default.palette.note_silent);
    assert_eq!(
        migrated.palette.note,
        Color::rgb(0x12, 0x34, 0x56),
        "the colour the old file chose is still its own"
    );
}

/// A dead row that looks like a live one says nothing, and a silent note that
/// looks like a sounding one is worse than no marking at all — it is a lie
/// about what will happen when you press play.
#[test]
fn the_key_maps_colours_are_tellable_from_the_ones_they_sit_beside() {
    for theme in [Theme::dark_default(), Theme::light_default()] {
        let p = &theme.palette;
        assert_ne!(
            p.row_dead, p.panel,
            "{}: a dead row is the plain grid colour",
            theme.name
        );
        assert_ne!(
            p.row_dead, p.row_accidental,
            "{}: a dead row is indistinguishable from an accidental's",
            theme.name
        );
        assert_ne!(
            p.key_dead, p.key_white,
            "{}: a dead key looks like a live natural",
            theme.name
        );
        assert_ne!(
            p.note_silent, p.note,
            "{}: a silent note looks like one that sounds",
            theme.name
        );
    }
}

/// v5 added the grid's third level.
///
/// Reported from using the window: *"it's kind of hard to tell the time right
/// now so ensure between bars or measures there's more dividers so I can see
/// the on and offbeats."* The dividers were there — the roll has always drawn
/// subdivision, beat and bar lines — but subdivisions and beats came out in
/// the *same* ink, so the grid read as one undifferentiated comb and there was
/// no way to count a bar by eye.
#[test]
fn a_v4_theme_gains_the_subdivision_line_and_keeps_its_own() {
    let mut json: serde_json::Value =
        serde_json::from_str(&Theme::dark_default().to_json()).expect("valid JSON");
    json["format_version"] = serde_json::json!(4);
    json["palette"]["grid_line"] = serde_json::json!("#123456");
    json["palette"]
        .as_object_mut()
        .expect("palette is an object")
        .remove("grid_line_sub")
        .expect("v5 added this");

    let migrated = Theme::from_json(&json.to_string()).expect("a v4 theme must still open");
    assert_eq!(migrated.format_version, THEME_FORMAT_VERSION);
    assert_eq!(
        migrated.palette.grid_line_sub,
        Theme::dark_default().palette.grid_line_sub
    );
    assert_eq!(
        migrated.palette.grid_line,
        Color::rgb(0x12, 0x34, 0x56),
        "the beat line the old file chose is still its own"
    );
}

/// The three levels have to be three: a grid whose bar, beat and sixteenth
/// lines are the same colour is a grid you cannot count.
#[test]
fn the_grids_three_levels_are_three_different_inks() {
    for theme in [Theme::dark_default(), Theme::light_default()] {
        let p = &theme.palette;
        assert_ne!(
            p.grid_line_sub, p.grid_line,
            "{}: a sixteenth line looks like a beat line",
            theme.name
        );
        assert_ne!(
            p.grid_line, p.grid_line_strong,
            "{}: a beat line looks like a bar line",
            theme.name
        );
        assert_ne!(
            p.grid_line_sub, p.panel,
            "{}: a subdivision that cannot be seen is not a divider",
            theme.name
        );
    }
}
