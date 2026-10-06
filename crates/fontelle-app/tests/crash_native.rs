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

/// > *"sometimes they'll just revert back to the init preset"* — and the
/// > sweep behind it found plugins that crash in their own code.
///
/// padthv1 calls a pure virtual function and `libstdc++` aborts: the fault
/// is in `libc`, and the module line names `libc`. The host marks every call
/// it makes into a plugin on the thread that makes it, and the report names
/// the plugin from that mark — the one fact the next launch needs to open
/// the project without it.
#[cfg(unix)]
#[test]
fn a_crash_inside_a_plugin_call_names_the_plugin_whatever_module_it_was_in() {
    const NAMED_CHILD: &str = "FONTELLE_CRASH_NAMED_CHILD";
    let key = fontelle_types::PluginKey::clap("com.fopull.fontelle.testgain");
    if let Some(dir) = std::env::var_os(NAMED_CHILD) {
        crashlog::begin(&PathBuf::from(dir), Some("Faulty Project"));
        let label = fontelle_host::guard::Label::new("Fontelle Test Gain", &key);
        let _inside = fontelle_host::guard::calling(label);
        std::process::abort();
    }

    let dir = std::env::temp_dir().join(format!("fontelle-crash-named-{}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    let status = Command::new(std::env::current_exe().expect("this test binary"))
        .args([
            "--exact",
            "a_crash_inside_a_plugin_call_names_the_plugin_whatever_module_it_was_in",
            "--nocapture",
        ])
        .env(NAMED_CHILD, &dir)
        .status()
        .expect("the child runs");
    assert!(!status.success(), "the child should have aborted");

    let text = newest_report(&dir);
    assert!(
        text.contains("plugin:    Fontelle Test Gain\tclap:com.fopull.fontelle.testgain"),
        "the report names the plugin:\n{text}"
    );
    assert_eq!(crashlog::culprit(&text), Some(key), "and says it back");
    std::fs::remove_dir_all(&dir).ok();
}

/// A plugin's own thread that aborts just after its editor opened is put down
/// to that plugin.
///
/// Reported: Vital's editor aborted the studio from Vital's own "Render
/// Thread" (bgfx: *"Failed to create surface"*). The CLAP build's report
/// named Vital; the VST 3 build's named nobody, because by the time the
/// renderer gave up the main thread had come back out of the editor's
/// calls — and a report naming nobody holds nothing back next time.
#[cfg(unix)]
#[test]
fn a_plugin_thread_that_aborts_as_its_editor_opens_names_the_plugin() {
    const EDITOR_CHILD: &str = "FONTELLE_CRASH_EDITOR_CHILD";
    let key = fontelle_types::PluginKey::new(
        fontelle_types::PluginFormat::Vst3,
        "56535449-6e76-6974-616c-000000000000",
    );
    if let Some(dir) = std::env::var_os(EDITOR_CHILD) {
        crashlog::begin(&PathBuf::from(dir), Some("Faulty Project"));
        let label = fontelle_host::guard::Label::new("Vital", &key);
        fontelle_host::guard::editor_opened(label);
        // The plugin's renderer: a thread the host never marks.
        std::thread::spawn(|| std::process::abort())
            .join()
            .expect("it aborts");
        unreachable!("the renderer aborts the process");
    }

    let dir = std::env::temp_dir().join(format!("fontelle-crash-editor-{}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    let status = Command::new(std::env::current_exe().expect("this test binary"))
        .args([
            "--exact",
            "a_plugin_thread_that_aborts_as_its_editor_opens_names_the_plugin",
            "--nocapture",
        ])
        .env(EDITOR_CHILD, &dir)
        .status()
        .expect("the child runs");
    assert!(!status.success(), "the child should have aborted");

    let text = newest_report(&dir);
    assert_eq!(
        crashlog::culprit(&text),
        Some(key),
        "the report names the plugin:\n{text}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// A report with no plugin in it names none — Fontelle's own crash is not a
/// reason to hold a plugin back.
#[test]
fn a_report_that_names_no_plugin_has_no_culprit() {
    let marker = crashlog::Marker::here(Some("Song"));
    let text = crashlog::native_report_text("SIGSEGV", Some("/usr/bin/fontelle"), &marker);
    assert_eq!(crashlog::culprit(&text), None);
}

/// And the host makes the mark: a real plugin aborting in its own `process`,
/// called the way the audio thread calls it, is named in the report.
#[cfg(target_os = "linux")]
#[test]
fn a_plugin_that_aborts_in_its_own_process_is_named_in_the_report() {
    const PROCESS_CHILD: &str = "FONTELLE_CRASH_PROCESS_CHILD";
    if let Some(dir) = std::env::var_os(PROCESS_CHILD) {
        crashlog::begin(&PathBuf::from(dir), None);
        let mut host = fontelle_host::PluginHost::new();
        let mut plugin = host
            .open(
                &test_plugin(),
                &fontelle_types::PluginKey::clap("com.fopull.fontelle.testgain"),
            )
            .expect("the test gain opens");
        let mut processor = plugin.activate(48_000.0, 256).expect("it activates");
        let mut bus = vec![vec![0.0f32; 256]; 2];
        processor.process_insert(&mut bus, 256);
        unreachable!("the plugin aborts in its process");
    }

    let dir = std::env::temp_dir().join(format!("fontelle-crash-process-{}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    let status = Command::new(std::env::current_exe().expect("this test binary"))
        .args([
            "--exact",
            "a_plugin_that_aborts_in_its_own_process_is_named_in_the_report",
            "--nocapture",
        ])
        .env(PROCESS_CHILD, &dir)
        .env(fontelle_testplug::ABORTS_IN_PROCESS_ENV, "1")
        .status()
        .expect("the child runs");
    assert!(!status.success(), "the child should have aborted");
    let text = newest_report(&dir);
    assert_eq!(
        crashlog::culprit(&text),
        Some(fontelle_types::PluginKey::clap(
            "com.fopull.fontelle.testgain"
        )),
        "the report names the plugin:\n{text}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[cfg(unix)]
fn newest_report(dir: &std::path::Path) -> String {
    let report = std::fs::read_dir(dir)
        .expect("the directory")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("crash-"))
        })
        .max()
        .expect("a report");
    std::fs::read_to_string(report).expect("readable")
}

/// The test CLAP bundle, built beside this binary, under a `.clap` name.
#[cfg(target_os = "linux")]
fn test_plugin() -> PathBuf {
    let mut path = std::env::current_exe().unwrap();
    path.pop();
    path.pop();
    let built = path.join(if cfg!(target_os = "macos") {
        "libfontelle_testplug.dylib"
    } else {
        "libfontelle_testplug.so"
    });
    let folder = std::env::temp_dir().join(format!("fontelle-crash-plugin-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&folder);
    let bundle = folder.join("fontelle-testplug.clap");
    std::fs::copy(&built, &bundle).expect("the test plugin copies");
    bundle
}
