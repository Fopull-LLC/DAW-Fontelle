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
/// crash that never happened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Marker {
    pub pid: u32,
    pub version: String,
    /// When the run started, in seconds since the epoch.
    pub started: u64,
    /// What was open, so a report says which project was in front of somebody
    /// when it went.
    pub project: Option<String>,
}

impl Marker {
    /// This process, now.
    pub fn here(project: Option<&str>) -> Self {
        Self {
            pid: std::process::id(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            started: now(),
            project: project.map(str::to_string),
        }
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
        Some(Self {
            pid: pid?,
            version: version?,
            started: started?,
            project,
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
    // Read *before* writing this run's marker, or the run would find its own.
    let left_behind = std::fs::read_to_string(marker_path(dir)).ok();
    let newest = left_behind.as_ref().and_then(|_| newest_report(dir));
    let verdict = last_run(left_behind.as_deref(), newest.as_deref());

    let marker = Marker::here(project);
    // Nothing here is allowed to fail loudly: a studio that would not open
    // because a log directory is read-only is a worse bug than any it could
    // catch.
    let _ = std::fs::create_dir_all(dir);
    let _ = std::fs::write(marker_path(dir), marker.line());
    native::install(dir, &marker);
    install_hook(dir.to_path_buf(), marker);
    verdict
}

/// Ends a run cleanly: the marker goes, so the next launch has no news.
pub fn end(dir: &Path) {
    let _ = std::fs::remove_file(marker_path(dir));
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
fn install_hook(dir: PathBuf, marker: Marker) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let payload = if let Some(s) = info.payload().downcast_ref::<&str>() {
            (*s).to_string()
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "(a panic whose payload is not a string)".to_string()
        };
        let location = info.location().map(|l| l.to_string());
        let backtrace = std::backtrace::Backtrace::force_capture().to_string();
        let text = report_text(&payload, location.as_deref(), &backtrace, &marker);
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
        reports: Vec<(libc::c_int, Vec<u8>)>,
    }

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
                let text = super::native_report_text(what, None, marker);
                (*signal, text.into_bytes())
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
        _info: *mut libc::siginfo_t,
        _context: *mut libc::c_void,
    ) {
        // SAFETY: only async-signal-safe calls (`open`, `write`, `close`,
        // `sigaction`, `raise`) on data prepared before any fault.
        unsafe {
            let prepared = PREPARED.load(Ordering::Acquire);
            if let Some(prepared) = prepared.as_ref()
                && let Some((_, text)) = prepared.reports.iter().find(|(s, _)| *s == signal)
            {
                let fd = libc::open(
                    prepared.path.as_ptr(),
                    libc::O_WRONLY | libc::O_CREAT | libc::O_TRUNC | libc::O_CLOEXEC,
                    0o644,
                );
                if fd >= 0 {
                    libc::write(fd, text.as_ptr().cast(), text.len());
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
        let text = super::native_report_text(&what, module.as_deref(), marker);
        let _ = std::fs::create_dir_all(dir);
        let _ = std::fs::write(dir.join(super::report_name(super::now())), text);
        CONTINUE_SEARCH
    }
}

#[cfg(not(any(unix, windows)))]
mod native {
    pub(super) fn install(_: &std::path::Path, _: &super::Marker) {}
}
