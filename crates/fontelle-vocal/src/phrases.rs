//! A vocal part cut into phrases, and the keys their takes are cached by.

use std::hash::Hasher;

use fontelle_host::{PhraseSpec, SpecNote};
use fontelle_model::{ClipSource, NoteHome, Project, TempoMap};
use fontelle_types::{ChannelId, ClipId, NoteId, Tick};

/// A rest at least this long ends a phrase: long enough to breathe, so the
/// singer sings what comes after as a fresh start.
pub const MIN_REST_SECONDS: f64 = 0.2;
/// No phrase runs longer than this: an engine sings a phrase in one go, so a
/// verse with no rest in it would otherwise be one enormous render, and an
/// edit anywhere in it would sing all of it again.
pub const MAX_PHRASE_SECONDS: f64 = 12.0;

/// One note of a phrase, as the document has it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoteRef {
    pub clip: ClipId,
    /// Where an edit to it goes (a clip, or the prefab it follows).
    pub home: NoteHome,
    pub note: NoteId,
    /// Its start and end on the song, in ticks (a looped clip's later passes
    /// are later ticks of the same note).
    pub start: Tick,
    pub end: Tick,
}

/// A run of notes sung as one.
#[derive(Debug, Clone, PartialEq)]
pub struct Phrase {
    /// The song seconds its first note starts and its last ends.
    pub start: f64,
    pub end: f64,
    pub start_tick: Tick,
    pub end_tick: Tick,
    pub notes: Vec<NoteRef>,
    /// What the engine is asked, in phrase-relative seconds, with every
    /// empty lyric already the language's own syllable.
    pub spec: PhraseSpec,
}

/// What a take is cached under: a hash of the engine, its version, the
/// singer, the language, the quality and the phrase in its own time — and
/// the seed it is sung with, from the same things less the quality, so a
/// preview and its final are the same take.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RenderKey {
    pub hash: u64,
    pub seed: u64,
}

fn content_hash(engine: &str, version: &str, spec: &PhraseSpec) -> u64 {
    let mut h = twox_hash::XxHash64::with_seed(0x766f_6361_6c00_0001);
    let text = |h: &mut twox_hash::XxHash64, s: &str| {
        h.write_u64(s.len() as u64);
        h.write(s.as_bytes());
    };
    text(&mut h, engine);
    text(&mut h, version);
    text(&mut h, &spec.singer);
    text(&mut h, &spec.language);
    h.write_u64(spec.notes.len() as u64);
    // Times to the microsecond: the same phrase moved by a tempo-exact
    // amount must hash the same despite the last bits of a float.
    let micros = |s: f64| (s * 1e6).round() as i64;
    for n in &spec.notes {
        h.write_i64(micros(n.start));
        h.write_i64(micros(n.duration));
        h.write_i32((n.pitch * 100.0).round() as i32);
        text(&mut h, &n.lyric);
    }
    h.finish()
}

/// The key a take of `spec` at `quality` from engine `engine` `version` is
/// cached under. See [`RenderKey`].
pub fn render_key(engine: &str, version: &str, spec: &PhraseSpec, quality: u32) -> RenderKey {
    let content = content_hash(engine, version, spec);
    let mut h = twox_hash::XxHash64::with_seed(content);
    h.write_u32(quality);
    let mut s = twox_hash::XxHash64::with_seed(0x5eed);
    s.write_u64(content);
    RenderKey {
        hash: h.finish(),
        seed: s.finish(),
    }
}

/// Every phrase `channel` sings in `project`, in time order.
///
/// From exactly the notes the song would play on that channel — the
/// sequencer's own rules: a muted clip, row or channel sings nothing, a
/// solo silences the rest, a clip's end cuts its notes and a looped clip
/// sings every pass. Slide notes start no syllable and are left out. Notes
/// that overlap (a vocal line is one voice) are cut at the next one's start.
pub fn phrases_of(project: &Project, channel: ChannelId, tempo: &TempoMap) -> Vec<Phrase> {
    let Some(this) = project.channels.get(channel) else {
        return Vec::new();
    };
    let any_channel_soloed = project.channels.values().any(|c| c.soloed);
    if this.muted || (any_channel_soloed && !this.soloed) {
        return Vec::new();
    }
    let any_lane_soloed = project.lanes.values().any(|l| l.soloed);
    let (singer, language) = match &this.vocal {
        Some(v) => (v.singer.clone(), v.language.clone()),
        None => (String::new(), "en".to_string()),
    };
    let syllable = fontelle_model::lyrics::default_syllable(&language);

    struct Sounding<'a> {
        at: NoteRef,
        key: u8,
        fine: i16,
        lyric: &'a str,
    }
    let mut notes: Vec<Sounding> = Vec::new();
    for (clip_id, clip) in project.clips.iter() {
        if clip.muted {
            continue;
        }
        if project
            .lanes
            .get(clip.lane)
            .is_some_and(|l| l.muted || (any_lane_soloed && !l.soloed))
        {
            continue;
        }
        let Some(home) = project.note_home(clip_id) else {
            continue;
        };
        // The notes a prefab instance plays are its prefab's.
        let source = match home {
            NoteHome::Prefab(prefab) => project.prefabs.get(prefab).map(|p| &p.source),
            NoteHome::Clip(_) => Some(&clip.source),
        };
        let Some(ClipSource::Notes(data)) = source else {
            continue;
        };
        let period = clip.loop_length.filter(|p| *p > 0);
        for repeat in 0..clip.repeats() {
            let offset = clip.repeat_start(repeat);
            for (id, note) in data.notes.iter() {
                if note.slide || note.channel_or(data.channel) != channel {
                    continue;
                }
                if period.is_some_and(|p| note.start >= p) {
                    continue;
                }
                let start = note.start + offset;
                if start >= clip.length {
                    continue;
                }
                let pass_end = period.map_or(Tick::MAX, |p| offset + p);
                let end = (start + note.length).min(pass_end).min(clip.length);
                if end <= start {
                    continue;
                }
                notes.push(Sounding {
                    at: NoteRef {
                        clip: clip_id,
                        home,
                        note: id,
                        start: clip.start + start,
                        end: clip.start + end,
                    },
                    key: note.key,
                    fine: note.fine_pitch,
                    lyric: &note.lyric,
                });
            }
        }
    }
    // One voice: by start, the higher of two notes starting together wins
    // (the melody is usually on top), and a note is cut where the next one
    // starts.
    notes.sort_by(|a, b| a.at.start.cmp(&b.at.start).then(b.key.cmp(&a.key)));
    notes.dedup_by(|later, earlier| later.at.start == earlier.at.start);
    for i in 1..notes.len() {
        let next = notes[i].at.start;
        if notes[i - 1].at.end > next {
            notes[i - 1].at.end = next;
        }
    }

    let mut phrases: Vec<Phrase> = Vec::new();
    let mut current: Vec<(NoteRef, f64, f64, f32, String)> = Vec::new();
    let flush = |current: &mut Vec<(NoteRef, f64, f64, f32, String)>, phrases: &mut Vec<Phrase>| {
        let Some(first) = current.first() else {
            return;
        };
        let origin = first.1;
        let last = current.last().expect("not empty");
        let phrase = Phrase {
            start: origin,
            end: last.2,
            start_tick: first.0.start,
            end_tick: last.0.end,
            notes: current.iter().map(|c| c.0).collect(),
            spec: PhraseSpec {
                singer: singer.clone(),
                language: language.clone(),
                notes: current
                    .iter()
                    .map(|(_, s, e, pitch, lyric)| SpecNote {
                        start: s - origin,
                        duration: e - s,
                        pitch: *pitch,
                        lyric: lyric.clone(),
                    })
                    .collect(),
            },
        };
        phrases.push(phrase);
        current.clear();
    };
    for n in &notes {
        let start = tempo.tick_to_seconds(n.at.start);
        let end = tempo.tick_to_seconds(n.at.end);
        if let (Some(first), Some(last)) = (current.first(), current.last()) {
            let rest = start - last.2;
            let too_long = end - first.1 > MAX_PHRASE_SECONDS;
            if rest >= MIN_REST_SECONDS - 1e-9 || too_long {
                flush(&mut current, &mut phrases);
            }
        }
        let lyric = if n.lyric.trim().is_empty() {
            syllable.to_string()
        } else {
            n.lyric.trim().to_string()
        };
        let pitch = f32::from(n.key) + f32::from(n.fine) / 100.0;
        current.push((n.at, start, end, pitch, lyric));
    }
    flush(&mut current, &mut phrases);
    phrases
}
