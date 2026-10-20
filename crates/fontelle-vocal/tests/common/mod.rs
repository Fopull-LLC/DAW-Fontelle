//! A small song with a vocal channel, for the phrase and render tests.

#![allow(dead_code)]

pub mod zip;

use fontelle_model::{
    Arena, Channel, Clip, ClipSource, Lane, Note, NoteData, Project, TempoMap, VocalVoice,
};
use fontelle_types::{ChannelId, ClipId, InstrumentKind, LaneId, NoteId, PPQN, Tick};

/// 120 bpm: a beat is half a second.
pub const BEAT: Tick = PPQN;

pub fn note(start: Tick, length: Tick, key: u8, lyric: &str) -> Note {
    Note {
        start,
        length,
        key,
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
        muted: false,
    }
}

pub fn channel(name: &str, voice: Option<VocalVoice>) -> Channel {
    Channel {
        preset: None,
        instrument: voice.as_ref().map(|_| InstrumentKind::Vocal),
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
        vocal: voice,
        vocal_style: Default::default(),
    }
}

pub struct Song {
    pub project: Project,
    pub voice: ChannelId,
    pub other: ChannelId,
    pub lane: LaneId,
    pub clip: ClipId,
    pub notes: Vec<NoteId>,
}

impl Song {
    /// Another copy of the song, ids and all.
    pub fn clone_song(&self) -> Song {
        Song {
            project: self.project.clone(),
            voice: self.voice,
            other: self.other,
            lane: self.lane,
            clip: self.clip,
            notes: self.notes.clone(),
        }
    }

    pub fn tempo(&self) -> TempoMap {
        fontelle_model::effective_tempo_map(&self.project)
    }

    pub fn notes_mut(&mut self) -> &mut Arena<NoteId, Note> {
        let ClipSource::Notes(data) = &mut self.project.clips[self.clip].source else {
            panic!("a note clip");
        };
        &mut data.notes
    }
}

/// A clip at the top of the song on the vocal channel holding `notes`.
pub fn song(language: &str, notes: Vec<Note>) -> Song {
    let mut project = Project::new("song");
    project.tempo_map = TempoMap::new(120.0, 48_000.0);
    let voice = project.channels.insert(channel(
        "Voice",
        Some(VocalVoice::new("Engine", "alto", language)),
    ));
    let other = project.channels.insert(channel("Piano", None));
    let lane = project.lanes.insert(Lane {
        name: "Lane".into(),
        height: 32.0,
        color: [0; 4],
        muted: false,
        locked: false,
        soloed: false,
        order: 0,
    });
    let mut arena = Arena::default();
    let ids = notes.into_iter().map(|n| arena.insert(n)).collect();
    let clip = project.clips.insert(Clip {
        name: None,
        lane,
        start: 0,
        length: BEAT * 256,
        source: ClipSource::Notes(NoteData {
            channel: voice,
            notes: arena,
            vocal: None,
        }),
        prefab_link: None,
        color: None,
        muted: false,
        loop_length: None,
    });
    Song {
        project,
        voice,
        other,
        lane,
        clip,
        notes: ids,
    }
}
