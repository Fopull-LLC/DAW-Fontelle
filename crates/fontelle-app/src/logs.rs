//! What a run said, kept in a file somebody can send.
//!
//! > *"if you can get the error log that would be helpful" — "where get" —
//! > "im not sure how to do it on windows. for me i just run it in the
//! > terminal and it outputs the logs there for me to copy"*
//!
//! Everything Fontelle has to say goes to stderr and stdout: a plugin that
//! refused its editor, a device that would not open, a scan that skipped a
//! file. From a terminal that is enough. From a shortcut there is no terminal,
//! and on Windows none anybody can find, so all of it went nowhere and a bug
//! report had nothing in it.
//!
//! [`start`] gives each run its own file in `logs/` under Fontelle's data
//! folder, beside the crash reports ([`crate::crashlog`]), and **tees** both
//! streams into it — the terminal still gets every line, so launching from
//! one works as it always did. The start menu's *Logs folder* opens the
//! folder, which is the whole answer to "where get".
//!
//! The copy is made by a thread per stream reading a pipe the stream now
//! writes into, and written straight to the file with no buffer of its own,
//! so a line printed just before the process dies is on disk rather than in
//! a buffer that died with it.
//!
//! Like the crash log, **a log that cannot be written never stops the
//! program**: every failure here is an absent log, not an error.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

/// How many session logs are kept. A month of daily use, and a folder small
/// enough to send whole.
pub const KEEP: usize = 30;

/// The folder the logs and the crash reports are in.
pub fn logs_dir(data: &Path) -> PathBuf {
    data.join("logs")
}

/// The folder for this machine, if Fontelle has a data folder at all.
pub fn default_dir() -> Option<PathBuf> {
    crate::settings::Settings::data_dir().map(|data| logs_dir(&data))
}

/// What the log of a run that started at `unix` (seconds, UTC) is called:
/// `fontelle-2026-09-24_22-05-09.log`.
///
/// The date is written out rather than left as a count of seconds because
/// the person choosing which file to attach is reading the names, and it is
/// written biggest-first so the names sort in the order the runs happened.
pub fn session_log_name(unix: u64) -> String {
    let (date, time) = utc(unix);
    format!("fontelle-{date}_{}.log", time.replace(':', "-"))
}

/// `("2026-09-24", "22:05:09")` for `unix` seconds.
fn utc(unix: u64) -> (String, String) {
    let (year, month, day) = civil_from_days((unix / 86_400) as i64);
    let seconds = unix % 86_400;
    (
        format!("{year:04}-{month:02}-{day:02}"),
        format!(
            "{:02}:{:02}:{:02}",
            seconds / 3_600,
            seconds / 60 % 60,
            seconds % 60
        ),
    )
}

/// Days since 1970-01-01 as a proleptic Gregorian `(year, month, day)` —
/// Howard Hinnant's `civil_from_days`, which is exact for every day an
/// `i64` can count and saves a date crate for one file name.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// Whether `name` is a session log — the only files [`prune`] may touch.
fn is_session_log(name: &str) -> bool {
    name.starts_with("fontelle-") && name.ends_with(".log")
}

/// Lets go of all but the newest `keep` session logs in `dir`.
///
/// Only session logs: a crash report is the evidence somebody is about to be
/// asked for, and anything else in the folder is somebody's own.
pub fn prune(dir: &Path, keep: usize) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut logs: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .is_some_and(|name| is_session_log(&name.to_string_lossy()))
        })
        .collect();
    logs.sort();
    let surplus = logs.len().saturating_sub(keep);
    for old in logs.into_iter().take(surplus) {
        let _ = std::fs::remove_file(old);
    }
}

/// This run's log, once [`start`] has made one.
static CURRENT: OnceLock<PathBuf> = OnceLock::new();

/// This run's log, if there is one.
pub fn current() -> Option<&'static Path> {
    CURRENT.get().map(PathBuf::as_path)
}

/// Opens this run's log in `dir` and sends stderr and stdout through it.
///
/// Answers where the log is, or `None` when there is none — an unwritable
/// folder, a platform that would not make a pipe. Once per process: a second
/// call answers the first call's file.
pub fn start(dir: &Path) -> Option<PathBuf> {
    if let Some(path) = CURRENT.get() {
        return Some(path.clone());
    }
    std::fs::create_dir_all(dir).ok()?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // Two launches inside one second get two files, not one file with the
    // second run's header in the middle of the first's.
    let base = session_log_name(now);
    let mut path = dir.join(&base);
    let mut n = 1;
    while path.exists() {
        n += 1;
        path = dir.join(base.replace(".log", &format!("-{n}.log")));
    }
    let mut file = std::fs::File::create(&path).ok()?;
    use std::io::Write;
    let (date, time) = utc(now);
    let _ = writeln!(
        file,
        "Fontelle {} on {} {} \u{2014} started {date} {time} UTC\n",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
    );
    prune(dir, KEEP);
    let file = Arc::new(Mutex::new(file));
    platform::tee(platform::Stream::Stderr, Arc::clone(&file));
    platform::tee(platform::Stream::Stdout, file);
    let _ = CURRENT.set(path.clone());
    Some(path)
}

/// Copies everything read from `reader` into the log and on to the stream's
/// old destination, until the pipe closes.
fn pump(
    mut reader: impl std::io::Read,
    log: &Mutex<std::fs::File>,
    mut terminal: Option<impl std::io::Write>,
) {
    use std::io::Write;
    let mut buffer = [0u8; 8192];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => {
                if let Ok(mut file) = log.lock() {
                    let _ = file.write_all(&buffer[..n]);
                }
                if let Some(out) = terminal.as_mut() {
                    // A terminal that went away (the launching shell closed)
                    // is no reason to stop keeping the log.
                    if out.write_all(&buffer[..n]).is_err() {
                        terminal = None;
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => break,
        }
    }
}

#[cfg(unix)]
mod platform {
    use std::fs::File;
    use std::os::fd::FromRawFd;
    use std::sync::{Arc, Mutex};

    pub enum Stream {
        Stderr,
        Stdout,
    }

    /// Points the stream's descriptor at a pipe and copies the pipe into the
    /// log and on to wherever the descriptor pointed before.
    pub fn tee(stream: Stream, log: Arc<Mutex<File>>) {
        let fd = match stream {
            Stream::Stderr => libc::STDERR_FILENO,
            Stream::Stdout => libc::STDOUT_FILENO,
        };
        let mut ends = [0; 2];
        // SAFETY: plain descriptor calls on descriptors this function owns;
        // every failure path closes what it opened and leaves `fd` as it was.
        unsafe {
            if libc::pipe(ends.as_mut_ptr()) != 0 {
                return;
            }
            let [read, write] = ends;
            let original = libc::dup(fd);
            if libc::dup2(write, fd) < 0 {
                libc::close(read);
                libc::close(write);
                if original >= 0 {
                    libc::close(original);
                }
                return;
            }
            libc::close(write);
            // Neither end is any child's business: a `curl` or a picker run
            // from here inherits the stream itself, which is the pipe.
            libc::fcntl(read, libc::F_SETFD, libc::FD_CLOEXEC);
            if original >= 0 {
                libc::fcntl(original, libc::F_SETFD, libc::FD_CLOEXEC);
            }
            let reader = File::from_raw_fd(read);
            let terminal = (original >= 0).then(|| File::from_raw_fd(original));
            let _ = std::thread::Builder::new()
                .name("fontelle-log".to_string())
                .spawn(move || super::pump(reader, &log, terminal));
        }
    }
}

#[cfg(windows)]
mod platform {
    use std::fs::File;
    use std::os::windows::io::FromRawHandle;
    use std::sync::{Arc, Mutex};

    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Console::{
        GetStdHandle, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE, SetConsoleOutputCP, SetStdHandle,
    };
    use windows_sys::Win32::System::Pipes::CreatePipe;

    pub enum Stream {
        Stderr,
        Stdout,
    }

    /// Windows' half: `SetStdHandle` rather than `dup2`, which is enough for
    /// everything Rust prints — std asks for the handle on every write.
    pub fn tee(stream: Stream, log: Arc<Mutex<File>>) {
        let which = match stream {
            Stream::Stderr => STD_ERROR_HANDLE,
            Stream::Stdout => STD_OUTPUT_HANDLE,
        };
        // SAFETY: handle calls on handles this function owns; the original
        // is only ever written to, never closed — it is the console's.
        unsafe {
            let mut read: HANDLE = std::ptr::null_mut();
            let mut write: HANDLE = std::ptr::null_mut();
            if CreatePipe(&mut read, &mut write, std::ptr::null(), 0) == 0 {
                return;
            }
            let original = GetStdHandle(which);
            if SetStdHandle(which, write) == 0 {
                CloseHandle(read);
                CloseHandle(write);
                return;
            }
            // The bytes that reach a console from here are UTF-8, and a
            // console left on its OEM code page prints every dash as three
            // characters of nonsense.
            SetConsoleOutputCP(65001);
            let reader = File::from_raw_handle(read);
            let terminal = (!original.is_null() && original != INVALID_HANDLE_VALUE)
                .then(|| std::mem::ManuallyDrop::new(File::from_raw_handle(original)));
            let _ = std::thread::Builder::new()
                .name("fontelle-log".to_string())
                .spawn(move || super::pump(reader, &log, terminal.map(Console)));
        }
    }

    /// The console, written to and never closed.
    struct Console(std::mem::ManuallyDrop<File>);

    impl std::io::Write for Console {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            (&*self.0).write(buf)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            (&*self.0).flush()
        }
    }
}

#[cfg(not(any(unix, windows)))]
mod platform {
    pub enum Stream {
        Stderr,
        Stdout,
    }
    pub fn tee(_: Stream, _: std::sync::Arc<std::sync::Mutex<std::fs::File>>) {}
}
