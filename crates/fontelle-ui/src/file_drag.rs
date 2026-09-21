//! A file dragged in from the desktop, under Wayland.
//!
//! > *"im noticing that on my particular setup at least i still cannot drag
//! > in files from my file explorer. im on cacheyos with kde plasma i use
//! > dolphin as my file browser."*
//!
//! # Why nothing arrived
//!
//! `winit` 0.30's Wayland backend has **no file drag-and-drop at all** — the
//! `HoveredFile` and `DroppedFile` events the window handles are produced by
//! its X11 backend (XDND), and by nothing on Wayland. On a Plasma Wayland
//! session the studio is a native Wayland window, so a drag out of Dolphin
//! was offered to a client that had never bound a data device and could not
//! hear it. The status line's *"Drop to open …"* had only ever been seen on
//! X11.
//!
//! # What this does about it
//!
//! It speaks the protocol itself, the way [`crate::activation`] speaks
//! xdg-activation: on the `wl_display` winit opened, with an event queue of
//! its own, it binds `wl_data_device_manager` and the seat and asks for a
//! `wl_data_device`. The compositor sends every drag over one of this
//! process's surfaces to that device — `enter`, `motion`, `leave`, `drop` —
//! with the pointer's **position** on every one of them, which is more than
//! XDND gives winit: on X11 a drop lands wherever the pointer was last seen
//! *before* the drag, since the source holds the pointer grab.
//!
//! A drag offers mime types; this takes `text/uri-list` and nothing else,
//! and asks the source for it as soon as the drag enters, so the file's name
//! is on the chip while it is still in the air. The bytes arrive down a
//! socket the source writes to on its own time, read a little each pass so
//! nothing waits on it — except the drop, which waits (briefly) for the list
//! if the source has not sent it yet. The action offered is **copy** and
//! only copy: a file manager that is told its drop was a *move* deletes the
//! original, and a sound imported into a song is never that.
//!
//! [`FileDrag::poll`] is called once a pass by the window and hands back what
//! happened as [`FileDragEvent`]s in that window's own terms — a surface it
//! can match against its windows and coordinates in logical pixels — so the
//! window treats a file from the desktop exactly as a row carried out of the
//! browser (`canvas::carry_target` with `desktop` set).
//!
//! On X11, and on a machine with no Wayland at all, [`FileDrag::open`]
//! answers `None` and the window keeps winit's own events.

use std::path::PathBuf;

/// What a drag did since the last poll, in the window's terms.
///
/// `surface` is the `wl_surface` the pointer is over as a raw proxy pointer,
/// which is what a `winit` window's handle gives out too — so the window can
/// tell the studio from an editor window without this module knowing either
/// exists. Positions are surface-local **logical** pixels, the unit the
/// layout is in.
#[derive(Debug, Clone, PartialEq)]
pub enum FileDragEvent {
    /// A drag came in over `surface`.
    Enter { surface: usize, x: f32, y: f32 },
    /// It moved.
    Motion { x: f32, y: f32 },
    /// The files being carried, once the source has said what they are —
    /// usually a moment after `Enter`, and before `Drop`.
    Files(Vec<PathBuf>),
    /// It left, or was cancelled. Nothing is in the air any more.
    Leave,
    /// It was let go over `surface`, carrying `paths`, at the last
    /// `Motion`'s position — the drop itself carries none. Never empty: a
    /// drop that carried no file this reads arrives as `Leave`.
    Drop { surface: usize, paths: Vec<PathBuf> },
}

/// The local paths in a `text/uri-list`.
///
/// One URI per line, `CRLF` by the RFC and bare `LF` from the things that
/// did not read it; `#` lines are comments. Only `file:` URIs on this
/// machine — no authority, or `localhost` — become paths; a file on another
/// host, or an `https:` link dragged out of a browser, is not something to
/// import. Percent-escapes are undone, because a folder called `My Loops`
/// arrives as `My%20Loops`, and an escape that is not one (`50%.wav`) is
/// left as it was rather than eaten.
pub fn paths_of_uri_list(text: &str) -> Vec<PathBuf> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| {
            let rest = line.strip_prefix("file:")?;
            // `file:///path` and `file://localhost/path` are this machine;
            // `file://host/path` is somebody else's.
            let path = match rest.strip_prefix("//") {
                Some(with_authority) => {
                    let slash = with_authority.find('/')?;
                    let (host, path) = with_authority.split_at(slash);
                    if !host.is_empty() && host != "localhost" {
                        return None;
                    }
                    path
                }
                None => rest,
            };
            Some(PathBuf::from(percent_decode(path)))
        })
        .collect()
}

/// `%XX` escapes undone, byte-wise, so a multi-byte character split across
/// escapes comes back whole. Anything that is not a valid escape is kept.
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: u8| (b as char).to_digit(16).map(|d| d as u8);
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(high), Some(low)) = (hex(bytes[i + 1]), hex(bytes[i + 2]))
        {
            out.push(high << 4 | low);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Where the pointer is over `window`, in logical pixels, on a display
/// where the window is told nothing while a file is dragged over it.
///
/// Under X11 the drag's source holds a pointer grab for the whole gesture,
/// so the window hears no `CursorMoved` and winit's `HoveredFile` carries no
/// position: a mark drawn at the last known pointer would sit wherever the
/// pointer was before the drag began. The X server will still say where the
/// pointer is when asked, so this asks, over a connection of its own. `None`
/// where the display is not X11, or the pointer is not over the window.
pub fn pointer_in(window: &winit::window::Window) -> Option<(f32, f32)> {
    imp::pointer_in(window)
}

#[cfg(target_os = "linux")]
mod imp {
    use std::io::Read;
    use std::os::fd::AsFd;
    use std::os::unix::net::UnixStream;
    use std::path::PathBuf;
    use std::sync::Mutex;
    use std::time::Duration;

    use wayland_client::backend::Backend;
    use wayland_client::globals::{GlobalListContents, registry_queue_init};
    use wayland_client::protocol::wl_data_device::{self, WlDataDevice};
    use wayland_client::protocol::wl_data_device_manager::{DndAction, WlDataDeviceManager};
    use wayland_client::protocol::wl_data_offer::{self, WlDataOffer};
    use wayland_client::protocol::wl_registry;
    use wayland_client::protocol::wl_seat::{self, WlSeat};
    use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle};
    use winit::raw_window_handle::{HasDisplayHandle, RawDisplayHandle};
    use winit::window::Window;

    use super::{FileDragEvent, paths_of_uri_list};

    /// The one mime type a drag is taken in.
    const URI_LIST: &str = "text/uri-list";

    /// How long a drop waits for a list the source has not sent yet. A file
    /// manager answers in a millisecond; this is for one that has hung, so
    /// the window does not hang with it.
    const DROP_WAIT: Duration = Duration::from_millis(1500);

    /// A hand on the seat's data device, over the display the studio window
    /// is already on.
    pub struct FileDrag {
        conn: Connection,
        queue: EventQueue<Dragging>,
        state: Dragging,
        _device: WlDataDevice,
    }

    /// The drag in the air, if there is one.
    struct Drag {
        offer: WlDataOffer,
        /// The `enter` serial, which every `accept` has to carry.
        serial: u32,
        surface: usize,
        /// Whether the offer had a list of files on it. One that did not is
        /// still tracked — its `leave` or `drop` still has to be answered —
        /// but nothing is said about it.
        accepted: bool,
        /// The socket the source writes the list down, until it has.
        reading: Option<UnixStream>,
        received: Vec<u8>,
        paths: Option<Vec<PathBuf>>,
    }

    /// What this connection's own objects have been told.
    #[derive(Default)]
    struct Dragging {
        drag: Option<Drag>,
        events: Vec<FileDragEvent>,
    }

    /// The mime types an offer has been described with, filled in by the
    /// `offer` events that precede its `enter`.
    #[derive(Default)]
    struct OfferData {
        mimes: Mutex<Vec<String>>,
    }

    impl FileDrag {
        /// Opens the protocol on `window`'s display, or `None` where that
        /// display is not Wayland.
        pub fn open(window: &Window) -> Option<Self> {
            let RawDisplayHandle::Wayland(display) = window.display_handle().ok()?.as_raw() else {
                return None;
            };
            // SAFETY: the pointer is the live `wl_display` winit opened for
            // this process, and winit keeps it open until its event loop
            // ends — which is why the app lets go of this in `exiting`,
            // before that: dropped with the app, after `run_app` returned,
            // it destroyed its proxies on a closed display (a segfault on
            // every exit). A foreign-display backend does not own the
            // display and does not close it when dropped. The same argument
            // `activation.rs` makes.
            let backend = unsafe { Backend::from_foreign_display(display.display.as_ptr().cast()) };
            let conn = Connection::from_backend(backend);
            let (globals, queue) = registry_queue_init::<Dragging>(&conn).ok()?;
            let handle = queue.handle();
            // Version 3 is where actions live, and a v3 target that never
            // says which action it takes gets its drop cancelled — see
            // `Dispatch<WlDataDevice>`. Older compositors get the v1 rules.
            let manager = globals
                .bind::<WlDataDeviceManager, _, _>(&handle, 1..=3, ())
                .ok()?;
            let seat = globals
                .bind::<WlSeat, _, _>(&handle, 1..=WlSeat::interface().version, ())
                .ok()?;
            let device = manager.get_data_device(&seat, &handle, ());
            conn.flush().ok()?;
            Some(Self {
                conn,
                queue,
                state: Dragging::default(),
                _device: device,
            })
        }

        /// Handles whatever the device has sent since the last time, reads
        /// whatever the source has written, and hands back what happened.
        /// Called once a pass by the window.
        pub fn poll(&mut self) -> Vec<FileDragEvent> {
            let _ = self.queue.dispatch_pending(&mut self.state);
            self.read_some();
            let _ = self.conn.flush();
            std::mem::take(&mut self.state.events)
        }

        /// Whether a drag is over one of this process's windows right now —
        /// the window keeps its loop awake while one is, so the list the
        /// source is writing gets read.
        pub fn dragging(&self) -> bool {
            self.state.drag.as_ref().is_some_and(|drag| drag.accepted)
        }

        /// Reads what the source has written so far, without waiting for
        /// it; at the end of the list, says what the files are.
        fn read_some(&mut self) {
            let Some(drag) = &mut self.state.drag else {
                return;
            };
            let Some(stream) = &mut drag.reading else {
                return;
            };
            let mut buf = [0u8; 4096];
            loop {
                match stream.read(&mut buf) {
                    Ok(0) => {
                        drag.reading = None;
                        let paths = paths_of_uri_list(&String::from_utf8_lossy(&drag.received));
                        drag.paths = Some(paths.clone());
                        if !paths.is_empty() {
                            self.state.events.push(FileDragEvent::Files(paths));
                        }
                        return;
                    }
                    Ok(n) => drag.received.extend_from_slice(&buf[..n]),
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => return,
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(_) => {
                        drag.reading = None;
                        drag.paths = Some(Vec::new());
                        return;
                    }
                }
            }
        }
    }

    impl Drag {
        /// Asks the source for the list, down a socket of this process's
        /// own. The source writes when it gets round to it; `read_some` and
        /// `wait_for_paths` are the two ways of collecting it.
        fn request_list(&mut self, conn: &Connection) {
            let Ok((ours, theirs)) = UnixStream::pair() else {
                return;
            };
            if ours.set_nonblocking(true).is_err() {
                return;
            }
            self.offer.receive(URI_LIST.to_string(), theirs.as_fd());
            // Flushed now: the request has to reach the compositor before
            // the source can see it, and `poll` is not for a while.
            let _ = conn.flush();
            drop(theirs);
            self.received.clear();
            self.reading = Some(ours);
        }

        /// The files, waiting for the source if it has not sent them yet.
        fn wait_for_paths(&mut self, conn: &Connection) -> Vec<PathBuf> {
            if let Some(paths) = &self.paths {
                return paths.clone();
            }
            if self.reading.is_none() {
                self.request_list(conn);
            }
            let Some(stream) = self.reading.take() else {
                return Vec::new();
            };
            // Blocking, with a deadline: a drop waits for its files, but not
            // for a source that has died with the drag in the air.
            let _ = stream.set_nonblocking(false);
            let _ = stream.set_read_timeout(Some(DROP_WAIT));
            let mut stream = stream;
            let mut buf = [0u8; 4096];
            let started = std::time::Instant::now();
            loop {
                match stream.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => self.received.extend_from_slice(&buf[..n]),
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(_) => break,
                }
                if started.elapsed() > DROP_WAIT {
                    break;
                }
            }
            let paths = paths_of_uri_list(&String::from_utf8_lossy(&self.received));
            self.paths = Some(paths.clone());
            paths
        }

        /// Tells the source what this window will do with the drag: take it
        /// as a copy, or not at all. Said on `enter` and again on every
        /// `motion`, which is what a v3 source needs to hear to let go.
        fn answer(&self) {
            if self.accepted {
                self.offer.accept(self.serial, Some(URI_LIST.to_string()));
                if self.offer.version() >= 3 {
                    self.offer.set_actions(DndAction::Copy, DndAction::Copy);
                }
            } else {
                self.offer.accept(self.serial, None);
                if self.offer.version() >= 3 {
                    self.offer.set_actions(DndAction::None, DndAction::None);
                }
            }
        }
    }

    /// See [`super::pointer_in`].
    pub fn pointer_in(window: &Window) -> Option<(f32, f32)> {
        use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
        use x11rb::protocol::xproto::ConnectionExt as _;

        let id = match window.window_handle().ok()?.as_raw() {
            RawWindowHandle::Xlib(handle) => handle.window as u32,
            RawWindowHandle::Xcb(handle) => handle.window.get(),
            _ => return None,
        };
        // A connection per ask. Once a pass while a file is in the air, so
        // the cost is a socket open per frame for the length of a drag —
        // nothing beside the frame — and nothing to keep in step with the
        // display winit is on.
        let (conn, _) = x11rb::connect(None).ok()?;
        let reply = conn.query_pointer(id).ok()?.reply().ok()?;
        if !reply.same_screen {
            return None;
        }
        let scale = window.scale_factor();
        Some((
            (f64::from(reply.win_x) / scale) as f32,
            (f64::from(reply.win_y) / scale) as f32,
        ))
    }

    impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for Dragging {
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

    impl Dispatch<WlDataDeviceManager, ()> for Dragging {
        fn event(
            _: &mut Self,
            _: &WlDataDeviceManager,
            _: <WlDataDeviceManager as Proxy>::Event,
            _: &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
        }
    }

    impl Dispatch<WlSeat, ()> for Dragging {
        fn event(
            _: &mut Self,
            _: &WlSeat,
            _: wl_seat::Event,
            _: &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
        }
    }

    impl Dispatch<WlDataOffer, OfferData> for Dragging {
        fn event(
            _: &mut Self,
            _: &WlDataOffer,
            event: wl_data_offer::Event,
            data: &OfferData,
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
            // The mime types come first, one event each, before the device
            // says what the offer is for. Kept on the offer, so `enter` can
            // read them off whichever offer it names.
            if let wl_data_offer::Event::Offer { mime_type } = event
                && let Ok(mut mimes) = data.mimes.lock()
            {
                mimes.push(mime_type);
            }
        }
    }

    impl Dispatch<WlDataDevice, ()> for Dragging {
        fn event(
            state: &mut Self,
            _: &WlDataDevice,
            event: wl_data_device::Event,
            _: &(),
            conn: &Connection,
            _: &QueueHandle<Self>,
        ) {
            match event {
                // Announced ahead of `enter` or `selection`; nothing to do
                // until one of those says which.
                wl_data_device::Event::DataOffer { .. } => {}

                wl_data_device::Event::Enter {
                    serial,
                    surface,
                    x,
                    y,
                    id,
                } => {
                    // A drag that never left cleanly: let the old one go.
                    if let Some(old) = state.drag.take() {
                        old.offer.destroy();
                    }
                    let Some(offer) = id else {
                        return;
                    };
                    let accepted = offer
                        .data::<OfferData>()
                        .and_then(|data| data.mimes.lock().ok())
                        .is_some_and(|mimes| mimes.iter().any(|mime| mime == URI_LIST));
                    let mut drag = Drag {
                        offer,
                        serial,
                        surface: surface.id().as_ptr() as usize,
                        accepted,
                        reading: None,
                        received: Vec::new(),
                        paths: None,
                    };
                    drag.answer();
                    if accepted {
                        // Asked for now rather than at the drop, so the
                        // chip can say what is in the air while it is.
                        drag.request_list(conn);
                        state.events.push(FileDragEvent::Enter {
                            surface: drag.surface,
                            x: x as f32,
                            y: y as f32,
                        });
                    }
                    state.drag = Some(drag);
                }

                wl_data_device::Event::Motion { x, y, .. } => {
                    if let Some(drag) = &state.drag {
                        drag.answer();
                        if drag.accepted {
                            state.events.push(FileDragEvent::Motion {
                                x: x as f32,
                                y: y as f32,
                            });
                        }
                    }
                }

                wl_data_device::Event::Leave => {
                    if let Some(drag) = state.drag.take() {
                        drag.offer.destroy();
                        if drag.accepted {
                            state.events.push(FileDragEvent::Leave);
                        }
                    }
                }

                wl_data_device::Event::Drop => {
                    let Some(mut drag) = state.drag.take() else {
                        return;
                    };
                    if !drag.accepted {
                        drag.offer.destroy();
                        return;
                    }
                    let paths = drag.wait_for_paths(conn);
                    // `finish` is what tells a v3 source its drop went
                    // through — without it Dolphin shows the drag as
                    // cancelled — and it may only be said for a drop that
                    // was taken.
                    if !paths.is_empty() && drag.offer.version() >= 3 {
                        drag.offer.finish();
                    }
                    drag.offer.destroy();
                    if paths.is_empty() {
                        state.events.push(FileDragEvent::Leave);
                    } else {
                        state.events.push(FileDragEvent::Drop {
                            surface: drag.surface,
                            paths,
                        });
                    }
                }

                // The clipboard. Not this module's business, but the offer
                // is this process's to let go of.
                wl_data_device::Event::Selection { id: Some(offer) } => offer.destroy(),

                _ => {}
            }
        }

        wayland_client::event_created_child!(Dragging, WlDataDevice, [
            wl_data_device::EVT_DATA_OFFER_OPCODE => (WlDataOffer, OfferData::default()),
        ]);
    }
}

#[cfg(not(target_os = "linux"))]
mod imp {
    use winit::window::Window;

    /// Nothing to speak to: only Wayland needs this, and winit's own
    /// events cover the other desktops.
    pub struct FileDrag;

    impl FileDrag {
        pub fn open(_: &Window) -> Option<Self> {
            None
        }

        pub fn poll(&mut self) -> Vec<super::FileDragEvent> {
            Vec::new()
        }

        pub fn dragging(&self) -> bool {
            false
        }
    }

    pub fn pointer_in(_: &Window) -> Option<(f32, f32)> {
        None
    }
}

pub use imp::FileDrag;
