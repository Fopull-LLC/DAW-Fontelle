//! A note knows which instrument it plays, so one clip can hold several.
//!
//! Reported from using the window:
//!
//! > *"if i have a clip that has notes on multiple instruments, theres
//! > currently no way to actually swap between editing the different
//! > instruments notes youre just locked to editing the piano roll for the
//! > instrument the clips track is on. please fix this to make it much more
//! > how im envisioning where clips can have multiple instruments, we just
//! > base our interactions on what your currently selected instrument in the
//! > channel rack is."*
//!
//! TDD §10.4 said *the clip carries the instrument*, and that is still where
//! a clip's **home** channel lives — its caption, its colour, and what a note
//! with no channel of its own plays. What changed is that a note may name a
//! channel of its own, which is FL's pattern: one block on the arrangement,
//! a drum part and a bass part inside it, and the piano roll showing whichever
//! one the rack has selected.
//!
//! `None` rather than always naming a channel, so every note ever saved reads
//! back exactly as it was written — on the clip's channel — without a
//! migration and without a note carrying a copy of a value the clip already
//! holds.

use fontelle_model::{Arena, Note, NoteData};
use fontelle_types::{ChannelId, PPQN};

fn ids(n: usize) -> Vec<ChannelId> {
    let mut arena: Arena<ChannelId, ()> = Arena::default();
    (0..n).map(|_| arena.insert(())).collect()
}

fn a_note(key: u8, channel: Option<ChannelId>) -> Note {
    Note {
        start: 0,
        length: PPQN,
        key,
        velocity: 100,
        pan: 0,
        fine_pitch: 0,
        release: 0,
        mod_x: 0,
        mod_y: 0,
        slide: false,
        channel,
    }
}

#[test]
fn a_note_with_no_channel_of_its_own_plays_the_clips() {
    let ch = ids(2);
    let note = a_note(60, None);
    assert_eq!(note.channel_or(ch[0]), ch[0]);
    let other = a_note(60, Some(ch[1]));
    assert_eq!(other.channel_or(ch[0]), ch[1]);
}

#[test]
fn a_note_saved_before_notes_had_channels_reads_back_on_the_clips_channel() {
    // The field is new; every project on disk was written without it.
    let json = r#"{"start":0,"length":960,"key":60,"velocity":100,"pan":0,
                   "fine_pitch":0,"release":0,"mod_x":0,"mod_y":0,"slide":false}"#;
    let note: Note = serde_json::from_str(json).expect("an old note still reads");
    assert_eq!(note.channel, None);
}

#[test]
fn a_notes_channel_survives_a_save() {
    let ch = ids(1);
    let note = a_note(64, Some(ch[0]));
    let json = serde_json::to_string(&note).expect("serialisable");
    let back: Note = serde_json::from_str(&json).expect("readable");
    assert_eq!(back, note);
}

#[test]
fn a_clip_lists_the_channels_its_notes_are_on_with_its_own_first() {
    // What the arrangement captions a block with, and what the roll's ghost
    // filter steps through: the clip's own channel, then every other one
    // that has a note in the clip, each once, in the order they appear.
    let ch = ids(3);
    let mut notes = Arena::default();
    notes.insert(a_note(60, None));
    notes.insert(a_note(62, Some(ch[2])));
    notes.insert(a_note(64, Some(ch[0])));
    notes.insert(a_note(65, Some(ch[2])));
    notes.insert(a_note(67, Some(ch[1])));
    let data = NoteData {
        channel: ch[0],
        notes,
    };
    assert_eq!(data.channels(), vec![ch[0], ch[2], ch[1]]);
}

#[test]
fn a_clip_with_notes_on_one_channel_lists_just_that_one() {
    let ch = ids(2);
    let mut notes = Arena::default();
    notes.insert(a_note(60, None));
    notes.insert(a_note(62, Some(ch[0])));
    let data = NoteData {
        channel: ch[0],
        notes,
    };
    assert_eq!(data.channels(), vec![ch[0]]);
    // And an empty clip is still on its own channel.
    let empty = NoteData {
        channel: ch[1],
        notes: Arena::default(),
    };
    assert_eq!(empty.channels(), vec![ch[1]]);
}

#[test]
fn the_notes_on_one_channel_can_be_asked_for() {
    let ch = ids(2);
    let mut notes = Arena::default();
    let home = notes.insert(a_note(60, None));
    let explicit_home = notes.insert(a_note(62, Some(ch[0])));
    let other = notes.insert(a_note(64, Some(ch[1])));
    let data = NoteData {
        channel: ch[0],
        notes,
    };
    let mut on_home: Vec<_> = data.notes_on(ch[0]).map(|(id, _)| id).collect();
    on_home.sort();
    let mut wanted = vec![home, explicit_home];
    wanted.sort();
    assert_eq!(on_home, wanted);
    let on_other: Vec<_> = data.notes_on(ch[1]).map(|(id, _)| id).collect();
    assert_eq!(on_other, vec![other]);
}
