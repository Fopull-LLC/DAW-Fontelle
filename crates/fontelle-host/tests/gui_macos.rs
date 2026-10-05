//! A plugin's own editor on macOS.
//!
//! > `docs/plugin-experience-backlog.md` §3: plugin editors did not open on a
//! > Mac at all — every preset browser, wavetable editor and patch built
//! > inside a plugin was out of reach — and the message said to go and find
//! > an X server.
//!
//! The CLAP `cocoa` API and VST 3's `"NSView"` both hand the plugin an
//! `NSView`; `PluginWindow` on macOS is an `NSWindow` holding that view and,
//! above it, the studio's strip. The claims are the Windows ones
//! (`gui_windows.rs`) made of an AppKit window, and an editor of each format
//! embedded in it.
//!
//! **Its own `main`**: AppKit is main-thread only, and libtest runs each test
//! on a thread of its own (`Cargo.toml`, `harness = false`).

#[cfg(target_os = "macos")]
mod common;

#[cfg(not(target_os = "macos"))]
fn main() {}

#[cfg(target_os = "macos")]
fn main() {
    let tests: [(&str, fn()); 4] = [
        (
            "a_plugin_window_is_a_real_window_that_reports_what_the_desktop_does",
            a_plugin_window_is_a_real_window_that_reports_what_the_desktop_does,
        ),
        (
            "the_strip_is_the_studios_and_the_rest_is_the_plugins",
            the_strip_is_the_studios_and_the_rest_is_the_plugins,
        ),
        (
            "a_clap_editor_is_embedded_in_the_window",
            a_clap_editor_is_embedded_in_the_window,
        ),
        (
            "a_vst3_editor_is_attached_to_the_window",
            a_vst3_editor_is_attached_to_the_window,
        ),
    ];
    let mut failed = Vec::new();
    for (name, test) in tests {
        print!("test {name} ... ");
        match std::panic::catch_unwind(test) {
            Ok(()) => println!("ok"),
            Err(_) => {
                println!("FAILED");
                failed.push(name);
            }
        }
    }
    if failed.is_empty() {
        println!("\ntest result: ok. {} passed", tests.len());
    } else {
        println!("\ntest result: FAILED. {failed:?}");
        std::process::exit(101);
    }
}

#[cfg(target_os = "macos")]
use fontelle_host::{GuiSize, PluginHost, PluginWindow};

#[cfg(target_os = "macos")]
fn a_plugin_window_is_a_real_window_that_reports_what_the_desktop_does() {
    let mut window = PluginWindow::open(
        "Synth",
        GuiSize {
            width: 640,
            height: 480,
        },
    )
    .expect("a window on macOS");
    assert!(window.is_on_screen());
    assert_ne!(window.id(), 0, "an NSView to hand the plugin");
    assert_eq!(
        window.size(),
        GuiSize {
            width: 640,
            height: 480
        },
        "the size is the area the plugin draws in"
    );
    assert!(window.scale() >= 1.0);

    // A size the host set is not news the next time it asks.
    window.resize(GuiSize {
        width: 800,
        height: 500,
    });
    fontelle_host::pump_gui_messages();
    assert_eq!(
        window.size(),
        GuiSize {
            width: 800,
            height: 500
        }
    );
    assert_eq!(window.poll().resized, None);
    assert!(!window.poll().closed);
}

#[cfg(target_os = "macos")]
fn the_strip_is_the_studios_and_the_rest_is_the_plugins() {
    const HEADER: u32 = 32;
    let mut window = PluginWindow::open_with_header(
        "Synth",
        GuiSize {
            width: 640,
            height: 480,
        },
        HEADER,
    )
    .expect("a window on macOS");
    assert_eq!(window.header_height(), HEADER);
    assert_eq!(
        window.size(),
        GuiSize {
            width: 640,
            height: 480
        },
        "the plugin's area, under the strip"
    );
    assert_ne!(window.id(), 0);
    assert_ne!(
        window.id(),
        window.frame_id(),
        "the plugin has a view of its own"
    );
    let red: Vec<u8> = (0..640 * HEADER).flat_map(|_| [220, 30, 40, 255]).collect();
    window.set_header(&red, 640, HEADER);
    fontelle_host::pump_gui_messages();

    window.resize(GuiSize {
        width: 700,
        height: 400,
    });
    fontelle_host::pump_gui_messages();
    assert_eq!(
        window.size(),
        GuiSize {
            width: 700,
            height: 400
        }
    );
}

#[cfg(target_os = "macos")]
fn a_clap_editor_is_embedded_in_the_window() {
    let mut host = PluginHost::new();
    let mut plugin = host
        .open(
            &common::bundle(),
            &fontelle_types::PluginKey::clap("com.fopull.fontelle.testface"),
        )
        .expect("the test face opens");
    assert!(plugin.has_editor(), "a cocoa editor, on macOS");
    let window = PluginWindow::open(
        "Face",
        GuiSize {
            width: 300,
            height: 200,
        },
    )
    .expect("a window");
    let wanted = plugin
        .open_editor(&window, window.scale())
        .expect("the editor goes into the window");
    assert_eq!(
        (wanted.width, wanted.height),
        (
            fontelle_testplug::FACE_WIDTH,
            fontelle_testplug::FACE_HEIGHT
        )
    );
    fontelle_host::pump_gui_messages();
    plugin.close_editor();
}

#[cfg(target_os = "macos")]
fn a_vst3_editor_is_attached_to_the_window() {
    let mut host = PluginHost::new();
    let mut plugin = host
        .open(
            &common::vst3_bundle(),
            &fontelle_types::PluginKey::new(
                fontelle_types::PluginFormat::Vst3,
                common::VST3_COMBINED,
            ),
        )
        .expect("the test VST 3 opens");
    assert!(plugin.has_editor(), "an NSView editor, on macOS");
    let window = PluginWindow::open(
        "Combined",
        GuiSize {
            width: 300,
            height: 200,
        },
    )
    .expect("a window");
    plugin
        .open_editor(&window, window.scale())
        .expect("the view is attached");
    fontelle_host::pump_gui_messages();
    plugin.close_editor();
}
