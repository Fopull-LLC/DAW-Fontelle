//! Deleting an instrument takes its **notes**, not the clips they share.
//!
//! `RemoveChannel` took every clip whose *home* was the channel — the
//! channel it was drawn with — whatever else was in it. So Ty's clip, drawn
//! with A and then written with B (*"play a different instrument in that
//! same clip and remove the first one"*), went with A, B's part and all.
//!
//! `DeleteChannel` is what the studio runs now: A's notes come out of every
//! clip and prefab; a clip goes only if nothing but A was in it (or it was
//! empty and drawn with A, as before); a clip that keeps other notes is
//! re-homed onto the instrument it is now about (`ClipChannels::face`), so
//! its caption, colour and a click on it follow what is left. Undo puts back
//! exactly what was there. `RemoveChannel` is untouched — it is on the wire
//! and in undo stacks already, and an older peer still sends it.

use fontelle_model::{
    Arena, Channel, Clip, ClipSource, Command, DeleteChannel, Lane, Note, NoteData, Project,
    RemoveChannel,
};
use fontelle_types::{ChannelId, ClipId, LaneId, PPQN};

fn snapshot(project: &Project) -> serde_json::Value {
    serde_json::to_value(project).expect("a project must serialise")
}

fn a_channel(name: &str) -> Channel {
    Channel {
        preset: None,
        instrument: None,
        name: name.into(),
        color: [0; 4],
        mixer_track: None,
        patch_data: None,
        plugin: None,
        pan: 0.0,
        muted: false,
        soloed: false,
        named_keys: false,
        ab: Default::default(),
        gain_db: 0.0,
        vocal: None,
        vocal_style: Default::default(),
    }
}

fn a_note(beat: i64, channel: Option<ChannelId>) -> Note {
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
        channel,
        lyric: String::new(),
        voice: None,
    }
}

struct Song {
    project: Project,
    lane: LaneId,
    a: ChannelId,
    b: ChannelId,
}

fn song() -> Song {
    let mut project = Project::new("delete");
    let a = project.channels.insert(a_channel("A"));
    let b = project.channels.insert(a_channel("B"));
    let lane = project.lanes.insert(Lane {
        name: "Lane".into(),
        height: 32.0,
        color: [0; 4],
        muted: false,
        locked: false,
        soloed: false,
        order: 0,
    });
    Song {
        project,
        lane,
        a,
        b,
    }
}

/// A clip whose home is `home`, holding a note per entry of `notes`.
fn clip(s: &mut Song, home: ChannelId, notes: &[Option<ChannelId>]) -> ClipId {
    let mut arena = Arena::default();
    for (beat, channel) in notes.iter().enumerate() {
        arena.insert(a_note(beat as i64, *channel));
    }
    s.project.clips.insert(Clip {
        name: None,
        lane: s.lane,
        start: 0,
        length: PPQN * 4,
        source: ClipSource::Notes(NoteData {
            channel: home,
            notes: arena,
            vocal: None,
        }),
        prefab_link: None,
        color: None,
        muted: false,
        loop_length: None,
    })
}

fn notes_of(project: &Project, clip: ClipId) -> (ChannelId, Vec<ChannelId>) {
    let source = project.clip_source(clip).expect("the clip");
    let ClipSource::Notes(data) = source.as_ref() else {
        panic!("a note clip")
    };
    (
        data.channel,
        data.notes
            .values()
            .map(|n| n.channel_or(data.channel))
            .collect(),
    )
}

#[test]
fn tys_clip_stays_plays_b_and_comes_back_whole() {
    let mut s = song();
    let (a, b) = (s.a, s.b);
    // Drawn with A, a note of A's, then B's part written into it.
    let id = clip(&mut s, a, &[None, Some(b), Some(b)]);
    let before = snapshot(&s.project);

    let mut delete = DeleteChannel::new(a);
    delete.apply(&mut s.project).unwrap();
    assert!(!s.project.channels.contains_key(a));
    assert!(
        s.project.clips.contains_key(id),
        "the clip with B's notes stays"
    );
    let (home, notes) = notes_of(&s.project, id);
    assert_eq!(notes, vec![b, b], "A's note is gone, B's two are there");
    assert_eq!(home, b, "and it is B's clip now");

    delete.invert().apply(&mut s.project).unwrap();
    assert_eq!(snapshot(&s.project), before, "undo puts back exactly");
}

#[test]
fn a_clip_of_nothing_but_the_channel_goes_with_it() {
    let mut s = song();
    let (a, b) = (s.a, s.b);
    let only_a = clip(&mut s, a, &[None, None]);
    let empty_a = clip(&mut s, a, &[]);
    let a_in_bs = clip(&mut s, b, &[Some(a)]);
    let empty_b = clip(&mut s, b, &[]);
    let before = snapshot(&s.project);

    let mut delete = DeleteChannel::new(a);
    delete.apply(&mut s.project).unwrap();
    assert!(!s.project.clips.contains_key(only_a));
    assert!(
        !s.project.clips.contains_key(empty_a),
        "empty and drawn with A, as before"
    );
    assert!(
        !s.project.clips.contains_key(a_in_bs),
        "nothing but A's notes in it, whatever it was drawn with"
    );
    assert!(
        s.project.clips.contains_key(empty_b),
        "nothing to do with A"
    );

    delete.invert().apply(&mut s.project).unwrap();
    assert_eq!(snapshot(&s.project), before);
}

#[test]
fn a_prefab_loses_the_channels_notes_and_its_places_stay() {
    let mut s = song();
    let (a, b) = (s.a, s.b);
    let mut notes = Arena::default();
    notes.insert(a_note(0, None));
    notes.insert(a_note(1, Some(b)));
    let mut add = fontelle_model::AddPrefab::new(
        "Riff",
        ClipSource::Notes(NoteData {
            channel: a,
            notes,
            vocal: None,
        }),
    );
    add.apply(&mut s.project).unwrap();
    let prefab = add.prefab().unwrap();
    let mut place = fontelle_model::AddPrefabInstance::new(prefab, s.lane, 0, PPQN * 4);
    place.apply(&mut s.project).unwrap();
    let place = place.clip().unwrap();
    let before = snapshot(&s.project);

    let mut delete = DeleteChannel::new(a);
    delete.apply(&mut s.project).unwrap();
    assert!(
        s.project.clips.contains_key(place),
        "B still plays through it"
    );
    let (home, notes) = notes_of(&s.project, place);
    assert_eq!((home, notes), (b, vec![b]));

    delete.invert().apply(&mut s.project).unwrap();
    assert_eq!(snapshot(&s.project), before);
}

#[test]
fn a_place_for_a_prefab_of_nothing_but_the_channel_goes() {
    let mut s = song();
    let a = s.a;
    let mut notes = Arena::default();
    notes.insert(a_note(0, None));
    let mut add = fontelle_model::AddPrefab::new(
        "Hit",
        ClipSource::Notes(NoteData {
            channel: a,
            notes,
            vocal: None,
        }),
    );
    add.apply(&mut s.project).unwrap();
    let mut place =
        fontelle_model::AddPrefabInstance::new(add.prefab().unwrap(), s.lane, 0, PPQN * 4);
    place.apply(&mut s.project).unwrap();
    let place = place.clip().unwrap();
    let before = snapshot(&s.project);

    let mut delete = DeleteChannel::new(a);
    delete.apply(&mut s.project).unwrap();
    assert!(!s.project.clips.contains_key(place));
    delete.invert().apply(&mut s.project).unwrap();
    assert_eq!(snapshot(&s.project), before);
}

#[test]
fn the_old_command_still_does_what_it_always_did() {
    // On the wire and in undo stacks already: an older peer's delete must
    // land here as it lands there.
    let mut s = song();
    let (a, b) = (s.a, s.b);
    let id = clip(&mut s, a, &[Some(b)]);
    RemoveChannel::new(a).apply(&mut s.project).unwrap();
    assert!(!s.project.clips.contains_key(id));
}

#[test]
fn it_crosses_the_wire_and_its_undo_does_too() {
    let mut s = song();
    let (a, b) = (s.a, s.b);
    clip(&mut s, a, &[None, Some(b)]);
    let before = snapshot(&s.project);
    let mut here = s.project.clone();
    let mut there = s.project.clone();

    let mut delete = DeleteChannel::new(a);
    delete.apply(&mut here).unwrap();
    let edit = fontelle_model::Edit::from_bytes(&delete.to_edit().to_bytes()).unwrap();
    assert_eq!(edit.tag(), "DeleteChannel");
    edit.into_command().apply(&mut there).unwrap();
    assert_eq!(snapshot(&there), snapshot(&here));

    let undo = delete.invert();
    let edit = fontelle_model::Edit::from_bytes(&undo.to_edit().to_bytes()).unwrap();
    edit.into_command().apply(&mut there).unwrap();
    assert_eq!(snapshot(&there), before);
}
