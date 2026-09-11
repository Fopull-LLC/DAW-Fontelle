//! Why the last run went away — the pure half of [`fontelle_app::crashlog`].
//!
//! The module's own documentation carries a table of what the next launch
//! concludes from what the last one left behind. This file is that table, so
//! it is a claim rather than a comment.
//!
//! **Nothing here calls `crashlog::begin`**, which installs a process-wide
//! panic hook: under that hook every assertion failure in this binary would
//! write a crash report of its own. The hook has its own test binary
//! (`crash_report_hook.rs`) for exactly that reason.

use std::path::{Path, PathBuf};

use fontelle_app::crashlog::{LastRun, Marker, last_run, report_name, report_text};

fn marker() -> Marker {
    Marker {
        pid: 3538614,
        version: "0.0.0".to_string(),
        started: 1_757_500_000,
        project: Some("SynthTesty".to_string()),
    }
}

// ------------------------------------------------------------ the table ---

#[test]
fn no_marker_means_the_last_run_closed_properly() {
    // And the same for a first run, which is the commonest case of all: there
    // is no marker because there was no previous run.
    assert_eq!(last_run(None, None), LastRun::Clean);
    assert_eq!(
        last_run(
            None,
            Some(Path::new("/data/fontelle/crash-000000000001.log"))
        ),
        LastRun::Clean,
        "a report from an older run is not evidence about this one"
    );
}

#[test]
fn a_marker_and_a_report_is_a_panic_and_names_the_file() {
    let report = PathBuf::from("/data/fontelle/crash-000001757500.log");
    assert_eq!(
        last_run(Some(&marker().line()), Some(&report)),
        LastRun::Panicked {
            report,
            pid: 3538614
        }
    );
}

#[test]
fn a_marker_with_no_report_is_a_kill_rather_than_a_crash() {
    // The case that cost a day: `pkill` writes no message, dumps no core and
    // leaves no journal entry, so the *absence* of a report beside a marker is
    // the only evidence there is that nothing in the program went wrong.
    assert_eq!(
        last_run(Some(&marker().line()), None),
        LastRun::Killed { pid: 3538614 }
    );
}

#[test]
fn a_marker_this_build_cannot_read_is_not_a_crash() {
    // A truncated write, a marker from a future version, an empty file. None
    // of them is evidence of anything, and reporting a crash that never
    // happened is worse than saying nothing.
    for text in [
        "",
        "   ",
        "\n",
        "garbage",
        "pid=notanumber version=1 started=2",
    ] {
        assert_eq!(
            last_run(Some(text), None),
            LastRun::Clean,
            "{text:?} was read as evidence"
        );
    }
}

// ----------------------------------------------------------- the marker ---

#[test]
fn a_marker_survives_the_round_trip() {
    let m = marker();
    assert_eq!(Marker::parse(&m.line()), Some(m));
}

#[test]
fn a_project_name_with_spaces_in_it_comes_back_whole() {
    // Ty's own projects are called things like "messin w gooble".
    let m = Marker {
        project: Some("messin w gooble".to_string()),
        ..marker()
    };
    assert_eq!(
        Marker::parse(&m.line()).and_then(|back| back.project),
        Some("messin w gooble".to_string())
    );
}

#[test]
fn a_run_with_nothing_open_says_so_rather_than_writing_an_empty_name() {
    let m = Marker {
        project: None,
        ..marker()
    };
    let back = Marker::parse(&m.line()).expect("a marker with no project");
    assert_eq!(back.project, None);
    assert_eq!(back.pid, m.pid);
}

#[test]
fn a_name_with_a_newline_in_it_cannot_forge_half_a_marker() {
    // The whole file is one line by construction: the next launch reads
    // `lines().next()`, so a name carrying a newline would otherwise let the
    // first line end early and still parse.
    let m = Marker {
        project: Some("bad\nname".to_string()),
        ..marker()
    };
    let line = m.line();
    assert_eq!(line.lines().count(), 1, "the marker is one line: {line:?}");
    assert_eq!(
        Marker::parse(&line).and_then(|back| back.project),
        Some("bad name".to_string())
    );
}

#[test]
fn a_project_name_that_looks_like_a_field_does_not_overwrite_one() {
    // The parse walks the fields and reads `project=` to the end of the line,
    // so a name that contains something shaped like another key must not be
    // read as that key. The pid is what the "ended from outside" message
    // prints, and a wrong pid there points at somebody else's process.
    let m = Marker {
        project: Some("pid=1 version=x started=0".to_string()),
        ..marker()
    };
    let back = Marker::parse(&m.line()).expect("a marker");
    assert_eq!(back.pid, 3538614, "the name overwrote the pid");
    assert_eq!(back.version, "0.0.0", "the name overwrote the version");
    assert_eq!(back.started, 1_757_500_000, "the name overwrote the clock");
    assert_eq!(back.project.as_deref(), Some("pid=1 version=x started=0"));
}

// ----------------------------------------------------------- the report ---

#[test]
fn a_report_carries_what_went_wrong_and_what_was_open() {
    let text = report_text(
        "index out of bounds: the len is 3 but the index is 7",
        Some("crates/fontelle-ui/src/app.rs:1234:5"),
        "   0: fontelle_ui::app::press\n   1: winit::run_app",
        &marker(),
    );
    for wanted in [
        "index out of bounds",
        "crates/fontelle-ui/src/app.rs:1234:5",
        "SynthTesty",
        "3538614",
        "0.0.0",
        "fontelle_ui::app::press",
    ] {
        assert!(
            text.contains(wanted),
            "the report never says {wanted:?}:\n{text}"
        );
    }
}

#[test]
fn a_report_with_no_backtrace_says_how_to_get_one() {
    // The difference between "no backtrace" and "RUST_BACKTRACE was not set"
    // is the difference between a report somebody can act on and one they
    // cannot.
    let text = report_text("boom", None, "   \n  ", &marker());
    assert!(
        text.to_lowercase().contains("rust_backtrace"),
        "a report with no backtrace has to say how to get one:\n{text}"
    );
}

#[test]
fn a_panic_with_no_location_still_writes_a_report() {
    let text = report_text("boom", None, "", &marker());
    assert!(text.contains("boom"));
    assert!(
        text.contains("no location"),
        "it should say the location is missing rather than leave a blank:\n{text}"
    );
}

#[test]
fn a_report_with_nothing_open_says_that_too() {
    let text = report_text(
        "boom",
        None,
        "",
        &Marker {
            project: None,
            ..marker()
        },
    );
    assert!(
        text.contains("(none open)"),
        "an empty project line reads as a bug in the log:\n{text}"
    );
}

#[test]
fn report_names_sort_in_the_order_the_crashes_happened() {
    // Which is what makes "the newest report" a `max()` over the names rather
    // than a stat of every file in the directory.
    let mut names = [
        report_name(1_757_500_000),
        report_name(9),
        report_name(1_757_499_999),
    ];
    names.sort();
    assert_eq!(
        names,
        [
            report_name(9),
            report_name(1_757_499_999),
            report_name(1_757_500_000)
        ]
    );
    assert!(names[0].starts_with("crash-"), "{:?}", names[0]);
    assert!(names[0].ends_with(".log"), "{:?}", names[0]);
}

// ---------------------------------------------------------- what it says ---

#[test]
fn a_clean_run_has_no_news() {
    assert_eq!(LastRun::Clean.message(), None);
}

#[test]
fn a_kill_is_reported_as_not_being_a_crash_in_fontelle() {
    // *"it crashes at a certain action but if i open it up again and do that
    // same action its not guarenteed to crash again"* — the one sentence that
    // would have saved the day it cost, said by the program itself.
    let said = LastRun::Killed { pid: 4242 }
        .message()
        .expect("a kill is news");
    assert!(said.contains("4242"), "{said}");
    assert!(
        said.to_lowercase().contains("outside"),
        "it has to say the process was ended from outside: {said}"
    );
    assert!(
        said.to_lowercase().contains("not a fontelle crash"),
        "and that Fontelle did not do it, in those words: {said}"
    );
}

#[test]
fn the_news_says_what_it_is_before_the_status_line_runs_out() {
    // The window's status line is one row at the foot of a 248-pixel panel,
    // and it **clips**: about forty characters reach the eye. Measured from a
    // screenshot on 2026-09-10, where the whole of what showed was "Fontelle
    // did not close cleanly last time" — which is the question, not the
    // answer. The verdict has to be inside the part that survives.
    const CLIPPED: usize = 40;
    let head = |verdict: LastRun| -> String {
        verdict
            .message()
            .expect("news")
            .chars()
            .take(CLIPPED)
            .collect::<String>()
            .to_lowercase()
    };

    let killed = head(LastRun::Killed { pid: 4242 });
    assert!(
        killed.contains("outside") || killed.contains("not a"),
        "the visible part has to say it was not Fontelle: {killed:?}"
    );

    let panicked = head(LastRun::Panicked {
        report: PathBuf::from("/home/someone/.local/share/fontelle/crash-000001757500.log"),
        pid: 7,
    });
    assert!(
        panicked.contains("crash"),
        "and this one has to say that it *was*: {panicked:?}"
    );
}

#[test]
fn a_panic_is_reported_with_the_path_of_its_report() {
    let said = LastRun::Panicked {
        report: PathBuf::from("/data/fontelle/crash-000001757500.log"),
        pid: 7,
    }
    .message()
    .expect("a panic is news");
    assert!(
        said.contains("/data/fontelle/crash-000001757500.log"),
        "the message has to name the file to read: {said}"
    );
}
