//! The space bar in a plugin's window plays and stops the song.
//!
//! Reported: space only worked while the studio's own window had the
//! keyboard; with a plugin's editor in front it did nothing. The window now
//! hears a space the plugin did not take — pressed on the strip, or passed
//! up from a plugin window that does not listen for keys — and `poll` says
//! so ([`GuiPoll::play_pause`]). A plugin that listens for keys keeps them:
//! a space typed into its preset name stays a space. And it is no hotkey:
//! nothing reaches a window the keyboard is not in.
//!
//! Against the X server this runs under; skipped where there is none.

#![cfg(target_os = "linux")]

use fontelle_host::{GuiPoll, GuiSize, PluginWindow};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{
    ConnectionExt as _, CreateWindowAux, EventMask, KEY_PRESS_EVENT, KEY_RELEASE_EVENT, KeyButMask,
    KeyPressEvent, WindowClass,
};
use x11rb::rust_connection::RustConnection;

fn open() -> Option<(PluginWindow, RustConnection)> {
    if std::env::var_os("DISPLAY").is_none() {
        eprintln!("skipping: no X display");
        return None;
    }
    let window = match PluginWindow::open_with_header(
        "Plugin",
        GuiSize {
            width: 300,
            height: 200,
        },
        30,
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

/// The keycode the server has for `keysym` on this keyboard.
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
        .position(|syms| syms.contains(&keysym))
        .expect("the keyboard has the key");
    min + at as u8
}

const SPACE: u32 = 0x20;
const LETTER_A: u32 = 0x61;

fn key(kind: u8, code: u8, window: u32, time: u32, state: KeyButMask) -> KeyPressEvent {
    KeyPressEvent {
        response_type: kind,
        detail: code,
        sequence: 0,
        time,
        root: 0,
        event: window,
        child: 0,
        root_x: 0,
        root_y: 0,
        event_x: 10,
        event_y: 10,
        state,
        same_screen: true,
    }
}

/// Sends `event` to `window`; with `propagate`, as the server does with a
/// key on a window that did not select it — on up to the first that did.
fn send(connection: &RustConnection, window: u32, propagate: bool, event: KeyPressEvent) {
    let mask = if event.response_type == KEY_PRESS_EVENT {
        EventMask::KEY_PRESS
    } else {
        EventMask::KEY_RELEASE
    };
    connection
        .send_event(propagate, window, mask, event)
        .unwrap();
    connection.flush().unwrap();
}

/// Everything polled for a quarter of a second, added up.
fn settle(window: &mut PluginWindow) -> u32 {
    let mut total = 0;
    let started = std::time::Instant::now();
    while started.elapsed() < std::time::Duration::from_millis(250) {
        let polled: GuiPoll = window.poll();
        total += polled.play_pause;
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    total
}

/// A window inside the plugin's area, as a plugin makes one: listening for
/// keys or not.
fn plugin_child(connection: &RustConnection, parent: u32, listens: bool) -> u32 {
    let child = connection.generate_id().unwrap();
    let mask = if listens {
        EventMask::KEY_PRESS | EventMask::KEY_RELEASE
    } else {
        EventMask::EXPOSURE
    };
    connection
        .create_window(
            x11rb::COPY_DEPTH_FROM_PARENT,
            child,
            parent,
            0,
            0,
            100,
            100,
            0,
            WindowClass::INPUT_OUTPUT,
            0,
            &CreateWindowAux::new().event_mask(mask),
        )
        .unwrap();
    connection.map_window(child).unwrap();
    connection.flush().unwrap();
    child
}

#[test]
fn a_space_on_the_window_is_play_and_stop() {
    let Some((mut window, connection)) = open() else {
        return;
    };
    let space = keycode(&connection, SPACE);
    let frame = window.frame_id() as u32;
    send(
        &connection,
        frame,
        false,
        key(KEY_PRESS_EVENT, space, frame, 100, KeyButMask::default()),
    );
    send(
        &connection,
        frame,
        false,
        key(KEY_RELEASE_EVENT, space, frame, 180, KeyButMask::default()),
    );
    assert_eq!(settle(&mut window), 1);
    assert_eq!(settle(&mut window), 0, "said once");
}

#[test]
fn a_space_the_plugin_did_not_take_comes_up_to_the_window() {
    let Some((mut window, connection)) = open() else {
        return;
    };
    let space = keycode(&connection, SPACE);
    let deaf = plugin_child(&connection, window.id() as u32, false);
    send(
        &connection,
        deaf,
        true,
        key(KEY_PRESS_EVENT, space, deaf, 100, KeyButMask::default()),
    );
    assert_eq!(settle(&mut window), 1);
}

#[test]
fn a_space_the_plugin_takes_stays_the_plugins() {
    let Some((mut window, connection)) = open() else {
        return;
    };
    let space = keycode(&connection, SPACE);
    // On its own connection, as a plugin's is: a window that listens for keys
    // is where they stop.
    let (plugin, _) = x11rb::connect(None).unwrap();
    let typing = plugin_child(&plugin, window.id() as u32, true);
    send(
        &connection,
        typing,
        true,
        key(KEY_PRESS_EVENT, space, typing, 100, KeyButMask::default()),
    );
    assert_eq!(settle(&mut window), 0, "typed into the plugin");
    drop(plugin);
}

#[test]
fn another_key_or_a_held_modifier_or_a_repeat_is_not_play() {
    let Some((mut window, connection)) = open() else {
        return;
    };
    let space = keycode(&connection, SPACE);
    let letter = keycode(&connection, LETTER_A);
    let frame = window.frame_id() as u32;
    let none = KeyButMask::default();
    send(
        &connection,
        frame,
        false,
        key(KEY_PRESS_EVENT, letter, frame, 100, none),
    );
    send(
        &connection,
        frame,
        false,
        key(KEY_PRESS_EVENT, space, frame, 110, KeyButMask::CONTROL),
    );
    send(
        &connection,
        frame,
        false,
        key(KEY_RELEASE_EVENT, space, frame, 120, KeyButMask::CONTROL),
    );
    assert_eq!(settle(&mut window), 0);

    // Held: the server repeats it as a release and a press at one moment.
    send(
        &connection,
        frame,
        false,
        key(KEY_PRESS_EVENT, space, frame, 200, none),
    );
    for t in [230, 260, 290] {
        send(
            &connection,
            frame,
            false,
            key(KEY_RELEASE_EVENT, space, frame, t, none),
        );
        send(
            &connection,
            frame,
            false,
            key(KEY_PRESS_EVENT, space, frame, t, none),
        );
    }
    send(
        &connection,
        frame,
        false,
        key(KEY_RELEASE_EVENT, space, frame, 400, none),
    );
    assert_eq!(settle(&mut window), 1, "held is one press");
}

#[test]
fn a_window_on_no_screen_can_be_told_of_a_space() {
    let mut window = PluginWindow::headless_with_header(300, 200, 30);
    assert_eq!(window.poll().play_pause, 0);
    window.press_space();
    assert_eq!(window.poll().play_pause, 1);
    assert_eq!(window.poll().play_pause, 0);
}
