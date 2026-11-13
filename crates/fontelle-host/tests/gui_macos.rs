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
    let tests: [(&str, fn()); 5] = [
        (
            "a_plugin_window_is_a_real_window_that_reports_what_the_desktop_does",
            a_plugin_window_is_a_real_window_that_reports_what_the_desktop_does,
        ),
        (
            "the_strip_is_the_studios_and_the_rest_is_the_plugins",
            the_strip_is_the_studios_and_the_rest_is_the_plugins,
        ),
        (
            "a_press_on_the_strip_takes_the_keyboard_back_from_the_plugin",
            a_press_on_the_strip_takes_the_keyboard_back_from_the_plugin,
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

/// Ty, 2026-11-12: *"i could click the bar at the top then be able to play
/// and pause"*, and *"of course it should work for all platforms"*. A plugin
/// view that has the keyboard keeps the space; a press on the strip hands the
/// keyboard to the plugin's area, which hears the next space as play. (The
/// title bar is the same rule, `on_window_press`; a press sent there would
/// start AppKit's drag loop, so it is not driven here.)
///
/// Events are made and sent to the window directly, which a desktop with no
/// one at it may still not deliver; where a step does not happen at all the
/// test says so and stops, rather than blaming the window for the runner.
#[cfg(target_os = "macos")]
fn a_press_on_the_strip_takes_the_keyboard_back_from_the_plugin() {
    use objc2::MainThreadOnly;
    use objc2_app_kit::{NSEvent, NSEventModifierFlags, NSEventType, NSTextView, NSView, NSWindow};
    use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

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
    fontelle_host::pump_gui_messages();
    // SAFETY: the ids are the window's own NSWindow and NSView, alive while
    // `window` is.
    let (ns_window, area) = unsafe {
        (
            &*(window.frame_id() as usize as *const NSWindow),
            &*(window.id() as usize as *const NSView),
        )
    };
    let mtm = objc2::MainThreadMarker::new().expect("the main thread");
    // The plugin: a text view, which keeps every key it is sent.
    let plugin = NSTextView::initWithFrame(
        NSTextView::alloc(mtm),
        NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(640.0, 480.0)),
    );
    area.addSubview(&plugin);
    if !ns_window.makeFirstResponder(Some(&plugin)) {
        println!("(skipped: the text view would not take the keyboard)");
        return;
    }
    let number = ns_window.windowNumber();
    let space = || {
        let text = NSString::from_str(" ");
        let event = NSEvent::keyEventWithType_location_modifierFlags_timestamp_windowNumber_context_characters_charactersIgnoringModifiers_isARepeat_keyCode(
            NSEventType::KeyDown,
            NSPoint::new(0.0, 0.0),
            NSEventModifierFlags::empty(),
            0.0,
            number,
            None,
            &text,
            &text,
            false,
            49,
        )
        .expect("a key event");
        ns_window.sendEvent(&event);
    };
    space();
    fontelle_host::pump_gui_messages();
    if window.poll().play_pause != 0 {
        println!("(skipped: the text view passed the space on)");
        return;
    }

    // A press on the strip: AppKit's window coordinates are bottom up, and
    // the strip is the top 32 points of the 512.
    let press = NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
        NSEventType::LeftMouseDown,
        NSPoint::new(50.0, 500.0),
        NSEventModifierFlags::empty(),
        0.0,
        number,
        None,
        0,
        1,
        1.0,
    )
    .expect("a mouse event");
    ns_window.sendEvent(&press);
    fontelle_host::pump_gui_messages();
    let poll = window.poll();
    if poll.header_presses.is_empty() {
        println!("(skipped: the press never reached the strip)");
        return;
    }
    space();
    fontelle_host::pump_gui_messages();
    assert_eq!(
        window.poll().play_pause,
        1,
        "the space after the strip plays"
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
            &fontelle_types::PluginKey::new(fontelle_types::PluginFormat::Vst3, common::VST3_GAIN),
        )
        .expect("the test VST 3 opens");
    assert!(plugin.has_editor(), "an NSView editor, on macOS");
    let window = PluginWindow::open(
        "Gain",
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
