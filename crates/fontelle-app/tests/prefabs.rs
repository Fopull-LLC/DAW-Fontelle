//! Prefabs, through the studio.
//!
//! > *"the prefab tab should be where the channel rack is can be tabbed
//! > between instruments and prefabs which just shows a list of all of the ones
//! > you have and you can scroll down them all and a plus icon to make a new
//! > one and name it and stuff. editing a prefab clip basically works like just
//! > editing a normal clip except you dont have to only be selecting it in the
//! > arrangement a clip thats referencing the prefab you could also edit it
//! > just by selecting the prefab in the prefab menu and then selecting the
//! > instrument you want to edit in the prefab clip and then just editing the
//! > piano roll of it."* — Ty
//!
//! The model's half is `fontelle-model/tests/prefabs.rs` and the compiler's is
//! `fontelle-sequencer/tests/prefabs.rs`. What is here is the half that is
//! about **where you are looking**: which of the two ways into a prefab you
//! took, and the rule that both of them reach the same notes.

mod common;

use fontelle_types::{InstrumentKind, PPQN, Tick};
use fontelle_ui::canvas::{ArrangeEdit, RollEdit};
use fontelle_ui::document::{DocumentHost, StudioHost};

use common::SR;

fn a_session() -> fontelle_app::Session {
    common::a_session_for(common::a_project_with_a_clip(8, 120.0, SR))
}

fn a_note(start: Tick, key: u8) -> fontelle_model::Note {
    fontelle_model::Note {
        start,
        length: PPQN,
        key,
        velocity: 100,
        pan: 0,
        fine_pitch: 0,
        release: 0,
        mod_x: 0,
        mod_y: 0,
        slide: false,
        channel: None,
    }
}

/// The keys the roll is currently showing, in order.
fn showing(session: &fontelle_app::Session) -> Vec<u8> {
    let mut keys: Vec<(Tick, u8)> = session
        .notes()
        .values()
        .map(|note| (note.start, note.key))
        .collect();
    keys.sort_unstable();
    keys.into_iter().map(|(_, key)| key).collect()
}

// --------------------------------------------------------- the list ---

#[test]
fn a_new_project_has_no_prefabs_and_one_can_be_made_and_named() {
    let mut session = a_session();
    assert!(session.prefabs().is_empty(), "nothing until you make one");

    session.add_prefab();
    let rows = session.prefabs();
    assert_eq!(rows.len(), 1, "the plus icon makes one");
    assert!(!rows[0].name.is_empty(), "and it arrives with a name");
    assert_eq!(rows[0].uses, 0, "used nowhere yet");

    session.rename_prefab(0, "Chorus riff");
    assert_eq!(session.prefabs()[0].name, "Chorus riff");

    session.undo();
    assert_eq!(
        session.prefabs()[0].name,
        rows[0].name,
        "a rename is one press to take back"
    );
}

/// Made ones are named apart, so a list of them is a list you can read.
#[test]
fn two_new_prefabs_do_not_share_a_name() {
    let mut session = a_session();
    session.add_prefab();
    session.add_prefab();
    let rows = session.prefabs();
    assert_eq!(rows.len(), 2);
    assert_ne!(rows[0].name, rows[1].name);
}

#[test]
fn a_prefab_can_be_deleted_and_the_delete_taken_back() {
    let mut session = a_session();
    session.add_prefab();
    session.remove_prefab(0);
    assert!(session.prefabs().is_empty());
    session.undo();
    assert_eq!(session.prefabs().len(), 1);
}

// ------------------------------------------------- editing from the list ---

/// **The second way in.** Select the prefab, select the instrument, edit the
/// roll — with nothing selected on the arrangement at all.
#[test]
fn selecting_a_prefab_in_the_list_opens_it_in_the_roll() {
    let mut session = a_session();
    session.add_prefab();
    session.select_prefab(Some(0));
    assert_eq!(session.selected_prefab(), Some(0));

    session.edit(RollEdit::Add {
        note: a_note(0, 60),
    });
    session.end_gesture();
    assert_eq!(showing(&session), vec![60], "the roll is the prefab's");

    // And it went into the prefab rather than into whatever clip was open.
    session.select_prefab(None);
    assert_eq!(
        showing(&session),
        Vec::<u8>::new(),
        "the clip that was open is untouched"
    );
}

/// And **which instrument** you are writing follows the rack, exactly as it
/// does in an ordinary clip.
///
/// *"selecting the instrument you want to edit in the prefab clip"* — the same
/// rule as `multi-instrument-clips`: the rack's selection is what an edit
/// means, and the roll shows that instrument's notes.
#[test]
fn the_rack_says_which_instrument_a_prefab_edit_is_for() {
    let mut session = a_session();
    session.add_channel_of(InstrumentKind::Osc3).unwrap();
    assert_eq!(session.channels().len(), 2);

    session.add_prefab();
    session.select_prefab(Some(0));

    session.select_channel(0);
    session.edit(RollEdit::Add {
        note: a_note(0, 60),
    });
    session.end_gesture();

    session.select_channel(1);
    session.edit(RollEdit::Add {
        note: a_note(PPQN, 72),
    });
    session.end_gesture();

    assert_eq!(showing(&session), vec![72], "the second instrument's part");
    session.select_channel(0);
    assert_eq!(showing(&session), vec![60], "and the first's");
}

// ------------------------------------------ editing from the arrangement ---

/// **The first way in**, and the claim the feature exists for: one edit, every
/// place.
#[test]
fn an_edit_made_through_one_instance_shows_up_in_every_other() {
    let mut session = a_session();
    session.add_prefab();
    session.select_prefab(Some(0));

    let here = session.draw_prefab(0, 0, 0).expect("a place for it");
    let there = session.draw_prefab(0, 1, PPQN * 16).expect("and another");
    assert_eq!(session.prefabs()[0].uses, 2, "the list counts its places");

    // Off the list, and onto one of the two places on the arrangement.
    session.select_prefab(None);
    session.open_clip(here);
    session.edit(RollEdit::Add {
        note: a_note(0, 60),
    });
    session.end_gesture();
    assert_eq!(showing(&session), vec![60]);

    session.open_clip(there);
    assert_eq!(
        showing(&session),
        vec![60],
        "the other place shows the same edit"
    );
}

/// Drawing a place puts down a clip and **no notes of its own**.
#[test]
fn drawing_a_place_puts_a_clip_on_the_row_you_drew_on() {
    let mut session = a_session();
    session.add_prefab();
    let before = session.clips().len();
    let clip = session.draw_prefab(0, 1, PPQN * 4).expect("a place");

    let clips = session.clips();
    assert_eq!(clips.len(), before + 1);
    let placed = clips.iter().find(|c| c.id == clip).expect("it is listed");
    assert_eq!(placed.start, PPQN * 4);

    session.undo();
    assert_eq!(
        session.clips().len(),
        before,
        "and one press takes the place back"
    );
}

/// The arrangement says which blocks are places for a prefab, so they can be
/// drawn as such.
#[test]
fn the_arrangement_says_which_blocks_follow_a_prefab() {
    let mut session = a_session();
    session.add_prefab();
    session.rename_prefab(0, "Riff");
    let clip = session.draw_prefab(0, 1, 0).expect("a place");

    let clips = session.clips();
    let placed = clips.iter().find(|c| c.id == clip).expect("listed");
    assert_eq!(
        placed.prefab.as_deref(),
        Some("Riff"),
        "a place says whose it is"
    );
    assert_eq!(
        placed.name, "Riff",
        "and is captioned with it, not with the instrument"
    );
    let plain = clips.iter().find(|c| c.id != clip).expect("the other one");
    assert_eq!(plain.prefab, None, "an ordinary clip says nothing");
}

/// Taking one place off the prefab keeps what it was playing and stops it
/// following.
#[test]
fn a_place_can_be_freed_from_its_prefab() {
    let mut session = a_session();
    session.add_prefab();
    session.select_prefab(Some(0));
    session.edit(RollEdit::Add {
        note: a_note(0, 60),
    });
    session.end_gesture();
    session.select_prefab(None);

    let kept = session.draw_prefab(0, 0, 0).expect("a place");
    let freed = session.draw_prefab(0, 1, PPQN * 16).expect("another");
    session.detach_prefab(freed);

    session.open_clip(freed);
    assert_eq!(showing(&session), vec![60], "still playing what it was");

    // Now an edit to the prefab leaves it alone.
    session.select_prefab(Some(0));
    session.edit(RollEdit::Add {
        note: a_note(PPQN, 67),
    });
    session.end_gesture();
    session.select_prefab(None);

    session.open_clip(freed);
    assert_eq!(showing(&session), vec![60], "the freed one is its own");
    session.open_clip(kept);
    assert_eq!(showing(&session), vec![60, 67], "the other still follows");
}

// ------------------------------------------------------ hearing them ---

/// A place reaches the compiled timeline, which is the only claim that means
/// the feature works.
#[test]
fn a_place_reaches_the_timeline() {
    let mut session = a_session();
    session.add_prefab();
    session.select_prefab(Some(0));
    session.edit(RollEdit::Add {
        note: a_note(0, 60),
    });
    session.end_gesture();
    session.select_prefab(None);
    session.draw_prefab(0, 0, 0).expect("a place");

    let timeline = session.compiled();
    let ons = timeline
        .events
        .iter()
        .filter(|e| matches!(e.payload, fontelle_types::EventPayload::NoteOn { .. }))
        .count();
    assert_eq!(ons, 1, "the place plays the prefab's one note");
}

// ------------------------------------------ turning a clip into a prefab ---

/// The other way somebody makes one: they have written something they want
/// again.
#[test]
fn a_clip_that_has_been_written_can_become_a_prefab_in_place() {
    let mut session = a_session();
    let clip = session.clips()[0].id;
    session.open_clip(clip);
    session.edit(RollEdit::Add {
        note: a_note(0, 60),
    });
    session.end_gesture();

    session.make_prefab_from(clip).expect("a prefab from it");
    assert_eq!(session.prefabs().len(), 1);
    assert_eq!(session.prefabs()[0].uses, 1, "the clip it came from is one");

    // The clip goes on sounding exactly as it did, and now follows.
    session.open_clip(clip);
    assert_eq!(showing(&session), vec![60]);

    // And drawing another place gives two views of the one part.
    let other = session
        .draw_prefab(0, 1, PPQN * 16)
        .expect("a second place");
    session.open_clip(other);
    assert_eq!(showing(&session), vec![60]);

    session.open_clip(clip);
    session.edit(RollEdit::Add {
        note: a_note(PPQN, 64),
    });
    session.end_gesture();
    session.open_clip(other);
    assert_eq!(showing(&session), vec![60, 64], "one edit, both places");
}

// -------------------------------------------------------- the tab ---

/// The panel where the channel rack is has two tabs and remembers which one is
/// showing.
#[test]
fn the_rack_panel_tabs_between_instruments_and_prefabs() {
    use fontelle_ui::document::RackTab;
    let mut session = a_session();
    assert_eq!(
        session.rack_tab(),
        RackTab::Instruments,
        "it opens on the instruments, which is where it always was"
    );
    session.set_rack_tab(RackTab::Prefabs);
    assert_eq!(session.rack_tab(), RackTab::Prefabs);
}

/// Deleting a prefab that is drawn in places leaves the arrangement sounding
/// the same.
#[test]
fn deleting_a_prefab_does_not_empty_the_arrangement() {
    let mut session = a_session();
    session.add_prefab();
    session.select_prefab(Some(0));
    session.edit(RollEdit::Add {
        note: a_note(0, 60),
    });
    session.end_gesture();
    session.select_prefab(None);
    let place = session.draw_prefab(0, 0, 0).expect("a place");

    session.remove_prefab(0);
    session.open_clip(place);
    assert_eq!(
        showing(&session),
        vec![60],
        "the block keeps what it was playing"
    );
}

/// A place that is drawn and then deleted takes nothing else with it.
#[test]
fn deleting_a_place_leaves_the_prefab_and_its_other_places() {
    let mut session = a_session();
    session.add_prefab();
    let here = session.draw_prefab(0, 0, 0).expect("a place");
    let there = session.draw_prefab(0, 1, PPQN * 16).expect("another");

    session.arrange(ArrangeEdit::Remove(vec![here]));
    assert_eq!(session.prefabs().len(), 1, "the prefab is still there");
    assert_eq!(session.prefabs()[0].uses, 1, "with its other place");
    assert!(session.clips().iter().any(|c| c.id == there));
}
