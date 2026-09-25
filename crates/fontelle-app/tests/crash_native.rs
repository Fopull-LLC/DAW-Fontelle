//! A crash that is not a panic still leaves a report.
//!
//! > *"oh now it crashed xD"*
//!
//! A panic has always written one (`crash_report_hook.rs`). A fault in native
//! code — a plugin's DLL reading freed memory, a driver — raises no panic: the
//! process simply ends, and the next launch used to report that as *"ended
//! from outside"*, which sends whoever reads it looking in the wrong place.
//! Now the fault itself writes a report, and on Windows it names the module
//! the fault was in, which is the one fact that says "the plugin" or "us".
//!
//! The crash has to be real, so this test runs **itself** as a child process
//! that installs the handler and then faults, and reads what the child left.

use std::path::PathBuf;
use std::process::Command;

use fontelle_app::crashlog;

const CHILD: &str = "FONTELLE_CRASH_NATIVE_CHILD";

#[test]
fn a_fault_in_native_code_writes_a_report_and_the_next_launch_calls_it_a_crash() {
    if let Some(dir) = std::env::var_os(CHILD) {
        // The child: begin a run, then do what a broken plugin does.
        crashlog::begin(&PathBuf::from(dir), Some("Faulty Project"));
        // SAFETY: none — this is the crash under test. A volatile read so the
        // compiler cannot prove it away.
        unsafe {
            let nowhere = std::hint::black_box(std::ptr::null::<u64>());
            std::ptr::read_volatile(nowhere);
        }
        unreachable!("the read above does not return");
    }

    let dir = std::env::temp_dir().join("fontelle-crash-native");
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).expect("a scratch directory");

    let status = Command::new(std::env::current_exe().expect("this test binary"))
        .args([
            "--exact",
            "a_fault_in_native_code_writes_a_report_and_the_next_launch_calls_it_a_crash",
            "--nocapture",
        ])
        .env(CHILD, &dir)
        .status()
        .expect("the child runs");
    assert!(!status.success(), "the child should have died of the fault");

    let reports: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("the directory")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("crash-"))
        })
        .collect();
    assert_eq!(reports.len(), 1, "one fault, one report: {reports:?}");
    let text = std::fs::read_to_string(&reports[0]).expect("the report");
    assert!(
        text.contains("Faulty Project"),
        "the report says what was open:\n{text}"
    );
    let named = if cfg!(windows) {
        "access violation"
    } else {
        "SIGSEGV"
    };
    assert!(text.contains(named), "and what the fault was:\n{text}");
    if cfg!(windows) {
        // Which binary the faulting address is inside — here, the test
        // itself; in the field, a plugin's DLL or Fontelle.
        assert!(
            text.contains("crash_native"),
            "and which module it was in:\n{text}"
        );
    }

    // The next launch reads it as a crash, not as a kill from outside.
    let verdict = crashlog::begin(&dir, None);
    assert!(
        matches!(verdict, crashlog::LastRun::Panicked { .. }),
        "a fault is a crash: {verdict:?}"
    );
    crashlog::end(&dir);
}
