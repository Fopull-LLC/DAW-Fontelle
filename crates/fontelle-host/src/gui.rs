//! A plugin's **own** editor window (TDD §8.4, §16).
//!
//! > *"shouldnt these also be showing the custom plugins own display in their
//! > windows not a auto made one from the parameters."*
//!
//! Until this existed, a plugin was edited on Fontelle's own panel: a grid of
//! knobs built from the parameter list, which is a fallback and reads like
//! one. It is the right fallback — it is automatable, it is the same panel a
//! soundfont gets, and it works for a plugin that has no editor at all — but
//! it is not what a synth looks like, and for a plugin whose whole state is a
//! file it has loaded (an LV2 sampler, a soundfont player) it is not enough to
//! use the plugin with.
//!
//! # Why there is an X11 window in here
//!
//! CLAP's GUI extension offers two shapes: **floating**, where the plugin owns
//! its window, and **embedded**, where the host provides one and the plugin
//! draws into it. Floating is optional and, on Linux, rare —  Surge XT's CLAP
//! build answers `is_api_supported` with *x11, embedded* and refuses all three
//! of the others, and that is the ordinary answer from anything built on JUCE.
//! So a host that wants to show a plugin's editor on this platform has to hand
//! it an **X11 window**, whatever the host's own windows are made of.
//!
//! Fontelle's windows are `winit`'s, and on this desktop that means Wayland,
//! which a plugin cannot embed into. Rather than move the whole studio to
//! XWayland — one process has one windowing backend, so that would be every
//! window, not just this one — the editor window is made here, directly, with
//! `x11rb`. It is a top-level window the desktop manages like any other; the
//! plugin fills it; and the studio beside it is untouched. That works on an
//! X11 session and on a Wayland one with XWayland, which is every Linux
//! desktop this decade.
//!
//! # What the host owes a plugin's GUI
//!
//! More than a window. A CLAP plugin's editor is not given a thread: it runs
//! on the host's **main thread**, and the host has to drive it —
//!
//! - **timers** (`clap_host_timer_support`): the plugin registers a period and
//!   the host calls `on_timer`. This is how a JUCE editor repaints.
//! - **file descriptors** (`clap_host_posix_fd_support`): the plugin registers
//!   its X11 connection and the host calls `on_fd` when it has something to
//!   read. This is how the editor sees a mouse click.
//!
//! Without those two a plugin window opens grey and stays grey, which is the
//! commonest way a first attempt at this fails. [`HostedPlugin::tick_gui`] is
//! where both are paid, once per frame, from the loop that already runs.

#[cfg(unix)]
use std::os::fd::RawFd;
use std::time::{Duration, Instant};

#[cfg(target_os = "linux")]
use x11rb::connection::Connection;
#[cfg(target_os = "linux")]
use x11rb::protocol::Event as X11Event;
#[cfg(target_os = "linux")]
use x11rb::protocol::xproto::{
    AtomEnum, ConnectionExt as _, CreateWindowAux, EventMask, KeyButMask, PropMode, WindowClass,
};
#[cfg(target_os = "linux")]
use x11rb::rust_connection::RustConnection;
#[cfg(target_os = "linux")]
use x11rb::wrapper::ConnectionExt as _;

/// How big a plugin's editor is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GuiSize {
    pub width: u32,
    pub height: u32,
}

impl GuiSize {
    /// A size no plugin asked for, for a plugin that will not say.
    pub const FALLBACK: Self = Self {
        width: 800,
        height: 600,
    };

    /// Clamped to something a desktop can show. A plugin that reports nonsense
    /// — zero, or forty thousand — gets a window somebody can still close.
    fn sane(self) -> Self {
        Self {
            width: self.width.clamp(64, 8192),
            height: self.height.clamp(64, 8192),
        }
    }
}

/// What happened to a plugin's editor window since it was last asked.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct GuiPoll {
    /// The desktop resized the window; the plugin has been told. The
    /// plugin's area, below the strip.
    pub resized: Option<GuiSize>,
    /// Somebody pressed the window's close button.
    pub closed: bool,
    /// Presses on the strip across the top, in its own pixels, in order.
    pub header_presses: Vec<(i32, i32)>,
    /// Where the pointer is over the strip, when that changed: `Some(None)`
    /// when it left.
    pub header_hover: Option<Option<(i32, i32)>>,
    /// Presses of the space bar the plugin did not take: on the strip, or
    /// passed up from a plugin window that does not listen for keys. The
    /// studio's play and stop, as its own window's space is. A held space is
    /// one press, and one with Shift, Ctrl, Alt or the logo key is none.
    ///
    /// Reported: space only played while the studio's window had the
    /// keyboard. A plugin that listens for keys keeps them, so a space typed
    /// into its preset name is still a space; and only a window the keyboard
    /// is in hears a key, so this is no hotkey.
    pub play_pause: u32,
}

/// The scale a desktop asks X11 programs to draw at, from the X server's
/// resource database: `Xft.dpi` over the 96 that is 1×.
///
/// KDE and GNOME both set it when the desktop is scaled — on Wayland too,
/// for the programs XWayland runs, which every plugin editor is. Clamped to
/// 1×–4×: smaller is not a scale anybody means, and a stray value must not
/// open a window bigger than any screen. Nothing said is 1×.
pub fn scale_from_resources(resources: &str) -> f64 {
    resources
        .lines()
        .find_map(|line| line.strip_prefix("Xft.dpi:"))
        .and_then(|dpi| dpi.trim().parse::<f64>().ok())
        .filter(|dpi| dpi.is_finite())
        .map_or(1.0, |dpi| (dpi / 96.0).clamp(1.0, 4.0))
}

/// The `GDK_SCALE` the studio sets for itself, and so for every plugin editor
/// in it, before anything else runs: `already_set` is whether the
/// environment has one, `xsettings` whether the X display has an XSETTINGS
/// manager ([`xsettings_owner`]; `None` for no display).
///
/// Reported: amsynth 2.0.0's editor took the studio down — SIGSEGV in
/// `amsynth_lv2ui.so` at 0x38, on the main thread. On KDE Plasma under
/// Wayland nobody owns `_XSETTINGS_S0` on Xwayland, and amsynth 2.0.0 reads
/// its scale off JUCE's XSETTINGS object without asking whether there is one
/// (fixed after 2.0.0: *"Fix crash if there are no XSETTINGS"*, amsynth
/// issue #244). It reads `GDK_SCALE` first and stops there when it is set.
///
/// **1, and only where there are no XSETTINGS**, because that is the scale
/// GTK takes on X11 anyway when there are none to say otherwise: a GTK
/// editor is drawn as it was. Qt and JUCE do not read it. A value somebody
/// set is theirs.
pub fn gdk_scale_for(already_set: bool, xsettings: Option<bool>) -> Option<&'static str> {
    (!already_set && xsettings == Some(false)).then_some("1")
}

/// Whether the X display `display` (`None`: `$DISPLAY`) has an XSETTINGS
/// manager — somebody owning `_XSETTINGS_S<screen>`. `None` when there is no
/// display to ask.
#[cfg(target_os = "linux")]
pub fn xsettings_owner(display: Option<&str>) -> Option<bool> {
    let (connection, screen) = x11rb::connect(display).ok()?;
    let name = format!("_XSETTINGS_S{screen}");
    let atom = connection
        .intern_atom(false, name.as_bytes())
        .ok()?
        .reply()
        .ok()?
        .atom;
    let owner = connection.get_selection_owner(atom).ok()?.reply().ok()?;
    Some(owner.owner != 0)
}

/// Sets [`gdk_scale_for`]'s `GDK_SCALE` for this process, if there is one to
/// set.
///
/// # Safety
///
/// Changes the environment: call it before the process has a second thread,
/// as `std::env::set_var` asks.
#[cfg(target_os = "linux")]
pub unsafe fn steady_gdk_scale() {
    let already_set = std::env::var_os("GDK_SCALE").is_some();
    // No display at all is asked about only when there is one to ask.
    let xsettings = if already_set || std::env::var_os("DISPLAY").is_none() {
        None
    } else {
        xsettings_owner(None)
    };
    if let Some(scale) = gdk_scale_for(already_set, xsettings) {
        // SAFETY: the caller's — no other thread yet.
        unsafe { std::env::set_var("GDK_SCALE", scale) };
    }
}

/// Why a plugin's editor could not be opened.
#[derive(Debug)]
pub enum GuiError {
    /// The plugin has no editor at all, or none in a shape this can host.
    NoEditor,
    /// There is no X server to make a window on. On a bare Wayland session
    /// with no XWayland, which is rare and worth saying rather than crashing.
    NoDisplay(String),
    /// The plugin refused a step of the opening sequence.
    Refused(&'static str),
    /// This build shows no plugin windows on this platform — macOS, until
    /// an editor can be embedded in an `NSView`.
    NotOnThisPlatform,
}

impl std::fmt::Display for GuiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoEditor => write!(f, "this plugin has no editor Fontelle can show"),
            Self::NoDisplay(why) => write!(
                f,
                "a plugin editor needs an X server (XWayland on a Wayland desktop): {why}"
            ),
            Self::Refused(step) => write!(f, "the plugin refused to {step} its editor"),
            Self::NotOnThisPlatform => write!(
                f,
                "plugin windows are not shown on this platform yet \u{2014} its \
                 parameters are in Fontelle's own panel for it"
            ),
        }
    }
}

impl std::error::Error for GuiError {}

/// The window a plugin draws its editor into.
///
/// A top-level X11 window, made and owned here — see the module note on why it
/// is not one of `winit`'s. The plugin is given the id of a window inside it
/// and creates its own child window there; everything the desktop does to the
/// frame (a resize, the close button) arrives through [`poll`](Self::poll).
///
/// # The strip
///
/// > *"we need to ensure our presets system works with it kind of like how
/// > flx does"*
///
/// FL Studio wraps a plugin's editor in a bar of its own. So may this: a
/// window opened [`with a header`](Self::open_with_header) is a strip that
/// many pixels high across the top — Fontelle's, drawn from pixels the studio
/// renders ([`set_header`](Self::set_header)), its presses reported by
/// [`poll`](Self::poll) — and under it the window the plugin is handed. Every
/// size this type speaks of is the **plugin's** area; the frame is that and
/// the strip.
pub struct PluginWindow {
    /// `None` for a window that is not on a screen — see
    /// [`headless`](Self::headless).
    server: Option<OnScreen>,
    size: GuiSize,
    /// How tall the strip across the top is, in pixels. Zero: no strip, and
    /// the plugin is handed the frame itself.
    header: u32,
    /// Presses a headless window was told of — see
    /// [`press_header`](Self::press_header).
    pressed: Vec<(i32, i32)>,
    /// Where the pointer was over the strip when last reported. Read only
    /// where there is a strip on a screen — X11 and Win32; macOS has none yet.
    #[cfg_attr(not(any(target_os = "linux", windows)), allow(dead_code))]
    hover: Option<(i32, i32)>,
    /// Spaces a headless window was told of — see
    /// [`press_space`](Self::press_space).
    spaces: u32,
}

impl PluginWindow {
    fn offscreen(size: GuiSize, header: u32) -> Self {
        Self {
            server: None,
            size: size.sane(),
            header,
            pressed: Vec::new(),
            hover: None,
            spaces: 0,
        }
    }

    /// **A headless window only**: what a space on it would report on a
    /// screen — the next [`poll`](Self::poll) says so.
    pub fn press_space(&mut self) {
        if self.server.is_none() {
            self.spaces += 1;
        }
    }

    /// A window that is not on a screen, with a strip `header` pixels high —
    /// [`headless`](Self::headless) for the studio's side of the strip.
    pub fn headless_with_header(width: u32, height: u32, header: u32) -> Self {
        Self::offscreen(GuiSize { width, height }, header)
    }

    /// How tall the strip across the top is. Zero for a window with none.
    pub fn header_height(&self) -> u32 {
        self.header
    }

    /// **A headless window only**: what a press on its strip at `(x, y)`
    /// would report on a screen — the next [`poll`](Self::poll) says so. A
    /// window on a screen hears its presses from the desktop, and this does
    /// nothing to it.
    pub fn press_header(&mut self, x: i32, y: i32) {
        if self.server.is_none() && y >= 0 && (y as u32) < self.header {
            self.pressed.push((x, y));
        }
    }

    /// The pointer moved to `at` over the strip, or off it — a hover change is
    /// reported only when it is one.
    #[cfg_attr(not(any(target_os = "linux", windows)), allow(dead_code))]
    fn hovered(&mut self, at: Option<(i32, i32)>, result: &mut GuiPoll) {
        if at != self.hover {
            self.hover = at;
            result.header_hover = Some(at);
        }
    }
}

/// The half of a [`PluginWindow`] that only exists when there is an X server.
#[cfg(target_os = "linux")]
struct OnScreen {
    connection: RustConnection,
    /// The frame: the strip and, under it, the plugin's window.
    window: u32,
    /// The window the plugin is handed. The frame itself when there is no
    /// strip.
    embed: u32,
    /// The atom the window manager sends when the close button is pressed.
    delete_window: u32,
    /// What the strip's pixels are drawn with.
    gc: u32,
    depth: u8,
    /// The strip, as the server takes it (BGRX), and its size — kept so an
    /// expose can put it back.
    strip: Option<(Vec<u8>, u32, u32)>,
    /// The desktop's scale, as the X server's resources say it — see
    /// [`scale_from_resources`].
    scale: f64,
    /// The space bar's keycode on this keyboard; zero when it has none.
    space: u8,
    /// Whether the space is down, and when it last came up: a held key
    /// repeats as a release and a press at the same moment, which is not a
    /// second press.
    space_down: bool,
    space_released_at: Option<u32>,
}

/// The same on Windows: the window's handle and what its window procedure
/// has seen. See [`win32`].
#[cfg(windows)]
struct OnScreen {
    hwnd: windows_sys::Win32::Foundation::HWND,
    /// Written by the window procedure, read by [`PluginWindow::poll`]. Boxed
    /// so its address — which the window keeps — does not move.
    seen: Box<win32::Seen>,
}

/// The same on macOS: the window, the view the plugin is handed, and the
/// strip's view. See [`cocoa`].
#[cfg(target_os = "macos")]
struct OnScreen {
    window: objc2::rc::Retained<objc2_app_kit::NSWindow>,
    embed: objc2::rc::Retained<cocoa::AreaView>,
    strip: Option<objc2::rc::Retained<cocoa::StripView>>,
    /// The close button has been reported.
    closed: bool,
}

/// Anywhere else, no plugin window at all. Uninhabited, so every branch on
/// `server` below is a branch the compiler knows is not taken.
#[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
enum OnScreen {}

#[cfg(target_os = "linux")]
impl PluginWindow {
    /// Opens a window of `size`, titled `title`, and maps it.
    pub fn open(title: &str, size: GuiSize) -> Result<Self, GuiError> {
        Self::open_with_header(title, size, 0)
    }

    /// Opens a window whose plugin area is `size`, with a strip `header`
    /// pixels high across the top of it — see the type's note.
    pub fn open_with_header(title: &str, size: GuiSize, header: u32) -> Result<Self, GuiError> {
        let size = size.sane();
        let (connection, screen_index) =
            x11rb::connect(None).map_err(|e| GuiError::NoDisplay(e.to_string()))?;
        let screen = &connection.setup().roots[screen_index];
        let (root, visual, black, depth) = (
            screen.root,
            screen.root_visual,
            screen.black_pixel,
            screen.root_depth,
        );
        let fail = |e: &dyn std::fmt::Display| GuiError::NoDisplay(e.to_string());
        let window = connection.generate_id().map_err(|e| fail(&e))?;
        // `STRUCTURE_NOTIFY` is the resize; the plugin's own connection asks
        // for everything else on its own child window, which is how X11
        // embedding works. The strip's presses, pointer and exposes are the
        // frame's — a press on the plugin never reaches here unless the plugin
        // did not want it, and one below the strip is dropped in `poll`.
        // Keys too: the frame hears one when the keyboard is in it — the
        // strip, or a window the desktop focused before the plugin took it —
        // and the window under the strip hears one a plugin window passed up
        // because it does not listen for keys. See `GuiPoll::play_pause`.
        let keys = EventMask::KEY_PRESS | EventMask::KEY_RELEASE;
        let mut mask = EventMask::STRUCTURE_NOTIFY | keys;
        if header > 0 {
            mask = mask
                | EventMask::EXPOSURE
                | EventMask::BUTTON_PRESS
                | EventMask::POINTER_MOTION
                | EventMask::LEAVE_WINDOW;
        }
        connection
            .create_window(
                x11rb::COPY_DEPTH_FROM_PARENT,
                window,
                root,
                0,
                0,
                size.width as u16,
                (size.height + header) as u16,
                0,
                WindowClass::INPUT_OUTPUT,
                visual,
                &CreateWindowAux::new()
                    // Black rather than whatever was on the screen: a plugin
                    // paints its own background, and the frame between the
                    // window appearing and its first paint should not be a
                    // photograph of the desktop.
                    .background_pixel(black)
                    .event_mask(mask),
            )
            .map_err(|e| fail(&e))?;
        let embed = if header > 0 {
            let embed = connection.generate_id().map_err(|e| fail(&e))?;
            connection
                .create_window(
                    x11rb::COPY_DEPTH_FROM_PARENT,
                    embed,
                    window,
                    0,
                    header as i16,
                    size.width as u16,
                    size.height as u16,
                    0,
                    WindowClass::INPUT_OUTPUT,
                    visual,
                    &CreateWindowAux::new()
                        .background_pixel(black)
                        .event_mask(keys),
                )
                .map_err(|e| fail(&e))?;
            let _ = connection.map_window(embed);
            embed
        } else {
            window
        };
        let gc = connection.generate_id().map_err(|e| fail(&e))?;
        let _ = connection.create_gc(gc, window, &x11rb::protocol::xproto::CreateGCAux::new());

        // The close button. Without this the desktop kills the connection
        // instead of telling us, which takes the studio with it.
        let atom = |name: &[u8]| {
            connection
                .intern_atom(false, name)
                .ok()
                .and_then(|cookie| cookie.reply().ok())
                .map(|reply| reply.atom)
                .unwrap_or(0)
        };
        let protocols = atom(b"WM_PROTOCOLS");
        let delete_window = atom(b"WM_DELETE_WINDOW");
        if protocols != 0 && delete_window != 0 {
            let _ = connection.change_property32(
                PropMode::REPLACE,
                window,
                protocols,
                AtomEnum::ATOM,
                &[delete_window],
            );
        }
        // What the desktop is scaled to, from the resources a scaled desktop
        // sets for X11 programs — see `scale_from_resources`.
        let scale = x11rb::protocol::xproto::ConnectionExt::get_property(
            &connection,
            false,
            root,
            AtomEnum::RESOURCE_MANAGER,
            AtomEnum::STRING,
            0,
            1 << 16,
        )
        .ok()
        .and_then(|cookie| cookie.reply().ok())
        .map_or(1.0, |reply| {
            scale_from_resources(&String::from_utf8_lossy(&reply.value))
        });
        let space = space_keycode(&connection);
        let mut screen_window = Self::offscreen(size, header);
        screen_window.server = Some(OnScreen {
            connection,
            window,
            embed,
            delete_window,
            gc,
            depth,
            strip: None,
            scale,
            space,
            space_down: false,
            space_released_at: None,
        });
        screen_window.set_title(title);
        if let Some(server) = &screen_window.server {
            server
                .connection
                .map_window(server.window)
                .map_err(|e| fail(&e))?;
            let _ = server.connection.flush();
        }
        Ok(screen_window)
    }

    /// The frame's own id — the strip and the plugin's window both. What a
    /// test sends a press to; a plugin is given [`id`](Self::id).
    pub fn frame_id(&self) -> u64 {
        self.server
            .as_ref()
            .map_or(0, |server| u64::from(server.window))
    }

    /// Shows `rgba` — `width` by `height`, row after row — as the strip.
    ///
    /// Kept, and put back whenever the server says the strip was uncovered.
    /// Sent in bands no longer than a request may be: a strip across a wide
    /// window at twice the scale is more than the 256 KiB a server without
    /// BIG-REQUESTS takes in one.
    pub fn set_header(&mut self, rgba: &[u8], width: u32, height: u32) {
        let header = self.header;
        let Some(server) = &mut self.server else {
            return;
        };
        if header == 0 || rgba.len() < (width * height * 4) as usize {
            return;
        }
        let bgrx: Vec<u8> = rgba
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| [p[2], p[1], p[0], 0])
            .collect();
        server.strip = Some((bgrx, width, height.min(header)));
        paint_strip(server);
    }
    /// A window that is not on a screen.
    ///
    /// **For tests, and only for tests.** A plugin's editor can be found,
    /// loaded, started, driven and asked what it thinks — all of which is the
    /// host's half and all of which is worth testing — on a machine with no
    /// display, which is where these tests mostly run. A UI given window `0`
    /// draws nowhere; everything either side of the drawing still happens.
    pub fn headless(width: u32, height: u32) -> Self {
        Self::offscreen(GuiSize { width, height }, 0)
    }

    /// Whether this window is on a screen at all.
    pub fn is_on_screen(&self) -> bool {
        self.server.is_some()
    }

    /// The X11 id a plugin is given as its parent — the window under the
    /// strip, or the frame when there is none. Zero for a headless one. A
    /// `u64` because on Windows the same number is an `HWND`.
    pub fn id(&self) -> u64 {
        self.server
            .as_ref()
            .map_or(0, |server| u64::from(server.embed))
    }

    pub fn size(&self) -> GuiSize {
        self.size
    }

    /// The display scale the plugin is told. One on X11, where a plugin
    /// reads the desktop's own setting.
    pub fn scale(&self) -> f64 {
        self.server.as_ref().map_or(1.0, |server| server.scale)
    }

    /// Names the window, so a desktop full of them can be told apart.
    pub fn set_title(&mut self, title: &str) {
        let Some(server) = &self.server else {
            return;
        };
        let _ = server.connection.change_property8(
            PropMode::REPLACE,
            server.window,
            AtomEnum::WM_NAME,
            AtomEnum::STRING,
            title.as_bytes(),
        );
        // And the modern one, which is what every desktop actually reads.
        let atom = |name: &[u8]| {
            server
                .connection
                .intern_atom(false, name)
                .ok()
                .and_then(|cookie| cookie.reply().ok())
                .map(|reply| reply.atom)
                .unwrap_or(0)
        };
        let (name, utf8) = (atom(b"_NET_WM_NAME"), atom(b"UTF8_STRING"));
        if name != 0 && utf8 != 0 {
            let _ = server.connection.change_property8(
                PropMode::REPLACE,
                server.window,
                name,
                utf8,
                title.as_bytes(),
            );
        }
        let _ = server.connection.flush();
    }

    /// Makes the frame `size`. What a plugin's own `request_resize` ends up
    /// calling.
    pub fn resize(&mut self, size: GuiSize) {
        let size = size.sane();
        if size == self.size {
            return;
        }
        self.size = size;
        let Some(server) = &self.server else {
            return;
        };
        let _ = server.connection.configure_window(
            server.window,
            &x11rb::protocol::xproto::ConfigureWindowAux::new()
                .width(size.width)
                .height(size.height + self.header),
        );
        if server.embed != server.window {
            let _ = server.connection.configure_window(
                server.embed,
                &x11rb::protocol::xproto::ConfigureWindowAux::new()
                    .width(size.width)
                    .height(size.height),
            );
        }
        let _ = server.connection.flush();
    }

    /// Brings it to the front.
    pub fn raise(&mut self) {
        let Some(server) = &self.server else {
            return;
        };
        let _ = server.connection.configure_window(
            server.window,
            &x11rb::protocol::xproto::ConfigureWindowAux::new()
                .stack_mode(x11rb::protocol::xproto::StackMode::ABOVE),
        );
        let _ = server.connection.flush();
    }

    /// Everything the desktop has done to the window since last time.
    ///
    /// **Never blocks**: this is called from the studio's own frame loop, and
    /// a host that waited on an X server would be a studio that stopped
    /// drawing whenever a plugin window was quiet.
    pub fn poll(&mut self) -> GuiPoll {
        let mut result = GuiPoll::default();
        let header = self.header;
        let Some(server) = &mut self.server else {
            result.header_presses = std::mem::take(&mut self.pressed);
            result.play_pause = std::mem::take(&mut self.spaces);
            return result;
        };
        let delete_window = server.delete_window;
        let mut seen = None;
        let mut expose = false;
        let mut pointer = None;
        while let Ok(Some(event)) = server.connection.poll_for_event() {
            match event {
                X11Event::ConfigureNotify(configure) if configure.window == server.window => {
                    // The frame: the plugin's area is what is left under the
                    // strip.
                    let size = GuiSize {
                        width: u32::from(configure.width),
                        height: u32::from(configure.height).saturating_sub(header),
                    };
                    if size != self.size && size.width > 0 && size.height > 0 {
                        seen = Some(size);
                    }
                }
                X11Event::ClientMessage(message)
                    if delete_window != 0 && message.data.as_data32()[0] == delete_window =>
                {
                    result.closed = true;
                }
                X11Event::Expose(exposed) if exposed.window == server.window => {
                    expose = true;
                }
                X11Event::ButtonPress(press)
                    if press.event == server.window
                        && press.detail == 1
                        && press.event_y >= 0
                        && (press.event_y as u32) < header =>
                {
                    result
                        .header_presses
                        .push((i32::from(press.event_x), i32::from(press.event_y)));
                }
                X11Event::MotionNotify(motion) if motion.event == server.window => {
                    let over = motion.event_y >= 0 && (motion.event_y as u32) < header;
                    pointer = Some(
                        over.then_some((i32::from(motion.event_x), i32::from(motion.event_y))),
                    );
                }
                X11Event::LeaveNotify(leave) if leave.event == server.window => {
                    pointer = Some(None);
                }
                // Any window it came to is ours: only the frame and the
                // window under the strip listen for keys on this connection.
                X11Event::KeyPress(press) if server.space != 0 && press.detail == server.space => {
                    let repeat = server.space_down || server.space_released_at == Some(press.time);
                    server.space_down = true;
                    let held = KeyButMask::SHIFT
                        | KeyButMask::CONTROL
                        | KeyButMask::MOD1
                        | KeyButMask::MOD4;
                    if !repeat && u16::from(press.state) & u16::from(held) == 0 {
                        result.play_pause += 1;
                    }
                }
                X11Event::KeyRelease(release)
                    if server.space != 0 && release.detail == server.space =>
                {
                    server.space_down = false;
                    server.space_released_at = Some(release.time);
                }
                _ => {}
            }
        }
        if let Some(size) = seen {
            self.size = size;
            result.resized = Some(size);
            if server.embed != server.window {
                let _ = server.connection.configure_window(
                    server.embed,
                    &x11rb::protocol::xproto::ConfigureWindowAux::new()
                        .width(size.width)
                        .height(size.height),
                );
                let _ = server.connection.flush();
            }
        }
        if expose {
            paint_strip(server);
        }
        if let Some(at) = pointer {
            self.hovered(at, &mut result);
        }
        result
    }

    /// The window's pixels, as `(width, height, RGBA)`.
    ///
    /// **For looking at what a plugin drew**, which is the only way to know
    /// that an editor opened rather than merely being created: the plugin
    /// paints into a child window of this one, so the image is taken with
    /// inferiors included. §2.5's "seen once by a human" for a window this
    /// program does not draw a pixel of.
    pub fn grab(&self) -> Option<(u16, u16, Vec<u8>)> {
        // The frame: the strip and the plugin both.
        let size = GuiSize {
            width: self.size.width,
            height: self.size.height + self.header,
        };
        let server = self.server.as_ref()?;
        let image = server
            .connection
            .get_image(
                x11rb::protocol::xproto::ImageFormat::Z_PIXMAP,
                server.window,
                0,
                0,
                size.width as u16,
                size.height as u16,
                !0,
            )
            .ok()?
            .reply()
            .ok()?;
        // X11 hands back BGRA on every visual this runs on.
        let rgba = image
            .data
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| [p[2], p[1], p[0], 255])
            .collect();
        Some((size.width as u16, size.height as u16, rgba))
    }
}

/// Puts the strip's pixels on the frame, in bands a request can hold.
/// The keycode the space bar sends on the X server's keyboard, or zero.
#[cfg(target_os = "linux")]
fn space_keycode(connection: &RustConnection) -> u8 {
    const SPACE: u32 = 0x20;
    let setup = connection.setup();
    let (min, max) = (setup.min_keycode, setup.max_keycode);
    let Some(map) = x11rb::protocol::xproto::ConnectionExt::get_keyboard_mapping(
        connection,
        min,
        max.saturating_sub(min).saturating_add(1),
    )
    .ok()
    .and_then(|cookie| cookie.reply().ok()) else {
        return 0;
    };
    let per = usize::from(map.keysyms_per_keycode).max(1);
    map.keysyms
        .chunks(per)
        .position(|syms| syms.first() == Some(&SPACE))
        .map_or(0, |at| min.saturating_add(at as u8))
}

#[cfg(target_os = "linux")]
fn paint_strip(server: &OnScreen) {
    let Some((pixels, width, height)) = &server.strip else {
        return;
    };
    let stride = *width as usize * 4;
    // What one request may carry, less its own header.
    use x11rb::connection::RequestConnection;
    let room = server
        .connection
        .maximum_request_bytes()
        .saturating_sub(64)
        .max(stride);
    let rows = (room / stride).max(1);
    let mut y = 0usize;
    while y < *height as usize {
        let band = rows.min(*height as usize - y);
        let _ = server.connection.put_image(
            x11rb::protocol::xproto::ImageFormat::Z_PIXMAP,
            server.window,
            server.gc,
            *width as u16,
            band as u16,
            0,
            y as i16,
            0,
            server.depth,
            &pixels[y * stride..(y + band) * stride],
        );
        y += band;
    }
    let _ = server.connection.flush();
}

#[cfg(target_os = "linux")]
impl Drop for PluginWindow {
    fn drop(&mut self) {
        let Some(server) = &self.server else {
            return;
        };
        let _ = server.connection.destroy_window(server.window);
        let _ = server.connection.flush();
    }
}

/// The window on a platform that cannot show one yet: it opens nowhere and
/// says so, and the headless form — every size question, every request the
/// plugin makes of it — behaves exactly as on Linux, so the code either side
/// of the drawing is one code. See [`OnScreen`].
#[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
impl PluginWindow {
    pub fn open(_title: &str, _size: GuiSize) -> Result<Self, GuiError> {
        Err(GuiError::NotOnThisPlatform)
    }

    pub fn open_with_header(title: &str, size: GuiSize, _header: u32) -> Result<Self, GuiError> {
        Self::open(title, size)
    }

    pub fn frame_id(&self) -> u64 {
        0
    }

    pub fn set_header(&mut self, _rgba: &[u8], _width: u32, _height: u32) {}

    pub fn scale(&self) -> f64 {
        1.0
    }

    pub fn headless(width: u32, height: u32) -> Self {
        Self::offscreen(GuiSize { width, height }, 0)
    }

    pub fn is_on_screen(&self) -> bool {
        self.server.is_some()
    }

    pub fn id(&self) -> u64 {
        0
    }

    pub fn size(&self) -> GuiSize {
        self.size
    }

    pub fn set_title(&mut self, _title: &str) {}

    pub fn resize(&mut self, size: GuiSize) {
        self.size = size.sane();
    }

    pub fn raise(&mut self) {}

    pub fn poll(&mut self) -> GuiPoll {
        GuiPoll {
            header_presses: std::mem::take(&mut self.pressed),
            play_pause: std::mem::take(&mut self.spaces),
            ..GuiPoll::default()
        }
    }

    pub fn grab(&self) -> Option<(u16, u16, Vec<u8>)> {
        None
    }
}

/// Dispatches every window message waiting on this thread.
///
/// **Not for the studio**, whose event loop already does exactly this — a
/// plugin's windows are windows on the studio's thread, and winit's loop
/// dispatches all of them. For a loop that has no event loop of its own: the
/// editor probe and the tests. A no-op where a window is not driven by
/// messages.
pub fn pump_gui_messages() {
    #[cfg(windows)]
    win32::pump();
    #[cfg(target_os = "macos")]
    cocoa::pump();
}

/// A plugin editor's window on Windows.
///
/// > *"this is what happens to outside vsts in ur daw — it works but it's
/// > like only the knobs of like every parameter"*
///
/// Embedding on Windows is the simplest of the three: a VST 3 view attached
/// with `"HWND"`, a CLAP GUI given a `win32` window and a VST 2 editor opened
/// with `effEditOpen` all make a **child window** inside the `HWND` they are
/// given, and draw and take input there on their own. The studio's winit loop
/// dispatches every message on its thread — this window's and the plugin's
/// children's included — so nothing here pumps; the window procedure only
/// notes what the desktop did to the frame for [`PluginWindow::poll`].
#[cfg(windows)]
mod win32 {
    use std::cell::{Cell, RefCell};

    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
    use windows_sys::Win32::Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLACK_BRUSH, BeginPaint, DIB_RGB_COLORS, EndPaint,
        GetStockObject, HBRUSH, InvalidateRect, PAINTSTRUCT, SetDIBitsToDevice,
    };
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        GetKeyState, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent, VK_CONTROL, VK_LWIN, VK_MENU,
        VK_RWIN, VK_SHIFT, VK_SPACE,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        AdjustWindowRectEx, BringWindowToTop, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT,
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GWLP_USERDATA,
        GetClientRect, GetParent, GetWindowLongPtrW, IDC_ARROW, IsIconic, LoadCursorW, MSG,
        PM_REMOVE, PeekMessageW, RegisterClassExW, SW_RESTORE, SW_SHOW, SWP_NOACTIVATE, SWP_NOMOVE,
        SWP_NOZORDER, SetForegroundWindow, SetWindowLongPtrW, SetWindowPos, SetWindowTextW,
        ShowWindow, TranslateMessage, WM_CLOSE, WM_KEYDOWN, WM_LBUTTONDOWN, WM_MOUSEMOVE, WM_PAINT,
        WM_SIZE, WNDCLASSEXW, WS_CHILD, WS_CLIPCHILDREN, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
    };

    /// `WM_MOUSELEAVE`, which `windows-sys` files under the common controls.
    const WM_MOUSELEAVE: u32 = 0x02A3;

    use super::GuiSize;

    /// What the window procedure has seen since the host last asked.
    #[derive(Default)]
    pub(super) struct Seen {
        /// The plugin's area — the client area less the strip.
        pub(super) client: Cell<Option<GuiSize>>,
        pub(super) closed: Cell<bool>,
        /// How tall the strip is; zero for none.
        pub(super) header: Cell<u32>,
        /// The window the plugin is in, under the strip — resized with the
        /// frame. Null when there is no strip.
        pub(super) embed: Cell<HWND>,
        /// The strip's pixels, BGRA top-down, and their size.
        pub(super) strip: RefCell<Option<(Vec<u8>, u32, u32)>>,
        pub(super) presses: RefCell<Vec<(i32, i32)>>,
        /// `Some(None)`: the pointer left.
        pub(super) pointer: Cell<Option<Option<(i32, i32)>>>,
        pub(super) tracking: Cell<bool>,
        /// Spaces the plugin did not take — see `GuiPoll::play_pause`.
        pub(super) play_pause: Cell<u32>,
    }

    const STYLE: u32 = WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN;

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// The class every plugin window is made of, registered once.
    fn class() -> &'static [u16] {
        static CLASS: std::sync::OnceLock<Vec<u16>> = std::sync::OnceLock::new();
        CLASS.get_or_init(|| {
            let name = wide("FontellePluginEditor");
            // SAFETY: a class with a static name and a window procedure that
            // lives for the program; registering twice is refused harmlessly.
            unsafe {
                let class = WNDCLASSEXW {
                    cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                    style: CS_HREDRAW | CS_VREDRAW,
                    lpfnWndProc: Some(procedure),
                    hInstance: GetModuleHandleW(std::ptr::null()),
                    hCursor: LoadCursorW(std::ptr::null_mut(), IDC_ARROW),
                    // Black rather than white: a plugin paints its own
                    // background, and the frame before its first paint should
                    // not flash.
                    hbrBackground: GetStockObject(BLACK_BRUSH) as HBRUSH,
                    lpszClassName: name.as_ptr(),
                    ..std::mem::zeroed()
                };
                RegisterClassExW(&class);
            }
            name
        })
    }

    /// The outer size that gives a client area of `size`.
    fn outer(size: GuiSize) -> (i32, i32) {
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: size.width as i32,
            bottom: size.height as i32,
        };
        // SAFETY: a plain rectangle computation.
        unsafe { AdjustWindowRectEx(&mut rect, STYLE, 0, 0) };
        (rect.right - rect.left, rect.bottom - rect.top)
    }

    /// The class the plugin's own window under the strip is made of.
    fn embed_class() -> &'static [u16] {
        static CLASS: std::sync::OnceLock<Vec<u16>> = std::sync::OnceLock::new();
        CLASS.get_or_init(|| {
            let name = wide("FontellePluginEmbed");
            // SAFETY: as `class`, with the frame's procedure: it has no
            // user data of its own, so all it does with anything but a key
            // is the default — and a key a plugin passes up to it (JUCE's
            // editors pass up what they do not use) goes to the frame's.
            unsafe {
                let class = WNDCLASSEXW {
                    cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                    lpfnWndProc: Some(procedure),
                    hInstance: GetModuleHandleW(std::ptr::null()),
                    hCursor: LoadCursorW(std::ptr::null_mut(), IDC_ARROW),
                    hbrBackground: GetStockObject(BLACK_BRUSH) as HBRUSH,
                    lpszClassName: name.as_ptr(),
                    ..std::mem::zeroed()
                };
                RegisterClassExW(&class);
            }
            name
        })
    }

    pub(super) fn open(
        title: &str,
        size: GuiSize,
        header: u32,
    ) -> Result<(HWND, Box<Seen>), String> {
        let class = class();
        let title = wide(title);
        let (width, height) = outer(GuiSize {
            width: size.width,
            height: size.height + header,
        });
        // SAFETY: a top-level window of a registered class, on this thread,
        // whose user data is a `Seen` that outlives it (see `close`).
        unsafe {
            let hwnd = CreateWindowExW(
                0,
                class.as_ptr(),
                title.as_ptr(),
                STYLE,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                width,
                height,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                GetModuleHandleW(std::ptr::null()),
                std::ptr::null(),
            );
            if hwnd.is_null() {
                return Err(format!(
                    "the window could not be made: {}",
                    std::io::Error::last_os_error()
                ));
            }
            let seen = Box::new(Seen::default());
            seen.header.set(header);
            seen.embed.set(std::ptr::null_mut());
            if header > 0 {
                let embed = CreateWindowExW(
                    0,
                    embed_class().as_ptr(),
                    std::ptr::null(),
                    WS_CHILD | WS_VISIBLE | WS_CLIPCHILDREN,
                    0,
                    header as i32,
                    size.width as i32,
                    size.height as i32,
                    hwnd,
                    std::ptr::null_mut(),
                    GetModuleHandleW(std::ptr::null()),
                    std::ptr::null(),
                );
                seen.embed.set(embed);
            }
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, &*seen as *const Seen as isize);
            ShowWindow(hwnd, SW_SHOW);
            SetForegroundWindow(hwnd);
            Ok((hwnd, seen))
        }
    }

    pub(super) fn close(hwnd: HWND) {
        // SAFETY: the user data is cleared before the window goes, so no
        // message after this reads the `Seen` about to be dropped.
        unsafe {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            DestroyWindow(hwnd);
        }
    }

    /// The plugin's area: the client area, less the strip.
    pub(super) fn client(hwnd: HWND, header: u32) -> GuiSize {
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        // SAFETY: a live window of this thread.
        unsafe { GetClientRect(hwnd, &mut rect) };
        GuiSize {
            width: (rect.right - rect.left).max(0) as u32,
            height: ((rect.bottom - rect.top).max(0) as u32).saturating_sub(header),
        }
    }

    /// Hands the strip new pixels and asks for it to be painted.
    pub(super) fn set_strip(hwnd: HWND, seen: &Seen, rgba: &[u8], width: u32, height: u32) {
        let bgra: Vec<u8> = rgba
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| [p[2], p[1], p[0], 255])
            .collect();
        *seen.strip.borrow_mut() = Some((bgra, width, height));
        let strip = RECT {
            left: 0,
            top: 0,
            right: width as i32,
            bottom: seen.header.get() as i32,
        };
        // SAFETY: a live window of this thread.
        unsafe { InvalidateRect(hwnd, &strip, 0) };
    }

    pub(super) fn resize(hwnd: HWND, size: GuiSize, header: u32) {
        let (width, height) = outer(GuiSize {
            width: size.width,
            height: size.height + header,
        });
        // SAFETY: a live window of this thread.
        unsafe {
            SetWindowPos(
                hwnd,
                std::ptr::null_mut(),
                0,
                0,
                width,
                height,
                SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
    }

    pub(super) fn raise(hwnd: HWND) {
        // SAFETY: a live window of this thread.
        unsafe {
            if IsIconic(hwnd) != 0 {
                ShowWindow(hwnd, SW_RESTORE);
            }
            BringWindowToTop(hwnd);
            SetForegroundWindow(hwnd);
        }
    }

    pub(super) fn set_title(hwnd: HWND, title: &str) {
        let title = wide(title);
        // SAFETY: a live window of this thread and a NUL-terminated string.
        unsafe { SetWindowTextW(hwnd, title.as_ptr()) };
    }

    /// The monitor's scale: 96 DPI is one. The studio is per-monitor DPI
    /// aware (winit makes it so), so a plugin's sizes are real pixels and
    /// this is what it scales its drawing by.
    pub(super) fn scale(hwnd: HWND) -> f64 {
        // SAFETY: a live window of this thread.
        let dpi = unsafe { GetDpiForWindow(hwnd) };
        if dpi == 0 { 1.0 } else { f64::from(dpi) / 96.0 }
    }

    pub(super) fn pump() {
        // SAFETY: the ordinary message loop, drained without waiting.
        unsafe {
            let mut message: MSG = std::mem::zeroed();
            while PeekMessageW(&mut message, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
    }

    /// A mouse message's position, signed: `GET_X_LPARAM`.
    fn point(lparam: LPARAM) -> (i32, i32) {
        let x = (lparam & 0xFFFF) as u16 as i16;
        let y = ((lparam >> 16) & 0xFFFF) as u16 as i16;
        (i32::from(x), i32::from(y))
    }

    unsafe extern "system" fn procedure(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        // SAFETY: the user data is a `Seen` set by `open` and cleared by
        // `close` before it is dropped, or zero.
        let seen = unsafe { (GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const Seen).as_ref() };
        // A space, on the frame or passed up to the window under the strip,
        // that the plugin did not take: play and stop. Not a repeat (bit 30,
        // the key was already down), and not with a modifier held.
        if message == WM_KEYDOWN && wparam == WPARAM::from(VK_SPACE) && (lparam >> 30) & 1 == 0 {
            // SAFETY: plain reads of the keyboard state and of this window's
            // parent, whose user data is a `Seen` or zero as above.
            let target = seen.or_else(|| unsafe {
                (GetWindowLongPtrW(GetParent(hwnd), GWLP_USERDATA) as *const Seen).as_ref()
            });
            let held = [VK_SHIFT, VK_CONTROL, VK_MENU, VK_LWIN, VK_RWIN]
                .into_iter()
                .any(|key| unsafe { GetKeyState(i32::from(key)) } < 0);
            if let Some(target) = target
                && !held
            {
                target.play_pause.set(target.play_pause.get() + 1);
                return 0;
            }
        }
        match (message, seen) {
            // The close button closes the *editor*: the host is told and
            // decides, and the window stays until it is let go of.
            (WM_CLOSE, Some(seen)) => {
                seen.closed.set(true);
                0
            }
            (WM_SIZE, Some(seen)) => {
                let header = seen.header.get();
                let width = (lparam as u32) & 0xFFFF;
                let height = (((lparam as u32) >> 16) & 0xFFFF).saturating_sub(header);
                if width > 0 && height > 0 {
                    seen.client.set(Some(GuiSize { width, height }));
                    let embed = seen.embed.get();
                    if !embed.is_null() {
                        // SAFETY: the child this window made.
                        unsafe {
                            SetWindowPos(
                                embed,
                                std::ptr::null_mut(),
                                0,
                                header as i32,
                                width as i32,
                                height as i32,
                                SWP_NOZORDER | SWP_NOACTIVATE,
                            )
                        };
                    }
                }
                // SAFETY: the default handling of a message this window got.
                unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
            }
            (WM_PAINT, Some(seen)) if seen.header.get() > 0 => {
                // SAFETY: a paint of this window, begun and ended here; the
                // bits are the strip's, top-down (a negative height).
                unsafe {
                    let mut paint: PAINTSTRUCT = std::mem::zeroed();
                    let dc = BeginPaint(hwnd, &mut paint);
                    if let Some((bits, width, height)) = &*seen.strip.borrow() {
                        let mut info: BITMAPINFO = std::mem::zeroed();
                        info.bmiHeader = BITMAPINFOHEADER {
                            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                            biWidth: *width as i32,
                            biHeight: -(*height as i32),
                            biPlanes: 1,
                            biBitCount: 32,
                            biCompression: BI_RGB,
                            ..std::mem::zeroed()
                        };
                        SetDIBitsToDevice(
                            dc,
                            0,
                            0,
                            *width,
                            *height,
                            0,
                            0,
                            0,
                            *height,
                            bits.as_ptr().cast(),
                            &info,
                            DIB_RGB_COLORS,
                        );
                    }
                    EndPaint(hwnd, &paint);
                }
                0
            }
            (WM_LBUTTONDOWN, Some(seen)) => {
                let (x, y) = point(lparam);
                if y >= 0 && (y as u32) < seen.header.get() {
                    seen.presses.borrow_mut().push((x, y));
                }
                0
            }
            (WM_MOUSEMOVE, Some(seen)) if seen.header.get() > 0 => {
                let (x, y) = point(lparam);
                let over = y >= 0 && (y as u32) < seen.header.get();
                seen.pointer.set(Some(over.then_some((x, y))));
                if !seen.tracking.replace(true) {
                    let mut track = TRACKMOUSEEVENT {
                        cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE,
                        hwndTrack: hwnd,
                        dwHoverTime: 0,
                    };
                    // SAFETY: a plain request about this window.
                    unsafe { TrackMouseEvent(&mut track) };
                }
                0
            }
            (WM_MOUSELEAVE, Some(seen)) => {
                seen.tracking.set(false);
                seen.pointer.set(Some(None));
                0
            }
            // SAFETY: as above.
            _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
        }
    }
}

#[cfg(windows)]
impl PluginWindow {
    /// Opens a window whose client area is `size`, titled `title`, and shows
    /// it in front.
    pub fn open(title: &str, size: GuiSize) -> Result<Self, GuiError> {
        Self::open_with_header(title, size, 0)
    }

    /// Opens a window whose plugin area is `size`, with a strip `header`
    /// pixels high across its top — see the type's note.
    pub fn open_with_header(title: &str, size: GuiSize, header: u32) -> Result<Self, GuiError> {
        let size = size.sane();
        let (hwnd, seen) = win32::open(title, size, header).map_err(GuiError::NoDisplay)?;
        let size = win32::client(hwnd, header);
        seen.client.set(None);
        let mut window = Self::offscreen(size, header);
        window.server = Some(OnScreen { hwnd, seen });
        Ok(window)
    }

    /// See the Linux one: for tests, a window on no screen.
    pub fn headless(width: u32, height: u32) -> Self {
        Self::offscreen(GuiSize { width, height }, 0)
    }

    /// The frame's own `HWND`: the strip and the plugin's window both.
    pub fn frame_id(&self) -> u64 {
        self.server
            .as_ref()
            .map_or(0, |server| server.hwnd as usize as u64)
    }

    /// Shows `rgba` as the strip — see the Linux one.
    pub fn set_header(&mut self, rgba: &[u8], width: u32, height: u32) {
        if self.header == 0 || rgba.len() < (width * height * 4) as usize {
            return;
        }
        if let Some(server) = &self.server {
            win32::set_strip(
                server.hwnd,
                &server.seen,
                rgba,
                width,
                height.min(self.header),
            );
        }
    }

    pub fn is_on_screen(&self) -> bool {
        self.server.is_some()
    }

    /// The `HWND` a plugin is given as its parent — the window under the
    /// strip, or the frame when there is none. Zero for a headless one.
    pub fn id(&self) -> u64 {
        self.server.as_ref().map_or(0, |server| {
            let embed = server.seen.embed.get();
            if embed.is_null() {
                server.hwnd as usize as u64
            } else {
                embed as usize as u64
            }
        })
    }

    pub fn size(&self) -> GuiSize {
        self.size
    }

    /// The monitor's scale, for a plugin that is told rather than asks.
    pub fn scale(&self) -> f64 {
        self.server
            .as_ref()
            .map_or(1.0, |server| win32::scale(server.hwnd))
    }

    pub fn set_title(&mut self, title: &str) {
        if let Some(server) = &self.server {
            win32::set_title(server.hwnd, title);
        }
    }

    /// Makes the client area `size`. What a plugin's own resize request
    /// ends up calling.
    pub fn resize(&mut self, size: GuiSize) {
        let size = size.sane();
        if size == self.size {
            return;
        }
        let Some(server) = &self.server else {
            self.size = size;
            return;
        };
        win32::resize(server.hwnd, size, self.header);
        // What the desktop actually gave — a screen smaller than the plugin
        // gets a smaller window — and not news the next time it is polled.
        self.size = win32::client(server.hwnd, self.header);
        server.seen.client.set(None);
    }

    pub fn raise(&mut self) {
        if let Some(server) = &self.server {
            win32::raise(server.hwnd);
        }
    }

    /// Everything the desktop has done to the window since last time. Never
    /// blocks: the window procedure has already noted it.
    pub fn poll(&mut self) -> GuiPoll {
        let mut result = GuiPoll::default();
        let Some(server) = &self.server else {
            result.header_presses = std::mem::take(&mut self.pressed);
            result.play_pause = std::mem::take(&mut self.spaces);
            return result;
        };
        result.closed = server.seen.closed.replace(false);
        result.play_pause = server.seen.play_pause.replace(0);
        result.header_presses = std::mem::take(&mut *server.seen.presses.borrow_mut());
        let pointer = server.seen.pointer.take();
        if let Some(size) = server.seen.client.take()
            && size != self.size
        {
            self.size = size;
            result.resized = Some(size);
        }
        if let Some(at) = pointer {
            self.hovered(at, &mut result);
        }
        result
    }

    /// Not on Windows: the plugin draws with whatever it likes, and there is
    /// no one call that reads it back.
    pub fn grab(&self) -> Option<(u16, u16, Vec<u8>)> {
        None
    }
}

#[cfg(windows)]
impl Drop for PluginWindow {
    fn drop(&mut self) {
        if let Some(server) = &self.server {
            win32::close(server.hwnd);
        }
    }
}

/// One timer a plugin asked the host to run.
#[derive(Debug, Clone, Copy)]
pub(crate) struct HostTimer {
    pub(crate) id: u32,
    period: Duration,
    due: Instant,
}

/// The timers and file descriptors a plugin's editor asked for.
///
/// Kept on the host's main-thread handler, which is the only place CLAP allows
/// them to be registered from — and read once a frame by
/// [`crate::HostedPlugin::tick_gui`].
#[derive(Debug, Default)]
pub(crate) struct GuiPump {
    pub(crate) timers: Vec<HostTimer>,
    next_timer: u32,
    /// `posix-fd` is what its name says: a plugin on Windows has no
    /// descriptor to register, and CLAP does not offer the extension there.
    #[cfg(unix)]
    pub(crate) fds: Vec<RawFd>,
}

/// The fastest a plugin's timer is allowed to be.
///
/// CLAP says the host may raise a period that is too short, and asks that at
/// least 30 Hz be allowed. A plugin asking for a one-millisecond timer is
/// asking for a thousand callbacks a second on the thread that draws the
/// studio.
const FASTEST_TIMER: Duration = Duration::from_millis(16);

impl GuiPump {
    pub(crate) fn register_timer(&mut self, period_ms: u32) -> u32 {
        self.next_timer = self.next_timer.wrapping_add(1);
        let id = self.next_timer;
        let period = Duration::from_millis(u64::from(period_ms)).max(FASTEST_TIMER);
        self.timers.push(HostTimer {
            id,
            period,
            due: Instant::now() + period,
        });
        id
    }

    pub(crate) fn unregister_timer(&mut self, id: u32) -> bool {
        let before = self.timers.len();
        self.timers.retain(|timer| timer.id != id);
        self.timers.len() != before
    }

    #[cfg(unix)]
    pub(crate) fn register_fd(&mut self, fd: RawFd) {
        if !self.fds.contains(&fd) {
            self.fds.push(fd);
        }
    }

    #[cfg(unix)]
    pub(crate) fn unregister_fd(&mut self, fd: RawFd) {
        self.fds.retain(|held| *held != fd);
    }

    /// Which timers are due now, marking them for their next tick.
    pub(crate) fn due_timers(&mut self, now: Instant) -> Vec<u32> {
        let mut due = Vec::new();
        for timer in self.timers.iter_mut() {
            if timer.due <= now {
                // From *now* rather than from the old deadline: a studio that
                // was busy for a second must not then fire sixty catch-up
                // callbacks at a plugin.
                timer.due = now + timer.period;
                due.push(timer.id);
            }
        }
        due
    }

    /// Which registered descriptors have something to read, right now.
    ///
    /// A zero-length `poll(2)`, so this never waits: the studio's frame loop
    /// is the clock, and a host that blocked here would stop drawing.
    #[cfg(unix)]
    pub(crate) fn ready_fds(&self) -> Vec<RawFd> {
        if self.fds.is_empty() {
            return Vec::new();
        }
        let mut polls: Vec<libc_pollfd> = self
            .fds
            .iter()
            .map(|fd| libc_pollfd {
                fd: *fd,
                events: POLLIN,
                revents: 0,
            })
            .collect();
        // SAFETY: `polls` is a valid array of `nfds` initialised `pollfd`s for
        // the duration of the call, and a zero timeout cannot block.
        let ready = unsafe { poll(polls.as_mut_ptr(), polls.len() as u64, 0) };
        if ready <= 0 {
            return Vec::new();
        }
        polls
            .iter()
            .filter(|poll| poll.revents != 0)
            .map(|poll| poll.fd)
            .collect()
    }
}

// `poll(2)`, declared here rather than through a crate.
//
// One function and one struct, both fixed by POSIX, against a dependency whose
// whole job would be to declare them. `nfds_t` is `unsigned long` on every
// platform this builds for.
#[cfg(unix)]
#[repr(C)]
#[derive(Clone, Copy)]
struct libc_pollfd {
    fd: RawFd,
    events: i16,
    revents: i16,
}

#[cfg(unix)]
const POLLIN: i16 = 0x001;

#[cfg(unix)]
unsafe extern "C" {
    fn poll(fds: *mut libc_pollfd, nfds: u64, timeout: i32) -> i32;
}

/// A plugin editor's window on macOS.
///
/// > `docs/plugin-experience-backlog.md` §3: plugin editors did not open on a
/// > Mac at all, so every preset browser, wavetable editor and patch built
/// > inside a plugin was out of reach.
///
/// The CLAP `cocoa` API and VST 3's `"NSView"` both hand the plugin an
/// `NSView` it adds its own view to. So the window is an `NSWindow` whose
/// content holds that view, and — when there is one — the studio's strip
/// above it: a view of Fontelle's own that draws the image it is handed and
/// notes presses for [`PluginWindow::poll`]. The studio's winit loop runs
/// the application, which drives the plugin's views with everything else;
/// a test with no loop of its own calls [`pump_gui_messages`].
///
/// **Main thread only**, as everything in AppKit is: opening one anywhere
/// else is refused rather than attempted.
#[cfg(target_os = "macos")]
mod cocoa {
    use std::cell::RefCell;

    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::{
        AllocAnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send,
    };
    use objc2_app_kit::{
        NSApplication, NSAutoresizingMaskOptions, NSBackingStoreType, NSBitmapImageRep,
        NSDeviceRGBColorSpace, NSEvent, NSEventMask, NSEventModifierFlags, NSImage, NSView,
        NSWindow, NSWindowStyleMask,
    };
    use objc2_foundation::{NSDate, NSDefaultRunLoopMode, NSPoint, NSRect, NSSize, NSString};

    use super::{GuiError, GuiSize};

    /// What the strip view keeps: the image it draws and the presses on it.
    #[derive(Default)]
    pub(super) struct StripIvars {
        image: RefCell<Option<Retained<NSImage>>>,
        presses: RefCell<Vec<(i32, i32)>>,
    }

    define_class!(
        /// The studio's strip across the top of a plugin's window.
        #[unsafe(super(NSView))]
        #[thread_kind = MainThreadOnly]
        #[name = "FontellePluginStrip"]
        #[ivars = StripIvars]
        pub(super) struct StripView;

        impl StripView {
            /// Top-down, as the strip's pixels and its presses are.
            #[unsafe(method(isFlipped))]
            fn is_flipped(&self) -> bool {
                true
            }

            /// A press on a window that was not in front is still a press.
            #[unsafe(method(acceptsFirstMouse:))]
            fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
                true
            }

            #[unsafe(method(mouseDown:))]
            fn mouse_down(&self, event: &NSEvent) {
                let at = self.convertPoint_fromView(event.locationInWindow(), None);
                self.ivars()
                    .presses
                    .borrow_mut()
                    .push((at.x.round() as i32, at.y.round() as i32));
            }

            #[unsafe(method(drawRect:))]
            fn draw_rect(&self, _dirty: NSRect) {
                if let Some(image) = self.ivars().image.borrow().as_ref() {
                    image.drawInRect(self.bounds());
                }
            }
        }
    );

    /// What the plugin's area keeps: spaces the plugin passed up.
    #[derive(Default)]
    pub(super) struct AreaIvars {
        spaces: std::cell::Cell<u32>,
    }

    define_class!(
        /// The view a plugin's own view is put in. It takes the keyboard
        /// while the plugin has not, and hears what the plugin's views pass
        /// up the responder chain because they did not use it — a space
        /// among them is the studio's play and stop (`GuiPoll::play_pause`).
        #[unsafe(super(NSView))]
        #[thread_kind = MainThreadOnly]
        #[name = "FontellePluginArea"]
        #[ivars = AreaIvars]
        pub(super) struct AreaView;

        impl AreaView {
            #[unsafe(method(acceptsFirstResponder))]
            fn accepts_first_responder(&self) -> bool {
                true
            }

            #[unsafe(method(keyDown:))]
            fn key_down(&self, event: &NSEvent) {
                let held = NSEventModifierFlags::Shift
                    | NSEventModifierFlags::Control
                    | NSEventModifierFlags::Option
                    | NSEventModifierFlags::Command;
                let space = event
                    .charactersIgnoringModifiers()
                    .is_some_and(|keys| keys.to_string() == " ");
                if space && !event.isARepeat() && !event.modifierFlags().intersects(held) {
                    let spaces = &self.ivars().spaces;
                    spaces.set(spaces.get() + 1);
                    return;
                }
                // Anything else goes on up the chain, as it would have.
                unsafe { msg_send![super(self), keyDown: event] }
            }
        }
    );

    impl AreaView {
        fn new(mtm: MainThreadMarker, frame: NSRect) -> Retained<Self> {
            let this = Self::alloc(mtm).set_ivars(AreaIvars::default());
            unsafe { msg_send![super(this), initWithFrame: frame] }
        }

        pub(super) fn take_spaces(&self) -> u32 {
            self.ivars().spaces.replace(0)
        }
    }

    impl StripView {
        fn new(mtm: MainThreadMarker, frame: NSRect) -> Retained<Self> {
            let this = Self::alloc(mtm).set_ivars(StripIvars::default());
            unsafe { msg_send![super(this), initWithFrame: frame] }
        }

        pub(super) fn take_presses(&self) -> Vec<(i32, i32)> {
            std::mem::take(&mut *self.ivars().presses.borrow_mut())
        }

        /// Shows `rgba` (`width` × `height`, top row first) as the strip.
        pub(super) fn show(&self, rgba: &[u8], width: u32, height: u32) {
            let Some(rep) = (unsafe {
                NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
                    NSBitmapImageRep::alloc(),
                    std::ptr::null_mut(),
                    width as isize,
                    height as isize,
                    8,
                    4,
                    true,
                    false,
                    NSDeviceRGBColorSpace,
                    (width * 4) as isize,
                    32,
                )
            }) else {
                return;
            };
            let data = rep.bitmapData();
            if data.is_null() {
                return;
            }
            let len = (width * height * 4) as usize;
            // SAFETY: the rep allocated `bytesPerRow × height` bytes, which
            // is `len`; `rgba` was checked to hold at least that many.
            unsafe { std::ptr::copy_nonoverlapping(rgba.as_ptr(), data, len) };
            let image = NSImage::initWithSize(
                NSImage::alloc(),
                NSSize::new(f64::from(width), f64::from(height)),
            );
            image.addRepresentation(&rep);
            *self.ivars().image.borrow_mut() = Some(image);
            self.setNeedsDisplay(true);
        }
    }

    fn main_thread() -> Result<MainThreadMarker, GuiError> {
        MainThreadMarker::new().ok_or_else(|| {
            GuiError::NoDisplay("a plugin window on macOS has to be made on the main thread".into())
        })
    }

    pub(super) fn open(
        title: &str,
        size: GuiSize,
        header: u32,
    ) -> Result<super::OnScreen, GuiError> {
        let mtm = main_thread()?;
        // So a window can be made and shown with no studio loop around it —
        // the tests. The studio's own loop has made it already.
        let _ = NSApplication::sharedApplication(mtm);
        let (width, height) = (f64::from(size.width), f64::from(size.height));
        let total = height + f64::from(header);
        let style = NSWindowStyleMask::Titled
            | NSWindowStyleMask::Closable
            | NSWindowStyleMask::Miniaturizable
            | NSWindowStyleMask::Resizable;
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                NSRect::new(NSPoint::new(200.0, 200.0), NSSize::new(width, total)),
                style,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        // Closing it hides it; the host lets it go (`Drop`), not AppKit.
        unsafe { window.setReleasedWhenClosed(false) };
        window.setTitle(&NSString::from_str(title));
        let content = window
            .contentView()
            .ok_or_else(|| GuiError::NoDisplay("the window has no content view".into()))?;
        // The plugin's view at the bottom, the strip above it — AppKit's
        // origin is the bottom left.
        let embed = AreaView::new(
            mtm,
            NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(width, height)),
        );
        embed.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable
                | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        content.addSubview(&embed);
        let strip = (header > 0).then(|| {
            let strip = StripView::new(
                mtm,
                NSRect::new(
                    NSPoint::new(0.0, height),
                    NSSize::new(width, f64::from(header)),
                ),
            );
            strip.setAutoresizingMask(
                NSAutoresizingMaskOptions::ViewWidthSizable
                    | NSAutoresizingMaskOptions::ViewMinYMargin,
            );
            content.addSubview(&strip);
            strip
        });
        window.makeKeyAndOrderFront(None);
        // The keyboard starts in the plugin's area, so a space before the
        // plugin takes it is the studio's rather than the window's beep.
        window.makeFirstResponder(Some(&embed));
        Ok(super::OnScreen {
            window,
            embed,
            strip,
            closed: false,
        })
    }

    /// The plugin's area, as the window has it now.
    pub(super) fn embed_size(server: &super::OnScreen) -> GuiSize {
        let frame = server.embed.frame();
        GuiSize {
            width: frame.size.width.round().max(0.0) as u32,
            height: frame.size.height.round().max(0.0) as u32,
        }
    }

    pub(super) fn resize(server: &super::OnScreen, size: GuiSize, header: u32) {
        server.window.setContentSize(NSSize::new(
            f64::from(size.width),
            f64::from(size.height + header),
        ));
    }

    /// Every event waiting, handed to the application — for a loop with no
    /// event loop of its own.
    pub(super) fn pump() {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let app = NSApplication::sharedApplication(mtm);
        loop {
            let event = unsafe {
                app.nextEventMatchingMask_untilDate_inMode_dequeue(
                    NSEventMask::Any,
                    Some(&NSDate::distantPast()),
                    NSDefaultRunLoopMode,
                    true,
                )
            };
            let Some(event) = event else { break };
            app.sendEvent(&event);
        }
    }

    pub(super) fn view_ptr(view: &NSView) -> u64 {
        view as *const NSView as *const AnyObject as usize as u64
    }

    pub(super) fn window_ptr(window: &NSWindow) -> u64 {
        window as *const NSWindow as usize as u64
    }
}

#[cfg(target_os = "macos")]
impl PluginWindow {
    /// Opens a window whose plugin area is `size`, titled `title`.
    pub fn open(title: &str, size: GuiSize) -> Result<Self, GuiError> {
        Self::open_with_header(title, size, 0)
    }

    /// Opens a window whose plugin area is `size`, with a strip `header`
    /// points high across its top — see the type's note.
    pub fn open_with_header(title: &str, size: GuiSize, header: u32) -> Result<Self, GuiError> {
        let size = size.sane();
        let server = cocoa::open(title, size, header)?;
        let mut window = Self::offscreen(cocoa::embed_size(&server), header);
        window.server = Some(server);
        Ok(window)
    }

    /// See the Linux one: for tests, a window on no screen.
    pub fn headless(width: u32, height: u32) -> Self {
        Self::offscreen(GuiSize { width, height }, 0)
    }

    /// The `NSWindow` itself.
    pub fn frame_id(&self) -> u64 {
        self.server
            .as_ref()
            .map_or(0, |server| cocoa::window_ptr(&server.window))
    }

    /// Shows `rgba` as the strip — see the Linux one.
    pub fn set_header(&mut self, rgba: &[u8], width: u32, height: u32) {
        let height = height.min(self.header);
        if self.header == 0
            || width == 0
            || height == 0
            || rgba.len() < (width * height * 4) as usize
        {
            return;
        }
        if let Some(strip) = self
            .server
            .as_ref()
            .and_then(|server| server.strip.as_ref())
        {
            strip.show(rgba, width, height);
        }
    }

    pub fn is_on_screen(&self) -> bool {
        self.server.is_some()
    }

    /// The `NSView` a plugin is given to add its own to. Zero for a headless
    /// window.
    pub fn id(&self) -> u64 {
        self.server
            .as_ref()
            .map_or(0, |server| cocoa::view_ptr(&server.embed))
    }

    pub fn size(&self) -> GuiSize {
        self.size
    }

    /// The display's backing scale — 2 on a Retina screen.
    pub fn scale(&self) -> f64 {
        self.server
            .as_ref()
            .map_or(1.0, |server| server.window.backingScaleFactor().max(1.0))
    }

    pub fn set_title(&mut self, title: &str) {
        if let Some(server) = &self.server {
            server
                .window
                .setTitle(&objc2_foundation::NSString::from_str(title));
        }
    }

    /// Makes the plugin's area `size`. What a plugin's own resize request
    /// ends up calling.
    pub fn resize(&mut self, size: GuiSize) {
        let size = size.sane();
        if size == self.size {
            return;
        }
        let Some(server) = &self.server else {
            self.size = size;
            return;
        };
        cocoa::resize(server, size, self.header);
        self.size = cocoa::embed_size(server);
    }

    pub fn raise(&mut self) {
        if let Some(server) = &self.server {
            server.window.makeKeyAndOrderFront(None);
        }
    }

    /// Everything the desktop did to the window since last time.
    pub fn poll(&mut self) -> GuiPoll {
        let mut result = GuiPoll::default();
        let Some(server) = &mut self.server else {
            result.header_presses = std::mem::take(&mut self.pressed);
            result.play_pause = std::mem::take(&mut self.spaces);
            return result;
        };
        result.play_pause = server.embed.take_spaces();
        // The close button hides the window (`setReleasedWhenClosed(false)`).
        if !server.closed && !server.window.isVisible() {
            server.closed = true;
            result.closed = true;
        }
        if let Some(strip) = &server.strip {
            result.header_presses = strip.take_presses();
        }
        let size = cocoa::embed_size(server);
        if size != self.size {
            self.size = size;
            result.resized = Some(size);
        }
        result
    }

    /// Not on macOS: the plugin draws its own view.
    pub fn grab(&self) -> Option<(u16, u16, Vec<u8>)> {
        None
    }
}

#[cfg(target_os = "macos")]
impl Drop for PluginWindow {
    fn drop(&mut self) {
        if let Some(server) = &self.server {
            server.window.close();
        }
    }
}
