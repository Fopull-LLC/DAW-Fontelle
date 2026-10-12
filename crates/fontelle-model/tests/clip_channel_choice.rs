//! Which instrument a click on a clip means — read off the notes that are
//! actually in it, not the channel the clip was drawn with.
//!
//! Ty, using the window:
//!
//! > *"i also dont like that clips being made are tied to the instrument you
//! > had selected when you made it. so if you end up deciding you dont like
//! > the instrument you initially started with and play a different
//! > instrument in that same clip and remove the first one, it will play the
//! > instrument right but clicking on that clip to select it does not select
//! > the instrument thats actually playing in the channel rack instead it
//! > selects the original instrument that was tied to that clip. this is bad
//! > ux. please make it work more like the user would expect where it picks
//! > either the most relevant instrument (or just a different one than the
//! > one thats currently selected, maybe cycle through?)"*
//!
//! A clip's **home** channel (`NoteData::channel`) stays what it is in the
//! file — the wire and the file are append-only, and a note with no channel
//! of its own still plays it. What changes is that everything a person
//! *sees* of it is derived at read time from the notes that play:
//! [`ClipChannels`] is that reading, pure, so the rules are pinned here and
//! the session only asks.

use fontelle_model::{Arena, ClipChannels, Note, NoteData};
use fontelle_types::{ChannelId, PPQN};

fn ids(n: usize) -> Vec<ChannelId> {
    let mut arena: Arena<ChannelId, ()> = Arena::default();
    (0..n).map(|_| arena.insert(())).collect()
}

fn a_note(at: i64, channel: Option<ChannelId>) -> Note {
    Note {
        start: at * PPQN,
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
        channel,
    }
}

/// A clip whose home is `home`, holding `count` notes on each channel named.
fn clip(home: ChannelId, notes: &[(Option<ChannelId>, usize)]) -> NoteData {
    let mut data = NoteData {
        channel: home,
        notes: Arena::default(),
    };
    let mut at = 0;
    for (channel, count) in notes {
        for _ in 0..*count {
            data.notes.insert(a_note(at, *channel));
            at += 1;
        }
    }
    data
}

// --------------------------------------------------------- empty clips ---

#[test]
fn an_empty_clip_answers_with_the_channel_it_was_drawn_with() {
    // Nothing plays in it yet, so the only thing it can mean is what it was
    // made for — the next note drawn into it lands there.
    let ch = ids(2);
    let data = clip(ch[0], &[]);
    let pick = ClipChannels::of(&data, &ch);
    assert!(pick.playing().is_empty());
    assert_eq!(pick.face(), Some(ch[0]));
    assert_eq!(pick.on_open(Some(ch[1])), Some(ch[0]));
    assert_eq!(pick.cycled(Some(ch[0])), Some(ch[0]));
    assert_eq!(pick.others(), 0);
}

#[test]
fn an_empty_clip_whose_channel_has_gone_selects_nothing() {
    // A place following a prefab whose source names a channel that was
    // deleted: never select a missing channel.
    let ch = ids(3);
    let data = clip(ch[2], &[]);
    let rack = [ch[0], ch[1]];
    let pick = ClipChannels::of(&data, &rack);
    assert_eq!(pick.face(), None);
    assert_eq!(
        pick.on_open(Some(ch[0])),
        None,
        "None leaves the rack alone"
    );
    assert_eq!(pick.cycled(Some(ch[0])), None);
}

// ------------------------------------------------- Ty's case exactly ---

#[test]
fn a_clip_drawn_with_one_instrument_and_played_by_another_means_the_other() {
    // Drawn with A; every note in it is B's now.
    let ch = ids(2);
    let (a, b) = (ch[0], ch[1]);
    let data = clip(a, &[(Some(b), 4)]);
    let pick = ClipChannels::of(&data, &ch);
    assert_eq!(pick.playing(), &[(b, 4)]);
    assert_eq!(pick.face(), Some(b), "captioned and coloured as B");
    assert_eq!(pick.on_open(Some(a)), Some(b), "a click selects B, not A");
    assert_eq!(pick.on_open(None), Some(b));
    assert_eq!(
        pick.cycled(Some(b)),
        Some(b),
        "one instrument: nothing to cycle"
    );
    assert_eq!(pick.others(), 0, "no +1: A plays nothing here");
}

#[test]
fn a_note_naming_a_deleted_channel_is_never_chosen() {
    let ch = ids(3);
    let data = clip(ch[0], &[(Some(ch[2]), 9), (None, 1)]);
    let rack = [ch[0], ch[1]];
    let pick = ClipChannels::of(&data, &rack);
    assert_eq!(pick.playing(), &[(ch[0], 1)]);
    assert_eq!(pick.on_open(Some(ch[1])), Some(ch[0]));
}

// ---------------------------------------------------- several channels ---

#[test]
fn with_several_the_one_with_the_most_notes_is_the_most_relevant() {
    // Most notes, because it is the part the clip is *about*: a drum clip
    // with one stray bass note is a drum clip, and its block should say so.
    let ch = ids(3);
    let data = clip(ch[0], &[(None, 1), (Some(ch[1]), 5)]);
    let pick = ClipChannels::of(&data, &ch);
    assert_eq!(pick.playing(), &[(ch[0], 1), (ch[1], 5)], "rack order");
    assert_eq!(pick.face(), Some(ch[1]));
    assert_eq!(pick.others(), 1);
    // The rack on a channel that plays nothing here: the most relevant.
    assert_eq!(pick.on_open(Some(ch[2])), Some(ch[1]));
}

#[test]
fn with_several_a_selection_that_already_plays_there_is_kept() {
    // You were writing the bass in this clip; clicking it again to pick it
    // up must not throw you onto the drums.
    let ch = ids(2);
    let data = clip(ch[0], &[(None, 1), (Some(ch[1]), 5)]);
    let pick = ClipChannels::of(&data, &ch);
    assert_eq!(pick.on_open(Some(ch[0])), Some(ch[0]));
}

#[test]
fn a_tie_goes_to_the_channel_higher_in_the_rack() {
    // Rack order, not the order the notes were written in — the rack is
    // what the person is looking at.
    let ch = ids(3);
    let data = clip(ch[2], &[(None, 3), (Some(ch[1]), 3)]);
    let pick = ClipChannels::of(&data, &ch);
    assert_eq!(pick.playing(), &[(ch[1], 3), (ch[2], 3)]);
    assert_eq!(pick.face(), Some(ch[1]));
}

#[test]
fn clicking_again_cycles_through_the_channels_in_rack_order() {
    let ch = ids(4);
    let data = clip(ch[3], &[(Some(ch[0]), 1), (Some(ch[2]), 2), (None, 1)]);
    let pick = ClipChannels::of(&data, &ch);
    assert_eq!(pick.cycled(Some(ch[0])), Some(ch[2]));
    assert_eq!(pick.cycled(Some(ch[2])), Some(ch[3]));
    assert_eq!(pick.cycled(Some(ch[3])), Some(ch[0]), "and round again");
    // From a channel that plays nothing here, or none, the cycle starts at
    // the most relevant.
    assert_eq!(pick.cycled(Some(ch[1])), Some(ch[2]));
    assert_eq!(pick.cycled(None), Some(ch[2]));
}
