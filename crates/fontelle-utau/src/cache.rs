//! Analyses of samples, kept: in memory for the backend's life, on disk
//! (keyed by the file's content) across runs. Analysing is the slow part of
//! singing a bank for the first time; afterwards a phrase is assembly and
//! synthesis only.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use sha2::{Digest, Sha256};

use crate::world::{self, AP_BANDS, Analysis, AnalysisOptions};

/// Bumped whenever analysis changes what it produces: old files are then
/// simply not found.
const ANALYSIS_VERSION: u32 = 2;
/// Every sample is analysed at this rate.
pub const ANALYSIS_RATE: u32 = 44_100;

type MemKey = (PathBuf, u64, Option<SystemTime>, u32);

pub struct AnalysisCache {
    dir: Option<PathBuf>,
    budget: usize,
    mem: Mutex<Mem>,
}

#[derive(Default)]
struct Mem {
    map: HashMap<MemKey, (Arc<Analysis>, u64)>,
    bytes: usize,
    tick: u64,
}

impl AnalysisCache {
    /// `dir`: where analyses are kept on disk; `None` keeps them in memory
    /// only. `budget`: bytes of analyses kept in memory.
    pub fn new(dir: Option<PathBuf>, budget: usize) -> Self {
        Self {
            dir,
            budget,
            mem: Mutex::new(Mem::default()),
        }
    }

    fn mem_key(path: &Path, opts: &AnalysisOptions) -> MemKey {
        let meta = std::fs::metadata(path).ok();
        (
            path.to_path_buf(),
            meta.as_ref().map(|m| m.len()).unwrap_or(0),
            meta.and_then(|m| m.modified().ok()),
            opts.prior.unwrap_or(0.0).to_bits() ^ opts.f0_floor.to_bits().rotate_left(7),
        )
    }

    fn disk_path(&self, bytes: &[u8], opts: &AnalysisOptions) -> Option<PathBuf> {
        let dir = self.dir.as_ref()?;
        let mut h = Sha256::new();
        h.update(ANALYSIS_VERSION.to_le_bytes());
        h.update(ANALYSIS_RATE.to_le_bytes());
        h.update(opts.f0_floor.to_le_bytes());
        h.update(opts.f0_ceil.to_le_bytes());
        h.update(opts.prior.unwrap_or(0.0).to_le_bytes());
        h.update(bytes);
        let hex: String = h
            .finalize()
            .iter()
            .take(20)
            .map(|b| format!("{b:02x}"))
            .collect();
        Some(dir.join(&hex[..2]).join(format!("{hex}.fua")))
    }

    /// The analysis of the sample at `path`, made if it is not kept.
    pub fn get(&self, path: &Path, opts: &AnalysisOptions) -> Result<Arc<Analysis>, String> {
        let key = Self::mem_key(path, opts);
        {
            let mut mem = self.mem.lock().unwrap_or_else(|e| e.into_inner());
            mem.tick += 1;
            let tick = mem.tick;
            if let Some((a, used)) = mem.map.get_mut(&key) {
                *used = tick;
                return Ok(a.clone());
            }
        }
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let disk = self.disk_path(&bytes, opts);
        let analysis = match disk.as_deref().and_then(read_analysis) {
            Some(a) => a,
            None => {
                let (x, rate) = crate::wav::parse_wav(&bytes)
                    .map_err(|e| format!("{}: {e}", path.display()))?;
                let x = crate::wav::resample(&x, rate, ANALYSIS_RATE);
                let a = world::analyze(&x, ANALYSIS_RATE, opts);
                if let Some(d) = &disk {
                    // A cache that cannot be written is only slower.
                    let _ = write_analysis(d, &a);
                }
                a
            }
        };
        let a = Arc::new(analysis);
        let mut mem = self.mem.lock().unwrap_or_else(|e| e.into_inner());
        mem.tick += 1;
        let tick = mem.tick;
        mem.bytes += a.bytes();
        if let Some((old, _)) = mem.map.insert(key, (a.clone(), tick)) {
            mem.bytes -= old.bytes();
        }
        while mem.bytes > self.budget && mem.map.len() > 1 {
            let Some(oldest) = mem
                .map
                .iter()
                .min_by_key(|(_, (_, used))| *used)
                .map(|(k, _)| k.clone())
            else {
                break;
            };
            if let Some((old, _)) = mem.map.remove(&oldest) {
                mem.bytes -= old.bytes();
            }
        }
        Ok(a)
    }

    /// Whether the analysis is in memory or on disk already.
    pub fn is_cached(&self, path: &Path) -> bool {
        self.is_cached_with(path, &options_for(path))
    }

    pub fn is_cached_with(&self, path: &Path, opts: &AnalysisOptions) -> bool {
        let key = Self::mem_key(path, opts);
        if self
            .mem
            .lock()
            .map(|m| m.map.contains_key(&key))
            .unwrap_or(false)
        {
            return true;
        }
        let Ok(bytes) = std::fs::read(path) else {
            return false;
        };
        self.disk_path(&bytes, opts).is_some_and(|p| p.is_file())
    }
}

/// How a sample is analysed: with its `.frq`'s average F0 as the prior
/// when the bank has one beside it (`a.wav` → `a_wav.frq`).
pub fn options_for(path: &Path) -> AnalysisOptions {
    let mut opts = AnalysisOptions::default();
    if let Some(name) = path.file_name() {
        let frq = path.with_file_name(format!("{}.frq", name.to_string_lossy().replace('.', "_")));
        if let Some(avg) = frq_average(&frq) {
            opts.prior = Some(avg);
        }
    }
    opts
}

/// A `.frq` file's average F0: "FREQ0003", hop (i32), average (f64),
/// 16 blank bytes, length (i32), then (f0, amplitude) f64 pairs.
pub fn frq_average(path: &Path) -> Option<f32> {
    let b = std::fs::read(path).ok()?;
    if b.len() < 40 || &b[..4] != b"FREQ" {
        return None;
    }
    let avg = f64::from_le_bytes(b[12..20].try_into().ok()?);
    let avg = if avg > 40.0 && avg < 2000.0 {
        avg
    } else {
        // Some writers leave the average empty: take the frames' median.
        let n = i32::from_le_bytes(b[36..40].try_into().ok()?).max(0) as usize;
        let mut f: Vec<f64> = (0..n)
            .filter_map(|i| {
                let at = 40 + i * 16;
                b.get(at..at + 8)
                    .and_then(|s| s.try_into().ok())
                    .map(f64::from_le_bytes)
            })
            .filter(|f| *f > 40.0 && *f < 2000.0)
            .collect();
        if f.is_empty() {
            return None;
        }
        f.sort_by(f64::total_cmp);
        f[f.len() / 2]
    };
    Some(avg as f32)
}

const MAGIC: &[u8; 4] = b"FUA1";
/// Envelope levels kept to a hundredth of a dB from -300 dB.
const DB_STEP: f64 = 0.01;
const DB_FLOOR: f64 = -300.0;

fn write_analysis(path: &Path, a: &Analysis) -> std::io::Result<()> {
    let mut raw = Vec::with_capacity(16 + a.f0.len() * 4 + a.sp.len() * 2 + a.ap.len() * 2);
    raw.extend_from_slice(&a.fs.to_le_bytes());
    raw.extend_from_slice(&(a.fft_size as u32).to_le_bytes());
    raw.extend_from_slice(&(a.f0.len() as u32).to_le_bytes());
    for f in &a.f0 {
        raw.extend_from_slice(&f.to_le_bytes());
    }
    for &p in &a.sp {
        let db = 10.0 * f64::from(p).max(1e-30).log10();
        let q = ((db - DB_FLOOR) / DB_STEP).round().clamp(0.0, 65535.0) as u16;
        raw.extend_from_slice(&q.to_le_bytes());
    }
    for &v in &a.ap {
        let q = (f64::from(v).clamp(0.0, 1.0) * 65535.0).round() as u16;
        raw.extend_from_slice(&q.to_le_bytes());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    {
        let f = std::fs::File::create(&tmp)?;
        let mut w = std::io::BufWriter::new(f);
        w.write_all(MAGIC)?;
        let mut z = flate2::write::ZlibEncoder::new(w, flate2::Compression::fast());
        z.write_all(&raw)?;
        z.finish()?.flush()?;
    }
    std::fs::rename(&tmp, path)
}

fn read_analysis(path: &Path) -> Option<Analysis> {
    let bytes = std::fs::read(path).ok()?;
    if bytes.len() < 4 || &bytes[..4] != MAGIC {
        return None;
    }
    let mut raw = Vec::new();
    flate2::read::ZlibDecoder::new(&bytes[4..])
        .read_to_end(&mut raw)
        .ok()?;
    let u32_at =
        |i: usize| -> Option<u32> { Some(u32::from_le_bytes(raw.get(i..i + 4)?.try_into().ok()?)) };
    let fs = u32_at(0)?;
    let fft_size = u32_at(4)? as usize;
    let frames = u32_at(8)? as usize;
    let bins = fft_size / 2 + 1;
    let need = 12 + frames * 4 + frames * bins * 2 + frames * AP_BANDS * 2;
    if raw.len() != need {
        return None;
    }
    let mut at = 12;
    let f0 = (0..frames)
        .map(|i| f32::from_le_bytes(raw[at + i * 4..at + i * 4 + 4].try_into().unwrap_or([0; 4])))
        .collect();
    at += frames * 4;
    let u16s = |at: usize, n: usize| -> Vec<u16> {
        (0..n)
            .map(|i| u16::from_le_bytes([raw[at + 2 * i], raw[at + 2 * i + 1]]))
            .collect()
    };
    let sp = u16s(at, frames * bins)
        .into_iter()
        .map(|q| 10f64.powf((f64::from(q) * DB_STEP + DB_FLOOR) / 10.0) as f32)
        .collect();
    at += frames * bins * 2;
    let ap = u16s(at, frames * AP_BANDS)
        .into_iter()
        .map(|q| (f64::from(q) / 65535.0) as f32)
        .collect();
    Some(Analysis {
        fs,
        fft_size,
        f0,
        sp,
        ap,
    })
}

/// `<cache>/fontelle/utau`: `$XDG_CACHE_HOME` or `~/.cache` (Linux),
/// `~/Library/Caches` (macOS), `%LOCALAPPDATA%` (Windows).
pub fn default_cache_dir() -> Option<PathBuf> {
    let os = if cfg!(windows) {
        CacheOs::Windows
    } else if cfg!(target_os = "macos") {
        CacheOs::Macos
    } else {
        CacheOs::Linux
    };
    default_cache_dir_from(&|key| std::env::var(key).ok(), os)
}

/// Which platform's rule [`default_cache_dir_from`] follows — a parameter so
/// all three are tested on whichever machine runs the tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheOs {
    Linux,
    Macos,
    Windows,
}

/// [`default_cache_dir`] for an environment: **an absolute folder or none**.
///
/// A relative variable (or an empty `HOME`) once came out as a relative
/// `.cache/fontelle/utau` — under the working directory, which is the
/// install folder when Steam starts the program, and Steam verifies that
/// folder file by file (hub card 0429). No cache is better than that.
pub fn default_cache_dir_from(
    env: &dyn Fn(&str) -> Option<String>,
    os: CacheOs,
) -> Option<PathBuf> {
    let absolute = |value: &String| match os {
        CacheOs::Windows => {
            let b = value.as_bytes();
            (b.len() >= 3 && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/'))
                || value.starts_with("\\\\")
        }
        _ => value.starts_with('/'),
    };
    let home = || env("HOME").filter(absolute).map(PathBuf::from);
    let base = match os {
        CacheOs::Windows => env("LOCALAPPDATA").filter(absolute).map(PathBuf::from),
        CacheOs::Macos => home().map(|h| h.join("Library").join("Caches")),
        CacheOs::Linux => env("XDG_CACHE_HOME")
            .filter(absolute)
            .map(PathBuf::from)
            .or_else(|| home().map(|h| h.join(".cache"))),
    }?;
    Some(base.join("fontelle").join("utau"))
}
