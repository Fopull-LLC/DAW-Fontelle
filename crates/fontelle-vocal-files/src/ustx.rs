//! OpenUtau's `.ustx` (YAML), read and written.
//!
//! What maps, and to what:
//!
//! - Each **track**'s voice parts → one part (notes placed by each voice
//!   part's position; 480 a quarter in the file).
//! - `tempos` (or an older file's one `bpm`), `time_signatures` (or
//!   `beat_per_bar`/`beat_unit`) → the tempo map and the bar.
//! - A note's `lyric` (`+`, OpenUtau's extender, is the `-` slur here),
//!   `tone`, `vibrato` (length %, period ms, depth cents, in/out %), the
//!   `vel` phoneme expression → velocity as consonant speed.
//! - A note's `pitch` points: two flat points snapped to the note before
//!   are a glide (`Note::voice` portamento, OpenUtau's own default being
//!   the backend's own); anything else is drawn into the pitch-bend lane
//!   (`crate::pitch`).
//! - `phoneme_overrides` naming every phoneme of a note from the first →
//!   hand-placed phonemes (OpenUtau's timing deltas are relative to its
//!   own phonemizer's placement, and are named in `unmapped`).
//! - Curves: `pitd` → pitch bend (cents), `dyn` → dynamics (tenths of a
//!   dB), `bre`/`brec` → breathiness, `voic` → voicing, `ten`/`tenc` →
//!   tension, `gen`/`genc` → gender (±100 → ±600 cents). Any other curve is
//!   kept as OpenUtau's own lane (`VocalPart::extra["openutau"]`) and
//!   written back out as it came.

use std::collections::BTreeMap;

use fontelle_model::{LaneCurve, Note, TempoSegment, consonant_speed, velocity_for_speed};
use fontelle_sing::{Lane, PhonemeEdit, Vibrato};
use fontelle_types::{PPQN, Tick};
use serde::Serialize;
use serde_yaml_ng::Value;

use crate::pitch::{self, NoteCurve, Point, Shape};
use crate::{SongPart, VocalSong, vocal_note};

/// The backend key OpenUtau's own curves are kept under.
pub const OPENUTAU: &str = "openutau";

/// A note's pitch points and whether the first snaps to the note before.
type DrawnPitch = (Vec<Point>, bool);

/// OpenUtau's default glide: 80 ms centred on the note's start.
const DEFAULT_GLIDE: [f64; 2] = [-40.0, 40.0];

/// A standard lane as an OpenUtau curve: its abbreviation, how a lane value
/// becomes the curve's integer, and the curve's own default.
struct CurveMap {
    abbr: &'static str,
    lane: Lane,
    /// curve = lane * scale + offset
    scale: f32,
    offset: f32,
    min: f32,
    max: f32,
    name: &'static str,
}

const CURVES: [CurveMap; 6] = [
    CurveMap {
        abbr: "pitd",
        lane: Lane::PitchBend,
        scale: 1.0,
        offset: 0.0,
        min: -1200.0,
        max: 1200.0,
        name: "pitch deviation",
    },
    CurveMap {
        abbr: "dyn",
        lane: Lane::Dynamics,
        scale: 10.0,
        offset: 0.0,
        min: -240.0,
        max: 120.0,
        name: "dynamics (curve)",
    },
    CurveMap {
        abbr: "brec",
        lane: Lane::Breathiness,
        scale: 100.0,
        offset: 0.0,
        min: -100.0,
        max: 100.0,
        name: "breathiness (curve)",
    },
    CurveMap {
        abbr: "voic",
        lane: Lane::Voicing,
        scale: 100.0,
        offset: 100.0,
        min: 0.0,
        max: 100.0,
        name: "voicing (curve)",
    },
    CurveMap {
        abbr: "tenc",
        lane: Lane::Tension,
        scale: 100.0,
        offset: 0.0,
        min: -100.0,
        max: 100.0,
        name: "tension (curve)",
    },
    CurveMap {
        abbr: "genc",
        lane: Lane::Gender,
        scale: 1.0 / 6.0,
        offset: 0.0,
        min: -100.0,
        max: 100.0,
        name: "gender (curve)",
    },
];

/// The other spellings a file may use for the same lanes.
fn curve_for(abbr: &str) -> Option<&'static CurveMap> {
    let canonical = match abbr {
        "bre" => "brec",
        "ten" => "tenc",
        "gen" => "genc",
        other => other,
    };
    CURVES.iter().find(|c| c.abbr == canonical)
}

fn num(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
    .filter(|x: &f64| x.is_finite())
}

fn field<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    v.as_mapping()?.get(key)
}

fn number(v: &Value, key: &str) -> Option<f64> {
    field(v, key).and_then(num)
}

fn text(v: &Value, key: &str) -> Option<String> {
    match field(v, key)? {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn list<'a>(v: &'a Value, key: &str) -> &'a [Value] {
    field(v, key)
        .and_then(Value::as_sequence)
        .map_or(&[], |s| s.as_slice())
}

pub fn read(text_in: &str, name: &str) -> Result<VocalSong, String> {
    let doc: Value = serde_yaml_ng::from_str(text_in)
        .map_err(|e| format!("{name} is not an OpenUtau project: {e}"))?;
    if doc.as_mapping().is_none() {
        return Err(format!("{name} is not an OpenUtau project"));
    }
    let resolution = number(&doc, "resolution").unwrap_or(480.0).max(1.0);
    let tick = |t: f64| -> Tick { (t * PPQN as f64 / resolution).round() as Tick };

    let mut song = VocalSong::new(
        text(&doc, "name")
            .filter(|n| !n.trim().is_empty())
            .unwrap_or_else(|| name.to_string()),
    );
    let mut tempo: Vec<TempoSegment> = list(&doc, "tempos")
        .iter()
        .filter_map(|t| {
            Some(TempoSegment {
                start_tick: tick(number(t, "position").unwrap_or(0.0)),
                bpm: number(t, "bpm")?,
            })
        })
        .collect();
    if tempo.is_empty() {
        tempo.push(TempoSegment {
            start_tick: 0,
            bpm: number(&doc, "bpm").unwrap_or(120.0),
        });
    }
    song.tempo = tempo;
    let signature = list(&doc, "time_signatures").first();
    let beats = signature
        .and_then(|s| number(s, "beat_per_bar"))
        .or_else(|| number(&doc, "beat_per_bar"))
        .unwrap_or(4.0);
    let unit = signature
        .and_then(|s| number(s, "beat_unit"))
        .or_else(|| number(&doc, "beat_unit"))
        .unwrap_or(4.0)
        .max(1.0);
    song.beat_unit = (unit.round() as u32).max(1);
    song.beats_per_bar = (beats.round() as u32).max(1);
    if list(&doc, "time_signatures").len() > 1 {
        song.unmapped("time signature changes (the first is kept)");
    }
    song.tidy();
    let tempo_map = song.tempo_map();

    let tracks: Vec<(String, Option<String>)> = list(&doc, "tracks")
        .iter()
        .map(|t| {
            (
                text(t, "track_name").unwrap_or_default(),
                text(t, "singer").filter(|s| !s.trim().is_empty()),
            )
        })
        .collect();

    // Voice parts grouped by track, in track order.
    let mut by_track: BTreeMap<i64, Vec<&Value>> = BTreeMap::new();
    for part in list(&doc, "voice_parts") {
        let track = number(part, "track_no").unwrap_or(0.0) as i64;
        by_track.entry(track).or_default().push(part);
    }
    for (track, parts) in by_track {
        let (track_name, singer) = tracks.get(track as usize).cloned().unwrap_or_default();
        let part_name = parts
            .first()
            .and_then(|p| text(p, "name"))
            .unwrap_or_default();
        let mut out = SongPart::new(if track_name.trim().is_empty() {
            if part_name.trim().is_empty() {
                format!("Track {}", track + 1)
            } else {
                part_name
            }
        } else {
            track_name
        });
        out.singer = singer;
        // Notes with their curve points, before they are sorted.
        let mut notes: Vec<(Note, Option<DrawnPitch>)> = Vec::new();
        let mut curves: BTreeMap<String, Vec<(Tick, f32)>> = BTreeMap::new();
        for part in parts {
            let position = number(part, "position").unwrap_or(0.0);
            for n in list(part, "notes") {
                let start = tick(position + number(n, "position").unwrap_or(0.0));
                let length = tick(number(n, "duration").unwrap_or(0.0));
                if length <= 0 {
                    continue;
                }
                let key = number(n, "tone").unwrap_or(60.0).clamp(0.0, 127.0) as u8;
                let mut lyric = text(n, "lyric").unwrap_or_default();
                if lyric.starts_with('+') {
                    lyric = "-".to_string();
                }
                let mut note = vocal_note(start, length, key, lyric);
                read_expressions(&mut song, n, &mut note);
                read_overrides(&mut song, n, &mut note);
                if let Some(v) = field(n, "vibrato").and_then(vibrato) {
                    pitch::edit_voice(&mut note, |voice| voice.vibrato = Some(v));
                }
                let points = field(n, "pitch").map(|p| {
                    let data: Vec<Point> = list(p, "data")
                        .iter()
                        .map(|d| Point {
                            ms: number(d, "x").unwrap_or(0.0),
                            cents: number(d, "y").unwrap_or(0.0) * 10.0,
                            shape: Shape::from_ustx(&text(d, "shape").unwrap_or_default()),
                        })
                        .collect();
                    let snap = field(p, "snap_first")
                        .and_then(Value::as_bool)
                        .unwrap_or(true);
                    (data, snap)
                });
                notes.push((note, points));
            }
            for c in list(part, "curves") {
                let abbr = text(c, "abbr").unwrap_or_default();
                let xs: Vec<f64> = list(c, "xs").iter().filter_map(num).collect();
                let ys: Vec<f64> = list(c, "ys").iter().filter_map(num).collect();
                let points = curves.entry(abbr).or_default();
                points.extend(
                    xs.iter()
                        .zip(&ys)
                        .map(|(x, y)| (tick(position + x), *y as f32)),
                );
            }
        }
        notes.sort_by_key(|(n, _)| n.start);
        // Glides and drawn curves, now the note before each is known.
        let mut note_curves: Vec<NoteCurve> = Vec::new();
        for i in 0..notes.len() {
            let Some((mut points, snap)) = notes[i].1.clone() else {
                continue;
            };
            if points.is_empty() {
                continue;
            }
            let is_default = points.len() == 2
                && snap
                && points
                    .iter()
                    .all(|p| p.cents.abs() < 1e-9 && p.shape == Shape::Io)
                && (points[0].ms - DEFAULT_GLIDE[0]).abs() < 0.5
                && (points[1].ms - DEFAULT_GLIDE[1]).abs() < 0.5;
            if is_default {
                continue;
            }
            if snap && let Some(glide) = pitch::glide_of(&points) {
                pitch::edit_voice(&mut notes[i].0, |v| v.portamento = Some(glide));
                continue;
            }
            if snap {
                let key = notes[i].0.key;
                let previous = i
                    .checked_sub(1)
                    .map(|p| &notes[p].0)
                    .filter(|p| p.start + p.length >= notes[i].0.start);
                if let Some(p) = previous {
                    points[0].cents = (f64::from(p.key) - f64::from(key)) * 100.0;
                }
            }
            note_curves.push(NoteCurve { note: i, points });
        }
        out.notes = notes.into_iter().map(|(n, _)| n).collect();

        let mut pitd: Option<LaneCurve> = None;
        for (abbr, mut points) in curves {
            points.sort_by_key(|p| p.0);
            match curve_for(&abbr) {
                Some(map) => {
                    let default = map.offset;
                    let lane: Vec<(Tick, f32)> = with_default_ends(points, default)
                        .into_iter()
                        .map(|(t, y)| (t, (y - map.offset) / map.scale))
                        .collect();
                    let curve = LaneCurve::from_samples(lane, 0.0);
                    if map.lane == Lane::PitchBend {
                        pitd = Some(curve);
                    } else if !curve.is_empty() {
                        out.vocal.lanes.insert(map.lane, curve);
                    }
                }
                None => {
                    let curve = LaneCurve::from_points(points);
                    if !curve.is_empty() {
                        out.vocal
                            .extra
                            .entry(OPENUTAU.to_string())
                            .or_default()
                            .insert(abbr, curve);
                    }
                }
            }
        }
        let bend = if note_curves.is_empty() {
            pitd.filter(|c| !c.is_empty())
        } else {
            let span = pitd
                .as_ref()
                .and_then(|c| Some((c.points().first()?.0, c.points().last()?.0)));
            let at = |t: Tick| pitd.as_ref().map_or(0.0, |c| f64::from(c.at(t, 0.0)));
            pitch::bend_lane(&mut out.notes, &note_curves, &tempo_map, &at, span)
        };
        if let Some(bend) = bend {
            out.vocal.lanes.insert(Lane::PitchBend, bend);
        }
        song.parts.push(out);
    }
    song.tidy();
    if song.parts.is_empty() {
        return Err(format!("there are no notes in {name}"));
    }
    Ok(song)
}

/// OpenUtau reads a curve as its default outside its points; a lane here
/// holds its ends, so the default is put back either side.
fn with_default_ends(mut points: Vec<(Tick, f32)>, default: f32) -> Vec<(Tick, f32)> {
    let (Some(&first), Some(&last)) = (points.first(), points.last()) else {
        return points;
    };
    if (first.1 - default).abs() > 1e-6 {
        points.insert(0, (first.0 - 1, default));
    }
    if (last.1 - default).abs() > 1e-6 {
        points.push((last.0 + 1, default));
    }
    points
}

fn vibrato(v: &Value) -> Option<Vibrato> {
    let length = number(v, "length").unwrap_or(0.0);
    let depth = number(v, "depth").unwrap_or(0.0);
    let period = number(v, "period").unwrap_or(0.0);
    if length <= 0.0 || depth <= 0.0 || period <= 0.0 {
        return None;
    }
    Some(Vibrato {
        depth: depth as f32,
        rate: (1000.0 / period) as f32,
        delay: (1.0 - length / 100.0).clamp(0.0, 1.0) as f32,
        fade_in: (number(v, "in").unwrap_or(0.0) / 100.0).clamp(0.0, 1.0) as f32,
        fade_out: (number(v, "out").unwrap_or(0.0) / 100.0).clamp(0.0, 1.0) as f32,
    })
}

/// `vel` on the first phoneme is the note's consonant speed; anything else
/// is OpenUtau's own and is named.
fn read_expressions(song: &mut VocalSong, n: &Value, note: &mut Note) {
    let mut all: Vec<&Value> = list(n, "phoneme_expressions").iter().collect();
    all.extend(list(n, "note_expressions"));
    for e in all {
        let abbr = text(e, "abbr").unwrap_or_default();
        let index = number(e, "index").unwrap_or(0.0) as i64;
        let Some(value) = number(e, "value") else {
            continue;
        };
        match abbr.as_str() {
            "vel" if index == 0 => {
                note.velocity = velocity_for_speed(2f32.powf((value as f32 - 100.0) / 100.0));
            }
            "vel" => song.unmapped(
                "consonant speed (vel) on a note\u{2019}s later phonemes: a note has one speed here",
            ),
            "clr" => song.unmapped("voice colours (clr): choose the colour per note in Fontelle"),
            other => song.unmapped(format!("the per-note expression \u{201c}{other}\u{201d}")),
        }
    }
}

fn read_overrides(song: &mut VocalSong, n: &Value, note: &mut Note) {
    let overrides = list(n, "phoneme_overrides");
    if overrides.is_empty() {
        return;
    }
    let mut named: BTreeMap<i64, String> = BTreeMap::new();
    for o in overrides {
        let index = number(o, "index").unwrap_or(0.0) as i64;
        if let Some(p) = text(o, "phoneme").filter(|p| !p.trim().is_empty()) {
            named.insert(index, p.trim().to_string());
        }
        if ["offset", "preutter_delta", "overlap_delta"]
            .iter()
            .any(|k| number(o, k).is_some_and(|v| v != 0.0))
        {
            song.unmapped(
                "phoneme timing adjustments (OpenUtau's are moves from where its own \
                 phonemizer put them)",
            );
        }
    }
    if named.is_empty() {
        return;
    }
    let contiguous = named.keys().copied().eq(0..named.len() as i64);
    if !contiguous {
        song.unmapped("phonemes overridden one by one (only a note's whole reading is kept)");
        return;
    }
    let phonemes = named
        .into_values()
        .map(|symbol| PhonemeEdit {
            symbol,
            offset: None,
        })
        .collect();
    pitch::edit_voice(note, |v| v.phonemes = Some(phonemes));
}

// ------------------------------------------------------------------ writing

#[derive(Serialize)]
struct Project {
    name: String,
    comment: String,
    output_dir: String,
    cache_dir: String,
    ustx_version: String,
    resolution: i64,
    bpm: f64,
    beat_per_bar: u32,
    beat_unit: u32,
    expressions: BTreeMap<String, Expression>,
    exp_selectors: Vec<String>,
    exp_primary: i32,
    exp_secondary: i32,
    key: i32,
    time_signatures: Vec<TimeSignature>,
    tempos: Vec<Tempo>,
    tracks: Vec<Track>,
    voice_parts: Vec<VoicePart>,
    wave_parts: Vec<Value>,
}

#[derive(Serialize)]
struct Expression {
    name: String,
    abbr: String,
    #[serde(rename = "type")]
    kind: String,
    min: f32,
    max: f32,
    default_value: f32,
    is_flag: bool,
    flag: String,
}

#[derive(Serialize)]
struct TimeSignature {
    bar_position: i64,
    beat_per_bar: u32,
    beat_unit: u32,
}

#[derive(Serialize)]
struct Tempo {
    position: i64,
    bpm: f64,
}

#[derive(Serialize)]
struct Track {
    singer: String,
    phonemizer: String,
    renderer_settings: BTreeMap<String, Value>,
    track_name: String,
    track_color: String,
    mute: bool,
    solo: bool,
    volume: f64,
    pan: f64,
}

#[derive(Serialize)]
struct VoicePart {
    duration: i64,
    name: String,
    comment: String,
    track_no: usize,
    position: i64,
    notes: Vec<UNote>,
    curves: Vec<Curve>,
}

#[derive(Serialize)]
struct UNote {
    position: i64,
    duration: i64,
    tone: u8,
    lyric: String,
    pitch: Pitch,
    vibrato: UVibrato,
    phoneme_expressions: Vec<PhonemeExpression>,
    phoneme_overrides: Vec<PhonemeOverride>,
}

#[derive(Serialize)]
struct Pitch {
    data: Vec<PitchPoint>,
    snap_first: bool,
}

#[derive(Serialize)]
struct PitchPoint {
    x: f64,
    y: f64,
    shape: String,
}

#[derive(Serialize)]
struct UVibrato {
    length: f64,
    period: f64,
    depth: f64,
    #[serde(rename = "in")]
    fade_in: f64,
    out: f64,
    shift: f64,
    drift: f64,
    vol_link: f64,
}

#[derive(Serialize)]
struct PhonemeExpression {
    index: i64,
    abbr: String,
    value: f64,
}

#[derive(Serialize)]
struct PhonemeOverride {
    index: i64,
    phoneme: String,
}

#[derive(Serialize)]
struct Curve {
    xs: Vec<i64>,
    ys: Vec<i64>,
    abbr: String,
}

/// File ticks (480 a quarter) of song ticks.
fn file_ticks(t: Tick) -> i64 {
    (t as f64 * 480.0 / PPQN as f64).round() as i64
}

fn round2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

/// The song as an OpenUtau project: a track and one voice part per part.
pub fn write(song: &VocalSong) -> String {
    let written_beats = song.beats_per_bar.max(1);
    let mut expressions = BTreeMap::new();
    for map in &CURVES {
        expressions.insert(
            map.abbr.to_string(),
            Expression {
                name: map.name.to_string(),
                abbr: map.abbr.to_string(),
                kind: "Curve".to_string(),
                min: map.min,
                max: map.max,
                default_value: map.offset,
                is_flag: false,
                flag: String::new(),
            },
        );
    }
    expressions.insert(
        "vel".to_string(),
        Expression {
            name: "velocity".to_string(),
            abbr: "vel".to_string(),
            kind: "Numerical".to_string(),
            min: 0.0,
            max: 200.0,
            default_value: 100.0,
            is_flag: false,
            flag: String::new(),
        },
    );
    let mut tracks = Vec::new();
    let mut voice_parts = Vec::new();
    for (index, part) in song.parts.iter().enumerate() {
        tracks.push(Track {
            singer: part.singer.clone().unwrap_or_default(),
            phonemizer: "OpenUtau.Core.DefaultPhonemizer".to_string(),
            renderer_settings: BTreeMap::new(),
            track_name: part.name.clone(),
            track_color: "Blue".to_string(),
            mute: false,
            solo: false,
            volume: 0.0,
            pan: 0.0,
        });
        let notes = part.notes.iter().map(write_note).collect();
        let end = file_ticks(part.end());
        let mut curves = Vec::new();
        for map in &CURVES {
            if let Some(lane) = part.vocal.lanes.get(&map.lane) {
                curves.push(write_curve(map.abbr, lane, end, |v| {
                    v * map.scale + map.offset
                }));
            }
        }
        if let Some(own) = part.vocal.extra.get(OPENUTAU) {
            for (abbr, lane) in own {
                curves.push(write_curve(abbr, lane, end, |v| v));
            }
        }
        voice_parts.push(VoicePart {
            duration: end.max(1),
            name: part.name.clone(),
            comment: String::new(),
            track_no: index,
            position: 0,
            notes,
            curves,
        });
    }
    let project = Project {
        name: song.name.clone(),
        comment: String::new(),
        output_dir: "Vocal".to_string(),
        cache_dir: "UCache".to_string(),
        ustx_version: "0.6".to_string(),
        resolution: 480,
        bpm: song.tempo.first().map_or(120.0, |t| t.bpm),
        beat_per_bar: written_beats,
        beat_unit: song.beat_unit.max(1),
        expressions,
        exp_selectors: vec![
            "dyn".into(),
            "pitd".into(),
            "brec".into(),
            "tenc".into(),
            "vel".into(),
        ],
        exp_primary: 0,
        exp_secondary: 1,
        key: 0,
        time_signatures: vec![TimeSignature {
            bar_position: 0,
            beat_per_bar: written_beats,
            beat_unit: song.beat_unit.max(1),
        }],
        tempos: song
            .tempo
            .iter()
            .map(|t| Tempo {
                position: file_ticks(t.start_tick),
                bpm: t.bpm,
            })
            .collect(),
        tracks,
        voice_parts,
        wave_parts: Vec::new(),
    };
    serde_yaml_ng::to_string(&project).unwrap_or_default()
}

fn write_note(note: &Note) -> UNote {
    let voice = note.voice.as_deref();
    let glide = voice.and_then(|v| v.portamento);
    let data: Vec<PitchPoint> = match glide {
        Some(g) => pitch::glide_points(&g)
            .iter()
            .map(|p| PitchPoint {
                x: round2(p.ms),
                y: 0.0,
                shape: p.shape.ustx().to_string(),
            })
            .collect(),
        None => DEFAULT_GLIDE
            .iter()
            .map(|x| PitchPoint {
                x: *x,
                y: 0.0,
                shape: "io".to_string(),
            })
            .collect(),
    };
    let vibrato = match voice.and_then(|v| v.vibrato) {
        Some(v) => UVibrato {
            length: round2(f64::from(1.0 - v.delay) * 100.0),
            period: round2(1000.0 / f64::from(v.rate.max(0.01))),
            depth: round2(f64::from(v.depth)),
            fade_in: round2(f64::from(v.fade_in) * 100.0),
            out: round2(f64::from(v.fade_out) * 100.0),
            shift: 0.0,
            drift: 0.0,
            vol_link: 0.0,
        },
        None => UVibrato {
            length: 0.0,
            period: 175.0,
            depth: 25.0,
            fade_in: 10.0,
            out: 10.0,
            shift: 0.0,
            drift: 0.0,
            vol_link: 0.0,
        },
    };
    let mut phoneme_expressions = Vec::new();
    if note.velocity != 100 {
        let vel = 100.0 + 100.0 * f64::from(consonant_speed(note.velocity)).log2();
        phoneme_expressions.push(PhonemeExpression {
            index: 0,
            abbr: "vel".to_string(),
            value: vel.round().clamp(0.0, 200.0),
        });
    }
    let phoneme_overrides = voice
        .and_then(|v| v.phonemes.as_ref())
        .map(|ps| {
            ps.iter()
                .enumerate()
                .map(|(i, p)| PhonemeOverride {
                    index: i as i64,
                    phoneme: p.symbol.clone(),
                })
                .collect()
        })
        .unwrap_or_default();
    UNote {
        position: file_ticks(note.start),
        duration: file_ticks(note.length),
        tone: note.key,
        lyric: if note.lyric.trim() == "-" {
            "+".to_string()
        } else {
            note.lyric.clone()
        },
        pitch: Pitch {
            data,
            snap_first: true,
        },
        vibrato,
        phoneme_expressions,
        phoneme_overrides,
    }
}

/// A lane's points as a curve; its held ends written out to the part's
/// edges, since OpenUtau reads its default past a curve's last point.
fn write_curve(abbr: &str, lane: &LaneCurve, end: i64, to_file: impl Fn(f32) -> f32) -> Curve {
    let mut points: Vec<(i64, i64)> = lane
        .points()
        .iter()
        .map(|(t, v)| (file_ticks(*t), to_file(*v).round() as i64))
        .collect();
    points.dedup_by_key(|p| p.0);
    if let (Some(&first), Some(&last)) = (points.first(), points.last()) {
        if first.0 > 0 {
            points.insert(0, (0, first.1));
        }
        if last.0 < end {
            points.push((end, last.1));
        }
    }
    Curve {
        xs: points.iter().map(|p| p.0).collect(),
        ys: points.iter().map(|p| p.1).collect(),
        abbr: abbr.to_string(),
    }
}
