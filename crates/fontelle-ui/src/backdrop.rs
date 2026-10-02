//! Moving backdrops: a theme's WGSL shader drawn behind its panels (hub card
//! 0366).
//!
//! Ported from Floptle's `floptle-theme/src/backdrop.rs`, and the contract a
//! shader is written against is kept **identical** to it — the same
//! function, the same uniform fields at the same offsets, the same helpers —
//! so a shader written for one program runs in the other. Fontelle's own
//! two numbers, [`PRELUDE`]'s `beat` and `level`, come after every shared
//! field.
//!
//! # Writing one
//!
//! ```wgsl
//! fn backdrop(uv: vec2<f32>, px: vec2<f32>) -> vec4<f32> {
//!     // uv: 0–1 across the whole window. px: the same, in points.
//!     let wave = 0.5 + 0.5 * sin(uv.x * 8.0 + bd.time);
//!     return vec4<f32>(mix(bd.color2.rgb, bd.color0.rgb, wave * 0.3), 1.0);
//! }
//! ```
//!
//! It returns an sRGB colour with **straight** alpha. `docs/themes.md` lists
//! every field it can read.
//!
//! # Why it costs the audio nothing, and the window little
//!
//! Everything here runs on the window's own thread, in its render path, and
//! reads only what the window has already polled (the transport's position,
//! the master meter). Nothing here is reachable from `fontelle-engine`,
//! which does not depend on this crate.
//!
//! A shader is not drawn per panel. Each distinct shader layer is drawn into
//! **one** window-sized texture, at a fraction of the window's resolution
//! ([`Motion::scale`], half by default), only when the window's loop says a
//! frame is due ([`backdrop_wake`]: at most [`Motion::fps`], and never while
//! the window is unfocused, occluded or minimised), and only where a section
//! showing it is on screen. The texture is handed to vello as an ordinary
//! image ([`vello::Renderer::register_texture`]), which every section shows
//! its part of — so `draw_window` stays a pure function of what it is given.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use vello::peniko::ImageData;
use vello::wgpu;

use crate::layout::Rect;
use crate::theme::{BackdropPanel, Palette, ShaderLayer, Theme};

macro_rules! shaders {
    ($($name:literal),* $(,)?) => {
        /// The shaders Fontelle ships, named in a theme as `builtin:<name>`.
        /// Floptle's seven first, by the same names.
        pub const BUILTIN_SHADERS: &[(&str, &str)] = &[
            $(($name, include_str!(concat!("../shaders/", $name, ".wgsl"))),)*
        ];
    };
}
shaders!(
    "galaxy",
    "aurora",
    "grid",
    "scanlines",
    "drift",
    "waves",
    "starfield",
    // Fontelle's own, for its built-in looks.
    "stage",
    "stillwater",
    "sunroom",
    "controlroom",
    "rain",
    "neon",
    "phosphor",
    "bubblegum",
    "embers",
    "midnight",
    "paper",
);

/// `builtin:<name>`'s source.
pub fn builtin_source(name: &str) -> Option<&'static str> {
    let n = name.strip_prefix("builtin:")?;
    BUILTIN_SHADERS.iter().find(|s| s.0 == n).map(|s| s.1)
}

/// A shader layer's WGSL: the built-in it names, or the text it carries.
/// `None` for a `builtin:` name this build does not have.
pub fn source_of(shader: &str) -> Option<std::borrow::Cow<'_, str>> {
    if shader.starts_with("builtin:") {
        return builtin_source(shader).map(std::borrow::Cow::Borrowed);
    }
    Some(std::borrow::Cow::Borrowed(shader))
}

/// What every backdrop shader is given. Floptle's block, then Fontelle's —
/// kept in step with [`uniform_bytes`].
pub const PRELUDE: &str = r#"
struct Backdrop {
    // The texture being drawn, in pixels.
    resolution: vec2<f32>,
    // The window, in points.
    window: vec2<f32>,
    // Seconds, times the layer's speed. Holds still when effects are paused.
    time: f32,
    // The layer's `scale`.
    scale: f32,
    // Pixels per point of the texture (window scale times backdrop scale).
    density: f32,
    _pad0: f32,
    // The layer's colours: by default accent, playhead, window, text.
    color0: vec4<f32>,
    color1: vec4<f32>,
    color2: vec4<f32>,
    color3: vec4<f32>,
    // The layer's eight `params`.
    params0: vec4<f32>,
    params1: vec4<f32>,
    // The pointer, 0-1 across the window; z is 1 while it is over the window.
    pointer: vec4<f32>,
    // Fontelle's own, after every field Floptle has.
    // The song's position in beats while it plays; otherwise a free 120 BPM
    // clock. fract(bd.beat) is where in the beat it is.
    beat: f32,
    // The master meter, 0-1.
    level: f32,
    _pad1: f32,
    _pad2: f32,
};
@group(0) @binding(0) var<uniform> bd: Backdrop;
@group(0) @binding(1) var bd_image: texture_2d<f32>;
@group(0) @binding(2) var bd_sampler: sampler;

fn bd_hash(p: vec2<f32>) -> f32 {
    var p3 = fract(vec3<f32>(p.x, p.y, p.x) * 0.1031);
    p3 = p3 + dot(p3, p3.yzx + 33.33);
    return fract((p3.x + p3.y) * p3.z);
}
fn bd_noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    // Quintic: no visible creases along the grid.
    let u = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    let a = bd_hash(i);
    let b = bd_hash(i + vec2<f32>(1.0, 0.0));
    let c = bd_hash(i + vec2<f32>(0.0, 1.0));
    let d = bd_hash(i + vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}
fn bd_fbm(p0: vec2<f32>, octaves: i32) -> f32 {
    // Each octave turned against the last, so no two share the grid's axes.
    let m = mat2x2<f32>(0.8, -0.6, 0.6, 0.8);
    var p = p0;
    var v = 0.0;
    var a = 0.5;
    for (var i = 0; i < min(octaves, 8); i = i + 1) {
        v = v + a * bd_noise(p);
        p = m * p * 2.02 + vec2<f32>(17.1, 9.2);
        a = a * 0.5;
    }
    return v;
}
fn bd_rot(a: f32) -> mat2x2<f32> {
    let c = cos(a);
    let s = sin(a);
    return mat2x2<f32>(c, -s, s, c);
}
"#;

/// The entry points around the theme's `backdrop`. Unlike Floptle's, the
/// output stays **straight** alpha: vello composites images that way, where
/// egui wanted it premultiplied.
const MAIN: &str = r#"
struct BdOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};
@vertex
fn bd_vs(@builtin(vertex_index) i: u32) -> BdOut {
    let x = f32((i << 1u) & 2u);
    let y = f32(i & 2u);
    var o: BdOut;
    o.pos = vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
    o.uv = vec2<f32>(x, y);
    return o;
}
@fragment
fn bd_fs(i: BdOut) -> @location(0) vec4<f32> {
    let uv = i.pos.xy / bd.resolution;
    let c = backdrop(uv, uv * bd.window);
    return vec4<f32>(clamp(c.rgb, vec3<f32>(0.0), vec3<f32>(1.0)), clamp(c.a, 0.0, 1.0));
}
"#;

/// The full WGSL for a backdrop function.
pub fn full_source(body: &str) -> String {
    format!("{PRELUDE}\n{body}\n{MAIN}")
}

/// Checks a backdrop compiles, with naga, before wgpu ever sees it: wgpu's
/// own answer to a bad shader is a panic on another thread. The error is
/// naga's, pointing at the line.
pub fn validate(body: &str) -> Result<(), String> {
    // Asked first: without it, naga's answer is about the entry point that
    // calls it, which is not a line the author wrote.
    if !body.contains("fn backdrop") {
        return Err(NO_BACKDROP.into());
    }
    let src = full_source(body);
    let module = naga::front::wgsl::parse_str(&src).map_err(|e| e.emit_to_string(&src))?;
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::empty(),
    )
    .validate(&module)
    .map_err(|e| e.emit_to_string(&src))?;
    let has_backdrop = module
        .functions
        .iter()
        .any(|(_, f)| f.name.as_deref() == Some("backdrop"));
    if !has_backdrop {
        return Err(NO_BACKDROP.into());
    }
    Ok(())
}

const NO_BACKDROP: &str =
    "the shader has no `fn backdrop(uv: vec2<f32>, px: vec2<f32>) -> vec4<f32>`";

// ------------------------------------------------- how much may move ---

/// How much a backdrop may move: Settings ▸ Appearance ▸ Effects.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Effects {
    /// As the theme says.
    #[default]
    Moving,
    /// Each shader drawn once and held: no per-frame cost.
    Still,
    /// Colours only: no shader and no picture behind any panel.
    Off,
}

impl Effects {
    pub const ALL: [Self; 3] = [Self::Moving, Self::Still, Self::Off];

    pub fn label(self) -> &'static str {
        match self {
            Self::Moving => "Moving",
            Self::Still => "Still",
            Self::Off => "Off",
        }
    }
}

/// The window's half of the settings: how much, how often, how sharp.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Motion {
    pub effects: Effects,
    /// The most frames a second a moving backdrop asks for. 5–60.
    pub fps: f32,
    /// The backdrop's resolution against the window's, 0.25–1.
    pub scale: f32,
    /// Hold every backdrop still while the transport plays. Floptle's
    /// `pause_while_playing`; off here, where the music is the point.
    pub hold_while_playing: bool,
    /// Keep moving while another window has the focus. On by default — Ty:
    /// *"me personally i like having lots of moving windows so even if im
    /// not focused i want it animating and looking cool in the background"*
    /// — and a switch for whoever wants the focused window alone to move.
    pub when_unfocused: bool,
}

impl Default for Motion {
    fn default() -> Self {
        Self {
            effects: Effects::Moving,
            fps: DEFAULT_FPS,
            scale: DEFAULT_SCALE,
            hold_while_playing: false,
            when_unfocused: true,
        }
    }
}

pub const DEFAULT_FPS: f32 = 30.0;
pub const DEFAULT_SCALE: f32 = 0.5;

/// Whether the window can be seen: what decides whether a moving backdrop
/// may hold the loop awake.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowSight {
    pub focused: bool,
    pub occluded: bool,
    pub minimized: bool,
}

/// When the window's loop should next wake to move the backdrops, or `None`
/// for never.
///
/// The loop is `ControlFlow::Wait` and §16.3 forbids a frame nobody asked
/// for, so this is the **only** thing that keeps a moving theme moving, and
/// the cap on its rate. Minimised or covered, Effects at Still or Off, or
/// the switch held while playing: nothing is asked for, and the window
/// sleeps.
///
/// **Focus is asked only when the person said so** ([`Motion::when_unfocused`],
/// on by default). Hub card 0366 first paused an unfocused window,
/// as Floptle's Hub does; Ty, watching it: *"im not seeing any moving
/// animations ... please make sure these can be fully animated themes"*. A
/// studio is looked at while a plugin's window or another program has the
/// focus, and Floptle's editor keeps moving then too.
pub fn backdrop_wake(
    motion: &Motion,
    animated: bool,
    sight: WindowSight,
    playing: bool,
) -> Option<Duration> {
    let looking = !sight.occluded && !sight.minimized && (sight.focused || motion.when_unfocused);
    let held = motion.hold_while_playing && playing;
    (animated && looking && !held && motion.effects == Effects::Moving)
        .then(|| Duration::from_secs_f32(1.0 / motion.fps.clamp(5.0, 60.0)))
}

/// The animation clock: it advances only while backdrops are moving, so a
/// pause and a resume pick up where they stopped rather than jumping.
#[derive(Debug, Clone, Default)]
pub struct BackdropClock {
    time: f32,
    last: Option<Instant>,
}

impl BackdropClock {
    /// Moves the clock to `now` — by the time since the last call when
    /// `moving`, by nothing otherwise — and answers it in seconds.
    pub fn advance(&mut self, now: Instant, moving: bool) -> f32 {
        // A long gap (a stall, a held frame) moves it a quarter second at
        // most, so a resume never jumps.
        let dt = self
            .last
            .map_or(0.0, |t| now.saturating_duration_since(t).as_secs_f32())
            .min(0.25);
        self.last = Some(now);
        if moving {
            // Wrapped, so a week-long session keeps f32 precision; a shader
            // sees one seam an hour.
            self.time = (self.time + dt) % 3600.0;
        }
        self.time
    }

    pub fn time(&self) -> f32 {
        self.time
    }
}

/// `bd.beat`: the song's position in beats while it plays, so a theme's
/// pulse lands with the music; otherwise a free 120 BPM clock off the
/// backdrop's own seconds, so a stopped song still breathes.
pub fn song_beat(playing: bool, song_beats: Option<f64>, seconds: f32) -> f32 {
    match (playing, song_beats) {
        (true, Some(beats)) => beats as f32,
        _ => seconds * 2.0,
    }
}

// ------------------------------------------------------------- frames ---

/// What one pass of the renderer is handed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BackdropFrame {
    /// The clock, in seconds ([`BackdropClock`]); each layer's `bd.time` is
    /// this times its speed.
    pub time: f32,
    pub pointer: [f32; 4],
    pub beat: f32,
    pub level: f32,
    /// Draw what is on screen again. A picture never drawn is drawn
    /// whatever this says; with it false, the rest are held as they are.
    pub redraw: bool,
}

/// The pictures this frame's shaders made, by [`layer_key`]: what the
/// window hands `draw_window` to show.
#[derive(Debug, Clone, Default)]
pub struct ShaderFrames(pub HashMap<u64, ImageData>);

impl ShaderFrames {
    pub fn get(&self, key: u64) -> Option<&ImageData> {
        self.0.get(&key)
    }
}

/// What a pass did.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BackdropReport {
    /// How many shader pictures were drawn.
    pub drawn: u32,
    /// Shaders that would not compile: the shader, and naga's reason.
    pub errors: Vec<(String, String)>,
}

/// The identity of a shader layer's picture: everything that changes what
/// it draws. Opacity and blend are not in it; they apply when a section
/// shows it, so two sections showing one shader at different strengths
/// share one texture.
pub fn layer_key(layer: &ShaderLayer, palette: &Palette) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    layer.shader.hash(&mut h);
    layer.speed.to_bits().hash(&mut h);
    layer.scale.to_bits().hash(&mut h);
    for c in layer.colors_for(palette) {
        c.0.hash(&mut h);
    }
    for p in layer.params8() {
        p.to_bits().hash(&mut h);
    }
    layer.image.hash(&mut h);
    h.finish()
}

/// The uniform block's bytes, in [`PRELUDE`]'s order. 160 bytes: std140
/// rounds the struct to its widest member, a `vec4`.
#[allow(clippy::too_many_arguments)]
fn uniform_bytes(
    resolution: [f32; 2],
    window: [f32; 2],
    time: f32,
    scale: f32,
    density: f32,
    colors: [[f32; 4]; 4],
    params: [f32; 8],
    pointer: [f32; 4],
    beat: f32,
    level: f32,
) -> Vec<u8> {
    let mut f: Vec<f32> = Vec::with_capacity(40);
    f.extend(resolution);
    f.extend(window);
    f.extend([time, scale, density, 0.0]);
    for c in colors {
        f.extend(c);
    }
    f.extend(params);
    f.extend(pointer);
    f.extend([beat, level, 0.0, 0.0]);
    f.iter().flat_map(|v| v.to_le_bytes()).collect()
}

const UNIFORM_SIZE: u64 = 160;

/// The texture format a backdrop is drawn in: what vello's
/// `register_texture` takes.
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

struct Pipeline {
    pipeline: wgpu::RenderPipeline,
}

struct Slot {
    view: wgpu::TextureView,
    /// vello's handle on the texture, which `draw_window` draws.
    image: ImageData,
    size: [u32; 2],
    uniforms: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    drawn: bool,
    last_used: Instant,
}

/// Draws a theme's shader layers into textures vello shows. One per window
/// (and one in [`crate::render::Headless`]); the window calls
/// [`BackdropRenderer::prepare`] before it builds the frame's scene.
pub struct BackdropRenderer {
    layout: wgpu::BindGroupLayout,
    pipeline_layout: wgpu::PipelineLayout,
    sampler: wgpu::Sampler,
    white: wgpu::TextureView,
    pipelines: HashMap<u64, Result<Arc<Pipeline>, String>>,
    images: HashMap<u64, wgpu::TextureView>,
    slots: HashMap<u64, Slot>,
    frames: ShaderFrames,
}

impl BackdropRenderer {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let entry = |binding, ty| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty,
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("fontelle backdrop"),
            entries: &[
                entry(
                    0,
                    wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                ),
                entry(
                    1,
                    wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                ),
                entry(
                    2,
                    wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                ),
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("fontelle backdrop"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("fontelle backdrop"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let white = upload(device, queue, 1, 1, &[255, 255, 255, 255]);
        Self {
            layout,
            pipeline_layout,
            sampler,
            white,
            pipelines: HashMap::new(),
            images: HashMap::new(),
            slots: HashMap::new(),
            frames: ShaderFrames::default(),
        }
    }

    /// The pictures drawn so far, for `draw_window`.
    pub fn frames(&self) -> &ShaderFrames {
        &self.frames
    }

    fn pipeline(&mut self, device: &wgpu::Device, body: &str) -> Result<Arc<Pipeline>, String> {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        body.hash(&mut h);
        let key = h.finish();
        if let Some(p) = self.pipelines.get(&key) {
            return p.clone();
        }
        let made = validate(body).and_then(|()| {
            // naga said yes; a backend that still says no is caught here
            // rather than by wgpu's uncaptured-error handler, which panics.
            let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("fontelle backdrop"),
                source: wgpu::ShaderSource::Wgsl(full_source(body).into()),
            });
            let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("fontelle backdrop"),
                layout: Some(&self.pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("bd_vs"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some("bd_fs"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: FORMAT,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            });
            match wait_polling(device, scope.pop()) {
                Some(Some(e)) => Err(e.to_string()),
                // No error, or no answer in time: a backend that has not
                // said no has said yes.
                _ => Ok(Arc::new(Pipeline { pipeline })),
            }
        });
        self.pipelines.insert(key, made.clone());
        made
    }

    fn image_view(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        image: Option<&String>,
    ) -> wgpu::TextureView {
        let Some(image) = image else {
            return self.white.clone();
        };
        let mut h = std::collections::hash_map::DefaultHasher::new();
        image.hash(&mut h);
        let key = h.finish();
        if let Some(view) = self.images.get(&key) {
            return view.clone();
        }
        use base64::Engine as _;
        let view = base64::engine::general_purpose::STANDARD
            .decode(image)
            .ok()
            .and_then(|bytes| crate::theme::decode_image(&bytes).ok())
            .map(|i| upload(device, queue, i.width, i.height, i.data.data()))
            .unwrap_or_else(|| self.white.clone());
        self.images.insert(key, view.clone());
        view
    }

    /// Draws the theme's shader pictures that `uses` shows — each section
    /// on screen and where it is, in points — into their textures, and
    /// keeps [`frames`](Self::frames) for the scene. `size` is the window in
    /// pixels and `ppp` its pixels per point; `scale` is
    /// [`Motion::scale`].
    #[allow(clippy::too_many_arguments)]
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        vello: &mut vello::Renderer,
        theme: &Theme,
        uses: &[(BackdropPanel, Rect)],
        size: [u32; 2],
        ppp: f32,
        scale: f32,
        frame: &BackdropFrame,
    ) -> BackdropReport {
        let now = Instant::now();
        let mut report = BackdropReport::default();
        let scale = scale.clamp(0.25, 1.0);
        let max = device.limits().max_texture_dimension_2d.max(1);
        let tex_size = [
            ((size[0] as f32 * scale).round() as u32).clamp(1, max),
            ((size[1] as f32 * scale).round() as u32).clamp(1, max),
        ];
        let ppp = ppp.max(0.01);
        let window = [size[0] as f32 / ppp, size[1] as f32 / ppp];
        // Texture pixels per point, both ways: the texture may have been
        // clamped by the device's limit.
        let k = [
            tex_size[0] as f32 / window[0].max(1.0),
            tex_size[1] as f32 / window[1].max(1.0),
        ];

        // Which layers are wanted, and the region of each on screen.
        let mut wanted: Vec<(u64, &ShaderLayer, Rect)> = Vec::new();
        let mut in_theme: Vec<u64> = Vec::new();
        for (panel, layer) in theme.backdrops.shaders() {
            let key = layer_key(layer, &theme.palette);
            in_theme.push(key);
            for (used, rect) in uses {
                if *used != panel || rect.is_empty() || layer.opacity <= 0.0 {
                    continue;
                }
                match wanted.iter_mut().find(|(k, _, _)| *k == key) {
                    Some((_, _, region)) => *region = union(*region, *rect),
                    None => wanted.push((key, layer, *rect)),
                }
            }
        }
        // A theme taken off takes its pictures with it at once; a section
        // scrolled away keeps its picture two seconds, for when it is back.
        let stale: Vec<u64> = self
            .slots
            .iter()
            .filter(|(key, slot)| {
                !in_theme.contains(key)
                    || now.duration_since(slot.last_used) > Duration::from_secs(2)
            })
            .map(|(key, _)| *key)
            .collect();
        for key in stale {
            if let Some(slot) = self.slots.remove(&key) {
                vello.unregister_texture(slot.image);
            }
        }

        let mut encoder: Option<wgpu::CommandEncoder> = None;
        let mut dirty: Vec<u64> = Vec::new();
        for (key, layer, region) in wanted {
            let Some(body) = source_of(&layer.shader) else {
                report.errors.push((
                    layer.shader.clone(),
                    format!("there is no built-in shader called {}", layer.shader),
                ));
                continue;
            };
            let pipe = match self.pipeline(device, &body) {
                Ok(p) => p,
                Err(e) => {
                    report.errors.push((layer.shader.clone(), e));
                    continue;
                }
            };
            if self.slots.get(&key).is_none_or(|s| s.size != tex_size) {
                if let Some(old) = self.slots.remove(&key) {
                    vello.unregister_texture(old.image);
                }
                let img = self.image_view(device, queue, layer.image.as_ref());
                let slot = self.slot(device, vello, tex_size, &img, now);
                self.slots.insert(key, slot);
            }
            let slot = self.slots.get_mut(&key).expect("just made");
            slot.last_used = now;
            if slot.drawn && !frame.redraw {
                continue;
            }
            let colors = layer
                .colors_for(&theme.palette)
                .map(|c| c.0.map(|v| v as f32 / 255.0));
            queue.write_buffer(
                &slot.uniforms,
                0,
                &uniform_bytes(
                    [tex_size[0] as f32, tex_size[1] as f32],
                    window,
                    frame.time * layer.speed,
                    layer.scale,
                    ppp * scale,
                    colors,
                    layer.params8(),
                    frame.pointer,
                    frame.beat,
                    frame.level,
                ),
            );
            // Only where a section shows it — rounded outward, a pixel of
            // margin for the bilinear tap. A first draw is whole: it may be
            // held as it is for as long as the theme is worn.
            let (x0, y0, x1, y1) = if slot.drawn {
                let clampx = |v: f32| (v as i64).clamp(0, tex_size[0] as i64) as u32;
                let clampy = |v: f32| (v as i64).clamp(0, tex_size[1] as i64) as u32;
                (
                    clampx((region.x * k[0]).floor() - 1.0),
                    clampy((region.y * k[1]).floor() - 1.0),
                    clampx((region.right() * k[0]).ceil() + 1.0),
                    clampy((region.bottom() * k[1]).ceil() + 1.0),
                )
            } else {
                (0, 0, tex_size[0], tex_size[1])
            };
            if x1 <= x0 || y1 <= y0 {
                continue;
            }
            let enc = encoder.get_or_insert_with(|| {
                device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("fontelle backdrop"),
                })
            });
            {
                let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("fontelle backdrop"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &slot.view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: if slot.drawn {
                                wgpu::LoadOp::Load
                            } else {
                                wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)
                            },
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_pipeline(&pipe.pipeline);
                pass.set_bind_group(0, &slot.bind_group, &[]);
                pass.set_scissor_rect(x0, y0, x1 - x0, y1 - y0);
                pass.draw(0..3, 0..1);
            }
            slot.drawn = true;
            dirty.push(key);
            report.drawn += 1;
        }
        if let Some(enc) = encoder {
            queue.submit([enc.finish()]);
        }
        // vello copies a registered texture into its atlas only when told
        // it changed.
        for key in dirty {
            if let Some(slot) = self.slots.get(&key) {
                vello.mark_override_image_dirty(&slot.image);
            }
        }
        self.frames = ShaderFrames(
            self.slots
                .iter()
                .filter(|(_, s)| s.drawn)
                .map(|(k, s)| (*k, s.image.clone()))
                .collect(),
        );
        report
    }

    fn slot(
        &self,
        device: &wgpu::Device,
        vello: &mut vello::Renderer,
        size: [u32; 2],
        image: &wgpu::TextureView,
        now: Instant,
    ) -> Slot {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("fontelle backdrop"),
            size: wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            // COPY_SRC: vello copies it into its image atlas.
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("fontelle backdrop"),
            size: UNIFORM_SIZE,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("fontelle backdrop"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniforms.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(image),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        Slot {
            view,
            image: vello.register_texture(texture),
            size,
            uniforms,
            bind_group,
            drawn: false,
            last_used: now,
        }
    }
}

fn union(a: Rect, b: Rect) -> Rect {
    let x = a.x.min(b.x);
    let y = a.y.min(b.y);
    Rect::new(
        x,
        y,
        a.right().max(b.right()) - x,
        a.bottom().max(b.bottom()) - y,
    )
}

fn upload(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    w: u32,
    h: u32,
    rgba: &[u8],
) -> wgpu::TextureView {
    let size = wgpu::Extent3d {
        width: w,
        height: h,
        depth_or_array_layers: 1,
    };
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("fontelle backdrop image"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &tex,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        rgba,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4 * w),
            rows_per_image: Some(h),
        },
        size,
    );
    tex.create_view(&Default::default())
}

/// Waits for a wgpu future, polling the device while it is pending.
///
/// Floptle's lesson: **never block on one of these** — it resolves only
/// once the device is polled, and blocking hung a window after its first
/// frame. Bounded, so a driver that never answers costs a moment.
fn wait_polling<F: std::future::Future>(device: &wgpu::Device, f: F) -> Option<F::Output> {
    use std::task::{Context, Poll, Waker};
    let mut cx = Context::from_waker(Waker::noop());
    let mut f = std::pin::pin!(f);
    let start = Instant::now();
    loop {
        if let Poll::Ready(v) = f.as_mut().poll(&mut cx) {
            return Some(v);
        }
        if start.elapsed() > Duration::from_millis(500) {
            return None;
        }
        let _ = device.poll(wgpu::PollType::Poll);
        std::thread::yield_now();
    }
}
