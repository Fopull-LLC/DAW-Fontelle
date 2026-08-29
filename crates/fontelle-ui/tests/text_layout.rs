//! Shaping is `cosmic-text`'s job; deciding *where the glyphs go* is ours, and
//! it needs no GPU, so it is tested here (`docs/first-usable-plan.md` §2.5).

use std::sync::{Mutex, OnceLock};

use fontelle_ui::text::TextContext;
use fontelle_ui::theme::Theme;

/// One `FontSystem` for the whole file: constructing it scans the system font
/// directories, which costs more than every assertion below put together.
fn text() -> &'static Mutex<TextContext> {
    static CTX: OnceLock<Mutex<TextContext>> = OnceLock::new();
    CTX.get_or_init(|| Mutex::new(TextContext::new()))
}

/// True when this machine has fonts at all. A container with none is not a
/// failing build, but it is also not a passing text test.
fn has_fonts() -> bool {
    let mut ctx = text().lock().expect("font system");
    !ctx.layout("M", &Theme::dark_default().font, None)
        .is_empty()
}

#[test]
fn a_string_lays_out_one_glyph_per_character() {
    if !has_fonts() {
        eprintln!("skipping: no system fonts on this machine");
        return;
    }
    let font = Theme::dark_default().font;
    let mut ctx = text().lock().expect("font system");
    let layout = ctx.layout("Fontelle", &font, None);

    assert_eq!(
        layout.glyph_count(),
        8,
        "eight plain Latin letters should shape to eight glyphs"
    );
    assert!(layout.width > 0.0 && layout.height > 0.0);
}

#[test]
fn glyphs_advance_left_to_right() {
    if !has_fonts() {
        return;
    }
    let font = Theme::dark_default().font;
    let mut ctx = text().lock().expect("font system");
    let layout = ctx.layout("Fontelle", &font, None);

    let xs: Vec<f32> = layout
        .runs
        .iter()
        .flat_map(|run| run.glyphs.iter().map(|g| g.x))
        .collect();
    assert!(
        xs.windows(2).all(|w| w[1] > w[0]),
        "glyph x positions are not increasing: {xs:?}"
    );
    assert_eq!(
        xs.first().copied(),
        Some(0.0),
        "the first glyph is not at the origin"
    );
}

#[test]
fn an_empty_string_draws_nothing() {
    let font = Theme::dark_default().font;
    let mut ctx = text().lock().expect("font system");
    let layout = ctx.layout("", &font, None);
    assert!(layout.is_empty());
    assert_eq!(layout.glyph_count(), 0);
    assert_eq!(layout.width, 0.0);
}

#[test]
fn a_bigger_font_size_makes_wider_text() {
    if !has_fonts() {
        return;
    }
    let mut small = Theme::dark_default().font;
    small.size = 12.0;
    let mut large = small.clone();
    large.size = 24.0;

    let mut ctx = text().lock().expect("font system");
    let a = ctx.layout("Fontelle", &small, None).width;
    let b = ctx.layout("Fontelle", &large, None).width;
    assert!(b > a, "24pt ({b}) is not wider than 12pt ({a})");
}

#[test]
fn glyphs_are_grouped_by_font_so_each_group_is_one_draw_call() {
    if !has_fonts() {
        return;
    }
    // vello draws glyphs one font at a time. Grouping here rather than at the
    // call site is what keeps a line of chrome text to a single `draw_glyphs`
    // when it is all one face, which it almost always is.
    let font = Theme::dark_default().font;
    let mut ctx = text().lock().expect("font system");
    let layout = ctx.layout("Fontelle", &font, None);
    assert_eq!(
        layout.runs.len(),
        1,
        "plain ASCII split across several fonts"
    );
    assert_eq!(layout.runs[0].font_size, font.size);
}

#[test]
fn text_wraps_when_it_is_given_a_width_to_wrap_in() {
    if !has_fonts() {
        return;
    }
    let font = Theme::dark_default().font;
    let mut ctx = text().lock().expect("font system");
    let unwrapped = ctx.layout("Fontelle the digital audio workstation", &font, None);
    let wrapped = ctx.layout(
        "Fontelle the digital audio workstation",
        &font,
        Some(unwrapped.width / 3.0),
    );

    assert!(
        wrapped.height > unwrapped.height,
        "constrained text did not wrap"
    );
    assert!(wrapped.width <= unwrapped.width / 3.0 + 1.0);
}
