//! SFZ import (`docs/flopsynth-next.md` §4.3): the subset every library
//! uses, read into a [`UserSample`] — one zone per `<region>`, with the
//! velocity window, the trim and the loop the file gives it.
//!
//! The subset: `<region>` (and `<group>`/`<global>`/`<master>` headers,
//! whose opcodes carry into the regions under them), `sample`,
//! `lokey`/`hikey`/`key`, `pitch_keycenter`, `lovel`/`hivel`,
//! `loop_mode`/`loop_start`/`loop_end`, `tune`, `volume`, and
//! `default_path` from `<control>`. Everything else is skipped without
//! complaint: a library's envelopes, filters and controllers are the
//! patch's business here, not the file's.

use std::collections::HashMap;
use std::path::Path;

use fontelle_core::{SampleZone, UserSample};

use crate::audio_import::import_audio;
use crate::sf2_import::ImportError;

/// Reads `path` as an SFZ file into a [`UserSample`] named after the file.
///
/// Each region's `sample` is resolved relative to the file's own folder
/// (and `<control>`'s `default_path`), decoded through [`import_audio`]
/// and folded to mono. A region whose sample cannot be read is skipped
/// with the reason on `stderr`; a file with no readable region is an
/// error, since a multisample of nothing is not one.
pub fn import_sfz(path: &Path) -> Result<UserSample, ImportError> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| ImportError(format!("could not read {}: {e}", path.display())))?;
    let folder = path.parent().map(Path::to_path_buf).unwrap_or_default();
    let name = path
        .file_stem()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "SFZ".to_string());

    let mut zones = Vec::new();
    let mut failed = Vec::new();
    let mut default_path = String::new();
    // The opcodes in force at each level, innermost last: a region's own
    // over its group's over the master's over the global's.
    let mut global: HashMap<String, String> = HashMap::new();
    let mut master: HashMap<String, String> = HashMap::new();
    let mut group: HashMap<String, String> = HashMap::new();
    let mut region: Option<HashMap<String, String>> = None;
    let mut control = false;

    let finish_region = |region: &mut Option<HashMap<String, String>>,
                         zones: &mut Vec<SampleZone>,
                         failed: &mut Vec<String>,
                         layers: [&HashMap<String, String>; 3],
                         default_path: &str| {
        let Some(own) = region.take() else {
            return;
        };
        let mut opcodes: HashMap<String, String> = HashMap::new();
        for layer in layers {
            opcodes.extend(layer.iter().map(|(k, v)| (k.clone(), v.clone())));
        }
        opcodes.extend(own);
        match zone_from(&opcodes, &folder, default_path) {
            Ok(Some(zone)) => zones.push(zone),
            Ok(None) => {}
            Err(why) => failed.push(why),
        }
    };

    for raw in text.lines() {
        // A comment runs to the end of the line.
        let line = raw.split("//").next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        // Headers and opcodes may share a line: `<region> sample=a.wav`.
        let mut rest = line;
        while !rest.is_empty() {
            rest = rest.trim_start();
            if let Some(after) = rest.strip_prefix('<') {
                let Some((header, tail)) = after.split_once('>') else {
                    break;
                };
                finish_region(
                    &mut region,
                    &mut zones,
                    &mut failed,
                    [&global, &master, &group],
                    &default_path,
                );
                control = false;
                match header.trim() {
                    "region" => region = Some(HashMap::new()),
                    "group" => group.clear(),
                    "master" => master.clear(),
                    "global" => global.clear(),
                    "control" => control = true,
                    _ => {}
                }
                rest = tail;
                continue;
            }
            // An opcode: `name=value`, the value running to the next
            // `name=` or the end — a sample's name may carry spaces.
            let Some((key, after)) = rest.split_once('=') else {
                break;
            };
            let key = key.trim().to_string();
            let value_end = next_opcode_at(after).unwrap_or(after.len());
            let value = after[..value_end].trim().to_string();
            rest = &after[value_end..];
            if control {
                if key == "default_path" {
                    default_path = value;
                }
                continue;
            }
            let target = match &mut region {
                Some(own) => own,
                None => &mut group,
            };
            // Before a group header the opcodes are the global's; `<group>`
            // resets the group, and a `<master>` the master.
            target.insert(key, value);
        }
    }
    finish_region(
        &mut region,
        &mut zones,
        &mut failed,
        [&global, &master, &group],
        &default_path,
    );
    for why in &failed {
        eprintln!("sfz {}: {why}", path.display());
    }
    if zones.is_empty() {
        return Err(ImportError(format!(
            "{} has no region whose sample could be read{}",
            path.display(),
            failed
                .first()
                .map(|why| format!(" ({why})"))
                .unwrap_or_default()
        )));
    }
    // By key, then by velocity, so a chooser lists them as a keyboard.
    zones.sort_by_key(|zone| (zone.key_range.0, zone.vel_range.0));
    Ok(UserSample {
        name,
        factory: None,
        zones,
    })
}

/// Where the next `name=` starts in `text`, if one does: the start of the
/// word before the next `=` — so a value runs up to the next opcode.
fn next_opcode_at(text: &str) -> Option<usize> {
    let equals = text.find('=')?;
    let word_start = text[..equals]
        .rfind(char::is_whitespace)
        .map_or(0, |i| i + 1);
    // A `=` with no word before it is part of the value.
    (word_start < equals).then_some(word_start)
}

/// One region's opcodes as a zone: `None` for a region with no sample.
fn zone_from(
    opcodes: &HashMap<String, String>,
    folder: &Path,
    default_path: &str,
) -> Result<Option<SampleZone>, String> {
    let Some(sample) = opcodes.get("sample") else {
        return Ok(None);
    };
    let file = folder
        .join(default_path.replace('\\', "/"))
        .join(sample.replace('\\', "/"));
    let decoded = import_audio(&file).map_err(|e| format!("{}: {e}", file.display()))?;
    if decoded.frames == 0 {
        return Err(format!("{}: no sound in it", file.display()));
    }
    let channels = decoded.channels.max(1) as usize;
    let samples: Vec<f32> = decoded
        .samples
        .chunks(channels)
        .map(|frame| frame.iter().sum::<f32>() / channels as f32)
        .collect();
    let number = |key: &str| -> Option<f32> { opcodes.get(key).and_then(|v| note_or_number(v)) };
    let key = number("key").map(|k| k as u8);
    let lokey = number("lokey")
        .map(|k| k.clamp(0.0, 127.0) as u8)
        .or(key)
        .unwrap_or(0);
    let hikey = number("hikey")
        .map(|k| k.clamp(0.0, 127.0) as u8)
        .or(key)
        .unwrap_or(127);
    let root_key = number("pitch_keycenter")
        .map(|k| k.clamp(0.0, 127.0) as u8)
        .or(key)
        .unwrap_or(60);
    let lovel = number("lovel").map_or(0, |v| v.clamp(0.0, 127.0) as u8);
    let hivel = number("hivel").map_or(127, |v| v.clamp(0.0, 127.0) as u8);
    let tune = number("tune").unwrap_or(0.0);
    let volume = number("volume").unwrap_or(0.0);
    let loop_mode = opcodes
        .get("loop_mode")
        .map(String::as_str)
        .unwrap_or("no_loop");
    let loop_frames = match (loop_mode, number("loop_start"), number("loop_end")) {
        ("loop_continuous" | "loop_sustain", from, to) => {
            // As the file has them: the last frame of the loop, inclusive.
            let from = from.unwrap_or(0.0).max(0.0) as u32;
            let last = decoded.frames.saturating_sub(1) as u32;
            let to = to.map_or(last, |t| t.max(0.0) as u32).min(last);
            (to > from).then_some((from, to))
        }
        _ => None,
    };
    Ok(Some(SampleZone {
        name: file
            .file_stem()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        root_key,
        fine_cents: tune,
        key_range: (lokey.min(hikey), hikey.max(lokey)),
        sample_rate: decoded.sample_rate,
        samples: samples.into(),
        vel_range: (lovel.min(hivel), hivel.max(lovel)),
        gain_db: volume,
        loop_frames,
    }))
}

/// An SFZ number, or a note name such as `c4` or `F#3` (middle C is 60).
fn note_or_number(text: &str) -> Option<f32> {
    let text = text.trim();
    if let Ok(number) = text.parse::<f32>() {
        return Some(number);
    }
    let mut chars = text.chars();
    let letter = chars.next()?.to_ascii_lowercase();
    let class = match letter {
        'c' => 0,
        'd' => 2,
        'e' => 4,
        'f' => 5,
        'g' => 7,
        'a' => 9,
        'b' => 11,
        _ => return None,
    };
    let rest: String = chars.collect();
    let (accidental, octave) = match rest.chars().next() {
        Some('#') => (1, &rest[1..]),
        Some('b') => (-1, &rest[1..]),
        _ => (0, &rest[..]),
    };
    let octave: i32 = octave.parse().ok()?;
    Some(((octave + 1) * 12 + class + accidental) as f32)
}
