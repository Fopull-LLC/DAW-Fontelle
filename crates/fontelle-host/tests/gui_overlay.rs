//! A menu Fontelle draws over a plugin's own window: the strip's preset
//! drop-down.
//!
//! > *"when clicking the preset dropdown on the top of the plugin window it
//! > didnt drop down any of those presets for me to select there or hit
//! > random preset to get a random one, it just opened the preset tab on the
//! > left."*
//!
//! The strip is a few dozen pixels tall and a list of presets is not, so the
//! list is a second surface of the studio's inside the frame — over the
//! plugin's area, under the strip's name field — which takes the pointer and
//! the keyboard while it is up: a press anywhere is the menu's (one off it
//! dismisses it), and typing searches it.
//!
//! The headless half runs everywhere; the X11 half against the X server this
//! runs under, skipped where there is none.

use fontelle_host::{OverlayEvent, OverlayKey, PluginWindow};

#[test]
fn a_window_on_no_screen_keeps_where_its_menu_is_and_reports_what_it_is_told() {
    let mut window = PluginWindow::headless_with_header(400, 300, 32);
    assert_eq!(window.overlay(), None);
    let pixels = vec![200u8; 120 * 90 * 4];
    window.show_overlay(40, 32, &pixels, 120, 90);
    assert_eq!(window.overlay(), Some((40, 32, 120, 90)));

    window.overlay_event(OverlayEvent::Press(50, 60));
    window.overlay_event(OverlayEvent::Key(OverlayKey::Text("ob".into())));
    let polled = window.poll();
    assert_eq!(
        polled.overlay,
        vec![
            OverlayEvent::Press(50, 60),
            OverlayEvent::Key(OverlayKey::Text("ob".into()))
        ]
    );
    assert!(window.poll().overlay.is_empty(), "taken once");

    window.hide_overlay();
    assert_eq!(window.overlay(), None);
    window.overlay_event(OverlayEvent::Press(1, 1));
    assert!(
        window.poll().overlay.is_empty(),
        "a window with no menu up has nothing to say about one"
    );
}

#[test]
fn a_menu_is_kept_inside_the_frame() {
    // 400 wide, the strip and 300 under it: 332 tall in all.
    let mut window = PluginWindow::headless_with_header(400, 300, 32);
    let pixels = vec![0u8; 300 * 400 * 4];
    window.show_overlay(250, 100, &pixels, 300, 400);
    let (x, y, width, height) = window.overlay().unwrap();
    assert!(x >= 0 && y >= 0);
    assert!(x as u32 + width <= 400, "{x} + {width}");
    assert!(y as u32 + height <= 332, "{y} + {height}");
}

#[cfg(target_os = "linux")]
mod on_x11 {
    use super::*;
    use fontelle_host::GuiSize;
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{
        BUTTON_PRESS_EVENT, ButtonPressEvent, ConnectionExt as _, EventMask, KEY_PRESS_EVENT,
        KeyButMask, KeyPressEvent,
    };
    use x11rb::rust_connection::RustConnection;

    const HEADER: u32 = 32;

    fn open() -> Option<(PluginWindow, RustConnection)> {
        if std::env::var_os("DISPLAY").is_none() {
            eprintln!("skipping: no X display");
            return None;
        }
        let window = match PluginWindow::open_with_header(
            "Plugin",
            GuiSize {
                width: 400,
                height: 300,
            },
            HEADER,
        ) {
            Ok(window) => window,
            Err(e) => {
                eprintln!("skipping: {e}");
                return None;
            }
        };
        let (connection, _) = x11rb::connect(None).expect("the display the window is on");
        Some((window, connection))
    }

    fn poll_until(
        window: &mut PluginWindow,
        mut done: impl FnMut(&fontelle_host::GuiPoll, &PluginWindow) -> bool,
    ) -> bool {
        let started = std::time::Instant::now();
        while started.elapsed() < std::time::Duration::from_secs(2) {
            let polled = window.poll();
            if done(&polled, window) {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        false
    }

    fn pixel(window: &PluginWindow, x: u32, y: u32) -> Option<[u8; 3]> {
        let (width, _, pixels) = window.grab()?;
        let i = ((y * u32::from(width) + x) * 4) as usize;
        Some([pixels[i], pixels[i + 1], pixels[i + 2]])
    }

    /// The menu's own window: the frame's topmost child.
    fn menu_window(connection: &RustConnection, window: &PluginWindow) -> u32 {
        let tree = connection
            .query_tree(window.frame_id() as u32)
            .unwrap()
            .reply()
            .unwrap();
        *tree.children.last().expect("the frame has children")
    }

    fn keycode(connection: &RustConnection, keysym: u32) -> u8 {
        let setup = connection.setup();
        let (min, max) = (setup.min_keycode, setup.max_keycode);
        let map = connection
            .get_keyboard_mapping(min, max - min + 1)
            .unwrap()
            .reply()
            .unwrap();
        let per = map.keysyms_per_keycode as usize;
        let at = map
            .keysyms
            .chunks(per)
            .position(|syms| syms.first() == Some(&keysym))
            .expect("the keyboard has the key");
        min + at as u8
    }

    #[test]
    fn the_menu_is_drawn_over_the_plugins_area_and_gone_when_hidden() {
        let Some((mut window, _connection)) = open() else {
            return;
        };
        let mut red = Vec::new();
        for _ in 0..200 * 150 {
            red.extend_from_slice(&[220, 30, 40, 255]);
        }
        window.show_overlay(10, HEADER as i32, &red, 200, 150);
        let mut seen = None;
        let shown = poll_until(&mut window, |_, window| {
            seen = pixel(window, 60, HEADER + 60);
            seen == Some([220, 30, 40])
        });
        assert!(shown, "the menu over the plugin's area: {seen:?}");
        assert_ne!(
            pixel(&window, 300, HEADER + 200),
            Some([220, 30, 40]),
            "only where the menu is"
        );
        window.hide_overlay();
        let gone = poll_until(&mut window, |_, window| {
            pixel(window, 60, HEADER + 60) != Some([220, 30, 40])
        });
        assert!(gone, "hidden, the plugin shows again");
    }

    #[test]
    fn a_press_and_typing_on_the_menu_come_back_in_the_frames_pixels() {
        let Some((mut window, connection)) = open() else {
            return;
        };
        let pixels = vec![90u8; 200 * 150 * 4];
        window.show_overlay(10, HEADER as i32, &pixels, 200, 150);
        let _ = window.poll();
        let menu = menu_window(&connection, &window);
        assert_ne!(
            menu,
            window.id() as u32,
            "the menu is not the plugin's window"
        );
        let press = ButtonPressEvent {
            response_type: BUTTON_PRESS_EVENT,
            detail: 1,
            sequence: 0,
            time: 0,
            root: 0,
            event: menu,
            child: 0,
            root_x: 0,
            root_y: 0,
            event_x: 20,
            event_y: 30,
            state: Default::default(),
            same_screen: true,
        };
        connection
            .send_event(false, menu, EventMask::BUTTON_PRESS, press)
            .unwrap();
        let key = |code: u8, state: KeyButMask| KeyPressEvent {
            response_type: KEY_PRESS_EVENT,
            detail: code,
            sequence: 0,
            time: 0,
            root: 0,
            event: menu,
            child: 0,
            root_x: 0,
            root_y: 0,
            event_x: 1,
            event_y: 1,
            state,
            same_screen: true,
        };
        for (keysym, state) in [
            (0x6f, KeyButMask::default()), // o
            (0x62, KeyButMask::SHIFT),     // B
            (0x20, KeyButMask::default()), // space: a letter here, not play
            (0xff08, KeyButMask::default()),
            (0xff1b, KeyButMask::default()),
        ] {
            connection
                .send_event(
                    false,
                    menu,
                    EventMask::KEY_PRESS,
                    key(keycode(&connection, keysym), state),
                )
                .unwrap();
        }
        connection.flush().unwrap();
        let mut events = Vec::new();
        let mut spaces = 0;
        poll_until(&mut window, |polled, _| {
            events.extend(polled.overlay.iter().cloned());
            spaces += polled.play_pause;
            events.len() >= 6
        });
        assert_eq!(
            events,
            vec![
                OverlayEvent::Press(30, HEADER as i32 + 30),
                OverlayEvent::Key(OverlayKey::Text("o".into())),
                OverlayEvent::Key(OverlayKey::Text("B".into())),
                OverlayEvent::Key(OverlayKey::Text(" ".into())),
                OverlayEvent::Key(OverlayKey::Backspace),
                OverlayEvent::Key(OverlayKey::Escape),
            ]
        );
        assert_eq!(spaces, 0, "a space typed into the search is not play");
    }
}
