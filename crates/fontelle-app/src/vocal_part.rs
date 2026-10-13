//! A vocal part as an external engine's window sees it, and the window's
//! edits as the document's commands.
//!
//! The window works in song seconds and note ids (`fontelle_sing::PartView`,
//! `PartEdit`); the document in clip ticks, `NoteId`s and commands. This is
//! the translation both ways, kept apart from the session so it can be
//! tested on a bare project: one open clip's notes on one vocal channel, its
//! lanes sampled into seconds, and a batch of edits made into one
//! [`Compound`] — one undo step, refused whole if any edit is.

use std::collections::BTreeMap;

use fontelle_model::{
    ClipSource, Command, Compound, LaneCurve, LaneKey, NoteData, NoteHome, NoteVoice, Project,
    SetLyrics, SetNoteVelocity, SetNoteVoices, SetVocalLane, SetVocalSettings, SetVocalState,
    TempoMap,
};
use fontelle_sing::{CURVE_STEP, Curve, PartEdit, PartView, PhraseNote, ViewNote};
use fontelle_types::{ChannelId, ClipId, NoteId, Tick};
use fontelle_vocal::{note_from_wire, note_wire_id};

/// The notes a window edits: the clip's, on the channel, and where the
/// clip's own tick 0 is in the song.
fn notes_of(project: &Project, channel: ChannelId, clip: ClipId) -> Option<(&NoteData, Tick)> {
    let found = project.clips.get(clip)?;
    let source = match project.note_home(clip)? {
        NoteHome::Prefab(p) => &project.prefabs.get(p)?.source,
        NoteHome::Clip(_) => &found.source,
    };
    let ClipSource::Notes(data) = source else {
        return None;
    };
    let _ = channel;
    Some((data, found.start))
}

fn seconds_at(tempo: &TempoMap, tick: Tick) -> f64 {
    tempo.tick_to_seconds(tick)
}

fn tick_at(tempo: &TempoMap, seconds: f64) -> Tick {
    tempo.sample_to_tick((seconds * tempo.sample_rate_hz()).round() as i64)
}

/// A clip-tick lane sampled in song seconds over `from..=to`.
fn lane_seconds(
    curve: &LaneCurve,
    tempo: &TempoMap,
    origin: Tick,
    from: f64,
    to: f64,
    neutral: f32,
) -> Curve {
    let count = (((to - from) / CURVE_STEP).ceil().max(0.0) as usize) + 1;
    Curve {
        start: from,
        step: CURVE_STEP,
        values: (0..count)
            .map(|i| {
                curve.at(
                    tick_at(tempo, from + i as f64 * CURVE_STEP) - origin,
                    neutral,
                )
            })
            .collect(),
    }
}

/// A song-seconds curve as a clip-tick lane.
pub fn lane_from_seconds(curve: &Curve, tempo: &TempoMap, origin: Tick, neutral: f32) -> LaneCurve {
    let points = curve
        .values
        .iter()
        .enumerate()
        .filter(|(_, v)| v.is_finite())
        .map(|(i, v)| {
            (
                tick_at(tempo, curve.start + i as f64 * curve.step) - origin,
                *v,
            )
        })
        .collect();
    LaneCurve::from_samples(points, neutral)
}

/// The part `channel` sings in `clip`, as `backend`'s window sees it.
#[allow(clippy::too_many_arguments)]
pub fn part_view(
    project: &Project,
    tempo: &TempoMap,
    channel: ChannelId,
    clip: ClipId,
    selection: &[NoteId],
    playhead: f64,
    playing: bool,
    sung: &dyn Fn(NoteId) -> Vec<String>,
) -> Option<PartView> {
    let voice = project.channels.get(channel)?.vocal.clone()?;
    let (data, origin) = notes_of(project, channel, clip)?;
    let mut notes: Vec<(NoteId, &fontelle_model::Note)> = data.notes_on(channel).collect();
    notes.sort_by_key(|(_, n)| (n.start, std::cmp::Reverse(n.key)));
    let view_notes: Vec<ViewNote> = notes
        .iter()
        .map(|(id, n)| {
            let start = seconds_at(tempo, origin + n.start);
            let end = seconds_at(tempo, origin + n.start + n.length);
            let mut note = PhraseNote::new(
                start,
                end - start,
                f32::from(n.key) + f32::from(n.fine_pitch) / 100.0,
                n.lyric.clone(),
            );
            note.velocity = fontelle_model::consonant_speed(n.velocity);
            if let Some(v) = &n.voice {
                note.phonemes.clone_from(&v.phonemes);
                note.vibrato = v.vibrato;
                note.portamento = v.portamento;
                note.options = v.options_for(&voice.engine);
            }
            ViewNote {
                id: note_wire_id(*id),
                note,
                sung: sung(*id),
            }
        })
        .collect();
    let clip_end = project
        .clips
        .get(clip)
        .map_or(origin, |c| c.start + c.length);
    let from = seconds_at(tempo, origin);
    let to = seconds_at(tempo, clip_end);
    let part = data.vocal.as_deref().cloned().unwrap_or_default();
    let curves = part
        .lanes
        .iter()
        .map(|(lane, c)| {
            (
                *lane,
                lane_seconds(c, tempo, origin, from, to, lane.neutral()),
            )
        })
        .collect();
    let extra_curves = part
        .extra
        .get(&voice.engine)
        .map(|lanes| {
            lanes
                .iter()
                .map(|(id, c)| (id.clone(), lane_seconds(c, tempo, origin, from, to, 0.0)))
                .collect()
        })
        .unwrap_or_default();
    Some(PartView {
        track: project.channels[channel].name.clone(),
        voice: voice.singer.clone(),
        language: voice.language.clone(),
        notes: view_notes,
        curves,
        extra_curves,
        settings: part
            .settings
            .get(&voice.engine)
            .cloned()
            .unwrap_or_default(),
        state: part.state.get(&voice.engine).cloned(),
        selection: selection.iter().map(|id| note_wire_id(*id)).collect(),
        playhead,
        playing,
    })
}

/// What a batch of a window's edits does to the document.
pub struct Applied {
    /// One step, refused whole if any part is.
    pub command: Option<Box<dyn Command>>,
    /// Notes the window asked to have selected.
    pub select: Option<Vec<NoteId>>,
}

/// A window's batch of edits as one [`Compound`]. `Err` names the edit that
/// cannot be made (a note not in the clip), and nothing is changed.
///
/// `PartEdit::Settings` **merges** (keys given are set, others kept), as
/// `fontelle_sing` documents.
pub fn edits_to_command(
    project: &Project,
    tempo: &TempoMap,
    channel: ChannelId,
    clip: ClipId,
    edits: &[PartEdit],
    caps: &fontelle_sing::Capabilities,
) -> Result<Applied, String> {
    let voice = project
        .channels
        .get(channel)
        .and_then(|c| c.vocal.clone())
        .ok_or("the track has no singer")?;
    let backend = voice.engine.clone();
    let (data, origin) = notes_of(project, channel, clip).ok_or("the part is not open")?;
    let home = project.note_home(clip).ok_or("the part is not open")?;
    let note = |id: u64| -> Result<NoteId, String> {
        let id = note_from_wire(id);
        data.notes
            .get(id)
            .filter(|n| n.channel_or(data.channel) == channel)
            .map(|_| id)
            .ok_or_else(|| "an edit names a note that is not in the part".to_string())
    };
    // Each note's voice as the batch leaves it, so two edits to one note in
    // one batch both land.
    let mut changed_voices: BTreeMap<NoteId, NoteVoice> = BTreeMap::new();
    let voice_of = |changed: &BTreeMap<NoteId, NoteVoice>, id: NoteId| -> NoteVoice {
        changed
            .get(&id)
            .cloned()
            .or_else(|| data.notes[id].voice.as_deref().cloned())
            .unwrap_or_default()
    };
    let mut parts: Vec<Box<dyn Command>> = Vec::new();
    let mut select = None;
    let mut settings = data
        .vocal
        .as_ref()
        .and_then(|p| p.settings.get(&backend))
        .cloned()
        .unwrap_or_default();
    let mut settings_changed = false;
    for edit in edits {
        match edit {
            PartEdit::Settings(given) => {
                if given.values().any(|v| !v.is_finite()) {
                    return Err("a setting is not a number".into());
                }
                settings.extend(given.iter().map(|(k, v)| (k.clone(), *v)));
                settings_changed = true;
            }
            PartEdit::State(state) => parts.push(Box::new(SetVocalState::new(
                home,
                backend.clone(),
                state.clone(),
            ))),
            PartEdit::Curve { lane, curve } => {
                let lane_curve = curve
                    .as_ref()
                    .map(|c| lane_from_seconds(c, tempo, origin, lane.neutral()));
                parts.push(Box::new(SetVocalLane::new(
                    home,
                    LaneKey::Standard(*lane),
                    lane_curve,
                )));
            }
            PartEdit::ExtraCurve { id, curve } => {
                let neutral = caps
                    .extra_lanes
                    .iter()
                    .find(|l| &l.id == id)
                    .map_or(0.0, |l| l.default);
                let lane_curve = curve
                    .as_ref()
                    .map(|c| lane_from_seconds(c, tempo, origin, neutral));
                parts.push(Box::new(SetVocalLane::new(
                    home,
                    LaneKey::Extra {
                        backend: backend.clone(),
                        id: id.clone(),
                    },
                    lane_curve,
                )));
            }
            PartEdit::Lyrics(lyrics) => {
                let ids = lyrics
                    .iter()
                    .map(|(id, _)| note(*id))
                    .collect::<Result<Vec<_>, _>>()?;
                let words = lyrics.iter().map(|(_, l)| l.clone()).collect();
                parts.push(Box::new(SetLyrics::new(home, ids, words)));
            }
            PartEdit::Velocity { notes, velocity } => {
                let ids = notes
                    .iter()
                    .map(|id| note(*id))
                    .collect::<Result<Vec<_>, _>>()?;
                parts.push(Box::new(SetNoteVelocity::new(
                    home,
                    ids,
                    fontelle_model::velocity_for_speed(*velocity),
                )));
            }
            PartEdit::Phonemes { note: id, phonemes } => {
                let id = note(*id)?;
                let mut v = voice_of(&changed_voices, id);
                v.phonemes.clone_from(phonemes);
                changed_voices.insert(id, v);
            }
            PartEdit::Vibrato { notes, vibrato } => {
                for id in notes {
                    let id = note(*id)?;
                    let mut v = voice_of(&changed_voices, id);
                    v.vibrato = *vibrato;
                    changed_voices.insert(id, v);
                }
            }
            PartEdit::Portamento { notes, portamento } => {
                for id in notes {
                    let id = note(*id)?;
                    let mut v = voice_of(&changed_voices, id);
                    v.portamento = *portamento;
                    changed_voices.insert(id, v);
                }
            }
            PartEdit::NoteOptions { notes, options } => {
                for id in notes {
                    let id = note(*id)?;
                    let mut v = voice_of(&changed_voices, id);
                    let own = v.options.entry(backend.clone()).or_default();
                    own.extend(options.iter().map(|(k, o)| (k.clone(), o.clone())));
                    changed_voices.insert(id, v);
                }
            }
            PartEdit::Select(ids) => {
                select = Some(
                    ids.iter()
                        .map(|id| note(*id))
                        .collect::<Result<Vec<_>, _>>()?,
                );
            }
        }
    }
    if settings_changed {
        parts.push(Box::new(SetVocalSettings::new(
            home,
            backend.clone(),
            settings,
        )));
    }
    if !changed_voices.is_empty() {
        let (ids, voices): (Vec<NoteId>, Vec<Option<NoteVoice>>) = changed_voices
            .into_iter()
            .map(|(id, v)| (id, Some(v)))
            .unzip();
        parts.push(Box::new(SetNoteVoices::new(
            home,
            ids,
            voices,
            "Edit in the singer's window",
        )));
    }
    let command: Option<Box<dyn Command>> = (!parts.is_empty()).then(|| {
        Box::new(Compound::new(format!("Edit in {}", voice.engine), parts)) as Box<dyn Command>
    });
    Ok(Applied { command, select })
}
