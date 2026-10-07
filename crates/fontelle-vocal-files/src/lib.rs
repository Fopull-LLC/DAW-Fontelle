//! Vocal song files in and out (`docs/vocal-plan.md` §3, *Writing*).
//!
//! Producers arrive from UTAU, OpenUtau, Vocaloid and notation programs with
//! their songs in those programs' files. Each reader here turns one into a
//! [`VocalSong`]: the song's tempo and metre, and one [`SongPart`] per sung
//! line — notes with their lyrics and expression (`Note::voice`, velocity as
//! consonant speed) and the part's lanes (`VocalPart`), all on this
//! project's grid ([`fontelle_types::PPQN`]) **from the song's tick 0**. The
//! app places a part as a clip and decides what happens to the tempo; this
//! crate only reads and writes.
//!
//! What a format carries that the part has no place for is not dropped
//! silently: [`VocalSong::unmapped`] names it, once per kind, so the app can
//! say so.
//!
//! | file | read | written |
//! |---|---|---|
//! | `.ust` (UTAU) | [`ust::read`] | — |
//! | `.ustx` (OpenUtau) | [`ustx::read`] | [`ustx::write`] |
//! | `.vsqx` (Vocaloid 3/4) | [`vsqx::read`] | — |
//! | `.mid` with lyric events | [`midi::read`] | [`midi::write`] |
//! | `.musicxml` / `.xml` / `.mxl` | [`musicxml::read`] | — |

use std::path::Path;

pub mod midi;
pub mod musicxml;
mod pitch;
mod project;
mod song;
pub mod ust;
pub mod ustx;
pub mod vsqx;
mod zip;

pub use project::{clip_of, song_of_channel};
pub use song::{SongPart, VocalSong, vocal_note};

/// Which kind of file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Ust,
    Ustx,
    Vsqx,
    Midi,
    MusicXml,
}

impl Format {
    /// What a person calls it.
    pub fn label(self) -> &'static str {
        match self {
            Format::Ust => "UST",
            Format::Ustx => "USTX",
            Format::Vsqx => "VSQX",
            Format::Midi => "MIDI",
            Format::MusicXml => "MusicXML",
        }
    }

    /// By extension. `.xml` is taken as MusicXML here; [`is_vocal_song`]
    /// looks inside before saying a dropped `.xml` is one.
    pub fn of_path(path: &Path) -> Option<Format> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        Some(match ext.as_str() {
            "ust" => Format::Ust,
            "ustx" => Format::Ustx,
            "vsqx" | "vsq4" | "vsq3" => Format::Vsqx,
            "mid" | "midi" | "kar" => Format::Midi,
            "musicxml" | "xml" | "mxl" => Format::MusicXml,
            _ => return None,
        })
    }
}

/// Reads `path` by its extension.
pub fn read_file(path: &Path) -> Result<VocalSong, String> {
    let format = Format::of_path(path).ok_or_else(|| {
        format!(
            "{} is not a vocal song file (.ust, .ustx, .vsqx, .mid, .musicxml, .mxl)",
            path.display()
        )
    })?;
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let name = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let compressed = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("mxl"));
    if compressed {
        return musicxml::read_mxl(&bytes, &name);
    }
    read(format, &bytes, &name)
}

/// Reads `bytes` as `format`; `name` is the song's name when the file has
/// none of its own.
pub fn read(format: Format, bytes: &[u8], name: &str) -> Result<VocalSong, String> {
    match format {
        Format::Ust => ust::read(bytes, name),
        Format::Ustx => ustx::read(&text(bytes), name),
        Format::Vsqx => vsqx::read(&text(bytes), name),
        Format::Midi => midi::read(bytes, name),
        Format::MusicXml if bytes.starts_with(b"PK") => musicxml::read_mxl(bytes, name),
        Format::MusicXml => musicxml::read(&text(bytes), name),
    }
}

/// Whether a dropped file is a vocal song: `.ust`, `.ustx`, `.vsqx`,
/// `.musicxml` and `.mxl` always; an `.xml` only when it is MusicXML; a
/// `.mid` only when it has lyrics (one without is an ordinary MIDI import).
pub fn is_vocal_song(path: &Path) -> bool {
    let Some(format) = Format::of_path(path) else {
        return false;
    };
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    match format {
        Format::Midi => std::fs::read(path).is_ok_and(|b| midi::has_lyrics(&b)),
        Format::MusicXml if ext == "xml" => std::fs::read(path)
            .is_ok_and(|b| musicxml::looks_like_musicxml(&String::from_utf8_lossy(&b))),
        _ => true,
    }
}

/// UTF-8 (a byte-order mark dropped), else UTF-16 by its mark, else lossy.
pub(crate) fn text(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(rest).into_owned();
    }
    if bytes.starts_with(&[0xFF, 0xFE]) {
        return encoding_rs::UTF_16LE.decode(bytes).0.into_owned();
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        return encoding_rs::UTF_16BE.decode(bytes).0.into_owned();
    }
    String::from_utf8_lossy(bytes).into_owned()
}
