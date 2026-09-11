//! The panic hook itself: a panic anywhere in the process leaves a report on
//! disk, whether or not anybody is reading stderr.
//!
//! Its own test binary, because [`fontelle_app::crashlog::begin`] installs a
//! **process-wide** hook and this file deliberately panics under it. Sharing a
//! binary with the other crash-log tests would have their panics (and any
//! assertion failure anywhere) writing reports too.

use fontelle_app::crashlog;

#[test]
fn a_panic_writes_a_report_that_says_what_and_where() {
    let dir = std::env::temp_dir().join("fontelle-crashlog-hook");
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).expect("a scratch directory");

    crashlog::begin(&dir, Some("SynthTesty"));

    // Caught, so the test survives to read what the hook wrote. The hook runs
    // before the unwind either way — that is the whole point of a hook rather
    // than a wrapper around `main`.
    let result = std::panic::catch_unwind(|| {
        // A real one, of the shape this is built to catch: an index that was
        // valid a frame ago. Through a `Vec` and a value the compiler cannot
        // see, or this is a compile error rather than a panic.
        let rows: Vec<u8> = vec![1, 2, 3];
        let stale = "7".parse::<usize>().expect("a number");
        let _ = rows[stale];
    });
    assert!(result.is_err(), "the panic should have happened");

    let mut reports: Vec<_> = std::fs::read_dir(&dir)
        .expect("the directory")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("crash-"))
        })
        .collect();
    reports.sort();
    assert_eq!(reports.len(), 1, "one panic, one report: {reports:?}");

    let text = std::fs::read_to_string(&reports[0]).expect("the report");
    assert!(
        text.contains("index out of bounds"),
        "the report has to carry the panic's own words:\n{text}"
    );
    assert!(
        text.contains("crash_report_hook.rs"),
        "and where it happened:\n{text}"
    );
    assert!(
        text.contains("SynthTesty"),
        "and what was open at the time:\n{text}"
    );

    // And the next launch finds it and says so, naming that file.
    let verdict = crashlog::begin(&dir, None);
    let crashlog::LastRun::Panicked { report, .. } = verdict else {
        panic!("got {verdict:?}");
    };
    assert_eq!(report, reports[0]);
}
