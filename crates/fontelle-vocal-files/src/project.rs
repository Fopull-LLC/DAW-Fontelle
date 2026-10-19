//! Between a file's song and the document: a part placed as a clip, and a
//! vocal track read back out as a song to write.

use fontelle_model::{ClipSource, LaneCurve, NoteHome, Project, VocalClipImport, VocalPart};
use fontelle_types::{ChannelId, Tick};

use crate::{SongPart, VocalSong};

/// `part` as a clip `offset` ticks into the song, **by bars**: from the
/// start of the bar its first note is in to the end of the bar its last note
/// ends in, bars of the file's own `meter`; notes and lanes from the clip's
/// start.
pub fn clip_of(part: &SongPart, offset: Tick, meter: &fontelle_types::Meter) -> VocalClipImport {
    let first = part.notes.iter().map(|n| n.start).min().unwrap_or(0).max(0);
    let end = part.end();
    let (first_bar, _) = meter.bar_at(first);
    let from = meter.tick_of_bar(first_bar);
    let to = match meter.bar_at(end) {
        (bar, 0) => meter.tick_of_bar(bar),
        (bar, _) => meter.tick_of_bar(bar + 1),
    };
    let bar = meter.bar_ticks(first_bar);
    let notes = part
        .notes
        .iter()
        .map(|n| {
            let mut n = n.clone();
            n.start -= from;
            n
        })
        .collect();
    let vocal = (!part.vocal.is_empty()).then(|| Box::new(part.vocal.shifted(-from)));
    VocalClipImport {
        name: part.name.clone(),
        start: offset + from,
        length: (to - from).max(bar),
        notes,
        vocal,
    }
}

/// `channel`'s part as a song: every clip that plays its notes (a prefab
/// instance's are its prefab's; a looped clip's every pass), on the song's
/// ticks, with their lanes; the song's tempo and bar. Muted clips are
/// written too — they are the part.
pub fn song_of_channel(project: &Project, channel: ChannelId) -> VocalSong {
    let name = project
        .channels
        .get(channel)
        .map(|c| c.name.clone())
        .unwrap_or_else(|| project.meta.name.clone());
    let mut song = VocalSong::new(name.clone());
    song.tempo = project.tempo_map.segments().to_vec();
    song.beats_per_bar = project.beats_per_bar;
    song.beat_unit = project.beat_unit;
    let mut part = SongPart::new(name);
    let mut clips: Vec<_> = project.clips.iter().collect();
    clips.sort_by_key(|(_, c)| c.start);
    let mut lanes: Vec<(Tick, Tick, &VocalPart)> = Vec::new();
    for (id, clip) in clips {
        let Some(home) = project.note_home(id) else {
            continue;
        };
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
            let pass_end = period.map_or(clip.length, |p| (offset + p).min(clip.length));
            if let Some(v) = data.vocal.as_deref() {
                lanes.push((clip.start + offset, clip.start + pass_end, v));
            }
            for note in data.notes.values() {
                if note.slide || note.channel_or(data.channel) != channel {
                    continue;
                }
                if period.is_some_and(|p| note.start >= p) {
                    continue;
                }
                let start = note.start + offset;
                let end = (start + note.length).min(pass_end);
                if start >= clip.length || end <= start {
                    continue;
                }
                let mut n = note.clone();
                n.start = clip.start + start;
                n.length = end - start;
                n.channel = None;
                part.notes.push(n);
            }
        }
    }
    // Each pass's lanes over its own stretch of the song.
    let keys: std::collections::BTreeSet<_> = lanes
        .iter()
        .flat_map(|(_, _, v)| v.lanes.keys().copied())
        .collect();
    for key in keys {
        let mut points: Vec<(Tick, f32)> = Vec::new();
        for (from, to, v) in &lanes {
            let Some(curve) = v.lanes.get(&key) else {
                continue;
            };
            let at = |t: Tick| curve.at(t - from, 0.0);
            points.push((*from, at(*from)));
            points.extend(
                curve
                    .points()
                    .iter()
                    .map(|(t, val)| (t + from, *val))
                    .filter(|(t, _)| *t > *from && *t < *to),
            );
            points.push((to - 1, at(to - 1)));
        }
        let curve = LaneCurve::from_samples(points, 0.0);
        if !curve.is_empty() {
            part.vocal.lanes.insert(key, curve);
        }
    }
    // A backend's own lanes and OpenUtau's kept curves, from the first pass
    // that has them (shifted onto the song).
    for (from, _, v) in &lanes {
        for (backend, own) in &v.extra {
            let target = part.vocal.extra.entry(backend.clone()).or_default();
            for (id, curve) in own {
                target
                    .entry(id.clone())
                    .or_insert_with(|| curve.shifted(*from));
            }
        }
    }
    if !part.notes.is_empty() {
        song.parts.push(part);
    }
    song.tidy();
    song
}
