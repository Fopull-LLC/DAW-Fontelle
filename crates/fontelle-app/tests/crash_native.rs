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
    if cfg!(any(windows, target_os = "linux")) {
        // Which binary the faulting address is inside — here, the test
        // itself; in the field, a plugin's DLL or Fontelle.
        //
        // On Linux too, now: four reports from a Fedora user, all SIGSEGV
        // and all "module: (this platform does not say)", were four
        // reports that could not say whether the synth he was trying had
        // crashed or we had.
        assert!(
            text.contains("crash_native") && !text.contains("does not say"),
            "and which module it was in:\n{text}"
        );
    }
    if cfg!(target_os = "linux") {
        // Which thread: the audio thread, the window's, or a plugin's own.
        // The kernel keeps fifteen bytes of a thread's name.
        assert!(
            text.contains("thread:    a_fault_in_nat"),
            "and which thread:\n{text}"
        );
        // Where: the instruction, and the memory it reached for — here, a
        // read of address zero.
        assert!(text.contains("code at:   0x"), "and where:\n{text}");
        assert!(
            text.contains("fault at:  0x0000000000000000"),
            "and what it touched:\n{text}"
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

/// The lookup the handler makes, on a copy of `/proc/self/maps`: which file
/// an address is mapped from. Pure, so its edges are tested here rather than
/// by crashing.
#[cfg(target_os = "linux")]
#[test]
fn an_address_is_found_in_the_file_it_is_mapped_from() {
    let maps = b"\
55d0a0000000-55d0a0100000 r-xp 00000000 08:01 1234 /usr/bin/fontelle
55d0a0100000-55d0a0200000 rw-p 00000000 00:00 0 [heap]
7f0010000000-7f0010800000 r-xp 00010000 08:01 99 /home/k/.vst3/Vital.vst3/Contents/x86_64-linux/Vital.so
7f0010800000-7f0010900000 rw-p 00000000 00:00 0
7f0020000000-7f0020001000 r-xp 00000000 08:01 7 /usr/lib/a path with spaces.so
";
    let at = |pc: usize| crashlog::module_in_maps(maps, pc);
    assert_eq!(
        at(0x7f0010000040).as_deref(),
        Some("/home/k/.vst3/Vital.vst3/Contents/x86_64-linux/Vital.so")
    );
    assert_eq!(
        at(0x55d0a0000000).as_deref(),
        Some("/usr/bin/fontelle"),
        "start is inside"
    );
    assert_eq!(at(0x55d0a0100000).as_deref(), Some("[heap]"), "end is not");
    assert_eq!(at(0x7f0010800010), None, "anonymous memory has no file");
    assert_eq!(at(0x1000), None, "nor does nothing at all");
    assert_eq!(
        at(0x7f0020000000).as_deref(),
        Some("/usr/lib/a path with spaces.so")
    );
    // Fed in pieces, as the handler reads it — a line split across two reads
    // is still one line.
    let mut scan = crashlog::MapsScan::new(0x7f0010000040);
    for chunk in maps.chunks(7) {
        scan.feed(chunk);
    }
    assert_eq!(
        std::str::from_utf8(scan.found().unwrap()).unwrap(),
        "/home/k/.vst3/Vital.vst3/Contents/x86_64-linux/Vital.so"
    );
}
