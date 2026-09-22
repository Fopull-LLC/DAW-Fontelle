//! The notepad's words in the document (`docs/effects-catalogue.md` §2.8).
//!
//! What a pad holds is not an `EffectConfig` — it is a list of strings, so it
//! sits beside the slot's settings the way a hosted plugin's state does
//! (`EffectSlot::notepad`). This is the command that writes it, which has the
//! same job every other command here has: put back exactly what was there.

use fontelle_model::{AddInsert, Command, EditNotepad, History, MixerTrack, Project, RemoveInsert};
use fontelle_types::{EffectKind, MixerTrackId, NotepadEdit, NotepadPages};

fn fixture() -> (Project, MixerTrackId) {
    let mut project = Project::new("notepad");
    let master = project.mixer.tracks.insert(MixerTrack::new("Master"));
    project.mixer.master = Some(master);
    let track = project.mixer.tracks.insert(MixerTrack::new("Vocal"));
    (project, track)
}

/// A track with a notepad in slot 0.
fn with_a_pad() -> (Project, MixerTrackId) {
    let (mut project, track) = fixture();
    AddInsert::new(track, EffectKind::Notepad)
        .apply(&mut project)
        .unwrap();
    (project, track)
}

fn pages(project: &Project, track: MixerTrackId) -> NotepadPages {
    project.mixer.tracks[track].inserts[0]
        .notepad
        .clone()
        .unwrap_or_default()
}

fn write(track: MixerTrackId, page: usize, text: &str) -> Box<dyn Command> {
    Box::new(EditNotepad::new(
        track,
        0,
        NotepadEdit::Write {
            page,
            text: text.to_string(),
        },
    ))
}

// -------------------------------------------------------------- the slot

#[test]
fn a_fresh_notepad_insert_has_one_empty_page() {
    let (project, track) = with_a_pad();
    let pad = pages(&project, track);
    assert_eq!(pad.len(), 1);
    assert_eq!(pad.showing_text(), "");
}

#[test]
fn an_effect_that_is_not_a_notepad_carries_no_pages() {
    // The field is `Option` and stays `None` on the other twenty, so a
    // project full of EQs is not a project full of empty notepads.
    let (mut project, track) = fixture();
    AddInsert::new(track, EffectKind::Reverb)
        .apply(&mut project)
        .unwrap();
    assert!(project.mixer.tracks[track].inserts[0].notepad.is_none());
}

// ----------------------------------------------------------- the command

#[test]
fn typing_is_undone_word_for_word() {
    let (mut project, track) = with_a_pad();
    let mut history = History::new();
    history
        .apply(write(track, 0, "when the lights go down"), &mut project)
        .unwrap();
    assert_eq!(
        pages(&project, track).showing_text(),
        "when the lights go down"
    );
    history.undo(&mut project).unwrap().unwrap();
    assert_eq!(pages(&project, track).showing_text(), "");
    history.redo(&mut project).unwrap().unwrap();
    assert_eq!(
        pages(&project, track).showing_text(),
        "when the lights go down"
    );
}

#[test]
fn a_run_of_typing_is_one_history_entry() {
    // A letter per undo is not an undo anybody can use. The window breaks the
    // gesture where a person would expect a stop — a caret moved, a page
    // turned, a new line — and until then the keystrokes coalesce, exactly
    // as a knob drag does.
    let (mut project, track) = with_a_pad();
    let mut history = History::new();
    for text in ["w", "wh", "whe", "when"] {
        history.apply(write(track, 0, text), &mut project).unwrap();
    }
    assert_eq!(history.depth(), 1, "four keystrokes, one entry");
    history.undo(&mut project).unwrap().unwrap();
    assert_eq!(
        pages(&project, track).showing_text(),
        "",
        "the undo goes back to before the run, not to 'whe'"
    );
}

#[test]
fn a_broken_gesture_starts_a_new_entry() {
    let (mut project, track) = with_a_pad();
    let mut history = History::new();
    history
        .apply(write(track, 0, "verse"), &mut project)
        .unwrap();
    history.break_gesture();
    history
        .apply(write(track, 0, "verse one"), &mut project)
        .unwrap();
    assert_eq!(history.depth(), 2);
    history.undo(&mut project).unwrap().unwrap();
    assert_eq!(pages(&project, track).showing_text(), "verse");
}

#[test]
fn typing_on_two_pages_is_two_entries_whatever_the_gesture() {
    // Coalescing is per page: a merge across pages would make one undo put
    // back two different pages' words.
    let (mut project, track) = with_a_pad();
    let mut history = History::new();
    history
        .apply(
            Box::new(EditNotepad::new(
                track,
                0,
                NotepadEdit::InsertPage {
                    at: 1,
                    text: String::new(),
                },
            )),
            &mut project,
        )
        .unwrap();
    history
        .apply(write(track, 1, "chorus"), &mut project)
        .unwrap();
    history
        .apply(write(track, 0, "verse"), &mut project)
        .unwrap();
    assert_eq!(history.depth(), 3);
}

#[test]
fn turning_pages_is_one_entry_however_far_you_walk() {
    let (mut project, track) = with_a_pad();
    let mut history = History::new();
    for at in 1..4 {
        EditNotepad::new(
            track,
            0,
            NotepadEdit::InsertPage {
                at,
                text: format!("page {at}"),
            },
        )
        .apply(&mut project)
        .unwrap();
    }
    history.break_gesture();
    for page in [2, 1, 0] {
        history
            .apply(
                Box::new(EditNotepad::new(track, 0, NotepadEdit::Show { page })),
                &mut project,
            )
            .unwrap();
    }
    assert_eq!(history.depth(), 1, "one entry for the walk");
    history.undo(&mut project).unwrap().unwrap();
    assert_eq!(pages(&project, track).showing(), 3, "back where it started");
}

#[test]
fn a_page_removed_comes_back_with_its_words() {
    let (mut project, track) = with_a_pad();
    let mut history = History::new();
    EditNotepad::new(
        track,
        0,
        NotepadEdit::InsertPage {
            at: 1,
            text: "the bridge".to_string(),
        },
    )
    .apply(&mut project)
    .unwrap();
    history
        .apply(
            Box::new(EditNotepad::new(
                track,
                0,
                NotepadEdit::RemovePage { page: 1 },
            )),
            &mut project,
        )
        .unwrap();
    assert_eq!(pages(&project, track).len(), 1);
    history.undo(&mut project).unwrap().unwrap();
    assert_eq!(pages(&project, track).text(1), "the bridge");
}

#[test]
fn an_edit_that_changes_nothing_is_refused_rather_than_recorded() {
    // The same page's same words, and the last page taken away: neither is a
    // history entry, or Ctrl+Z would walk back through edits that never
    // happened.
    let (mut project, track) = with_a_pad();
    let mut history = History::new();
    assert!(history.apply(write(track, 0, ""), &mut project).is_err());
    assert!(
        history
            .apply(
                Box::new(EditNotepad::new(
                    track,
                    0,
                    NotepadEdit::RemovePage { page: 0 }
                )),
                &mut project,
            )
            .is_err()
    );
    assert_eq!(history.depth(), 0, "a refused edit is not an entry");
}

#[test]
fn an_edit_aimed_at_something_that_is_not_a_notepad_is_refused() {
    let (mut project, track) = fixture();
    AddInsert::new(track, EffectKind::Delay)
        .apply(&mut project)
        .unwrap();
    let mut history = History::new();
    assert!(
        history
            .apply(write(track, 0, "hello"), &mut project)
            .is_err()
    );
    assert!(project.mixer.tracks[track].inserts[0].notepad.is_none());
    // And at a slot that is not there at all.
    assert!(
        history
            .apply(
                Box::new(EditNotepad::new(track, 9, NotepadEdit::Show { page: 0 })),
                &mut project,
            )
            .is_err()
    );
}

#[test]
fn a_notepad_taken_off_the_chain_comes_back_with_what_it_said() {
    // `RemoveInsert` keeps the whole slot for the undo, and the words are
    // part of the slot: an undo that put back an empty pad would be a
    // command that lost a page of lyrics while claiming not to.
    let (mut project, track) = with_a_pad();
    let mut history = History::new();
    write(track, 0, "do not lose me")
        .apply(&mut project)
        .unwrap();
    history
        .apply(Box::new(RemoveInsert::new(track, 0)), &mut project)
        .unwrap();
    assert!(project.mixer.tracks[track].inserts.is_empty());
    history.undo(&mut project).unwrap().unwrap();
    assert_eq!(pages(&project, track).showing_text(), "do not lose me");
}

#[test]
fn the_entry_says_what_it_did() {
    let (_, track) = fixture();
    assert_eq!(
        EditNotepad::new(track, 0, NotepadEdit::Show { page: 1 }).label(),
        "Turn page"
    );
    assert_eq!(write(track, 0, "x").label(), "Type");
}

// -------------------------------------------------------------- the file

#[test]
fn a_pads_words_survive_a_save_and_an_open() {
    // The guard on the field's serde: `storage.rs` round-trips every effect's
    // *config*, and a notepad's content is not in its config.
    let dir = std::env::temp_dir().join(format!("fontelle-notepad-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (mut project, track) = with_a_pad();
    write(track, 0, "verse one\nverse two")
        .apply(&mut project)
        .unwrap();
    EditNotepad::new(
        track,
        0,
        NotepadEdit::InsertPage {
            at: 1,
            text: "chorus".to_string(),
        },
    )
    .apply(&mut project)
    .unwrap();
    fontelle_model::save_project(&project, &dir).expect("save");
    let back = fontelle_model::load_project(&dir).expect("open");
    let pad = back.mixer.tracks[track].inserts[0]
        .notepad
        .clone()
        .expect("the pad came back");
    assert_eq!(pad.text(0), "verse one\nverse two");
    assert_eq!(pad.text(1), "chorus");
    assert_eq!(pad.showing(), 1, "and on the page you left it on");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_project_with_no_notepad_says_nothing_about_one() {
    // The field is skipped when empty, so every project written before the
    // notepad existed is written back byte for byte.
    let (mut project, track) = fixture();
    AddInsert::new(track, EffectKind::Eq)
        .apply(&mut project)
        .unwrap();
    let json = serde_json::to_string(&project.mixer.tracks[track].inserts[0]).unwrap();
    assert!(!json.contains("notepad"), "{json}");
}
