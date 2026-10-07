//! The vocal track in the window: the singer picker's rows, the inline
//! lyric field's keys, and the shortcut that opens it.
//!
//! What a vocal producer does all day is type words under notes: select,
//! type, Tab, type, Tab — a line at a time — so the field's keys are the
//! feature, and they are tested here without a window.

use fontelle_model::{Arena, Note, VocalVoice};
use fontelle_types::{NoteId, PPQN};
use fontelle_ui::canvas::{
    Action, KEYBIND_SECTIONS, KeybindEntry, Keymap, LyricEditor, LyricKey, SingerRow,
    TextClipboard, line_text, singer_menu_rows,
};
use fontelle_ui::document::SingerChoice;
use winit::keyboard::{Key, NamedKey, SmolStr};

fn note(beat: i64, lyric: &str) -> Note {
    Note {
        start: beat * PPQN,
        length: PPQN,
        key: 60,
        velocity: 100,
        pan: 0,
        fine_pitch: 0,
        release: 0,
        mod_x: 0,
        mod_y: 0,
        slide: false,
        path: Vec::new(),
        channel: None,
        lyric: lyric.to_string(),
        voice: None,
    }
}

/// Three notes, inserted out of time order.
fn notes() -> (Arena<NoteId, Note>, [NoteId; 3]) {
    let mut arena = Arena::default();
    let c = arena.insert(note(2, "la"));
    let a = arena.insert(note(0, "hel-"));
    let b = arena.insert(note(1, "lo"));
    (arena, [a, b, c])
}

fn chars(editor: &mut LyricEditor, notes: &Arena<NoteId, Note>, text: &str) {
    let mut clip = TextClipboard::default();
    for c in text.chars() {
        let key = Key::Character(SmolStr::new(c.to_string()));
        editor.key(&key, false, false, &mut clip, notes);
    }
}

fn press(
    editor: &mut LyricEditor,
    notes: &Arena<NoteId, Note>,
    key: NamedKey,
    shift: bool,
) -> LyricKey {
    let mut clip = TextClipboard::default();
    editor.key(&Key::Named(key), false, shift, &mut clip, notes)
}

// ------------------------------------------------------------- the field

#[test]
fn a_notes_field_opens_on_its_word_all_selected_and_typing_replaces_it() {
    let (arena, [a, ..]) = notes();
    let mut editor = LyricEditor::open(&arena, &[a]).unwrap();
    assert_eq!(editor.entry().text(), "hel-");
    assert_eq!(editor.entry().selected_text(), "hel-");
    chars(&mut editor, &arena, "oh");
    assert_eq!(editor.entry().text(), "oh");
    assert_eq!(
        press(&mut editor, &arena, NamedKey::Enter, false),
        LyricKey::Commit {
            ids: vec![a],
            text: "oh".into(),
            line: false,
        }
    );
}

#[test]
fn tab_commits_and_moves_to_the_next_note_in_time_and_shift_tab_back() {
    let (arena, [a, b, c]) = notes();
    let mut editor = LyricEditor::open(&arena, &[a]).unwrap();
    chars(&mut editor, &arena, "a-");
    assert_eq!(
        press(&mut editor, &arena, NamedKey::Tab, false),
        LyricKey::CommitAndMove {
            ids: vec![a],
            text: "a-".into(),
            line: false,
            to: b,
        }
    );
    let editor = LyricEditor::open(&arena, &[b]).unwrap();
    let mut editor = editor;
    assert_eq!(
        press(&mut editor, &arena, NamedKey::Tab, true),
        LyricKey::CommitAndMove {
            ids: vec![b],
            text: "lo".into(),
            line: false,
            to: a,
        }
    );
    // Off the end: Tab on the last note commits and closes.
    let mut editor = LyricEditor::open(&arena, &[c]).unwrap();
    assert_eq!(
        press(&mut editor, &arena, NamedKey::Tab, false),
        LyricKey::Commit {
            ids: vec![c],
            text: "la".into(),
            line: false,
        }
    );
}

#[test]
fn escape_cancels_and_other_keys_are_the_fields() {
    let (arena, [a, ..]) = notes();
    let mut editor = LyricEditor::open(&arena, &[a]).unwrap();
    chars(&mut editor, &arena, "zzz");
    assert_eq!(
        press(&mut editor, &arena, NamedKey::Escape, false),
        LyricKey::Cancel
    );
    // A letter that is a tool's shortcut is a letter in a lyric.
    let mut editor = LyricEditor::open(&arena, &[a]).unwrap();
    let mut clip = TextClipboard::default();
    assert_eq!(
        editor.key(&Key::Character("d".into()), false, false, &mut clip, &arena),
        LyricKey::Edited
    );
}

#[test]
fn several_notes_open_as_one_line_and_commit_as_a_line() {
    let (arena, [a, b, c]) = notes();
    // Selected in any order: the line is in time order.
    let mut editor = LyricEditor::open(&arena, &[c, a, b]).unwrap();
    assert_eq!(editor.ids(), &[a, b, c]);
    assert_eq!(editor.entry().text(), "hel-lo la");
    chars(&mut editor, &arena, "a-ma-zing");
    assert_eq!(
        press(&mut editor, &arena, NamedKey::Enter, false),
        LyricKey::Commit {
            ids: vec![a, b, c],
            text: "a-ma-zing".into(),
            line: true,
        }
    );
}

#[test]
fn a_line_is_written_back_the_way_it_would_be_typed() {
    let (arena, [a, b, c]) = notes();
    assert_eq!(line_text(&arena, &[a, b, c]), "hel-lo la");
    let mut arena = arena;
    arena[c].lyric = "-".into();
    assert_eq!(line_text(&arena, &[a, b, c]), "hel-lo -");
}

// ------------------------------------------------------------ the picker

fn choice(id: &str, name: &str, languages: &[&str], research: bool) -> SingerChoice {
    SingerChoice {
        engine: "Engine".into(),
        id: id.into(),
        name: name.into(),
        languages: languages.iter().map(|l| l.to_string()).collect(),
        research_only: research,
    }
}

#[test]
fn the_picker_offers_every_singer_in_every_language_and_badges_research_voices() {
    let singers = vec![
        choice("alto", "Alto", &["en", "ja"], false),
        choice("lab", "Lab Voice", &["ja"], true),
    ];
    let current = VocalVoice::new("Engine", "alto", "en");
    let rows = singer_menu_rows(&singers, Some(&current), &[], "");
    let picks: Vec<(&str, &SingerRow)> = rows
        .iter()
        .filter(|(_, r)| matches!(r, SingerRow::Choose { .. }))
        .map(|(e, r)| (e.label.as_str(), r))
        .collect();
    assert_eq!(picks.len(), 3, "{rows:?}");
    assert!(picks[0].0.contains("Alto") && picks[0].0.contains("English"));
    assert!(picks[1].0.contains("Japanese"));
    // The one chosen says so and is not offered again.
    let chosen = rows
        .iter()
        .find(|(e, _)| e.label.contains("English"))
        .unwrap();
    assert!(!chosen.0.enabled);
    let lab = rows
        .iter()
        .find(|(e, _)| e.label.contains("Lab Voice"))
        .unwrap();
    assert!(
        lab.0.label.to_lowercase().contains("research only"),
        "{}",
        lab.0.label
    );
    assert!(lab.0.swatch.is_some(), "a badge beside the name");
    assert_eq!(
        lab.1,
        SingerRow::Choose {
            singer: 1,
            language: "ja".into()
        }
    );
}

#[test]
fn with_no_engine_the_picker_says_so_and_where_one_goes() {
    let rows = singer_menu_rows(&[], None, &[], "/home/ty/.local/share/fontelle/singers");
    let text: Vec<&str> = rows.iter().map(|(e, _)| e.label.as_str()).collect();
    assert!(
        text.iter()
            .any(|t| t.contains("No singer engine is installed")),
        "{text:?}"
    );
    assert!(
        text.iter()
            .any(|t| t.contains("/home/ty/.local/share/fontelle/singers"))
    );
    assert!(
        rows.iter()
            .all(|(_, r)| !matches!(r, SingerRow::Choose { .. }))
    );
}

#[test]
fn an_engine_that_did_not_load_is_named_in_the_picker() {
    let rows = singer_menu_rows(
        &[choice("alto", "Alto", &["en"], false)],
        None,
        &["/x/libold.so: needs a newer Fontelle".to_string()],
        "",
    );
    assert!(
        rows.iter()
            .any(|(e, _)| e.label.contains("libold.so") && !e.enabled)
    );
}

// ------------------------------------------------------------- the key

#[test]
fn editing_a_lyric_has_a_key_of_its_own_on_the_shortcuts_page() {
    let keymap = Keymap::default();
    let label = keymap.label(Action::EditLyric);
    assert_eq!(label, "Enter");
    assert!(Action::EditLyric.does().contains("lyric"));
    assert!(KEYBIND_SECTIONS.iter().any(|section| {
        section
            .binds
            .contains(&KeybindEntry::Action(Action::EditLyric))
    }));
}
