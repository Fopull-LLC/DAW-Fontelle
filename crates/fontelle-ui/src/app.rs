//! The window: a `winit` event loop over a `wgpu` surface, drawing a `vello`
//! scene (TDD §16.2).
//!
//! This is deliberately the thinnest layer in the crate. Everything it decides
//! — the theme, the geometry, the glyph positions, and *whether a frame happens
//! at all* — is decided by a tested function somewhere else and read here
//! (`docs/first-usable-plan.md` §2.5). What is left is the part that genuinely
//! needs a window, and it is small enough to see all of.
//!
//! # Zero frames when idle (§16.3)
//!
//! The hard requirement, and the reason the loop is shaped the way it is:
//!
//! 1. The control flow is [`ControlFlow::Wait`]. With nothing to do the thread
//!    blocks in the compositor, using no CPU at all — not a timer, not a poll.
//! 2. Nothing calls `request_redraw` except a change: a resize, a theme swap, a
//!    widget invalidating itself. There is no "draw every frame" path to
//!    accidentally take.
//! 3. An animation (item 7's playhead) turns the loop continuous by holding a
//!    [`Redraw`] animator, and gives it back when it stops.
//!
//! [`WindowApp::frames_drawn`] counts what actually reached the GPU, so the
//! claim is checkable rather than asserted.

use std::sync::Arc;

use vello::util::{RenderContext, RenderSurface};
use vello::wgpu;
use vello::{AaConfig, RenderParams, Renderer, RendererOptions, Scene};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

use crate::canvas::{
    BrowserHit, BrowserLayout, Modifiers, MouseButton, PianoRoll, RackHit, RackLayout, RollControl,
    RollLayout, Tool, ToolbarLayout, browser_hit, browser_layout, rack_hit, rack_layout,
    roll_layout, scrolled, toolbar_hit, toolbar_layout, x_to_tick, y_to_key, zoom_x, zoom_y,
};
use crate::document::{ChannelInfo, LibraryEntry, StudioHost};
use crate::layout::{WindowLayout, window_layout};
use crate::render::{
    ADD_CHANNEL, BrowserChrome, Chrome, RackChrome, RenderError, RollChrome, SEARCH_HINT,
    TransportChrome, draw_window, key_name, label_stride, labelled_bar,
};
use crate::text::{Labels, TextContext, TextLayout};
use crate::theme::Theme;
use crate::transport::{
    Meter, TransportBarLayout, TransportHit, TransportHost, TransportView, apply, format_readout,
    hit, transport_bar_layout,
};
use crate::widget::{Sleep, WidgetId, WidgetTree, sleep_budget};

/// The piano roll's panel.
const PANEL: WidgetId = WidgetId::new(0);
/// The channel rack, and the soundfont browser under it (item 9). Their own
/// widgets so clicking a soundfont redraws the browser and not the roll.
const RACK: WidgetId = WidgetId::new(2);
const BROWSER: WidgetId = WidgetId::new(3);
/// The transport bar (item 7). Its own widget so a moving playhead dirties the
/// bar and nothing else — §16.4's rule, applied before there is any geometry
/// expensive enough for it to matter, because that is the only time it is
/// cheap to get right.
const TRANSPORT: WidgetId = WidgetId::new(1);

/// Until the time signature is in the document, 4/4 — which is what the
/// importer and the demo both assume anyway.
const BEATS_PER_BAR: u32 = 4;

/// What the window is opened with.
pub struct WindowOptions {
    /// The OS window's title.
    pub title: String,
    /// The name in the panel's header.
    pub panel_title: String,
    pub theme: Theme,
    /// Logical pixels.
    pub size: (u32, u32),
    /// Close the window by itself after this long. `None` is the normal case;
    /// a duration is how an unattended run proves it started, drew, and idled
    /// without someone sitting there to close it.
    pub run_for: Option<std::time::Duration>,
    /// The engine the transport bar drives. `None` opens a window with the bar
    /// drawn but inert — no audio device, or no project yet.
    pub host: Option<Box<dyn TransportHost>>,
    /// The studio the window shows and edits: the open clip, the channel rack
    /// and the soundfont bank. `None` opens an empty window.
    pub document: Option<Box<dyn StudioHost>>,
}

impl Default for WindowOptions {
    fn default() -> Self {
        Self {
            title: "Fontelle".to_string(),
            panel_title: "Fontelle".to_string(),
            theme: Theme::dark_default(),
            size: (1280, 720),
            run_for: None,
            host: None,
            document: None,
        }
    }
}

#[derive(Debug)]
pub enum WindowError {
    EventLoop(String),
    Window(String),
    Render(RenderError),
}

impl std::fmt::Display for WindowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EventLoop(why) => write!(f, "the event loop could not start: {why}"),
            Self::Window(why) => write!(f, "the window could not be opened: {why}"),
            Self::Render(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for WindowError {}

/// Opens the window and runs until it is closed.
pub fn run_window(options: WindowOptions) -> Result<WindowApp, WindowError> {
    let event_loop = EventLoop::new().map_err(|e| WindowError::EventLoop(e.to_string()))?;
    // §16.3: sleep until the OS has something to say. This one line is the
    // idle-CPU target; `Poll` here would burn a core doing nothing.
    event_loop.set_control_flow(ControlFlow::Wait);

    let mut app = WindowApp::new(options);
    event_loop
        .run_app(&mut app)
        .map_err(|e| WindowError::EventLoop(e.to_string()))?;
    match app.failure.take() {
        Some(e) => Err(e),
        None => Ok(app),
    }
}

/// The live window. Public so a caller can read [`WindowApp::frames_drawn`]
/// after the loop returns.
pub struct WindowApp {
    options: WindowOptions,
    text: TextContext,
    tree: WidgetTree,
    scene: Scene,
    context: RenderContext,
    /// One per device `RenderContext` has opened, indexed by its `dev_id`.
    renderers: Vec<Option<Renderer>>,
    live: Option<Live>,
    title: TextLayout,
    layout: WindowLayout,
    frames: u64,
    started: std::time::Instant,
    failure: Option<WindowError>,

    // --- the transport bar (item 7) ---
    bar: TransportBarLayout,
    /// The engine's state as of the last tick. Compared against the next one
    /// to decide whether anything moved, which is what keeps a stopped,
    /// silent window at zero frames.
    view: TransportView,
    meters: [Meter; 2],
    readout: TextLayout,
    cursor: (f32, f32),
    modifiers: winit::keyboard::ModifiersState,
    hover: Option<TransportHit>,
    last_tick: std::time::Instant,
    roll: PianoRoll,
    roll_layout: RollLayout,
    roll_bar: ToolbarLayout,

    // --- the docked panels (item 9) ---
    /// Everything the chrome draws that had to be shaped first. See
    /// [`crate::text::Labels`].
    labels: Labels,
    rack: RackLayout,
    browser: BrowserLayout,
    /// The studio's revision as of the last read, so the lists below are
    /// rebuilt when something changes them and not once a frame.
    studio_revision: u64,
    channels: Vec<ChannelInfo>,
    files: Vec<LibraryEntry>,
    presets: Vec<LibraryEntry>,
    selected_channel: usize,
    selected_file: Option<usize>,
    /// The live search (TDD §17.5), and whether it has the keyboard.
    query: String,
    searching: bool,
    status: String,
    rack_scroll: usize,
    file_scroll: usize,
    preset_scroll: usize,
    hover_control: Option<RollControl>,
    /// Whether a mouse button is down, so a `CursorMoved` is a drag rather
    /// than a hover.
    dragging: bool,
    /// The key currently sounding because the mouse is on it, so it can be
    /// released when the mouse is.
    auditioning: Option<u8>,
    /// Whether this window currently holds an animator on the tree's
    /// [`crate::widget::Redraw`]. Kept so `begin`/`end` stay paired — the
    /// counter is there so several moving things can coexist, and a caller
    /// that begins twice for one thing defeats it.
    animating: bool,
}

/// The window and its surface, which only exist between `resumed` and
/// `suspended` — on mobile the surface really does go away and come back, and
/// pretending otherwise is how a desktop-only assumption gets baked in.
struct Live {
    window: Arc<Window>,
    surface: RenderSurface<'static>,
}

impl WindowApp {
    fn new(options: WindowOptions) -> Self {
        let mut text = TextContext::new();
        let title = text.layout(&options.panel_title, &options.theme.font, None);
        let layout = window_layout(
            options.size.0 as f32,
            options.size.1 as f32,
            &options.theme.metrics,
        );
        Self {
            text,
            title,
            layout,
            tree: WidgetTree::new(),
            scene: Scene::new(),
            context: RenderContext::new(),
            renderers: Vec::new(),
            live: None,
            frames: 0,
            started: std::time::Instant::now(),
            failure: None,
            bar: transport_bar_layout(layout.transport, &options.theme.metrics),
            view: TransportView::unavailable(),
            meters: [Meter::new(); 2],
            readout: TextLayout::default(),
            cursor: (f32::MIN, f32::MIN),
            modifiers: winit::keyboard::ModifiersState::empty(),
            hover: None,
            last_tick: std::time::Instant::now(),
            roll: PianoRoll::new(Default::default()),
            roll_layout: roll_layout(layout.panel.body, &options.theme.metrics, true),
            roll_bar: ToolbarLayout { items: Vec::new() },
            labels: Labels::new(),
            rack: rack_layout(layout.rack.body, &options.theme.metrics, 0, 0),
            browser: browser_layout(layout.browser.body, &options.theme.metrics, 0, 0, 0, 0),
            studio_revision: u64::MAX,
            channels: Vec::new(),
            files: Vec::new(),
            presets: Vec::new(),
            selected_channel: 0,
            selected_file: None,
            query: String::new(),
            searching: false,
            status: String::new(),
            rack_scroll: 0,
            file_scroll: 0,
            preset_scroll: 0,
            hover_control: None,
            dragging: false,
            auditioning: None,
            animating: false,
            options,
        }
    }

    /// How many frames actually reached the GPU. The evidence for §16.3.
    pub fn frames_drawn(&self) -> u64 {
        self.frames
    }

    /// Recomputes the geometry for a new size and marks everything dirty.
    fn resize(&mut self, width: u32, height: u32, scale: f64) {
        let logical = (width as f64 / scale, height as f64 / scale);
        self.layout = window_layout(
            logical.0 as f32,
            logical.1 as f32,
            &self.options.theme.metrics,
        );
        self.bar = transport_bar_layout(self.layout.transport, &self.options.theme.metrics);
        self.relayout_panels();
        self.tree.insert(TRANSPORT, self.layout.transport);
        self.tree.insert(PANEL, self.layout.panel.frame);
        self.tree.insert(RACK, self.layout.rack.frame);
        self.tree.insert(BROWSER, self.layout.browser.frame);
        // Re-shaped against the header it has to fit in: a title wider than its
        // panel should wrap inside the header rather than run out over the
        // window, and how wide that is only becomes known here.
        let room = self.layout.panel.header.width - 2.0 * self.options.theme.metrics.panel_padding;
        self.title = self.text.layout(
            &self.options.panel_title,
            &self.options.theme.font,
            (room > 0.0).then_some(room),
        );
        // A resize invalidates the lot: the compositor hands back a surface
        // with nothing in it.
        self.tree.invalidate_rect(self.layout.window);
    }

    fn draw(&mut self) {
        if self.live.is_none() {
            return;
        }
        // Everything the frame is about to need, before the frame: the panels'
        // contents when the studio has changed them, and a shaped form of
        // every string the (pure) renderer will look up.
        self.refresh_studio();
        self.shape_labels();

        // §16.3, the whole of it: no dirty region, no frame.
        let Some(_region) = self.tree.take_dirty() else {
            return;
        };
        let Some(live) = &self.live else { return };

        let device = &self.context.devices[live.surface.dev_id];
        let Some(Some(renderer)) = self.renderers.get_mut(live.surface.dev_id) else {
            return;
        };

        draw_window(
            &mut self.scene,
            &self.options.theme,
            &self.layout,
            &Chrome {
                panel_title: &self.title,
                transport: TransportChrome {
                    layout: self.bar,
                    view: self.view,
                    meters: self.meters,
                    readout: &self.readout,
                    hover: self.hover,
                },
                roll: self.options.document.as_ref().map(|doc| RollChrome {
                    layout: self.roll_layout,
                    toolbar: self.roll_bar.clone(),
                    view: self.roll.view,
                    notes: doc.notes(),
                    selection: self.roll.selection(),
                    playhead_tick: doc.playhead_tick(self.view.position_sample),
                    beats_per_bar: doc.beats_per_bar(),
                    tool: self.roll.tool,
                    snap: self.roll.view.snap,
                    marquee: self.roll.marquee(),
                    hover: self.hover_control,
                }),
                rack: self.options.document.as_ref().map(|_| RackChrome {
                    panel: self.layout.rack,
                    layout: self.rack.clone(),
                    channels: &self.channels,
                    selected: self.selected_channel,
                }),
                browser: self.options.document.as_ref().map(|_| BrowserChrome {
                    panel: self.layout.browser,
                    layout: self.browser.clone(),
                    query: &self.query,
                    files: &self.files,
                    presets: &self.presets,
                    selected_file: self.selected_file,
                    searching: self.searching,
                }),
                labels: &self.labels,
                status: &self.status,
            },
        );

        let surface_texture = match live.surface.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t)
            | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
            // A surface going out of date mid-resize, or being occluded, is
            // ordinary. Put the region back and ask again rather than tearing
            // the loop down over it.
            _ => {
                self.tree.invalidate_rect(self.layout.window);
                live.window.request_redraw();
                return;
            }
        };
        if let Err(e) = renderer.render_to_texture(
            &device.device,
            &device.queue,
            &self.scene,
            &live.surface.target_view,
            &RenderParams {
                base_color: self.options.theme.palette.window.to_peniko(),
                width: live.surface.config.width,
                height: live.surface.config.height,
                antialiasing_method: AaConfig::Area,
            },
        ) {
            self.failure = Some(WindowError::Render(RenderError::Render(e.to_string())));
            return;
        }

        let mut encoder = device
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("fontelle-ui present"),
            });
        live.surface.blitter.copy(
            &device.device,
            &mut encoder,
            &live.surface.target_view,
            // The surface's own format, not an sRGB view of it: vello
            // configures the surface with an empty `view_formats`, and builds
            // its blitter for the plain format, so asking for the sRGB
            // variant here is a validation error rather than a colour space.
            &surface_texture
                .texture
                .create_view(&wgpu::TextureViewDescriptor::default()),
        );
        device.queue.submit([encoder.finish()]);
        live.window.pre_present_notify();
        surface_texture.present();
        let _ = device.device.poll(wgpu::PollType::Poll);

        self.frames += 1;
    }

    /// Reads the engine once, folds it into the meters, and dirties only what
    /// moved.
    ///
    /// This is the "state up" half of TDD §2.2, and the place the §16.3
    /// promise is kept: it is called on every pass of the loop, and on a
    /// stopped, silent window it finds nothing changed and marks nothing
    /// dirty, so no frame is issued. The comparison is against the whole
    /// view *and* the meter state, because a meter still falling after the
    /// last note is something moving even though the transport is not.
    fn tick(&mut self) {
        let now = std::time::Instant::now();
        // Clamped: a window that was dragged, minimised, or simply not
        // scheduled for a second must not make the meters jump a second's
        // worth of release in one step.
        let dt = (now - self.last_tick).as_secs_f32().min(0.25);
        self.last_tick = now;

        let view = match &mut self.options.host {
            Some(host) => host.view(),
            None => TransportView::unavailable(),
        };
        let mut meters = self.meters;
        for (index, meter) in meters.iter_mut().enumerate() {
            meter.update(view.peaks.get(index).copied().unwrap_or(0.0), dt);
        }

        // Whether anything on screen will change without the user doing
        // something: a rolling transport moves the playhead, and a meter above
        // the floor is still falling. Held as an animator on the tree, which
        // is what `arm_deadline` reads — one source of truth for "may the
        // window sleep", rather than two that can disagree.
        let moving = view.playing
            || meters
                .iter()
                .any(|m| m.level_db > crate::transport::METER_FLOOR_DB);
        if moving != self.animating {
            if moving {
                self.tree.redraw_mut().begin_animating();
            } else {
                self.tree.redraw_mut().end_animating();
            }
            self.animating = moving;
        }

        if view == self.view && meters == self.meters {
            return;
        }
        // The read-out is re-shaped only when the position it shows actually
        // changed, not once per frame: shaping allocates, and a stopped
        // transport should not be doing it at all.
        if view.position_sample != self.view.position_sample || !self.view.available {
            self.readout = self.text.layout(
                &format_readout(&view, BEATS_PER_BAR),
                &self.options.theme.font,
                None,
            );
        }
        self.view = view;
        self.meters = meters;
        self.tree.invalidate(TRANSPORT);
    }

    /// Recomputes what the pointer is over, dirtying the bar only if it
    /// changed.
    fn update_hover(&mut self) {
        let hover = hit(&self.bar, &self.view, self.cursor.0, self.cursor.1);
        if hover != self.hover {
            self.hover = hover;
            self.tree.invalidate(TRANSPORT);
        }
        let control = toolbar_hit(&self.roll_bar, self.cursor.0, self.cursor.1);
        if control != self.hover_control {
            self.hover_control = control;
            self.tree.invalidate(PANEL);
        }
    }

    /// True once the `run_for` deadline has passed, if there is one.
    fn expired(&self) -> bool {
        self.options
            .run_for
            .is_some_and(|d| self.started.elapsed() >= d)
    }
}

impl ApplicationHandler for WindowApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.live.is_some() {
            return;
        }
        self.started = std::time::Instant::now();

        let attributes = Window::default_attributes()
            .with_title(self.options.title.clone())
            .with_inner_size(winit::dpi::LogicalSize::new(
                self.options.size.0,
                self.options.size.1,
            ));
        let window = match event_loop.create_window(attributes) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                self.failure = Some(WindowError::Window(e.to_string()));
                event_loop.exit();
                return;
            }
        };

        let physical = window.inner_size();
        let surface = match crate::render::block_on(self.context.create_surface(
            window.clone(),
            physical.width.max(1),
            physical.height.max(1),
            // Vsync: the compositor's cadence is the only one worth drawing at,
            // and it is also what stops a redraw storm turning into a busy loop.
            wgpu::PresentMode::AutoVsync,
        )) {
            Ok(s) => s,
            Err(e) => {
                self.failure = Some(WindowError::Render(RenderError::Render(e.to_string())));
                event_loop.exit();
                return;
            }
        };

        // One renderer per device, built once. Shader compilation is most of
        // the cold-start budget (§19 wants under a second), so it must not
        // happen per frame or per resize.
        if self.renderers.len() <= surface.dev_id {
            self.renderers.resize_with(surface.dev_id + 1, || None);
        }
        if self.renderers[surface.dev_id].is_none() {
            match Renderer::new(
                &self.context.devices[surface.dev_id].device,
                RendererOptions {
                    use_cpu: false,
                    // Only the mode we ask for at render time: compiling the
                    // permutations we will never use is pure startup cost.
                    antialiasing_support: vello::AaSupport::area_only(),
                    num_init_threads: None,
                    pipeline_cache: None,
                },
            ) {
                Ok(r) => self.renderers[surface.dev_id] = Some(r),
                Err(e) => {
                    self.failure = Some(WindowError::Render(RenderError::Render(e.to_string())));
                    event_loop.exit();
                    return;
                }
            }
        }

        let scale = window.scale_factor();
        self.live = Some(Live { window, surface });
        self.resize(physical.width, physical.height, scale);
        if let Some(live) = &self.live {
            live.window.request_redraw();
        }
        self.arm_deadline(event_loop);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),

            WindowEvent::Resized(size) => {
                let Some(live) = &mut self.live else { return };
                let (w, h) = (size.width.max(1), size.height.max(1));
                self.context.resize_surface(&mut live.surface, w, h);
                let scale = live.window.scale_factor();
                self.resize(w, h, scale);
                if let Some(live) = &self.live {
                    live.window.request_redraw();
                }
            }

            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                let Some(live) = &self.live else { return };
                let size = live.window.inner_size();
                self.resize(size.width.max(1), size.height.max(1), scale_factor);
                if let Some(live) = &self.live {
                    live.window.request_redraw();
                }
            }

            WindowEvent::CursorMoved { position, .. } => {
                let Some(live) = &self.live else { return };
                // Logical pixels, because that is what the layout is in.
                let scale = live.window.scale_factor();
                self.cursor = ((position.x / scale) as f32, (position.y / scale) as f32);
                self.update_hover();
                if self.dragging {
                    self.drag_roll();
                }
                self.request_redraw_if_dirty();
            }

            WindowEvent::CursorLeft { .. } => {
                self.cursor = (f32::MIN, f32::MIN);
                self.update_hover();
                self.request_redraw_if_dirty();
            }

            WindowEvent::MouseInput {
                state: winit::event::ElementState::Pressed,
                button,
                ..
            } => {
                let (x, y) = self.cursor;
                self.dragging = true;
                self.press(button, x, y);
                self.request_redraw_if_dirty();
            }

            WindowEvent::MouseInput {
                state: winit::event::ElementState::Released,
                ..
            } => {
                self.dragging = false;
                let (x, y) = self.cursor;
                // Where the button came up is what a marquee needs: that is
                // the moment it decides what it caught.
                let grid = self.roll_layout.grid;
                match &self.options.document {
                    Some(doc) => self.roll.release_over(x, y, grid, doc.notes()),
                    None => self.roll.release(),
                }
                self.stop_audition();
                // One drag, one undo entry (§10.6). Only the caller knows the
                // mouse came up, which is exactly why `History` cannot decide
                // this for itself.
                if let Some(doc) = &mut self.options.document {
                    doc.end_gesture();
                }
                self.tree.invalidate(PANEL);
                self.request_redraw_if_dirty();
            }

            WindowEvent::MouseWheel { delta, .. } => {
                let (dx, dy) = match delta {
                    winit::event::MouseScrollDelta::LineDelta(x, y) => (x, y),
                    winit::event::MouseScrollDelta::PixelDelta(p) => {
                        (p.x as f32 / 40.0, p.y as f32 / 40.0)
                    }
                };
                self.scroll_roll(dx, dy);
                self.request_redraw_if_dirty();
            }

            WindowEvent::ModifiersChanged(state) => {
                self.modifiers = state.state();
                // §16.5's Alt and Shift apply to whatever gesture is running,
                // so the roll is told rather than asked.
                self.roll.set_modifiers(Modifiers {
                    ctrl: self.modifiers.control_key(),
                    shift: self.modifiers.shift_key(),
                    alt: self.modifiers.alt_key(),
                });
            }

            WindowEvent::KeyboardInput { event, .. } => {
                if event.state == winit::event::ElementState::Pressed {
                    self.key(&event);
                    self.request_redraw_if_dirty();
                }
            }

            // The compositor threw our pixels away, or is about to show them
            // again. Either way what we believe is on screen is no longer true.
            WindowEvent::Occluded(false) => {
                self.tree.invalidate_rect(self.layout.window);
                if let Some(live) = &self.live {
                    live.window.request_redraw();
                }
            }

            WindowEvent::RedrawRequested => {
                // The OS can ask for this on its own — after an unminimise, or
                // a compositor restart — and when it does it is asking for the
                // whole surface, because we cannot know what it kept.
                if !self.tree.has_dirty_regions() {
                    self.tree.invalidate_rect(self.layout.window);
                }
                self.draw();
                if self.failure.is_some() {
                    event_loop.exit();
                    return;
                }
                self.arm_deadline(event_loop);
            }

            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // The graph the audio thread handed back is freed here, on this
        // thread — see `fontelle_engine::GraphPublisher`. Cheap, and it has to
        // happen somewhere that runs whether or not a frame does.
        if let Some(doc) = &mut self.options.document {
            doc.pump();
        }
        self.refresh_studio();
        self.tick();
        self.request_redraw_if_dirty();
        self.arm_deadline(event_loop);
    }
}

impl WindowApp {
    // ------------------------------------------------------------ layout ---

    /// Recomputes every panel's inner geometry. Called on a resize, and
    /// whenever something changes how many rows a list has.
    fn relayout_panels(&mut self) {
        let m = &self.options.theme.metrics;
        self.roll_layout = roll_layout(self.layout.panel.body, m, self.roll.velocity_lane);
        self.roll_bar = toolbar_layout(self.roll_layout.toolbar, m);
        self.rack = rack_layout(
            self.layout.rack.body,
            m,
            self.channels.len(),
            self.rack_scroll,
        );
        self.browser = browser_layout(
            self.layout.browser.body,
            m,
            self.files.len(),
            self.presets.len(),
            self.file_scroll,
            self.preset_scroll,
        );
    }

    /// Re-reads the studio's lists, but only when it says they have changed.
    ///
    /// The alternative — asking for `Vec<ChannelInfo>` every frame — allocates
    /// once a frame for a list that changes when somebody clicks something.
    fn refresh_studio(&mut self) {
        let Some(doc) = &mut self.options.document else {
            return;
        };
        let revision = doc.revision();
        if revision == self.studio_revision {
            return;
        }
        self.studio_revision = revision;
        self.channels = doc.channels();
        self.files = doc.library_files();
        self.presets = doc.library_presets();
        self.selected_channel = doc.selected_channel();
        self.selected_file = doc.selected_file();
        self.query = doc.query().to_string();
        let status = doc.take_message().unwrap_or_else(|| doc.library_status());
        self.status = status;

        // A list that shrank under a scroll offset leaves a panel that looks
        // empty until somebody scrolls back up.
        self.file_scroll = self.file_scroll.min(self.files.len().saturating_sub(1));
        self.preset_scroll = self.preset_scroll.min(self.presets.len().saturating_sub(1));
        self.rack_scroll = self.rack_scroll.min(self.channels.len().saturating_sub(1));

        self.relayout_panels();
        self.tree.invalidate(RACK);
        self.tree.invalidate(BROWSER);
        self.tree.invalidate(PANEL);
    }

    /// Shapes everything the next frame will want to draw.
    ///
    /// `draw_window` is a pure function and cannot shape anything, so every
    /// string it will look up has to be in [`Labels`] first. Cached, so this is
    /// a few dozen hash lookups on a frame where nothing new appeared.
    fn shape_labels(&mut self) {
        let font = self.options.theme.font.clone();
        let want = |labels: &mut Labels, text: &mut TextContext, s: &str| {
            labels.ensure(s, &font, text);
        };

        for fixed in [
            "Channels",
            "Soundfonts",
            ADD_CHANNEL,
            SEARCH_HINT,
            "S",
            "M",
            "vel",
        ] {
            want(&mut self.labels, &mut self.text, fixed);
        }
        for (control, _) in &self.roll_bar.items {
            let caption = match control {
                RollControl::Snap => self.roll.view.snap.label(),
                other => other.label(),
            };
            self.labels.ensure(caption, &font, &mut self.text);
        }
        if !self.query.is_empty() {
            let query = self.query.clone();
            want(&mut self.labels, &mut self.text, &query);
        }
        if !self.status.is_empty() {
            let status = self.status.clone();
            want(&mut self.labels, &mut self.text, &status);
        }

        for row in &self.rack.rows {
            if let Some(channel) = self.channels.get(row.index) {
                self.labels.ensure(&channel.name, &font, &mut self.text);
            }
        }
        for (index, _) in &self.browser.file_rows {
            if let Some(entry) = self.files.get(*index) {
                self.labels.ensure(&entry.name, &font, &mut self.text);
                self.labels.ensure(&entry.detail, &font, &mut self.text);
            }
        }
        for (index, _) in &self.browser.preset_rows {
            if let Some(entry) = self.presets.get(*index) {
                self.labels.ensure(&entry.name, &font, &mut self.text);
                self.labels.ensure(&entry.detail, &font, &mut self.text);
            }
        }

        // The roll's own text: a name beside every C, and a number on every
        // bar line the ruler has room to number.
        if self.options.document.is_some() {
            for key in crate::canvas::visible_keys(&self.roll.view, self.roll_layout.grid) {
                if key % 12 == 0 {
                    self.labels.ensure(&key_name(key), &font, &mut self.text);
                }
            }
            let beats = self
                .options
                .document
                .as_ref()
                .map_or(BEATS_PER_BAR, |doc| doc.beats_per_bar());
            let bar = fontelle_types::PPQN * i64::from(beats.max(1));
            if let Some(stride) = label_stride(bar as f32 * self.roll.view.pixels_per_tick) {
                let ticks = crate::canvas::visible_ticks(&self.roll.view, self.roll_layout.grid);
                let mut tick = ticks.start - ticks.start.rem_euclid(bar);
                while tick < ticks.end {
                    let number = tick / bar + 1;
                    if labelled_bar(number, stride) {
                        self.labels
                            .ensure(&number.to_string(), &font, &mut self.text);
                    }
                    tick += bar;
                }
            }
        }
    }

    // ------------------------------------------------------------- mouse ---

    /// One press, routed to whichever panel it landed in.
    fn press(&mut self, button: winit::event::MouseButton, x: f32, y: f32) {
        // The transport bar first: it is the only thing above the panels.
        if let Some(what) = hit(&self.bar, &self.view, x, y) {
            if button == winit::event::MouseButton::Left
                && let Some(host) = &mut self.options.host
            {
                // Commands down (TDD §2.2): one click, one call, which is a
                // handful of relaxed stores on the other side. Nothing here
                // waits for the audio thread to acknowledge it — the next
                // `tick` reads back what actually happened.
                apply(host.as_mut(), what);
                self.tick();
            }
            return;
        }

        // Clicking anywhere but the search box gives the keyboard back to the
        // roll — otherwise typing a note-tool shortcut types it into the
        // search field instead.
        let in_search = self.browser.search.contains(x, y);
        if self.searching != in_search {
            self.searching = in_search;
            self.tree.invalidate(BROWSER);
        }

        if self.layout.rack.frame.contains(x, y) {
            self.press_rack(x, y);
            return;
        }
        if self.layout.browser.frame.contains(x, y) {
            self.press_browser(x, y);
            return;
        }

        let Some(button) = (match button {
            winit::event::MouseButton::Left => Some(MouseButton::Left),
            winit::event::MouseButton::Right => Some(MouseButton::Right),
            _ => None,
        }) else {
            return;
        };

        if self.roll_layout.toolbar.contains(x, y) {
            if button == MouseButton::Left
                && let Some(control) = toolbar_hit(&self.roll_bar, x, y)
            {
                self.activate(control);
            }
            return;
        }
        if self.roll_layout.velocity.contains(x, y) {
            self.press_velocity(x, y);
            return;
        }
        if self.roll_layout.keys.contains(x, y) {
            // The on-screen keyboard: clicking a key plays it (TDD §14.1's
            // live path). Nothing is written down — it is an audition.
            let key = y_to_key(&self.roll.view, self.roll_layout.grid, y);
            self.start_audition(key);
            return;
        }
        if self.roll_layout.ruler.contains(x, y) {
            self.seek_to(x);
            return;
        }
        self.press_roll(button, x, y);
    }

    /// A press that landed on the roll's grid.
    fn press_roll(&mut self, button: MouseButton, x: f32, y: f32) {
        let Some(doc) = &self.options.document else {
            return;
        };
        let beats_per_bar = doc.beats_per_bar();
        let edits = self.roll.press(
            button,
            x,
            y,
            self.roll_layout.grid,
            doc.notes(),
            beats_per_bar,
        );
        let drew = edits
            .iter()
            .any(|edit| matches!(edit, crate::canvas::RollEdit::Add { .. }));
        let key = y_to_key(&self.roll.view, self.roll_layout.grid, y);
        self.apply_roll_edits(edits);
        // What you draw, you hear — even stopped. The note is auditioned on
        // the live path rather than by starting the transport, which is the
        // difference between hearing what you just wrote and playing the song.
        if drew {
            self.start_audition(key);
        }
        // A press always changes the selection or the gesture, both of which
        // are visible.
        self.tree.invalidate(PANEL);
    }

    fn press_velocity(&mut self, x: f32, y: f32) {
        let Some(doc) = &self.options.document else {
            return;
        };
        let edits = self.roll.press_velocity(
            x,
            y,
            self.roll_layout.velocity,
            self.roll_layout.grid,
            doc.notes(),
        );
        self.apply_roll_edits(edits);
        self.tree.invalidate(PANEL);
    }

    fn press_rack(&mut self, x: f32, y: f32) {
        match rack_hit(&self.rack, x, y) {
            RackHit::Row(index) => {
                if let Some(doc) = &mut self.options.document {
                    doc.select_channel(index);
                }
                self.roll.clear_selection();
            }
            RackHit::Mute(index) => {
                if let Some(doc) = &mut self.options.document {
                    doc.toggle_mute(index);
                }
            }
            RackHit::Solo(index) => {
                if let Some(doc) = &mut self.options.document {
                    doc.toggle_solo(index);
                }
            }
            RackHit::Add => {
                // Nothing to add yet if no preset is chosen; the browser is
                // where that happens, so say so rather than doing nothing.
                self.add_channel_from_browser();
            }
            RackHit::Nothing => {}
        }
        self.tree.invalidate(RACK);
    }

    fn press_browser(&mut self, x: f32, y: f32) {
        match browser_hit(&self.browser, x, y) {
            BrowserHit::Search => {
                self.searching = true;
            }
            BrowserHit::File(index) => {
                if let Some(doc) = &mut self.options.document
                    && let Err(e) = doc.open_file(index)
                {
                    self.status = e;
                }
                self.preset_scroll = 0;
            }
            BrowserHit::Preset(index) => {
                // A preset click puts it on the **selected** channel, which is
                // what "try this sound on this part" means. The add button —
                // and Ctrl+click — make a new one instead.
                let modifiers = self.modifiers;
                if let Some(doc) = &mut self.options.document {
                    let result = if modifiers.control_key() {
                        doc.add_channel_with(index)
                    } else {
                        doc.set_channel_instrument(index)
                    };
                    if let Err(e) = result {
                        self.status = e;
                    }
                }
            }
            BrowserHit::Nothing => {}
        }
        self.tree.invalidate(BROWSER);
        self.tree.invalidate(RACK);
    }

    /// The rack's add button: the selected preset onto a new channel.
    fn add_channel_from_browser(&mut self) {
        let preset = self
            .browser
            .preset_rows
            .first()
            .map(|(index, _)| *index)
            .unwrap_or(0);
        if self.presets.is_empty() {
            self.status =
                "pick a soundfont below first — then its preset goes on a new channel".to_string();
            return;
        }
        if let Some(doc) = &mut self.options.document
            && let Err(e) = doc.add_channel_with(preset)
        {
            self.status = e;
        }
    }

    /// A toolbar button.
    fn activate(&mut self, control: RollControl) {
        match control {
            RollControl::Tool(tool) => self.set_tool(tool),
            RollControl::Snap => self.cycle_snap(),
            RollControl::ZoomInX => self.zoom(1.25, 1.0),
            RollControl::ZoomOutX => self.zoom(0.8, 1.0),
            RollControl::ZoomInY => self.zoom(1.0, 1.25),
            RollControl::ZoomOutY => self.zoom(1.0, 0.8),
            RollControl::Velocity => {
                self.roll.velocity_lane = !self.roll.velocity_lane;
                self.relayout_panels();
                self.tree.invalidate(PANEL);
            }
        }
    }

    /// Zoom about the middle of the grid — what a button press means, as
    /// against the wheel, which zooms about the pointer.
    fn zoom(&mut self, x: f32, y: f32) {
        let grid = self.roll_layout.grid;
        if x != 1.0 {
            zoom_x(&mut self.roll.view, grid, grid.x + grid.width / 2.0, x);
        }
        if y != 1.0 {
            zoom_y(&mut self.roll.view, grid, grid.y + grid.height / 2.0, y);
        }
        self.tree.invalidate(PANEL);
    }

    /// Clicking the roll's ruler moves the playhead there.
    ///
    /// The tick is turned into a sample by the **document**, not by arithmetic
    /// on a BPM: a song with a tempo change has no single BPM to multiply by
    /// (INVARIANT 5), and only the document holds the map.
    fn seek_to(&mut self, x: f32) {
        let tick = x_to_tick(&self.roll.view, self.roll_layout.grid, x);
        let Some(sample) = self
            .options
            .document
            .as_ref()
            .map(|doc| doc.sample_of_clip_tick(tick))
        else {
            return;
        };
        if let Some(host) = &mut self.options.host {
            host.seek(sample);
        }
        self.tick();
    }

    fn drag_roll(&mut self) {
        let Some(doc) = &self.options.document else {
            return;
        };
        let beats_per_bar = doc.beats_per_bar();
        let (x, y) = self.cursor;
        let edits = if self.roll.is_editing_velocity() {
            self.roll.drag_velocity(
                x,
                y,
                self.roll_layout.velocity,
                self.roll_layout.grid,
                doc.notes(),
            )
        } else {
            self.roll
                .drag(x, y, self.roll_layout.grid, doc.notes(), beats_per_bar)
        };
        let dragging_box = self.roll.marquee().is_some();
        self.apply_roll_edits(edits);
        if dragging_box {
            // A marquee changes nothing in the document and everything on
            // screen, so it has to dirty the panel on its own account.
            self.tree.invalidate(PANEL);
        }
    }

    /// The one place the roll's wishes become document changes.
    ///
    /// Note what is *not* here: any path from the roll to a `&mut Project`.
    /// INVARIANT 2 holds because there is nothing to hold it wrong with.
    fn apply_roll_edits(&mut self, edits: Vec<crate::canvas::RollEdit>) {
        if edits.is_empty() {
            return;
        }
        let mut added = Vec::new();
        let mut inserted = Vec::new();
        if let Some(doc) = &mut self.options.document {
            for edit in edits {
                let is_add = matches!(edit, crate::canvas::RollEdit::Add { .. });
                let ids = doc.edit(edit);
                if is_add {
                    added.extend(ids);
                } else {
                    inserted.extend(ids);
                }
            }
        }
        // The handshake that makes drawing and sizing one gesture — see this
        // module's own docs and `canvas::piano_roll`.
        if let Some(id) = added.first() {
            self.roll.note_added(*id);
        }
        if !inserted.is_empty() {
            self.roll.notes_inserted(inserted);
        }
        self.tree.invalidate(PANEL);
        self.refresh_title();
    }

    /// Sounds a key on the live path, releasing whatever was sounding before.
    fn start_audition(&mut self, key: u8) {
        let velocity = self.roll.default_velocity;
        if let Some(previous) = self.auditioning.replace(key)
            && let Some(doc) = &mut self.options.document
        {
            doc.audition_off(previous);
        }
        if let Some(doc) = &mut self.options.document {
            doc.audition_on(key, velocity);
        }
    }

    fn stop_audition(&mut self) {
        if let Some(key) = self.auditioning.take()
            && let Some(doc) = &mut self.options.document
        {
            doc.audition_off(key);
        }
    }

    /// The wheel, routed by what it is over.
    ///
    /// Over the roll: vertical scroll moves through the keys, `Shift` scrolls
    /// the song, `Ctrl` zooms time about the pointer and `Ctrl+Shift` (or
    /// `Alt`) zooms pitch. Over a list: it scrolls that list. The FL habits,
    /// and the ones a mouse can express.
    fn scroll_roll(&mut self, dx: f32, dy: f32) {
        let (x, y) = self.cursor;

        if self.layout.rack.frame.contains(x, y) {
            self.rack_scroll =
                scrolled(self.rack_scroll, -(dy.round() as i32), self.channels.len());
            self.relayout_panels();
            self.tree.invalidate(RACK);
            return;
        }
        if self.layout.browser.frame.contains(x, y) {
            let over_presets = self.browser.presets.contains(x, y);
            if over_presets {
                self.preset_scroll =
                    scrolled(self.preset_scroll, -(dy.round() as i32), self.presets.len());
            } else {
                self.file_scroll =
                    scrolled(self.file_scroll, -(dy.round() as i32), self.files.len());
            }
            self.relayout_panels();
            self.tree.invalidate(BROWSER);
            return;
        }

        if self.options.document.is_none() {
            return;
        }
        let grid = self.roll_layout.grid;
        let (ctrl, shift, alt) = (
            self.modifiers.control_key(),
            self.modifiers.shift_key(),
            self.modifiers.alt_key(),
        );

        if ctrl && shift || alt {
            zoom_y(&mut self.roll.view, grid, y.max(grid.y), 1.15_f32.powf(dy));
        } else if ctrl {
            zoom_x(&mut self.roll.view, grid, x.max(grid.x), 1.15_f32.powf(dy));
        } else if shift || dx != 0.0 {
            let by = if dx != 0.0 { dx } else { dy };
            let v = &mut self.roll.view;
            let step = (120.0 / v.pixels_per_tick.max(0.0001)) as fontelle_types::Tick;
            v.scroll_tick = (v.scroll_tick - by as fontelle_types::Tick * step).max(0);
        } else {
            let v = &mut self.roll.view;
            let rows = (dy * 3.0).round() as i32;
            v.top_key = (i32::from(v.top_key) + rows).clamp(11, 127) as u8;
        }
        self.tree.invalidate(PANEL);
    }

    // ---------------------------------------------------------- keyboard ---

    /// The subset of §16.5's keymap the gate needs. Every binding here is
    /// hard-coded, and §16.5 says all of them are remappable — the map is a
    /// later item, and one binding written down twice is one to find and move.
    fn key(&mut self, event: &winit::event::KeyEvent) {
        use winit::keyboard::{Key, NamedKey};

        let ctrl = self.modifiers.control_key();
        let shift = self.modifiers.shift_key();

        // While the search box has the keyboard, it has all of it: a typed "d"
        // is a letter in a soundfont's name, not the delete tool.
        if self.searching {
            match &event.logical_key {
                Key::Named(NamedKey::Escape) | Key::Named(NamedKey::Enter) => {
                    self.searching = false;
                }
                Key::Named(NamedKey::Backspace) => {
                    let mut query = self.query.clone();
                    query.pop();
                    self.set_query(query);
                }
                Key::Character(c) => {
                    let mut query = self.query.clone();
                    query.push_str(c);
                    self.set_query(query);
                }
                Key::Named(NamedKey::Space) => {
                    let mut query = self.query.clone();
                    query.push(' ');
                    self.set_query(query);
                }
                _ => {}
            }
            self.tree.invalidate(BROWSER);
            return;
        }

        match &event.logical_key {
            Key::Named(NamedKey::Space) => {
                if let Some(host) = &mut self.options.host {
                    let view = host.view();
                    if view.playing {
                        host.stop();
                    } else {
                        host.play();
                    }
                    self.tick();
                }
            }
            Key::Named(NamedKey::Delete) | Key::Named(NamedKey::Backspace) => {
                let edits = self.roll.delete_selection();
                self.apply_roll_edits(edits);
                if let Some(doc) = &mut self.options.document {
                    doc.end_gesture();
                }
            }
            Key::Named(NamedKey::Escape) => {
                self.roll.clear_selection();
                self.tree.invalidate(PANEL);
            }
            Key::Character(c) => {
                let c = c.to_lowercase();
                match c.as_str() {
                    "z" if ctrl && !shift => self.undo(),
                    "z" if ctrl && shift => self.redo(),
                    "y" if ctrl => self.redo(),
                    "a" if ctrl => {
                        if let Some(doc) = &self.options.document {
                            self.roll.select_all(doc.notes());
                            self.tree.invalidate(PANEL);
                        }
                    }
                    "s" if ctrl => self.save(),
                    "c" if ctrl => self.copy(),
                    "x" if ctrl => self.cut(),
                    "v" if ctrl => self.paste(),
                    "b" if ctrl => self.duplicate(),
                    "d" if ctrl => self.duplicate(),
                    "f" if ctrl => {
                        self.searching = true;
                        self.tree.invalidate(BROWSER);
                    }
                    // Tools, from the FL keymap.
                    "p" => self.set_tool(Tool::Draw),
                    "b" => self.set_tool(Tool::Paint),
                    "e" => self.set_tool(Tool::Select),
                    "d" => self.set_tool(Tool::Delete),
                    // Snap, cycled rather than given six bindings nobody would
                    // remember.
                    "s" => self.cycle_snap(),
                    "+" | "=" => self.zoom(1.25, 1.0),
                    "-" | "_" => self.zoom(0.8, 1.0),
                    _ => {}
                }
            }
            _ => {}
        }
    }

    fn set_query(&mut self, query: String) {
        if let Some(doc) = &mut self.options.document {
            doc.set_query(&query);
        }
        self.query = query;
        self.file_scroll = 0;
        self.preset_scroll = 0;
    }

    fn copy(&mut self) {
        if let Some(doc) = &self.options.document {
            let n = self.roll.copy(doc.notes());
            if n > 0 {
                self.status = format!("{n} note(s) copied");
            }
        }
    }

    fn cut(&mut self) {
        let edits = match &self.options.document {
            Some(doc) => self.roll.cut(doc.notes()),
            None => Vec::new(),
        };
        self.apply_roll_edits(edits);
        if let Some(doc) = &mut self.options.document {
            doc.end_gesture();
        }
    }

    fn paste(&mut self) {
        // At the playhead when it is over this clip, otherwise where the
        // pointer is — which is what people reach for when the song is
        // stopped somewhere else.
        let at = self
            .options
            .document
            .as_ref()
            .and_then(|doc| doc.playhead_tick(self.view.position_sample))
            .unwrap_or_else(|| x_to_tick(&self.roll.view, self.roll_layout.grid, self.cursor.0));
        let edits = self.roll.paste(at);
        self.apply_roll_edits(edits);
        if let Some(doc) = &mut self.options.document {
            doc.end_gesture();
        }
    }

    fn duplicate(&mut self) {
        let edits = match &self.options.document {
            Some(doc) => {
                let beats = doc.beats_per_bar();
                self.roll.duplicate(doc.notes(), beats)
            }
            None => Vec::new(),
        };
        self.apply_roll_edits(edits);
        if let Some(doc) = &mut self.options.document {
            doc.end_gesture();
        }
    }

    fn set_tool(&mut self, tool: Tool) {
        self.roll.tool = tool;
        self.tree.invalidate(PANEL);
    }

    fn cycle_snap(&mut self) {
        self.roll.view.snap = self.roll.view.snap.next();
        self.tree.invalidate(PANEL);
    }

    fn undo(&mut self) {
        if let Some(doc) = &mut self.options.document {
            doc.undo();
            self.roll.clear_selection();
        }
        self.tree.invalidate(PANEL);
        self.refresh_title();
    }

    fn redo(&mut self) {
        if let Some(doc) = &mut self.options.document {
            doc.redo();
            self.roll.clear_selection();
        }
        self.tree.invalidate(PANEL);
        self.refresh_title();
    }

    fn save(&mut self) {
        let Some(doc) = &mut self.options.document else {
            return;
        };
        match doc.save() {
            Ok(()) => {
                println!("Fontelle: saved");
                self.status = "saved".to_string();
            }
            Err(e) => {
                eprintln!("Fontelle: could not save — {e}");
                self.status = format!("could not save — {e}");
            }
        }
        self.tree.invalidate(BROWSER);
        self.refresh_title();
    }

    /// Keeps the dirty marker in the OS title bar honest (§17, item 10's
    /// smallest useful half).
    fn refresh_title(&mut self) {
        let Some(doc) = &self.options.document else {
            return;
        };
        let title = format!(
            "{}{} — Fontelle",
            doc.name(),
            if doc.is_dirty() { " •" } else { "" }
        );
        if let Some(live) = &self.live {
            live.window.set_title(&title);
        }
    }

    /// Asks for a frame only when there is one to draw.
    ///
    /// The counterpart to `Redraw::take_dirty` returning `None`: between them,
    /// a window with nothing happening in it neither requests nor issues a
    /// frame (§16.3).
    fn request_redraw_if_dirty(&mut self) {
        if !self.tree.has_dirty_regions() {
            return;
        }
        if let Some(live) = &self.live {
            live.window.request_redraw();
        }
    }

    /// Decides when the loop is allowed to wake next.
    ///
    /// The policy itself is [`sleep_budget`], which is tested; this is the
    /// three lines that hand it to winit, plus the `--run-for` deadline, which
    /// only ever brings the wake-up *forward*.
    ///
    /// Note that even the animating case is `WaitUntil` and not `Poll`: the
    /// thread sleeps between frames rather than spinning.
    fn arm_deadline(&mut self, event_loop: &ActiveEventLoop) {
        let now = std::time::Instant::now();

        let mut wake = match sleep_budget(
            self.tree.redraw().is_animating(),
            self.options.host.is_some(),
        ) {
            Sleep::Forever => None,
            Sleep::AtMost(budget) => Some(now + budget),
        };

        if let Some(limit) = self.options.run_for {
            if self.expired() {
                event_loop.exit();
                return;
            }
            let deadline = now + (limit - self.started.elapsed());
            wake = Some(wake.map_or(deadline, |w| w.min(deadline)));
        }

        event_loop.set_control_flow(match wake {
            Some(at) => ControlFlow::WaitUntil(at),
            None => ControlFlow::Wait,
        });
    }
}

/// The window `fontelle` opens when it is run with no arguments.
pub fn run() -> Result<WindowApp, WindowError> {
    run_window(WindowOptions::default())
}
