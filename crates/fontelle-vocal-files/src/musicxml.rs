//! MusicXML (`score-partwise`), plain and compressed (`.mxl`).
//!
//! Which part: **every part with words** comes in, each its own line, in
//! the first voice that sings in it (`<voice>`; another voice in the same
//! staff is a second line nobody asked for); a score with no words at all
//! brings its first part. Parts without words are named in `unmapped`.
//!
//! What maps, and to what:
//!
//! - Pitched notes (step, alter, octave) → notes; rests are gaps; a chord's
//!   later notes are left (the written-first one sings); grace notes have
//!   no time of their own and are left.
//! - Ties (`<tie>`/`<tied>`) → one note.
//! - Lyrics (verse 1): `syllabic` begin/middle keep a trailing hyphen, as
//!   typed lyrics do (`"Hap-" "py"`); a note with no word under a slur, or
//!   after an `<extend/>`, is the `-` slur.
//! - Tempo (`<sound tempo>`, else a metronome mark) and the first time
//!   signature (the bar counts quarters here: 6/8 is three).

use fontelle_model::TempoSegment;
use fontelle_types::{PPQN, Tick};
use roxmltree::{Document, Node};

use crate::{SongPart, VocalSong, vocal_note};

fn child<'a, 'i>(node: Node<'a, 'i>, name: &str) -> Option<Node<'a, 'i>> {
    node.children()
        .find(|c| c.is_element() && c.tag_name().name() == name)
}

fn text_of(node: Node, name: &str) -> Option<String> {
    child(node, name)
        .and_then(|n| n.text())
        .map(|t| t.trim().to_string())
}

fn number_of(node: Node, name: &str) -> Option<f64> {
    text_of(node, name)?
        .parse::<f64>()
        .ok()
        .filter(|x| x.is_finite())
}

/// Whether `text` is a MusicXML score.
pub fn looks_like_musicxml(text: &str) -> bool {
    let head: String = text.chars().take(4096).collect();
    head.contains("<score-partwise") || head.contains("<score-timewise")
}

pub fn read_mxl(bytes: &[u8], name: &str) -> Result<VocalSong, String> {
    let files = crate::zip::read(bytes).map_err(|e| format!("{name}: {e}"))?;
    let container = files
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case("META-INF/container.xml"))
        .map(|(_, data)| String::from_utf8_lossy(data).into_owned());
    let root = container.and_then(|c| {
        let doc = Document::parse_with_options(&c, xml_options()).ok()?;
        doc.descendants()
            .find(|n| n.has_tag_name("rootfile"))
            .and_then(|n| n.attribute("full-path"))
            .map(str::to_string)
    });
    let score = match root {
        Some(path) => files.iter().find(|(n, _)| *n == path),
        None => files.iter().find(|(n, _)| {
            !n.starts_with("META-INF") && (n.ends_with(".xml") || n.ends_with(".musicxml"))
        }),
    }
    .ok_or_else(|| format!("{name} holds no score"))?;
    read(&crate::text(&score.1), name)
}

/// A sounding note as it is read, before ties are joined.
struct Read {
    start: Tick,
    length: Tick,
    key: u8,
    lyric: Option<String>,
    /// Ends a word's syllable with more to come.
    hyphen: bool,
    tie_start: bool,
    tie_stop: bool,
    slurred: bool,
}

pub fn read(text: &str, name: &str) -> Result<VocalSong, String> {
    let doc = Document::parse_with_options(text, xml_options())
        .map_err(|e| format!("{name} is not MusicXML: {e}"))?;
    let root = doc.root_element();
    match root.tag_name().name() {
        "score-partwise" => {}
        "score-timewise" => {
            return Err(format!(
                "{name} is a time-wise score \u{2014} save it part-wise to bring it in"
            ));
        }
        _ => return Err(format!("{name} is not MusicXML")),
    }
    let title = child(root, "work")
        .and_then(|w| text_of(w, "work-title"))
        .or_else(|| text_of(root, "movement-title"))
        .filter(|t| !t.is_empty());
    let mut song = VocalSong::new(title.unwrap_or_else(|| name.to_string()));
    let names: Vec<(String, String)> = child(root, "part-list")
        .map(|list| {
            list.children()
                .filter(|c| c.has_tag_name("score-part"))
                .map(|p| {
                    (
                        p.attribute("id").unwrap_or_default().to_string(),
                        text_of(p, "part-name").unwrap_or_default(),
                    )
                })
                .collect()
        })
        .unwrap_or_default();

    let mut tempo: Vec<TempoSegment> = Vec::new();
    let mut signature: Option<(f64, f64)> = None;
    let mut read_parts: Vec<(String, Vec<Read>, bool)> = Vec::new();
    for part in root.children().filter(|c| c.has_tag_name("part")) {
        let id = part.attribute("id").unwrap_or_default();
        let part_name = names
            .iter()
            .find(|(i, _)| i == id)
            .map(|(_, n)| n.clone())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| format!("Part {}", read_parts.len() + 1));
        // The voice that sings: the first with a lyric, else the first.
        let mut voice: Option<String> = None;
        for note in part.descendants().filter(|n| n.has_tag_name("note")) {
            let v = text_of(note, "voice").unwrap_or_else(|| "1".into());
            if child(note, "lyric").is_some() {
                voice = Some(v);
                break;
            }
            voice.get_or_insert(v);
        }
        let voice = voice.unwrap_or_else(|| "1".into());
        let sings = part
            .descendants()
            .any(|n| n.has_tag_name("lyric") && n.descendants().any(|t| t.has_tag_name("text")));

        let mut divisions = 1.0f64;
        let mut at = 0.0f64; // quarters from the top
        let mut notes: Vec<Read> = Vec::new();
        let mut slur_depth = 0i32;
        let mut extending = false;
        let mut voices_seen: Vec<String> = Vec::new();
        for measure in part.children().filter(|c| c.has_tag_name("measure")) {
            let measure_start = at;
            let mut furthest = at;
            for item in measure.children().filter(|c| c.is_element()) {
                match item.tag_name().name() {
                    "attributes" => {
                        if let Some(d) = number_of(item, "divisions") {
                            divisions = d.max(1e-9);
                        }
                        if let Some(time) = child(item, "time") {
                            let beats = text_of(time, "beats")
                                .and_then(|b| {
                                    b.split('+')
                                        .map(|x| x.trim().parse::<f64>().ok())
                                        .sum::<Option<f64>>()
                                })
                                .unwrap_or(4.0);
                            let unit = number_of(time, "beat-type").unwrap_or(4.0).max(1.0);
                            match signature {
                                None => signature = Some((beats, unit)),
                                Some(s) if s != (beats, unit) => {
                                    song.unmapped("time signature changes (the first is kept)")
                                }
                                _ => {}
                            }
                        }
                    }
                    "direction" | "sound" => {
                        let sound = if item.has_tag_name("sound") {
                            Some(item)
                        } else {
                            item.descendants().find(|n| n.has_tag_name("sound"))
                        };
                        let bpm = sound
                            .and_then(|s| s.attribute("tempo"))
                            .and_then(|t| t.parse::<f64>().ok())
                            .or_else(|| metronome(item));
                        if let Some(bpm) = bpm.filter(|b| *b > 0.0) {
                            let tick = (at * PPQN as f64).round() as Tick;
                            if !tempo.iter().any(|t| t.start_tick == tick) {
                                tempo.push(TempoSegment {
                                    start_tick: tick,
                                    bpm,
                                });
                            }
                        }
                    }
                    "backup" => at -= number_of(item, "duration").unwrap_or(0.0) / divisions,
                    "forward" => at += number_of(item, "duration").unwrap_or(0.0) / divisions,
                    "note" => {
                        if child(item, "grace").is_some() {
                            continue;
                        }
                        let duration = number_of(item, "duration").unwrap_or(0.0) / divisions;
                        let chord = child(item, "chord").is_some();
                        let start = if chord {
                            notes.last().map_or(at, |n| n.start as f64 / PPQN as f64)
                        } else {
                            at
                        };
                        if !chord {
                            at += duration;
                        }
                        furthest = furthest.max(at);
                        let this_voice = text_of(item, "voice").unwrap_or_else(|| "1".into());
                        if !voices_seen.contains(&this_voice) {
                            voices_seen.push(this_voice.clone());
                        }
                        if this_voice != voice || chord {
                            continue;
                        }
                        let notations = child(item, "notations");
                        let slur = |kind: &str| {
                            notations.is_some_and(|n| {
                                n.children().any(|s| {
                                    s.has_tag_name("slur") && s.attribute("type") == Some(kind)
                                })
                            })
                        };
                        let starts_slur = slur("start");
                        let stops_slur = slur("stop");
                        let tie = |kind: &str| {
                            item.children()
                                .any(|t| t.has_tag_name("tie") && t.attribute("type") == Some(kind))
                                || notations.is_some_and(|n| {
                                    n.children().any(|t| {
                                        t.has_tag_name("tied") && t.attribute("type") == Some(kind)
                                    })
                                })
                        };
                        let Some(pitch) = child(item, "pitch") else {
                            // A rest ends a melisma.
                            extending = false;
                            continue;
                        };
                        let step = text_of(pitch, "step").unwrap_or_else(|| "C".into());
                        let alter = number_of(pitch, "alter").unwrap_or(0.0);
                        let octave = number_of(pitch, "octave").unwrap_or(4.0);
                        let base = match step.as_str() {
                            "C" => 0,
                            "D" => 2,
                            "E" => 4,
                            "F" => 5,
                            "G" => 7,
                            "A" => 9,
                            "B" => 11,
                            _ => 0,
                        };
                        let key = (12.0 * (octave + 1.0) + f64::from(base) + alter)
                            .round()
                            .clamp(0.0, 127.0) as u8;
                        let lyric = item
                            .children()
                            .filter(|l| l.has_tag_name("lyric"))
                            .find(|l| matches!(l.attribute("number"), None | Some("1")))
                            .or_else(|| item.children().find(|l| l.has_tag_name("lyric")));
                        let (word, hyphen, extend) = match lyric {
                            Some(l) => {
                                let word: String = l
                                    .children()
                                    .filter(|t| t.has_tag_name("text"))
                                    .filter_map(|t| t.text())
                                    .collect::<Vec<_>>()
                                    .join("");
                                let syllabic = text_of(l, "syllabic").unwrap_or_default();
                                (
                                    Some(word.trim().to_string()).filter(|w| !w.is_empty()),
                                    matches!(syllabic.as_str(), "begin" | "middle"),
                                    child(l, "extend").is_some(),
                                )
                            }
                            None => (None, false, false),
                        };
                        let slurred = slur_depth > 0 || extending;
                        if starts_slur {
                            slur_depth += 1;
                        }
                        if stops_slur {
                            slur_depth = (slur_depth - 1).max(0);
                        }
                        if word.is_some() {
                            extending = extend;
                        }
                        notes.push(Read {
                            start: (start * PPQN as f64).round() as Tick,
                            length: (duration * PPQN as f64).round() as Tick,
                            key,
                            lyric: word,
                            hyphen,
                            tie_start: tie("start"),
                            tie_stop: tie("stop"),
                            slurred,
                        });
                    }
                    _ => {}
                }
            }
            // A measure ends where its longest voice does.
            at = furthest.max(measure_start);
        }
        if voices_seen.len() > 1 {
            song.unmapped(format!(
                "the other voices of \u{201c}{part_name}\u{201d} (its first sung voice is kept)"
            ));
        }
        read_parts.push((part_name, notes, sings));
    }
    let any_sings = read_parts.iter().any(|(_, _, s)| *s);
    for (index, (part_name, notes, sings)) in read_parts.into_iter().enumerate() {
        if (any_sings && !sings) || (!any_sings && index > 0) {
            song.unmapped(format!(
                "the part \u{201c}{part_name}\u{201d}, which has no words"
            ));
            continue;
        }
        let mut part = SongPart::new(part_name);
        let mut tied = false;
        for n in notes {
            // A tie's later notes lengthen the first.
            if tied
                && n.tie_stop
                && let Some(last) = part.notes.last_mut()
                && last.key == n.key
            {
                last.length = n.start + n.length - last.start;
                tied = n.tie_start;
                continue;
            }
            tied = n.tie_start;
            let lyric = match n.lyric {
                Some(w) if n.hyphen => format!("{w}-"),
                Some(w) => w,
                None if n.slurred => "-".to_string(),
                None => String::new(),
            };
            part.notes
                .push(vocal_note(n.start, n.length.max(1), n.key, lyric));
        }
        song.parts.push(part);
    }
    if let Some((beats, unit)) = signature {
        song.beat_unit = unit as u32;
        song.beats_per_bar = ((beats * 4.0 / unit).round() as u32).max(1);
    }
    tempo.sort_by_key(|t| t.start_tick);
    if !tempo.is_empty() {
        if tempo[0].start_tick > 0 {
            tempo[0].start_tick = 0;
        }
        song.tempo = tempo;
    }
    song.tidy();
    if song.parts.is_empty() {
        return Err(format!("there are no notes in {name}"));
    }
    Ok(song)
}

/// A metronome mark's tempo in quarters a minute.
fn metronome(direction: Node) -> Option<f64> {
    let mark = direction
        .descendants()
        .find(|n| n.has_tag_name("metronome"))?;
    let per_minute: f64 = text_of(mark, "per-minute")?.parse().ok()?;
    let unit = match text_of(mark, "beat-unit")?.as_str() {
        "whole" => 4.0,
        "half" => 2.0,
        "quarter" => 1.0,
        "eighth" => 0.5,
        "16th" => 0.25,
        _ => 1.0,
    };
    let dotted = mark.children().any(|c| c.has_tag_name("beat-unit-dot"));
    Some(per_minute * unit * if dotted { 1.5 } else { 1.0 })
}

/// Notation programs write a DOCTYPE; it is read past, not fetched.
fn xml_options() -> roxmltree::ParsingOptions {
    roxmltree::ParsingOptions {
        allow_dtd: true,
        ..roxmltree::ParsingOptions::default()
    }
}
