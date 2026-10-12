//! The Fontelle Singer's instrument panel, drawn: a stage with the singer
//! on it, lit from behind, over cards of controls.
//!
//! Its own look, not Flopsynth's ship: a singer is a person on a stage, so
//! the top of the window is a spotlight on their portrait — the name big,
//! the instrument and kind over it, the languages and terms as chips — and
//! the controls under it sit in cards, each with a coloured nameplate
//! (Voice rose, Expression teal, Performance violet, Output amber). The
//! Expression card draws the line a note is sung on with the auto vibrato
//! and glide it gives, so a turned knob is seen as well as heard. A control
//! the voice ignores is drawn faded, and its card says why.
//!
//! Every string drawn here is listed by [`singer_panel_strings`], which the
//! window shapes ahead of the frame.

use vello::Scene;
use vello::kurbo::{Affine, BezPath, Point, Stroke};
use vello::peniko::{BlendMode, Fill};

use super::vocal::{
    PICKER_BADGE_STYLE, PICKER_TEXT_STYLE, PORTRAIT_INITIAL_STYLE, chip, draw_portrait, initial,
};
use super::{
    draw_param_cell, draw_text_clipped, fill_glow, fill_rect, fill_rect_rounded,
    fill_rect_vertical, mix, stroke_rect_rounded,
};
use crate::canvas::{
    InstrumentView, SingerPanelButton, SingerPanelHit, SingerPanelLayout, singer_footnote,
};
use crate::document::{SingerKind, SingerPanelView};
use crate::layout::Rect;
use crate::text::{Labels, TextStyle};
use crate::theme::{Color, Theme};

/// The instrument and kind over the name.
pub const SINGER_KICKER_STYLE: TextStyle = TextStyle::from_tenths(115, 600);
/// The singer's name on the stage.
pub const SINGER_NAME_STYLE: TextStyle = TextStyle::from_tenths(260, 600);
/// A card's nameplate.
pub const SINGER_CARD_STYLE: TextStyle = TextStyle::from_tenths(135, 600);
/// The line under a card saying why a control is greyed.
pub const SINGER_NOTE_STYLE: TextStyle = TextStyle::from_tenths(115, 400);

/// What the singer panel's window draws.
pub struct SingerChrome<'a> {
    pub layout: SingerPanelLayout,
    pub view: &'a SingerPanelView,
    pub controls: &'a InstrumentView,
    pub portraits: &'a crate::portrait::Portraits,
    /// Which control the pointer is over, and which is being dragged.
    pub hover: Option<(usize, usize)>,
    pub active: Option<(usize, usize)>,
    /// Which piece of the stage the pointer is over.
    pub hover_hit: Option<SingerPanelHit>,
}

/// Each section's colour, by its name; others the accent.
fn section_ink(name: &str, accent: Color) -> Color {
    match name {
        "Voice" => Color::rgba(0xe0, 0x6c, 0x9f, 0xff),
        "Expression" => Color::rgba(0x3f, 0xb8, 0xaf, 0xff),
        "Performance" => Color::rgba(0x8f, 0x74, 0xe6, 0xff),
        "Output" => Color::rgba(0xe0, 0xa4, 0x4c, 0xff),
        _ => accent,
    }
}

fn kind_label(kind: SingerKind) -> &'static str {
    match kind {
        SingerKind::Utau => "UTAU",
        SingerKind::DiffSinger => "DiffSinger",
        SingerKind::Engine => "Engine",
    }
}

/// "FONTELLE SINGER · UTAU VOICEBANK".
fn kicker(view: &SingerPanelView) -> String {
    let instrument = view.instrument.to_uppercase();
    match &view.singer {
        Some(s) if s.kind.is_voicebank() => format!(
            "{instrument} \u{b7} {} VOICEBANK",
            kind_label(s.kind).to_uppercase()
        ),
        Some(s) if s.research_only => format!("{instrument} \u{b7} RESEARCH ONLY"),
        None if view.missing.is_some() => format!("{instrument} \u{b7} NOT INSTALLED HERE"),
        _ => instrument,
    }
}

/// What the stage's big line says: the singer, or what to do.
fn headline(view: &SingerPanelView) -> String {
    match (&view.singer, &view.missing) {
        (Some(s), _) => s.name.clone(),
        (None, Some(missing)) => missing.clone(),
        (None, None) => "No singer yet".to_string(),
    }
}

/// The line under the name: the author and the bank's folder, or the
/// message when there is one.
fn details(view: &SingerPanelView) -> String {
    if let Some(message) = &view.message {
        return message.clone();
    }
    let mut parts = Vec::new();
    if let Some(author) = &view.author {
        parts.push(format!("by {author}"));
    }
    if let Some(s) = &view.singer
        && let Some(folder) = s.folder.as_deref().filter(|f| *f != s.name)
    {
        parts.push(folder.to_string());
    }
    if parts.is_empty()
        && let Some(s) = &view.singer
    {
        parts.push(s.engine_name.clone());
    }
    parts.join("  \u{b7}  ")
}

/// The terms chip's word.
fn terms_word(view: &SingerPanelView) -> &'static str {
    match &view.singer {
        Some(s) if s.terms_agreed => "Terms agreed",
        Some(s) if s.terms_foreign => "Terms \u{2014} translate",
        _ => "Terms to read",
    }
}

/// Every string the panel draws and the style it is drawn at, for the
/// window to shape ahead of the frame.
pub fn singer_panel_strings(
    view: &SingerPanelView,
    controls: &InstrumentView,
) -> Vec<(String, TextStyle)> {
    let mut out = vec![
        (kicker(view), SINGER_KICKER_STYLE),
        (headline(view), SINGER_NAME_STYLE),
        (details(view), PICKER_TEXT_STYLE),
        (terms_word(view).to_string(), PICKER_BADGE_STYLE),
        (initial(&headline(view)), PORTRAIT_INITIAL_STYLE),
    ];
    for b in [
        SingerPanelButton::ChangeSinger,
        SingerPanelButton::Sample,
        SingerPanelButton::OpenEditor,
        SingerPanelButton::Voicebanks,
        SingerPanelButton::FindSingers,
    ] {
        out.push((b.label(&view.instrument), PICKER_TEXT_STYLE));
    }
    if let Some(s) = &view.singer {
        for tag in &s.languages {
            out.push((tag.to_uppercase(), PICKER_BADGE_STYLE));
        }
    }
    for (g, group) in controls.groups.iter().enumerate() {
        out.push((group.name.clone(), SINGER_CARD_STYLE));
        if let Some(note) = singer_footnote(view, controls, g) {
            out.push((note, SINGER_NOTE_STYLE));
        }
    }
    out
}

/// The panel: the stage, then the cards, clipped to the window's body so
/// what is scrolled away is not drawn over the title.
pub fn draw_singer_panel(scene: &mut Scene, theme: &Theme, labels: &Labels, chrome: &SingerChrome) {
    let l = &chrome.layout;
    if l.body.is_empty() {
        return;
    }
    let p = &theme.palette;
    // The room: a wash from the stage's light down to the floor.
    fill_rect_vertical(scene, l.body, 0.0, mix(p.window, p.accent, 0.06), p.window);
    scene.push_layer(
        Fill::NonZero,
        BlendMode::default(),
        1.0,
        Affine::IDENTITY,
        &vello::kurbo::Rect::new(
            l.body.x as f64,
            l.body.y as f64,
            l.body.right() as f64,
            l.body.bottom() as f64,
        ),
    );
    draw_stage(scene, theme, labels, chrome);
    for (g, card) in &l.cards {
        draw_card(scene, theme, labels, chrome, *g, *card);
    }
    scene.pop_layer();
}

fn draw_stage(scene: &mut Scene, theme: &Theme, labels: &Labels, chrome: &SingerChrome) {
    let p = &theme.palette;
    let l = &chrome.layout;
    let view = chrome.view;
    let stage = l.header;
    // The stage itself: a deep panel, lit from the portrait's side.
    fill_rect_vertical(
        scene,
        stage,
        14.0,
        mix(p.panel_header, p.accent, 0.10),
        mix(p.panel_header, p.window, 0.35),
    );
    // The spotlight behind the singer.
    let centre = (
        l.portrait.x + l.portrait.width / 2.0,
        l.portrait.y + l.portrait.height / 2.0,
    );
    fill_glow(scene, centre, l.portrait.width * 1.25, p.accent, 70);
    stroke_rect_rounded(scene, stage, 14.0, 1.0, mix(p.border, p.accent, 0.3));

    // The portrait, framed.
    let image = view
        .singer
        .as_ref()
        .and_then(|s| s.image.as_deref())
        .and_then(|path| chrome.portraits.peek(path));
    let hot_portrait = chrome.hover_hit == Some(SingerPanelHit::Portrait);
    draw_portrait(scene, labels, image, l.portrait, &headline(view));
    stroke_rect_rounded(
        scene,
        l.portrait.inset(-2.0),
        14.0,
        2.0,
        if hot_portrait {
            p.accent
        } else {
            mix(p.border, p.text, 0.25)
        },
    );

    // The words.
    // Each drawn a third of the way to the theme's text, so it reads on a
    // light theme's stage as well as a dark one's.
    let kind_ink = match view.singer.as_ref().map(|s| s.kind) {
        Some(SingerKind::Utau) => mix(Color::rgba(0x6f, 0xa8, 0xf0, 0xff), p.text, 0.33),
        Some(SingerKind::DiffSinger) => mix(Color::rgba(0xb0, 0x8c, 0xf5, 0xff), p.text, 0.33),
        _ => p.accent,
    };
    for (rect, text, style, ink) in [
        (l.kicker, kicker(view), SINGER_KICKER_STYLE, kind_ink),
        (l.name, headline(view), SINGER_NAME_STYLE, p.text),
        (
            l.details,
            details(view),
            PICKER_TEXT_STYLE,
            if view.message.is_some() {
                mix(Color::rgba(0xf0, 0xb0, 0x5a, 0xff), p.text, 0.33)
            } else {
                p.text_muted
            },
        ),
    ] {
        if let Some(t) = labels.get_styled(&text, style) {
            draw_text_clipped(
                scene,
                t,
                rect,
                rect.x,
                rect.y + ((rect.height - t.height) / 2.0).max(0.0),
                ink,
            );
        }
    }

    // A chip a language, the one it sings in lit.
    if let Some(s) = &view.singer {
        for (n, rect) in &l.languages {
            let Some(tag) = s.languages.get(*n) else {
                continue;
            };
            let current = *tag == view.language;
            let hot = chrome.hover_hit == Some(SingerPanelHit::Language(*n));
            let (fill, ink) = if current {
                (p.accent, p.window)
            } else if hot {
                (p.row_accidental, p.text)
            } else {
                (mix(p.window, p.panel_header, 0.4), p.text)
            };
            chip(scene, labels, *rect, &tag.to_uppercase(), fill, ink);
        }
    }
    if let Some(rect) = l.terms {
        let agreed = view.singer.as_ref().is_some_and(|s| s.terms_agreed);
        let fill = if agreed {
            mix(p.window, p.panel_header, 0.4)
        } else {
            Color::rgba(0x6b, 0x4a, 0x16, 0xff)
        };
        let fill = if chrome.hover_hit == Some(SingerPanelHit::Terms) {
            mix(fill, p.text, 0.15)
        } else {
            fill
        };
        let ink = if agreed {
            p.text_muted
        } else {
            Color::rgba(0xff, 0xd7, 0x8a, 0xff)
        };
        chip(scene, labels, rect, terms_word(view), fill, ink);
    }

    // The buttons: Change singer… is the one most come here for.
    for (b, rect) in &l.buttons {
        let primary = *b == SingerPanelButton::ChangeSinger
            || (*b == SingerPanelButton::OpenEditor && !view.editor_open);
        let hot = chrome.hover_hit == Some(SingerPanelHit::Button(*b));
        let fill = match (primary, hot) {
            (true, false) => p.accent,
            (true, true) => mix(p.accent, p.text, 0.2),
            (false, false) => mix(p.window, p.panel_header, 0.5),
            (false, true) => p.row_accidental,
        };
        fill_rect_rounded(scene, *rect, 7.0, fill);
        stroke_rect_rounded(scene, *rect, 7.0, 1.0, mix(p.border, fill, 0.5));
        let label = b.label(&view.instrument);
        if let Some(t) = labels.get_styled(&label, PICKER_TEXT_STYLE) {
            draw_text_clipped(
                scene,
                t,
                *rect,
                rect.x + ((rect.width - t.width) / 2.0).max(6.0),
                rect.y + ((rect.height - t.height) / 2.0).max(0.0),
                if primary { p.window } else { p.text },
            );
        }
    }
}

fn draw_card(
    scene: &mut Scene,
    theme: &Theme,
    labels: &Labels,
    chrome: &SingerChrome,
    g: usize,
    card: Rect,
) {
    let p = &theme.palette;
    let l = &chrome.layout;
    let Some(group) = chrome.controls.groups.get(g) else {
        return;
    };
    if card.bottom() < l.body.y || card.y > l.body.bottom() {
        return;
    }
    let ink = section_ink(&group.name, p.accent);
    fill_rect_vertical(
        scene,
        card,
        10.0,
        mix(p.panel_header, ink, 0.05),
        mix(p.panel_header, p.window, 0.25),
    );
    stroke_rect_rounded(scene, card, 10.0, 1.0, mix(p.border, ink, 0.25));
    // The nameplate: a bar of the section's colour and its name.
    let heading = Rect::new(card.x, card.y, card.width, crate::canvas::CARD_HEADING);
    fill_rect_rounded(
        scene,
        Rect::new(
            heading.x + 12.0,
            heading.y + 9.0,
            4.0,
            heading.height - 16.0,
        ),
        2.0,
        ink,
    );
    if let Some(t) = labels.get_styled(&group.name, SINGER_CARD_STYLE) {
        draw_text_clipped(
            scene,
            t,
            heading,
            heading.x + 24.0,
            heading.y + ((heading.height - t.height) / 2.0).max(0.0) + 1.0,
            p.text,
        );
    }
    fill_rect(
        scene,
        Rect::new(
            heading.x + 12.0,
            heading.bottom() - 1.0,
            heading.width - 24.0,
            1.0,
        ),
        mix(p.border, ink, 0.2),
    );

    if let Some(picture) = l.picture
        && card.contains(picture.x + 1.0, picture.y + 1.0)
    {
        draw_picture(scene, theme, chrome.view, picture, ink);
    }

    for (cg, n, cell) in &l.controls.cells {
        if *cg != g {
            continue;
        }
        let Some(param) = group.params.get(*n) else {
            continue;
        };
        let hot = chrome.active == Some((g, *n));
        let lit = hot || chrome.hover == Some((g, *n));
        let faded = l.greyed.contains(&(g, *n));
        if faded {
            scene.push_layer(
                Fill::NonZero,
                BlendMode::default(),
                0.35,
                Affine::IDENTITY,
                &vello::kurbo::Rect::new(
                    cell.x as f64,
                    cell.y as f64,
                    cell.right() as f64,
                    cell.bottom() as f64,
                ),
            );
        }
        draw_param_cell(scene, theme, labels, param, *cell, hot, lit);
        if faded {
            scene.pop_layer();
        }
    }

    if let Some((_, note)) = l.footnotes.iter().find(|(fg, _)| *fg == g)
        && let Some(text) = singer_footnote(chrome.view, chrome.controls, g)
        && let Some(t) = labels.get_styled(&text, SINGER_NOTE_STYLE)
    {
        draw_text_clipped(
            scene,
            t,
            *note,
            note.x + 4.0,
            note.y + ((note.height - t.height) / 2.0).max(0.0),
            p.text_muted,
        );
    }
}

/// The Expression card's picture: a note's pitch as it will be sung — the
/// glide up into it from the note before, then the vibrato after its
/// delay, fading in and out — or a flat line when both are off.
fn draw_picture(scene: &mut Scene, theme: &Theme, view: &SingerPanelView, r: Rect, ink: Color) {
    let p = &theme.palette;
    fill_rect_rounded(scene, r, 8.0, mix(p.window, ink, 0.06));
    stroke_rect_rounded(scene, r, 8.0, 1.0, mix(p.border, ink, 0.15));
    let area = r.inset(10.0);
    if area.is_empty() {
        return;
    }
    // Two notes: a short one low on the left, the long one it glides to.
    let low_y = area.bottom() - area.height * 0.18;
    let high_y = area.y + area.height * 0.48;
    let split = area.x + area.width * 0.18;
    let bar = |x0: f32, x1: f32, y: f32| Rect::new(x0, y - 4.0, (x1 - x0).max(0.0), 8.0);
    fill_rect_rounded(
        scene,
        bar(area.x, split - 3.0, low_y),
        3.0,
        mix(p.border, ink, 0.35),
    );
    fill_rect_rounded(
        scene,
        bar(split, area.right(), high_y),
        3.0,
        mix(p.border, ink, 0.35),
    );

    // The line: seconds across the long note, at a scale where the
    // default vibrato shows a few cycles.
    let seconds = 1.2_f32;
    let px_per_second = (area.right() - split) / seconds;
    let glide = view.glide.map(|g| g.length.max(0.0) * px_per_second);
    let mut points = Vec::new();
    let mut x = area.x;
    while x <= area.right() {
        let mut y = if x < split { low_y } else { high_y };
        if let Some(width) = glide.filter(|w| *w > 0.5) {
            let from = split - width * view.glide.map_or(0.5, |g| g.position.clamp(0.0, 1.0));
            if x >= from && x <= from + width {
                let t = (x - from) / width;
                let eased = match view.glide.map(|g| g.shape) {
                    Some(fontelle_sing::GlideShape::Linear) => t,
                    Some(fontelle_sing::GlideShape::Out) => 1.0 - (1.0 - t).powi(2),
                    Some(fontelle_sing::GlideShape::In) => t * t,
                    _ => t * t * (3.0 - 2.0 * t),
                };
                y = low_y + (high_y - low_y) * eased;
            }
        } else if x >= split {
            y = high_y;
        }
        if let Some(v) = view.vibrato
            && x > split
        {
            let along = (x - split) / (area.right() - split);
            let start = v.delay.clamp(0.0, 1.0);
            if along > start {
                let span = (1.0 - start).max(1e-3);
                let into = (along - start) / span;
                let fade_in = if v.fade_in > 0.0 {
                    (into / v.fade_in).min(1.0)
                } else {
                    1.0
                };
                let fade_out = if v.fade_out > 0.0 {
                    ((1.0 - into) / v.fade_out).min(1.0)
                } else {
                    1.0
                };
                let t = (x - split) / px_per_second;
                // A semitone is the gap between the two bars' rows, drawn
                // half again as tall so a light vibrato is still seen.
                let cents =
                    v.depth * fade_in.min(fade_out) * (t * v.rate * std::f32::consts::TAU).sin();
                y -= cents / 100.0 * (low_y - high_y) * 1.5;
            }
        }
        points.push((x, y.clamp(r.y + 2.0, r.bottom() - 2.0)));
        x += 1.5;
    }
    let mut path = BezPath::new();
    for (i, (x, y)) in points.iter().enumerate() {
        let pt = Point::new(f64::from(*x), f64::from(*y));
        if i == 0 {
            path.move_to(pt);
        } else {
            path.line_to(pt);
        }
    }
    scene.stroke(
        &Stroke::new(2.5),
        Affine::IDENTITY,
        ink.to_peniko(),
        None,
        &path,
    );
}
