//! Making a theme inside the studio (next update, phase 5): Settings ▸
//! Appearance ▸ *Edit this theme…* opens a card beside the studio, every
//! change shows on the studio as it is made, Save keeps it as the user's own
//! look, and Close without saving puts the old look back.

mod common;

use fontelle_ui::Windowless;
use fontelle_ui::canvas::ThemeEditorHit;
use fontelle_ui::layout::Rect;
use fontelle_ui::theme::{Layer, Theme};

fn studio() -> Windowless {
    Windowless::new(
        Box::new(common::a_session_for(common::a_clip_project(4))),
        Theme::dark_default(),
        (1600, 1000),
    )
}

fn centre(r: Rect) -> (f32, f32) {
    (r.x + r.width / 2.0, r.y + r.height / 2.0)
}

fn click(w: &mut Windowless, r: Rect) {
    let (x, y) = centre(r);
    w.click_studio(x, y);
}

fn open_editor(w: &mut Windowless) {
    let index = w
        .document()
        .settings()
        .iter()
        .position(|row| row.name.contains("Edit this theme"))
        .expect("Appearance has an Edit this theme row");
    w.press_setting(index);
    assert!(w.theme_editor().is_some(), "the card opens");
}

/// Accent, made pure red on the card: the hue strip's top, then the
/// square's top right corner.
fn make_accent_red(w: &mut Windowless) {
    let row = w
        .theme_editor_rows()
        .into_iter()
        .find(|(_, name)| name == "accent")
        .expect("accent is listed")
        .0;
    click(w, row);
    let card = w.theme_editor().unwrap();
    w.click_studio(card.hue.x + card.hue.width / 2.0, card.hue.y + 0.5);
    w.click_studio(card.square.right() - 0.5, card.square.y + 0.5);
}

fn is_red(c: fontelle_ui::theme::Color) -> bool {
    c.0[0] > 245 && c.0[1] < 10 && c.0[2] < 10
}

#[test]
fn a_colour_shows_at_once_and_save_keeps_it_as_your_own() {
    let mut w = studio();
    let before = w.document().studio_theme().unwrap();
    open_editor(&mut w);
    make_accent_red(&mut w);
    assert!(
        is_red(w.window_theme().palette.accent),
        "the studio wears it"
    );
    assert_eq!(
        w.document().studio_theme().unwrap().palette.accent,
        before.palette.accent,
        "nothing is saved before Save"
    );

    let card = w.theme_editor().unwrap();
    click(&mut w, card.save);
    let saved = w.document().studio_theme().unwrap();
    assert!(is_red(saved.palette.accent));
    assert_ne!(
        saved.name, before.name,
        "a built-in look is never overwritten"
    );
    assert!(saved.name.contains("(mine)"), "{}", saved.name);

    let card = w.theme_editor().expect("Save leaves the card open");
    click(&mut w, card.close);
    assert!(w.theme_editor().is_none());
    assert!(is_red(w.window_theme().palette.accent));
}

#[test]
fn closing_without_saving_puts_the_look_back() {
    let mut w = studio();
    let before = w.window_theme().palette.accent;
    open_editor(&mut w);
    make_accent_red(&mut w);
    let card = w.theme_editor().unwrap();
    click(&mut w, card.close);
    assert!(w.theme_editor().is_none());
    assert_eq!(w.window_theme().palette.accent, before);
}

/// The card swallows its own clicks: nothing under it is pressed.
#[test]
fn a_click_on_the_card_reaches_nothing_under_it() {
    let mut w = studio();
    open_editor(&mut w);
    let card = w.theme_editor().unwrap();
    let (x, y) = (card.frame.x + 3.0, card.frame.bottom() - 3.0);
    assert_eq!(
        fontelle_ui::canvas::theme_editor_hit(&card, x, y),
        Some(ThemeEditorHit::Card)
    );
    let clips = w.clip_count();
    w.click_studio(x, y);
    assert_eq!(w.clip_count(), clips);
    assert!(w.theme_editor().is_some());
}

fn window_shaders(theme: &Theme) -> Vec<String> {
    theme
        .backdrops
        .window
        .iter()
        .filter_map(|l| match l {
            Layer::Shader(s) => Some(s.shader.clone()),
            _ => None,
        })
        .collect()
}

/// A shader layer is written on the code card: a line that does not compile
/// is pointed at and the studio keeps the last one that did; one that does
/// is shown at once, and saved with the theme.
#[test]
fn a_shader_is_written_on_the_code_card_and_shown_as_it_compiles() {
    let mut w = studio();
    open_editor(&mut w);
    let card = w.theme_editor().unwrap();
    click(&mut w, card.tabs[1]);
    // The Window section is the one chosen first.
    let card = w.theme_editor().unwrap();
    click(&mut w, card.add[2]);
    let shaders = window_shaders(w.window_theme());
    assert_eq!(shaders.len(), 1, "a shader layer is added");
    let starter = shaders[0].clone();
    assert!(fontelle_ui::backdrop::shader_problem(&starter).is_none());

    let card = w.theme_editor().unwrap();
    click(&mut w, card.code);
    assert!(w.shader_editor().is_some(), "the code card opens");

    // The caret starts at the top: a stray letter breaks the first line.
    w.type_text("x");
    let problem = w.shader_problem().expect("it says what is wrong");
    assert!(problem.starts_with("Line 1"), "{problem}");
    assert_eq!(window_shaders(w.window_theme()), vec![starter.clone()]);

    // Backspace, then a line of its own.
    w.type_text("\u{8}// mine\n");
    assert_eq!(w.shader_problem(), None);
    let live = window_shaders(w.window_theme());
    assert!(live[0].starts_with("// mine\n"), "{:?}", live[0]);

    let code = w.shader_editor().unwrap();
    click(&mut w, code.done);
    assert!(w.shader_editor().is_none());
    let card = w.theme_editor().unwrap();
    click(&mut w, card.save);
    let saved = w.document().studio_theme().unwrap();
    assert!(window_shaders(&saved)[0].starts_with("// mine\n"));
}

/// A layer moves, and goes, from its row.
#[test]
fn layers_are_reordered_and_removed_from_their_rows() {
    let mut w = studio();
    open_editor(&mut w);
    let card = w.theme_editor().unwrap();
    click(&mut w, card.tabs[1]);
    let card = w.theme_editor().unwrap();
    click(&mut w, card.add[0]); // a colour
    let card = w.theme_editor().unwrap();
    click(&mut w, card.add[1]); // a gradient, on top of it
    let kinds = |t: &Theme| -> Vec<&'static str> {
        t.backdrops
            .window
            .iter()
            .map(|l| match l {
                Layer::Solid { .. } => "solid",
                Layer::Gradient(_) => "gradient",
                Layer::Shader(_) => "shader",
                Layer::Image(_) => "image",
            })
            .collect()
    };
    assert_eq!(kinds(w.window_theme()), ["solid", "gradient"]);
    // Rows list the top first: the gradient is row 0. Down puts it under.
    let card = w.theme_editor().unwrap();
    assert_eq!(card.layers.len(), 2);
    click(&mut w, card.layers[0].down);
    assert_eq!(kinds(w.window_theme()), ["gradient", "solid"]);
    let card = w.theme_editor().unwrap();
    click(&mut w, card.layers[0].remove);
    assert_eq!(kinds(w.window_theme()), ["gradient"]);
}

/// The button is on the settings page, which covers the studio: the page
/// gets out of the way, or the preview is hidden behind it.
#[test]
fn the_settings_page_steps_aside_for_the_card() {
    let mut w = studio();
    w.open_settings("Appearance");
    assert!(w.settings_page_open());
    open_editor(&mut w);
    assert!(!w.settings_page_open());
}
