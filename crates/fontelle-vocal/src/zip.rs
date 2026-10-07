//! Reading a zip — a voicebank's, or an `.oudep` (which is a zip) — with
//! its file names in whatever they were written in.
//!
//! Why not a zip crate: the names. A UTAU bank zipped on a Japanese Windows
//! has Shift-JIS names and no flag saying so; a Chinese bank, GBK; a newer
//! one, UTF-8 with the flag, or without it. The encoding is chosen **once
//! for the whole archive**, by which reading of its names looks like a
//! language (kana for Japanese, ideographs, no half-width katakana soup),
//! which is what OpenUtau's installer asks the user and what this can
//! answer by itself nearly always. Only the stored and deflated methods are
//! read — what every zip tool writes — and zip64 sizes and offsets.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// One file in the archive.
#[derive(Debug, Clone)]
pub struct ZipEntry {
    /// Decoded, `/`-separated, as written (not yet checked for safety).
    pub name: String,
    method: u16,
    crc: u32,
    compressed: u64,
    pub size: u64,
    local_offset: u64,
    pub is_dir: bool,
}

pub struct ZipArchive {
    file: File,
    pub entries: Vec<ZipEntry>,
}

fn u16_at(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

fn u64_at(b: &[u8], at: usize) -> u64 {
    let mut a = [0u8; 8];
    a.copy_from_slice(&b[at..at + 8]);
    u64::from_le_bytes(a)
}

struct RawEntry {
    name: Vec<u8>,
    utf8: bool,
    unicode_path: Option<String>,
    method: u16,
    crc: u32,
    compressed: u64,
    size: u64,
    local_offset: u64,
}

impl ZipArchive {
    pub fn open(path: &Path) -> Result<Self, String> {
        let mut file = File::open(path).map_err(|e| e.to_string())?;
        let len = file.metadata().map_err(|e| e.to_string())?.len();
        if len < 22 {
            return Err("too short to be a zip".into());
        }
        // The end-of-central-directory record is in the last 64 KiB + 22.
        let tail_len = len.min(65_557);
        file.seek(SeekFrom::Start(len - tail_len))
            .map_err(|e| e.to_string())?;
        let mut tail = vec![0u8; tail_len as usize];
        file.read_exact(&mut tail).map_err(|e| e.to_string())?;
        let eocd = (0..=tail.len() - 22)
            .rev()
            .find(|&i| u32_at(&tail, i) == 0x0605_4b50)
            .ok_or("no zip directory (not a zip, or cut short)")?;
        let mut count = u64::from(u16_at(&tail, eocd + 10));
        let mut cd_size = u64::from(u32_at(&tail, eocd + 12));
        let mut cd_offset = u64::from(u32_at(&tail, eocd + 16));
        // Zip64: a locator just before the record points at the real one.
        if eocd >= 20 && u32_at(&tail, eocd - 20) == 0x0706_4b50 {
            let at = u64_at(&tail, eocd - 20 + 8);
            let mut rec = [0u8; 56];
            file.seek(SeekFrom::Start(at)).map_err(|e| e.to_string())?;
            file.read_exact(&mut rec).map_err(|e| e.to_string())?;
            if u32_at(&rec, 0) == 0x0606_4b50 {
                count = u64_at(&rec, 32);
                cd_size = u64_at(&rec, 40);
                cd_offset = u64_at(&rec, 48);
            }
        }
        if cd_offset + cd_size > len || cd_size > 512 * 1024 * 1024 {
            return Err("the zip's directory is out of range".into());
        }
        let mut cd = vec![0u8; cd_size as usize];
        file.seek(SeekFrom::Start(cd_offset))
            .map_err(|e| e.to_string())?;
        file.read_exact(&mut cd).map_err(|e| e.to_string())?;
        let mut raw = Vec::new();
        let mut at = 0usize;
        while raw.len() < count as usize && at + 46 <= cd.len() {
            if u32_at(&cd, at) != 0x0201_4b50 {
                return Err("a damaged zip directory".into());
            }
            let flags = u16_at(&cd, at + 8);
            let method = u16_at(&cd, at + 10);
            let crc = u32_at(&cd, at + 16);
            let mut compressed = u64::from(u32_at(&cd, at + 20));
            let mut size = u64::from(u32_at(&cd, at + 24));
            let name_len = usize::from(u16_at(&cd, at + 28));
            let extra_len = usize::from(u16_at(&cd, at + 30));
            let comment_len = usize::from(u16_at(&cd, at + 32));
            let mut local_offset = u64::from(u32_at(&cd, at + 42));
            let name_at = at + 46;
            let extra_at = name_at + name_len;
            let next = extra_at + extra_len + comment_len;
            if next > cd.len() {
                return Err("a damaged zip directory".into());
            }
            let name = cd[name_at..extra_at].to_vec();
            let mut unicode_path = None;
            let mut e = extra_at;
            while e + 4 <= extra_at + extra_len {
                let id = u16_at(&cd, e);
                let n = usize::from(u16_at(&cd, e + 2));
                let body = e + 4;
                if body + n > extra_at + extra_len {
                    break;
                }
                match id {
                    // Zip64: the 32-bit fields that are all ones, in order.
                    0x0001 => {
                        let mut p = body;
                        if size == 0xFFFF_FFFF && p + 8 <= body + n {
                            size = u64_at(&cd, p);
                            p += 8;
                        }
                        if compressed == 0xFFFF_FFFF && p + 8 <= body + n {
                            compressed = u64_at(&cd, p);
                            p += 8;
                        }
                        if local_offset == 0xFFFF_FFFF && p + 8 <= body + n {
                            local_offset = u64_at(&cd, p);
                        }
                    }
                    // Info-ZIP's Unicode path: version, CRC of the name,
                    // the name in UTF-8.
                    0x7075 if n > 5 => {
                        if let Ok(text) = std::str::from_utf8(&cd[body + 5..body + n]) {
                            unicode_path = Some(text.to_string());
                        }
                    }
                    _ => {}
                }
                e = body + n;
            }
            raw.push(RawEntry {
                name,
                utf8: flags & (1 << 11) != 0,
                unicode_path,
                method,
                crc,
                compressed,
                size,
                local_offset,
            });
            at = next;
        }
        let legacy: Vec<&[u8]> = raw
            .iter()
            .filter(|r| !r.utf8 && r.unicode_path.is_none())
            .map(|r| r.name.as_slice())
            .collect();
        let encoding = guess_names(&legacy);
        let entries = raw
            .into_iter()
            .map(|r| {
                let name = if let Some(u) = r.unicode_path {
                    u
                } else if r.utf8 {
                    String::from_utf8_lossy(&r.name).into_owned()
                } else {
                    encoding.decode(&r.name)
                };
                let name = name.replace('\\', "/");
                ZipEntry {
                    is_dir: name.ends_with('/'),
                    name,
                    method: r.method,
                    crc: r.crc,
                    compressed: r.compressed,
                    size: r.size,
                    local_offset: r.local_offset,
                }
            })
            .collect();
        Ok(Self { file, entries })
    }

    /// Writes entry `index` to `to`, checking its size and CRC.
    pub fn extract(&mut self, index: usize, to: &Path) -> Result<(), String> {
        let entry = self.entries[index].clone();
        let mut header = [0u8; 30];
        self.file
            .seek(SeekFrom::Start(entry.local_offset))
            .map_err(|e| e.to_string())?;
        self.file
            .read_exact(&mut header)
            .map_err(|e| e.to_string())?;
        if u32_at(&header, 0) != 0x0403_4b50 {
            return Err(format!("{}: a damaged entry", entry.name));
        }
        let skip = u64::from(u16_at(&header, 26)) + u64::from(u16_at(&header, 28));
        self.file
            .seek(SeekFrom::Current(skip as i64))
            .map_err(|e| e.to_string())?;
        let mut out = std::io::BufWriter::new(File::create(to).map_err(|e| e.to_string())?);
        let mut crc = Crc32::new();
        let mut written = 0u64;
        let mut input = (&mut self.file).take(entry.compressed);
        match entry.method {
            0 => {
                let mut buf = vec![0u8; 64 * 1024];
                loop {
                    let n = input.read(&mut buf).map_err(|e| e.to_string())?;
                    if n == 0 {
                        break;
                    }
                    crc.update(&buf[..n]);
                    out.write_all(&buf[..n]).map_err(|e| e.to_string())?;
                    written += n as u64;
                }
            }
            8 => {
                use miniz_oxide::inflate::stream::{InflateState, inflate};
                use miniz_oxide::{DataFormat, MZFlush, MZStatus};
                let mut state = InflateState::new_boxed(DataFormat::Raw);
                let mut inbuf = vec![0u8; 64 * 1024];
                let mut outbuf = vec![0u8; 256 * 1024];
                let mut have = 0usize;
                let mut eof = false;
                loop {
                    if !eof && have < inbuf.len() {
                        let n = input.read(&mut inbuf[have..]).map_err(|e| e.to_string())?;
                        eof = n == 0;
                        have += n;
                    }
                    let flush = if eof { MZFlush::Finish } else { MZFlush::None };
                    let r = inflate(&mut state, &inbuf[..have], &mut outbuf, flush);
                    inbuf.copy_within(r.bytes_consumed..have, 0);
                    have -= r.bytes_consumed;
                    let chunk = &outbuf[..r.bytes_written];
                    crc.update(chunk);
                    out.write_all(chunk).map_err(|e| e.to_string())?;
                    written += chunk.len() as u64;
                    match r.status {
                        Ok(MZStatus::StreamEnd) => break,
                        Ok(_) => {
                            if eof && r.bytes_consumed == 0 && r.bytes_written == 0 {
                                return Err(format!("{}: cut short", entry.name));
                            }
                        }
                        Err(e) => return Err(format!("{}: {e:?}", entry.name)),
                    }
                }
            }
            other => {
                return Err(format!(
                    "{} is packed with method {other}, which only deflate and stored are read for",
                    entry.name
                ));
            }
        }
        out.flush().map_err(|e| e.to_string())?;
        if written != entry.size || crc.finish() != entry.crc {
            return Err(format!("{} did not unpack whole", entry.name));
        }
        Ok(())
    }
}

/// A file name, made safe to put under a folder: `/`-separated parts with
/// no `..`, no root, no drive. `Err` names a part that would climb out;
/// `Ok(None)` is an entry to skip (a Mac's resource forks, `.DS_Store`).
pub fn safe_path(name: &str) -> Result<Option<PathBuf>, String> {
    let name = name.replace('\\', "/");
    if name.starts_with('/') || name.as_bytes().get(1) == Some(&b':') {
        return Err(name);
    }
    let mut out = PathBuf::new();
    for part in name.split('/') {
        match part {
            "" | "." => continue,
            ".." => return Err(name),
            "__MACOSX" | ".DS_Store" => return Ok(None),
            p if p.chars().any(|c| c.is_control()) => return Err(name),
            p => out.push(p),
        }
    }
    Ok((!out.as_os_str().is_empty()).then_some(out))
}

/// How the archive's legacy names are read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameEncoding {
    Utf8,
    ShiftJis,
    Gbk,
    /// The original PC character set, for anything else.
    Cp437,
}

impl NameEncoding {
    fn decode(self, bytes: &[u8]) -> String {
        match self {
            Self::Utf8 => String::from_utf8_lossy(bytes).into_owned(),
            Self::ShiftJis => encoding_rs::SHIFT_JIS.decode(bytes).0.into_owned(),
            Self::Gbk => encoding_rs::GBK.decode(bytes).0.into_owned(),
            Self::Cp437 => bytes
                .iter()
                .map(|&b| if b < 0x80 { b as char } else { '\u{fffd}' })
                .collect(),
        }
    }
}

/// How much `text` looks like somebody's language: kana count twice (only
/// Japanese has them), ideographs and Hangul once, half-width katakana —
/// what GBK bytes read as Shift-JIS turn into — against.
fn plausibility(text: &str) -> i64 {
    text.chars()
        .map(|c| match c as u32 {
            0x3040..=0x30FF => 2,
            0x4E00..=0x9FFF | 0xAC00..=0xD7AF => 1,
            0xFF61..=0xFF9F => -3,
            0x20..=0x7E => 0,
            0xFFFD => -100,
            0xE000..=0xF8FF => -5,
            _ => -1,
        })
        .sum()
}

fn strict(encoding: &'static encoding_rs::Encoding, bytes: &[u8]) -> Option<String> {
    encoding
        .decode_without_bom_handling_and_without_replacement(bytes)
        .map(|s| s.into_owned())
}

/// The reading of `names` that looks most like a language; UTF-8 when they
/// are all valid UTF-8 (Shift-JIS and GBK names almost never are).
fn guess_names(names: &[&[u8]]) -> NameEncoding {
    let non_ascii: Vec<&[u8]> = names.iter().copied().filter(|n| !n.is_ascii()).collect();
    if non_ascii.is_empty() || non_ascii.iter().all(|n| std::str::from_utf8(n).is_ok()) {
        return NameEncoding::Utf8;
    }
    let score = |enc: &'static encoding_rs::Encoding| -> Option<i64> {
        let mut total = 0;
        for n in &non_ascii {
            total += plausibility(&strict(enc, n)?);
        }
        Some(total)
    };
    let candidates = [
        (NameEncoding::ShiftJis, score(encoding_rs::SHIFT_JIS)),
        (NameEncoding::Gbk, score(encoding_rs::GBK)),
    ];
    candidates
        .iter()
        .filter_map(|(e, s)| s.map(|s| (*e, s)))
        .max_by_key(|(_, s)| *s)
        .map_or(NameEncoding::Cp437, |(e, _)| e)
}

/// Text from a bank — `oto.ini`, `character.txt`, a readme — in whichever
/// encoding it was written: a BOM's, UTF-8 if it is valid, else the more
/// plausible of Shift-JIS and GBK.
pub fn decode_text(bytes: &[u8]) -> String {
    if let Some((enc, bom)) = encoding_rs::Encoding::for_bom(bytes) {
        return enc
            .decode_without_bom_handling(&bytes[bom..])
            .0
            .into_owned();
    }
    if let Ok(text) = std::str::from_utf8(bytes) {
        return text.to_string();
    }
    let sjis = strict(encoding_rs::SHIFT_JIS, bytes);
    let gbk = strict(encoding_rs::GBK, bytes);
    match (sjis, gbk) {
        (Some(a), Some(b)) => {
            if plausibility(&b) > plausibility(&a) {
                b
            } else {
                a
            }
        }
        (Some(a), None) => a,
        (None, Some(b)) => b,
        (None, None) => encoding_rs::SHIFT_JIS.decode(bytes).0.into_owned(),
    }
}

/// CRC-32 (the zip one), by table.
struct Crc32 {
    table: [u32; 256],
    value: u32,
}

impl Crc32 {
    fn new() -> Self {
        let mut table = [0u32; 256];
        for (i, slot) in table.iter_mut().enumerate() {
            let mut c = i as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 {
                    0xEDB8_8320 ^ (c >> 1)
                } else {
                    c >> 1
                };
            }
            *slot = c;
        }
        Self {
            table,
            value: 0xFFFF_FFFF,
        }
    }

    fn update(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.value =
                self.table[((self.value ^ u32::from(b)) & 0xFF) as usize] ^ (self.value >> 8);
        }
    }

    fn finish(&self) -> u32 {
        !self.value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_that_climb_out_are_refused_and_mac_litter_skipped() {
        assert!(safe_path("../x").is_err());
        assert!(safe_path("a/../../x").is_err());
        assert!(safe_path("/etc/passwd").is_err());
        assert!(safe_path("C:/x").is_err());
        assert_eq!(safe_path("__MACOSX/a/._b").unwrap(), None);
        assert_eq!(
            safe_path("a\\b/./c.wav").unwrap(),
            Some(PathBuf::from("a/b/c.wav"))
        );
    }

    #[test]
    fn crc_is_the_zip_one() {
        let mut c = Crc32::new();
        c.update(b"123456789");
        assert_eq!(c.finish(), 0xCBF4_3926);
    }
}
