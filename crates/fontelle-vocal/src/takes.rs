//! Takes kept on disk, so a song reopened plays at once instead of singing
//! everything again.
//!
//! **Where** is the app's choice ([`crate::VocalRenderer::set_store`]): a
//! saved song's own `cache/vocal/` (beside Analyze Musically's
//! `cache/analysis/`), an unsaved one's Fontelle cache folder. Only **final**
//! takes are written — a preview is replaced within seconds — by the render
//! thread that sang them, and read back by a render thread before it would
//! sing: a phrase whose final is on disk is not previewed at all.
//!
//! **The file**: `<render key hash>.take`, a 24-byte header (`FTAK`, the
//! format version, the quality, the key's hash and seed — a file whose key
//! is not the one asked for is ignored), then the take deflated: sample rate,
//! start, pitch step, and the samples as **16-bit** (scaled by the take's
//! own peak when it is louder than full scale), delta-coded and split into
//! high and low byte planes so the slow-moving high bytes deflate to little;
//! the pitch line as floats; the phonemes. An xxHash64 of the body closes
//! it, so a cut-short or damaged file reads as absent, never as noise.
//!
//! **The limit**: after each save the folder is trimmed to its size limit,
//! the least recently used take first (a load touches its file).

use std::hash::Hasher;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

use fontelle_sing::{Quality, SungNote, SungPhoneme};

use crate::phrases::RenderKey;
use crate::render::Rendered;

/// How much a song's takes may take on disk unless told otherwise.
pub const DEFAULT_TAKE_BYTES: u64 = 512 * 1024 * 1024;

const MAGIC: &[u8; 4] = b"FTAK";
const VERSION: u16 = 1;
const HEADER: usize = 24;
const EXTENSION: &str = "take";

static TEMP: AtomicU64 = AtomicU64::new(0);

/// A folder of final takes, by render key, under a size limit.
#[derive(Debug, Clone)]
pub struct TakeStore {
    dir: PathBuf,
    limit: u64,
}

impl TakeStore {
    /// Takes in `dir` (made when the first is saved), at most `limit` bytes
    /// of them.
    pub fn new(dir: PathBuf, limit: u64) -> Self {
        Self { dir, limit }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn path(&self, key: RenderKey) -> PathBuf {
        self.dir.join(format!("{:016x}.{EXTENSION}", key.hash))
    }

    /// Writes `take` under `key` (whole or not at all), then trims the
    /// folder to its limit.
    pub fn save(&self, key: RenderKey, take: &Rendered) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        let bytes = encode(key, take);
        let temp = self.dir.join(format!(
            ".writing-{}-{}",
            std::process::id(),
            TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        let written = (|| {
            let mut f = std::fs::File::create(&temp)?;
            f.write_all(&bytes)?;
            f.sync_data()?;
            std::fs::rename(&temp, self.path(key))
        })();
        if written.is_err() {
            let _ = std::fs::remove_file(&temp);
        }
        written?;
        self.trim();
        Ok(())
    }

    /// The take saved under `key`, if there is one and it reads whole; it
    /// becomes the most recently used.
    pub fn load(&self, key: RenderKey) -> Option<Rendered> {
        let path = self.path(key);
        let bytes = std::fs::read(&path).ok()?;
        let take = decode(key, &bytes)?;
        if let Ok(f) = std::fs::File::options().write(true).open(&path) {
            let _ = f.set_modified(SystemTime::now());
        }
        Some(take)
    }

    /// Whether a take is saved under `key` (not whether it reads).
    pub fn has(&self, key: RenderKey) -> bool {
        self.path(key).is_file()
    }

    /// Bytes the saved takes take.
    pub fn size(&self) -> u64 {
        self.files().iter().map(|f| f.2).sum()
    }

    /// (path, last used, size) of every take.
    fn files(&self) -> Vec<(PathBuf, SystemTime, u64)> {
        std::fs::read_dir(&self.dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == EXTENSION))
            .filter_map(|e| {
                let meta = e.metadata().ok()?;
                Some((
                    e.path(),
                    meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                    meta.len(),
                ))
            })
            .collect()
    }

    /// Removes the least recently used takes until the rest fit the limit.
    pub fn trim(&self) {
        let mut files = self.files();
        let mut total: u64 = files.iter().map(|f| f.2).sum();
        if total <= self.limit {
            return;
        }
        files.sort_by_key(|f| f.1);
        for (path, _, size) in files {
            if total <= self.limit {
                break;
            }
            if std::fs::remove_file(&path).is_ok() {
                total -= size;
            }
        }
    }
}

fn checksum(body: &[u8]) -> u64 {
    let mut h = twox_hash::XxHash64::with_seed(0x7461_6b65);
    h.write(body);
    h.finish()
}

fn encode(key: RenderKey, take: &Rendered) -> Vec<u8> {
    let mut body = Vec::with_capacity(take.samples.len() * 2 + 1024);
    body.extend_from_slice(&take.sample_rate.to_le_bytes());
    body.extend_from_slice(&take.start.to_le_bytes());
    body.extend_from_slice(&take.pitch_step.to_le_bytes());
    let peak = take.samples.iter().fold(
        0.0f32,
        |m, s| if s.is_finite() { m.max(s.abs()) } else { m },
    );
    let scale = peak.max(1.0);
    body.extend_from_slice(&scale.to_le_bytes());
    body.extend_from_slice(&(take.samples.len() as u64).to_le_bytes());
    // 16-bit, delta-coded, high bytes then low bytes.
    let mut high = Vec::with_capacity(take.samples.len());
    let mut low = Vec::with_capacity(take.samples.len());
    let mut last = 0i16;
    for s in take.samples.iter() {
        let s = if s.is_finite() { *s } else { 0.0 };
        let q = (s / scale * 32767.0).round().clamp(-32767.0, 32767.0) as i16;
        let d = q.wrapping_sub(last).to_le_bytes();
        last = q;
        low.push(d[0]);
        high.push(d[1]);
    }
    body.extend_from_slice(&high);
    body.extend_from_slice(&low);
    body.extend_from_slice(&(take.pitch.len() as u64).to_le_bytes());
    for p in take.pitch.iter() {
        body.extend_from_slice(&p.to_le_bytes());
    }
    body.extend_from_slice(&(take.phonemes.len() as u32).to_le_bytes());
    for note in &take.phonemes {
        body.extend_from_slice(&(note.note as u32).to_le_bytes());
        body.push(u8::from(note.guessed));
        body.extend_from_slice(&(note.phonemes.len() as u32).to_le_bytes());
        for p in &note.phonemes {
            body.extend_from_slice(&(p.symbol.len() as u32).to_le_bytes());
            body.extend_from_slice(p.symbol.as_bytes());
            match p.offset {
                Some(o) => {
                    body.push(1);
                    body.extend_from_slice(&o.to_le_bytes());
                }
                None => body.push(0),
            }
        }
    }
    let sum = checksum(&body);
    let packed = miniz_oxide::deflate::compress_to_vec(&body, 6);
    let mut out = Vec::with_capacity(HEADER + packed.len() + 8);
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.push(match take.quality {
        Quality::Preview => 0,
        Quality::Final => 1,
    });
    out.push(0);
    out.extend_from_slice(&key.hash.to_le_bytes());
    out.extend_from_slice(&key.seed.to_le_bytes());
    out.extend_from_slice(&packed);
    out.extend_from_slice(&sum.to_le_bytes());
    out
}

/// Reads little-endian fields off the front of a slice.
struct Cursor<'a>(&'a [u8]);

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        if self.0.len() < n {
            return None;
        }
        let (a, b) = self.0.split_at(n);
        self.0 = b;
        Some(a)
    }
    fn array<const N: usize>(&mut self) -> Option<[u8; N]> {
        self.take(N)?.try_into().ok()
    }
    fn u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }
    fn u32(&mut self) -> Option<u32> {
        self.array().map(u32::from_le_bytes)
    }
    fn u64(&mut self) -> Option<u64> {
        self.array().map(u64::from_le_bytes)
    }
    fn f32(&mut self) -> Option<f32> {
        self.array().map(f32::from_le_bytes)
    }
    fn f64(&mut self) -> Option<f64> {
        self.array().map(f64::from_le_bytes)
    }
    /// A count of things at least `each` bytes long, checked against what
    /// is left so a damaged count cannot ask for a huge allocation.
    fn count(&mut self, n: u64, each: usize) -> Option<usize> {
        let n = usize::try_from(n).ok()?;
        (n.checked_mul(each.max(1))? <= self.0.len()).then_some(n)
    }
}

fn decode(key: RenderKey, bytes: &[u8]) -> Option<Rendered> {
    if bytes.len() < HEADER + 8 || &bytes[..4] != MAGIC {
        return None;
    }
    let mut head = Cursor(&bytes[4..HEADER]);
    if u16::from_le_bytes(head.array()?) != VERSION {
        return None;
    }
    let quality = match head.u8()? {
        0 => Quality::Preview,
        1 => Quality::Final,
        _ => return None,
    };
    head.u8()?;
    if head.u64()? != key.hash || head.u64()? != key.seed {
        return None;
    }
    let (packed, sum) = bytes[HEADER..].split_at(bytes.len() - HEADER - 8);
    let body = miniz_oxide::inflate::decompress_to_vec_with_limit(packed, 1 << 31).ok()?;
    if checksum(&body) != u64::from_le_bytes(sum.try_into().ok()?) {
        return None;
    }
    let mut c = Cursor(&body);
    let sample_rate = c.u32()?;
    let start = c.f64()?;
    let pitch_step = c.f64()?;
    let scale = c.f32()?;
    let n = c.u64()?;
    let n = c.count(n, 2)?;
    let high = c.take(n)?;
    let low = c.take(n)?;
    let mut last = 0i16;
    let samples: Vec<f32> = high
        .iter()
        .zip(low)
        .map(|(h, l)| {
            last = last.wrapping_add(i16::from_le_bytes([*l, *h]));
            f32::from(last) / 32767.0 * scale
        })
        .collect();
    let n = c.u64()?;
    let n = c.count(n, 4)?;
    let pitch: Vec<f32> = (0..n).map(|_| c.f32()).collect::<Option<_>>()?;
    let notes = c.u32()?;
    let notes = c.count(u64::from(notes), 9)?;
    let mut phonemes = Vec::with_capacity(notes);
    for _ in 0..notes {
        let note = c.u32()? as usize;
        let guessed = c.u8()? != 0;
        let count = c.u32()?;
        let count = c.count(u64::from(count), 5)?;
        let mut list = Vec::with_capacity(count);
        for _ in 0..count {
            let len = c.u32()? as usize;
            let symbol = String::from_utf8(c.take(len)?.to_vec()).ok()?;
            let offset = match c.u8()? {
                0 => None,
                _ => Some(c.f64()?),
            };
            list.push(SungPhoneme { symbol, offset });
        }
        phonemes.push(SungNote {
            note,
            phonemes: list,
            guessed,
        });
    }
    Some(Rendered {
        quality,
        sample_rate,
        samples: samples.into(),
        start,
        pitch: pitch.into(),
        pitch_step,
        phonemes,
    })
}
