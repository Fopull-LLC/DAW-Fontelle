//! Why the last run went away.
//!
//! > *"for some reason the daw keeps crashing a lot but it doesnt reproduce
//! > cleanly. i basically just use the daw and it crashes at a certain action
//! > but if i open it up again and do that same action its not guarenteed to
//! > crash again. its strange."*
//!
//! A window that vanishes leaves nothing behind. A panic writes its message to
//! a stderr nobody is reading — a studio started from a launcher has no
//! terminal at all — and a process **ended from outside** leaves not even
//! that: no message, no core dump, no journal entry, nothing anywhere on the
//! machine. The two are indistinguishable after the fact and have nothing to
//! do with each other, so the first thing worth building is not a fix but a
//! way to *tell them apart*. (On 2026-09-10 it turned out to be the second
//! one, every time: an agent session's `pkill -x fontelle` closing the window
//! out from under the user, which is why no action ever reproduced it.)
//!
//! The mechanism is one file. [`begin`] writes a **marker** naming the run;
//! [`end`] removes it on the way out. What the next launch finds says what
//! happened:
//!
//! | marker | report | verdict |
//! |---|---|---|
//! | absent | — | [`LastRun::Clean`] — it closed properly, or this is the first run |
//! | present | present | [`LastRun::Panicked`] — the report says where |
//! | present | absent | [`LastRun::Killed`] — nothing in the program went wrong |
//!
//! **Diagnostics never take the program down.** Every failure in here — an
//! unwritable directory, a truncated marker, a clock before the epoch — is
//! swallowed and reported as "nothing to say". A studio that refused to open
//! because it could not write a log would be a worse bug than the one it is
//! trying to catch.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// What a run leaves behind to say it is under way.
///
/// One line of `key=value`, because a half-written file has to be
/// *unparseable* rather than plausible: a marker read wrongly would report a
/// crash that never happened. Where the song lives and where an untitled one
/// is backed up follow on lines of their own ([`text`](Self::text)): each is
/// a path, read to the end of its line, and an older Fontelle reads only the
/// first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Marker {
    pub pid: u32,
    pub version: String,
    /// When the run started, in seconds since the epoch.
    pub started: u64,
    /// What was open, so a report says which project was in front of somebody
    /// when it went.
    pub project: Option<String>,
    /// The bundle of the song that was open, if it had one — whose
    /// `backups/autosave.fontelle` the next launch may offer.
    pub path: Option<PathBuf>,
    /// Where a song never saved was backed up — see [`recovery`].
    pub backup: Option<PathBuf>,
}

impl Marker {
    /// This process, now.
    pub fn here(project: Option<&str>) -> Self {
        Self {
            pid: std::process::id(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            started: now(),
            project: project.map(str::to_string),
            path: None,
            backup: None,
        }
    }

    /// The whole marker file: [`line`](Self::line), then a line for each
    /// path there is.
    pub fn text(&self) -> String {
        let flat = |path: &Path| {
            path.to_string_lossy()
                .chars()
                .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
                .collect::<String>()
        };
        let mut text = self.line();
        if let Some(path) = &self.path {
            text.push_str(&format!("\npath={}", flat(path)));
        }
        if let Some(backup) = &self.backup {
            text.push_str(&format!("\nbackup={}", flat(backup)));
        }
        text
    }

    /// The one line written to the marker file.
    ///
    /// Values are written with their whitespace flattened: the whole file is
    /// one line by construction, so a name with a newline in it cannot make
    /// the next launch read half a marker as a whole one.
    pub fn line(&self) -> String {
        let flat = |s: &str| {
            s.chars()
                .map(|c| if c.is_whitespace() { ' ' } else { c })
                .collect::<String>()
        };
        let mut line = format!(
            "pid={} version={} started={}",
            self.pid,
            flat(&self.version),
            self.started
        );
        if let Some(project) = &self.project {
            line.push_str(&format!(" project={}", flat(project)));
        }
        line
    }

    /// Reads one back. `None` for anything this build cannot make sense of —
    /// which is not the same statement as "it crashed", and must not be
    /// treated as one.
    pub fn parse(text: &str) -> Option<Self> {
        let line = text.lines().next()?.trim();
        if line.is_empty() {
            return None;
        }
        let mut pid = None;
        let mut version = None;
        let mut started = None;
        let mut project = None;
        // The project's name may hold spaces, so it is read to the end of the
        // line rather than as one word.
        for (index, field) in line.split(' ').enumerate() {
            let Some((key, value)) = field.split_once('=') else {
                continue;
            };
            match key {
                "pid" => pid = value.parse::<u32>().ok(),
                "version" => version = Some(value.to_string()),
                "started" => started = value.parse::<u64>().ok(),
                "project" => {
                    let rest: Vec<&str> = line.split(' ').skip(index).collect();
                    project = rest
                        .join(" ")
                        .strip_prefix("project=")
                        .map(str::to_string)
                        .filter(|s| !s.is_empty());
                    // **Nothing after the name is a field.** The name runs to
                    // the end of the line, so reading on would let a project
                    // called `pid=1 song` overwrite the pid this marker is
                    // about — and that pid is the number the "ended from
                    // outside" message prints at somebody.
                    break;
                }
                _ => {}
            }
        }
        // The lines after the first: a path each, to the end of its line.
        let mut path = None;
        let mut backup = None;
        for line in text.lines().skip(1) {
            if let Some(value) = line.strip_prefix("path=") {
                path = Some(PathBuf::from(value)).filter(|p| !p.as_os_str().is_empty());
            } else if let Some(value) = line.strip_prefix("backup=") {
                backup = Some(PathBuf::from(value)).filter(|p| !p.as_os_str().is_empty());
            }
        }
        Some(Self {
            pid: pid?,
            version: version?,
            started: started?,
            project,
            path,
            backup,
        })
    }
}

/// How the previous run ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LastRun {
    /// It closed properly — or there was no previous run. Nothing to say.
    Clean,
    /// Something in the program went wrong, and wrote `report`.
    Panicked { report: PathBuf, pid: u32 },
    /// Nothing in the program went wrong: the process was ended from outside
    /// it. A `pkill`, the OOM killer, a compositor or session restart, the
    /// machine losing power.
    Killed { pid: u32 },
}

impl LastRun {
    /// One line for the window's status bar, or `None` when there is no news.
    ///
    /// The wording is the point, twice over.
    ///
    /// **Which word.** *"Fontelle crashed"* over a kill from outside sends
    /// whoever reads it hunting a bug that is not there — a whole day went
    /// that way — so the two sentences say different things and only one of
    /// them uses the word.
    ///
    /// **And which end of the sentence.** The status line is one row at the
    /// foot of a 248-pixel panel and it clips at about forty characters: the
    /// first version of this said *"Fontelle did not close cleanly last
    /// time"* and stopped, which is the question rather than the answer. The
    /// verdict goes first and the detail follows it, for the terminal and the
    /// log to carry.
    pub fn message(&self) -> Option<String> {
        match self {
            Self::Clean => None,
            // The file's name and the folder's, not the whole path: the
            // start menu has two rows for this, and its *Logs folder* link
            // opens the folder.
            Self::Panicked { report, .. } => Some(format!(
                "Crashed last time \u{2014} the report is {} in the logs folder",
                report.file_name().map_or_else(
                    || report.display().to_string(),
                    |name| { name.to_string_lossy().into_owned() }
                )
            )),
            Self::Killed { pid } => Some(format!(
                "Not a Fontelle crash \u{2014} the last run was ended from outside \
                 (pid {pid}), and raised no error of its own"
            )),
        }
    }
}

/// The verdict, from what the previous run left behind.
///
/// Pure, so the table in this module's own documentation is a test rather than
/// a comment (`fontelle-app/tests/crash_report.rs`).
pub fn last_run(marker: Option<&str>, report: Option<&Path>) -> LastRun {
    // **The marker decides**, not the report: a report from an older run is
    // not evidence about this one, and a run that closed properly is not
    // retrospectively a crash because there is an old log beside it.
    let Some(marker) = marker.and_then(Marker::parse) else {
        return LastRun::Clean;
    };
    match report {
        Some(report) => LastRun::Panicked {
            report: report.to_path_buf(),
            pid: marker.pid,
        },
        None => LastRun::Killed { pid: marker.pid },
    }
}

/// What a crash report says.
pub fn report_text(
    payload: &str,
    location: Option<&str>,
    backtrace: &str,
    marker: &Marker,
) -> String {
    let mut text = String::new();
    text.push_str("Fontelle crash report\n");
    text.push_str("=====================\n\n");
    text.push_str(&format!("version:   {}\n", marker.version));
    text.push_str(&format!("pid:       {}\n", marker.pid));
    text.push_str(&format!("started:   {} (unix)\n", marker.started));
    text.push_str(&format!("crashed:   {} (unix)\n", now()));
    text.push_str(&format!(
        "project:   {}\n",
        marker.project.as_deref().unwrap_or("(none open)")
    ));
    text.push_str(&format!(
        "where:     {}\n\n",
        location.unwrap_or("(the panic carried no location)")
    ));
    text.push_str("what went wrong\n---------------\n");
    text.push_str(payload);
    text.push('\n');
    if backtrace.trim().is_empty() {
        // Worth saying rather than leaving a blank heading: the difference
        // between "no backtrace" and "RUST_BACKTRACE was not set" is the
        // difference between a bug report I can act on and one I cannot.
        text.push_str(
            "\nbacktrace\n---------\n(none \u{2014} run Fontelle with RUST_BACKTRACE=1 \
             to get one in the next report)\n",
        );
    } else {
        text.push_str("\nbacktrace\n---------\n");
        text.push_str(backtrace);
        text.push('\n');
    }
    text
}

/// What a report says when the program did not panic but **faulted**: a
/// signal on Linux and macOS, an unhandled exception on Windows.
///
/// > *"oh now it crashed xD"*
///
/// There is no message and no backtrace to give — the fault is in native
/// code, most often a plugin's — so it says what the fault was and, where
/// the platform can say it, **which module** the faulting address is in.
/// That one line is the difference between "a plugin crashed" and "Fontelle
/// crashed", which is the first thing anybody reading it needs to know.
pub fn native_report_text(what: &str, module: Option<&str>, marker: &Marker) -> String {
    let mut text = String::new();
    text.push_str("Fontelle crash report\n");
    text.push_str("=====================\n\n");
    text.push_str(&format!("version:   {}\n", marker.version));
    text.push_str(&format!("pid:       {}\n", marker.pid));
    text.push_str(&format!("started:   {} (unix)\n", marker.started));
    text.push_str(&format!(
        "project:   {}\n",
        marker.project.as_deref().unwrap_or("(none open)")
    ));
    text.push_str(&format!(
        "module:    {}\n\n",
        module.unwrap_or("(this platform does not say)")
    ));
    text.push_str("what went wrong\n---------------\n");
    text.push_str(what);
    text.push_str(
        "\n\nThis was a fault in native code rather than a panic, so there is no \
         message from Fontelle itself. If the module above is a plugin, the plugin \
         crashed; the session log beside this report says what was happening.\n",
    );
    text
}

/// The plugin a report says the crashing thread was inside, if it says one
/// — the `plugin:` line the fault handler writes from
/// [`fontelle_host::guard::current`].
pub fn culprit(report: &str) -> Option<fontelle_types::PluginKey> {
    let line = report
        .lines()
        .find_map(|line| line.strip_prefix(PLUGIN_LINE))?;
    let (_, key) = line.rsplit_once('\t')?;
    fontelle_types::PluginKey::parse(key.trim())
}

/// The plugin the faulting thread was inside, failing that the one the main
/// thread was, and failing that the one whose editor has just opened or
/// closed — async-signal-safe (`fontelle_host::guard`).
fn marked_plugin() -> Option<&'static [u8]> {
    fontelle_host::guard::current()
        .or_else(fontelle_host::guard::main)
        .or_else(|| fontelle_host::guard::editor(fontelle_host::guard::EDITOR_GRACE))
}

/// How a report's line naming the plugin begins — the name and the key
/// follow, a tab between them (`fontelle_host::guard::Label`).
pub const PLUGIN_LINE: &str = "plugin:    ";

/// What a report written at `unix` is called.
///
/// Zero-padded so the file names sort in the order the crashes happened, which
/// is what makes "the newest one" a `max()` rather than a stat of every file.
pub fn report_name(unix: u64) -> String {
    format!("crash-{unix:012}.log")
}

/// The marker file inside `dir`.
pub fn marker_path(dir: &Path) -> PathBuf {
    dir.join("running.marker")
}

/// Starts a run: reads what the last one left, clears it, writes this run's
/// marker, and installs the panic hook.
///
/// Returns what happened last time, for the window to say out loud.
pub fn begin(dir: &Path, project: Option<&str>) -> LastRun {
    begin_run(dir, project).verdict
}

/// What the run before this one left behind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Previous {
    pub verdict: LastRun,
    /// Its marker, when it did not end cleanly — where its song was.
    pub marker: Option<Marker>,
    /// The plugin its crash report says the crashing thread was inside —
    /// see [`culprit`].
    pub culprit: Option<fontelle_types::PluginKey>,
}

/// [`begin`], keeping everything the last run left: what is offered on the
/// start menu ([`recovery`]) and the plugin held back ([`culprit`]).
pub fn begin_run(dir: &Path, project: Option<&str>) -> Previous {
    // Read *before* writing this run's marker, or the run would find its own.
    let left_behind = std::fs::read_to_string(marker_path(dir)).ok();
    let newest = left_behind.as_ref().and_then(|_| newest_report(dir));
    let verdict = last_run(left_behind.as_deref(), newest.as_deref());
    let marker = left_behind.as_deref().and_then(Marker::parse);
    // Only a report written by that run says what that run was doing.
    let culprit = match (&verdict, &marker) {
        (LastRun::Panicked { report, .. }, Some(marker))
            if report_time(report).is_some_and(|at| at >= marker.started) =>
        {
            std::fs::read_to_string(report)
                .ok()
                .and_then(|text| culprit(&text))
        }
        _ => None,
    };

    let mine = Marker::here(project);
    // Nothing here is allowed to fail loudly: a studio that would not open
    // because a log directory is read-only is a worse bug than any it could
    // catch.
    let _ = std::fs::create_dir_all(dir);
    let _ = std::fs::write(marker_path(dir), mine.text());
    native::install(dir, &mine);
    if let Ok(mut run) = RUN.lock() {
        *run = Some((dir.to_path_buf(), mine));
    }
    install_hook(dir.to_path_buf());
    Previous {
        verdict,
        marker,
        culprit,
    }
}

/// When a report was written, from its name ([`report_name`]).
fn report_time(report: &Path) -> Option<u64> {
    report
        .file_name()?
        .to_str()?
        .strip_prefix("crash-")?
        .strip_suffix(".log")?
        .parse()
        .ok()
}

/// This run's marker and where it is written, once [`begin_run`] has run.
static RUN: std::sync::Mutex<Option<(PathBuf, Marker)>> = std::sync::Mutex::new(None);

/// Says which song is open now, and where an untitled one is backed up, so
/// a crash from here on is recovered into the right place. Nothing before
/// [`begin_run`] (a test, a bounce), and nothing loud if it fails.
pub fn note(project: Option<&str>, path: Option<&Path>, backup: Option<&Path>) {
    let Ok(mut run) = RUN.lock() else { return };
    let Some((dir, marker)) = run.as_mut() else {
        return;
    };
    let changed = marker.project.as_deref() != project
        || marker.path.as_deref() != path
        || marker.backup.as_deref() != backup;
    if !changed {
        return;
    }
    marker.project = project.map(str::to_string);
    marker.path = path.map(Path::to_path_buf);
    marker.backup = backup.map(Path::to_path_buf);
    let _ = std::fs::write(marker_path(dir), marker.text());
    // So a fault's report names the song open now, not the one at launch.
    native::install(dir, marker);
}

/// Ends a run cleanly: the marker goes, so the next launch has no news —
/// and an untitled song's backup with it, since a clean exit is somebody
/// having decided what to do with it.
pub fn end(dir: &Path) {
    if let Ok(run) = RUN.lock()
        && let Some(backup) = run.as_ref().and_then(|(_, marker)| marker.backup.clone())
    {
        let _ = std::fs::remove_dir_all(backup);
    }
    let _ = std::fs::remove_file(marker_path(dir));
}

/// What the start menu offers after a run that did not end cleanly: a
/// backup holding work its song on disk does not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recovery {
    /// The backup bundle.
    pub backup: PathBuf,
    /// The song it is a backup of, which it opens as; `None` for a song
    /// never saved, which opens untitled.
    pub home: Option<PathBuf>,
    /// What to call it on the menu.
    pub name: String,
}

/// Whether the last run left work worth offering back.
///
/// A saved song's backup (`backups/autosave.fontelle` inside it) when it is
/// newer than the song itself — an autosave writes only when there are
/// unsaved changes, so a newer one holds what the song does not — or an
/// untitled song's backup. Nothing after a clean exit: what was not saved
/// then was not wanted.
pub fn recovery(previous: &Previous) -> Option<Recovery> {
    if previous.verdict == LastRun::Clean {
        return None;
    }
    let marker = previous.marker.as_ref()?;
    let modified = |bundle: &Path| {
        std::fs::metadata(bundle.join("project.json"))
            .and_then(|m| m.modified())
            .ok()
    };
    if let Some(home) = &marker.path {
        let backup = home.join("backups").join("autosave.fontelle");
        let newer = match (modified(&backup), modified(home)) {
            (Some(backup), Some(song)) => backup > song,
            (Some(_), None) => true,
            _ => false,
        };
        let name = home
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .or_else(|| marker.project.clone())
            .unwrap_or_default();
        return newer.then(|| Recovery {
            backup,
            home: Some(home.clone()),
            name,
        });
    }
    let backup = marker.backup.as_ref()?;
    modified(backup).map(|_| Recovery {
        backup: backup.clone(),
        home: None,
        name: marker
            .project
            .clone()
            .unwrap_or_else(|| "Untitled".to_string()),
    })
}

/// The newest report in `dir`, by the name [`report_name`] gives them.
fn newest_report(dir: &Path) -> Option<PathBuf> {
    let mut best: Option<PathBuf> = None;
    for entry in std::fs::read_dir(dir).ok()? {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        let is_report = path
            .file_name()
            .map(|name| name.to_string_lossy())
            .is_some_and(|name| name.starts_with("crash-") && name.ends_with(".log"));
        if !is_report {
            continue;
        }
        if best.as_ref().is_none_or(|current| path > *current) {
            best = Some(path);
        }
    }
    best
}

/// Puts a report on disk whenever anything in this process panics.
///
/// Chained onto the hook that is already there rather than replacing it, so
/// the message still reaches stderr for whoever *is* watching a terminal.
fn install_hook(dir: PathBuf) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // The marker as it is now — the song open now, not the one at launch.
        let Some(marker) = RUN
            .try_lock()
            .ok()
            .and_then(|run| run.as_ref().map(|(_, marker)| marker.clone()))
        else {
            previous(info);
            return;
        };
        let payload = if let Some(s) = info.payload().downcast_ref::<&str>() {
            (*s).to_string()
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "(a panic whose payload is not a string)".to_string()
        };
        let location = info.location().map(|l| l.to_string());
        let backtrace = std::backtrace::Backtrace::force_capture().to_string();
        let mut text = report_text(&payload, location.as_deref(), &backtrace, &marker);
        if let Some(label) = marked_plugin() {
            text.push_str(PLUGIN_LINE);
            text.push_str(&String::from_utf8_lossy(label));
            text.push('\n');
        }
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join(report_name(now())), text);
        previous(info);
    }));
}

/// Seconds since the epoch, and zero for a clock that is before it — a
/// diagnostic must not be the thing that panics.
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Which file an address is mapped from, read out of `/proc/self/maps` a
/// piece at a time.
///
/// For the fault handler, which may not allocate: fixed buffers, fed whatever
/// `read` returns, a line kept across reads. A line longer than the buffer
/// is skipped rather than misread — a path that long is a report that says
/// "not in a file", not one that names the wrong one.
#[cfg(target_os = "linux")]
pub struct MapsScan {
    pc: usize,
    line: [u8; 512],
    len: usize,
    overflow: bool,
    found: [u8; 512],
    found_len: Option<usize>,
}

#[cfg(target_os = "linux")]
impl MapsScan {
    pub fn new(pc: usize) -> Self {
        Self {
            pc,
            line: [0; 512],
            len: 0,
            overflow: false,
            found: [0; 512],
            found_len: None,
        }
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            if byte == b'\n' {
                self.finish_line();
            } else if self.len < self.line.len() {
                self.line[self.len] = byte;
                self.len += 1;
            } else {
                self.overflow = true;
            }
        }
    }

    /// The last line, when the text did not end in a newline.
    pub fn finish(&mut self) {
        if self.len > 0 {
            self.finish_line();
        }
    }

    /// The file's path, if the address is in one.
    pub fn found(&self) -> Option<&[u8]> {
        self.found_len.map(|len| &self.found[..len])
    }

    fn finish_line(&mut self) {
        let (len, overflow) = (self.len, self.overflow);
        self.len = 0;
        self.overflow = false;
        if overflow || self.found_len.is_some() {
            return;
        }
        // `start-end perms offset dev inode   path`, the path being the
        // rest of the line and allowed spaces of its own.
        let line = &self.line[..len];
        let mut fields = line.splitn(6, |b| *b == b' ');
        let Some(range) = fields.next() else {
            return;
        };
        let mut ends = range.splitn(2, |b| *b == b'-');
        let (Some(start), Some(end)) = (ends.next().and_then(hex_of), ends.next().and_then(hex_of))
        else {
            return;
        };
        if !(start..end).contains(&self.pc) {
            return;
        }
        let path = fields.nth(4).unwrap_or(&[]);
        let path = match path.iter().position(|b| *b != b' ') {
            Some(first) => &path[first..],
            None => return,
        };
        let len = path.len().min(self.found.len());
        self.found[..len].copy_from_slice(&path[..len]);
        self.found_len = Some(len);
    }
}

#[cfg(target_os = "linux")]
fn hex_of(digits: &[u8]) -> Option<usize> {
    if digits.is_empty() || digits.len() > 16 {
        return None;
    }
    digits.iter().try_fold(0usize, |value, digit| {
        let d = (*digit as char).to_digit(16)?;
        Some(value << 4 | d as usize)
    })
}

/// [`MapsScan`] over a whole copy of the maps, for what is not a handler.
#[cfg(target_os = "linux")]
pub fn module_in_maps(maps: &[u8], pc: usize) -> Option<String> {
    let mut scan = MapsScan::new(pc);
    scan.feed(maps);
    scan.finish();
    scan.found()
        .map(|path| String::from_utf8_lossy(path).into_owned())
}

/// Writing a report when native code faults.
///
/// Installed by [`begin`] beside the panic hook. **Chained, never
/// swallowing**: after the report is written the fault goes on to whatever
/// would have handled it — Rust's own stack-overflow message, the system's
/// crash dialog, a core dump — so this adds a file and takes nothing away.
#[cfg(unix)]
mod native {
    use std::ffi::CString;
    use std::path::Path;
    use std::sync::OnceLock;
    use std::sync::atomic::{AtomicPtr, Ordering};

    /// The synchronous faults, and the abort a C++ `std::terminate` ends in.
    const SIGNALS: [(libc::c_int, &str); 5] = [
        (
            libc::SIGSEGV,
            "SIGSEGV \u{2014} a read or write of memory that was not there",
        ),
        (libc::SIGBUS, "SIGBUS \u{2014} a bad memory access"),
        (libc::SIGILL, "SIGILL \u{2014} an illegal instruction"),
        (libc::SIGFPE, "SIGFPE \u{2014} an arithmetic fault"),
        (libc::SIGABRT, "SIGABRT \u{2014} the process aborted"),
    ];

    /// Everything the handler needs, made **before** there is a fault: a
    /// signal handler may not allocate, format or lock, so the report for
    /// each signal is written out in full now and the handler only copies
    /// bytes to a file.
    struct Prepared {
        path: CString,
        /// Per signal, the report up to where the module is named and the
        /// rest after it: on Linux the handler fills in the module, the
        /// thread and the addresses between the two.
        reports: Vec<(libc::c_int, Vec<u8>, Vec<u8>)>,
    }

    /// Stands where the module goes while the report is prepared.
    const MODULE_HERE: &str = "\u{1}module\u{1}";

    /// The current run's, swapped whole when `begin` is called again.
    static PREPARED: AtomicPtr<Prepared> = AtomicPtr::new(std::ptr::null_mut());
    /// What each signal did before, to hand the fault on to.
    static PREVIOUS: OnceLock<Vec<(libc::c_int, libc::sigaction)>> = OnceLock::new();

    pub(super) fn install(dir: &Path, marker: &super::Marker) {
        use std::os::unix::ffi::OsStrExt;
        let path = dir.join(super::report_name(marker.started));
        let Ok(path) = CString::new(path.as_os_str().as_bytes()) else {
            return;
        };
        let reports = SIGNALS
            .iter()
            .map(|(signal, what)| {
                if cfg!(target_os = "linux") {
                    let text = super::native_report_text(what, Some(MODULE_HERE), marker);
                    let (head, tail) = text.split_once(MODULE_HERE).unwrap_or((&text, ""));
                    (*signal, head.as_bytes().to_vec(), tail.as_bytes().to_vec())
                } else {
                    let text = super::native_report_text(what, None, marker);
                    (*signal, text.into_bytes(), Vec::new())
                }
            })
            .collect();
        // Leaked on purpose: a handler may read it at any moment for the rest
        // of the process, and `begin` runs once or twice a process.
        let prepared = Box::into_raw(Box::new(Prepared { path, reports }));
        PREPARED.store(prepared, Ordering::Release);
        PREVIOUS.get_or_init(|| {
            SIGNALS
                .iter()
                .filter_map(|(signal, _)| {
                    // SAFETY: `sigaction` with a zeroed, fully initialised
                    // struct; the handler it installs is async-signal-safe.
                    unsafe {
                        let mut ours: libc::sigaction = std::mem::zeroed();
                        ours.sa_sigaction = on_fault as *const () as usize;
                        ours.sa_flags = libc::SA_SIGINFO | libc::SA_ONSTACK;
                        libc::sigemptyset(&mut ours.sa_mask);
                        let mut before: libc::sigaction = std::mem::zeroed();
                        (libc::sigaction(*signal, &ours, &mut before) == 0)
                            .then_some((*signal, before))
                    }
                })
                .collect()
        });
    }

    /// Writes the prepared report for `signal`, puts the old handler back
    /// and lets the fault happen again under it.
    extern "C" fn on_fault(
        signal: libc::c_int,
        info: *mut libc::siginfo_t,
        context: *mut libc::c_void,
    ) {
        // SAFETY: only async-signal-safe calls (`open`, `read`, `write`,
        // `close`, `prctl`, `sigaction`, `raise`) on data prepared before
        // any fault, and on the kernel's own `siginfo`/`ucontext`.
        unsafe {
            let prepared = PREPARED.load(Ordering::Acquire);
            if let Some(prepared) = prepared.as_ref()
                && let Some((_, head, tail)) = prepared.reports.iter().find(|(s, ..)| *s == signal)
            {
                let fd = libc::open(
                    prepared.path.as_ptr(),
                    libc::O_WRONLY | libc::O_CREAT | libc::O_TRUNC | libc::O_CLOEXEC,
                    0o644,
                );
                if fd >= 0 {
                    let put = |bytes: &[u8]| {
                        libc::write(fd, bytes.as_ptr().cast(), bytes.len());
                    };
                    put(head);
                    #[cfg(target_os = "linux")]
                    if !tail.is_empty() {
                        where_it_was(signal, info, context, &put);
                        inside_plugin(&put);
                        // The tail opens with the blank line the module's
                        // line ended in; ours ended in their own.
                        put(&tail[1..]);
                    }
                    #[cfg(not(target_os = "linux"))]
                    {
                        let _ = (info, context, tail);
                        inside_plugin(&put);
                    }
                    libc::close(fd);
                }
            }
            if let Some(before) = PREVIOUS
                .get()
                .and_then(|all| all.iter().find(|(s, _)| *s == signal))
            {
                libc::sigaction(signal, &before.1, std::ptr::null_mut());
            }
            // A fault re-runs the instruction on return, now under the old
            // handler. An abort does not, so it is sent again.
            if signal == libc::SIGABRT {
                libc::raise(signal);
            }
        }
    }

    /// Which plugin the faulting thread was calling into, when the host had
    /// marked it — or, for a thread the host never marks (a plugin's own),
    /// the one the main thread was inside. See [`fontelle_host::guard`]. The
    /// module line says which *file* the instruction was in, and that is
    /// `libc` for a plugin that aborts; this says which plugin.
    fn inside_plugin(put: &dyn Fn(&[u8])) {
        if let Some(label) = super::marked_plugin() {
            put(super::PLUGIN_LINE.as_bytes());
            put(label);
            put(b"\n");
        }
    }

    /// The module, the thread and the addresses, written straight into the
    /// report: which file the faulting instruction is in (from
    /// `/proc/self/maps`, read with `open` and `read` — `dladdr` takes the
    /// loader's lock, which the fault may be holding), the thread's name,
    /// and for a memory fault the address it reached for.
    ///
    /// Four reports from a Fedora user said "module: (this platform does not
    /// say)", and so could not say whether the synth he was trying had
    /// crashed or we had.
    #[cfg(target_os = "linux")]
    unsafe fn where_it_was(
        signal: libc::c_int,
        info: *mut libc::siginfo_t,
        context: *mut libc::c_void,
        put: &dyn Fn(&[u8]),
    ) {
        // SAFETY: the kernel hands a handler installed with `SA_SIGINFO` a
        // valid `siginfo_t` and `ucontext_t`; both are only read.
        unsafe {
            let pc = code_address(context);
            let mut scan = super::MapsScan::new(pc);
            let maps = libc::open(
                c"/proc/self/maps".as_ptr(),
                libc::O_RDONLY | libc::O_CLOEXEC,
            );
            if pc != 0 && maps >= 0 {
                let mut buffer = [0u8; 1024];
                loop {
                    let n = libc::read(maps, buffer.as_mut_ptr().cast(), buffer.len());
                    if n <= 0 {
                        break;
                    }
                    scan.feed(&buffer[..n as usize]);
                }
                scan.finish();
            }
            if maps >= 0 {
                libc::close(maps);
            }
            put(scan
                .found()
                .unwrap_or(b"(not in any file \xe2\x80\x94 memory made at run time)"));
            put(b"\nthread:    ");
            let mut name = [0u8; 16];
            if libc::prctl(libc::PR_GET_NAME, name.as_mut_ptr()) == 0 {
                let len = name.iter().position(|b| *b == 0).unwrap_or(name.len());
                put(&name[..len]);
            }
            put(b"\ncode at:   ");
            put(&hex(pc));
            if matches!(signal, libc::SIGSEGV | libc::SIGBUS) && !info.is_null() {
                put(b"\nfault at:  ");
                put(&hex((*info).si_addr() as usize));
            }
            put(b"\n");
        }
    }

    /// Where the faulting instruction is, from the saved registers.
    #[cfg(target_os = "linux")]
    unsafe fn code_address(context: *mut libc::c_void) -> usize {
        if context.is_null() {
            return 0;
        }
        // SAFETY: see `where_it_was`.
        unsafe {
            let context = &*(context as *const libc::ucontext_t);
            #[cfg(target_arch = "x86_64")]
            {
                context.uc_mcontext.gregs[libc::REG_RIP as usize] as usize
            }
            #[cfg(target_arch = "aarch64")]
            {
                context.uc_mcontext.pc as usize
            }
            #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
            {
                let _ = context;
                0
            }
        }
    }

    /// `0x` and sixteen hex digits, without formatting machinery.
    #[cfg(target_os = "linux")]
    fn hex(value: usize) -> [u8; 18] {
        let mut out = [b'0'; 18];
        out[1] = b'x';
        for (i, slot) in out[2..].iter_mut().enumerate() {
            let digit = (value >> ((15 - i) * 4)) & 0xf;
            *slot = b"0123456789abcdef"[digit];
        }
        out
    }
}

#[cfg(windows)]
mod native {
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;

    use windows_sys::Win32::Foundation::HMODULE;
    use windows_sys::Win32::System::Diagnostics::Debug::{
        EXCEPTION_POINTERS, SetUnhandledExceptionFilter,
    };
    use windows_sys::Win32::System::LibraryLoader::{
        GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
        GetModuleFileNameW, GetModuleHandleExW,
    };

    /// Where the report goes, and what the run was.
    static RUN: Mutex<Option<(PathBuf, super::Marker)>> = Mutex::new(None);

    /// `EXCEPTION_CONTINUE_SEARCH`: the fault goes on to the system's own
    /// handling once the report is written.
    const CONTINUE_SEARCH: i32 = 0;

    pub(super) fn install(dir: &Path, marker: &super::Marker) {
        if let Ok(mut run) = RUN.lock() {
            *run = Some((dir.to_path_buf(), marker.clone()));
        }
        // SAFETY: installs a process-wide filter whose function lives for the
        // whole program.
        unsafe {
            SetUnhandledExceptionFilter(Some(on_exception));
        }
    }

    /// What an exception code means, in words.
    fn describe(code: i32) -> String {
        let words = match code as u32 {
            0xC000_0005 => "an access violation (a read or write of memory that was not there)",
            0xC000_00FD => "a stack overflow",
            0xC000_001D => "an illegal instruction",
            0xC000_0094 => "an integer division by zero",
            0xC000_0409 => "a stack buffer overrun, or a fast-fail abort",
            0xC000_0374 => "a corrupted heap",
            0x8000_0003 => "a breakpoint",
            0xE06D_7363 => "an uncaught C++ exception",
            _ => "an unhandled exception",
        };
        format!("{words} \u{2014} exception code 0x{:08X}", code as u32)
    }

    /// The file a code address is inside — a plugin's DLL, or Fontelle.
    fn module_of(address: *const std::ffi::c_void) -> Option<String> {
        let mut module: HMODULE = std::ptr::null_mut();
        // SAFETY: asks the loader about an address without taking a
        // reference; the buffer is a fixed array of the length passed.
        unsafe {
            if GetModuleHandleExW(
                GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS
                    | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                address.cast(),
                &mut module,
            ) == 0
            {
                return None;
            }
            let mut name = [0u16; 1024];
            let length = GetModuleFileNameW(module, name.as_mut_ptr(), name.len() as u32);
            (length > 0).then(|| String::from_utf16_lossy(&name[..length as usize]))
        }
    }

    unsafe extern "system" fn on_exception(info: *const EXCEPTION_POINTERS) -> i32 {
        // Best effort, on a process that is going down: a lock that is held
        // or a record that is not there is a report that is not written.
        let Ok(run) = RUN.try_lock() else {
            return CONTINUE_SEARCH;
        };
        let Some((dir, marker)) = run.as_ref() else {
            return CONTINUE_SEARCH;
        };
        // SAFETY: the system hands the filter a valid record for the fault.
        let (code, address) = unsafe {
            let Some(record) = info.as_ref().and_then(|info| info.ExceptionRecord.as_ref()) else {
                return CONTINUE_SEARCH;
            };
            (record.ExceptionCode, record.ExceptionAddress)
        };
        let what = format!("{} at {address:p}", describe(code));
        let module = module_of(address);
        let mut text = super::native_report_text(&what, module.as_deref(), marker);
        // Which plugin this thread was inside — see `fontelle_host::guard`.
        if let Some(label) = super::marked_plugin() {
            text.push_str(super::PLUGIN_LINE);
            text.push_str(&String::from_utf8_lossy(label));
            text.push('\n');
        }
        let _ = std::fs::create_dir_all(dir);
        let _ = std::fs::write(dir.join(super::report_name(super::now())), text);
        CONTINUE_SEARCH
    }
}

#[cfg(not(any(unix, windows)))]
mod native {
    pub(super) fn install(_: &std::path::Path, _: &super::Marker) {}
}
