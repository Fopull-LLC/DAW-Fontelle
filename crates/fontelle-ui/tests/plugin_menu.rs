//! The preset drop-down at the top of a plugin's own window.
//!
//! > *"i still wasnt able to change or view presets via our preset dropdown
//! > at the top of plugin windows... when clicking the preset dropdown on the
//! > top of the plugin window it didnt drop down any of those presets for me
//! > to select there or hit random preset to get a random one, it just opened
//! > the preset tab on the left."*
//!
//! The name on the strip drops a list in the plugin's window, under the
//! name: the user's own presets, then the plugin's library by category, the
//! one playing marked, Random, Previous and Next, typing to search, and Show
//! in browser at the foot for the tab it used to open. What a press, a key or
//! the wheel does to it is worked out here, purely; the window does what it
//! says.

use std::sync::{Mutex, OnceLock};

use fontelle_types::PresetOrigin;
use fontelle_ui::canvas::{
    CHOSEN_MARK, PLUGIN_HEADER_HEIGHT, PluginHeaderView, PluginMenuInput, PluginMenuKey,
    PluginMenuOutcome, PluginMenuRow, PluginPresetMenu, PresetBarView, PresetChoice, PresetDevice,
    RANDOM_PRESET, SHOW_IN_BROWSER, plugin_header_layout, plugin_preset_menu,
};
use fontelle_ui::render::Headless;
use fontelle_ui::theme::Theme;

fn choice(name: &str, category: &str, origin: PresetOrigin) -> PresetChoice {
    PresetChoice {
        name: name.to_string(),
        category: category.to_string(),
        origin,
        favourite: false,
        tags: Vec::new(),
        notes: String::new(),
    }
}

/// Cardinal's shape: the user's two, then patches at the top of its folder
/// (no category) and under `examples`.
fn choices() -> Vec<PresetChoice> {
    vec![
        choice("My Patch", "Saved", PresetOrigin::User),
        choice("Night Drive", "Saved", PresetOrigin::User),
        choice("welcome-wasm", "", PresetOrigin::Plugin),
        choice("welcome-wasm-mini", "", PresetOrigin::Plugin),
        choice("DRMR_-_BassGrowl", "examples", PresetOrigin::Plugin),
        choice("DRMR_-_Bells", "examples", PresetOrigin::Plugin),
        choice("Lead Organ", "examples", PresetOrigin::Plugin),
    ]
}

const DEVICE: PresetDevice = PresetDevice::Channel { index: 0 };

fn header(scale: f32) -> PluginHeaderView {
    PluginHeaderView {
        device: DEVICE,
        bar: PresetBarView {
            name: Some("welcome-wasm".to_string()),
            category: String::new(),
            origin: Some(PresetOrigin::Plugin),
            dirty: false,
            favourite: false,
            can_save: false,
        },
        width: (640.0 * scale) as u32,
        height: (PLUGIN_HEADER_HEIGHT * scale) as u32,
        area: (420.0 * scale) as u32,
        scale,
        hover: None,
    }
}

fn open(scale: f32, current: Option<usize>) -> PluginPresetMenu {
    let theme = Theme::dark_default();
    PluginPresetMenu::open(
        &header(scale),
        &choices(),
        current,
        &theme.metrics,
        theme.font.size,
    )
    .expect("a menu with room for it")
}

fn input(
    menu: &mut PluginPresetMenu,
    what: PluginMenuInput,
    current: Option<usize>,
) -> PluginMenuOutcome {
    let theme = Theme::dark_default();
    menu.input(
        what,
        &header(menu.scale()),
        &choices(),
        current,
        &theme.metrics,
        theme.font.size,
        7,
    )
}

/// The middle of row `index`, in the window's pixels.
fn middle_of(menu: &PluginPresetMenu, index: usize) -> (f32, f32) {
    let row = menu.menu.rows[index];
    assert!(!row.is_empty(), "row {index} is on screen");
    let scale = menu.scale();
    ((row.x + 20.0) * scale, (row.y + row.height / 2.0) * scale)
}

fn row_of(menu: &PluginPresetMenu, wanted: PluginMenuRow) -> usize {
    menu.rows
        .iter()
        .position(|row| *row == wanted)
        .unwrap_or_else(|| panic!("no {wanted:?} row"))
}

// ------------------------------------------------------------ the rows ---

#[test]
fn the_users_presets_come_first_then_the_plugins_library_by_category() {
    let (entries, rows) = plugin_preset_menu(&choices(), "", Some(2));
    let labels: Vec<&str> = entries.iter().map(|e| e.label.as_str()).collect();
    let at = |label: &str| {
        labels
            .iter()
            .position(|l| l.contains(label))
            .unwrap_or_else(|| panic!("no {label} in {labels:?}"))
    };
    assert!(at("My Patch") < at("welcome-wasm"));
    assert!(at("welcome-wasm-mini") < at("examples"));
    assert!(at("examples") < at("DRMR_-_BassGrowl"));
    assert!(
        labels[at("welcome-wasm")].starts_with(CHOSEN_MARK),
        "the one playing is marked: {labels:?}"
    );
    assert!(labels.contains(&RANDOM_PRESET));
    assert!(rows.contains(&PluginMenuRow::Random));
    assert!(rows.contains(&PluginMenuRow::Previous));
    assert!(rows.contains(&PluginMenuRow::Next));
    assert_eq!(rows.last(), Some(&PluginMenuRow::ShowInBrowser));
    assert_eq!(labels.last(), Some(&SHOW_IN_BROWSER));
    // No heading with nothing in it: the patches with no category are under
    // a heading that says whose they are.
    for (entry, row) in entries.iter().zip(&rows) {
        if *row == PluginMenuRow::Heading {
            assert!(!entry.label.trim().is_empty(), "{labels:?}");
        }
    }
    assert_eq!(entries.len(), rows.len());
}

#[test]
fn a_plugin_with_no_presets_still_offers_the_browser() {
    let (entries, rows) = plugin_preset_menu(&[], "", None);
    assert_eq!(rows.last(), Some(&PluginMenuRow::ShowInBrowser));
    assert!(!rows.contains(&PluginMenuRow::Random));
    assert!(entries.iter().any(|e| e.label == "No presets"));
}

// -------------------------------------------------------- where it is ---

#[test]
fn it_drops_from_the_name_on_the_strip_into_the_plugins_area() {
    for scale in [1.0, 2.0] {
        let menu = open(scale, None);
        let theme = Theme::dark_default();
        let bar = plugin_header_layout(640.0, &header(scale).bar, &theme.metrics);
        let frame = menu.menu.frame;
        assert!(
            (frame.y - PLUGIN_HEADER_HEIGHT).abs() < 1.0,
            "under the strip at {scale}x: {frame:?}"
        );
        assert!(
            (frame.x - bar.name.x).abs() < 1.0,
            "under the name: {frame:?}"
        );
        assert!(frame.right() <= 640.0 && frame.bottom() <= PLUGIN_HEADER_HEIGHT + 420.0);
        // In the window's own pixels, for the window to put it up.
        let (x, y, width, height) = menu.pixel_rect();
        assert_eq!(x, (frame.x * scale).round() as i32);
        assert_eq!(y, (frame.y * scale).round() as i32);
        assert!(width >= (frame.width * scale) as u32 && height >= (frame.height * scale) as u32);
    }
}

#[test]
fn it_opens_on_the_preset_playing() {
    let mut many = choices();
    for n in 0..300 {
        many.push(choice(
            &format!("Patch {n:03}"),
            "bank",
            PresetOrigin::Plugin,
        ));
    }
    let theme = Theme::dark_default();
    let menu = PluginPresetMenu::open(
        &header(1.0),
        &many,
        Some(250),
        &theme.metrics,
        theme.font.size,
    )
    .unwrap();
    let marked = menu
        .menu
        .entries
        .iter()
        .rposition(|e| e.label.starts_with(CHOSEN_MARK))
        .unwrap();
    assert!(
        !menu.menu.rows[marked].is_empty(),
        "the marked row is on screen"
    );
}

// --------------------------------------------------------- using it ---

#[test]
fn pressing_a_preset_chooses_it_and_closes() {
    let mut menu = open(1.0, None);
    let row = row_of(&menu, PluginMenuRow::Preset(4));
    let (x, y) = middle_of(&menu, row);
    assert_eq!(
        input(&mut menu, PluginMenuInput::Press(x, y), None),
        PluginMenuOutcome::Choose(4)
    );
}

#[test]
fn at_twice_the_scale_a_press_lands_on_the_same_row() {
    let mut menu = open(2.0, None);
    let row = row_of(&menu, PluginMenuRow::Preset(0));
    let (x, y) = middle_of(&menu, row);
    assert_eq!(
        input(&mut menu, PluginMenuInput::Press(x, y), None),
        PluginMenuOutcome::Choose(0)
    );
}

#[test]
fn random_previous_and_next_keep_it_open() {
    let mut menu = open(1.0, Some(2));
    let random = row_of(&menu, PluginMenuRow::Random);
    let (x, y) = middle_of(&menu, random);
    match input(&mut menu, PluginMenuInput::Press(x, y), Some(2)) {
        PluginMenuOutcome::Random(which) => assert!(which < choices().len()),
        other => panic!("{other:?}"),
    }
    let next = row_of(&menu, PluginMenuRow::Next);
    let (x, y) = middle_of(&menu, next);
    assert_eq!(
        input(&mut menu, PluginMenuInput::Press(x, y), Some(2)),
        PluginMenuOutcome::Step(1)
    );
    let previous = row_of(&menu, PluginMenuRow::Previous);
    let (x, y) = middle_of(&menu, previous);
    assert_eq!(
        input(&mut menu, PluginMenuInput::Press(x, y), Some(2)),
        PluginMenuOutcome::Step(-1)
    );
}

#[test]
fn show_in_browser_is_at_the_foot() {
    let mut menu = open(1.0, None);
    let at = row_of(&menu, PluginMenuRow::ShowInBrowser);
    let (x, y) = middle_of(&menu, at);
    assert_eq!(
        input(&mut menu, PluginMenuInput::Press(x, y), None),
        PluginMenuOutcome::ShowInBrowser
    );
}

#[test]
fn a_press_off_it_closes_it_and_one_on_the_strip_is_the_strips() {
    let mut menu = open(1.0, None);
    // In the plugin's area, past the menu.
    assert_eq!(
        input(&mut menu, PluginMenuInput::Press(630.0, 400.0), None),
        PluginMenuOutcome::Close
    );
    // On the name again: shut, the toggle a drop-down is.
    let theme = Theme::dark_default();
    let bar = plugin_header_layout(640.0, &header(1.0).bar, &theme.metrics);
    let mut menu = open(1.0, None);
    assert_eq!(
        input(
            &mut menu,
            PluginMenuInput::Press(bar.name.x + 10.0, bar.name.y + 5.0),
            None
        ),
        PluginMenuOutcome::Close
    );
    // On the strip's arrow: shut, and the arrow does what it does.
    let mut menu = open(1.0, None);
    let (x, y) = (bar.next.x + 3.0, bar.next.y + 5.0);
    assert_eq!(
        input(&mut menu, PluginMenuInput::Press(x, y), None),
        PluginMenuOutcome::PassToStrip(x, y)
    );
    let mut menu = open(1.0, None);
    assert_eq!(
        input(&mut menu, PluginMenuInput::Lost, None),
        PluginMenuOutcome::Close
    );
}

#[test]
fn typing_narrows_it_and_enter_takes_the_first_hit() {
    let mut menu = open(1.0, None);
    for letter in ["b", "e", "l"] {
        assert_eq!(
            input(
                &mut menu,
                PluginMenuInput::Key(PluginMenuKey::Text(letter.to_string())),
                None
            ),
            PluginMenuOutcome::Redraw
        );
    }
    assert_eq!(menu.query(), "bel");
    let presets: Vec<PluginMenuRow> = menu
        .rows
        .iter()
        .copied()
        .filter(|row| matches!(row, PluginMenuRow::Preset(_)))
        .collect();
    assert_eq!(presets, vec![PluginMenuRow::Preset(5)], "DRMR_-_Bells only");
    assert!(
        menu.menu.entries[0].label.contains("bel"),
        "the heading says what was typed: {:?}",
        menu.menu.entries[0].label
    );
    assert_eq!(
        input(&mut menu, PluginMenuInput::Key(PluginMenuKey::Enter), None),
        PluginMenuOutcome::Choose(5)
    );
    // Backspace takes a letter back; Escape shuts it.
    let mut menu = open(1.0, None);
    input(
        &mut menu,
        PluginMenuInput::Key(PluginMenuKey::Text("x".into())),
        None,
    );
    input(
        &mut menu,
        PluginMenuInput::Key(PluginMenuKey::Backspace),
        None,
    );
    assert_eq!(menu.query(), "");
    assert_eq!(
        input(&mut menu, PluginMenuInput::Key(PluginMenuKey::Escape), None),
        PluginMenuOutcome::Close
    );
}

#[test]
fn the_arrows_walk_the_presets_and_enter_takes_the_one_lit() {
    let mut menu = open(1.0, None);
    input(&mut menu, PluginMenuInput::Key(PluginMenuKey::Down), None);
    let first = menu.hover().expect("a row is lit");
    input(&mut menu, PluginMenuInput::Key(PluginMenuKey::Down), None);
    let second = menu.hover().unwrap();
    assert!(second > first);
    let wanted = menu.rows[second];
    let outcome = input(&mut menu, PluginMenuInput::Key(PluginMenuKey::Enter), None);
    match wanted {
        PluginMenuRow::Preset(which) => assert_eq!(outcome, PluginMenuOutcome::Choose(which)),
        PluginMenuRow::Previous => assert_eq!(outcome, PluginMenuOutcome::Step(-1)),
        PluginMenuRow::Next => assert_eq!(outcome, PluginMenuOutcome::Step(1)),
        other => panic!("lit {other:?}"),
    }
}

#[test]
fn the_pointer_lights_a_row_and_the_wheel_scrolls_a_long_list() {
    let mut menu = open(1.0, None);
    let row = row_of(&menu, PluginMenuRow::Preset(3));
    let (x, y) = middle_of(&menu, row);
    assert_eq!(
        input(&mut menu, PluginMenuInput::Pointer(x, y), None),
        PluginMenuOutcome::Redraw
    );
    assert_eq!(menu.hover(), Some(row));
    assert_eq!(
        input(&mut menu, PluginMenuInput::Pointer(x, y), None),
        PluginMenuOutcome::Nothing,
        "the same row twice is not a redraw"
    );

    let mut many = choices();
    for n in 0..400 {
        many.push(choice(
            &format!("Patch {n:03}"),
            "bank",
            PresetOrigin::Plugin,
        ));
    }
    let theme = Theme::dark_default();
    // A narrow window, so the list cannot go into columns and scrolls.
    let mut narrow = header(1.0);
    narrow.width = 300;
    let mut menu =
        PluginPresetMenu::open(&narrow, &many, None, &theme.metrics, theme.font.size).unwrap();
    assert!(menu.menu.scrolls());
    let before = menu.menu.scroll();
    let outcome = menu.input(
        PluginMenuInput::Scroll(2),
        &narrow,
        &many,
        None,
        &theme.metrics,
        theme.font.size,
        0,
    );
    assert_eq!(outcome, PluginMenuOutcome::Redraw);
    assert!(menu.menu.scroll() > before);
}

/// After a preset is loaded with the menu still up (Random, the arrows),
/// the mark moves to it and the list stays where it was scrolled.
#[test]
fn refreshed_it_marks_the_new_preset_where_it_was() {
    let mut menu = open(1.0, Some(2));
    let theme = Theme::dark_default();
    menu.refresh(
        &header(1.0),
        &choices(),
        Some(5),
        &theme.metrics,
        theme.font.size,
    );
    let marked: Vec<&str> = menu
        .menu
        .entries
        .iter()
        .filter(|e| e.label.starts_with(CHOSEN_MARK))
        .map(|e| e.label.as_str())
        .collect();
    assert_eq!(marked, vec![&format!("{CHOSEN_MARK}DRMR_-_Bells")[..]]);
}

// ------------------------------------------------------- its pixels ---

fn headless() -> Option<&'static Mutex<Headless>> {
    if cfg!(windows) && std::env::var_os("CI").is_some() {
        eprintln!("skipping: no GPU on this runner");
        return None;
    }
    static SHARED: OnceLock<Option<Mutex<Headless>>> = OnceLock::new();
    SHARED
        .get_or_init(|| match Headless::new() {
            Ok(h) => Some(Mutex::new(h)),
            Err(e) => {
                eprintln!("skipping: no usable GPU adapter ({e})");
                None
            }
        })
        .as_ref()
}

fn dump(pixels: &[u8], name: &str, width: u32, height: u32) {
    let Ok(dir) = std::env::var("FONTELLE_UI_DUMP") else {
        return;
    };
    let path = std::path::Path::new(&dir).join(format!("{name}.png"));
    let file = std::fs::File::create(&path).expect("somewhere to write");
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .unwrap()
        .write_image_data(pixels)
        .unwrap();
    eprintln!("wrote {}", path.display());
}

/// Drawn into pixels the size of its place in the window, at the window's
/// scale, in both built-in themes — and, for looking at, composited onto a
/// picture of the window it drops into (the strip over a grey plugin).
#[test]
fn the_menu_is_drawn_into_pixels_for_the_plugins_window() {
    let Some(headless) = headless() else {
        return;
    };
    for (theme, name) in [
        (Theme::dark_default(), "plugin-preset-menu-dark"),
        (Theme::light_default(), "plugin-preset-menu-light"),
    ] {
        let mut labels = fontelle_ui::text::Labels::new();
        let mut text = fontelle_ui::TextContext::new();
        let scale = 1.0;
        let header = header(scale);
        let mut menu = PluginPresetMenu::open(
            &header,
            &choices(),
            Some(2),
            &theme.metrics,
            theme.font.size,
        )
        .unwrap();
        let row = row_of(&menu, PluginMenuRow::Preset(4));
        let (x, y) = middle_of(&menu, row);
        menu.input(
            PluginMenuInput::Pointer(x, y),
            &header,
            &choices(),
            Some(2),
            &theme.metrics,
            theme.font.size,
            0,
        );
        let (mx, my, width, height) = menu.pixel_rect();
        let mut renderer = headless.lock().unwrap();
        let pixels = fontelle_ui::render::plugin_menu_pixels(
            &mut renderer,
            &theme,
            &mut labels,
            &mut text,
            &menu,
        )
        .expect("the menu renders");
        assert_eq!(pixels.len(), (width * height * 4) as usize);
        // Something is written on it.
        let ground = theme.palette.solid().panel_header.0;
        let inked = pixels
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| {
                p.iter()
                    .zip(ground.iter())
                    .take(3)
                    .any(|(a, b)| a.abs_diff(*b) > 60)
            })
            .count();
        assert!(inked > 200, "{name}: {inked} pixels of ink");

        // The window as it would look: the strip, the plugin's grey, the
        // menu over both.
        let strip = fontelle_ui::render::plugin_header_pixels(
            &mut renderer,
            &theme,
            &mut labels,
            &mut text,
            &header.bar,
            Some(fontelle_ui::canvas::PresetBarHit::Name),
            header.width,
            header.height,
            scale,
        )
        .unwrap();
        let (fw, fh) = (header.width, header.height + header.area);
        let mut frame = vec![0x40u8; (fw * fh * 4) as usize];
        frame[..strip.len()].copy_from_slice(&strip);
        for row in 0..height {
            for col in 0..width {
                let (fx, fy) = (mx as u32 + col, my as u32 + row);
                if fx < fw && fy < fh {
                    let to = ((fy * fw + fx) * 4) as usize;
                    let from = ((row * width + col) * 4) as usize;
                    frame[to..to + 4].copy_from_slice(&pixels[from..from + 4]);
                }
            }
        }
        dump(&frame, name, fw, fh);
        // And the menu alone, for `fontelle-host`'s editor probe
        // (`PROBE_MENU`) to put up over a real plugin.
        dump(&pixels, &format!("{name}-alone"), width, height);
    }
}

/// A library of hundreds (OB-Xf's 488) in a window of OB-Xf's size: drawn,
/// for looking at.
#[test]
fn a_long_library_is_drawn_into_the_plugins_window() {
    let Some(headless) = headless() else {
        return;
    };
    let theme = Theme::dark_default();
    let mut many = vec![choice("Mine", "Saved", PresetOrigin::User)];
    for category in [
        "Atmospheres",
        "Basses",
        "Bells",
        "Brass",
        "Drums",
        "FX",
        "Keys",
        "Leads",
        "Pads",
    ] {
        for n in 0..54 {
            many.push(choice(
                &format!("{category} {n:02}"),
                category,
                PresetOrigin::Plugin,
            ));
        }
    }
    let mut header = header(1.0);
    header.width = 1000;
    header.area = 620;
    let menu =
        PluginPresetMenu::open(&header, &many, Some(200), &theme.metrics, theme.font.size).unwrap();
    let (_, _, width, height) = menu.pixel_rect();
    let mut labels = fontelle_ui::text::Labels::new();
    let mut text = fontelle_ui::TextContext::new();
    let pixels = fontelle_ui::render::plugin_menu_pixels(
        &mut headless.lock().unwrap(),
        &theme,
        &mut labels,
        &mut text,
        &menu,
    )
    .unwrap();
    assert_eq!(pixels.len(), (width * height * 4) as usize);
    dump(&pixels, "plugin-preset-menu-long", width, height);
}
