//! Reading a plugin bundle in a **child process**, so a broken one cannot
//! take the studio down with it.
//!
//! A CLAP bundle (and a VST 3 one with no `moduleinfo.json`, and anything a
//! bridge serves) is read by loading it: its library's initialisers run, and
//! then its entry point. Done in the studio's own process, for every bundle
//! on the machine, one bad plugin anywhere on the disk was a studio that
//! would not start — and a good one could be the cause too: ZamHeadX2's entry
//! sets up FFTW, FFTW kept a pointer into a library the scan had already
//! unloaded, and the process died inside a plugin nobody had asked for. On
//! one machine and not another, by what is installed.
//!
//! So the studio runs **itself** with [`PROBE_FLAG`] and a bundle's path,
//! the child loads that one bundle and prints what it found, and the parent
//! reads it. A child that crashes, or does not answer within the timeout,
//! is a failure in the scan's list, and the studio goes on. What was read is
//! remembered by the file's size and modification time — across starts,
//! with a cache file — so a machine with three hundred plugins pays for
//! them once.
//!
//! LV2 is not read this way: a bundle's Turtle says what it holds, and no
//! library is loaded to read it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant, UNIX_EPOCH};

use fontelle_types::{PluginFormat, PluginKey};

use crate::scan::PluginInfo;

/// The argument that makes a Fontelle binary read one bundle and exit:
/// `fontelle --fontelle-scan-bundle <path> [--bridges <folder>]…`.
pub const PROBE_FLAG: &str = "--fontelle-scan-bundle";

/// How long a bundle may take to be read before it is given up on. A big
/// one loads in well under a second; this is for one that never returns.
pub const DEFAULT_PROBE_TIMEOUT: Duration = Duration::from_secs(20);

/// How many bundles are read at once.
const PARALLEL: usize = 6;

/// What a bundle held, or why it could not be read.
type Outcome = Result<Vec<PluginInfo>, String>;

/// A bundle as it was when it was read: a different size or time is a
/// different bundle.
#[derive(Clone, PartialEq, Eq, Hash)]
struct Stamp {
    path: PathBuf,
    len: u64,
    modified: u64,
}

impl Stamp {
    fn of(path: &Path) -> Option<Self> {
        // A bundle may be a folder (VST 3, a macOS CLAP): its library's
        // time is what changes when it is updated, and the folder's own
        // often does not. The newest file inside, to a short depth.
        let (len, modified) = newest(path, 0)?;
        Some(Self {
            path: path.to_path_buf(),
            len,
            modified,
        })
    }
}

fn newest(path: &Path, depth: usize) -> Option<(u64, u64)> {
    let meta = std::fs::metadata(path).ok()?;
    let time = |meta: &std::fs::Metadata| {
        meta.modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_secs())
    };
    if !meta.is_dir() {
        return Some((meta.len(), time(&meta)));
    }
    let mut best = (0u64, time(&meta));
    if depth < 4 {
        for entry in std::fs::read_dir(path).ok()?.flatten() {
            if let Some((len, modified)) = newest(&entry.path(), depth + 1) {
                best.0 += len;
                best.1 = best.1.max(modified);
            }
        }
    }
    Some(best)
}

/// Reads bundles in child processes, and remembers what they held.
pub struct BundleProber {
    helper: PathBuf,
    timeout: Duration,
    cache_file: Option<PathBuf>,
    known: Mutex<HashMap<Stamp, Outcome>>,
    probes: AtomicUsize,
}

impl BundleProber {
    /// Reads bundles by running `helper` — a Fontelle binary, which answers
    /// [`PROBE_FLAG`].
    pub fn new(helper: PathBuf) -> Self {
        Self {
            helper,
            timeout: DEFAULT_PROBE_TIMEOUT,
            cache_file: None,
            known: Mutex::new(HashMap::new()),
            probes: AtomicUsize::new(0),
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Remembers across starts in `file`, reading what is already there.
    pub fn with_cache_file(mut self, file: PathBuf) -> Self {
        if let Ok(text) = std::fs::read_to_string(&file) {
            let mut known = self.known.lock().unwrap();
            for (stamp, outcome) in read_cache(&text) {
                known.insert(stamp, outcome);
            }
        }
        self.cache_file = Some(file);
        self
    }

    /// How many child processes have been run — for a test that a cache
    /// asked nobody.
    pub fn probes(&self) -> usize {
        self.probes.load(Ordering::Relaxed)
    }

    /// Forgets every bundle that could not be read, so the next scan tries
    /// them again — what a rescan somebody asked for does. What was read
    /// stays.
    pub fn forget_failures(&self) {
        self.known
            .lock()
            .unwrap()
            .retain(|_, outcome| outcome.is_ok());
    }

    /// Writes what is known to the cache file, if there is one. Quietly: a
    /// cache that could not be written is a slower next start, not an error
    /// anybody can act on.
    pub fn save(&self) {
        let Some(file) = &self.cache_file else {
            return;
        };
        let text = write_cache(&self.known.lock().unwrap());
        if let Some(parent) = file.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let staging = file.with_extension("tmp");
        if std::fs::write(&staging, text).is_ok() {
            let _ = std::fs::rename(&staging, file);
        }
    }

    /// What each of `bundles` holds, in their order — from what is known,
    /// or from a child process each, several at a time.
    pub(crate) fn read_all(&self, bundles: &[PathBuf], bridge_folders: &[PathBuf]) -> Vec<Outcome> {
        let stamps: Vec<Option<Stamp>> = bundles.iter().map(|path| Stamp::of(path)).collect();
        let mut results: Vec<Option<Outcome>> = {
            let known = self.known.lock().unwrap();
            stamps
                .iter()
                .map(|stamp| stamp.as_ref().and_then(|s| known.get(s).cloned()))
                .collect()
        };
        let wanted: Vec<usize> = (0..bundles.len())
            .filter(|i| results[*i].is_none())
            .collect();
        let next = AtomicUsize::new(0);
        let found: Mutex<Vec<(usize, Outcome)>> = Mutex::new(Vec::new());
        std::thread::scope(|scope| {
            for _ in 0..PARALLEL.min(wanted.len()) {
                scope.spawn(|| {
                    loop {
                        let at = next.fetch_add(1, Ordering::Relaxed);
                        let Some(&index) = wanted.get(at) else {
                            break;
                        };
                        let outcome = self.read_one(&bundles[index], bridge_folders);
                        found.lock().unwrap().push((index, outcome));
                    }
                });
            }
        });
        let mut known = self.known.lock().unwrap();
        for (index, outcome) in found.into_inner().unwrap() {
            if let Some(stamp) = &stamps[index] {
                known.insert(stamp.clone(), outcome.clone());
            }
            results[index] = Some(outcome);
        }
        drop(known);
        results
            .into_iter()
            .map(|outcome| outcome.unwrap_or_else(|| Err("not read".to_string())))
            .collect()
    }

    fn read_one(&self, bundle: &Path, bridge_folders: &[PathBuf]) -> Outcome {
        self.probes.fetch_add(1, Ordering::Relaxed);
        let mut command = std::process::Command::new(&self.helper);
        command.arg(PROBE_FLAG).arg(bundle);
        for folder in bridge_folders {
            command.arg("--bridges").arg(folder);
        }
        // What a plugin prints while it loads is the plugin's business: the
        // answer is the last line on stdout, and stderr goes nowhere.
        command
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null());
        let mut child = command
            .spawn()
            .map_err(|e| format!("could not start the scanner: {e}"))?;
        let mut stdout = child.stdout.take();
        let reader = std::thread::spawn(move || {
            let mut text = String::new();
            if let Some(out) = stdout.as_mut() {
                use std::io::Read;
                let _ = out.read_to_string(&mut text);
            }
            text
        });
        let started = Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Some(status),
                Ok(None) if started.elapsed() >= self.timeout => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(5)),
                Err(e) => return Err(format!("the scanner was lost: {e}")),
            }
        };
        let text = reader.join().unwrap_or_default();
        let Some(status) = status else {
            return Err(format!(
                "did not answer within {} seconds while being loaded — left out",
                self.timeout.as_secs()
            ));
        };
        if let Some(answer) = text.lines().rev().find(|line| line.starts_with('{')) {
            return read_answer(answer);
        }
        Err(match crashed_by(&status) {
            Some(how) => format!("crashed while being loaded ({how}) — left out"),
            None => format!("the scanner stopped without an answer ({status})"),
        })
    }
}

#[cfg(unix)]
fn crashed_by(status: &std::process::ExitStatus) -> Option<String> {
    use std::os::unix::process::ExitStatusExt;
    status.signal().map(|signal| {
        let name = match signal {
            6 => "SIGABRT",
            11 => "SIGSEGV",
            7 => "SIGBUS",
            4 => "SIGILL",
            8 => "SIGFPE",
            _ => return format!("signal {signal}"),
        };
        name.to_string()
    })
}

#[cfg(not(unix))]
fn crashed_by(status: &std::process::ExitStatus) -> Option<String> {
    // A Windows process that faults exits with the exception's code; one
    // that calls `abort` exits with 3. The child answers before it exits
    // and exits with 0, so any other end without an answer is a crash.
    status.code().filter(|code| *code != 0).map(|code| {
        if (code as u32) >= 0xC000_0000 {
            format!("exception 0x{:08X}", code as u32)
        } else {
            format!("exit code {code}")
        }
    })
}

// ------------------------------------------------------------ the wire

fn info_to_json(info: &PluginInfo) -> serde_json::Value {
    serde_json::json!({
        "key": info.key.to_string(),
        "path": info.path,
        "name": info.name,
        "vendor": info.vendor,
        "version": info.version,
        "features": info.features,
    })
}

fn info_from_json(value: &serde_json::Value) -> Option<PluginInfo> {
    Some(PluginInfo {
        key: PluginKey::parse(value.get("key")?.as_str()?)?,
        path: PathBuf::from(value.get("path")?.as_str()?),
        name: value.get("name")?.as_str()?.to_string(),
        vendor: value.get("vendor")?.as_str()?.to_string(),
        version: value.get("version")?.as_str()?.to_string(),
        features: value
            .get("features")?
            .as_array()?
            .iter()
            .filter_map(|f| f.as_str().map(str::to_string))
            .collect(),
    })
}

fn outcome_to_json(outcome: &Outcome) -> serde_json::Value {
    match outcome {
        Ok(found) => {
            serde_json::json!({ "ok": found.iter().map(info_to_json).collect::<Vec<_>>() })
        }
        Err(why) => serde_json::json!({ "err": why }),
    }
}

fn outcome_from_json(value: &serde_json::Value) -> Option<Outcome> {
    if let Some(found) = value.get("ok").and_then(|v| v.as_array()) {
        return Some(Ok(found.iter().filter_map(info_from_json).collect()));
    }
    value
        .get("err")
        .and_then(|v| v.as_str())
        .map(|why| Err(why.to_string()))
}

fn read_answer(line: &str) -> Outcome {
    serde_json::from_str::<serde_json::Value>(line)
        .ok()
        .and_then(|value| outcome_from_json(&value))
        .unwrap_or_else(|| Err("the scanner's answer could not be read".to_string()))
}

const CACHE_VERSION: u64 = 1;

fn write_cache(known: &HashMap<Stamp, Outcome>) -> String {
    let mut entries: Vec<serde_json::Value> = known
        .iter()
        .map(|(stamp, outcome)| {
            serde_json::json!({
                "path": stamp.path,
                "len": stamp.len,
                "modified": stamp.modified,
                "outcome": outcome_to_json(outcome),
            })
        })
        .collect();
    entries.sort_by(|a, b| a["path"].as_str().cmp(&b["path"].as_str()));
    serde_json::to_string_pretty(&serde_json::json!({
        "version": CACHE_VERSION,
        "bundles": entries,
    }))
    .unwrap_or_default()
}

fn read_cache(text: &str) -> Vec<(Stamp, Outcome)> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
        return Vec::new();
    };
    if value.get("version").and_then(|v| v.as_u64()) != Some(CACHE_VERSION) {
        return Vec::new();
    }
    value
        .get("bundles")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            Some((
                Stamp {
                    path: PathBuf::from(entry.get("path")?.as_str()?),
                    len: entry.get("len")?.as_u64()?,
                    modified: entry.get("modified")?.as_u64()?,
                },
                outcome_from_json(entry.get("outcome")?)?,
            ))
        })
        .collect()
}

// ------------------------------------------------------------ the child

/// The child's half: reads the bundle named after [`PROBE_FLAG`] in `args`
/// (everything after the program's name) and prints the answer as one line
/// of JSON. Returns the process's exit code. `None` when `args` are not a
/// probe's, so the caller goes on to whatever else it is.
pub fn probe_main(args: &[String]) -> Option<i32> {
    let at = args.iter().position(|a| a == PROBE_FLAG)?;
    let Some(bundle) = args.get(at + 1) else {
        println!("{}", outcome_to_json(&Err("no bundle named".to_string())));
        return Some(2);
    };
    let mut folders = Vec::new();
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        if arg == "--bridges"
            && let Some(folder) = rest.next()
        {
            folders.push(PathBuf::from(folder));
        }
    }
    let bridges = crate::Bridges::load(&folders);
    let outcome = crate::scan::scan_bundle_with(Path::new(bundle), &bridges);
    // The answer, then out at once: a plugin's static destructors run at a
    // normal exit, and one that crashes there would turn a bundle that was
    // read into one that looks as if it was not.
    use std::io::Write;
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{}", outcome_to_json(&outcome));
    let _ = out.flush();
    unsafe_quick_exit()
}

fn unsafe_quick_exit() -> ! {
    // `_exit`: no atexit handlers, no static destructors — the plugin's.
    #[cfg(unix)]
    unsafe {
        unsafe extern "C" {
            fn _exit(code: i32) -> !;
        }
        _exit(0)
    }
    #[cfg(not(unix))]
    std::process::exit(0)
}

/// Whether a bundle of `format` is read at all by this host or a bridge.
pub(crate) fn needs_loading_with(format: &PluginFormat, bridges: &crate::Bridges) -> bool {
    format.hosted() || bridges.serves(*format)
}

/// Whether a bundle has to be loaded to be read — and so is read by a child.
pub(crate) fn needs_loading(path: &Path, format: PluginFormat) -> bool {
    match format {
        PluginFormat::Lv2 => false,
        PluginFormat::Vst3 => !crate::vst3::has_moduleinfo(path),
        _ => true,
    }
}
