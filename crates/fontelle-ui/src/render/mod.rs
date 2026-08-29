//! The `wgpu` device and the scene that gets drawn onto it (TDD §16.2).
//!
//! Two things live here, and the split is deliberate:
//!
//! - [`draw_window`] is a **pure function of the theme, the layout and the
//!   text**: it puts shapes into a `vello::Scene` and touches no GPU state. So
//!   it can be rendered and inspected without a window, which is what
//!   `tests/render_headless.rs` does.
//! - [`GpuContext`] and [`Headless`] are the two places a scene turns into
//!   pixels — onto a window's surface, and into memory.
//!
//! `vello` is the vector path (§16.2). If it proves troublesome the documented
//! fallback is direct `wgpu` pipelines with `lyon`; keeping the scene building
//! separate from the device is also what would make that swap a rewrite of one
//! function rather than of the crate.

use vello::kurbo::{Affine, BezPath, Rect as KRect, RoundedRect, RoundedRectRadii, Stroke};
use vello::peniko::Fill;
use vello::util::RenderContext;
use vello::wgpu;
use vello::{AaConfig, RenderParams, Renderer, RendererOptions, Scene};

use fontelle_model::{Arena, Note};
use fontelle_types::{NoteId, PPQN, Tick};

use crate::canvas::{RollLayout, RollView, snap_unit, tick_to_x, visible_keys, visible_ticks};
use crate::layout::{Rect, WindowLayout};
use crate::text::TextLayout;
use crate::theme::{Color, Theme};
use crate::transport::{
    Meter, TransportBarLayout, TransportHit, TransportView, meter_fill, playhead_x,
};

/// Everything the window draws that had to be shaped or measured first.
///
/// Text shaping needs a mutable `FontSystem`, and [`draw_window`] is a pure
/// function of its inputs — so whatever needs shaping is shaped by the caller
/// and handed over already positioned. That split is what keeps the whole
/// picture testable off a GPU and off a window.
pub struct Chrome<'a> {
    pub panel_title: &'a TextLayout,
    pub transport: TransportChrome<'a>,
    /// The panel's contents. `None` draws an empty panel — which is what a
    /// window with no clip open shows.
    pub roll: Option<RollChrome<'a>>,
}

/// Everything the piano roll draws from. All of it is read-only: the roll is a
/// view and never a mutator (INVARIANT 2).
pub struct RollChrome<'a> {
    pub layout: RollLayout,
    pub view: RollView,
    pub notes: &'a Arena<NoteId, Note>,
    pub selection: &'a [NoteId],
    /// Where the playhead is *within the clip*, or `None` when the transport
    /// is somewhere this clip does not cover.
    pub playhead_tick: Option<Tick>,
    pub beats_per_bar: u32,
}

pub struct TransportChrome<'a> {
    pub layout: TransportBarLayout,
    pub view: TransportView,
    pub meters: [Meter; 2],
    /// The position read-out, already shaped.
    pub readout: &'a TextLayout,
    /// What the pointer is over, so the control under it can light up.
    pub hover: Option<TransportHit>,
}

/// Builds the whole window picture.
///
/// Everything the window looks like is decided here, from data. That is what
/// makes "the theme is ignored" and "the panel is in the wrong place" testable
/// failures rather than things somebody has to notice.
pub fn draw_window(scene: &mut Scene, theme: &Theme, layout: &WindowLayout, chrome: &Chrome<'_>) {
    scene.reset();

    let p = &theme.palette;
    let m = &theme.metrics;

    // The ground. Also painted by `RenderParams::base_color`, but painting it
    // here too is what keeps this function the whole picture — a partial
    // redraw clips to a dirty region and never gets a fresh base.
    fill_rect(scene, layout.window, p.window);

    draw_transport_bar(scene, theme, &chrome.transport);

    if layout.panel.frame.is_empty() {
        return;
    }

    let frame = rounded(layout.panel.frame, m.corner_radius);
    scene.fill(
        Fill::NonZero,
        Affine::IDENTITY,
        p.panel.to_peniko(),
        None,
        &frame,
    );

    // The header rounds at the top only. Drawing it as a plain rectangle would
    // square off the panel's top corners; drawing it fully rounded would round
    // its bottom two into the body it sits against.
    if !layout.panel.header.is_empty() {
        let h = layout.panel.header;
        scene.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            p.panel_header.to_peniko(),
            None,
            &RoundedRect::from_rect(
                KRect::new(h.x as f64, h.y as f64, h.right() as f64, h.bottom() as f64),
                RoundedRectRadii::new(m.corner_radius as f64, m.corner_radius as f64, 0.0, 0.0),
            ),
        );
    }

    if m.border_width > 0.0 {
        scene.stroke(
            &Stroke::new(m.border_width as f64),
            Affine::IDENTITY,
            p.border.to_peniko(),
            None,
            &frame,
        );
    }

    if let Some(roll) = &chrome.roll {
        draw_piano_roll(scene, theme, roll);
    }

    draw_text(
        scene,
        chrome.panel_title,
        layout.panel.header.x + m.panel_padding,
        // Vertically centred in the header by its own measured height, so a
        // theme with a bigger font stays centred without a second number to
        // keep in sync.
        layout.panel.header.y + (layout.panel.header.height - chrome.panel_title.height) / 2.0,
        p.text,
    );
}

/// The transport bar (item 7 of `docs/first-usable-plan.md`).
///
/// Drawn whether or not there is an engine behind it — a window that changes
/// shape when the sound card goes away is worse than one that says so — but
/// everything in it is muted and the playhead is absent when `view.available`
/// is false.
pub fn draw_transport_bar(scene: &mut Scene, theme: &Theme, chrome: &TransportChrome<'_>) {
    let l = &chrome.layout;
    let view = &chrome.view;
    let p = &theme.palette;
    let m = &theme.metrics;

    if l.bar.is_empty() {
        return;
    }

    let bar = rounded(l.bar, m.corner_radius);
    scene.fill(
        Fill::NonZero,
        Affine::IDENTITY,
        p.panel_header.to_peniko(),
        None,
        &bar,
    );
    if m.border_width > 0.0 {
        scene.stroke(
            &Stroke::new(m.border_width as f64),
            Affine::IDENTITY,
            p.border.to_peniko(),
            None,
            &bar,
        );
    }

    // A control nobody can use is drawn in the muted ink, which is the same
    // signal a disabled control gives everywhere else.
    let ink = if view.available { p.text } else { p.text_muted };

    for (rect, what) in [
        (l.play, TransportHit::Play),
        (l.stop, TransportHit::Stop),
        (l.loop_toggle, TransportHit::ToggleLoop),
    ] {
        if rect.is_empty() {
            continue;
        }
        if chrome.hover == Some(what) && view.available {
            scene.fill(
                Fill::NonZero,
                Affine::IDENTITY,
                p.border.to_peniko(),
                None,
                &rounded(rect, m.corner_radius),
            );
        }
        // Lit when the thing it controls is on: play while rolling, the loop
        // button while looping. Recording takes the peak colour, because it is
        // the one transport state with a consequence on disk.
        let colour = match what {
            TransportHit::Play if view.recording => p.meter_peak,
            TransportHit::Play if view.playing => p.accent,
            TransportHit::ToggleLoop if view.looping => p.accent,
            _ => ink,
        };
        let glyph = rect.inset(rect.height * 0.3);
        match what {
            TransportHit::Play => scene.fill(
                Fill::NonZero,
                Affine::IDENTITY,
                colour.to_peniko(),
                None,
                &triangle(glyph),
            ),
            TransportHit::Stop => fill_rect(scene, glyph, colour),
            TransportHit::ToggleLoop => scene.stroke(
                &Stroke::new((m.border_width * 2.0) as f64),
                Affine::IDENTITY,
                colour.to_peniko(),
                None,
                &rounded(glyph, glyph.height / 2.0),
            ),
            TransportHit::Scrub(_) => {}
        }
    }

    draw_text(
        scene,
        chrome.readout,
        l.readout.x,
        l.readout.y + (l.readout.height - chrome.readout.height) / 2.0,
        ink,
    );

    draw_ruler(scene, theme, l.ruler, view);
    draw_meter(scene, theme, l.meter, view, &chrome.meters);
}

/// The song end to end, with the loop range shaded and the playhead on top.
fn draw_ruler(scene: &mut Scene, theme: &Theme, ruler: Rect, view: &TransportView) {
    if ruler.is_empty() {
        return;
    }
    let p = &theme.palette;
    // A groove rather than the bar's own colour, so the playhead has something
    // to travel along even at position zero.
    let track = ruler.inset(ruler.height * 0.3);
    fill_rect(scene, track, p.grid_line);

    if view.looping && view.length_samples > 0 {
        let (from, to) = view.loop_range_samples;
        let x0 = playhead_x(track, from, view.length_samples);
        let x1 = playhead_x(track, to, view.length_samples);
        fill_rect(
            scene,
            Rect::new(x0, track.y, (x1 - x0).max(0.0), track.height),
            p.selection,
        );
    }

    if !view.available {
        return;
    }

    // Two logical pixels wide and drawn over everything: the playhead is the
    // one thing in the bar you look for rather than at.
    let x = playhead_x(track, view.position_sample, view.length_samples);
    fill_rect(
        scene,
        Rect::new(x - 1.0, ruler.y, 2.0, ruler.height),
        p.playhead,
    );
}

/// One horizontal bar per channel, with the peak-hold marker over it.
fn draw_meter(
    scene: &mut Scene,
    theme: &Theme,
    meter: Rect,
    view: &TransportView,
    meters: &[Meter; 2],
) {
    if meter.is_empty() {
        return;
    }
    let p = &theme.palette;
    let box_ = meter.inset(meter.height * 0.25);
    fill_rect(scene, box_, p.grid_line);
    if !view.available || box_.is_empty() {
        return;
    }

    let gap = 1.0;
    let lane = ((box_.height - gap) / 2.0).max(0.0);
    for (index, channel) in meters.iter().enumerate() {
        let y = box_.y + index as f32 * (lane + gap);
        let fill = meter_fill(channel.level_db);
        // Green until it is nearly there, then red. The limiter means a peak
        // is not a disaster, but it is still the thing worth seeing.
        let colour = if channel.level_db >= -3.0 {
            p.meter_peak
        } else {
            p.meter
        };
        fill_rect(scene, Rect::new(box_.x, y, box_.width * fill, lane), colour);

        let hold = meter_fill(channel.hold_db);
        if hold > 0.0 {
            fill_rect(
                scene,
                Rect::new(box_.x + box_.width * hold - 1.0, y, 1.0, lane),
                p.meter_peak,
            );
        }
    }
}

/// The piano roll (TDD §16.4).
///
/// Four layers, drawn back to front: the row shading, the grid lines, the
/// notes, the playhead. §16.4 wants those on independent invalidation so a
/// moving playhead does not redirty note geometry; today they share a frame and
/// the split lives in `WidgetTree`'s bounds, which is where it will be applied
/// when the roll is big enough for it to pay.
///
/// **Only the visible window is built.** `visible_ticks` and `visible_keys`
/// bound every loop here. The note scan is still linear in the clip's note
/// count — filtering, not indexing — which is honest for the sizes this opens
/// today and is the first thing to change when it is not.
pub fn draw_piano_roll(scene: &mut Scene, theme: &Theme, chrome: &RollChrome<'_>) {
    let p = &theme.palette;
    let l = &chrome.layout;
    let v = &chrome.view;
    let grid = l.grid;

    if grid.is_empty() {
        return;
    }
    fill_rect(scene, grid, p.panel);

    let keys = visible_keys(v, grid);
    let ticks = visible_ticks(v, grid);

    // Row shading: the accidentals sit a shade back, which is what makes an
    // octave countable without a line for every one of them.
    for key in keys.clone() {
        let y = crate::canvas::key_to_y(v, grid, key as u8);
        let row = Rect::new(grid.x, y, grid.width, v.key_height);
        if is_accidental(key) {
            fill_rect(scene, row.intersection(&grid), p.row_accidental);
        }
        // A stronger line under every C.
        if key % 12 == 0 {
            fill_rect(
                scene,
                Rect::new(grid.x, y + v.key_height - 1.0, grid.width, 1.0).intersection(&grid),
                p.grid_line_strong,
            );
        }
    }

    // Vertical lines. Beats always; the snap division too when it is finer,
    // so what you are snapping to is what you can see.
    let bar = PPQN * Tick::from(chrome.beats_per_bar.max(1));
    let step = snap_unit(v.snap, chrome.beats_per_bar);
    for (unit, colour) in [
        (step, p.grid_line),
        (PPQN, p.grid_line),
        (bar, p.grid_line_strong),
    ] {
        if unit <= 0 || (unit as f32) * v.pixels_per_tick < 4.0 {
            // Lines closer together than a few pixels are a grey wash, not a
            // grid.
            continue;
        }
        let mut tick = ticks.start - ticks.start.rem_euclid(unit);
        while tick < ticks.end {
            let x = tick_to_x(v, grid, tick).floor();
            fill_rect(
                scene,
                Rect::new(x, grid.y, 1.0, grid.height).intersection(&grid),
                colour,
            );
            tick += unit;
        }
    }

    // Notes.
    for (id, note) in chrome.notes.iter() {
        let key = i32::from(note.key);
        if !keys.contains(&key) {
            continue;
        }
        if note.start + note.length < ticks.start || note.start > ticks.end {
            continue;
        }
        let x0 = tick_to_x(v, grid, note.start);
        let x1 = tick_to_x(v, grid, note.start + note.length);
        let block = Rect::new(
            x0,
            crate::canvas::key_to_y(v, grid, note.key),
            // Always at least a pixel wide: a note too short to see is still a
            // note, and one that vanishes at low zoom cannot be clicked to
            // find out why.
            (x1 - x0).max(1.0),
            v.key_height,
        )
        .intersection(&grid);
        if block.is_empty() {
            continue;
        }
        let selected = chrome.selection.contains(&id);
        let fill = if selected { p.note_selected } else { p.note };
        scene.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            fill.to_peniko(),
            None,
            &rounded(block.inset(0.5), 2.0),
        );
    }

    if let Some(tick) = chrome.playhead_tick {
        let x = tick_to_x(v, grid, tick);
        if x >= grid.x && x <= grid.right() {
            fill_rect(
                scene,
                Rect::new(x - 1.0, grid.y, 2.0, grid.height),
                p.playhead,
            );
        }
    }

    draw_keyboard(scene, theme, l, v);
    draw_ruler_strip(scene, theme, l, v, chrome.beats_per_bar);
}

/// The keyboard down the left-hand side.
///
/// Drawn the way a keyboard looks: the naturals run the full width and the
/// accidentals sit short and dark on top of them. Painting the strip dark and
/// the naturals light instead gives a ladder of pale bars with gaps, which
/// reads as neither a keyboard nor an octave.
fn draw_keyboard(scene: &mut Scene, theme: &Theme, l: &RollLayout, v: &RollView) {
    let p = &theme.palette;
    if l.keys.is_empty() {
        return;
    }
    fill_rect(scene, l.keys, p.key_white);

    for key in visible_keys(v, l.grid) {
        let y = crate::canvas::key_to_y(v, l.grid, key as u8);
        let row = Rect::new(l.keys.x, y, l.keys.width, v.key_height).intersection(&l.keys);
        if row.is_empty() {
            continue;
        }
        if is_accidental(key) {
            fill_rect(
                scene,
                Rect::new(row.x, row.y, row.width * 0.62, row.height).intersection(&l.keys),
                p.key_black,
            );
        } else {
            // A hairline between naturals, so E/F and B/C do not merge into
            // one double-height key.
            fill_rect(
                scene,
                Rect::new(row.x, row.bottom() - 1.0, row.width, 1.0).intersection(&l.keys),
                p.border,
            );
        }
        // Every C gets the accent down its edge — the only orientation the
        // roll offers until the ruler learns to write bar numbers.
        if key % 12 == 0 {
            fill_rect(
                scene,
                Rect::new(row.x, row.y, 3.0, row.height).intersection(&l.keys),
                p.accent,
            );
        }
    }
    // A border between the keyboard and the grid, so they read as two things.
    fill_rect(
        scene,
        Rect::new(l.keys.right() - 1.0, l.keys.y, 1.0, l.keys.height),
        p.border,
    );
}

/// The bar ruler across the top.
fn draw_ruler_strip(
    scene: &mut Scene,
    theme: &Theme,
    l: &RollLayout,
    v: &RollView,
    beats_per_bar: u32,
) {
    let p = &theme.palette;
    if l.ruler.is_empty() {
        return;
    }
    fill_rect(scene, l.ruler, p.panel_header);

    let bar = PPQN * Tick::from(beats_per_bar.max(1));
    let ticks = visible_ticks(v, l.grid);
    let mut tick = ticks.start - ticks.start.rem_euclid(bar);
    while tick < ticks.end {
        let x = tick_to_x(v, l.grid, tick).floor();
        if x >= l.grid.x {
            fill_rect(
                scene,
                Rect::new(x, l.ruler.y, 1.0, l.ruler.height).intersection(&l.ruler),
                p.grid_line_strong,
            );
        }
        tick += bar;
    }
    fill_rect(
        scene,
        Rect::new(l.ruler.x, l.ruler.bottom() - 1.0, l.ruler.width, 1.0),
        p.border,
    );
}

/// Whether `key` is a black note. The pattern repeats every octave and C is 0.
fn is_accidental(key: i32) -> bool {
    matches!(key.rem_euclid(12), 1 | 3 | 6 | 8 | 10)
}

/// A right-pointing triangle inscribed in `r` — the play glyph.
fn triangle(r: Rect) -> BezPath {
    let mut path = BezPath::new();
    path.move_to((r.x as f64, r.y as f64));
    path.line_to((r.right() as f64, (r.y + r.height / 2.0) as f64));
    path.line_to((r.x as f64, r.bottom() as f64));
    path.close_path();
    path
}

/// Draws a laid-out string with its top-left at `(x, y)`.
pub fn draw_text(scene: &mut Scene, text: &TextLayout, x: f32, y: f32, color: Color) {
    for run in &text.runs {
        scene
            .draw_glyphs(&run.font)
            .font_size(run.font_size)
            .brush(color.to_peniko())
            .transform(Affine::translate((x as f64, y as f64)))
            .draw(Fill::NonZero, run.glyphs.iter().copied());
    }
}

fn fill_rect(scene: &mut Scene, r: Rect, color: Color) {
    if r.is_empty() {
        return;
    }
    scene.fill(
        Fill::NonZero,
        Affine::IDENTITY,
        color.to_peniko(),
        None,
        &KRect::new(r.x as f64, r.y as f64, r.right() as f64, r.bottom() as f64),
    );
}

fn rounded(r: Rect, radius: f32) -> RoundedRect {
    // A radius bigger than half the shorter side is not a rounder rectangle,
    // it is an undefined path.
    let radius = radius.min(r.width / 2.0).min(r.height / 2.0).max(0.0);
    RoundedRect::new(
        r.x as f64,
        r.y as f64,
        r.right() as f64,
        r.bottom() as f64,
        radius as f64,
    )
}

/// The size and colour space every target here agrees on.
const TARGET_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

#[derive(Debug)]
pub enum RenderError {
    /// No GPU we can use. On a developer's machine this is a problem; in a
    /// container it is a reason to skip, which is why it is a value and not a
    /// panic.
    NoAdapter(String),
    Device(String),
    Render(String),
}

impl std::fmt::Display for RenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoAdapter(why) => write!(f, "no usable GPU adapter: {why}"),
            Self::Device(why) => write!(f, "the GPU device could not be opened: {why}"),
            Self::Render(why) => write!(f, "the frame could not be rendered: {why}"),
        }
    }
}

impl std::error::Error for RenderError {}

/// Renders scenes into memory, with no window and no surface.
///
/// This is to the window what `fontelle_app::render_offline` is to the audio
/// device: the same pipeline, driven without hardware attached, so the result
/// can be asserted on. It is the answer to the plan's §2.5 problem for the one
/// part of a GUI that genuinely is pixels.
pub struct Headless {
    context: RenderContext,
    dev_id: usize,
    renderer: Renderer,
}

impl Headless {
    pub fn new() -> Result<Self, RenderError> {
        // vello's own `RenderContext` rather than a hand-built instance: it
        // owns the wgpu version this crate is pinned to, and picking an adapter
        // is exactly the part worth not reimplementing.
        let mut context = RenderContext::new();
        let dev_id = block_on(context.device(None)).ok_or_else(|| {
            RenderError::NoAdapter("no adapter matched the default options".to_string())
        })?;

        let renderer = Renderer::new(
            &context.devices[dev_id].device,
            RendererOptions {
                use_cpu: false,
                antialiasing_support: vello::AaSupport::area_only(),
                num_init_threads: None,
                pipeline_cache: None,
            },
        )
        .map_err(|e| RenderError::Render(e.to_string()))?;

        Ok(Self {
            context,
            dev_id,
            renderer,
        })
    }

    /// Renders `scene` and returns tightly packed RGBA8, row-major from the
    /// top-left.
    pub fn render(
        &mut self,
        scene: &Scene,
        width: u32,
        height: u32,
        base: Color,
    ) -> Result<Vec<u8>, RenderError> {
        let device = &self.context.devices[self.dev_id].device;
        let queue = &self.context.devices[self.dev_id].queue;

        let size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("fontelle-ui headless target"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: TARGET_FORMAT,
            usage: wgpu::TextureUsages::STORAGE_BINDING
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = target.create_view(&wgpu::TextureViewDescriptor::default());

        self.renderer
            .render_to_texture(
                device,
                queue,
                scene,
                &view,
                &RenderParams {
                    base_color: base.to_peniko(),
                    width,
                    height,
                    antialiasing_method: AaConfig::Area,
                },
            )
            .map_err(|e| RenderError::Render(e.to_string()))?;

        // Copies out of a texture want rows aligned to 256 bytes, so the
        // staging buffer is padded and the padding is dropped on the way out.
        let unpadded = width as usize * 4;
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as usize;
        let padded = unpadded.div_ceil(align) * align;

        let staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("fontelle-ui headless readback"),
            size: (padded * height as usize) as u64,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("fontelle-ui headless copy"),
        });
        encoder.copy_texture_to_buffer(
            target.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &staging,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded as u32),
                    rows_per_image: None,
                },
            },
            size,
        );
        queue.submit([encoder.finish()]);

        let slice = staging.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|e| RenderError::Render(e.to_string()))?;

        let mapped = slice.get_mapped_range();
        let mut pixels = Vec::with_capacity(unpadded * height as usize);
        for row in 0..height as usize {
            pixels.extend_from_slice(&mapped[row * padded..row * padded + unpadded]);
        }
        drop(mapped);
        staging.unmap();

        Ok(pixels)
    }
}

/// Runs a future to completion on this thread.
///
/// `wgpu`'s setup calls are the only futures this crate has, and they resolve
/// on the first poll on native targets. A parking waker rather than a spin so
/// this is correct even where they do not, and hand-rolled rather than adding
/// an async runtime for three call sites — the same trade `fontelle_model`'s
/// hand-rolled date formatting makes.
pub(crate) fn block_on<F: std::future::Future>(future: F) -> F::Output {
    use std::sync::Arc;
    use std::task::{Context, Poll, Wake, Waker};

    struct Unpark(std::thread::Thread);
    impl Wake for Unpark {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
        fn wake_by_ref(self: &Arc<Self>) {
            self.0.unpark();
        }
    }

    let waker = Waker::from(Arc::new(Unpark(std::thread::current())));
    let mut cx = Context::from_waker(&waker);
    // Safe: `future` lives on this stack frame for the whole loop and is never
    // moved after the first poll.
    let mut future = std::pin::pin!(future);
    loop {
        match future.as_mut().poll(&mut cx) {
            Poll::Ready(value) => return value,
            Poll::Pending => std::thread::park(),
        }
    }
}
