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
