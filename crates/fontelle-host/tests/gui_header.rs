//! The strip Fontelle draws across the top of a plugin's own editor window.
//!
//! The plugin used to be handed the top-level window itself. Now the
//! top-level is Fontelle's — a strip it draws the preset bar into, and
//! under it a child window the plugin is handed and fills. So the size the
//! plugin sees is the child's, a press on the strip is the studio's, and a
//! press on the plugin is the plugin's.
//!
//! Against the X server this runs under; skipped where there is none.

#![cfg(target_os = "linux")]

use fontelle_host::{GuiSize, PluginWindow};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{
    BUTTON_PRESS_EVENT, ButtonPressEvent, ConnectionExt as _, EventMask,
};

const HEADER: u32 = 34;

fn open() -> Option<PluginWindow> {
    if std::env::var_os("DISPLAY").is_none() {
        eprintln!("skipping: no X display");
        return None;
    }
    match PluginWindow::open_with_header(
        "Plugin",
        GuiSize {
            width: 400,
            height: 300,
        },
        HEADER,
    ) {
        Ok(window) => Some(window),
        Err(e) => {
            eprintln!("skipping: {e}");
            None
        }
    }
}

/// Polls until `done` says so, or a second has gone. `done` is handed what
/// the poll said and the window after it.
fn poll_until(
    window: &mut PluginWindow,
    mut done: impl FnMut(&fontelle_host::GuiPoll, &PluginWindow) -> bool,
) -> bool {
    let started = std::time::Instant::now();
    while started.elapsed() < std::time::Duration::from_secs(1) {
        let polled = window.poll();
        if done(&polled, window) {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    false
}

#[test]
fn the_plugin_is_handed_the_area_below_the_strip() {
    let Some(window) = open() else {
        return;
    };
    assert_eq!(window.header_height(), HEADER);
    assert_eq!(
        window.size(),
        GuiSize {
            width: 400,
            height: 300
        },
        "the size a plugin sees is its own area"
    );
    assert_ne!(window.id(), 0);
    assert_ne!(
        window.id(),
        window.frame_id(),
        "the plugin is handed a window of its own inside the frame"
    );
}

#[test]
fn a_press_on_the_strip_is_reported_and_one_on_the_plugin_is_not() {
    let Some(mut window) = open() else {
        return;
    };
    let (connection, _) = x11rb::connect(None).expect("the display the window is on");
    let press = |x: i16, y: i16| ButtonPressEvent {
        response_type: BUTTON_PRESS_EVENT,
        detail: 1,
        sequence: 0,
        time: 0,
        root: 0,
        event: window.frame_id() as u32,
        child: 0,
        root_x: x,
        root_y: y,
        event_x: x,
        event_y: y,
        state: Default::default(),
        same_screen: true,
    };
    // One on the plugin's area, then one on the strip — sent to the frame,
    // which is what the server does with a press on a window that did not
    // select it.
    for (x, y) in [(50, 200), (50, 10)] {
        connection
            .send_event(
                false,
                window.frame_id() as u32,
                EventMask::BUTTON_PRESS,
                press(x, y),
            )
            .unwrap();
    }
    connection.flush().unwrap();
    let mut presses = Vec::new();
    assert!(poll_until(&mut window, |polled, _| {
        presses.extend(polled.header_presses.iter().copied());
        !presses.is_empty()
    }));
    assert_eq!(presses, vec![(50, 10)]);
}

#[test]
fn the_strip_shows_the_pixels_it_was_given() {
    let Some(mut window) = open() else {
        return;
    };
    let mut red = Vec::with_capacity((400 * HEADER * 4) as usize);
    for _ in 0..400 * HEADER {
        red.extend_from_slice(&[220, 30, 40, 255]);
    }
    window.set_header(&red, 400, HEADER);
    let mut seen = None;
    let shown = poll_until(&mut window, |_, window| {
        seen = None;
        let Some((width, height, pixels)) = window_grab(window) else {
            return false;
        };
        let at = |x: u32, y: u32| {
            let i = ((y * width as u32 + x) * 4) as usize;
            [pixels[i], pixels[i + 1], pixels[i + 2]]
        };
        seen = Some((height, at(200, HEADER / 2), at(200, HEADER + 20)));
        at(200, HEADER / 2) == [220, 30, 40]
    });
    assert!(shown, "{seen:?}");
    let (height, _, below) = seen.unwrap();
    assert_eq!(
        u32::from(height),
        300 + HEADER,
        "the frame is the strip and the plugin"
    );
    assert_ne!(
        below,
        [220, 30, 40],
        "the strip stops where the plugin starts"
    );
}

fn window_grab(window: &PluginWindow) -> Option<(u16, u16, Vec<u8>)> {
    window.grab()
}

/// And a plugin asking for a size of its own is given it below the strip.
#[test]
fn a_plugin_that_asks_for_a_size_gets_it_below_the_strip() {
    let Some(mut window) = open() else {
        return;
    };
    window.resize(GuiSize {
        width: 500,
        height: 260,
    });
    assert_eq!(
        window.size(),
        GuiSize {
            width: 500,
            height: 260
        }
    );
    let (connection, _) = x11rb::connect(None).unwrap();
    let grown = poll_until(&mut window, |_, window| {
        connection
            .get_geometry(window.frame_id() as u32)
            .ok()
            .and_then(|cookie| cookie.reply().ok())
            .is_some_and(|geometry| {
                geometry.width == 500 && u32::from(geometry.height) == 260 + HEADER
            })
    });
    assert!(grown, "the frame grew by the plugin's size and the strip");
}

/// > *"plugin editors are not shown on this platform yet"* — said on a Mac as
/// > *"a plugin editor needs an X server (XWayland on a Wayland desktop)"*,
/// > which sends a Mac user looking for something no Mac has
/// > (`docs/plugin-experience-backlog.md` §3).
#[test]
fn a_platform_without_plugin_windows_says_so_and_where_the_controls_are() {
    let said = fontelle_host::GuiError::NotOnThisPlatform.to_string();
    assert!(!said.contains("X server"), "{said}");
    assert!(said.contains("panel"), "and where to go instead: {said}");
}

/// > Fedora and most new installs run Wayland, where a plugin's editor is
/// > an X11 window through XWayland (`docs/plugin-experience-backlog.md`
/// > §10).
///
/// A desktop scaled to 150% or 200% tells X11 programs so in the resource
/// database (`Xft.dpi`, 96 per 1×): KDE and GNOME both set it. A plugin told
/// it draws its editor that much bigger (CLAP `set_scale`, VST 3
/// `setContentScaleFactor`); one told 1× on a 2× screen draws a postage
/// stamp. The window said 1× always.
#[test]
fn an_editor_window_reads_the_desktops_scale_from_its_resources() {
    use fontelle_host::gui::scale_from_resources;
    assert_eq!(
        scale_from_resources("Xft.dpi:\t96\nXft.antialias:\t1\n"),
        1.0
    );
    assert_eq!(
        scale_from_resources("Xft.antialias:\t1\nXft.dpi:\t192\n"),
        2.0
    );
    assert_eq!(scale_from_resources("Xft.dpi: 144\n"), 1.5);
    assert_eq!(scale_from_resources(""), 1.0, "nothing said is 1×");
    assert_eq!(scale_from_resources("Xft.dpi:\tlots\n"), 1.0);
    assert_eq!(
        scale_from_resources("Xft.dpi:\t48\n"),
        1.0,
        "smaller than 1× is not a scale anybody means"
    );
    assert_eq!(scale_from_resources("Xft.dpi:\t9600\n"), 4.0, "nor past 4×");
}
