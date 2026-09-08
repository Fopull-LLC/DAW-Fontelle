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

#[test]
fn a_label_can_be_shaped_small_beside_its_full_size() {
    // Flopsynth's knob captions are eleven pixels where the chrome's are
    // thirteen: a hundred and thirty controls do not fit at the chrome's
    // size, and "mod from" in a fifty-pixel cell does not either. The same
    // string is kept at both sizes, and each is asked for by name.
    if !has_fonts() {
        return;
    }
    let theme = Theme::dark_default();
    let mut ctx = text().lock().expect("font system");
    let mut labels = fontelle_ui::text::Labels::new();
    labels.ensure("mod from", &theme.font, &mut ctx);
    labels.ensure_small("mod from", &theme.font, &mut ctx);
    let full = labels.get("mod from").expect("shaped at full size");
    let small = labels.get_small("mod from").expect("shaped small");
    assert!(
        small.width < full.width && small.height < full.height,
        "small is {}x{} and full is {}x{}",
        small.width,
        small.height,
        full.width,
        full.height
    );
    assert!(
        labels.get_small("output").is_none(),
        "nobody asked for that one small"
    );
}

#[test]
fn a_frame_that_needs_more_labels_than_the_cache_holds_keeps_every_one() {
    // The reported symptom: *"the text constantly keeps flickering while
    // trying to navigate the app."* The cache was emptied wholesale the
    // moment it filled — in the middle of a frame's shaping pass — so every
    // string shaped *before* that moment was gone by the time the renderer
    // looked it up, and drew as nothing for that one frame. A window whose
    // working set sits near the cap did it on nearly every frame.
    //
    // A string shaped for this frame must still be there when this frame is
    // drawn, however many of them there are.
    if !has_fonts() {
        return;
    }
    let theme = Theme::dark_default();
    let mut ctx = text().lock().expect("font system");
    let mut labels = fontelle_ui::text::Labels::new();
    labels.begin_frame();
    let names: Vec<String> = (0..700).map(|i| format!("Preset {i}")).collect();
    for name in &names {
        labels.ensure(name, &theme.font, &mut ctx);
        labels.ensure_small(name, &theme.font, &mut ctx);
    }
    for name in &names {
        assert!(
            labels.get(name).is_some(),
            "{name} was shaped this frame and was gone before it was drawn"
        );
        assert!(
            labels.get_small(name).is_some(),
            "{name} was shaped small this frame and was gone before it was drawn"
        );
    }
}

#[test]
fn labels_from_an_earlier_frame_are_let_go_when_the_cache_is_full() {
    // The other half of the bargain: scrolling through a big collection
    // must not accumulate a shaped layout for every name ever seen. When the
    // cache fills, what goes is what *this* frame has not asked for.
    if !has_fonts() {
        return;
    }
    let theme = Theme::dark_default();
    let mut ctx = text().lock().expect("font system");
    let mut labels = fontelle_ui::text::Labels::new();
    labels.begin_frame();
    for i in 0..400 {
        labels.ensure(&format!("Old {i}"), &theme.font, &mut ctx);
    }
    labels.begin_frame();
    let fresh: Vec<String> = (0..400).map(|i| format!("New {i}")).collect();
    for name in &fresh {
        labels.ensure(name, &theme.font, &mut ctx);
    }
    for name in &fresh {
        assert!(
            labels.get(name).is_some(),
            "{name} is this frame's and must stay"
        );
    }
    assert!(
        labels.len() < 800,
        "{} strings kept: the frame before this one was never let go",
        labels.len()
    );
    assert!(
        labels.get("Old 0").is_none(),
        "a string nobody has asked for since the cache filled is still in it"
    );
}
