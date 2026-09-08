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

use std::os::fd::RawFd;
use std::time::{Duration, Instant};

use x11rb::connection::Connection;
use x11rb::protocol::Event as X11Event;
use x11rb::protocol::xproto::{
    AtomEnum, ConnectionExt as _, CreateWindowAux, EventMask, PropMode, WindowClass,
};
use x11rb::rust_connection::RustConnection;
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
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct GuiPoll {
    /// The desktop resized the window; the plugin has been told.
    pub resized: Option<GuiSize>,
    /// Somebody pressed the window's close button.
    pub closed: bool,
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
        }
    }
}

impl std::error::Error for GuiError {}

/// The window a plugin draws its editor into.
///
/// A top-level X11 window, made and owned here — see the module note on why it
/// is not one of `winit`'s. The plugin is given its id and creates its own
/// child window inside it; everything the desktop does to the frame (a resize,
/// the close button) arrives through [`poll`](Self::poll).
pub struct PluginWindow {
    /// `None` for a window that is not on a screen — see
    /// [`headless`](Self::headless).
    server: Option<OnScreen>,
    size: GuiSize,
}

/// The half of a [`PluginWindow`] that only exists when there is an X server.
struct OnScreen {
    connection: RustConnection,
    window: u32,
    /// The atom the window manager sends when the close button is pressed.
    delete_window: u32,
}

impl PluginWindow {
    /// Opens a window of `size`, titled `title`, and maps it.
    pub fn open(title: &str, size: GuiSize) -> Result<Self, GuiError> {
        let size = size.sane();
        let (connection, screen_index) =
            x11rb::connect(None).map_err(|e| GuiError::NoDisplay(e.to_string()))?;
        let screen = &connection.setup().roots[screen_index];
        let window = connection
            .generate_id()
            .map_err(|e| GuiError::NoDisplay(e.to_string()))?;
        connection
            .create_window(
                x11rb::COPY_DEPTH_FROM_PARENT,
                window,
                screen.root,
                0,
                0,
                size.width as u16,
                size.height as u16,
                0,
                WindowClass::INPUT_OUTPUT,
                screen.root_visual,
                &CreateWindowAux::new()
                    // Black rather than whatever was on the screen: a plugin
                    // paints its own background, and the frame between the
                    // window appearing and its first paint should not be a
                    // photograph of the desktop.
                    .background_pixel(screen.black_pixel)
                    // `STRUCTURE_NOTIFY` is the resize; the plugin's own
                    // connection asks for everything else on its own child
                    // window, which is how X11 embedding works.
                    .event_mask(EventMask::STRUCTURE_NOTIFY),
            )
            .map_err(|e| GuiError::NoDisplay(e.to_string()))?;

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
        let mut screen_window = Self {
            server: Some(OnScreen {
                connection,
                window,
                delete_window,
            }),
            size,
        };
        screen_window.set_title(title);
        if let Some(server) = &screen_window.server {
            server
                .connection
                .map_window(server.window)
                .map_err(|e| GuiError::NoDisplay(e.to_string()))?;
            let _ = server.connection.flush();
        }
        Ok(screen_window)
    }

    /// A window that is not on a screen.
    ///
    /// **For tests, and only for tests.** A plugin's editor can be found,
    /// loaded, started, driven and asked what it thinks — all of which is the
    /// host's half and all of which is worth testing — on a machine with no
    /// display, which is where these tests mostly run. A UI given window `0`
    /// draws nowhere; everything either side of the drawing still happens.
    pub fn headless(width: u32, height: u32) -> Self {
        Self {
            server: None,
            size: GuiSize { width, height }.sane(),
        }
    }

    /// Whether this window is on a screen at all.
    pub fn is_on_screen(&self) -> bool {
        self.server.is_some()
    }

    /// The X11 id a plugin is given as its parent. Zero for a headless one.
    pub fn id(&self) -> u32 {
        self.server.as_ref().map_or(0, |server| server.window)
    }

    pub fn size(&self) -> GuiSize {
        self.size
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
                .height(size.height),
        );
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
        let Some(server) = &self.server else {
            return result;
        };
        let delete_window = server.delete_window;
        let mut seen = None;
        while let Ok(Some(event)) = server.connection.poll_for_event() {
            match event {
                X11Event::ConfigureNotify(configure) => {
                    let size = GuiSize {
                        width: u32::from(configure.width),
                        height: u32::from(configure.height),
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
                _ => {}
            }
        }
        if let Some(size) = seen {
            self.size = size;
            result.resized = Some(size);
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
        let size = self.size;
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

impl Drop for PluginWindow {
    fn drop(&mut self) {
        let Some(server) = &self.server else {
            return;
        };
        let _ = server.connection.destroy_window(server.window);
        let _ = server.connection.flush();
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

    pub(crate) fn register_fd(&mut self, fd: RawFd) {
        if !self.fds.contains(&fd) {
            self.fds.push(fd);
        }
    }

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
#[repr(C)]
#[derive(Clone, Copy)]
struct libc_pollfd {
    fd: RawFd,
    events: i16,
    revents: i16,
}

const POLLIN: i16 = 0x001;

unsafe extern "C" {
    fn poll(fds: *mut libc_pollfd, nfds: u64, timeout: i32) -> i32;
}
