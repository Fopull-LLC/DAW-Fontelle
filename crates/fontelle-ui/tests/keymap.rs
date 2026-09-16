//! The keymap: every shortcut as a **binding** rather than a literal in a
//! `match`, so the shortcuts page can change them.
//!
//! > *"could you make these keybinds in the ? tab completely configurable so
//! > users can cleanly click on one they don't like, then input the new
//! > binding they want ... it waits for the release to see your combination
//! > basically."*
//!
//! Three things are tested here, all pure. A **chord** — modifiers and one
//! key — that can be written down and read back, because the map is saved in
//! the settings file as text. The **map** from chords to actions: its
//! defaults (FL's, as the window has always had them), what it answers for a
//! chord in a given context, and what a rebind does to a chord that was
//! somebody else's. And the **listener** that turns a press-and-release into
//! a chord: it commits on the release of the key, with whatever modifiers
//! were down at either end, so pressing Ctrl, then X, then letting go in
//! either order is Ctrl+X and not X.

use fontelle_ui::canvas::{Action, Chord, ChordKey, Context, Keymap, Rebind};

fn chord(text: &str) -> Chord {
    Chord::parse(text).unwrap_or_else(|| panic!("{text:?} must parse"))
}

// ---------------------------------------------------------------- chords ---

#[test]
fn a_chord_reads_back_as_the_text_it_was_written_from() {
    for text in [
        "Space",
        "Home",
        "Tab",
        "F1",
        "F12",
        "Delete",
        "Backspace",
        "Ctrl+Z",
        "Ctrl+Shift+Z",
        "Ctrl+Shift+Alt+M",
        "Alt+A",
        "Shift+Tab",
        "+",
        "-",
        "1",
    ] {
        let parsed = chord(text);
        assert_eq!(parsed.label(), text, "{text:?} did not round-trip");
        assert_eq!(Chord::parse(&parsed.label()), Some(parsed));
    }
}

#[test]
fn a_chord_is_read_however_it_is_cased_or_spaced() {
    assert_eq!(chord("ctrl + shift + z"), chord("Ctrl+Shift+Z"));
    assert_eq!(chord("SPACE"), chord("Space"));
    assert_eq!(
        chord("Shift+ctrl+z"),
        chord("Ctrl+Shift+Z"),
        "modifier order is not meaning"
    );
    assert_eq!(chord("Z"), chord("z"));
}

#[test]
fn shift_is_part_of_every_chord_and_the_typed_symbol_is_only_a_fallback() {
    // A chord is the key that was pressed plus the modifiers held — so
    // Shift+1 is not 1, whatever `!` the layout prints for it. But `+` is
    // Shift+= on a US keyboard and its own key on another, and the window
    // has always taken both for zoom: so when a shifted non-letter is bound
    // to nothing, the symbol it *typed* is tried next. The typed symbol
    // never overrides a binding to the chord itself.
    assert_ne!(chord("Shift+1"), chord("1"));
    assert_ne!(chord("Shift+="), chord("="));
    assert_eq!(chord("Shift++").label(), "Shift++");

    let map = Keymap::default();
    assert_eq!(
        map.action_of_press(
            &chord("Shift+="),
            Some(ChordKey::Char('+')),
            Context::Studio
        ),
        Some(Action::ZoomIn),
        "Shift+= on a US keyboard types +, and + is zoom"
    );
    assert_eq!(
        map.action_of_press(
            &chord("Shift+1"),
            Some(ChordKey::Char('!')),
            Context::Studio
        ),
        None,
        "Shift+1 types !, which is bound to nothing — and 1 is not tried"
    );
    assert_eq!(
        map.action_of_press(
            &chord("Shift+M"),
            Some(ChordKey::Char('m')),
            Context::Studio
        ),
        None,
        "a letter has no typed fallback: Shift+M is not M"
    );
    // A binding to the shifted chord itself wins over what it types.
    let mut map = Keymap::default();
    map.rebind(Action::Play, chord("Shift+="));
    assert_eq!(
        map.action_of_press(
            &chord("Shift+="),
            Some(ChordKey::Char('+')),
            Context::Studio
        ),
        Some(Action::Play)
    );
    // And without shift there is nothing to fall back to.
    assert_eq!(
        map.action_of_press(&chord("="), Some(ChordKey::Char('=')), Context::Studio),
        Some(Action::ZoomIn)
    );
}

#[test]
fn text_that_is_not_a_chord_is_refused() {
    for text in [
        "",
        "Ctrl",
        "Ctrl+",
        "Ctrl+Shift",
        "Foo",
        "F0",
        "F25",
        "ab",
        "Ctrl+Space+Z",
    ] {
        assert_eq!(Chord::parse(text), None, "{text:?} was read as a chord");
    }
}

// ----------------------------------------------------------- the defaults ---

#[test]
fn every_action_has_a_default_that_parses_and_a_stable_id() {
    let map = Keymap::default();
    let mut ids: Vec<&str> = Vec::new();
    for action in Action::ALL {
        assert!(!map.chords(action).is_empty(), "{action:?} ships unbound");
        assert!(!action.does().is_empty(), "{action:?} says nothing");
        let id = action.id();
        assert!(
            !id.is_empty() && id.chars().all(|c| c.is_ascii_lowercase() || c == '-'),
            "{action:?}'s id {id:?} is not a plain kebab word"
        );
        ids.push(id);
    }
    ids.sort_unstable();
    let before = ids.len();
    ids.dedup();
    assert_eq!(ids.len(), before, "two actions share an id");
    assert_eq!(Action::ALL.len(), before);
}

#[test]
fn the_defaults_are_the_bindings_the_window_has_always_had() {
    let map = Keymap::default();
    let bound = |action: Action| map.label(action);
    assert_eq!(bound(Action::Play), "Space");
    assert_eq!(bound(Action::Stop), "Home");
    assert_eq!(bound(Action::Metronome), "Ctrl+M");
    assert_eq!(bound(Action::Save), "Ctrl+S");
    assert_eq!(bound(Action::Undo), "Ctrl+Z");
    assert_eq!(bound(Action::Redo), "Ctrl+Shift+Z / Ctrl+Y");
    assert_eq!(bound(Action::ExportWav), "Ctrl+E");
    assert_eq!(bound(Action::ExportMidi), "Ctrl+Shift+E");
    assert_eq!(bound(Action::Help), "F1");
    assert_eq!(bound(Action::ShowRoll), "1");
    assert_eq!(bound(Action::ShowMixer), "2");
    assert_eq!(bound(Action::RackTab), "F");
    assert_eq!(bound(Action::SwapSplit), "Tab");
    assert_eq!(bound(Action::ToggleTimeline), "Ctrl+T");
    assert_eq!(bound(Action::ToolsPanel), "T");
    assert_eq!(bound(Action::Search), "Ctrl+F");
    assert_eq!(bound(Action::ZoomIn), "+ / =");
    assert_eq!(bound(Action::ZoomOut), "- / _");
    assert_eq!(bound(Action::DrawTool), "P");
    assert_eq!(bound(Action::PaintTool), "B");
    assert_eq!(bound(Action::SelectTool), "E");
    assert_eq!(bound(Action::DeleteTool), "D");
    assert_eq!(bound(Action::SliceTool), "C");
    assert_eq!(bound(Action::SnapOrStretch), "S");
    assert_eq!(bound(Action::Slide), "A");
    assert_eq!(bound(Action::LaneProperty), "L");
    assert_eq!(bound(Action::Ghosts), "G");
    assert_eq!(bound(Action::LegatoOrPlayMode), "Ctrl+L");
    assert_eq!(bound(Action::SelectAll), "Ctrl+A");
    assert_eq!(bound(Action::Copy), "Ctrl+C");
    assert_eq!(bound(Action::Cut), "Ctrl+X");
    assert_eq!(bound(Action::Paste), "Ctrl+V");
    assert_eq!(bound(Action::Duplicate), "Ctrl+B / Ctrl+D");
    assert_eq!(bound(Action::DeleteSelection), "Delete / Backspace");
    assert_eq!(bound(Action::MuteClips), "Ctrl+Shift+M");
    assert_eq!(bound(Action::MuteTrack), "M");
    assert_eq!(bound(Action::SoloTrack), "N");
    assert_eq!(bound(Action::RemoveBand), "Delete / Backspace");
}

#[test]
fn no_two_actions_that_can_both_hear_a_key_share_a_default_for_it() {
    // `D` is the delete tool in the studio and Delete removes an EQ band in
    // an editor window: different keys, and even if they were the same the
    // two never listen at once. What must never happen is one chord with two
    // answers in one place.
    let map = Keymap::default();
    for a in Action::ALL {
        for b in Action::ALL {
            if a == b || !a.context().overlaps(b.context()) {
                continue;
            }
            for chord in map.chords(a) {
                assert!(
                    !map.chords(b).contains(chord),
                    "{a:?} and {b:?} both answer {}",
                    chord.label()
                );
            }
        }
    }
}

// --------------------------------------------------------------- lookups ---

#[test]
fn a_chord_answers_the_action_bound_to_it_in_the_context_that_asks() {
    let map = Keymap::default();
    // The global keys answer wherever they are pressed.
    assert_eq!(
        map.action(&chord("Ctrl+M"), Context::Studio),
        Some(Action::Metronome)
    );
    assert_eq!(
        map.action(&chord("Ctrl+M"), Context::Editor),
        Some(Action::Metronome)
    );
    assert_eq!(
        map.action(&chord("Space"), Context::Editor),
        Some(Action::Play)
    );
    // The studio's own keys do not reach an editor window.
    assert_eq!(
        map.action(&chord("D"), Context::Studio),
        Some(Action::DeleteTool)
    );
    assert_eq!(map.action(&chord("D"), Context::Editor), None);
    // And the same key means two things in the two places.
    assert_eq!(
        map.action(&chord("Delete"), Context::Studio),
        Some(Action::DeleteSelection)
    );
    assert_eq!(
        map.action(&chord("Delete"), Context::Editor),
        Some(Action::RemoveBand)
    );
    // Either of an action's chords answers it.
    assert_eq!(
        map.action(&chord("Ctrl+Y"), Context::Studio),
        Some(Action::Redo)
    );
    assert_eq!(
        map.action(&chord("Ctrl+Shift+Z"), Context::Studio),
        Some(Action::Redo)
    );
    // A chord bound to nothing is nothing.
    assert_eq!(
        map.action(&chord("Ctrl+Shift+Alt+Q"), Context::Studio),
        None
    );
}

#[test]
fn a_shifted_letter_is_not_the_unshifted_one() {
    let map = Keymap::default();
    assert_eq!(map.action(&chord("Shift+M"), Context::Studio), None);
    assert_eq!(
        map.action(&chord("M"), Context::Studio),
        Some(Action::MuteTrack)
    );
}

// ------------------------------------------------------------- rebinding ---

#[test]
fn rebinding_replaces_the_actions_chords_with_the_one_pressed() {
    let mut map = Keymap::default();
    let taken = map.rebind(Action::Redo, chord("Ctrl+R"));
    assert!(taken.is_empty(), "Ctrl+R was nobody's");
    assert_eq!(map.label(Action::Redo), "Ctrl+R");
    assert_eq!(
        map.action(&chord("Ctrl+R"), Context::Studio),
        Some(Action::Redo)
    );
    assert_eq!(
        map.action(&chord("Ctrl+Y"), Context::Studio),
        None,
        "the old chord is let go — one key, one meaning"
    );
}

#[test]
fn a_chord_taken_from_another_action_leaves_that_action_without_it() {
    // What every editor does, said plainly: the chord goes to the action you
    // pressed it on, and the action it came from is told. Nothing is
    // silently two things.
    let mut map = Keymap::default();
    let taken = map.rebind(Action::Play, chord("Ctrl+S"));
    assert_eq!(taken, vec![Action::Save], "Save lost Ctrl+S");
    assert_eq!(map.label(Action::Play), "Ctrl+S");
    assert!(map.chords(Action::Save).is_empty());
    assert_eq!(map.label(Action::Save), Keymap::UNBOUND);
    assert_eq!(
        map.action(&chord("Ctrl+S"), Context::Studio),
        Some(Action::Play)
    );
    // An action with two chords keeps the other one.
    let taken = map.rebind(Action::Stop, chord("Ctrl+Y"));
    assert_eq!(taken, vec![Action::Redo]);
    assert_eq!(map.label(Action::Redo), "Ctrl+Shift+Z");
}

#[test]
fn a_chord_is_only_taken_from_an_action_that_could_have_heard_it() {
    // Delete removes an EQ band in an editor window and deletes the
    // selection in the studio, and the two never listen at once — so
    // binding a studio action to Delete need not cost the editor its key.
    let mut map = Keymap::default();
    let taken = map.rebind(Action::MuteClips, chord("Delete"));
    assert_eq!(taken, vec![Action::DeleteSelection]);
    assert_eq!(map.label(Action::RemoveBand), "Delete / Backspace");
    // A global action, though, is heard everywhere, so it takes from both.
    let mut map = Keymap::default();
    let mut taken = map.rebind(Action::Play, chord("Delete"));
    taken.sort_by_key(|a| a.id());
    let mut wanted = vec![Action::DeleteSelection, Action::RemoveBand];
    wanted.sort_by_key(|a| a.id());
    assert_eq!(taken, wanted);
}

#[test]
fn rebinding_an_action_to_a_chord_it_already_has_changes_nothing_else() {
    let mut map = Keymap::default();
    let taken = map.rebind(Action::Redo, chord("Ctrl+Y"));
    assert!(taken.is_empty());
    assert_eq!(
        map.label(Action::Redo),
        "Ctrl+Y",
        "one chord now, as pressed"
    );
}

#[test]
fn reset_puts_every_default_back() {
    let mut map = Keymap::default();
    map.rebind(Action::Play, chord("Ctrl+S"));
    map.rebind(Action::DrawTool, chord("Q"));
    assert!(map.is_changed());
    map.reset();
    assert!(!map.is_changed());
    assert_eq!(map, Keymap::default());
}

// ---------------------------------------------------------- persistence ---

#[test]
fn only_what_differs_from_the_defaults_is_written_and_it_reads_back() {
    let mut map = Keymap::default();
    assert!(map.overrides().is_empty(), "the defaults write nothing");
    map.rebind(Action::Play, chord("Ctrl+S"));
    let written = map.overrides();
    let mut ids: Vec<&str> = written.iter().map(|(id, _)| id.as_str()).collect();
    ids.sort_unstable();
    assert_eq!(
        ids,
        vec!["play", "save"],
        "Play changed, and Save lost its chord; nothing else moved"
    );
    let play = written.iter().find(|(id, _)| id == "play").expect("play");
    assert_eq!(play.1, "Ctrl+S");
    let save = written.iter().find(|(id, _)| id == "save").expect("save");
    assert_eq!(save.1, "", "an unbound action is written as nothing");

    let back = Keymap::with_overrides(&written);
    assert_eq!(back, map);
}

#[test]
fn an_override_the_map_does_not_understand_is_ignored_not_fatal() {
    // A settings file from a newer build, or one somebody edited by hand: an
    // action this build has no name for, or a chord it cannot read, leaves
    // the default in place rather than refusing the whole file.
    let back = Keymap::with_overrides(&[
        ("no-such-action".to_string(), "Ctrl+Q".to_string()),
        ("play".to_string(), "Ctrl+".to_string()),
        ("stop".to_string(), "Ctrl+Shift+H".to_string()),
    ]);
    assert_eq!(back.label(Action::Play), "Space");
    assert_eq!(back.label(Action::Stop), "Ctrl+Shift+H");
}

#[test]
fn two_chords_are_written_and_read_as_two() {
    let mut map = Keymap::default();
    map.rebind(Action::Save, chord("Ctrl+Q"));
    // Redo still has both of its defaults, so it is not written — but a map
    // that reads a two-chord line must keep both.
    let back = Keymap::with_overrides(&[("redo".to_string(), "Ctrl+R / Ctrl+Y".to_string())]);
    assert_eq!(back.label(Action::Redo), "Ctrl+R / Ctrl+Y");
    assert_eq!(
        back.action(&chord("Ctrl+R"), Context::Studio),
        Some(Action::Redo)
    );
}

// ------------------------------------------------------------ listening ---

#[test]
fn the_listener_commits_on_the_release_of_the_key_with_the_modifiers_held() {
    let mut listen = Rebind::new(Action::Play);
    assert_eq!(listen.action(), Action::Play);
    // Ctrl goes down: a modifier alone is not a chord, and not the end of one.
    assert_eq!(listen.press(None, true, false, false), None);
    assert_eq!(listen.release(None, true, false, false), None);
    // X with Ctrl held: still nothing until it comes up.
    assert_eq!(
        listen.press(Some(ChordKey::Char('x')), true, false, false),
        None
    );
    assert_eq!(
        listen.release(Some(ChordKey::Char('x')), true, false, false),
        Some(chord("Ctrl+X"))
    );
}

#[test]
fn letting_go_of_the_modifier_first_still_counts_it() {
    // Ctrl, X, then Ctrl up before X up — the commonest way hands actually
    // do it. The modifiers at the press *and* at the release both count.
    let mut listen = Rebind::new(Action::Play);
    listen.press(Some(ChordKey::Char('x')), true, false, false);
    assert_eq!(
        listen.release(Some(ChordKey::Char('x')), false, false, false),
        Some(chord("Ctrl+X"))
    );
    // And the other way round: a modifier added while the key is held.
    let mut listen = Rebind::new(Action::Play);
    listen.press(Some(ChordKey::Char('x')), false, false, false);
    assert_eq!(
        listen.release(Some(ChordKey::Char('x')), true, true, false),
        Some(chord("Ctrl+Shift+X"))
    );
}

#[test]
fn a_second_key_replaces_the_first_and_a_stray_release_is_nothing() {
    let mut listen = Rebind::new(Action::Play);
    listen.press(Some(ChordKey::Char('x')), false, false, false);
    listen.press(Some(ChordKey::Char('y')), false, false, false);
    assert_eq!(
        listen.release(Some(ChordKey::Char('x')), false, false, false),
        None,
        "the key that came up is not the one being listened for"
    );
    assert_eq!(
        listen.release(Some(ChordKey::Char('y')), false, false, false),
        Some(chord("Y"))
    );
    // Nothing pressed yet: a release is nothing.
    let mut listen = Rebind::new(Action::Play);
    assert_eq!(
        listen.release(Some(ChordKey::Space), false, false, false),
        None
    );
}

#[test]
fn a_named_key_makes_a_chord_too() {
    let mut listen = Rebind::new(Action::SwapSplit);
    listen.press(Some(ChordKey::F(5)), false, true, false);
    assert_eq!(
        listen.release(Some(ChordKey::F(5)), false, true, false),
        Some(chord("Shift+F5"))
    );
}

// ------------------------------------------- what the keys are not ---

#[test]
fn an_arrow_is_not_a_chord_because_the_arrows_are_a_fixed_family() {
    // Four directions by four modifier sets with one meaning is a table
    // (`arrow`), answered before the map is asked — so a binding to an arrow
    // could never fire, and the listener must not offer one.
    for text in ["Up", "Down", "Left", "Right", "Ctrl+Up", "Shift+Left"] {
        assert_eq!(Chord::parse(text), None, "{text:?} was read as a chord");
    }
    // And Esc, which every prompt and page relies on to be itself.
    assert_eq!(Chord::parse("Esc"), None);
    assert_eq!(Chord::parse("Escape"), None);
}

// ---------------------------- every control a key also drives says so ---

#[test]
fn the_controls_a_key_also_drives_name_their_action_so_their_tips_read_the_map() {
    // *"make sure things like when a tooltip is shown it shows ur actual
    // configured keybind."* A tip's key comes from `action()` and the map,
    // never from the tip's own words — so every control with a key has one.
    use fontelle_ui::canvas::{BrowserHit, BrowserMode, MixerHit, PrefabHit, RackHit};
    use fontelle_ui::document::RackTab;
    use fontelle_ui::layout::EditorTab;
    assert_eq!(EditorTab::Roll.action(), Some(Action::ShowRoll));
    assert_eq!(EditorTab::Mixer.action(), Some(Action::ShowMixer));
    assert_eq!(MixerHit::Mute(0).action(), Some(Action::MuteTrack));
    assert_eq!(MixerHit::Solo(3).action(), Some(Action::SoloTrack));
    assert_eq!(MixerHit::Fader(0).action(), None);
    assert_eq!(
        RackHit::Tab(RackTab::Prefabs).action(),
        Some(Action::RackTab)
    );
    assert_eq!(
        PrefabHit::Tab(RackTab::Instruments).action(),
        Some(Action::RackTab)
    );
    assert_eq!(RackHit::Add.action(), None);
    assert_eq!(
        BrowserHit::Search(BrowserMode::Sounds).action(),
        Some(Action::Search)
    );
    assert_eq!(
        BrowserHit::Search(BrowserMode::Settings).action(),
        Some(Action::Search)
    );
}
