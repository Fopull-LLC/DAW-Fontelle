//! A vocal part cut into phrases, and the keys their takes are cached by.

use std::collections::{BTreeMap, BTreeSet};
use std::hash::Hasher;

use fontelle_model::{ClipSource, LaneCurve, LaneKey, NoteHome, Project, TempoMap, VocalPart};
use fontelle_sing::{CURVE_STEP, Capabilities, Curve, Lane, Quality};
use fontelle_types::{ChannelId, ClipId, NoteId, Tick};

/// A rest at least this long ends a phrase: long enough to breathe, so the
/// singer sings what comes after as a fresh start.
pub const MIN_REST_SECONDS: f64 = 0.2;
/// No phrase runs longer than this: an engine sings a phrase in one go, so a
/// verse with no rest in it would otherwise be one enormous render, and an
/// edit anywhere in it would sing all of it again.
pub const MAX_PHRASE_SECONDS: f64 = 12.0;
/// How far before a phrase's first note its curves start (a consonant sung
/// ahead of the note needs a value), and how far past its last note's end
/// they run (the release).
pub const CURVE_LEAD: f64 = 0.25;
pub const CURVE_TAIL: f64 = 0.25;

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
    /// What the singer is asked, in phrase-relative seconds: every note's
    /// lyric (an empty one already the language's own syllable) and
    /// expression, the part's lanes sampled over the phrase, and the
    /// singing backend's own lanes, settings and state.
    pub sing: fontelle_sing::Phrase,
}

/// What a take is cached under: a hash of the backend, its version, the
/// quality and **the whole phrase** in its own time — and the seed it is
/// sung with, from the same things less the quality, so a preview and its
/// final are the same take.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RenderKey {
    pub hash: u64,
    pub seed: u64,
}

/// Hashes `phrase` with its numbers rounded to what anybody could hear, so
/// a phrase moved by a tempo-exact amount — whose times differ in their
/// last bits — keys the same. Its identity (`start`, `note_ids`) is left
/// out on purpose: a moved phrase is the same take.
fn content_hash(backend: &str, version: &str, phrase: &fontelle_sing::Phrase) -> u64 {
    let mut h = twox_hash::XxHash64::with_seed(0x766f_6361_6c00_0002);
    let text = |h: &mut twox_hash::XxHash64, s: &str| {
        h.write_u64(s.len() as u64);
        h.write(s.as_bytes());
    };
    let micros = |s: f64| (s * 1e6).round() as i64;
    let fine = |v: f32| (f64::from(v) * 1e4).round() as i64;
    let curve = |h: &mut twox_hash::XxHash64, c: &Curve| {
        h.write_i64(micros(c.start));
        h.write_i64(micros(c.step));
        h.write_u64(c.values.len() as u64);
        for v in &c.values {
            // A thousandth of a unit: lanes are sampled from ticks, so a
            // moved phrase's samples land a hair apart.
            h.write_i64((f64::from(*v) * 1e3).round() as i64);
        }
    };
    text(&mut h, backend);
    text(&mut h, version);
    text(&mut h, &phrase.voice);
    text(&mut h, &phrase.language);
    h.write_u64(phrase.notes.len() as u64);
    for n in &phrase.notes {
        h.write_i64(micros(n.start));
        h.write_i64(micros(n.duration));
        h.write_i32((n.pitch * 100.0).round() as i32);
        text(&mut h, &n.lyric);
        h.write_i64(fine(n.velocity));
        match &n.phonemes {
            None => h.write_u8(0),
            Some(p) => {
                h.write_u8(1);
                h.write_u64(p.len() as u64);
                for e in p {
                    text(&mut h, &e.symbol);
                    h.write_i64(e.offset.map_or(i64::MIN, micros));
                }
            }
        }
        match n.vibrato {
            None => h.write_u8(0),
            Some(v) => {
                h.write_u8(1);
                for x in [v.depth, v.rate, v.delay, v.fade_in, v.fade_out] {
                    h.write_i64(fine(x));
                }
            }
        }
        match n.portamento {
            None => h.write_u8(0),
            Some(p) => {
                h.write_u8(1);
                h.write_i64(fine(p.length));
                h.write_i64(fine(p.position));
                text(&mut h, &format!("{:?}", p.shape));
            }
        }
        h.write_u64(n.options.len() as u64);
        for (k, v) in &n.options {
            text(&mut h, k);
            text(&mut h, v);
        }
    }
    h.write_u64(phrase.curves.len() as u64);
    for (lane, c) in &phrase.curves {
        text(&mut h, &format!("{lane:?}"));
        curve(&mut h, c);
    }
    h.write_u64(phrase.extra_curves.len() as u64);
    for (id, c) in &phrase.extra_curves {
        text(&mut h, id);
        curve(&mut h, c);
    }
    h.write_u64(phrase.settings.len() as u64);
    for (id, v) in &phrase.settings {
        text(&mut h, id);
        h.write_i64(fine(*v));
    }
    match &phrase.state {
        None => h.write_u8(0),
        Some(state) => {
            h.write_u8(1);
            h.write_u64(state.len() as u64);
            h.write(state);
        }
    }
    h.finish()
}

/// The key a take of `phrase` at `quality` from backend `backend` `version`
/// is cached under. See [`RenderKey`].
pub fn render_key(
    backend: &str,
    version: &str,
    phrase: &fontelle_sing::Phrase,
    quality: Quality,
) -> RenderKey {
    let content = content_hash(backend, version, phrase);
    let mut h = twox_hash::XxHash64::with_seed(content);
    h.write_u32(match quality {
        Quality::Preview => 0,
        Quality::Final => 1,
    });
    let mut s = twox_hash::XxHash64::with_seed(0x5eed);
    s.write_u64(content);
    RenderKey {
        hash: h.finish(),
        seed: s.finish(),
    }
}

/// A note's id as a backend sees it (`fontelle_sing::PartView`,
/// `Phrase::note_ids`): the slot key's own 64 bits, stable for the life of
/// the note and the same on every machine sharing the song.
pub fn note_wire_id(id: NoteId) -> u64 {
    use slotmap::Key;
    id.data().as_ffi()
}

/// [`note_wire_id`] back.
pub fn note_from_wire(id: u64) -> NoteId {
    slotmap::KeyData::from_ffi(id).into()
}

/// One pass of a clip's notes over the song, for reading its part's lanes:
/// song ticks `start..end`, the part's tick 0 at `origin`, repeating every
/// `period` when it loops.
struct Pass<'a> {
    start: Tick,
    end: Tick,
    origin: Tick,
    period: Option<Tick>,
    part: Option<&'a VocalPart>,
}

impl Pass<'_> {
    fn local(&self, tick: Tick) -> Tick {
        let local = tick - self.origin;
        match self.period {
            Some(p) => local.rem_euclid(p),
            None => local,
        }
    }
}

/// A lane's value at song tick `tick`: the pass sounding there, else neutral.
fn lane_at(passes: &[Pass], key: &LaneKey, tick: Tick, neutral: f32) -> f32 {
    let Some(pass) = passes
        .iter()
        .rev()
        .find(|p| tick >= p.start && tick < p.end)
    else {
        return neutral;
    };
    pass.part
        .and_then(|part| part.curve(key))
        .map_or(neutral, |c: &LaneCurve| c.at(pass.local(tick), neutral))
}

/// Every phrase `channel` sings, its backend's own lanes taken as neutral at
/// 0 — [`phrases_with`] when the backend's capabilities are known.
pub fn phrases_of(project: &Project, channel: ChannelId, tempo: &TempoMap) -> Vec<Phrase> {
    phrases_with(project, channel, tempo, &Capabilities::default())
}

/// Every phrase `channel` sings in `project`, in time order, as its
/// backend (`caps`) is asked to sing it.
///
/// From exactly the notes the song would play on that channel — the
/// sequencer's own rules: a muted clip, row or channel sings nothing, a
/// solo silences the rest, a clip's end cuts its notes and a looped clip
/// sings every pass. Slide notes start no syllable and are left out. Notes
/// that overlap (a vocal line is one voice) are cut at the next one's start.
///
/// Each note brings its expression (`Note::voice`, its velocity as
/// consonant speed, only the singing backend's options), and the part's
/// lanes — the standard ones and the backend's own — are sampled every
/// [`CURVE_STEP`] from [`CURVE_LEAD`] before the first note to
/// [`CURVE_TAIL`] after the last, from whichever clip is playing at each
/// moment; a lane that is neutral all the way through is left out, as the
/// contract says an undrawn lane is. Settings and state are the first
/// note's clip's.
pub fn phrases_with(
    project: &Project,
    channel: ChannelId,
    tempo: &TempoMap,
    caps: &Capabilities,
) -> Vec<Phrase> {
    let Some(this) = project.channels.get(channel) else {
        return Vec::new();
    };
    let any_channel_soloed = project.channels.values().any(|c| c.soloed);
    // A frozen track plays its render instead (`fontelle_model::FreezeVocal`).
    if this.muted || (any_channel_soloed && !this.soloed) || project.is_frozen(channel) {
        return Vec::new();
    }
    let any_lane_soloed = project.lanes.values().any(|l| l.soloed);
    let (backend, singer, language) = match &this.vocal {
        Some(v) => (v.engine.clone(), v.singer.clone(), v.language.clone()),
        None => (String::new(), String::new(), "en".to_string()),
    };
    let syllable = fontelle_model::lyrics::default_syllable(&language);

    struct Sounding<'a> {
        at: NoteRef,
        key: u8,
        fine: i16,
        note: &'a fontelle_model::Note,
        pass: usize,
    }
    let mut notes: Vec<Sounding> = Vec::new();
    let mut passes: Vec<Pass> = Vec::new();
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
        let sings_here = data.channel == channel
            || data
                .notes
                .values()
                .any(|n| n.channel_or(data.channel) == channel);
        if !sings_here {
            continue;
        }
        let period = clip.loop_length.filter(|p| *p > 0);
        for repeat in 0..clip.repeats() {
            let offset = clip.repeat_start(repeat);
            let pass_index = passes.len();
            let pass_end = period.map_or(clip.length, |p| (offset + p).min(clip.length));
            passes.push(Pass {
                start: clip.start + offset,
                end: clip.start + pass_end,
                origin: clip.start + offset,
                period: None,
                part: data.vocal.as_deref(),
            });
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
                    note,
                    pass: pass_index,
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

    // Which lanes there are to sample: every standard lane and every one of
    // this backend's own that any pass has drawn.
    let mut standard: BTreeSet<Lane> = BTreeSet::new();
    let mut extra: BTreeSet<String> = BTreeSet::new();
    for pass in &passes {
        if let Some(part) = pass.part {
            standard.extend(part.lanes.keys().copied());
            if let Some(own) = part.extra.get(&backend) {
                extra.extend(own.keys().cloned());
            }
        }
    }
    let extra_neutral = |id: &str| {
        caps.extra_lanes
            .iter()
            .find(|l| l.id == id)
            .map_or(0.0, |l| l.default)
    };
    let sample_rate = tempo.sample_rate_hz();
    let tick_at = |seconds: f64| tempo.sample_to_tick((seconds * sample_rate).round() as i64);
    let sample = |key: &LaneKey, neutral: f32, from: f64, to: f64| -> Option<Curve> {
        let count = (((to - from) / CURVE_STEP).ceil().max(0.0) as usize) + 1;
        let mut values = Vec::with_capacity(count);
        let mut any = false;
        for i in 0..count {
            let v = lane_at(&passes, key, tick_at(from + i as f64 * CURVE_STEP), neutral);
            any |= (v - neutral).abs() > 1e-6;
            values.push(v);
        }
        any.then_some(Curve {
            start: -CURVE_LEAD,
            step: CURVE_STEP,
            values,
        })
    };

    let mut phrases: Vec<Phrase> = Vec::new();
    let mut current: Vec<(usize, f64, f64)> = Vec::new();
    let flush = |current: &mut Vec<(usize, f64, f64)>, phrases: &mut Vec<Phrase>| {
        let Some(&(first_index, origin, _)) = current.first() else {
            return;
        };
        let &(last_index, _, last_end) = current.last().expect("not empty");
        let first = &notes[first_index];
        let from = origin - CURVE_LEAD;
        let to = last_end + CURVE_TAIL;
        let mut curves = BTreeMap::new();
        for lane in &standard {
            if let Some(c) = sample(&LaneKey::Standard(*lane), lane.neutral(), from, to) {
                curves.insert(*lane, c);
            }
        }
        let mut extra_curves = BTreeMap::new();
        for id in &extra {
            let key = LaneKey::Extra {
                backend: backend.clone(),
                id: id.clone(),
            };
            if let Some(c) = sample(&key, extra_neutral(id), from, to) {
                extra_curves.insert(id.clone(), c);
            }
        }
        let part = passes[first.pass].part;
        let phrase = Phrase {
            start: origin,
            end: last_end,
            start_tick: first.at.start,
            end_tick: notes[last_index].at.end,
            notes: current.iter().map(|(i, _, _)| notes[*i].at).collect(),
            sing: fontelle_sing::Phrase {
                voice: singer.clone(),
                language: language.clone(),
                notes: current
                    .iter()
                    .map(|(i, s, e)| {
                        let n = &notes[*i];
                        let lyric = if n.note.lyric.trim().is_empty() {
                            syllable.to_string()
                        } else {
                            n.note.lyric.trim().to_string()
                        };
                        let pitch = f32::from(n.key) + f32::from(n.fine) / 100.0;
                        let mut sung =
                            fontelle_sing::PhraseNote::new(s - origin, e - s, pitch, lyric);
                        sung.velocity = fontelle_model::consonant_speed(n.note.velocity);
                        if let Some(voice) = &n.note.voice {
                            sung.phonemes.clone_from(&voice.phonemes);
                            sung.vibrato = voice.vibrato;
                            sung.portamento = voice.portamento;
                            sung.options = voice.options_for(&backend);
                        }
                        sung
                    })
                    .collect(),
                curves,
                extra_curves,
                settings: part
                    .and_then(|p| p.settings.get(&backend))
                    .cloned()
                    .unwrap_or_default(),
                state: part.and_then(|p| p.state.get(&backend)).cloned(),
                // Identity, left out of the render key (see `content_hash`).
                start: origin,
                note_ids: current
                    .iter()
                    .map(|(i, _, _)| note_wire_id(notes[*i].at.note))
                    .collect(),
            },
        };
        phrases.push(phrase);
        current.clear();
    };
    for (index, n) in notes.iter().enumerate() {
        let start = tempo.tick_to_seconds(n.at.start);
        let end = tempo.tick_to_seconds(n.at.end);
        if let (Some(first), Some(last)) = (current.first(), current.last()) {
            let rest = start - last.2;
            let too_long = end - first.1 > MAX_PHRASE_SECONDS;
            if rest >= MIN_REST_SECONDS - 1e-9 || too_long {
                flush(&mut current, &mut phrases);
            }
        }
        current.push((index, start, end));
    }
    flush(&mut current, &mut phrases);
    phrases
}
