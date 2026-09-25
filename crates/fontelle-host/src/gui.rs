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
    AtomEnum, ConnectionExt as _, CreateWindowAux, EventMask, PropMode, WindowClass,
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
#[cfg(target_os = "linux")]
struct OnScreen {
    connection: RustConnection,
    window: u32,
    /// The atom the window manager sends when the close button is pressed.
    delete_window: u32,
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

/// The same on macOS, where a plugin's editor is not yet shown — the
/// embedding is an `NSView` and nothing here makes one. Uninhabited, so every
/// branch on `server` below is a branch the compiler knows is not taken.
#[cfg(not(any(target_os = "linux", windows)))]
enum OnScreen {}

#[cfg(target_os = "linux")]
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
    /// A `u64` because on Windows the same number is an `HWND`.
    pub fn id(&self) -> u64 {
        self.server
            .as_ref()
            .map_or(0, |server| u64::from(server.window))
    }

    pub fn size(&self) -> GuiSize {
        self.size
    }

    /// The display scale the plugin is told. One on X11, where a plugin
    /// reads the desktop's own setting.
    pub fn scale(&self) -> f64 {
        1.0
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
#[cfg(not(any(target_os = "linux", windows)))]
impl PluginWindow {
    pub fn open(_title: &str, _size: GuiSize) -> Result<Self, GuiError> {
        Err(GuiError::NoDisplay(
            "plugin editors are not shown on this platform yet".to_string(),
        ))
    }

    pub fn scale(&self) -> f64 {
        1.0
    }

    pub fn headless(width: u32, height: u32) -> Self {
        Self {
            server: None,
            size: GuiSize { width, height }.sane(),
        }
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
        GuiPoll::default()
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
    use std::cell::Cell;

    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
    use windows_sys::Win32::Graphics::Gdi::{BLACK_BRUSH, GetStockObject, HBRUSH};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        AdjustWindowRectEx, BringWindowToTop, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT,
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GWLP_USERDATA,
        GetClientRect, GetWindowLongPtrW, IDC_ARROW, IsIconic, LoadCursorW, MSG, PM_REMOVE,
        PeekMessageW, RegisterClassExW, SW_RESTORE, SW_SHOW, SWP_NOACTIVATE, SWP_NOMOVE,
        SWP_NOZORDER, SetForegroundWindow, SetWindowLongPtrW, SetWindowPos, SetWindowTextW,
        ShowWindow, TranslateMessage, WM_CLOSE, WM_SIZE, WNDCLASSEXW, WS_CLIPCHILDREN,
        WS_OVERLAPPEDWINDOW,
    };

    use super::GuiSize;

    /// What the window procedure has seen since the host last asked.
    #[derive(Default)]
    pub(super) struct Seen {
        pub(super) client: Cell<Option<GuiSize>>,
        pub(super) closed: Cell<bool>,
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

    pub(super) fn open(title: &str, size: GuiSize) -> Result<(HWND, Box<Seen>), String> {
        let class = class();
        let title = wide(title);
        let (width, height) = outer(size);
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

    pub(super) fn client(hwnd: HWND) -> GuiSize {
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
            height: (rect.bottom - rect.top).max(0) as u32,
        }
    }

    pub(super) fn resize(hwnd: HWND, size: GuiSize) {
        let (width, height) = outer(size);
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

    unsafe extern "system" fn procedure(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        // SAFETY: the user data is a `Seen` set by `open` and cleared by
        // `close` before it is dropped, or zero.
        let seen = unsafe { (GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const Seen).as_ref() };
        match (message, seen) {
            // The close button closes the *editor*: the host is told and
            // decides, and the window stays until it is let go of.
            (WM_CLOSE, Some(seen)) => {
                seen.closed.set(true);
                0
            }
            (WM_SIZE, Some(seen)) => {
                let width = (lparam as u32) & 0xFFFF;
                let height = ((lparam as u32) >> 16) & 0xFFFF;
                if width > 0 && height > 0 {
                    seen.client.set(Some(GuiSize { width, height }));
                }
                // SAFETY: the default handling of a message this window got.
                unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
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
        let size = size.sane();
        let (hwnd, seen) = win32::open(title, size).map_err(GuiError::NoDisplay)?;
        let size = win32::client(hwnd);
        seen.client.set(None);
        Ok(Self {
            server: Some(OnScreen { hwnd, seen }),
            size,
        })
    }

    /// See the Linux one: for tests, a window on no screen.
    pub fn headless(width: u32, height: u32) -> Self {
        Self {
            server: None,
            size: GuiSize { width, height }.sane(),
        }
    }

    pub fn is_on_screen(&self) -> bool {
        self.server.is_some()
    }

    /// The `HWND` a plugin is given as its parent. Zero for a headless one.
    pub fn id(&self) -> u64 {
        self.server
            .as_ref()
            .map_or(0, |server| server.hwnd as usize as u64)
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
        win32::resize(server.hwnd, size);
        // What the desktop actually gave — a screen smaller than the plugin
        // gets a smaller window — and not news the next time it is polled.
        self.size = win32::client(server.hwnd);
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
            return result;
        };
        result.closed = server.seen.closed.replace(false);
        if let Some(size) = server.seen.client.take()
            && size != self.size
        {
            self.size = size;
            result.resized = Some(size);
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
