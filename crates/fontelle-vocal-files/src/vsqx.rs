//! Vocaloid 3 and 4's `.vsqx` (XML).
//!
//! The two versions name the same things differently (`posTick`/`t`,
//! `noteNum`/`n`, `attr id="DYN"`/`v id="D"`); both are read. Positions in
//! the file count the **pre-measure** (bars before the song's first), which
//! is taken off.
//!
//! What maps, and to what:
//!
//! - Notes, lyrics (`-` is Vocaloid's slur too), velocity (consonant speed:
//!   64 is the voice's own) → notes.
//! - Phonemes the writer **locked** → hand-placed phonemes (Vocaloid's
//!   symbols; a Fontelle voice may not know them). An unlocked reading is
//!   Vocaloid's own guess and is left to Fontelle's phonemizer.
//! - `vibLen`/`vibType` with the `vibDep`/`vibRate` sequences → vibrato
//!   (depth 0..127 → 0..100 cents, rate 0..127 → 4..8 Hz).
//! - Tempo and the first time signature → the tempo map and the bar.
//! - Controllers, each a held value: `PIT` × `PBS` → pitch bend (cents);
//!   `DYN` → dynamics (64 is 0 dB, 127 +6 dB, 0 −24 dB); `BRE` →
//!   breathiness (0..1); `BRI` → tension (bright is pressed); `GEN` →
//!   gender (64 the voice's own, under it younger: ±600 cents). `CLE`,
//!   `OPE`, `POR` have no lane and are named in `unmapped`.

use fontelle_model::{LaneCurve, TempoSegment, velocity_for_speed};
use fontelle_sing::{Lane, PhonemeEdit, Vibrato};
use fontelle_types::{PPQN, Tick};
use roxmltree::{Document, Node};

use crate::pitch;
use crate::{SongPart, VocalSong, vocal_note};

fn child<'a, 'i>(node: Node<'a, 'i>, names: &[&str]) -> Option<Node<'a, 'i>> {
    node.children()
        .find(|c| c.is_element() && names.contains(&c.tag_name().name()))
}

fn children<'a, 'i>(
    node: Node<'a, 'i>,
    names: &'static [&'static str],
) -> impl Iterator<Item = Node<'a, 'i>> {
    node.children()
        .filter(move |c| c.is_element() && names.contains(&c.tag_name().name()))
}

fn text_of(node: Node, names: &[&str]) -> Option<String> {
    let n = child(node, names)?;
    let t: String = n
        .children()
        .filter_map(|c| c.text())
        .collect::<String>()
        .trim()
        .to_string();
    Some(t)
}

fn number_of(node: Node, names: &[&str]) -> Option<f64> {
    text_of(node, names)?
        .parse::<f64>()
        .ok()
        .filter(|x| x.is_finite())
}

pub fn read(text: &str, name: &str) -> Result<VocalSong, String> {
    let doc = Document::parse_with_options(text, xml_options())
        .map_err(|e| format!("{name} is not a VSQX file: {e}"))?;
    let root = doc.root_element();
    if !matches!(root.tag_name().name(), "vsq3" | "vsq4") {
        return Err(format!("{name} is not a VSQX file"));
    }
    let master =
        child(root, &["masterTrack"]).ok_or_else(|| format!("{name} has no master track"))?;
    let resolution = number_of(master, &["resolution"]).unwrap_or(480.0).max(1.0);
    let mut song = VocalSong::new(
        text_of(master, &["seqName"])
            .filter(|n| !n.is_empty() && !n.starts_with("Untitled"))
            .unwrap_or_else(|| name.to_string()),
    );
    let signatures: Vec<Node> = children(master, &["timeSig"]).collect();
    let (beats, unit) = signatures
        .first()
        .map(|s| {
            (
                number_of(*s, &["nu", "nume"]).unwrap_or(4.0),
                number_of(*s, &["de", "denomi"]).unwrap_or(4.0).max(1.0),
            )
        })
        .unwrap_or((4.0, 4.0));
    if signatures.len() > 1 {
        song.unmapped("time signature changes (the first is kept)");
    }
    song.beats_per_bar = (beats.round() as u32).max(1);
    song.beat_unit = (unit.round() as u32).max(1);
    let pre_measure = number_of(master, &["preMeasure"]).unwrap_or(0.0);
    let pre = pre_measure * beats * resolution * 4.0 / unit;
    let tick = |t: f64| -> Tick { ((t - pre) * PPQN as f64 / resolution).round() as Tick };
    song.tempo = children(master, &["tempo"])
        .filter_map(|t| {
            Some(TempoSegment {
                start_tick: tick(number_of(t, &["t", "posTick"])?).max(0),
                bpm: number_of(t, &["v", "bpm"])? / 100.0,
            })
        })
        .collect();
    song.tidy();

    for track in children(root, &["vsTrack"]) {
        let track_name = text_of(track, &["name", "trackName"]).unwrap_or_default();
        let mut part = SongPart::new(if track_name.is_empty() {
            format!("Track {}", song.parts.len() + 1)
        } else {
            track_name
        });
        // Controllers by name: (song tick, value).
        let mut controls: std::collections::BTreeMap<String, Vec<(Tick, f64)>> = Default::default();
        for vs_part in children(track, &["vsPart", "musicalPart"]) {
            let position = number_of(vs_part, &["t", "posTick"]).unwrap_or(pre);
            let at = |t: f64| tick(position + t);
            for cc in children(vs_part, &["cc", "mCtrl"]) {
                let Some(t) = number_of(cc, &["t", "posTick", "p"]) else {
                    continue;
                };
                let Some(v) = child(cc, &["v", "attr"]) else {
                    continue;
                };
                let id = v.attribute("id").unwrap_or_default().to_string();
                let Some(value) = v.text().and_then(|x| x.trim().parse::<f64>().ok()) else {
                    continue;
                };
                controls
                    .entry(long_id(&id))
                    .or_default()
                    .push((at(t), value));
            }
            for n in children(vs_part, &["note"]) {
                let start = at(number_of(n, &["t", "posTick"]).unwrap_or(0.0));
                let length = (number_of(n, &["dur", "durTick"]).unwrap_or(0.0) * PPQN as f64
                    / resolution)
                    .round() as Tick;
                if length <= 0 {
                    continue;
                }
                let key = number_of(n, &["n", "noteNum"])
                    .unwrap_or(60.0)
                    .clamp(0.0, 127.0) as u8;
                let lyric = text_of(n, &["y", "lyric"]).unwrap_or_default();
                let mut note = vocal_note(start, length, key, lyric);
                let velocity = number_of(n, &["v", "velocity"]).unwrap_or(64.0);
                note.velocity = velocity_for_speed(2f32.powf((velocity as f32 - 64.0) / 64.0));
                if let Some(p) = child(n, &["p", "phnms"])
                    && p.attribute("lock") == Some("1")
                {
                    let symbols: Vec<PhonemeEdit> = p
                        .text()
                        .unwrap_or_default()
                        .split_whitespace()
                        .map(|s| PhonemeEdit {
                            symbol: s.to_string(),
                            offset: None,
                        })
                        .collect();
                    if !symbols.is_empty() {
                        pitch::edit_voice(&mut note, |v| v.phonemes = Some(symbols));
                        song.unmapped(
                            "locked phonemes are kept in Vocaloid's symbols \u{2014} a voice \
                             that does not know them sings the lyric instead",
                        );
                    }
                }
                if let Some(style) = child(n, &["nStyle", "noteStyle"])
                    && let Some(v) = note_vibrato(style)
                {
                    pitch::edit_voice(&mut note, |voice| voice.vibrato = Some(v));
                }
                part.notes.push(note);
            }
        }
        part.notes.sort_by_key(|n| n.start);
        let range = controls.get("PBS").cloned().unwrap_or_default();
        for (id, mut points) in controls {
            points.sort_by_key(|p| p.0);
            let (lane, value): (Lane, Box<dyn Fn(f64) -> f32>) = match id.as_str() {
                "DYN" => (
                    Lane::Dynamics,
                    Box::new(|v| {
                        if v >= 64.0 {
                            ((v - 64.0) / 63.0 * 6.0) as f32
                        } else {
                            ((v - 64.0) / 64.0 * 24.0) as f32
                        }
                    }),
                ),
                "BRE" => (Lane::Breathiness, Box::new(|v| (v / 127.0) as f32)),
                "BRI" => (Lane::Tension, Box::new(|v| ((v - 64.0) / 64.0) as f32)),
                "GEN" => (
                    Lane::Gender,
                    Box::new(|v| (-(v - 64.0) / 64.0 * 600.0) as f32),
                ),
                "PIT" => {
                    if let Some(curve) = pitch_lane(&points, &range) {
                        part.vocal.lanes.insert(Lane::PitchBend, curve);
                    }
                    continue;
                }
                "PBS" => continue,
                other => {
                    song.unmapped(format!("the {other} controller"));
                    continue;
                }
            };
            let curve = held(points.iter().map(|(t, v)| (*t, value(*v))).collect());
            if !curve.is_empty() {
                part.vocal.lanes.insert(lane, curve);
            }
        }
        if !part.notes.is_empty() {
            song.parts.push(part);
        }
    }
    song.tidy();
    if song.parts.is_empty() {
        return Err(format!("there are no notes in {name}"));
    }
    Ok(song)
}

/// Vocaloid 4's one-letter controller names, as Vocaloid 3 spells them.
fn long_id(id: &str) -> String {
    match id {
        "D" => "DYN",
        "B" => "BRE",
        "R" => "BRI",
        "C" => "CLE",
        "G" => "GEN",
        "S" => "PBS",
        "P" => "PIT",
        "O" => "OPE",
        "W" => "POR",
        other => other,
    }
    .to_string()
}

/// A controller's events are held until the next: each change is a step.
fn held(points: Vec<(Tick, f32)>) -> LaneCurve {
    // Before the first event the controller is at its default, which every
    // mapping here makes the lane's neutral.
    let mut out: Vec<(Tick, f32)> = Vec::with_capacity(points.len() * 2);
    let mut before = 0.0f32;
    for (t, v) in points {
        if (before - v).abs() > 1e-6 {
            out.push((t - 1, before));
        }
        out.push((t, v));
        before = v;
    }
    LaneCurve::from_samples(out, 0.0)
}

/// PIT (−8192..8191) at the bend range PBS (semitones) in force then.
fn pitch_lane(pit: &[(Tick, f64)], pbs: &[(Tick, f64)]) -> Option<LaneCurve> {
    let range_at = |t: Tick| {
        pbs.iter()
            .rev()
            .find(|(at, _)| *at <= t)
            .map_or(2.0, |(_, r)| *r)
    };
    let points: Vec<(Tick, f32)> = pit
        .iter()
        .map(|(t, v)| (*t, (v / 8192.0 * range_at(*t) * 100.0) as f32))
        .collect();
    let curve = held(points);
    (!curve.is_empty()).then_some(curve)
}

fn note_vibrato(style: Node) -> Option<Vibrato> {
    let style_value = |id: &str| {
        children(style, &["v", "attr"])
            .find(|v| v.attribute("id") == Some(id))
            .and_then(|v| v.text())
            .and_then(|t| t.trim().parse::<f64>().ok())
    };
    let length = style_value("vibLen").unwrap_or(0.0);
    let kind = style_value("vibType").unwrap_or(0.0);
    if length <= 0.0 || kind <= 0.0 {
        return None;
    }
    let first_of = |id: &str| {
        children(style, &["seq", "seqAttr"])
            .find(|s| s.attribute("id") == Some(id))
            .and_then(|s| children(s, &["cc", "elem"]).next())
            .and_then(|cc| number_of(cc, &["v", "elv"]))
    };
    let depth = first_of("vibDep").unwrap_or(64.0);
    let rate = first_of("vibRate").unwrap_or(64.0);
    Some(Vibrato {
        depth: (depth / 127.0 * 100.0) as f32,
        rate: (4.0 + rate / 127.0 * 4.0) as f32,
        delay: (1.0 - length / 100.0).clamp(0.0, 1.0) as f32,
        fade_in: 0.2,
        fade_out: 0.1,
    })
}

/// Notation programs write a DOCTYPE; it is read past, not fetched.
fn xml_options() -> roxmltree::ParsingOptions {
    roxmltree::ParsingOptions {
        allow_dtd: true,
        ..roxmltree::ParsingOptions::default()
    }
}
