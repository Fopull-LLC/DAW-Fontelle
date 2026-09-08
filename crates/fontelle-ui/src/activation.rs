//! Bringing a window to the front under Wayland.
//!
//! Reported three times from KDE Plasma: *"if a plugin or instrument window is
//! already open clicking on that instrument … should focus that window
//! again"*; after the first attempt, *"its still not bringing the windows to
//! the front for me"*; and after the second, *"it does flash in my taskbar
//! like its trying to focus that window but its not actually bringing the
//! window to the front"*.
//!
//! # Why the obvious calls do nothing
//!
//! Under Wayland a client may not take the focus, raise itself, or put itself
//! above another window by asking: `winit`'s `focus_window` and
//! `set_window_level` are documented no-ops there, and the first attempt at
//! this — holding the window "always on top" for one frame — was leaning on a
//! call the compositor never sees. The one thing the protocol *does* offer is
//! **xdg-activation**: a window that has the user's attention asks the
//! compositor for a token, and the token is spent on the window that should
//! have it next. The compositor decides whether the request is legitimate.
//!
//! # What KWin actually checks
//!
//! Read from `kwin/src/xdgactivationv1.cpp` and `activation.cpp` (Plasma 6.7),
//! because the second attempt got the taskbar flash and nothing else:
//!
//! - A token is **granted** when the surface it is asked for is the active
//!   window's — which the studio is, because the click was in it.
//! - A token is **honoured** (`Workspace::mayActivate`) only when it carries
//!   a serial no older than the compositor's last interaction:
//!   `lastInteractionSerial() <= tokenSerial`. A token with no serial fails
//!   that, and a failed activation is `demandAttention()` — the flash.
//! - A token spent on a window that has not painted yet is *kept* by that
//!   window and used when it does, so a new window needs exactly one, in its
//!   attributes.
//!
//! `winit` speaks half of this — a token in the attributes of a window being
//! made, delivered several callbacks later — and exposes neither a way to
//! activate an open window nor the serial of the click. So this module speaks
//! the protocol itself, on the same connection `winit` is using: it borrows
//! the `wl_display`, binds `xdg_activation_v1` and the seat on an event queue
//! of its own, and keeps the serial of the last button or key it saw. A
//! pointer and keyboard of its own on the shared seat hear every input event
//! the process is sent, which is the only way to a serial `winit` keeps to
//! itself. A token is asked for with a round trip and returned as a string,
//! carrying that serial and the studio's surface; it goes either into the
//! attributes of a window about to be made or straight to `activate` on one
//! that exists.
//!
//! On X11 (and on a machine with no Wayland at all) [`Activation::open`]
//! answers `None`, and the window keeps the plain calls that X11 honours.

#[cfg(target_os = "linux")]
mod imp {
    use wayland_client::backend::{Backend, ObjectId};
    use wayland_client::globals::{GlobalListContents, registry_queue_init};
    use wayland_client::protocol::wl_keyboard::{self, WlKeyboard};
    use wayland_client::protocol::wl_pointer::{self, WlPointer};
    use wayland_client::protocol::wl_seat::{self, WlSeat};
    use wayland_client::protocol::{wl_registry, wl_surface::WlSurface};
    use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle, WEnum};
    use wayland_protocols::xdg::activation::v1::client::xdg_activation_token_v1::{
        self, XdgActivationTokenV1,
    };
    use wayland_protocols::xdg::activation::v1::client::xdg_activation_v1::XdgActivationV1;
    use winit::raw_window_handle::{
        HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle,
    };
    use winit::window::Window;

    /// A hand on the compositor's activation protocol, over the display the
    /// studio window is already on.
    pub struct Activation {
        conn: Connection,
        queue: EventQueue<Seen>,
        activation: XdgActivationV1,
        seat: WlSeat,
        seen: Seen,
    }

    /// What this connection's own objects have been told.
    #[derive(Default)]
    struct Seen {
        /// The one token in flight. A request is answered by a round trip,
        /// so there is never more than one.
        token: Option<String>,
        /// The serial of the last button or key the seat sent this process
        /// — what a token has to carry to be honoured. Serials only grow, so
        /// the largest seen is the latest.
        serial: u32,
        pointer: Option<WlPointer>,
        keyboard: Option<WlKeyboard>,
    }

    impl Activation {
        /// Opens the protocol on `window`'s display, or `None` where that
        /// display is not Wayland.
        pub fn open(window: &Window) -> Option<Self> {
            let RawDisplayHandle::Wayland(display) = window.display_handle().ok()?.as_raw() else {
                return None;
            };
            // SAFETY: the pointer is the live `wl_display` winit opened for
            // this process, and winit keeps it open for as long as it has a
            // window — which is longer than any `Activation` lives, since
            // one is only made from a window and dropped with the app. A
            // foreign-display backend does not own the display and does not
            // close it when dropped.
            let backend = unsafe { Backend::from_foreign_display(display.display.as_ptr().cast()) };
            let conn = Connection::from_backend(backend);
            let (globals, mut queue) = registry_queue_init::<Seen>(&conn).ok()?;
            let handle = queue.handle();
            let activation = globals
                .bind::<XdgActivationV1, _, _>(&handle, 1..=1, ())
                .ok()?;
            // Any version the compositor has: the capabilities event this
            // needs is in the first, and a range past the crate's own would
            // be a panic rather than a refusal.
            let seat = globals
                .bind::<WlSeat, _, _>(&handle, 1..=WlSeat::interface().version, ())
                .ok()?;
            let mut seen = Seen::default();
            // The seat answers with its capabilities, and the pointer and
            // keyboard are made from those — see `Dispatch<WlSeat>`.
            queue.roundtrip(&mut seen).ok()?;
            Some(Self {
                conn,
                queue,
                activation,
                seat,
                seen,
            })
        }

        /// Handles whatever the seat has sent since the last time. Called
        /// once a frame by the window, so the serial is current when a token
        /// is asked for and the queue never grows without bound.
        pub fn poll(&mut self) {
            let _ = self.queue.dispatch_pending(&mut self.seen);
        }

        /// `window`'s own surface, as an object on this connection.
        fn surface(&self, window: &Window) -> Option<WlSurface> {
            let RawWindowHandle::Wayland(handle) = window.window_handle().ok()?.as_raw() else {
                return None;
            };
            // SAFETY: the pointer is a `wl_surface` proxy winit made on the
            // same display this connection borrows, and winit keeps it for
            // the life of the window that gave it to us.
            let id = unsafe {
                ObjectId::from_ptr(WlSurface::interface(), handle.surface.as_ptr().cast())
            }
            .ok()?;
            WlSurface::from_id(&self.conn, id).ok()
        }

        /// Asks the compositor for a token on `from`'s behalf, and waits for
        /// it. `from` must be the window the user is looking at — the one the
        /// click was in — or the compositor is entitled to answer with a token
        /// that grants nothing.
        pub fn token_from(&mut self, from: &Window) -> Option<String> {
            let surface = self.surface(from)?;
            // The click that got us here has been read by winit already, and
            // this pointer's copy of it is waiting on this queue: take it, so
            // the token carries the serial the compositor will compare
            // against.
            self.poll();
            let handle: QueueHandle<Seen> = self.queue.handle();
            let request = self.activation.get_activation_token(&handle, ());
            request.set_serial(self.seen.serial, &self.seat);
            request.set_surface(&surface);
            request.commit();
            self.seen.token = None;
            // Reads the socket and dispatches *this* queue only; winit's
            // events are queued for winit exactly as they would have been.
            self.queue.roundtrip(&mut self.seen).ok()?;
            self.seen.token.take()
        }

        /// Spends `token` on `target`: the compositor brings it to the front
        /// and gives it the focus, if the token was granted.
        pub fn activate(&mut self, token: String, target: &Window) -> bool {
            let Some(surface) = self.surface(target) else {
                return false;
            };
            self.activation.activate(token, &surface);
            self.conn.flush().is_ok()
        }
    }

    impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for Seen {
        fn event(
            _: &mut Self,
            _: &wl_registry::WlRegistry,
            _: wl_registry::Event,
            _: &GlobalListContents,
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
        }
    }

    impl Dispatch<XdgActivationV1, ()> for Seen {
        fn event(
            _: &mut Self,
            _: &XdgActivationV1,
            _: <XdgActivationV1 as Proxy>::Event,
            _: &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
        }
    }

    impl Dispatch<XdgActivationTokenV1, ()> for Seen {
        fn event(
            state: &mut Self,
            request: &XdgActivationTokenV1,
            event: xdg_activation_token_v1::Event,
            _: &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
            if let xdg_activation_token_v1::Event::Done { token } = event {
                state.token = Some(token);
            }
            request.destroy();
        }
    }

    impl Dispatch<WlSeat, ()> for Seen {
        fn event(
            state: &mut Self,
            seat: &WlSeat,
            event: wl_seat::Event,
            _: &(),
            _: &Connection,
            handle: &QueueHandle<Self>,
        ) {
            let wl_seat::Event::Capabilities {
                capabilities: WEnum::Value(capabilities),
            } = event
            else {
                return;
            };
            if capabilities.contains(wl_seat::Capability::Pointer) && state.pointer.is_none() {
                state.pointer = Some(seat.get_pointer(handle, ()));
            }
            if capabilities.contains(wl_seat::Capability::Keyboard) && state.keyboard.is_none() {
                state.keyboard = Some(seat.get_keyboard(handle, ()));
            }
        }
    }

    impl Dispatch<WlPointer, ()> for Seen {
        fn event(
            state: &mut Self,
            _: &WlPointer,
            event: wl_pointer::Event,
            _: &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
            // A button is an interaction as the compositor counts them; an
            // enter is not, but its serial is never older than the last one.
            if let wl_pointer::Event::Button { serial, .. }
            | wl_pointer::Event::Enter { serial, .. } = event
            {
                state.serial = state.serial.max(serial);
            }
        }
    }

    impl Dispatch<WlKeyboard, ()> for Seen {
        fn event(
            state: &mut Self,
            _: &WlKeyboard,
            event: wl_keyboard::Event,
            _: &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
            if let wl_keyboard::Event::Key { serial, .. }
            | wl_keyboard::Event::Enter { serial, .. } = event
            {
                state.serial = state.serial.max(serial);
            }
        }
    }
}

#[cfg(not(target_os = "linux"))]
mod imp {
    use winit::window::Window;

    /// Nothing to speak to: only Wayland has this protocol.
    pub struct Activation;

    impl Activation {
        pub fn open(_: &Window) -> Option<Self> {
            None
        }

        pub fn poll(&mut self) {}

        pub fn token_from(&mut self, _: &Window) -> Option<String> {
            None
        }

        pub fn activate(&mut self, _: String, _: &Window) -> bool {
            false
        }
    }
}

pub use imp::Activation;
