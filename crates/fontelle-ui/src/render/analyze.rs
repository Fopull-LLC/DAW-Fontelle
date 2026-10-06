//! Drawing the Analyze Musically window (`docs/analyze-musically-plan.md`
//! §3.1): Flopsynth's bridge — the hull, the canopy with its tabs on the
//! glass, the consoles — with the note lane as the instrument on the canopy's
//! screen.
//!
//! Every rectangle and every string comes from `canvas/analyze.rs`, which is
//! pure and tested. What is left here is colour. No new palette tokens: the
//! confidences reuse the meter's inks (good, fair, poor) and the notes are
//! the roll's.

use vello::Scene;
use vello::kurbo::{Affine, BezPath, Circle, Rect as KRect, Stroke};
use vello::peniko::{
    BlendMode, Blob, Fill, ImageAlphaType, ImageBrush, ImageData, ImageFormat, ImageQuality,
    ImageSampler,
};

use super::{
    BridgeType, bridge, bridge_type, draw_flop_button, draw_flop_switch, draw_text_clipped,
    fill_glow, fill_rect, fill_rect_rounded, fill_rect_vertical, lighten, mix, stroke_polyline,
    stroke_rect_rounded,
};
use crate::canvas::{
    AnalyzeClarity, AnalyzeHit, AnalyzeLayout, AnalyzeMode, AnalyzePage, AnalyzeState, AnalyzeText,
    AnalyzeView, RowShade,
};
use crate::layout::Rect;
use crate::text::Labels;
use crate::theme::{Color, Theme};

/// What the window draws.
pub struct AnalyzeChrome<'a> {
    pub layout: AnalyzeLayout,
    pub view: &'a AnalyzeView,
    pub state: &'a AnalyzeState,
    /// The tip due under the pointer, and where it goes.
    pub tooltip: Option<(String, Rect)>,
    pub skin: Option<&'a crate::skin::Skin>,
}

/// The style each kind of string is drawn in, at the window's scale. One
/// function for the shaper and the renderer.
pub fn analyze_style(kind: AnalyzeText, scale: f32) -> crate::text::TextStyle {
    let t = bridge_type(scale);
    match kind {
        AnalyzeText::Heading => t.heading,
        AnalyzeText::Value => t.value,
        AnalyzeText::Caption => t.caption,
    }
}

fn darken(colour: Color, amount: f32) -> Color {
    let f = |c: u8| (f32::from(c) * (1.0 - amount)) as u8;
    Color([f(colour.0[0]), f(colour.0[1]), f(colour.0[2]), colour.0[3]])
}

/// Text at `style`, centred in `rect` (or from its left, `left` in).
#[allow(clippy::too_many_arguments)]
fn text_in(
    scene: &mut Scene,
    labels: &Labels,
    text: &str,
    style: crate::text::TextStyle,
    rect: Rect,
    left: Option<f32>,
    ink: Color,
) -> f32 {
    let Some(shaped) = labels.get_styled(text, style) else {
        return 0.0;
    };
    let x = match left {
        Some(inset) => rect.x + inset,
        None => rect.x + ((rect.width - shaped.width) / 2.0).max(1.0),
    };
    draw_text_clipped(
        scene,
        shaped,
        rect,
        x,
        rect.y + (rect.height - shaped.height) / 2.0,
        ink,
    );
    shaped.width
}

/// The lamp's ink for a confidence: the meter's green, the automation amber,
/// the meter's red.
fn clarity_ink(clarity: AnalyzeClarity, p: &crate::theme::Palette) -> Color {
    match clarity {
        AnalyzeClarity::Clear => p.meter,
        AnalyzeClarity::Usable => p.param_automated,
        AnalyzeClarity::Rough => p.meter_peak,
    }
}

pub(super) fn draw_analyze(
    scene: &mut Scene,
    theme: &Theme,
    labels: &Labels,
    chrome: &AnalyzeChrome<'_>,
) {
    let p = &theme.palette;
    let m = &theme.metrics;
    let l = &chrome.layout;
    if l.body.is_empty() {
        return;
    }
    let t = bridge_type(l.scale);

    // The bridge: the hull under everything, the canopy over the middle.
    let ground = Rect::new(
        l.body.x - m.panel_margin,
        l.body.y - m.panel_margin,
        l.body.width + m.panel_margin * 2.0,
        l.body.height + m.panel_margin * 2.0,
    );
    bridge::draw_hull(scene, theme, ground, l.canopy.bottom() + 4.0, chrome.skin);
    bridge::draw_canopy(
        scene,
        theme,
        l.canopy,
        Rect::ZERO,
        l.scale,
        None,
        chrome.skin,
    );

    draw_header(scene, theme, labels, chrome, &t);
    draw_glass_controls(scene, theme, labels, chrome, &t);

    if chrome.state.page == AnalyzePage::Notes {
        draw_lane(scene, theme, labels, chrome);
    } else {
        bridge::draw_screen(scene, theme, l.lane.screen, p.accent);
        draw_later(scene, theme, labels, chrome, &t);
    }

    draw_cards(scene, theme, labels, chrome, &t);
    if !l.job.is_empty() {
        draw_job(scene, theme, labels, chrome, &t);
    }
    if !l.popover.is_empty() {
        draw_popover(scene, theme, labels, chrome, &t);
    }
    if let Some((caption, rect)) = &chrome.tooltip
        && !rect.is_empty()
        && let Some(text) = labels.get(caption)
    {
        fill_rect_rounded(scene, *rect, m.corner_radius, p.border);
        fill_rect_rounded(scene, rect.inset(1.0), m.corner_radius, p.panel_header);
        draw_text_clipped(
            scene,
            text,
            *rect,
            rect.x + crate::tooltip::TOOLTIP_PAD,
            rect.y + (rect.height - text.height) / 2.0,
            p.text,
        );
    }
}

// ------------------------------------------------------------- header ---

fn draw_header(
    scene: &mut Scene,
    theme: &Theme,
    labels: &Labels,
    chrome: &AnalyzeChrome<'_>,
    t: &BridgeType,
) {
    let p = &theme.palette;
    let l = &chrome.layout;
    let view = chrome.view;
    let hover = chrome.state.hover;
    let s = l.scale;

    // The extraction badge: a lamp in its confidence's ink, then the words.
    plate(scene, theme, l.badge, hover == Some(AnalyzeHit::Badge));
    let lamp_ink = view
        .clarity
        .map_or(p.text_muted, |(clarity, _)| clarity_ink(clarity, p));
    let lamp = (l.badge.x + 12.0 * s, l.badge.y + l.badge.height / 2.0);
    fill_glow(scene, lamp, 10.0 * s, lamp_ink, 0x70);
    scene.fill(
        Fill::NonZero,
        Affine::IDENTITY,
        lamp_ink.to_peniko(),
        None,
        &Circle::new((lamp.0 as f64, lamp.1 as f64), f64::from(3.6 * s)),
    );
    let words = Rect::new(
        l.badge.x + 20.0 * s,
        l.badge.y,
        l.badge.width - 20.0 * s,
        l.badge.height,
    );
    text_in(
        scene,
        labels,
        &crate::canvas::badge_text(view),
        t.value,
        words,
        None,
        p.text,
    );

    // The key: its name (a click copies it), ▾ and ⧉ joined to it.
    let key_group = Rect::new(
        l.scale_name.x,
        l.scale_name.y,
        l.scale_copy.right() - l.scale_name.x,
        l.scale_name.height,
    );
    let key_hot = matches!(
        hover,
        Some(AnalyzeHit::ScaleName | AnalyzeHit::ScaleMenu | AnalyzeHit::ScaleCopy)
    );
    plate(scene, theme, key_group, key_hot);
    if hover == Some(AnalyzeHit::ScaleName) {
        fill_rect_rounded(
            scene,
            l.scale_name.inset(2.0),
            3.0,
            p.accent.with_alpha(0x30),
        );
    }
    let key_ink = if view.key.is_some() {
        lighten(p.accent, 0.35)
    } else {
        p.text_muted
    };
    text_in(
        scene,
        labels,
        &crate::canvas::scale_chip_text(view),
        t.value,
        l.scale_name,
        None,
        key_ink,
    );
    for divider in [l.scale_menu.x, l.scale_copy.x] {
        fill_rect(
            scene,
            Rect::new(divider, key_group.y + 5.0, 1.0, key_group.height - 10.0),
            p.border,
        );
    }
    let icon_ink = |hit: AnalyzeHit| {
        if hover == Some(hit) {
            lighten(p.accent, 0.4)
        } else {
            p.text_muted
        }
    };
    chevron(scene, l.scale_menu, icon_ink(AnalyzeHit::ScaleMenu), s);
    copy_icon(scene, l.scale_copy, icon_ink(AnalyzeHit::ScaleCopy), s);

    // Tuning, and the tempo when there is one: read-outs, quieter.
    plate(scene, theme, l.tuning, hover == Some(AnalyzeHit::Tuning));
    text_in(
        scene,
        labels,
        &crate::canvas::tuning_text(view),
        t.value,
        l.tuning,
        None,
        p.text_muted,
    );
    if let Some(bpm) = crate::canvas::bpm_text(view) {
        plate(scene, theme, l.bpm, hover == Some(AnalyzeHit::Bpm));
        text_in(scene, labels, &bpm, t.value, l.bpm, None, p.text_muted);
    }

    // Melody | Chords: one segmented chip, the mode in force lit.
    let mode = chrome.state.effective_mode(view);
    let both = Rect::new(
        l.mode_melody.x,
        l.mode_melody.y,
        l.mode_chords.right() - l.mode_melody.x,
        l.mode_melody.height,
    );
    plate(scene, theme, both, false);
    for (rect, this) in [
        (l.mode_melody, AnalyzeMode::Melody),
        (l.mode_chords, AnalyzeMode::Chords),
    ] {
        let lit = mode == this;
        if lit {
            fill_rect_rounded(scene, rect.inset(2.0), 3.0, p.accent.with_alpha(0x55));
            stroke_rect_rounded(scene, rect.inset(2.0), 3.0, 1.0, lighten(p.accent, 0.2));
        } else if hover == Some(AnalyzeHit::Mode(this)) {
            fill_rect_rounded(scene, rect.inset(2.0), 3.0, p.text.with_alpha(0x14));
        }
        text_in(
            scene,
            labels,
            this.label(),
            t.value,
            rect,
            None,
            if lit {
                lighten(p.text, 0.2)
            } else {
                p.text_muted
            },
        );
    }
}

/// A chip's plate on the hull: Flopsynth's button ground.
fn plate(scene: &mut Scene, theme: &Theme, rect: Rect, hot: bool) {
    let p = &theme.palette;
    let m = &theme.metrics;
    if rect.is_empty() {
        return;
    }
    fill_rect_vertical(
        scene,
        rect,
        m.corner_radius,
        lighten(p.panel_header, if hot { 0.12 } else { 0.06 }),
        mix(p.panel_header, p.window, 0.4),
    );
    stroke_rect_rounded(
        scene,
        rect,
        m.corner_radius,
        1.0,
        if hot { p.accent } else { p.border },
    );
}

/// ▾, drawn rather than set in type.
fn chevron(scene: &mut Scene, rect: Rect, ink: Color, s: f32) {
    let (cx, cy) = (rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
    let w = 4.0 * s;
    let mut path = BezPath::new();
    path.move_to((f64::from(cx - w), f64::from(cy - w * 0.5)));
    path.line_to((f64::from(cx), f64::from(cy + w * 0.5)));
    path.line_to((f64::from(cx + w), f64::from(cy - w * 0.5)));
    scene.stroke(
        &Stroke::new(f64::from(1.6 * s)),
        Affine::IDENTITY,
        ink.to_peniko(),
        None,
        &path,
    );
}

/// ⧉: two sheets, one over the other.
fn copy_icon(scene: &mut Scene, rect: Rect, ink: Color, s: f32) {
    let (cx, cy) = (rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
    let size = 7.0 * s;
    let back = Rect::new(cx - size * 0.75, cy - size * 0.75, size, size);
    let front = Rect::new(cx - size * 0.25, cy - size * 0.25, size, size);
    stroke_rect_rounded(scene, back, 1.5, 1.2 * s, ink.with_alpha(0xa0));
    fill_rect_rounded(scene, front, 1.5, darken(ink, 0.75).with_alpha(0xff));
    stroke_rect_rounded(scene, front, 1.5, 1.2 * s, ink);
}

// ------------------------------------------------------ on the glass ---

fn draw_glass_controls(
    scene: &mut Scene,
    theme: &Theme,
    labels: &Labels,
    chrome: &AnalyzeChrome<'_>,
    t: &BridgeType,
) {
    let l = &chrome.layout;
    let state = chrome.state;
    for (page, rect) in &l.tabs {
        bridge::draw_hud_tab(
            scene,
            theme,
            labels,
            *rect,
            page.label(),
            *page == state.page,
            t.heading,
        );
    }
    let wave = if state.spectrogram {
        crate::canvas::PITCH_TOGGLE
    } else {
        crate::canvas::WAVE_TOGGLE
    };
    for (rect, word, on) in [
        (l.wave_toggle, wave, false),
        (l.chords_toggle, crate::canvas::CHORDS_TOGGLE, state.chords),
        (
            l.scale_toggle,
            crate::canvas::SCALE_TOGGLE,
            state.show_scale,
        ),
    ] {
        bridge::draw_hud_tab(scene, theme, labels, rect, word, on, t.value);
    }
    bridge::draw_hud_tab(
        scene,
        theme,
        labels,
        l.window_scale,
        &crate::canvas::window_scale_label(state.scale),
        false,
        t.value,
    );
}

// ------------------------------------------------------------- the lane ---

fn draw_lane(scene: &mut Scene, theme: &Theme, labels: &Labels, chrome: &AnalyzeChrome<'_>) {
    let p = &theme.palette;
    let l = &chrome.layout;
    let lane = &l.lane;
    let view = chrome.view;
    let state = chrome.state;
    let t = bridge_type(l.scale);
    bridge::draw_screen(scene, theme, lane.screen, p.note);
    if lane.grid.is_empty() {
        return;
    }
    let grid = lane.grid;
    let clip = |scene: &mut Scene, rect: Rect| {
        scene.push_layer(
            Fill::NonZero,
            BlendMode::default(),
            1.0,
            Affine::IDENTITY,
            &KRect::new(
                f64::from(rect.x),
                f64::from(rect.y),
                f64::from(rect.right()),
                f64::from(rect.bottom()),
            ),
        );
    };

    // The rows: out-of-scale dimmed and the root marked, as the roll does;
    // a hairline between rows and a firmer one under every C.
    clip(scene, grid);
    for key in lane.visible_keys(state) {
        let row = lane.row(state, key);
        match crate::canvas::analyze_row_shade(view, state, key) {
            RowShade::OutOfScale => fill_rect(scene, row, p.row_out_of_scale),
            RowShade::Root => fill_rect(scene, row, p.row_scale_root),
            RowShade::Accidental => fill_rect(scene, row, p.row_accidental),
            _ => {}
        }
        let line = if key % 12 == 0 {
            p.text.with_alpha(0x26)
        } else {
            p.text.with_alpha(0x0c)
        };
        fill_rect(
            scene,
            Rect::new(grid.x, row.bottom() - 0.5, grid.width, 1.0),
            line,
        );
    }
    // Time: a faint rule at each of the ruler's steps.
    for (x, _) in crate::canvas::ruler_labels(l, view, state) {
        fill_rect(
            scene,
            Rect::new(x.round(), grid.y, 1.0, grid.height),
            p.text.with_alpha(0x10),
        );
    }

    // What the notes were heard in, behind them.
    if state.spectrogram {
        draw_spectrogram(scene, theme, chrome);
    } else {
        draw_waveform(scene, theme, chrome);
    }
    // The end of the audio: past it is nothing.
    let end = lane.x_of(state, view.duration);
    if end < grid.right() {
        fill_rect(
            scene,
            Rect::new(end, grid.y, grid.right() - end, grid.height),
            darken(p.window, 0.5).with_alpha(0x90),
        );
    }

    // The notes.
    let notes = state.notes(view);
    for (index, note) in notes.iter().enumerate() {
        let Some(blob) = l.blob(view, state, index) else {
            continue;
        };
        let selected = state.is_selected(index);
        let hot = state.hover == Some(AnalyzeHit::Note(index));
        let base = if selected { p.note_selected } else { p.note };
        let sure = note.confidence.clamp(0.0, 1.0);
        let radius = (blob.height / 2.0).min(6.0);
        if selected || hot {
            fill_glow(
                scene,
                (blob.x + blob.width / 2.0, blob.y + blob.height / 2.0),
                (blob.width * 0.6).min(60.0),
                base,
                0x30,
            );
        }
        if note.poly {
            // A chord note: a thin outline over a light fill (§3.3).
            fill_rect_rounded(
                scene,
                blob,
                radius,
                base.with_alpha((0x30 as f32 + 0x50 as f32 * sure) as u8),
            );
            stroke_rect_rounded(
                scene,
                blob,
                radius,
                1.2,
                base.with_alpha((0x90 as f32 + 0x6f as f32 * sure) as u8),
            );
        } else {
            fill_rect_vertical(
                scene,
                blob,
                radius,
                lighten(base, 0.18).with_alpha((0x60 as f32 + 0x9f as f32 * sure) as u8),
                base.with_alpha((0x50 as f32 + 0x9f as f32 * sure) as u8),
            );
        }
        if sure < 0.4 {
            hatch(scene, blob, p.window.with_alpha(0x60));
        }
        if selected {
            stroke_rect_rounded(scene, blob, radius, 1.5, lighten(p.note_selected, 0.5));
        } else if hot {
            stroke_rect_rounded(scene, blob, radius, 1.0, lighten(base, 0.5));
        }
        // The pitch through it.
        if note.curve.len() >= 2 {
            let points: Vec<(f32, f32)> = note
                .curve
                .iter()
                .map(|(time, cents)| {
                    (
                        lane.x_of(state, *time),
                        lane.y_of(state, f32::from(note.midi) + cents / 100.0),
                    )
                })
                .collect();
            stroke_polyline(
                scene,
                &points,
                grid,
                1.5,
                if selected {
                    p.window.with_alpha(0xc0)
                } else {
                    lighten(base, 0.65).with_alpha(0xe0)
                },
            );
        }
        // How far off it sits, when it is enough to hear.
        if let Some(tag) = crate::canvas::analyze_cents_tag(note)
            && let Some(text) = labels.get_styled(&tag, t.caption)
        {
            let x = (blob.right() - text.width).max(blob.x);
            let y = blob.y - text.height - 1.0;
            let ink = if note.cents > 0.0 {
                p.param_automated
            } else {
                lighten(p.accent, 0.3)
            };
            draw_text_clipped(scene, text, grid, x, y.max(grid.y), ink);
        }
    }

    // The marquee.
    if let Some(area) = state.marquee() {
        fill_rect(scene, area, p.accent.with_alpha(0x22));
        stroke_rect_rounded(scene, area, 0.0, 1.0, p.accent.with_alpha(0xc0));
    }

    // Nothing to show yet, or ever: say so in the middle of the lane.
    let said = if let Some(error) = &view.error {
        Some(error.clone())
    } else if notes.is_empty() && view.analysing.is_some() {
        Some("Listening\u{2026}".to_string())
    } else {
        None
    };
    if let Some(said) = said {
        text_in(scene, labels, &said, t.value, grid, None, p.text_muted);
    }
    scene.pop_layer();

    draw_keys(scene, theme, labels, chrome);
    draw_chord_lane(scene, theme, labels, chrome);
    draw_ruler(scene, theme, labels, chrome);
}

/// Diagonal hatching inside a doubtful note (confidence under 0.4).
fn hatch(scene: &mut Scene, rect: Rect, ink: Color) {
    scene.push_layer(
        Fill::NonZero,
        BlendMode::default(),
        1.0,
        Affine::IDENTITY,
        &KRect::new(
            f64::from(rect.x),
            f64::from(rect.y),
            f64::from(rect.right()),
            f64::from(rect.bottom()),
        ),
    );
    let mut x = rect.x - rect.height;
    while x < rect.right() {
        let mut path = BezPath::new();
        path.move_to((f64::from(x), f64::from(rect.bottom())));
        path.line_to((f64::from(x + rect.height), f64::from(rect.y)));
        scene.stroke(
            &Stroke::new(1.0),
            Affine::IDENTITY,
            ink.to_peniko(),
            None,
            &path,
        );
        x += 5.0;
    }
    scene.pop_layer();
}

/// The waveform: the extremes faint, across the middle of the lane.
fn draw_waveform(scene: &mut Scene, theme: &Theme, chrome: &AnalyzeChrome<'_>) {
    let p = &theme.palette;
    let lane = &chrome.layout.lane;
    let view = chrome.view;
    let state = chrome.state;
    let grid = lane.grid;
    if view.peaks.is_empty() || view.peaks_per_second <= 0.0 {
        return;
    }
    let middle = grid.y + grid.height / 2.0;
    let half = grid.height * 0.42;
    let ink = mix(p.accent, p.note, 0.3).with_alpha(0x22);
    let mut x = grid.x;
    while x < grid.right() {
        let from = lane.t_of(state, x);
        let to = lane.t_of(state, x + 1.0);
        let a = (from * view.peaks_per_second).floor().max(0.0) as usize;
        let b = ((to * view.peaks_per_second).ceil() as usize).max(a + 1);
        if a >= view.peaks.len() {
            break;
        }
        let (mut low, mut high) = (0.0f32, 0.0f32);
        for (lo, hi) in &view.peaks[a..b.min(view.peaks.len())] {
            low = low.min(*lo);
            high = high.max(*hi);
        }
        let top = middle - high.clamp(0.0, 1.0) * half;
        let bottom = middle - low.clamp(-1.0, 0.0) * half;
        fill_rect(scene, Rect::new(x, top, 1.0, (bottom - top).max(1.0)), ink);
        x += 1.0;
    }
}

/// The pitch picture: the contour, a third of a semitone a row, in the
/// accent warming to the playhead's ink where it is strongest.
fn draw_spectrogram(scene: &mut Scene, theme: &Theme, chrome: &AnalyzeChrome<'_>) {
    let p = &theme.palette;
    let lane = &chrome.layout.lane;
    let state = chrome.state;
    let Some(image) = &chrome.view.spectrogram else {
        return;
    };
    if image.columns == 0 || image.rows == 0 || image.columns_per_second <= 0.0 {
        return;
    }
    let grid = lane.grid;
    // The cells on screen, as an image of their own: built per frame, a few
    // tens of thousands of pixels, and drawn scaled.
    let c0 = (lane.t_of(state, grid.x) * image.columns_per_second)
        .floor()
        .max(0.0) as usize;
    let c1 = ((lane.t_of(state, grid.right()) * image.columns_per_second).ceil() as usize)
        .min(image.columns);
    let row_of = |midi: f32| (midi - image.lowest_midi) * image.rows_per_semitone;
    let r0 = row_of(lane.midi_of(state, grid.bottom())).floor().max(0.0) as usize;
    let r1 = (row_of(lane.midi_of(state, grid.y)).ceil() as usize + 1).min(image.rows);
    if c1 <= c0 || r1 <= r0 {
        return;
    }
    let (w, h) = (c1 - c0, r1 - r0);
    let cool = p.accent;
    let hot = lighten(p.playhead, 0.2);
    let mut rgba = vec![0u8; w * h * 4];
    for (ci, column) in (c0..c1).enumerate() {
        for (ri, row) in (r0..r1).enumerate() {
            let v = image.data[column * image.rows + row];
            if v < 24 {
                continue;
            }
            let f = f32::from(v) / 255.0;
            let ink = mix(cool, hot, (f - 0.5).max(0.0) * 2.0);
            // Top row first in the image: high pitch at the top.
            let at = ((h - 1 - ri) * w + ci) * 4;
            rgba[at] = ink.0[0];
            rgba[at + 1] = ink.0[1];
            rgba[at + 2] = ink.0[2];
            rgba[at + 3] = (f * 0.85 * 255.0) as u8;
        }
    }
    let data = ImageData {
        data: Blob::new(std::sync::Arc::new(rgba)),
        format: ImageFormat::Rgba8,
        alpha_type: ImageAlphaType::Alpha,
        width: w as u32,
        height: h as u32,
    };
    // Where the image's corners land: columns at their times, rows at their
    // pitches (a row is centred on its pitch).
    let x0 = lane.x_of(state, c0 as f64 / image.columns_per_second);
    let x1 = lane.x_of(state, c1 as f64 / image.columns_per_second);
    let top_midi = image.lowest_midi + (r1 as f32 - 0.5) / image.rows_per_semitone;
    let bottom_midi = image.lowest_midi + (r0 as f32 - 0.5) / image.rows_per_semitone;
    let y0 = lane.y_of(state, top_midi);
    let y1 = lane.y_of(state, bottom_midi);
    scene.draw_image(
        &ImageBrush {
            image: data,
            sampler: ImageSampler::new().with_quality(ImageQuality::Medium),
        },
        Affine::translate((f64::from(x0), f64::from(y0)))
            * Affine::scale_non_uniform(
                f64::from((x1 - x0) / w as f32),
                f64::from((y1 - y0) / h as f32),
            ),
    );
}

/// The mini keyboard down the left: the roll's keys, small, every C named.
fn draw_keys(scene: &mut Scene, theme: &Theme, labels: &Labels, chrome: &AnalyzeChrome<'_>) {
    let p = &theme.palette;
    let lane = &chrome.layout.lane;
    let state = chrome.state;
    let t = bridge_type(chrome.layout.scale);
    let keys = lane.keys;
    if keys.is_empty() {
        return;
    }
    scene.push_layer(
        Fill::NonZero,
        BlendMode::default(),
        1.0,
        Affine::IDENTITY,
        &KRect::new(
            f64::from(keys.x),
            f64::from(keys.y),
            f64::from(keys.right()),
            f64::from(keys.bottom()),
        ),
    );
    fill_rect(scene, keys, darken(p.window, 0.3));
    for key in lane.visible_keys(state) {
        let row = lane.row(state, key);
        let band = Rect::new(keys.x, row.y, keys.width, row.height);
        let black = matches!(key % 12, 1 | 3 | 6 | 8 | 10);
        let hot = state.hover == Some(AnalyzeHit::Key(key));
        let face = if black {
            mix(p.key_black, p.window, 0.3)
        } else {
            mix(p.key_white, p.panel_header, 0.7)
        };
        let face = if hot { mix(face, p.accent, 0.45) } else { face };
        let width = if black {
            keys.width * 0.62
        } else {
            keys.width - 1.0
        };
        fill_rect(
            scene,
            Rect::new(band.x, band.y + 0.5, width, (band.height - 1.0).max(1.0)),
            face,
        );
        if key % 12 == 0
            && let Some(text) = labels.get_styled(&crate::canvas::key_name(key), t.caption)
            && row.height >= text.height * 0.7
        {
            draw_text_clipped(
                scene,
                text,
                keys,
                keys.right() - text.width - 3.0,
                band.y + (band.height - text.height) / 2.0,
                darken(p.window, 0.2),
            );
        }
    }
    scene.pop_layer();
    fill_rect(
        scene,
        Rect::new(keys.right() - 1.0, keys.y, 1.0, keys.height),
        p.border,
    );
}

fn draw_chord_lane(scene: &mut Scene, theme: &Theme, labels: &Labels, chrome: &AnalyzeChrome<'_>) {
    let p = &theme.palette;
    let lane = &chrome.layout.lane;
    let state = chrome.state;
    let area = lane.chords;
    if area.is_empty() {
        return;
    }
    let t = bridge_type(chrome.layout.scale);
    fill_rect(scene, area, darken(p.window, 0.25).with_alpha(0xc0));
    for (index, chord) in chrome.view.chords.iter().enumerate() {
        if chord.label.is_empty() {
            continue;
        }
        let x0 = lane.x_of(state, chord.start).max(area.x);
        let x1 = lane.x_of(state, chord.end).min(area.right());
        if x1 <= x0 + 2.0 {
            continue;
        }
        let block = Rect::new(x0 + 1.0, area.y + 2.0, x1 - x0 - 2.0, area.height - 4.0);
        let hot = state.hover == Some(AnalyzeHit::Chord(index));
        fill_rect_rounded(
            scene,
            block,
            3.0,
            if hot {
                p.mod_macro.with_alpha(0x60)
            } else {
                p.mod_macro.with_alpha(0x28)
            },
        );
        fill_rect(
            scene,
            Rect::new(block.x, block.y, 2.0, block.height),
            p.mod_macro,
        );
        text_in(
            scene,
            labels,
            &chord.label,
            t.value,
            block,
            Some(6.0),
            lighten(p.text, 0.1),
        );
    }
}

fn draw_ruler(scene: &mut Scene, theme: &Theme, labels: &Labels, chrome: &AnalyzeChrome<'_>) {
    let p = &theme.palette;
    let l = &chrome.layout;
    let ruler = l.lane.ruler;
    if ruler.is_empty() {
        return;
    }
    let t = bridge_type(l.scale);
    fill_rect(scene, ruler, darken(p.window, 0.25).with_alpha(0xc0));
    fill_rect(
        scene,
        Rect::new(ruler.x, ruler.y, ruler.width, 1.0),
        p.border,
    );
    for (x, label) in crate::canvas::ruler_labels(l, chrome.view, chrome.state) {
        fill_rect(scene, Rect::new(x.round(), ruler.y, 1.0, 4.0), p.text_muted);
        if let Some(text) = labels.get_styled(&label, t.caption) {
            draw_text_clipped(
                scene,
                text,
                ruler,
                x + 3.0,
                ruler.y + (ruler.height - text.height) / 2.0 + 1.0,
                p.text_muted,
            );
        }
    }
}

// --------------------------------------------- pages not built yet ---

fn draw_later(
    scene: &mut Scene,
    theme: &Theme,
    labels: &Labels,
    chrome: &AnalyzeChrome<'_>,
    t: &BridgeType,
) {
    let p = &theme.palette;
    let l = &chrome.layout;
    let page = chrome.state.page;
    let frame = l.later;
    let header_h = 22.0 * l.scale;
    let ink = match page {
        AnalyzePage::Clean => p.mod_lfo,
        AnalyzePage::Slice => p.mod_macro,
        AnalyzePage::Record => p.meter_peak,
        AnalyzePage::Notes => p.note,
    };
    bridge::draw_console(
        scene,
        theme,
        labels,
        frame,
        Rect::new(frame.x, frame.y, frame.width, header_h),
        page.label(),
        ink,
        false,
        chrome.skin,
        t.heading,
    );
    let [first, second] = crate::canvas::later_lines(page);
    let body = Rect::new(
        frame.x + 18.0,
        frame.y + header_h + 10.0,
        frame.width - 36.0,
        (frame.height - header_h - 20.0) / 2.0,
    );
    text_in(scene, labels, &first, t.value, body, Some(0.0), p.text);
    let below = Rect::new(body.x, body.bottom(), body.width, body.height);
    text_in(
        scene,
        labels,
        &second,
        t.value,
        below,
        Some(0.0),
        p.text_muted,
    );
}

// ------------------------------------------------------------- consoles ---

fn draw_cards(
    scene: &mut Scene,
    theme: &Theme,
    labels: &Labels,
    chrome: &AnalyzeChrome<'_>,
    t: &BridgeType,
) {
    use crate::canvas::AnalyzeCard;
    let p = &theme.palette;
    let l = &chrome.layout;
    let state = chrome.state;
    let hover = state.hover;
    for (card, layout, ink) in [
        (AnalyzeCard::Note, &l.note_card, p.note),
        (AnalyzeCard::Output, &l.output_card, p.accent),
    ] {
        let hot = match card {
            AnalyzeCard::Note => matches!(hover, Some(AnalyzeHit::Note(_))),
            AnalyzeCard::Output => matches!(
                hover,
                Some(
                    AnalyzeHit::CopyNotes
                        | AnalyzeHit::MakeClip
                        | AnalyzeHit::CopyScaleButton
                        | AnalyzeHit::KeepBends
                )
            ),
        };
        bridge::draw_console(
            scene,
            theme,
            labels,
            layout.frame,
            layout.header,
            card.label(),
            ink,
            hot,
            chrome.skin,
            t.heading,
        );
    }

    // The note in hand.
    let lines = crate::canvas::note_card_lines(chrome.view, state);
    let focused = crate::canvas::note_card_focus(chrome.view, state).is_some();
    let body = l.note_card.body;
    let line_h = (body.height / 3.0).min(20.0 * l.scale);
    for (i, line) in lines.iter().enumerate() {
        let rect = Rect::new(body.x, body.y + i as f32 * line_h, body.width, line_h);
        if rect.bottom() > body.bottom() + 1.0 {
            break;
        }
        text_in(
            scene,
            labels,
            line,
            if i == 0 && focused {
                t.heading
            } else {
                t.value
            },
            rect,
            Some(0.0),
            if i == 0 { p.text } else { p.text_muted },
        );
    }

    // What to do with the notes.
    use crate::canvas::{COPY_NOTES, COPY_SCALE, KEEP_BENDS, MAKE_CLIP};
    for (rect, word, hit) in [
        (l.copy_notes, COPY_NOTES, AnalyzeHit::CopyNotes),
        (l.make_clip, MAKE_CLIP, AnalyzeHit::MakeClip),
        (l.copy_scale, COPY_SCALE, AnalyzeHit::CopyScaleButton),
    ] {
        draw_flop_button(
            scene,
            theme,
            labels,
            rect,
            word,
            hover == Some(hit),
            Some(t.value),
        );
    }
    let keep = l.keep_bends;
    let switch_w = 40.0 * l.scale;
    draw_flop_switch(
        scene,
        theme,
        Rect::new(keep.x, keep.y, switch_w, keep.height),
        switch_w,
        state.keep_bends,
        hover == Some(AnalyzeHit::KeepBends),
    );
    text_in(
        scene,
        labels,
        KEEP_BENDS,
        t.caption,
        Rect::new(
            keep.x + switch_w,
            keep.y,
            keep.width - switch_w,
            keep.height,
        ),
        Some(4.0),
        if state.keep_bends {
            p.text
        } else {
            p.text_muted
        },
    );
}

fn draw_job(
    scene: &mut Scene,
    theme: &Theme,
    labels: &Labels,
    chrome: &AnalyzeChrome<'_>,
    t: &BridgeType,
) {
    let p = &theme.palette;
    let m = &theme.metrics;
    let strip = chrome.layout.job;
    let fraction = chrome.view.analysing.unwrap_or(0.0).clamp(0.0, 1.0);
    fill_rect_rounded(scene, strip, m.corner_radius, darken(p.window, 0.3));
    let bar = Rect::new(strip.x, strip.y, strip.width * fraction, strip.height);
    fill_rect_vertical(
        scene,
        bar,
        m.corner_radius,
        p.accent.with_alpha(0x90),
        p.accent.with_alpha(0x50),
    );
    stroke_rect_rounded(
        scene,
        strip,
        m.corner_radius,
        1.0,
        p.accent.with_alpha(0x80),
    );
    text_in(
        scene,
        labels,
        &crate::canvas::job_text(chrome.view),
        t.value,
        strip,
        Some(10.0),
        p.text,
    );
}

/// The scale's popover: the octave with the scale lit, the notes and their
/// degrees under it, and the other readings.
fn draw_popover(
    scene: &mut Scene,
    theme: &Theme,
    labels: &Labels,
    chrome: &AnalyzeChrome<'_>,
    t: &BridgeType,
) {
    let p = &theme.palette;
    let m = &theme.metrics;
    let l = &chrome.layout;
    let s = l.scale;
    let Some(key) = &chrome.view.key else {
        return;
    };
    let frame = l.popover;
    fill_rect_rounded(
        scene,
        Rect::new(frame.x + 2.0, frame.y + 3.0, frame.width, frame.height),
        m.corner_radius + 2.0,
        darken(p.window, 0.6).with_alpha(0x90),
    );
    fill_rect_rounded(scene, frame, m.corner_radius + 2.0, p.panel_header);
    stroke_rect_rounded(
        scene,
        frame,
        m.corner_radius + 2.0,
        1.0,
        p.accent.with_alpha(0xa0),
    );
    let inner = frame.inset(10.0 * s);
    let lines = crate::canvas::popover_lines(chrome.view);
    let line_h = 16.0 * s;
    // The title.
    if let Some(title) = lines.first() {
        text_in(
            scene,
            labels,
            title,
            t.value,
            Rect::new(inner.x, inner.y, inner.width, line_h),
            Some(0.0),
            lighten(p.accent, 0.35),
        );
    }
    // One octave from the root, the scale's keys lit.
    let mask = key.key.mask().unwrap_or(0);
    let root = key.key.root % 12;
    let board = Rect::new(inner.x, inner.y + line_h + 4.0 * s, inner.width, 34.0 * s);
    let naturals: Vec<u8> = (0..12u8)
        .filter(|c| !matches!(c, 1 | 3 | 6 | 8 | 10))
        .collect();
    let key_w = board.width / naturals.len() as f32;
    let lit = |class: u8| mask & (1 << class) != 0;
    for (i, class) in naturals.iter().enumerate() {
        let r = Rect::new(
            board.x + i as f32 * key_w,
            board.y,
            key_w - 1.0,
            board.height,
        );
        let ink = if *class == root {
            p.accent
        } else if lit(*class) {
            mix(p.key_white, p.accent, 0.3)
        } else {
            mix(p.key_white, p.panel_header, 0.75)
        };
        fill_rect_rounded(scene, r, 2.0, ink);
    }
    let mut seen = 0usize;
    for class in 0..12u8 {
        if !matches!(class, 1 | 3 | 6 | 8 | 10) {
            seen += 1;
            continue;
        }
        let seam = board.x + seen as f32 * key_w;
        let w = key_w * 0.6;
        let r = Rect::new(seam - w / 2.0, board.y, w, board.height * 0.6);
        let ink = if class == root {
            p.accent
        } else if lit(class) {
            mix(p.key_black, p.accent, 0.55)
        } else {
            p.key_black
        };
        fill_rect_rounded(scene, r, 1.5, ink);
    }
    // Each of the scale's notes under its key: the name, then its degree.
    let (names, degrees) = crate::canvas::scale_degrees(&key.key);
    let steps = fontelle_types::scale(&key.key.scale).map_or(&[][..], |sc| sc.steps);
    let centre_of = |class: u8| -> f32 {
        let natural_index = naturals.iter().position(|c| *c == class);
        match natural_index {
            Some(i) => board.x + (i as f32 + 0.5) * key_w,
            None => {
                let before = naturals.iter().filter(|c| **c < class).count();
                board.x + before as f32 * key_w
            }
        }
    };
    let name_row = Rect::new(inner.x, board.bottom() + 4.0 * s, inner.width, 15.0 * s);
    let degree_row = Rect::new(inner.x, name_row.bottom(), inner.width, 13.0 * s);
    for ((step, name), degree) in steps.iter().zip(&names).zip(&degrees) {
        let class = (root + step) % 12;
        let x = centre_of(class);
        for (row, text, style, ink) in [
            (
                name_row,
                name,
                t.value,
                if class == root {
                    lighten(p.accent, 0.4)
                } else {
                    p.text
                },
            ),
            (degree_row, degree, t.caption, p.text_muted),
        ] {
            if let Some(shaped) = labels.get_styled(text, style) {
                draw_text_clipped(
                    scene,
                    shaped,
                    frame,
                    x - shaped.width / 2.0,
                    row.y + (row.height - shaped.height) / 2.0,
                    ink,
                );
            }
        }
    }
    let mut y = degree_row.bottom() + 6.0 * s;
    for (i, line) in lines.iter().enumerate().skip(1) {
        let ink = if i == 1 {
            lighten(p.accent, 0.3)
        } else {
            p.text_muted
        };
        text_in(
            scene,
            labels,
            line,
            t.value,
            Rect::new(inner.x, y, inner.width, line_h),
            Some(0.0),
            ink,
        );
        y += line_h;
    }
}

// ------------------------------------------------ laying it out, shaped ---

/// Every string the window will draw, shaped in its style — the window's
/// shaping pass and the headless shot's, one function.
pub fn shape_analyze(
    labels: &mut Labels,
    text: &mut crate::text::TextContext,
    font: &crate::theme::FontTokens,
    view: &AnalyzeView,
    state: &AnalyzeState,
    layout: &AnalyzeLayout,
) {
    for (string, kind) in crate::canvas::analyze_strings(view, state, layout) {
        labels.ensure_styled(&string, font, analyze_style(kind, state.scale), text);
    }
}

/// The window laid out in `body` with its header chips measured by the
/// shaper: a first pass on estimates names the strings, they are shaped, and
/// the second pass measures them.
pub fn lay_out_analyze(
    labels: &mut Labels,
    text: &mut crate::text::TextContext,
    font: &crate::theme::FontTokens,
    body: Rect,
    view: &AnalyzeView,
    state: &AnalyzeState,
) -> AnalyzeLayout {
    let probe =
        crate::canvas::analyze_layout(body, view, state, &|s| crate::canvas::estimated_width(s));
    shape_analyze(labels, text, font, view, state, &probe);
    let scale = state.scale.max(0.1);
    let style = analyze_style(AnalyzeText::Value, scale);
    let labels = &*labels;
    let measure = |s: &str| {
        labels
            .get_styled(s, style)
            .map(|shaped| shaped.width / scale)
            .unwrap_or_else(|| crate::canvas::estimated_width(s))
    };
    crate::canvas::analyze_layout(body, view, state, &measure)
}
