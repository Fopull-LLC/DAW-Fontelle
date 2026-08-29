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

use crate::canvas::{MouseButton, PianoRoll, RollLayout, SnapDivision, Tool, roll_layout};
use crate::document::DocumentHost;
use crate::layout::{WindowLayout, window_layout};
use crate::render::{Chrome, RenderError, RollChrome, TransportChrome, draw_window};
use crate::text::{TextContext, TextLayout};
use crate::theme::Theme;
use crate::transport::{
    Meter, TransportBarLayout, TransportHit, TransportHost, TransportView, apply, format_readout,
    hit, transport_bar_layout,
};
use crate::widget::{Sleep, WidgetId, WidgetTree, sleep_budget};

/// The one panel item 6 opens. Item 9 turns this into the docked set.
const PANEL: WidgetId = WidgetId::new(0);
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
    /// The document the piano roll shows and edits. `None` opens an empty
    /// panel.
    pub document: Option<Box<dyn DocumentHost>>,
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
            roll_layout: roll_layout(layout.panel.body, &options.theme.metrics),
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
        self.roll_layout = roll_layout(self.layout.panel.body, &self.options.theme.metrics);
        self.tree.insert(TRANSPORT, self.layout.transport);
        self.tree.insert(PANEL, self.layout.panel.frame);
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
        let Some(live) = &self.live else { return };
        // §16.3, the whole of it: no dirty region, no frame.
        let Some(_region) = self.tree.take_dirty() else {
            return;
        };

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
                    view: self.roll.view,
                    notes: doc.notes(),
                    selection: self.roll.selection(),
                    playhead_tick: doc.playhead_tick(self.view.position_sample),
                    beats_per_bar: doc.beats_per_bar(),
                }),
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
                self.drag_roll();
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
                if let Some(what) = hit(&self.bar, &self.view, x, y)
                    && button == winit::event::MouseButton::Left
                    && let Some(host) = &mut self.options.host
                {
                    // Commands down (TDD §2.2): one click, one call, which is
                    // a handful of relaxed stores on the other side. Nothing
                    // here waits for the audio thread to acknowledge it — the
                    // next `tick` reads back what actually happened.
                    apply(host.as_mut(), what);
                    self.tick();
                } else if let Some(button) = match button {
                    winit::event::MouseButton::Left => Some(MouseButton::Left),
                    winit::event::MouseButton::Right => Some(MouseButton::Right),
                    _ => None,
                } {
                    self.press_roll(button, x, y);
                }
                self.request_redraw_if_dirty();
            }

            WindowEvent::MouseInput {
                state: winit::event::ElementState::Released,
                ..
            } => {
                self.roll.release();
                // One drag, one undo entry (§10.6). Only the caller knows the
                // mouse came up, which is exactly why `History` cannot decide
                // this for itself.
                if let Some(doc) = &mut self.options.document {
                    doc.end_gesture();
                }
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

            WindowEvent::ModifiersChanged(state) => self.modifiers = state.state(),

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
        self.tick();
        self.request_redraw_if_dirty();
        self.arm_deadline(event_loop);
    }
}

impl WindowApp {
    /// A press that landed somewhere the transport bar did not want.
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
        self.apply_roll_edits(edits);
        // A press always changes the selection or the gesture, both of which
        // are visible.
        self.tree.invalidate(PANEL);
    }

    fn drag_roll(&mut self) {
        let Some(doc) = &self.options.document else {
            return;
        };
        let beats_per_bar = doc.beats_per_bar();
        let (x, y) = self.cursor;
        let edits = self
            .roll
            .drag(x, y, self.roll_layout.grid, doc.notes(), beats_per_bar);
        self.apply_roll_edits(edits);
    }

    /// The one place the roll's wishes become document changes.
    ///
    /// Note what is *not* here: any path from the roll to a `&mut Project`.
    /// INVARIANT 2 holds because there is nothing to hold it wrong with.
    fn apply_roll_edits(&mut self, edits: Vec<crate::canvas::RollEdit>) {
        if edits.is_empty() {
            return;
        }
        if let Some(doc) = &mut self.options.document {
            for edit in edits {
                doc.edit(edit);
            }
        }
        self.tree.invalidate(PANEL);
        self.refresh_title();
    }

    /// Vertical scroll moves through the keys; `Shift` scrolls the song;
    /// `Ctrl` zooms. The FL habits, and the ones a mouse can express.
    fn scroll_roll(&mut self, dx: f32, dy: f32) {
        if self.options.document.is_none() {
            return;
        }
        let v = &mut self.roll.view;
        if self.modifiers.control_key() {
            // Zoom about the left edge. Zooming about the pointer is better and
            // is a later refinement; this one is at least predictable.
            v.pixels_per_tick = (v.pixels_per_tick * 1.15_f32.powf(dy)).clamp(0.004, 4.0);
        } else if self.modifiers.shift_key() || dx != 0.0 {
            let by = if dx != 0.0 { dx } else { dy };
            let step = (120.0 / v.pixels_per_tick.max(0.0001)) as fontelle_types::Tick;
            v.scroll_tick = (v.scroll_tick - by as fontelle_types::Tick * step).max(0);
        } else {
            let rows = (dy * 3.0).round() as i32;
            v.top_key = (i32::from(v.top_key) + rows).clamp(11, 127) as u8;
        }
        self.tree.invalidate(PANEL);
    }

    /// The subset of §16.5's keymap the gate needs. Every binding here is
    /// hard-coded, and §16.5 says all of them are remappable — the map is a
    /// later item, and one binding written down twice is one to find and move.
    fn key(&mut self, event: &winit::event::KeyEvent) {
        use winit::keyboard::{Key, NamedKey};

        let ctrl = self.modifiers.control_key();
        let shift = self.modifiers.shift_key();

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
                    // Tools, from the FL keymap.
                    "p" if !ctrl => self.set_tool(Tool::Draw),
                    "e" if !ctrl => self.set_tool(Tool::Select),
                    "d" if !ctrl => self.set_tool(Tool::Delete),
                    // Snap, cycled rather than given four bindings nobody
                    // would remember.
                    "b" if !ctrl => self.cycle_snap(),
                    _ => {}
                }
            }
            _ => {}
        }
    }

    fn set_tool(&mut self, tool: Tool) {
        self.roll.tool = tool;
        self.tree.invalidate(PANEL);
    }

    fn cycle_snap(&mut self) {
        self.roll.view.snap = match self.roll.view.snap {
            SnapDivision::Bar => SnapDivision::Beat,
            SnapDivision::Beat => SnapDivision::Step,
            SnapDivision::Step => SnapDivision::Triplet,
            SnapDivision::Triplet => SnapDivision::None,
            _ => SnapDivision::Bar,
        };
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
            // Printed rather than shown: a message area is item 10's, and a
            // save that failed silently is the one outcome that must not
            // happen.
            Ok(()) => println!("Fontelle: saved"),
            Err(e) => eprintln!("Fontelle: could not save — {e}"),
        }
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
