//! UTAU's `.ust`: an INI of notes one after another.
//!
//! What maps, and to what:
//!
//! - `Length`/`NoteNum`/`Lyric` → notes (480 a quarter in the file, rests
//!   are `R`); a VCV alias's context (`"a か"`, `"- か"`) is dropped, the
//!   phonemizer works it out again.
//! - `Tempo` under `[#SETTING]` and on any note → the tempo map.
//! - `Velocity` (consonant speed, `2^((v-100)/100)`) → the note's velocity
//!   as Fontelle reads it.
//! - `Intensity` (%) → the dynamics lane, a step per note.
//! - Mode2 pitch (`PBS`/`PBW`/`PBY`/`PBM`) and Mode1 (`PitchBend` +
//!   `PBStart`) → the pitch-bend lane over the notes (`crate::pitch`).
//! - `VBR` → the note's vibrato.
//! - Flags `g` (gender) and `B` (breathiness), the song's and each
//!   note's → the gender and breathiness lanes. Other flags, `Modulation`,
//!   envelopes and per-note oto overrides are resampler business and are
//!   named in `unmapped`.

use std::collections::BTreeMap;

use fontelle_model::{LaneCurve, TempoSegment, velocity_for_speed};
use fontelle_sing::{Lane, Vibrato};
use fontelle_types::{PPQN, Tick};

use crate::pitch::{self, NoteCurve, Point, Shape};
use crate::{SongPart, VocalSong, vocal_note};

/// UTAU's resolution.
const UST_PPQ: i64 = 480;

fn ticks(ust: f64) -> Tick {
    (ust * PPQN as f64 / UST_PPQ as f64).round() as Tick
}

/// Reads a UST (Shift-JIS or UTF-8).
pub fn read(bytes: &[u8], name: &str) -> Result<VocalSong, String> {
    let text = decode(bytes);
    let sections = sections(&text);
    if !sections.iter().any(|(head, _)| head.starts_with('#')) {
        return Err(format!("{name} is not a UST file"));
    }
    let mut song = VocalSong::new(name);
    let setting = sections
        .iter()
        .find(|(head, _)| head == "#SETTING")
        .map(|(_, keys)| keys.clone())
        .unwrap_or_default();
    if let Some(title) = setting.get("ProjectName").filter(|t| !t.trim().is_empty()) {
        song.name = title.trim().to_string();
    }
    let mut bpm = setting
        .get("Tempo")
        .and_then(|t| number(t))
        .unwrap_or(120.0);
    song.tempo = vec![TempoSegment { start_tick: 0, bpm }];
    let song_flags = setting.get("Flags").cloned().unwrap_or_default();

    struct Raw<'a> {
        start: Tick,
        keys: &'a BTreeMap<String, String>,
    }
    let mut raw: Vec<Raw> = Vec::new();
    let mut at: f64 = 0.0;
    for (head, keys) in &sections {
        // `#0000`..: notes; `#PREV`/`#NEXT` are a plugin's context.
        if !head.starts_with('#') || !head[1..].chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let length = keys.get("Length").and_then(|l| number(l)).unwrap_or(0.0);
        let start = ticks(at);
        if let Some(t) = keys.get("Tempo").and_then(|t| number(t))
            && (t - bpm).abs() > 1e-9
        {
            bpm = t;
            song.tempo.push(TempoSegment {
                start_tick: start,
                bpm,
            });
        }
        at += length;
        let lyric = keys.get("Lyric").map(|l| l.trim()).unwrap_or("");
        if lyric.is_empty() || lyric.eq_ignore_ascii_case("r") || length <= 0.0 {
            continue;
        }
        raw.push(Raw { start, keys });
    }
    song.tempo.dedup_by_key(|s| s.start_tick);
    let tempo = song.tempo_map();

    let mut part = SongPart::new(song.name.clone());
    let mut curves: Vec<NoteCurve> = Vec::new();
    let mut mode1: Vec<(Tick, f64)> = Vec::new();
    let mut dynamics: Vec<(Tick, f32)> = Vec::new();
    let mut gender: Vec<(Tick, f32)> = Vec::new();
    let mut breath: Vec<(Tick, f32)> = Vec::new();
    for (index, r) in raw.iter().enumerate() {
        let k = r.keys;
        let length = ticks(k.get("Length").and_then(|l| number(l)).unwrap_or(0.0));
        let key = k
            .get("NoteNum")
            .and_then(|n| number(n))
            .unwrap_or(60.0)
            .clamp(0.0, 127.0) as u8;
        let mut note = vocal_note(r.start, length, key, strip_context(&k["Lyric"]));
        if let Some(v) = k.get("Velocity").and_then(|v| number(v)) {
            note.velocity = velocity_for_speed(2f32.powf((v as f32 - 100.0) / 100.0));
        }
        if let Some(vbr) = k.get("VBR").and_then(|v| vibrato(v)) {
            pitch::edit_voice(&mut note, |voice| voice.vibrato = Some(vbr));
        }
        let end = r.start + length;
        let step = |points: &mut Vec<(Tick, f32)>, value: f32| {
            points.push((r.start, value));
            points.push((end - 1, value));
        };
        let intensity = k.get("Intensity").and_then(|i| number(i)).unwrap_or(100.0);
        step(
            &mut dynamics,
            (20.0 * (intensity.max(1e-3) / 100.0).log10()).clamp(-24.0, 12.0) as f32,
        );
        // The note's flags after the song's: the later letter wins.
        let flags = format!("{song_flags}{}", k.get("Flags").map_or("", |f| f.as_str()));
        let (g, b, others) = read_flags(&flags);
        step(&mut gender, (-g * 6.0) as f32);
        step(&mut breath, ((b - 50.0) / 50.0) as f32);
        for other in others {
            song.unmapped(format!("the resampler flag \u{201c}{other}\u{201d}"));
        }
        for (field, what) in [
            ("Modulation", "Modulation (the recording's own pitch)"),
            ("Envelope", "envelopes"),
            ("PreUtterance", "per-note preutterance"),
            ("VoiceOverlap", "per-note overlap"),
            ("StartPoint", "per-note start points"),
        ] {
            if k.get(field).is_some_and(|v| {
                let v = v.trim();
                !v.is_empty() && number(v).is_none_or(|n| n != 0.0 || field == "Envelope")
            }) {
                song.unmapped(what);
            }
        }
        // Mode2's points, else Mode1's list.
        if let Some(pbs) = k.get("PBS") {
            let previous = index
                .checked_sub(1)
                .map(|i| &raw[i])
                .and_then(|p| {
                    let len = ticks(p.keys.get("Length").and_then(|l| number(l)).unwrap_or(0.0));
                    (p.start + len >= r.start).then_some(p)
                })
                .and_then(|p| p.keys.get("NoteNum").and_then(|n| number(n)));
            let points = mode2_points(
                pbs,
                k.get("PBW").map_or("", |s| s.as_str()),
                k.get("PBY").map_or("", |s| s.as_str()),
                k.get("PBM").map_or("", |s| s.as_str()),
                previous.map(|p| (p - f64::from(key)) * 100.0),
            );
            if points.len() >= 2 {
                curves.push(NoteCurve {
                    note: part.notes.len(),
                    points,
                });
            }
        } else if let Some(list) = k.get("PitchBend") {
            let from_ms = k.get("PBStart").and_then(|s| number(s)).unwrap_or(0.0);
            let seconds = tempo.tick_to_seconds(r.start) + from_ms / 1000.0;
            let origin = pitch::tick_of_seconds(&tempo, seconds);
            for (i, cents) in list.split(',').enumerate() {
                if let Some(c) = number(cents) {
                    mode1.push((origin + ticks(5.0 * i as f64), c));
                }
            }
        }
        part.notes.push(note);
    }
    let mode1_span = (mode1.first().map(|p| p.0)).zip(mode1.last().map(|p| p.0));
    let mode1_at = |t: Tick| -> f64 {
        let Some((first, last)) = mode1_span else {
            return 0.0;
        };
        if t < first || t > last {
            return 0.0;
        }
        let i = mode1.partition_point(|(at, _)| *at <= t);
        if i == 0 {
            return mode1[0].1;
        }
        let (t0, v0) = mode1[i - 1];
        match mode1.get(i) {
            Some(&(t1, v1)) if t1 > t0 => v0 + (v1 - v0) * (t - t0) as f64 / (t1 - t0) as f64,
            _ => v0,
        }
    };
    if let Some(bend) = pitch::bend_lane(&mut part.notes, &curves, &tempo, &mode1_at, mode1_span) {
        part.vocal.lanes.insert(Lane::PitchBend, bend);
    }
    for (lane, points) in [
        (Lane::Dynamics, dynamics),
        (Lane::Gender, gender),
        (Lane::Breathiness, breath),
    ] {
        let curve = LaneCurve::from_samples(points, 0.0);
        if !curve.is_empty() {
            part.vocal.lanes.insert(lane, curve);
        }
    }
    song.parts.push(part);
    song.tidy();
    if song.parts.is_empty() {
        return Err(format!("there are no notes in {name}"));
    }
    Ok(song)
}

/// UTF-8 when it is (a mark dropped), else Shift-JIS — what UTAU writes.
fn decode(bytes: &[u8]) -> String {
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_string(),
        Err(_) => encoding_rs::SHIFT_JIS.decode(bytes).0.into_owned(),
    }
}

/// `[#HEAD]` sections and their `key=value` lines, in file order.
fn sections(text: &str) -> Vec<(String, BTreeMap<String, String>)> {
    let mut out: Vec<(String, BTreeMap<String, String>)> = Vec::new();
    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        if let Some(head) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            out.push((head.to_string(), BTreeMap::new()));
            continue;
        }
        if let (Some((key, value)), Some(last)) = (line.split_once('='), out.last_mut()) {
            last.1.insert(key.trim().to_string(), value.to_string());
        }
    }
    out
}

/// A number, a decimal comma allowed (a UST saved in a European locale).
fn number(text: &str) -> Option<f64> {
    let t = text.trim();
    t.parse::<f64>()
        .ok()
        .or_else(|| t.replace(',', ".").parse::<f64>().ok())
        .filter(|v| v.is_finite())
}

/// `"a か"` or `"- か"` → `"か"`: the context a VCV alias carries.
fn strip_context(lyric: &str) -> String {
    let lyric = lyric.trim();
    if let Some((head, rest)) = lyric.split_once(' ')
        && !rest.trim().is_empty()
        && head.chars().count() <= 2
        && head
            .chars()
            .all(|c| c == '-' || c.is_ascii_alphabetic() || c == '\u{3093}')
    {
        return rest.trim().to_string();
    }
    lyric.to_string()
}

/// `VBR=length%,cycle ms,depth cents,in%,out%,phase%,height%,…`.
fn vibrato(text: &str) -> Option<Vibrato> {
    let v: Vec<f64> = text.split(',').map(|x| number(x).unwrap_or(0.0)).collect();
    let (length, cycle, depth) = (*v.first()?, *v.get(1)?, *v.get(2)?);
    if length <= 0.0 || depth <= 0.0 || cycle <= 0.0 {
        return None;
    }
    Some(Vibrato {
        depth: depth as f32,
        rate: (1000.0 / cycle) as f32,
        delay: (1.0 - length / 100.0).clamp(0.0, 1.0) as f32,
        fade_in: (v.get(3).copied().unwrap_or(0.0) / 100.0).clamp(0.0, 1.0) as f32,
        fade_out: (v.get(4).copied().unwrap_or(0.0) / 100.0).clamp(0.0, 1.0) as f32,
    })
}

/// Mode2's points: `PBS=ms;y` starts it (no `y`: from the note before),
/// each `PBW` width moves on to the next `PBY` height (tenths of a
/// semitone; past the list, 0), each stretch drawn as its `PBM` says.
fn mode2_points(pbs: &str, pbw: &str, pby: &str, pbm: &str, snap: Option<f64>) -> Vec<Point> {
    let mut parts = pbs.split(';');
    let Some(x0) = parts.next().and_then(number) else {
        return Vec::new();
    };
    let y0 = parts
        .next()
        .and_then(number)
        .map(|y| y * 10.0)
        .or(snap)
        .unwrap_or(0.0);
    let shapes: Vec<Shape> = pbm.split(',').map(Shape::from_pbm).collect();
    let heights: Vec<f64> = pby.split(',').map(|y| number(y).unwrap_or(0.0)).collect();
    let mut points = vec![Point {
        ms: x0,
        cents: y0,
        shape: shapes.first().copied().unwrap_or(Shape::Io),
    }];
    let mut x = x0;
    for (i, w) in pbw.split(',').enumerate() {
        let Some(w) = number(w) else {
            continue;
        };
        x += w;
        points.push(Point {
            ms: x,
            cents: heights.get(i).copied().unwrap_or(0.0) * 10.0,
            shape: shapes.get(i + 1).copied().unwrap_or(Shape::Io),
        });
    }
    points
}

/// The `g` and `B` flags' values (0 and 50 when absent) and every other
/// flag letter.
fn read_flags(flags: &str) -> (f64, f64, Vec<String>) {
    let (mut g, mut b) = (0.0, 50.0);
    let mut others = Vec::new();
    let chars: Vec<char> = flags.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        i += 1;
        if !c.is_ascii_alphabetic() {
            continue;
        }
        let mut digits = String::new();
        while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '-' || chars[i] == '+') {
            digits.push(chars[i]);
            i += 1;
        }
        let value = digits.parse::<f64>().ok();
        match (c, value) {
            ('g', Some(v)) => g = v.clamp(-100.0, 100.0),
            ('B', Some(v)) => b = v.clamp(0.0, 100.0),
            _ => {
                let flag = c.to_string();
                if !others.contains(&flag) {
                    others.push(flag);
                }
            }
        }
    }
    (g, b, others)
}
