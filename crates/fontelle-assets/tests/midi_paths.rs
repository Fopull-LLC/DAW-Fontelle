//! A note's **path** written to a `.mid` (`docs/note-paths-plan.md` §6) — as
//! MPE, because MIDI has no per-note pitch any other way.
//!
//! A part with a sliding note in it goes out as an MPE lower zone: the
//! configuration message on channel 1, and every note on a member channel
//! (2–16) of its own, with its slides written as that channel's pitch bend
//! at MPE's default range of 48 semitones. Bitwig, Ableton, Reaper and Logic
//! all read that as per-note pitch. A part with no slides is written exactly
//! as before.

use std::collections::BTreeMap;

use fontelle_assets::{MidiChannels, export_project_to_midi, read_midi};
use fontelle_model::{
    Arena, Channel, Clip, ClipSource, Lane, Note, NoteData, PathPoint, Project, TempoMap,
};
use fontelle_types::{ChannelId, PPQN, Tick};
use midly::{MidiMessage, Smf, TrackEventKind};

const SR: f64 = 48_000.0;

fn a_note(start: Tick, length: Tick, key: u8, velocity: u8) -> Note {
    Note {
        start,
        length,
        key,
        velocity,
        pan: 0,
        fine_pitch: 0,
        release: 0,
        mod_x: 0,
        mod_y: 0,
        slide: false,
        path: Vec::new(),
        channel: None,
    }
}

fn add_channel(project: &mut Project, name: &str) -> ChannelId {
    project.channels.insert(Channel {
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
    })
}

fn add_note_clip(
    project: &mut Project,
    home: ChannelId,
    start: Tick,
    length: Tick,
    loop_length: Option<Tick>,
    notes: Vec<Note>,
) {
    let lane = project.lanes.insert(Lane {
        name: "lane".into(),
        height: 32.0,
        color: [0; 4],
        muted: false,
        locked: false,
        soloed: false,
        order: 0,
    });
    let mut arena = Arena::default();
    for note in notes {
        arena.insert(note);
    }
    project.clips.insert(Clip {
        lane,
        start,
        length,
        source: ClipSource::Notes(NoteData {
            channel: home,
            notes: arena,
        }),
        prefab_link: None,
        color: None,
        muted: false,
        loop_length,
    });
}

fn with_path(mut note: Note, points: &[(Tick, i8)]) -> Note {
    note.path = points
        .iter()
        .map(|&(at, offset)| PathPoint { at, offset })
        .collect();
    note
}

fn song(notes: Vec<Note>) -> Vec<u8> {
    let mut project = Project::new("song");
    project.tempo_map = TempoMap::new(120.0, SR);
    let ch = add_channel(&mut project, "lead");
    add_note_clip(&mut project, ch, 0, PPQN * 16, None, notes);
    export_project_to_midi(&project)
}

/// Everything on each MIDI channel, as `(tick, message)`, in file order.
fn by_channel(bytes: &[u8]) -> BTreeMap<u8, Vec<(u64, MidiMessage)>> {
    let smf = Smf::parse(bytes).expect("a readable MIDI file");
    let mut out: BTreeMap<u8, Vec<(u64, MidiMessage)>> = BTreeMap::new();
    for track in &smf.tracks {
        let mut absolute = 0u64;
        for event in track {
            absolute += u64::from(event.delta.as_int());
            if let TrackEventKind::Midi { channel, message } = event.kind {
                out.entry(channel.as_int())
                    .or_default()
                    .push((absolute, message));
            }
        }
    }
    out
}

/// The bends on a channel, as semitones at MPE's ±48.
fn bends(events: &[(u64, MidiMessage)]) -> Vec<(u64, f32)> {
    events
        .iter()
        .filter_map(|(tick, message)| match message {
            MidiMessage::PitchBend { bend } => {
                Some((*tick, f32::from(bend.as_int()) / 8192.0 * 48.0))
            }
            _ => None,
        })
        .collect()
}

fn note_ons(events: &[(u64, MidiMessage)]) -> Vec<(u64, u8)> {
    events
        .iter()
        .filter_map(|(tick, message)| match message {
            MidiMessage::NoteOn { key, vel } if vel.as_int() > 0 => Some((*tick, key.as_int())),
            _ => None,
        })
        .collect()
}

#[test]
fn a_part_with_a_slide_is_written_as_an_mpe_zone() {
    let bytes = song(vec![with_path(
        a_note(0, PPQN * 4, 60, 100),
        &[(PPQN, 0), (PPQN * 2, 7)],
    )]);
    let channels = by_channel(&bytes);
    // The zone's configuration on the manager channel: RPN 6, fifteen
    // member channels.
    let manager = channels.get(&0).expect("the manager channel says the zone");
    let controllers: Vec<(u8, u8)> = manager
        .iter()
        .filter_map(|(_, m)| match m {
            MidiMessage::Controller { controller, value } => {
                Some((controller.as_int(), value.as_int()))
            }
            _ => None,
        })
        .collect();
    assert!(
        controllers
            .windows(3)
            .any(|w| w == [(101, 0), (100, 6), (6, 15)]),
        "RPN 6 = 15 members: {controllers:?}"
    );
    assert!(
        note_ons(manager).is_empty(),
        "no note on the manager channel"
    );

    // The note on a member channel of its own, starting unbent and landing
    // a fifth up exactly when its slide does.
    let (member, events) = channels
        .iter()
        .find(|(_, e)| !note_ons(e).is_empty())
        .expect("the note");
    assert!((1..=15).contains(member));
    assert_eq!(note_ons(events), vec![(0, 60)]);
    let bent = bends(events);
    assert!(
        bent.first().is_some_and(|&(t, s)| t == 0 && s == 0.0),
        "{bent:?}"
    );
    let beat = PPQN as u64;
    let landed = bent
        .iter()
        .find(|&&(t, _)| t == beat * 2)
        .expect("a bend where the slide lands");
    assert!((landed.1 - 7.0).abs() < 0.01, "{landed:?}");
    // The slide is a ramp, not a jump: bends in between, rising.
    let during: Vec<f32> = bent
        .iter()
        .filter(|&&(t, _)| t > beat && t < beat * 2)
        .map(|&(_, s)| s)
        .collect();
    assert!(during.len() >= 4, "{bent:?}");
    assert!(during.windows(2).all(|w| w[1] >= w[0]));
    // And nothing bends during the hold before it.
    assert!(bent.iter().all(|&(t, s)| t > beat || s == 0.0));
}

#[test]
fn a_chord_sliding_apart_takes_a_channel_a_note() {
    let bytes = song(vec![
        with_path(a_note(0, PPQN * 2, 60, 100), &[(PPQN, 5)]),
        with_path(a_note(0, PPQN * 2, 64, 100), &[(PPQN, -2)]),
    ]);
    let channels = by_channel(&bytes);
    let mut landed: Vec<(u8, f32)> = channels
        .iter()
        .filter(|(_, e)| !note_ons(e).is_empty())
        .map(|(_, e)| {
            (
                note_ons(e)[0].1,
                bends(e).last().map(|&(_, s)| s).unwrap_or(0.0),
            )
        })
        .collect();
    landed.sort_by_key(|(key, _)| *key);
    assert_eq!(landed.len(), 2, "two channels, one note each");
    assert_eq!(landed[0].0, 60);
    assert!((landed[0].1 - 5.0).abs() < 0.01);
    assert_eq!(landed[1].0, 64);
    assert!((landed[1].1 + 2.0).abs() < 0.01);
}

#[test]
fn a_part_with_no_slides_is_written_as_it_always_was() {
    let bytes = song(vec![a_note(0, PPQN, 60, 100), a_note(PPQN, PPQN, 64, 100)]);
    let channels = by_channel(&bytes);
    assert_eq!(channels.keys().copied().collect::<Vec<_>>(), vec![0]);
    assert!(bends(&channels[&0]).is_empty());
    assert!(
        channels[&0]
            .iter()
            .all(|(_, m)| !matches!(m, MidiMessage::Controller { .. }))
    );
}

/// What comes back when a song is read in, as `(start, key, path)`, sorted.
fn read_back(bytes: &[u8]) -> (usize, Vec<(Tick, u8, Vec<PathPoint>)>) {
    let back = read_midi(bytes, "song", MidiChannels::All).expect("reads the file we wrote");
    let mut notes: Vec<(Tick, u8, Vec<PathPoint>)> = back
        .project
        .clips
        .values()
        .filter_map(|clip| match &clip.source {
            ClipSource::Notes(data) => Some((clip.start, data)),
            _ => None,
        })
        .flat_map(|(at, data)| {
            data.notes
                .values()
                .map(move |n| (n.start + at, n.key, n.path.clone()))
        })
        .collect();
    notes.sort_by_key(|(start, key, _)| (*start, *key));
    (back.channels.len(), notes)
}

/// **An MPE zone reads back as one part with paths**, not fifteen parts of
/// one note each: the member channels are the zone's, and each note's bend
/// curve is its path again — the slides a file from Bitwig or from this
/// program's own export carries.
#[test]
fn an_mpe_zone_reads_back_as_one_part_whose_notes_slide() {
    let melody = with_path(
        a_note(0, PPQN * 6, 60, 100),
        &[(PPQN, 0), (PPQN * 2, 7), (PPQN * 3, 7), (PPQN * 4, 3)],
    );
    let chord = vec![
        with_path(a_note(PPQN * 8, PPQN * 2, 60, 100), &[(PPQN, 5)]),
        with_path(a_note(PPQN * 8, PPQN * 2, 64, 100), &[(PPQN, -2)]),
    ];
    let plain = a_note(PPQN * 12, PPQN, 67, 100);
    let mut all = vec![melody.clone(), plain.clone()];
    all.extend(chord.clone());
    let (parts, notes) = read_back(&song(all));

    assert_eq!(parts, 1, "one part, not one per member channel");
    assert_eq!(notes.len(), 4);
    assert_eq!(notes[0], (0, 60, melody.path.clone()), "the melody");
    assert_eq!(notes[1], (PPQN * 8, 60, chord[0].path.clone()));
    assert_eq!(notes[2], (PPQN * 8, 64, chord[1].path.clone()));
    assert_eq!(
        notes[3],
        (PPQN * 12, 67, Vec::new()),
        "a plain note stays plain"
    );
}
