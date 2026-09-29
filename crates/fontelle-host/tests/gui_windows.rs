//! A plugin's own editor on Windows.
//!
//! > *"this is what happens to outside vsts in ur daw — it works but it's
//! > like only the knobs of like every parameter"*
//!
//! Until this, `PluginWindow::open` answered *"plugin editors are shown on
//! Linux only in this build"* everywhere else, so every VST 3 and CLAP plugin
//! on Windows fell back to Fontelle's generated grid of knobs. On Windows the
//! window a plugin embeds into is a plain Win32 one: the host makes a
//! top-level window and hands its `HWND` over, and the plugin makes its child
//! window inside it. The studio's own event loop already dispatches every
//! message on its thread, so the plugin's window is driven for free.

#![cfg(windows)]

use fontelle_host::{GuiSize, PluginWindow};

#[test]
fn on_windows_a_plugin_window_is_a_real_window_that_reports_what_the_desktop_does() {
    let mut window = PluginWindow::open(
        "Synth",
        GuiSize {
            width: 640,
            height: 480,
        },
    )
    .expect("a window on Windows");
    assert!(window.is_on_screen());
    assert_ne!(window.id(), 0, "an HWND to hand the plugin");
    assert_eq!(
        window.size(),
        GuiSize {
            width: 640,
            height: 480
        },
        "the size is the client area the plugin draws in"
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

    // The close button closes the editor, not the studio: the window is
    // told, and says so, and is still there until the host lets it go.
    close_button(&window);
    fontelle_host::pump_gui_messages();
    assert!(window.poll().closed, "the close button is reported");
}

/// What pressing the title bar's × sends.
fn close_button(window: &PluginWindow) {
    unsafe extern "system" {
        fn PostMessageW(hwnd: *mut std::ffi::c_void, msg: u32, wparam: usize, lparam: isize)
        -> i32;
    }
    const WM_CLOSE: u32 = 0x0010;
    // SAFETY: a posted message to a window this thread owns.
    unsafe { PostMessageW(window.id() as usize as *mut _, WM_CLOSE, 0, 0) };
}

/// The strip across the top, on Windows: the plugin is handed a child window
/// under it, the sizes are the plugin's, and a press on the strip is the
/// studio's while one below it is not — the X11 test's claims
/// (`gui_header.rs`), made of the Win32 window.
#[test]
fn on_windows_the_strip_is_the_studios_and_the_rest_is_the_plugins() {
    const HEADER: u32 = 32;
    let mut window = PluginWindow::open_with_header(
        "Synth",
        GuiSize {
            width: 640,
            height: 480,
        },
        HEADER,
    )
    .expect("a window on Windows");
    assert_eq!(window.header_height(), HEADER);
    assert_eq!(
        window.size(),
        GuiSize {
            width: 640,
            height: 480
        }
    );
    assert_ne!(window.id(), 0);
    assert_ne!(
        window.id(),
        window.frame_id(),
        "the plugin has a window of its own"
    );

    let red: Vec<u8> = (0..640 * HEADER).flat_map(|_| [220, 30, 40, 255]).collect();
    window.set_header(&red, 640, HEADER);

    press(window.frame_id(), 50, 200);
    press(window.frame_id(), 50, 10);
    fontelle_host::pump_gui_messages();
    assert_eq!(window.poll().header_presses, vec![(50, 10)]);

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
        },
        "the plugin's area, under the strip"
    );
}

/// A left press at `(x, y)` of a window's client area.
fn press(hwnd: u64, x: i32, y: i32) {
    unsafe extern "system" {
        fn PostMessageW(hwnd: *mut std::ffi::c_void, msg: u32, wparam: usize, lparam: isize)
        -> i32;
    }
    const WM_LBUTTONDOWN: u32 = 0x0201;
    let lparam = ((y as isize & 0xFFFF) << 16) | (x as isize & 0xFFFF);
    // SAFETY: a posted message to a window this thread owns.
    unsafe { PostMessageW(hwnd as usize as *mut _, WM_LBUTTONDOWN, 0x0001, lparam) };
}
