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
    if state.page != AnalyzePage::Notes {
        return;
    }
    // The tools, the one in hand lit.
    for (tool, rect) in &l.tools {
        bridge::draw_hud_tab(
            scene,
            theme,
            labels,
            *rect,
            tool.label(),
            *tool == state.tool,
            t.value,
        );
    }
    // ▶ / ■, drawn; lit while it plays.
    let p = &theme.palette;
    let playing = state.playhead.is_some();
    bridge::draw_hud_tab(scene, theme, labels, l.play, "", playing, t.value);
    let ink = if playing || state.hover == Some(AnalyzeHit::Play) {
        lighten(p.accent, 0.45)
    } else {
        p.text
    };
    let (cx, cy) = (
        l.play.x + l.play.width / 2.0,
        l.play.y + l.play.height / 2.0,
    );
    let r = l.play.height * 0.24;
    if playing {
        fill_rect(scene, Rect::new(cx - r, cy - r, r * 2.0, r * 2.0), ink);
    } else {
        let mut path = BezPath::new();
        path.move_to((f64::from(cx - r * 0.8), f64::from(cy - r)));
        path.line_to((f64::from(cx + r), f64::from(cy)));
        path.line_to((f64::from(cx - r * 0.8), f64::from(cy + r)));
        path.close_path();
        scene.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            ink.to_peniko(),
            None,
            &path,
        );
    }
    bridge::draw_hud_tab(
        scene,
        theme,
        labels,
        l.listen,
        crate::canvas::LISTEN_ORIGINAL,
        state.listen_original,
        t.value,
    );
    text_in(
        scene,
        labels,
        &crate::canvas::readout_text(chrome.view, state),
        t.value,
        l.readout,
        Some(4.0),
        if playing { p.text } else { p.text_muted },
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
            RowShade::Root => fill_rect(scene, row, p.row_scale_root.with_alpha(0x90)),
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
        // The pitch through it; a moved note's sung line ghosted where it
        // was, and its new one through the blob (plan §3.3).
        if note.curve.len() >= 2 {
            let shift = note.edit.map_or(0.0, |e| e.shift_cents);
            if note.edit.is_some() {
                let ghost: Vec<(f32, f32)> = note
                    .curve
                    .iter()
                    .map(|(time, cents)| {
                        (
                            lane.x_of(state, *time),
                            lane.y_of(state, f32::from(note.midi) + cents / 100.0),
                        )
                    })
                    .collect();
                stroke_polyline(scene, &ghost, grid, 1.0, p.text.with_alpha(0x55));
            }
            let edit = note.edit.unwrap_or_default();
            let length = (note.end - note.start).max(1e-3);
            let points: Vec<(f32, f32)> = note
                .curve
                .iter()
                .map(|(time, cents)| {
                    // The move eased in and out over the glides, the drift
                    // and vibrato roughly as the render treats them.
                    let ms = ((time - note.start) * 1000.0) as f32;
                    let to_end = ((length - (time - note.start)) * 1000.0) as f32;
                    let ease = |x: f32, g: f32| {
                        if g <= 0.0 {
                            1.0
                        } else {
                            0.5 - 0.5 * (std::f32::consts::PI * (x / g).clamp(0.0, 1.0)).cos()
                        }
                    };
                    let e = ease(ms, edit.glide_in_ms).min(ease(to_end, edit.glide_out_ms));
                    let wobble = cents - note.cents;
                    let kept = 1.0 - edit.flatten * 0.5 - (1.0 - edit.vibrato) * 0.5;
                    let moved = cents + e * (shift + (kept - 1.0) * wobble);
                    (
                        lane.x_of(state, *time),
                        lane.y_of(state, f32::from(note.midi) + moved / 100.0),
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
        // How far off it sits, when it is enough to hear (once moved, where
        // it sits now).
        let now = crate::canvas::AnalyzedNote {
            cents: note.cents + note.edit.map_or(0.0, |e| e.shift_cents),
            curve: Vec::new(),
            ..note.clone()
        };
        let now = crate::canvas::AnalyzedNote {
            midi: (f32::from(now.midi) + (now.cents / 100.0).round()).clamp(0.0, 127.0) as u8,
            cents: now.cents - (now.cents / 100.0).round() * 100.0,
            ..now
        };
        if let Some(tag) = crate::canvas::analyze_cents_tag(&now)
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

    // The region Space loops, the cursor it plays from, and the playhead.
    if let Some((a, b)) = state.region {
        let (x0, x1) = (lane.x_of(state, a), lane.x_of(state, b));
        fill_rect(
            scene,
            Rect::new(x0, grid.y, (x1 - x0).max(1.0), grid.height),
            p.accent.with_alpha(0x14),
        );
    }
    let cursor = lane.x_of(state, state.cursor);
    if cursor >= grid.x && cursor <= grid.right() {
        fill_rect(
            scene,
            Rect::new(cursor.round(), grid.y, 1.0, grid.height),
            p.text.with_alpha(0x50),
        );
    }
    if let Some(at) = state.playhead {
        let x = lane.x_of(state, at);
        if x >= grid.x && x <= grid.right() {
            fill_glow(
                scene,
                (x, grid.y + grid.height / 2.0),
                6.0,
                p.playhead,
                0x30,
            );
            fill_rect(
                scene,
                Rect::new(x.round() - 0.5, grid.y, 2.0, grid.height),
                p.playhead,
            );
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

/// The pitch picture: the contour, a third of a semitone a row, drawn so the
/// lane stays as dark as the waveform's and only what was heard shows.
///
/// Ty, on P1's: *"it made that window a lot brighter ... ensure theres good
/// enough contrast to where the background is still dark like the other
/// mode but you can make out the pitches around the graph. right now it kind
/// of just looks cloudy."* Four things make the difference:
///
/// - **The floor goes.** The model's contour carries about a tenth of full
///   scale in every cell; what is at or under [`AnalyzeImage::floor`] is not
///   drawn at all, and what is over it is measured from it.
/// - **A ramp that starts in the dark.** Quiet energy is a deep shade of the
///   accent at low opacity; only the strongest reaches the accent and past
///   it towards white.
/// - **Ridges, not bands.** Where a row is the peak of its column the line
///   is drawn at its parabolic peak, a sub-row thick; the rows either side
///   are a faint shoulder.
/// - **Partials fainter than the notes.** A cell within a semitone of a
///   note sounding then is drawn whole; anything else — the octave, the
///   twelfth a voice throws — at three quarters.
fn draw_spectrogram(scene: &mut Scene, theme: &Theme, chrome: &AnalyzeChrome<'_>) {
    let p = &theme.palette;
    let lane = &chrome.layout.lane;
    let state = chrome.state;
    let view = chrome.view;
    let Some(image) = &view.spectrogram else {
        return;
    };
    if image.columns == 0
        || image.rows == 0
        || image.columns_per_second <= 0.0
        || image.rows_per_semitone <= 0.0
    {
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
    // No more columns than the lane has pixels: zoomed out on a long take,
    // a pixel takes the strongest of the cells it covers, so a note is not
    // thinned away and a frame does not build megabytes.
    let stride = (c1 - c0).div_ceil(grid.width.max(1.0) as usize).max(1);
    let w = (c1 - c0).div_ceil(stride);
    // Each row drawn as this many sub-rows, about a pixel and a half each,
    // so a ridge can be thinner than the row it is in.
    let px_per_row = state.row_height / image.rows_per_semitone;
    let sub = (px_per_row / 1.6).round().clamp(1.0, 4.0) as usize;
    let rows = r1 - r0;
    let h = rows * sub;
    let floor = f32::from(image.floor);
    let span = (255.0 - floor).max(1.0);
    let deep = mix(p.window, p.accent, 0.75);
    let hot = lighten(p.accent, 0.75);
    let notes = state.notes(view);
    let mut sounding: Vec<f32> = Vec::new();
    let mut column = vec![0.0f32; rows + 2];
    let mut rgba = vec![0u8; w * h * 4];
    for ci in 0..w {
        let first = c0 + ci * stride;
        let last = (first + stride).min(c1);
        let (t0, t1) = (
            first as f64 / image.columns_per_second,
            last as f64 / image.columns_per_second,
        );
        sounding.clear();
        sounding.extend(
            notes
                .iter()
                .filter(|n| n.start < t1 && n.end > t0)
                .map(|n| f32::from(n.midi) + n.cents / 100.0),
        );
        // This column's cells over the floor, a row either side for the
        // peak test (the strongest of the cells a pixel covers).
        for (i, cell) in column.iter_mut().enumerate() {
            let row = (r0 + i).checked_sub(1);
            *cell = row
                .filter(|r| *r < image.rows)
                .map(|row| {
                    (first..last)
                        .map(|c| image.data[c * image.rows + row])
                        .max()
                        .map_or(0.0, |v| (f32::from(v) - floor).max(0.0) / span)
                })
                .unwrap_or(0.0);
        }
        for ri in 0..rows {
            let (down, v, up) = (column[ri], column[ri + 1], column[ri + 2]);
            if v <= 0.0 {
                continue;
            }
            let midi = image.lowest_midi + (r0 + ri) as f32 / image.rows_per_semitone;
            let fundamental =
                sounding.is_empty() || sounding.iter().any(|m| (m - midi).abs() <= 0.7);
            let strength = v.powf(0.75) * if fundamental { 1.0 } else { 0.75 };
            let ridge = v >= up && v >= down;
            // Where in the row the peak sits, -0.5..0.5 of a row (up is +).
            let curvature = down - 2.0 * v + up;
            let peak = if ridge && curvature < 0.0 {
                (0.5 * (down - up) / curvature).clamp(-0.5, 0.5)
            } else {
                0.0
            };
            for k in 0..sub {
                let centre = (k as f32 + 0.5) / sub as f32 - 0.5;
                let on_peak = sub == 1 || (centre - peak).abs() <= 0.5 / sub as f32 + 1e-3;
                let t = match (ridge, on_peak) {
                    (true, true) => strength,
                    (true, false) => strength * 0.15,
                    (false, _) => strength * 0.15,
                };
                if t < 0.02 {
                    continue;
                }
                let ink = if t < 0.5 {
                    mix(deep, p.accent, t * 2.0)
                } else {
                    mix(p.accent, hot, (t - 0.5) * 2.0)
                };
                // Top row first in the image: high pitch at the top.
                let y = (rows - 1 - ri) * sub + (sub - 1 - k);
                let at = (y * w + ci) * 4;
                rgba[at] = ink.0[0];
                rgba[at + 1] = ink.0[1];
                rgba[at + 2] = ink.0[2];
                rgba[at + 3] = ((t.powf(0.7) * 1.5).min(1.0) * 255.0) as u8;
            }
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
    let x1 = lane.x_of(state, (c0 + w * stride) as f64 / image.columns_per_second);
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
    for (index, chord) in state.chord_spans(chrome.view).iter().enumerate() {
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
    let state = chrome.state;
    if let Some((a, b)) = state.region {
        let (x0, x1) = (
            l.lane.x_of(state, a).max(ruler.x),
            l.lane.x_of(state, b).min(ruler.right()),
        );
        if x1 > x0 {
            fill_rect(
                scene,
                Rect::new(x0, ruler.y + 1.0, x1 - x0, ruler.height - 1.0),
                p.accent.with_alpha(0x50),
            );
        }
    }
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
    // The cursor's flag on the ruler, and the playhead's.
    for (at, ink) in [(Some(state.cursor), p.text), (state.playhead, p.playhead)] {
        let Some(at) = at else { continue };
        let x = l.lane.x_of(state, at);
        if x < ruler.x || x > ruler.right() {
            continue;
        }
        let mut flag = BezPath::new();
        flag.move_to((f64::from(x - 5.0), f64::from(ruler.y + 1.0)));
        flag.line_to((f64::from(x + 5.0), f64::from(ruler.y + 1.0)));
        flag.line_to((f64::from(x), f64::from(ruler.y + 8.0)));
        flag.close_path();
        scene.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            ink.to_peniko(),
            None,
            &flag,
        );
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
                        | AnalyzeHit::Render
                        | AnalyzeHit::RenderMenu
                        | AnalyzeHit::Revert
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
    let line_h = (body.height / 4.0).min(18.0 * l.scale);
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
    use crate::canvas::{COPY_NOTES, COPY_SCALE, KEEP_BENDS, MAKE_CLIP, RENDER_TO_CLIP, REVERT};
    let mut buttons = vec![
        (l.copy_notes, COPY_NOTES, AnalyzeHit::CopyNotes),
        (l.make_clip, MAKE_CLIP, AnalyzeHit::MakeClip),
        (l.copy_scale, COPY_SCALE, AnalyzeHit::CopyScaleButton),
        (l.render, RENDER_TO_CLIP, AnalyzeHit::Render),
        (l.render_menu, "", AnalyzeHit::RenderMenu),
    ];
    if !l.revert.is_empty() {
        buttons.push((l.revert, REVERT, AnalyzeHit::Revert));
    }
    for (rect, word, hit) in buttons {
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
    chevron(
        scene,
        l.render_menu,
        if hover == Some(AnalyzeHit::RenderMenu) {
            lighten(p.accent, 0.4)
        } else {
            p.text
        },
        l.scale,
    );
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
