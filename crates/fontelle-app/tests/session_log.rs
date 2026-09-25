//! The session log: what a run said, kept where somebody can find it.
//!
//! > *"if you can get the error log that would be helpful" — "where get" —
//! > "im not sure how to do it on windows. for me i just run it in the
//! > terminal"*
//!
//! A studio started from a shortcut has no terminal, so everything it said
//! went nowhere. Each run now writes what it prints to a file of its own in
//! `logs/` under Fontelle's data folder, beside the crash reports, and the
//! start menu opens that folder.
//!
//! Its own test binary, because [`logs::start`] takes over the process's
//! stderr and stdout.

use std::io::Write;
use std::path::Path;
use std::time::{Duration, Instant};

use fontelle_app::logs;

#[test]
fn the_logs_live_in_their_own_folder_under_the_data_folder() {
    let data = Path::new("/home/someone/.local/share/fontelle");
    assert_eq!(logs::logs_dir(data), data.join("logs"));
}

#[test]
fn a_session_log_is_named_by_when_it_started_so_they_sort_and_read() {
    // 2026-09-24 22:05:09 UTC.
    assert_eq!(
        logs::session_log_name(1_790_287_509),
        "fontelle-2026-09-24_22-05-09.log"
    );
    // The epoch, and a leap day, because a calendar written by hand is
    // wrong at the edges first.
    assert_eq!(
        logs::session_log_name(0),
        "fontelle-1970-01-01_00-00-00.log"
    );
    assert_eq!(
        logs::session_log_name(1_709_164_800),
        "fontelle-2024-02-29_00-00-00.log"
    );
}

#[test]
fn old_session_logs_are_let_go_but_nothing_else_in_the_folder_is() {
    let dir = std::env::temp_dir().join("fontelle-session-log-prune");
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    for day in 1..=9 {
        let name = format!("fontelle-2026-09-0{day}_12-00-00.log");
        std::fs::write(dir.join(name), "a run").expect("a log");
    }
    // A crash report is evidence, and somebody's own file is theirs.
    std::fs::write(dir.join("crash-000000000001.log"), "a crash").expect("a report");
    std::fs::write(dir.join("notes.txt"), "mine").expect("a file");

    logs::prune(&dir, 3);

    let mut left: Vec<String> = std::fs::read_dir(&dir)
        .expect("the folder")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    left.sort();
    assert_eq!(
        left,
        vec![
            "crash-000000000001.log",
            "fontelle-2026-09-07_12-00-00.log",
            "fontelle-2026-09-08_12-00-00.log",
            "fontelle-2026-09-09_12-00-00.log",
            "notes.txt",
        ]
    );
}

#[test]
fn what_the_program_prints_lands_in_the_session_log() {
    let dir = std::env::temp_dir().join("fontelle-session-log-tee");
    std::fs::remove_dir_all(&dir).ok();

    let path = logs::start(&dir).expect("a session log");
    assert!(path.starts_with(&dir), "{path:?} is in the logs folder");

    // Straight to the handles rather than through `eprintln!`, which the
    // test harness captures before it reaches either.
    std::io::stderr()
        .write_all(b"the plugin refused to attach its editor\n")
        .unwrap();
    std::io::stdout()
        .write_all(b"  (no MIDI inputs found)\n")
        .unwrap();

    // The copy is made on a thread of its own, so it is waited for — with a
    // deadline, because a tee that never arrives is the failure being tested.
    let deadline = Instant::now() + Duration::from_secs(5);
    let text = loop {
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        if (text.contains("refused to attach") && text.contains("no MIDI inputs"))
            || Instant::now() > deadline
        {
            break text;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    assert!(
        text.contains("the plugin refused to attach its editor"),
        "stderr reaches the log:\n{text}"
    );
    assert!(
        text.contains("(no MIDI inputs found)"),
        "and so does stdout:\n{text}"
    );
    // A log sent in a bug report has to say what it is a log *of*.
    assert!(
        text.contains(env!("CARGO_PKG_VERSION")),
        "the version heads it:\n{text}"
    );
    assert!(
        text.contains(std::env::consts::OS),
        "and the platform:\n{text}"
    );
}
