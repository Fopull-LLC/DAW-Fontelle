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
use vello::peniko::{BlendMode, Fill};
use vello::util::RenderContext;
use vello::wgpu;
use vello::{AaConfig, RenderParams, Renderer, RendererOptions, Scene};

use fontelle_model::{Arena, Note};
use fontelle_types::{NoteId, PPQN, Tick};

use crate::canvas::{
    BrowserHit, BrowserLayout, RackLayout, RollControl, RollLayout, RollView, SnapDivision, Tool,
    ToolbarLayout, snap_unit, tick_to_x, velocity_to_y, visible_keys, visible_ticks,
};
use crate::document::{ChannelInfo, LibraryEntry};
use crate::layout::{PanelLayout, Rect, WindowLayout};
use crate::text::{Labels, TextLayout};
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
    /// The channel rack down the left. `None` when there is no studio behind
    /// the window at all.
    pub rack: Option<RackChrome<'a>>,
    pub browser: Option<BrowserChrome<'a>>,
    /// The browser panel's heading, carrying the count of what is in the bank —
    /// so "how many soundfonts have I got" needs no line of its own.
    pub browser_title: &'a str,
    /// Everything that had to be shaped: bar numbers, key names, channel and
    /// soundfont names, the toolbar's captions. Looked up by the string being
    /// drawn, because that is the only key both sides can agree on without the
    /// pure half of the renderer owning a font system.
    pub labels: &'a Labels,
    /// One line along the bottom of the browser: what went wrong, or where the
    /// soundfonts are meant to go.
    pub status: &'a str,
}

/// The channel rack's contents.
pub struct RackChrome<'a> {
    pub panel: PanelLayout,
    pub layout: RackLayout,
    pub channels: &'a [ChannelInfo],
    pub selected: usize,
}

/// The soundfont browser's contents (TDD §17.5).
pub struct BrowserChrome<'a> {
    pub panel: PanelLayout,
    pub layout: BrowserLayout,
    pub query: &'a str,
    pub files: &'a [LibraryEntry],
    pub presets: &'a [LibraryEntry],
    pub selected_file: Option<usize>,
    /// Whether the search box has the keyboard, so the caret is drawn.
    pub searching: bool,
    /// What the pointer is over, so a button can light up.
    pub hover: Option<BrowserHit>,
}

/// Everything the piano roll draws from. All of it is read-only: the roll is a
/// view and never a mutator (INVARIANT 2).
pub struct RollChrome<'a> {
    pub layout: RollLayout,
    pub toolbar: ToolbarLayout,
    pub view: RollView,
    pub notes: &'a Arena<NoteId, Note>,
    pub selection: &'a [NoteId],
    /// Where the playhead is *within the clip*, or `None` when the transport
    /// is somewhere this clip does not cover.
    pub playhead_tick: Option<Tick>,
    pub beats_per_bar: u32,
    /// What the mouse is doing, so the toolbar can light the tool that is on.
    pub tool: Tool,
    pub snap: SnapDivision,
    /// The selection box being dragged, if one is.
    pub marquee: Option<Rect>,
    pub hover: Option<RollControl>,
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

    if let Some(rack) = &chrome.rack {
        draw_panel_frame(scene, theme, &rack.panel);
        draw_label(
            scene,
            chrome.labels,
            "Channels",
            rack.panel.header,
            m,
            p.text,
        );
        draw_rack(scene, theme, chrome.labels, rack);
    }
    if let Some(browser) = &chrome.browser {
        draw_panel_frame(scene, theme, &browser.panel);
        draw_label(
            scene,
            chrome.labels,
            chrome.browser_title,
            browser.panel.header,
            m,
            p.text,
        );
        draw_browser(scene, theme, chrome.labels, browser, chrome.status);
    }

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
        draw_piano_roll(scene, theme, chrome.labels, roll);
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
pub fn draw_piano_roll(scene: &mut Scene, theme: &Theme, labels: &Labels, chrome: &RollChrome<'_>) {
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

    // The selection box, over the notes and under nothing.
    if let Some(box_) = chrome.marquee {
        let box_ = box_.intersection(&grid);
        if !box_.is_empty() {
            fill_rect(scene, box_, p.selection);
            scene.stroke(
                &Stroke::new(1.0),
                Affine::IDENTITY,
                p.accent.to_peniko(),
                None,
                &KRect::new(
                    box_.x as f64,
                    box_.y as f64,
                    box_.right() as f64,
                    box_.bottom() as f64,
                ),
            );
        }
    }

    draw_keyboard(scene, theme, labels, l, v);
    draw_ruler_strip(scene, theme, labels, l, v, chrome.beats_per_bar);
    draw_velocity_lane(scene, theme, labels, chrome);
    draw_roll_toolbar(scene, theme, labels, chrome);
}

/// The velocity lane under the grid (§16.5's first note property lane).
///
/// One bar per note, at the note's own x, so a phrase's dynamics are read
/// straight down from the notes that make them.
fn draw_velocity_lane(scene: &mut Scene, theme: &Theme, labels: &Labels, chrome: &RollChrome<'_>) {
    let p = &theme.palette;
    let lane = chrome.layout.velocity;
    if lane.is_empty() {
        return;
    }
    let v = &chrome.view;
    fill_rect(scene, chrome.layout.velocity_keys, p.panel_header);
    fill_rect(scene, lane, p.row_accidental);
    // A line at the top of the lane, so it reads as its own strip rather than
    // as more grid.
    fill_rect(
        scene,
        Rect::new(
            chrome.layout.velocity_keys.x,
            lane.y,
            chrome.layout.frame.width,
            1.0,
        ),
        p.border,
    );
    if let Some(text) = labels.get("vel") {
        draw_text(
            scene,
            text,
            chrome.layout.velocity_keys.x + 4.0,
            chrome.layout.velocity_keys.y + 4.0,
            p.text_muted,
        );
    }
    // Half-way, so an eye can tell 100 from 60 without counting pixels.
    fill_rect(
        scene,
        Rect::new(lane.x, lane.y + lane.height / 2.0, lane.width, 1.0),
        p.grid_line,
    );

    let ticks = visible_ticks(v, chrome.layout.grid);
    for (id, note) in chrome.notes.iter() {
        if note.start + note.length < ticks.start || note.start > ticks.end {
            continue;
        }
        let x = tick_to_x(v, chrome.layout.grid, note.start);
        let top = velocity_to_y(lane, note.velocity);
        let bar = Rect::new(x, top, 3.0, lane.bottom() - top).intersection(&lane);
        if bar.is_empty() {
            continue;
        }
        let selected = chrome.selection.contains(&id);
        fill_rect(scene, bar, if selected { p.note_selected } else { p.note });
    }
}

/// The toolbar: the tools, the snap chip, the zooms.
///
/// The answer to "I cannot see what this can do". Every one of them has a
/// keyboard shortcut too, and the shortcut is what a person ends up using —
/// but only after they have found out it exists.
fn draw_roll_toolbar(scene: &mut Scene, theme: &Theme, labels: &Labels, chrome: &RollChrome<'_>) {
    let p = &theme.palette;
    let m = &theme.metrics;
    if chrome.layout.toolbar.is_empty() {
        return;
    }
    fill_rect(scene, chrome.layout.toolbar, p.panel_header);

    for (control, rect) in &chrome.toolbar.items {
        if rect.is_empty() {
            continue;
        }
        let on = match control {
            RollControl::Tool(tool) => *tool == chrome.tool,
            RollControl::Velocity => !chrome.layout.velocity.is_empty(),
            _ => false,
        };
        if on || chrome.hover == Some(*control) {
            fill_rect_rounded(
                scene,
                *rect,
                m.corner_radius,
                if on { p.accent } else { p.border },
            );
        }
        // The snap chip says which division is live rather than the word
        // "snap": the state is the useful half.
        let caption = match control {
            RollControl::Snap => chrome.snap.label(),
            other => other.label(),
        };
        let Some(text) = labels.get(caption) else {
            continue;
        };
        let ink = if on { p.panel } else { p.text };
        draw_text_clipped(
            scene,
            text,
            *rect,
            rect.x + ((rect.width - text.width) / 2.0).max(1.0),
            rect.y + (rect.height - text.height) / 2.0,
            ink,
        );
    }
}

// --------------------------------------------------------- the sidebar ---

/// A panel's ground, header strip and border — the three things every docked
/// panel has and none of them is worth writing twice.
fn draw_panel_frame(scene: &mut Scene, theme: &Theme, panel: &PanelLayout) {
    let p = &theme.palette;
    let m = &theme.metrics;
    if panel.frame.is_empty() {
        return;
    }
    let frame = rounded(panel.frame, m.corner_radius);
    scene.fill(
        Fill::NonZero,
        Affine::IDENTITY,
        p.panel.to_peniko(),
        None,
        &frame,
    );
    if !panel.header.is_empty() {
        let h = panel.header;
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
}

/// A panel header's own caption.
fn draw_label(
    scene: &mut Scene,
    labels: &Labels,
    caption: &str,
    header: Rect,
    m: &crate::theme::Metrics,
    ink: Color,
) {
    let Some(text) = labels.get(caption) else {
        return;
    };
    draw_text_clipped(
        scene,
        text,
        header,
        header.x + m.panel_padding,
        header.y + (header.height - text.height) / 2.0,
        ink,
    );
}

fn draw_rack(scene: &mut Scene, theme: &Theme, labels: &Labels, chrome: &RackChrome<'_>) {
    let p = &theme.palette;
    let l = &chrome.layout;

    for row in &l.rows {
        let Some(channel) = chrome.channels.get(row.index) else {
            continue;
        };
        if row.index == chrome.selected {
            fill_rect(scene, row.frame, p.selection);
        }
        // A channel with no soundfont on it is drawn in the muted ink: a
        // channel that cannot make a sound and looks like one that can is the
        // most confusing thing a rack can do.
        let ink = if channel.has_instrument {
            p.text
        } else {
            p.text_muted
        };
        if let Some(text) = labels.get(&channel.name) {
            draw_text_clipped(
                scene,
                text,
                row.name,
                row.name.x + 6.0,
                row.name.y + (row.name.height - text.height) / 2.0,
                if channel.muted { p.text_muted } else { ink },
            );
        }
        for (rect, on, caption, colour) in [
            (row.solo, channel.soloed, "S", p.accent),
            (row.mute, channel.muted, "M", p.meter_peak),
        ] {
            fill_rect_rounded(scene, rect, 2.0, if on { colour } else { p.border });
            if let Some(text) = labels.get(caption) {
                draw_text_clipped(
                    scene,
                    text,
                    rect,
                    rect.x + ((rect.width - text.width) / 2.0).max(0.0),
                    rect.y + (rect.height - text.height) / 2.0,
                    if on { p.panel } else { p.text_muted },
                );
            }
        }
    }

    // The add button, always at the bottom.
    fill_rect_rounded(scene, l.add, theme.metrics.corner_radius, p.accent);
    if let Some(text) = labels.get(ADD_CHANNEL) {
        draw_text_clipped(
            scene,
            text,
            l.add,
            l.add.x + ((l.add.width - text.width) / 2.0).max(0.0),
            l.add.y + (l.add.height - text.height) / 2.0,
            p.panel,
        );
    }
}

/// The caption on the rack's add button, in one place so the window can shape
/// exactly the string the renderer will look for.
pub const ADD_CHANNEL: &str = "+ Add instrument";

fn draw_browser(
    scene: &mut Scene,
    theme: &Theme,
    labels: &Labels,
    chrome: &BrowserChrome<'_>,
    status: &str,
) {
    let p = &theme.palette;
    let l = &chrome.layout;

    // The search field.
    if !l.search.is_empty() {
        fill_rect_rounded(scene, l.search, 3.0, p.window);
        if chrome.searching {
            scene.stroke(
                &Stroke::new(1.0),
                Affine::IDENTITY,
                p.accent.to_peniko(),
                None,
                &rounded(l.search, 3.0),
            );
        }
        let shown = if chrome.query.is_empty() {
            SEARCH_HINT
        } else {
            chrome.query
        };
        if let Some(text) = labels.get(shown) {
            draw_text_clipped(
                scene,
                text,
                l.search,
                l.search.x + 6.0,
                l.search.y + (l.search.height - text.height) / 2.0,
                if chrome.query.is_empty() {
                    p.text_muted
                } else {
                    p.text
                },
            );
            if chrome.searching && !chrome.query.is_empty() {
                fill_rect(
                    scene,
                    Rect::new(
                        l.search.x + 7.0 + text.width,
                        l.search.y + 3.0,
                        1.0,
                        (l.search.height - 6.0).max(0.0),
                    )
                    .intersection(&l.search),
                    p.accent,
                );
            }
        }
    }

    let mut list = |area: Rect, rows: &[(usize, Rect)], entries: &[LibraryEntry], selected| {
        if area.is_empty() {
            return;
        }
        fill_rect(scene, area, p.window);
        for (index, rect) in rows {
            let Some(entry) = entries.get(*index) else {
                continue;
            };
            if Some(*index) == selected {
                fill_rect(scene, *rect, p.selection);
            }
            let detail = labels.get(&entry.detail);
            let detail_width = detail.map_or(0.0, |d| d.width);
            if let Some(text) = labels.get(&entry.name) {
                // Clipped to the room left beside the size, not to the whole
                // row: a long soundfont name has to stop, not overprint.
                let column = name_column(rect.inset(0.0), detail_width);
                draw_text_clipped(
                    scene,
                    text,
                    column,
                    column.x + 5.0,
                    rect.y + (rect.height - text.height) / 2.0,
                    p.text,
                );
            }
            if let Some(detail) = detail {
                let x = rect.right() - detail.width - 5.0;
                draw_text_clipped(
                    scene,
                    detail,
                    *rect,
                    x,
                    rect.y + (rect.height - detail.height) / 2.0,
                    p.text_muted,
                );
            }
        }
    };
    list(l.files, &l.file_rows, chrome.files, chrome.selected_file);
    list(l.presets, &l.preset_rows, chrome.presets, None);

    // The status line: where the bank is, or what just went wrong. Its own row
    // rather than a strip drawn over the bottom of the preset list, because a
    // folder path on top of a list of presets is two things at once.
    if !status.is_empty()
        && let Some(text) = labels.get(status)
    {
        draw_text_clipped(
            scene,
            text,
            l.status,
            l.status.x + 4.0,
            l.status.y + (l.status.height - text.height) / 2.0,
            p.text_muted,
        );
    }

    // And the two things a person needs to do with a folder: look in it, and
    // change which one it is. Pinned to the bottom of the panel, because on a
    // first run the bank is empty, the lists above are empty, and these are the
    // only things worth clicking.
    for (rect, caption, what) in [
        (l.open_folder, OPEN_FOLDER, BrowserHit::OpenFolder),
        (l.choose_folder, CHOOSE_FOLDER, BrowserHit::ChooseFolder),
    ] {
        if rect.is_empty() {
            continue;
        }
        let lit = chrome.hover == Some(what);
        fill_rect_rounded(
            scene,
            rect,
            theme.metrics.corner_radius,
            if lit { p.accent } else { p.border },
        );
        if let Some(text) = labels.get(caption) {
            draw_text_clipped(
                scene,
                text,
                rect,
                rect.x + ((rect.width - text.width) / 2.0).max(2.0),
                rect.y + (rect.height - text.height) / 2.0,
                if lit { p.panel } else { p.text },
            );
        }
    }
}

/// The placeholder in the empty search box, in one place for the same reason
/// [`ADD_CHANNEL`] is.
pub const SEARCH_HINT: &str = "search soundfonts\u{2026}";

/// The browser footer's captions, in one place for the same reason.
pub const OPEN_FOLDER: &str = "Open folder";
pub const CHOOSE_FOLDER: &str = "Change\u{2026}";

/// The keyboard down the left-hand side.
///
/// Drawn the way a keyboard looks: the naturals run the full width and the
/// accidentals sit short and dark on top of them. Painting the strip dark and
/// the naturals light instead gives a ladder of pale bars with gaps, which
/// reads as neither a keyboard nor an octave.
fn draw_keyboard(scene: &mut Scene, theme: &Theme, labels: &Labels, l: &RollLayout, v: &RollView) {
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
        // Every C gets the accent down its edge, and its own name beside it —
        // "C4" is how a person says where they are on a keyboard.
        if key % 12 == 0 {
            fill_rect(
                scene,
                Rect::new(row.x, row.y, 3.0, row.height).intersection(&l.keys),
                p.accent,
            );
            // A line box is a little taller than the ink in it, so a name is
            // allowed to be marginally taller than its row before it is
            // dropped — otherwise the keyboard is never labelled at any zoom
            // anyone actually uses.
            if let Some(text) = labels.get(&key_name(key))
                && row.height >= text.height - 3.0
            {
                draw_text_clipped(
                    scene,
                    text,
                    row,
                    row.x + 6.0,
                    row.y + (row.height - text.height) / 2.0,
                    p.key_black,
                );
            }
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
    labels: &Labels,
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
    // Numbered only when the numbers would not collide — at a zoom where two
    // bars are eight pixels apart, a wall of digits is less legible than none.
    let label_every = label_stride(bar as f32 * v.pixels_per_tick);
    let mut tick = ticks.start - ticks.start.rem_euclid(bar);
    while tick < ticks.end {
        let x = tick_to_x(v, l.grid, tick).floor();
        if x >= l.grid.x {
            fill_rect(
                scene,
                Rect::new(x, l.ruler.y, 1.0, l.ruler.height).intersection(&l.ruler),
                p.grid_line_strong,
            );
            // One-based, because bar 1 is where a musician says a song starts.
            let number = tick / bar + 1;
            if let Some(stride) = label_every
                && labelled_bar(number, stride)
                && let Some(text) = labels.get(&number.to_string())
            {
                draw_text_clipped(
                    scene,
                    text,
                    l.ruler,
                    x + 3.0,
                    l.ruler.y + (l.ruler.height - text.height) / 2.0,
                    p.text_muted,
                );
            }
        }
        tick += bar;
    }
    fill_rect(
        scene,
        Rect::new(l.ruler.x, l.ruler.bottom() - 1.0, l.ruler.width, 1.0),
        p.border,
    );
}

/// How many bars apart the ruler's numbers should be, or `None` when even the
/// widest spacing would crowd them.
///
/// Middle C is key 60 and is called C4, so the octave number is `key / 12 - 1`.
pub fn key_name(key: i32) -> String {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    format!(
        "{}{}",
        NAMES[key.rem_euclid(12) as usize],
        key.div_euclid(12) - 1
    )
}

/// Whether bar `number` (one-based) gets written on the ruler at `stride`.
///
/// Its own function because the obvious spelling is wrong: `number % stride ==
/// 1` reads correctly and is `0 == 1` for a stride of one, so the common case —
/// every bar numbered — numbered none of them. Found by looking at the window.
pub fn labelled_bar(number: i64, stride: i64) -> bool {
    stride > 0 && (number - 1).rem_euclid(stride) == 0
}

/// The room a row's first column has, given how wide its second one is.
///
/// A soundfont called `Bread Breads Distortion Guitar v2.1` in a 248-pixel
/// panel is the normal case, and clipping its name to the whole row draws it
/// straight through the size at the other end.
pub fn name_column(row: Rect, detail_width: f32) -> Rect {
    Rect::new(row.x, row.y, row.width - detail_width - ROW_GAP, row.height).clamped()
}

/// Between a row's two columns.
const ROW_GAP: f32 = 10.0;

/// The bar numbers to write, given how many pixels one bar is.
///
/// About thirty pixels is the narrowest a two- or three-digit number can be
/// written in without touching the next one.
pub fn label_stride(bar_px: f32) -> Option<i64> {
    if !bar_px.is_finite() || bar_px <= 0.0 {
        return None;
    }
    [1i64, 2, 4, 8, 16, 32]
        .into_iter()
        .find(|&stride| bar_px * stride as f32 >= 30.0)
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

/// [`fill_rect`] with rounded corners — every chip, switch and button here.
fn fill_rect_rounded(scene: &mut Scene, r: Rect, radius: f32, color: Color) {
    if r.is_empty() {
        return;
    }
    scene.fill(
        Fill::NonZero,
        Affine::IDENTITY,
        color.to_peniko(),
        None,
        &rounded(r, radius),
    );
}

/// Draws text that must not run out of the rectangle it belongs to.
///
/// A soundfont called `Orchestral Strings Ensemble Legato.sf2` in a 248-pixel
/// panel is the normal case, not the edge case, and text spilling over the
/// panel next to it is the difference between a tool and a mock-up.
fn draw_text_clipped(
    scene: &mut Scene,
    text: &TextLayout,
    clip: Rect,
    x: f32,
    y: f32,
    color: Color,
) {
    if clip.is_empty() || text.is_empty() {
        return;
    }
    let needs_clip = x < clip.x || x + text.width > clip.right();
    if needs_clip {
        scene.push_layer(
            Fill::NonZero,
            BlendMode::default(),
            1.0,
            Affine::IDENTITY,
            &KRect::new(
                clip.x as f64,
                clip.y as f64,
                clip.right() as f64,
                clip.bottom() as f64,
            ),
        );
    }
    draw_text(scene, text, x, y, color);
    if needs_clip {
        scene.pop_layer();
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
