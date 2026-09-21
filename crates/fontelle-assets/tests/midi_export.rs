//! `.mid` export (TDD §14.6): a project's note clips written back out as a
//! Standard MIDI File, the inverse of `import_midi`.
//!
//! What is asserted here is the contract the roll's *Export MIDI* relies on:
//! the file parses, its timing is the project's own tick grid, every note lands
//! on the tick it plays at, loops are unrolled the way the compiler unrolls
//! them, a note that names another channel goes to that channel's track, and a
//! slide — which starts no voice of its own — is not written as if it were an
//! ordinary note. The roundtrip test is the strongest of them: export, import,
//! and the notes come back the same.

use std::collections::BTreeMap;

use fontelle_assets::{MidiChannels, export_midi, export_project_to_midi, import_midi};
use fontelle_model::{Arena, Channel, Clip, ClipSource, Lane, Note, NoteData, Project, TempoMap};
use fontelle_types::{ChannelId, PPQN, Tick};
use midly::{MetaMessage, MidiMessage, Smf, Timing, TrackEventKind};

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

/// One decoded note: absolute tick on, absolute tick off, key, velocity, and
/// the MIDI channel it was written on.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
struct DecodedNote {
    on: u64,
    off: u64,
    key: u8,
    velocity: u8,
    channel: u8,
}

/// Every note in an exported SMF, matched on/off across the file, sorted.
fn decode_notes(bytes: &[u8]) -> Vec<DecodedNote> {
    let smf = Smf::parse(bytes).expect("exported bytes are a readable MIDI file");
    // (channel, key) -> (on tick, velocity), so an off finds its on.
    let mut pending: BTreeMap<(u8, u8), (u64, u8)> = BTreeMap::new();
    let mut notes = Vec::new();
    for track in &smf.tracks {
        let mut absolute: u64 = 0;
        for event in track {
            absolute += event.delta.as_int() as u64;
            if let TrackEventKind::Midi { channel, message } = event.kind {
                let ch = channel.as_int();
                match message {
                    MidiMessage::NoteOn { key, vel } if vel.as_int() > 0 => {
                        pending.insert((ch, key.as_int()), (absolute, vel.as_int()));
                    }
                    MidiMessage::NoteOff { key, .. } | MidiMessage::NoteOn { key, .. } => {
                        if let Some((on, velocity)) = pending.remove(&(ch, key.as_int())) {
                            notes.push(DecodedNote {
                                on,
                                off: absolute,
                                key: key.as_int(),
                                velocity,
                                channel: ch,
                            });
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    notes.sort();
    notes
}

/// Every SetTempo in the file, as (absolute tick, microseconds per quarter).
fn decode_tempos(bytes: &[u8]) -> Vec<(u64, u32)> {
    let smf = Smf::parse(bytes).expect("readable MIDI");
    let mut tempos = Vec::new();
    for track in &smf.tracks {
        let mut absolute: u64 = 0;
        for event in track {
            absolute += event.delta.as_int() as u64;
            if let TrackEventKind::Meta(MetaMessage::Tempo(us)) = event.kind {
                tempos.push((absolute, us.as_int()));
            }
        }
    }
    tempos
}

#[test]
fn an_exported_project_is_a_readable_smf_on_the_projects_own_grid() {
    let mut project = Project::new("song");
    project.tempo_map = TempoMap::new(120.0, SR);
    let ch = add_channel(&mut project, "lead");
    add_note_clip(
        &mut project,
        ch,
        0,
        PPQN * 4,
        None,
        vec![a_note(0, PPQN, 60, 100), a_note(PPQN, PPQN, 64, 80)],
    );

    let bytes = export_project_to_midi(&project);
    let smf = Smf::parse(&bytes).expect("parses");

    // A metrical file counting in the project's own PPQN, so no note timing has
    // to be rescaled on the way out or the way back in.
    assert_eq!(smf.header.timing, Timing::Metrical((PPQN as u16).into()));
}

#[test]
fn every_note_lands_on_the_tick_it_plays_at() {
    let mut project = Project::new("song");
    project.tempo_map = TempoMap::new(120.0, SR);
    let ch = add_channel(&mut project, "lead");
    add_note_clip(
        &mut project,
        ch,
        PPQN * 2, // the clip starts on bar two's beat one
        PPQN * 4,
        None,
        vec![a_note(0, PPQN, 60, 100), a_note(PPQN, PPQN / 2, 64, 90)],
    );

    let notes = decode_notes(&export_project_to_midi(&project));
    assert_eq!(notes.len(), 2);
    // Clip start (PPQN*2) offsets every note.
    assert_eq!(notes[0].on, (PPQN * 2) as u64);
    assert_eq!(notes[0].off, (PPQN * 3) as u64);
    assert_eq!(notes[0].key, 60);
    assert_eq!(notes[0].velocity, 100);
    assert_eq!(notes[1].on, (PPQN * 3) as u64);
    assert_eq!(notes[1].off, (PPQN * 3 + PPQN / 2) as u64);
    assert_eq!(notes[1].key, 64);
}

#[test]
fn the_tempo_is_written() {
    let mut project = Project::new("song");
    project.tempo_map = TempoMap::new(140.0, SR);
    let ch = add_channel(&mut project, "lead");
    add_note_clip(
        &mut project,
        ch,
        0,
        PPQN,
        None,
        vec![a_note(0, PPQN, 60, 100)],
    );

    let tempos = decode_tempos(&export_project_to_midi(&project));
    assert_eq!(tempos.len(), 1);
    assert_eq!(tempos[0].0, 0);
    // 140 bpm is 60_000_000 / 140 microseconds per quarter.
    assert_eq!(tempos[0].1, (60_000_000.0f64 / 140.0).round() as u32);
}

#[test]
fn a_loop_is_unrolled_into_one_note_per_pass_and_cut_at_the_clip_end() {
    let mut project = Project::new("song");
    project.tempo_map = TempoMap::new(120.0, SR);
    let ch = add_channel(&mut project, "drums");
    // One bar of content, a four-bar clip, looping every bar: four passes.
    add_note_clip(
        &mut project,
        ch,
        0,
        PPQN * 16,
        Some(PPQN * 4),
        vec![a_note(0, PPQN, 36, 100)],
    );

    let notes = decode_notes(&export_project_to_midi(&project));
    let ons: Vec<u64> = notes.iter().map(|n| n.on).collect();
    assert_eq!(
        ons,
        vec![0, (PPQN * 4) as u64, (PPQN * 8) as u64, (PPQN * 12) as u64]
    );
}

#[test]
fn a_note_naming_another_channel_is_written_on_that_channels_track() {
    let mut project = Project::new("song");
    project.tempo_map = TempoMap::new(120.0, SR);
    let lead = add_channel(&mut project, "lead");
    let bass = add_channel(&mut project, "bass");
    let mut cross = a_note(0, PPQN, 40, 100);
    cross.channel = Some(bass);
    add_note_clip(
        &mut project,
        lead,
        0,
        PPQN * 4,
        None,
        vec![a_note(0, PPQN, 60, 100), cross],
    );

    let notes = decode_notes(&export_project_to_midi(&project));
    // Two notes, on two different MIDI channels.
    assert_eq!(notes.len(), 2);
    let channels: std::collections::BTreeSet<u8> = notes.iter().map(|n| n.channel).collect();
    assert_eq!(
        channels.len(),
        2,
        "the two channels write to two MIDI channels"
    );
}

#[test]
fn a_slide_note_is_not_written_as_an_ordinary_note() {
    let mut project = Project::new("song");
    project.tempo_map = TempoMap::new(120.0, SR);
    let ch = add_channel(&mut project, "bass");
    let mut slide = a_note(PPQN, PPQN, 48, 100);
    slide.slide = true;
    add_note_clip(
        &mut project,
        ch,
        0,
        PPQN * 4,
        None,
        vec![a_note(0, PPQN * 2, 45, 100), slide],
    );

    let notes = decode_notes(&export_project_to_midi(&project));
    // The one ordinary note is written; the slide, which sounds no voice of
    // its own, is not — writing it would double the note it bends.
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].key, 45);
}

#[test]
fn export_midi_writes_a_file_that_imports_back_to_the_same_notes() {
    let mut project = Project::new("roundtrip");
    project.tempo_map = TempoMap::new(120.0, SR);
    let ch = add_channel(&mut project, "lead");
    add_note_clip(
        &mut project,
        ch,
        0,
        PPQN * 4,
        None,
        vec![
            a_note(0, PPQN, 60, 100),
            a_note(PPQN, PPQN, 62, 90),
            a_note(PPQN * 2, PPQN * 2, 64, 110),
        ],
    );

    let dir = std::env::temp_dir().join(format!("fontelle-midi-export-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("roundtrip.mid");
    export_midi(&project, &path).expect("writes the file");
    assert!(path.exists());

    let back = import_midi(&path, MidiChannels::All).expect("imports the file we wrote");
    assert_eq!(back.channels.len(), 1);

    // The notes come back the same — the strongest statement export can make.
    let clip = back
        .project
        .clips
        .values()
        .next()
        .expect("one clip came back");
    let ClipSource::Notes(data) = &clip.source else {
        panic!("a note clip");
    };
    let mut got: Vec<(Tick, Tick, u8, u8)> = data
        .notes
        .values()
        .map(|n| (n.start + clip.start, n.length, n.key, n.velocity))
        .collect();
    got.sort();
    assert_eq!(
        got,
        vec![
            (0, PPQN, 60, 100),
            (PPQN, PPQN, 62, 90),
            (PPQN * 2, PPQN * 2, 64, 110),
        ]
    );

    let _ = std::fs::remove_dir_all(&dir);
}
