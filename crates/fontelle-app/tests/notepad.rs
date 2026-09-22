//! The notepad, through the studio (`docs/effects-catalogue.md` §2.8).
//!
//! The document's half is `fontelle-model/tests/notepad.rs` and the window's
//! is `fontelle-ui/tests/notepad.rs`. This is the seam between them: what the
//! window is offered for a pad, what an edit from the window does to the
//! project, and the one thing about a notepad that outlives the project —
//! **the theme**, which is remembered as the next pad's default.

mod common;

use fontelle_app::{RealiseOptions, SampleLibrary, Session};
use fontelle_engine::{graph_channel, timeline_channel};
use fontelle_model::{AddMixerTrack, Command};
use fontelle_types::{CompiledTimeline, EffectKind, NotepadEdit, NotepadSize, NotepadTheme};
use fontelle_ui::document::{DocumentHost, StudioHost};

use common::SR;

fn studio() -> Session {
    let mut project = common::a_project_with_a_clip(8, 120.0, SR);
    AddMixerTrack::new("Vocal".to_string())
        .apply(&mut project)
        .expect("a mixer track must be addable");
    let clip = Session::first_clip(&project).expect("a blank project has one clip");
    let channel_nodes = fontelle_app::channel_nodes(&project);
    let (publisher, _timeline) = timeline_channel(CompiledTimeline::empty());
    let options = RealiseOptions {
        sample_rate: SR,
        block_size: fontelle_engine::BLOCK_SIZE,
        quality: fontelle_app::PLAYBACK_QUALITY,
    };
    let library = SampleLibrary::new();
    let realised =
        fontelle_app::realise(&project, &library, options).expect("a blank project must realise");
    let (graphs, _source) = graph_channel(realised.graph);
    Session::new(
        project,
        library,
        channel_nodes,
        publisher,
        options,
        clip,
        None,
    )
    .with_graphs(graphs, realised.track_controls)
    .with_param_nodes(realised.param_nodes)
}

/// A studio with a notepad on the first strip.
fn with_a_pad() -> (Session, usize, usize) {
    let mut session = studio();
    session.add_insert(0, EffectKind::Notepad);
    let slot = session.mixer_strips()[0].inserts.len() - 1;
    (session, 0, slot)
}

/// Somewhere for a settings file that is not the machine's own.
fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("fontelle-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    dir
}

// ---------------------------------------------------------------- the view

#[test]
fn a_notepad_insert_offers_a_page_and_no_knobs() {
    // The four effect windows are told apart by which of them offers a view,
    // and a pad offers the page.
    let (session, strip, slot) = with_a_pad();
    let view = session.notepad_view(strip, slot).expect("a pad");
    assert_eq!(view.track, "Vocal", "the strip it is an insert on");
    assert_eq!(view.pages, 1);
    assert_eq!(view.page, 0);
    assert_eq!(view.text, "");
    assert_eq!(view.theme, NotepadTheme::Phosphor);
    assert_eq!(view.size, NotepadSize::Medium);
    assert!(
        session.insert_view(strip, slot).is_none(),
        "a pad has no grid of knobs; it draws itself"
    );
    assert!(session.eq_config(strip, slot).is_none());
    assert!(session.tune_view(strip, slot).is_none());
}

#[test]
fn every_other_effect_offers_no_page() {
    let mut session = studio();
    for kind in EffectKind::ALL {
        if kind == EffectKind::Notepad {
            continue;
        }
        session.add_insert(0, kind);
        let slot = session.mixer_strips()[0].inserts.len() - 1;
        assert!(
            session.notepad_view(0, slot).is_none(),
            "{kind:?} offered a notepad page"
        );
    }
}

// --------------------------------------------------------------- the edits

#[test]
fn what_the_window_types_reaches_the_document_and_comes_back() {
    let (mut session, strip, slot) = with_a_pad();
    session.edit_notepad(
        strip,
        slot,
        NotepadEdit::Write {
            page: 0,
            text: "when the lights go down".to_string(),
        },
    );
    assert_eq!(
        session.notepad_view(strip, slot).unwrap().text,
        "when the lights go down"
    );
    // And it is the document's, so it undoes.
    session.undo();
    assert_eq!(session.notepad_view(strip, slot).unwrap().text, "");
}

#[test]
fn pages_are_added_turned_and_taken_away() {
    let (mut session, strip, slot) = with_a_pad();
    session.edit_notepad(
        strip,
        slot,
        NotepadEdit::InsertPage {
            at: 1,
            text: "chorus".to_string(),
        },
    );
    let view = session.notepad_view(strip, slot).unwrap();
    assert_eq!(
        (view.pages, view.page, view.text.as_str()),
        (2, 1, "chorus")
    );
    session.edit_notepad(strip, slot, NotepadEdit::Show { page: 0 });
    assert_eq!(session.notepad_view(strip, slot).unwrap().page, 0);
    session.edit_notepad(strip, slot, NotepadEdit::RemovePage { page: 1 });
    assert_eq!(session.notepad_view(strip, slot).unwrap().pages, 1);
}

#[test]
fn an_edit_aimed_at_an_insert_that_is_not_a_pad_does_nothing() {
    let mut session = studio();
    session.add_insert(0, EffectKind::Reverb);
    let slot = session.mixer_strips()[0].inserts.len() - 1;
    let before = session.undo_depth();
    session.edit_notepad(
        0,
        slot,
        NotepadEdit::Write {
            page: 0,
            text: "nowhere".to_string(),
        },
    );
    assert!(session.notepad_view(0, slot).is_none());
    // And nothing was written into the history for it.
    assert_eq!(
        session.undo_depth(),
        before,
        "a refused edit is not an entry"
    );
}

// -------------------------------------------------------------- the theme

#[test]
fn the_last_theme_chosen_is_what_the_next_pad_opens_in() {
    // > *"it should have themes you can switch between and it should alwasy
    // > save your default preferred theme as your last one selected."*
    //
    // A setting rather than document state, for the reason Flopsynth's window
    // scale is one: somebody who picked amber picked it for every session and
    // every project, not for the one pad they happened to be looking at.
    let dir = scratch("notepad-theme");
    let path = dir.join("settings.json");
    let mut session = {
        let (session, _, _) = with_a_pad();
        session.with_settings_path(path.clone())
    };
    let slot = session.mixer_strips()[0].inserts.len() - 1;

    // Stepping the chip is a parameter write like any other knob's.
    let amber = NotepadTheme::Amber.index();
    let spec = *fontelle_types::EffectConfig::new(EffectKind::Notepad)
        .specs()
        .iter()
        .find(|spec| spec.id == "theme")
        .unwrap();
    session.set_insert_param(0, slot, "theme", spec.normalise(amber as f32));
    assert_eq!(
        session.notepad_view(0, slot).unwrap().theme,
        NotepadTheme::Amber
    );

    let (saved, error) = fontelle_app::settings::Settings::load_from(&path);
    assert!(error.is_none());
    assert_eq!(
        saved.notepad_theme.as_deref(),
        Some(NotepadTheme::Amber.slug()),
        "kept for next time"
    );

    // A pad added now opens in it — the whole point of remembering.
    session.add_insert(0, EffectKind::Notepad);
    let second = session.mixer_strips()[0].inserts.len() - 1;
    assert_eq!(
        session.notepad_view(0, second).unwrap().theme,
        NotepadTheme::Amber
    );
    // And the first pad is untouched: a default is what a *new* one opens in.
    assert_eq!(
        session.notepad_view(0, slot).unwrap().theme,
        NotepadTheme::Amber
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_settings_file_naming_a_theme_that_is_gone_opens_in_the_first_one() {
    // The slug is a word rather than a position so that reordering the list
    // cannot change somebody's default — and a word this build does not know
    // is a file from a newer Fontelle, not a reason to refuse to open.
    let dir = scratch("notepad-unknown-theme");
    let path = dir.join("settings.json");
    let settings = fontelle_app::settings::Settings {
        notepad_theme: Some("chartreuse".to_string()),
        ..Default::default()
    };
    settings.save_to(&path).expect("write the settings");

    let (session, _, _) = with_a_pad();
    let mut session = session.with_settings_path(path);
    session.add_insert(0, EffectKind::Notepad);
    let slot = session.mixer_strips()[0].inserts.len() - 1;
    assert_eq!(
        session.notepad_view(0, slot).unwrap().theme,
        NotepadTheme::Phosphor
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_size_is_the_pads_own_rather_than_a_setting() {
    // Unlike the theme: how big the words are is about *this* page — a chorus
    // in a small window and a lyric sheet on a second screen are two pads —
    // so it is in the document and nowhere else.
    let (mut session, strip, slot) = with_a_pad();
    let spec = *fontelle_types::EffectConfig::new(EffectKind::Notepad)
        .specs()
        .iter()
        .find(|spec| spec.id == "size")
        .unwrap();
    session.set_insert_param(
        strip,
        slot,
        "size",
        spec.normalise(NotepadSize::Large.index() as f32),
    );
    assert_eq!(
        session.notepad_view(strip, slot).unwrap().size,
        NotepadSize::Large
    );
    session.add_insert(0, EffectKind::Notepad);
    let second = session.mixer_strips()[0].inserts.len() - 1;
    assert_eq!(
        session.notepad_view(0, second).unwrap().size,
        NotepadSize::Medium,
        "a new pad opens at the size every pad opens at"
    );
}
