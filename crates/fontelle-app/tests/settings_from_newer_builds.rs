//! Settings written by a newer build still load here, and survive a save.
//!
//! Found on this machine: a preview build starred an instrument this release
//! has never heard of — `{"instrument": "Theremin"}` in `favorites` — and the
//! release, reading `settings.json`, refused the whole file over that one
//! entry and started on **defaults**: every folder, every star, every
//! rebinding gone for as long as the release ran (`these settings could not
//! be read: unknown variant `Theremin``). Worse, its first save would have
//! written the defaults over the file.
//!
//! So an entry this build cannot read is set aside **on its own** — a
//! favourite out of the list, a field out of the file — everything else
//! loads, and the set-aside entries are written back **verbatim** on save,
//! so going back to the newer build finds its data where it left it.

use fontelle_app::settings::Settings;
use fontelle_types::{EffectKind, Favorite, InstrumentKind};

/// What a newer build wrote: one favourite of a kind this build lacks, among
/// two it knows; a field it has never heard of; and a known field holding a
/// value of a kind this build cannot read.
const FROM_A_NEWER_BUILD: &str = r#"{
  "format_version": 12,
  "soundfont_dirs": ["/music/sf2"],
  "projects_dir": "/music/songs",
  "theme": null,
  "favorites": [
    {"instrument": "Osc3"},
    {"instrument": "Theremin"},
    {"effect": "Reverb"}
  ],
  "keybinds": {"play": "Ctrl+P"},
  "singers_dir": "/music/singers",
  "new_song_routing": "Choir"
}"#;

#[test]
fn one_unreadable_favourite_does_not_cost_the_rest_of_the_file() {
    let settings = Settings::from_json(FROM_A_NEWER_BUILD).expect("it loads");
    assert_eq!(
        settings.soundfont_dirs,
        vec![std::path::PathBuf::from("/music/sf2")]
    );
    assert_eq!(
        settings.projects_dir,
        Some(std::path::PathBuf::from("/music/songs"))
    );
    assert_eq!(
        settings.favorites,
        vec![
            Favorite::Instrument(InstrumentKind::Osc3),
            Favorite::Effect(EffectKind::Reverb),
        ],
        "the two it knows, in their order"
    );
    assert_eq!(
        settings.keybinds.get("play").map(String::as_str),
        Some("Ctrl+P")
    );
    assert_eq!(
        settings.new_song_routing, None,
        "an unreadable value is the default"
    );
}

#[test]
fn what_it_could_not_read_is_written_back_as_it_was() {
    let settings = Settings::from_json(FROM_A_NEWER_BUILD).expect("it loads");
    let saved: serde_json::Value = serde_json::from_str(&settings.to_json()).unwrap();
    let favorites = saved["favorites"].as_array().expect("a list");
    assert!(
        favorites.contains(&serde_json::json!({"instrument": "Theremin"})),
        "the newer build's star survives: {favorites:?}"
    );
    assert_eq!(favorites.len(), 3);
    assert_eq!(
        saved["singers_dir"], "/music/singers",
        "an unknown field survives"
    );
    assert_eq!(
        saved["new_song_routing"], "Choir",
        "and an unreadable value"
    );

    // And it reads back here exactly as it did the first time.
    let again = Settings::from_json(&settings.to_json()).expect("it loads again");
    assert_eq!(again, settings);
}

#[test]
fn a_value_changed_here_wins_over_the_one_set_aside() {
    // The release cannot read "Choir"; once somebody picks a routing here,
    // that is the routing — the stale value must not come back over it.
    let mut settings = Settings::from_json(FROM_A_NEWER_BUILD).expect("it loads");
    settings.new_song_routing = Some(fontelle_model::RoutingMode::Lane);
    let saved: serde_json::Value = serde_json::from_str(&settings.to_json()).unwrap();
    assert_ne!(saved["new_song_routing"], "Choir");
    assert_eq!(
        Settings::from_json(&settings.to_json())
            .unwrap()
            .new_song_routing,
        Some(fontelle_model::RoutingMode::Lane)
    );
}

#[test]
fn unstarring_here_keeps_the_newer_builds_stars() {
    let mut settings = Settings::from_json(FROM_A_NEWER_BUILD).expect("it loads");
    settings.toggle_favorite(Favorite::Instrument(InstrumentKind::Osc3));
    let saved: serde_json::Value = serde_json::from_str(&settings.to_json()).unwrap();
    let favorites = saved["favorites"].as_array().unwrap();
    assert_eq!(favorites.len(), 2);
    assert!(favorites.contains(&serde_json::json!({"instrument": "Theremin"})));
}

#[test]
fn loading_from_a_file_keeps_everything_and_says_nothing_went_wrong() {
    let dir = std::env::temp_dir().join(format!("fontelle-newer-settings-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("settings.json");
    std::fs::write(&path, FROM_A_NEWER_BUILD).unwrap();
    let (settings, error) = Settings::load_from(&path);
    assert!(error.is_none(), "{error:?}");
    assert_eq!(settings.favorites.len(), 2);
    settings.save_to(&path).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("Theremin") && text.contains("singers_dir"));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_file_that_is_not_settings_at_all_is_still_refused() {
    assert!(Settings::from_json("[1, 2, 3]").is_err());
    assert!(
        Settings::from_json(r#"{"favorites": []}"#).is_err(),
        "no format_version"
    );
}
