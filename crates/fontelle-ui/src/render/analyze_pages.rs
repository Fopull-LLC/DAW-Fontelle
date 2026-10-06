//! Drawing Analyze Musically's Clean, Slice and Record pages and every
//! page's cards (`docs/analyze-musically-plan.md` §3.1, P3–P5).
//!
//! The rectangles and the words are `canvas::analyze`'s; this is colour.
//! Each page wears its card ink (plan §3.1: Clean the LFO teal, Slice the
//! macro amber, Record the meter's red) on its screen and its consoles, so a
//! glance says which page is up.

use vello::Scene;
use vello::kurbo::{Affine, BezPath, Circle, Rect as KRect, Stroke};
use vello::peniko::{BlendMode, Fill};

use super::analyze::{AnalyzeChrome, darken, text_in};
use super::{
    BridgeType, bridge, draw_flop_button, draw_flop_chip, draw_flop_knob, draw_flop_switch,
    draw_text_clipped, fill_glow, fill_rect, fill_rect_rounded, fill_rect_vertical, lighten, mix,
    stroke_polyline, stroke_rect_rounded,
};
use crate::canvas::{
    AnalyzeCard, AnalyzeControl, AnalyzeControlKind, AnalyzeHit, AnalyzePage, AnalyzeTool,
};
use crate::layout::Rect;
use crate::text::Labels;
use crate::theme::{Color, Theme};

/// The ink a card (and its page's screen) wears.
pub(super) fn card_ink(card: AnalyzeCard, p: &crate::theme::Palette) -> Color {
    match card {
        AnalyzeCard::Note => p.note,
        AnalyzeCard::Pitch => p.modulation,
        AnalyzeCard::Output => p.accent,
        AnalyzeCard::Noise | AnalyzeCard::Denoise | AnalyzeCard::Shape => p.mod_lfo,
        AnalyzeCard::Points | AnalyzeCard::Layout | AnalyzeCard::Send => p.mod_macro,
        AnalyzeCard::Source | AnalyzeCard::Record | AnalyzeCard::Takes => p.meter_peak,
    }
}

/// A page's own ink.
pub(super) fn page_ink(page: AnalyzePage, p: &crate::theme::Palette) -> Color {
    match page {
        AnalyzePage::Notes => p.note,
        AnalyzePage::Clean => p.mod_lfo,
        AnalyzePage::Slice => p.mod_macro,
        AnalyzePage::Record => p.meter_peak,
    }
}

fn clip_to(scene: &mut Scene, rect: Rect) {
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
}

// ------------------------------------------------------------ the cards ---

/// Every card's console, and the pages' controls on them. The Notes page's
/// Note and Output contents are `analyze::draw_cards`'.
pub(super) fn draw_page_cards(
    scene: &mut Scene,
    theme: &Theme,
    labels: &Labels,
    chrome: &AnalyzeChrome<'_>,
    t: &BridgeType,
) {
    let p = &theme.palette;
    let l = &chrome.layout;
    let state = chrome.state;
    let view = chrome.view;
    for (card, layout) in &l.cards {
        let hot = match state.hover {
            Some(AnalyzeHit::Control(c)) => l
                .control(c)
                .is_some_and(|r| layout.frame.contains(r.x + 1.0, r.y + 1.0)),
            _ => false,
        };
        bridge::draw_console(
            scene,
            theme,
            labels,
            layout.frame,
            layout.header,
            card.label(),
            card_ink(*card, p),
            hot,
            chrome.skin,
            t.heading,
        );
    }
    for (control, rect) in &l.controls {
        draw_control(scene, theme, labels, chrome, t, *control, *rect);
    }
    // The cards' lines of words.
    match state.page {
        AnalyzePage::Clean => {
            let captured = view.clean.denoise.noise.is_some();
            text_in(
                scene,
                labels,
                &crate::canvas::noise_text(view),
                t.value,
                l.info,
                Some(0.0),
                if captured {
                    lighten(p.mod_lfo, 0.3)
                } else {
                    p.text_muted
                },
            );
            if !captured && let Some(card) = l.cards.iter().find(|(c, _)| *c == AnalyzeCard::Noise)
            {
                let body = card.1.body;
                let line = Rect::new(
                    body.x,
                    body.bottom() - 16.0 * l.scale,
                    body.width,
                    16.0 * l.scale,
                );
                text_in(
                    scene,
                    labels,
                    crate::canvas::NOISE_HINT,
                    t.caption,
                    line,
                    Some(0.0),
                    p.text_muted,
                );
            }
            // Captions over the chooser.
            if let Some(chip) = l.control(AnalyzeControl::FadeShape) {
                caption_over(scene, labels, t, chip, "SHAPE", p.text_muted, l.scale);
            }
        }
        AnalyzePage::Slice => {
            text_in(
                scene,
                labels,
                &crate::canvas::slices_text(view, state),
                t.value,
                l.info,
                Some(0.0),
                lighten(p.mod_macro, 0.3),
            );
            if let Some(chip) = l.control(AnalyzeControl::AutoSlice) {
                caption_over(scene, labels, t, chip, "FIND", p.text_muted, l.scale);
            }
            draw_keyboard(scene, theme, labels, chrome, t);
        }
        AnalyzePage::Record => {
            draw_meter(scene, theme, chrome);
            let warn = state.meter.dropped_frames > 0
                || view.record.as_ref().is_some_and(|r| r.problem.is_some());
            text_in(
                scene,
                labels,
                &crate::canvas::record_text(view, state),
                t.caption,
                l.info,
                Some(0.0),
                if warn {
                    p.param_automated
                } else if state.meter.recording {
                    lighten(p.meter_peak, 0.3)
                } else {
                    p.text_muted
                },
            );
        }
        AnalyzePage::Notes => {}
    }
}

fn caption_over(
    scene: &mut Scene,
    labels: &Labels,
    t: &BridgeType,
    chip: Rect,
    caption: &str,
    ink: Color,
    s: f32,
) {
    let band = Rect::new(chip.x, chip.y - 14.0 * s, chip.width, 13.0 * s);
    text_in(scene, labels, caption, t.caption, band, Some(2.0), ink);
}

fn draw_control(
    scene: &mut Scene,
    theme: &Theme,
    labels: &Labels,
    chrome: &AnalyzeChrome<'_>,
    t: &BridgeType,
    control: AnalyzeControl,
    rect: Rect,
) {
    let p = &theme.palette;
    let state = chrome.state;
    let view = chrome.view;
    let hot = state.hover == Some(AnalyzeHit::Control(control));
    let enabled = crate::canvas::control_enabled(control, view, state);
    let s = chrome.layout.scale;
    match control.kind() {
        AnalyzeControlKind::Knob => {
            let AnalyzeControl::Knob(knob) = control else {
                return;
            };
            // A whole cell's knob, or a small one beside its read-out where
            // the card has two rows of them.
            let size = if rect.height < crate::canvas::FLOP_CELL_H * s - 0.5 {
                crate::canvas::KnobSize::Small
            } else {
                crate::canvas::KnobSize::Medium
            };
            let anatomy =
                crate::canvas::cell_anatomy(rect, size, &crate::canvas::ParamKind::Knob, s);
            let value = crate::canvas::analyze_knob_value(knob, view, state);
            let dragging = state.dragging_knob() == Some(knob);
            if let Some(label) = labels.get_styled(knob.caption(), t.caption) {
                draw_text_clipped(
                    scene,
                    label,
                    anatomy.caption,
                    anatomy.caption.x + ((anatomy.caption.width - label.width) / 2.0).max(1.0),
                    anatomy.caption.y + (anatomy.caption.height - label.height) / 2.0,
                    if value.is_some() && (hot || dragging) {
                        p.text
                    } else {
                        p.text_muted
                    },
                );
            }
            draw_flop_knob(
                scene,
                theme,
                anatomy.control,
                value.map_or(0.0, |v| knob.to_unit(v)),
                hot && value.is_some(),
                dragging,
                false,
                chrome.skin.and_then(|skin| skin.knob.as_ref()),
            );
            if value.is_none() {
                // Nothing to act on: the knob greyed under the window.
                fill_rect_rounded(
                    scene,
                    anatomy.control.inset(-2.0),
                    anatomy.control.width,
                    p.window.with_alpha(0x90),
                );
            }
            let words = crate::canvas::knob_text(knob, view, state);
            if let Some(label) = labels.get_styled(&words, t.value) {
                draw_text_clipped(
                    scene,
                    label,
                    anatomy.readout,
                    anatomy.readout.x + ((anatomy.readout.width - label.width) / 2.0).max(1.0),
                    anatomy.readout.y + (anatomy.readout.height - label.height) / 2.0,
                    if state.typing.is_some() && words.ends_with('\u{2502}') {
                        lighten(p.accent, 0.4)
                    } else if value.is_none() {
                        p.text_muted
                    } else if hot || dragging {
                        p.accent
                    } else {
                        p.text
                    },
                );
            }
        }
        AnalyzeControlKind::Button => {
            draw_flop_button(
                scene,
                theme,
                labels,
                rect,
                control.label(),
                hot && enabled,
                Some(t.value),
            );
            if !enabled {
                fill_rect_rounded(
                    scene,
                    rect,
                    theme.metrics.corner_radius,
                    p.window.with_alpha(0x80),
                );
            }
        }
        AnalyzeControlKind::Primary => {
            let ink = page_ink(state.page, p);
            let (top, bottom) = if enabled {
                (
                    lighten(ink, if hot { 0.25 } else { 0.12 }),
                    mix(ink, p.window, 0.25),
                )
            } else {
                (mix(ink, p.window, 0.65), mix(ink, p.window, 0.75))
            };
            if enabled && hot {
                fill_glow(
                    scene,
                    (rect.x + rect.width / 2.0, rect.y + rect.height / 2.0),
                    rect.width * 0.55,
                    ink,
                    0x40,
                );
            }
            fill_rect_vertical(scene, rect, theme.metrics.corner_radius + 1.0, top, bottom);
            stroke_rect_rounded(
                scene,
                rect,
                theme.metrics.corner_radius + 1.0,
                1.0,
                lighten(ink, 0.35).with_alpha(if enabled { 0xff } else { 0x60 }),
            );
            text_in(
                scene,
                labels,
                control.label(),
                t.value,
                rect,
                None,
                if enabled {
                    darken(p.window, 0.2)
                } else {
                    p.text_muted
                },
            );
        }
        AnalyzeControlKind::Switch => {
            let on = crate::canvas::switch_on(control, view, state);
            let pill_w = 40.0 * s;
            draw_flop_switch(
                scene,
                theme,
                Rect::new(rect.x, rect.y, pill_w, rect.height),
                pill_w,
                on,
                hot && enabled,
            );
            text_in(
                scene,
                labels,
                control.label(),
                t.value,
                Rect::new(
                    rect.x + pill_w,
                    rect.y,
                    (rect.width - pill_w).max(0.0),
                    rect.height,
                ),
                Some(2.0),
                if !enabled {
                    p.text_muted.with_alpha(0x90)
                } else if on {
                    p.text
                } else {
                    p.text_muted
                },
            );
        }
        AnalyzeControlKind::Chip => {
            draw_flop_chip(
                scene,
                theme,
                labels,
                rect,
                &crate::canvas::control_text(control, view, state),
                hot && enabled,
                Some(t.value),
            );
        }
        AnalyzeControlKind::Segment => {
            let lit = crate::canvas::switch_on(control, view, state);
            super::analyze::plate(scene, theme, rect, false);
            let ink = page_ink(state.page, p);
            if lit {
                fill_rect_rounded(scene, rect.inset(2.0), 3.0, ink.with_alpha(0x55));
                stroke_rect_rounded(scene, rect.inset(2.0), 3.0, 1.0, lighten(ink, 0.2));
            } else if hot && enabled {
                fill_rect_rounded(scene, rect.inset(2.0), 3.0, p.text.with_alpha(0x14));
            }
            text_in(
                scene,
                labels,
                control.label(),
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
        AnalyzeControlKind::Lamp => draw_lamp(scene, theme, labels, chrome, t, rect, hot),
    }
}

/// The record lamp: a dark red ring disarmed, lit armed, blazing while a
/// take runs, its word under it.
fn draw_lamp(
    scene: &mut Scene,
    theme: &Theme,
    labels: &Labels,
    chrome: &AnalyzeChrome<'_>,
    t: &BridgeType,
    rect: Rect,
    hot: bool,
) {
    let p = &theme.palette;
    let view = chrome.view;
    let armed = view.record.as_ref().is_some_and(|r| r.armed);
    let recording =
        chrome.state.meter.recording || view.record.as_ref().is_some_and(|r| r.recording);
    let word_h = 14.0 * chrome.layout.scale;
    let radius = ((rect.height - word_h) / 2.0).min(rect.width / 2.0) - 2.0;
    let centre = (rect.x + rect.width / 2.0, rect.y + radius + 2.0);
    let red = p.meter_peak;
    if armed || recording {
        fill_glow(
            scene,
            centre,
            radius * if recording { 2.6 } else { 1.9 },
            red,
            if recording { 0x90 } else { 0x50 },
        );
    }
    let face = if recording {
        lighten(red, 0.15)
    } else if armed {
        red
    } else {
        mix(red, p.window, 0.75)
    };
    scene.fill(
        Fill::NonZero,
        Affine::IDENTITY,
        face.to_peniko(),
        None,
        &Circle::new(
            (f64::from(centre.0), f64::from(centre.1)),
            f64::from(radius),
        ),
    );
    scene.stroke(
        &Stroke::new(if hot { 2.0 } else { 1.5 }),
        Affine::IDENTITY,
        if hot {
            lighten(red, 0.4)
        } else {
            mix(red, p.border, 0.4)
        }
        .to_peniko(),
        None,
        &Circle::new(
            (f64::from(centre.0), f64::from(centre.1)),
            f64::from(radius),
        ),
    );
    // The highlight that makes it a lamp, not a dot.
    scene.fill(
        Fill::NonZero,
        Affine::IDENTITY,
        p.text
            .with_alpha(if armed { 0x50 } else { 0x20 })
            .to_peniko(),
        None,
        &Circle::new(
            (
                f64::from(centre.0 - radius * 0.3),
                f64::from(centre.1 - radius * 0.35),
            ),
            f64::from(radius * 0.28),
        ),
    );
    let word = Rect::new(
        rect.x - 10.0,
        rect.bottom() - word_h,
        rect.width + 20.0,
        word_h,
    );
    text_in(
        scene,
        labels,
        &crate::canvas::control_text(AnalyzeControl::Arm, view, chrome.state),
        t.caption,
        word,
        None,
        if armed {
            lighten(red, 0.4)
        } else {
            p.text_muted
        },
    );
}

/// The Record card's meter: green to amber to red, -60 dB to 0.
fn draw_meter(scene: &mut Scene, theme: &Theme, chrome: &AnalyzeChrome<'_>) {
    let p = &theme.palette;
    let r = chrome.layout.meter;
    if r.is_empty() {
        return;
    }
    fill_rect_rounded(scene, r, 2.0, darken(p.window, 0.4));
    let level = chrome.state.meter.level.max(1e-6);
    let db = 20.0 * level.log10();
    let fill = ((db + 60.0) / 60.0).clamp(0.0, 1.0);
    if fill > 0.0 {
        let ink = if db > -3.0 {
            p.meter_peak
        } else if db > -12.0 {
            p.param_automated
        } else {
            p.meter
        };
        fill_rect_rounded(
            scene,
            Rect::new(r.x, r.y, r.width * fill, r.height),
            2.0,
            ink,
        );
    }
    // The threshold, when On input is what arms it.
    if let Some(record) = &chrome.view.record
        && record.arm == fontelle_types::ArmMode::OnInput
    {
        let x = r.x + r.width * ((record.threshold_db + 60.0) / 60.0).clamp(0.0, 1.0);
        fill_rect(
            scene,
            Rect::new(x - 0.5, r.y - 2.0, 1.5, r.height + 4.0),
            p.text,
        );
    }
    stroke_rect_rounded(scene, r, 2.0, 1.0, p.border);
}

/// The Layout card's keyboard: where the slices land, lit and numbered.
fn draw_keyboard(
    scene: &mut Scene,
    theme: &Theme,
    labels: &Labels,
    chrome: &AnalyzeChrome<'_>,
    t: &BridgeType,
) {
    let p = &theme.palette;
    let board = chrome.layout.keyboard;
    if board.is_empty() {
        return;
    }
    let keys = &chrome.state.slice_keys;
    // The octaves that hold the slices, three at most, from a C.
    let low = keys.iter().map(|k| k.low).min().unwrap_or(48);
    let high = keys.iter().map(|k| k.high).max().unwrap_or(71);
    let first = (low / 12) * 12;
    let octaves = (u32::from(high.saturating_sub(first)) / 12 + 1).clamp(2, 4) as u8;
    let last = (first + octaves * 12).min(127);
    let naturals: Vec<u8> = (first..last)
        .filter(|k| !matches!(k % 12, 1 | 3 | 6 | 8 | 10))
        .collect();
    let key_w = board.width / naturals.len().max(1) as f32;
    let lit = |key: u8| keys.iter().position(|k| key >= k.low && key <= k.high);
    let ink = p.mod_macro;
    for (i, key) in naturals.iter().enumerate() {
        let r = Rect::new(
            board.x + i as f32 * key_w,
            board.y,
            key_w - 1.0,
            board.height,
        );
        let face = match lit(*key) {
            Some(_) => mix(p.key_white, ink, 0.55),
            None => mix(p.key_white, p.panel_header, 0.7),
        };
        fill_rect_rounded(scene, r, 2.0, face);
        if let Some(slice) = lit(*key)
            && keys[slice].low == *key
            && let Some(label) = labels.get_styled(&(keys[slice].slice + 1).to_string(), t.caption)
            && label.width < key_w
        {
            draw_text_clipped(
                scene,
                label,
                r,
                r.x + (r.width - label.width) / 2.0,
                r.bottom() - label.height - 2.0,
                darken(p.window, 0.3),
            );
        }
        if key % 12 == 0
            && let Some(label) = labels.get_styled(&crate::canvas::key_name(*key), t.caption)
            && label.width < key_w * 1.6
        {
            draw_text_clipped(
                scene,
                label,
                board,
                r.x + 2.0,
                r.y + 2.0,
                darken(p.window, 0.2).with_alpha(0xb0),
            );
        }
    }
    let mut seen = 0usize;
    for key in first..last {
        if !matches!(key % 12, 1 | 3 | 6 | 8 | 10) {
            seen += 1;
            continue;
        }
        let seam = board.x + seen as f32 * key_w;
        let w = key_w * 0.6;
        let r = Rect::new(seam - w / 2.0, board.y, w, board.height * 0.58);
        let face = match lit(key) {
            Some(_) => mix(p.key_black, ink, 0.7),
            None => p.key_black,
        };
        fill_rect_rounded(scene, r, 1.5, face);
    }
}

// ------------------------------------------------------- the wave lane ---

/// The Clean, Slice and Record pages' lane: the waveform the whole screen
/// wide, with what the page does to it drawn on it.
pub(super) fn draw_wave_lane(
    scene: &mut Scene,
    theme: &Theme,
    labels: &Labels,
    chrome: &AnalyzeChrome<'_>,
    t: &BridgeType,
) {
    let p = &theme.palette;
    let l = &chrome.layout;
    let lane = &l.lane;
    let view = chrome.view;
    let state = chrome.state;
    let ink = page_ink(state.page, p);
    let grid = lane.grid;
    if grid.is_empty() {
        return;
    }
    clip_to(scene, grid);
    for (x, _) in crate::canvas::ruler_labels(l, view, state) {
        fill_rect(
            scene,
            Rect::new(x.round(), grid.y, 1.0, grid.height),
            p.text.with_alpha(0x10),
        );
    }
    let (trim_a, trim_b) = view.trim_seconds();
    // The level the page sets, drawn on the wave: the gain, and the fades
    // from the trim's ends — what will be heard, at a glance.
    let rate = f64::from(view.rate.max(1));
    let fade_in = view.clean.fade_in as f64 / rate;
    let fade_out = view.clean.fade_out as f64 / rate;
    let gain = if state.page == AnalyzePage::Clean {
        10f32.powf(view.clean.gain_db / 20.0)
    } else {
        1.0
    };
    let shape = view.clean.fade_shape;
    let fade_gain = |x: f32| {
        let curve = |u: f32| match shape {
            fontelle_types::StudyFadeShape::Linear => u,
            fontelle_types::StudyFadeShape::Smooth => (u * std::f32::consts::FRAC_PI_2).sin(),
            fontelle_types::StudyFadeShape::Exponential => u * u,
        };
        let at = lane.t_of(state, x);
        let mut g = 1.0;
        if fade_in > 0.0 && at >= trim_a && at < trim_a + fade_in {
            g *= curve(((at - trim_a) / fade_in) as f32);
        }
        if fade_out > 0.0 && at <= trim_b && at > trim_b - fade_out {
            g *= curve(((trim_b - at) / fade_out) as f32);
        }
        g
    };
    let clean_page = state.page == AnalyzePage::Clean;
    if !view.peaks.is_empty() && view.peaks_per_second > 0.0 {
        let middle = grid.y + grid.height / 2.0;
        let half = grid.height * 0.46;
        let wave = mix(ink, p.text, 0.25);
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
            let kept = from >= trim_a && from <= trim_b;
            let g = if clean_page && kept {
                gain * fade_gain(x)
            } else {
                1.0
            };
            let top = middle - (high * g).clamp(0.0, 1.0) * half;
            let bottom = middle - (low * g).clamp(-1.0, 0.0) * half;
            let alpha = if kept { 0xb0 } else { 0x40 };
            fill_rect(
                scene,
                Rect::new(x, top, 1.0, (bottom - top).max(1.0)),
                wave.with_alpha(alpha),
            );
            x += 1.0;
        }
        fill_rect(
            scene,
            Rect::new(grid.x, middle.round(), grid.width, 1.0),
            p.text.with_alpha(0x18),
        );
    }
    // Past the end of the audio is nothing.
    let end = lane.x_of(state, view.duration);
    if end < grid.right() {
        fill_rect(
            scene,
            Rect::new(end, grid.y, grid.right() - end, grid.height),
            darken(p.window, 0.5).with_alpha(0x90),
        );
    }
    match state.page {
        AnalyzePage::Clean => draw_clean_marks(scene, theme, labels, chrome, t),
        AnalyzePage::Slice => draw_slice_marks(scene, theme, labels, chrome, t),
        _ => {}
    }
    // The span chosen on the lane.
    if let Some((a, b)) = state.span {
        let (x0, x1) = (lane.x_of(state, a), lane.x_of(state, b));
        let noise = state.tool == AnalyzeTool::Noise;
        let tint = if noise { p.mod_lfo } else { p.accent };
        fill_rect(
            scene,
            Rect::new(x0, grid.y, (x1 - x0).max(1.0), grid.height),
            tint.with_alpha(0x26),
        );
        for x in [x0, x1] {
            fill_rect(
                scene,
                Rect::new(x.round(), grid.y, 1.0, grid.height),
                tint.with_alpha(0xa0),
            );
        }
    }
    // The cursor and the playhead.
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
    if let Some(error) = &view.error {
        text_in(scene, labels, error, t.value, grid, None, p.text_muted);
    }
    scene.pop_layer();
    super::analyze::draw_ruler(scene, theme, labels, chrome);
}

/// The Clean page's marks: what the trim leaves out shaded, its ends as
/// handles, the fades' curves with their handles, the captured noise, and
/// the transients faint along the top.
fn draw_clean_marks(
    scene: &mut Scene,
    theme: &Theme,
    labels: &Labels,
    chrome: &AnalyzeChrome<'_>,
    t: &BridgeType,
) {
    let p = &theme.palette;
    let lane = &chrome.layout.lane;
    let view = chrome.view;
    let state = chrome.state;
    let grid = lane.grid;
    let s = chrome.layout.scale;
    let ink = p.mod_lfo;
    // Transients, faint, along the top: where the sound starts.
    for (at, strength) in &view.onsets {
        if *strength < 0.25 {
            continue;
        }
        let x = lane.x_of(state, *at);
        if x >= grid.x && x <= grid.right() {
            fill_rect(
                scene,
                Rect::new(x.round(), grid.y, 1.0, 6.0 * s * strength.min(1.0) + 2.0),
                p.text.with_alpha(0x40),
            );
        }
    }
    // The captured noise's span.
    if let (Some((a, b)), Some(noise)) = (state.noise_span, &view.clean.denoise.noise) {
        let (x0, x1) = (lane.x_of(state, a), lane.x_of(state, b));
        let band = Rect::new(x0, grid.y, (x1 - x0).max(1.0), grid.height);
        fill_rect(scene, band, ink.with_alpha(0x1c));
        let level = format!("{} dB", noise.level_db.round() as i32);
        if let Some(label) = labels.get_styled(&level, t.caption) {
            draw_text_clipped(
                scene,
                label,
                grid,
                x0 + 4.0,
                grid.bottom() - label.height - 4.0,
                lighten(ink, 0.3),
            );
        }
    }
    let (a, b) = view.trim_seconds();
    let (xa, xb) = (lane.x_of(state, a), lane.x_of(state, b));
    let shade = darken(p.window, 0.55).with_alpha(0xb0);
    if xa > grid.x {
        fill_rect(
            scene,
            Rect::new(grid.x, grid.y, xa - grid.x, grid.height),
            shade,
        );
    }
    if xb < grid.right() {
        fill_rect(
            scene,
            Rect::new(xb, grid.y, grid.right() - xb, grid.height),
            shade,
        );
    }
    // The fades' envelopes, over the kept span.
    let rate = f64::from(view.rate.max(1));
    let (fade_in, fade_out) = (
        view.clean.fade_in as f64 / rate,
        view.clean.fade_out as f64 / rate,
    );
    let curve = |u: f32| match view.clean.fade_shape {
        fontelle_types::StudyFadeShape::Linear => u,
        fontelle_types::StudyFadeShape::Smooth => (u * std::f32::consts::FRAC_PI_2).sin(),
        fontelle_types::StudyFadeShape::Exponential => u * u,
    };
    let top = grid.y + 8.0 * s;
    let foot = grid.bottom() - 2.0;
    for (from, length, rising) in [(a, fade_in, true), (b - fade_out, fade_out, false)] {
        if length <= 0.0 {
            continue;
        }
        let points: Vec<(f32, f32)> = (0..=32)
            .map(|i| {
                let u = i as f32 / 32.0;
                let g = curve(if rising { u } else { 1.0 - u });
                (
                    lane.x_of(state, from + length * f64::from(u)),
                    foot - (foot - top) * g,
                )
            })
            .collect();
        stroke_polyline(scene, &points, grid, 1.5, lighten(ink, 0.25));
    }
    // The handles: the trim's ends full height, the fades' along the top.
    let hover = state.hover;
    for (x, start) in [(xa, true), (xb, false)] {
        let hot = hover == Some(AnalyzeHit::TrimEnd(start));
        fill_rect(
            scene,
            Rect::new(x - 1.0, grid.y, 2.0, grid.height),
            if hot { lighten(ink, 0.4) } else { ink },
        );
        let grip = Rect::new(
            x - 3.0 * s,
            grid.y + grid.height / 2.0 - 14.0 * s,
            6.0 * s,
            28.0 * s,
        );
        fill_rect_rounded(
            scene,
            grip,
            3.0 * s,
            if hot { lighten(ink, 0.4) } else { ink },
        );
    }
    for (at, fade) in [(a + fade_in, true), (b - fade_out, false)] {
        let x = lane.x_of(state, at);
        let hot = hover == Some(AnalyzeHit::FadeEnd(fade));
        let centre = (x, grid.y + 9.0 * s);
        if hot {
            fill_glow(scene, centre, 10.0 * s, ink, 0x60);
        }
        scene.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            (if hot { lighten(ink, 0.4) } else { ink }).to_peniko(),
            None,
            &Circle::new(
                (f64::from(centre.0), f64::from(centre.1)),
                f64::from(4.0 * s),
            ),
        );
    }
}

/// The Slice page's marks: each slice tinted in turn and numbered, the
/// markers as flags, the auto-slice's cuts dashed.
fn draw_slice_marks(
    scene: &mut Scene,
    theme: &Theme,
    labels: &Labels,
    chrome: &AnalyzeChrome<'_>,
    t: &BridgeType,
) {
    let p = &theme.palette;
    let lane = &chrome.layout.lane;
    let view = chrome.view;
    let state = chrome.state;
    let grid = lane.grid;
    let s = chrome.layout.scale;
    let ink = p.mod_macro;
    let cuts = crate::canvas::analyze_cuts(view, state);
    let (a, b) = view.trim_seconds();
    let mut edges = vec![a];
    edges.extend(cuts.iter().copied());
    edges.push(b);
    for (i, pair) in edges.windows(2).enumerate() {
        let (x0, x1) = (lane.x_of(state, pair[0]), lane.x_of(state, pair[1]));
        if x1 < grid.x || x0 > grid.right() {
            continue;
        }
        if i % 2 == 0 {
            fill_rect(
                scene,
                Rect::new(x0, grid.y, (x1 - x0).max(0.0), grid.height),
                ink.with_alpha(0x12),
            );
        }
        let number = (i + 1).to_string();
        if let Some(label) = labels.get_styled(&number, t.caption)
            && x1 - x0 > label.width + 8.0
        {
            draw_text_clipped(
                scene,
                label,
                grid,
                x0 + 5.0,
                grid.y + 14.0 * s,
                lighten(ink, 0.35),
            );
        }
    }
    let auto = state.auto != crate::canvas::AutoSlice::Off;
    // The auto-slice's cuts, dashed: a preview until used.
    if auto {
        for cut in &cuts {
            let x = lane.x_of(state, *cut).round();
            let mut y = grid.y;
            while y < grid.bottom() {
                fill_rect(scene, Rect::new(x, y, 1.0, 5.0 * s), ink.with_alpha(0xc0));
                y += 9.0 * s;
            }
        }
    }
    // The markers: a line and a flag; the one in hand lit.
    for marker in &view.markers {
        let x = lane.x_of(state, view.seconds_of(marker.at)).round();
        if x < grid.x - 6.0 || x > grid.right() + 6.0 {
            continue;
        }
        let held = state.selected_marker == Some(marker.id);
        let hot = state.hover == Some(AnalyzeHit::Marker(marker.id));
        let face = if held || hot { lighten(ink, 0.4) } else { ink };
        let alpha = if auto { 0x60 } else { 0xff };
        if held {
            fill_glow(scene, (x, grid.y + grid.height / 2.0), 8.0, ink, 0x40);
        }
        fill_rect(
            scene,
            Rect::new(x - 0.5, grid.y, 1.5, grid.height),
            face.with_alpha(alpha),
        );
        let mut flag = BezPath::new();
        flag.move_to((f64::from(x), f64::from(grid.y)));
        flag.line_to((f64::from(x + 9.0 * s), f64::from(grid.y + 4.5 * s)));
        flag.line_to((f64::from(x), f64::from(grid.y + 9.0 * s)));
        flag.close_path();
        scene.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            face.with_alpha(alpha).to_peniko(),
            None,
            &flag,
        );
    }
}

// ------------------------------------------------------------ the takes ---

/// The Record page's screen below the lane: a row a take, the comp drawn
/// on them; with none yet, the lamp and what to do.
pub(super) fn draw_takes(
    scene: &mut Scene,
    theme: &Theme,
    labels: &Labels,
    chrome: &AnalyzeChrome<'_>,
    t: &BridgeType,
) {
    let p = &theme.palette;
    let l = &chrome.layout;
    let view = chrome.view;
    let state = chrome.state;
    let area = l.takes_area;
    if area.is_empty() {
        return;
    }
    let ink = p.meter_peak;
    if view.takes.is_empty() {
        let [first, second] = crate::canvas::no_takes_lines(view);
        let s = l.scale;
        let mid = area.y + area.height / 2.0;
        let armed = view.record.as_ref().is_some_and(|r| r.armed);
        let recording = state.meter.recording;
        let r = 26.0 * s;
        let centre = (area.x + area.width / 2.0, mid - 30.0 * s);
        if armed || recording {
            fill_glow(
                scene,
                centre,
                r * 2.5,
                ink,
                if recording { 0x80 } else { 0x40 },
            );
        }
        scene.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            (if recording {
                lighten(ink, 0.1)
            } else if armed {
                ink
            } else {
                mix(ink, p.window, 0.7)
            })
            .to_peniko(),
            None,
            &Circle::new((f64::from(centre.0), f64::from(centre.1)), f64::from(r)),
        );
        let line = |y: f32| Rect::new(area.x, y, area.width, 20.0 * s);
        text_in(
            scene,
            labels,
            &first,
            t.value,
            line(mid + 8.0 * s),
            None,
            p.text,
        );
        text_in(
            scene,
            labels,
            &second,
            t.value,
            line(mid + 30.0 * s),
            None,
            p.text_muted,
        );
        return;
    }
    let longest = crate::canvas::takes_length(view).max(1e-6);
    for (index, row) in l.takes.iter().enumerate() {
        let Some(take) = view.takes.iter().find(|t| t.id == row.id) else {
            continue;
        };
        let current = view.current_take == Some(take.id);
        let hot = matches!(
            state.hover,
            Some(AnalyzeHit::TakeName(id) | AnalyzeHit::TakeLane(id) | AnalyzeHit::TakeStar(id) | AnalyzeHit::TakeDiscard(id)) if id == take.id
        );
        fill_rect_rounded(
            scene,
            row.row,
            3.0,
            if current {
                ink.with_alpha(0x2a)
            } else if hot {
                p.text.with_alpha(0x10)
            } else if index % 2 == 0 {
                p.text.with_alpha(0x06)
            } else {
                Color([0, 0, 0, 0])
            },
        );
        if current {
            fill_rect(
                scene,
                Rect::new(row.row.x, row.row.y, 3.0, row.row.height),
                ink,
            );
        }
        // ☆ / ★, the name, its length, ×.
        let star = if take.starred { "\u{2605}" } else { "\u{2606}" };
        text_in(
            scene,
            labels,
            star,
            t.value,
            row.star,
            None,
            if take.starred {
                p.param_automated
            } else if state.hover == Some(AnalyzeHit::TakeStar(take.id)) {
                p.text
            } else {
                p.text_muted
            },
        );
        let length = crate::canvas::take_length_text(take);
        let length_w = labels
            .get_styled(&length, t.caption)
            .map_or(0.0, |s| s.width)
            + 6.0;
        let name_box = Rect::new(
            row.name.x,
            row.name.y,
            (row.name.width - length_w).max(0.0),
            row.name.height,
        );
        text_in(
            scene,
            labels,
            &crate::canvas::take_name_text(take, state),
            t.value,
            name_box,
            Some(0.0),
            if current {
                p.text
            } else {
                lighten(p.text_muted, 0.15)
            },
        );
        text_in(
            scene,
            labels,
            &length,
            t.caption,
            Rect::new(name_box.right(), row.name.y, length_w, row.name.height),
            Some(2.0),
            p.text_muted,
        );
        if hot {
            text_in(
                scene,
                labels,
                "\u{00d7}",
                t.value,
                row.discard,
                None,
                if state.hover == Some(AnalyzeHit::TakeDiscard(take.id)) {
                    p.meter_peak
                } else {
                    p.text_muted
                },
            );
        }
        // The take's audio, across the takes' shared time.
        let lane = row.lane;
        fill_rect_rounded(scene, lane, 2.0, darken(p.window, 0.35).with_alpha(0xc0));
        if let Some(peaks) = view
            .take_peaks
            .get(view.takes.iter().position(|t| t.id == take.id).unwrap_or(0))
            && !peaks.is_empty()
        {
            let seconds = take.frames as f64 / f64::from(take.sample_rate.max(1));
            let end = crate::canvas::take_x_of(view, lane, seconds);
            let middle = lane.y + lane.height / 2.0;
            let half = lane.height * 0.42;
            let wave = mix(ink, p.text, 0.35).with_alpha(if current { 0xb0 } else { 0x70 });
            let mut x = lane.x;
            while x < end.min(lane.right()) {
                let from = f64::from((x - lane.x) / lane.width) * longest;
                let to = f64::from((x + 1.0 - lane.x) / lane.width) * longest;
                let a = (from * view.peaks_per_second).floor().max(0.0) as usize;
                let b = ((to * view.peaks_per_second).ceil() as usize).max(a + 1);
                if a >= peaks.len() {
                    break;
                }
                let (mut low, mut high) = (0.0f32, 0.0f32);
                for (lo, hi) in &peaks[a..b.min(peaks.len())] {
                    low = low.min(*lo);
                    high = high.max(*hi);
                }
                let top = middle - high.clamp(0.0, 1.0) * half;
                let bottom = middle - low.clamp(-1.0, 0.0) * half;
                fill_rect(scene, Rect::new(x, top, 1.0, (bottom - top).max(1.0)), wave);
                x += 1.0;
            }
        }
        // This take's part of the comp.
        let rate = f64::from(take.sample_rate.max(1));
        for span in view.comp.iter().filter(|s| s.take == take.id) {
            let x0 = crate::canvas::take_x_of(view, lane, span.start as f64 / rate);
            let x1 = crate::canvas::take_x_of(view, lane, span.end as f64 / rate);
            let r = Rect::new(x0, lane.y + 1.0, (x1 - x0).max(1.0), lane.height - 2.0);
            fill_rect_rounded(scene, r, 2.0, p.accent.with_alpha(0x34));
            stroke_rect_rounded(scene, r, 2.0, 1.0, lighten(p.accent, 0.3));
        }
    }
}

/// The header's Studies chip: its words and a wedge.
pub(super) fn draw_studies_chip(
    scene: &mut Scene,
    theme: &Theme,
    labels: &Labels,
    chrome: &AnalyzeChrome<'_>,
    t: &BridgeType,
) {
    let p = &theme.palette;
    let l = &chrome.layout;
    if l.studies.is_empty() {
        return;
    }
    let hot = chrome.state.hover == Some(AnalyzeHit::Studies);
    super::analyze::plate(scene, theme, l.studies, hot);
    let icon_w = 24.0 * l.scale;
    let words = Rect::new(
        l.studies.x,
        l.studies.y,
        l.studies.width - icon_w,
        l.studies.height,
    );
    text_in(
        scene,
        labels,
        crate::canvas::STUDIES,
        t.value,
        words,
        Some(10.0 * l.scale),
        if hot { p.text } else { p.text_muted },
    );
    super::analyze::chevron(
        scene,
        Rect::new(
            l.studies.right() - icon_w,
            l.studies.y,
            icon_w,
            l.studies.height,
        ),
        if hot {
            lighten(p.accent, 0.4)
        } else {
            p.text_muted
        },
        l.scale,
    );
}
