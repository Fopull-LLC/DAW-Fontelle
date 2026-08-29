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

use crate::layout::{WindowLayout, window_layout};
use crate::render::{RenderError, draw_window};
use crate::text::{TextContext, TextLayout};
use crate::theme::Theme;
use crate::widget::{WidgetId, WidgetTree};

/// The one panel item 6 opens. Item 9 turns this into the docked set.
const PANEL: WidgetId = WidgetId::new(0);

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
}

impl Default for WindowOptions {
    fn default() -> Self {
        Self {
            title: "Fontelle".to_string(),
            panel_title: "Fontelle".to_string(),
            theme: Theme::dark_default(),
            size: (1280, 720),
            run_for: None,
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
            &self.title,
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
        self.arm_deadline(event_loop);
    }
}

impl WindowApp {
    /// Keeps the `run_for` countdown honest without turning the loop into a
    /// poll: with a deadline the loop wakes once, at the deadline; without one
    /// it goes back to [`ControlFlow::Wait`] and sleeps indefinitely.
    fn arm_deadline(&mut self, event_loop: &ActiveEventLoop) {
        let Some(limit) = self.options.run_for else {
            return;
        };
        if self.expired() {
            event_loop.exit();
            return;
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(
            std::time::Instant::now() + (limit - self.started.elapsed()),
        ));
    }
}

/// The window `fontelle` opens when it is run with no arguments.
pub fn run() -> Result<WindowApp, WindowError> {
    run_window(WindowOptions::default())
}
