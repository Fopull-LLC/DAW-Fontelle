//! The vocal instrument panel in pixels, drawn from what the studio really
//! answers: the Fontelle Singer with a real UTAU bank and a real DiffSinger
//! bank (their own portraits, capabilities and settings), and an engine
//! with no window of its own. Dumped with `FONTELLE_UI_DUMP=<dir>`.
//!
//! Needs `FONTELLE_TEST_VOICEBANKS` (the folder of real banks on the
//! development machine; never in git) and a GPU adapter — software GL is
//! enough; skips, saying so, without either. Nothing is sung: the tracks
//! have no notes, so no phrase is rendered.

mod common;

use std::path::{Path, PathBuf};

use fontelle_app::Session;
use fontelle_types::{InstrumentKind, ParamAddress};
use fontelle_ui::canvas::{SINGER_PANEL_SIZE, singer_panel_layout};
use fontelle_ui::document::{DocumentHost, SingerKind, StudioHost};
use fontelle_ui::render::singer_panel::{SingerChrome, singer_panel_strings};
use fontelle_ui::render::{EditorWindowChrome, Headless, draw_editor_window};
use fontelle_ui::text::{Labels, TextContext};
use fontelle_ui::theme::Theme;

use fontelle_testsinger::ENGINE_NAME;

fn library_name(stem: &str) -> String {
    if cfg!(target_os = "windows") {
        format!("{stem}.dll")
    } else if cfg!(target_os = "macos") {
        format!("lib{stem}.dylib")
    } else {
        format!("lib{stem}.so")
    }
}

/// The ABI-1 test engine alone, copied somewhere of the test's own.
fn engine_folder() -> PathBuf {
    let mut path = std::env::current_exe().unwrap();
    path.pop();
    path.pop();
    let built = path.join(library_name("fontelle_testsinger"));
    assert!(built.exists(), "run `cargo build -p fontelle-testsinger`");
    let folder = std::env::temp_dir().join(format!("fontelle-panel-shots-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::copy(&built, folder.join(library_name("fontelle_testsinger"))).unwrap();
    folder
}

fn dump(pixels: &[u8], name: &str, w: u32, h: u32) {
    let Ok(dir) = std::env::var("FONTELLE_UI_DUMP") else {
        return;
    };
    let path = Path::new(&dir).join(format!("{name}.png"));
    let file = std::fs::File::create(&path).unwrap();
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()
        .unwrap()
        .write_image_data(pixels)
        .unwrap();
    eprintln!("wrote {}", path.display());
}

/// Draws the selected channel's instrument window as the studio would.
fn shoot(session: &Session, headless: &mut Headless, name: &str) -> Vec<u8> {
    shoot_in(session, headless, name, Theme::dark_default())
}

fn shoot_in(session: &Session, headless: &mut Headless, name: &str, theme: Theme) -> Vec<u8> {
    let header = session.singer_panel().expect("a singer panel");
    let controls = session.instrument().expect("its controls");
    let (w, h) = SINGER_PANEL_SIZE;
    let panel = fontelle_ui::layout::editor_window_layout(w as f32, h as f32, &theme.metrics);
    let layout = singer_panel_layout(panel.body, &theme.metrics, &header, &controls, 0.0);
    let mut text = TextContext::new();
    let mut labels = Labels::new();
    for (s, style) in singer_panel_strings(&header, &controls) {
        labels.ensure_styled(&s, &theme.font, style, &mut text);
    }
    for group in &controls.groups {
        for param in &group.params {
            labels.ensure(&param.label, &theme.font, &mut text);
            labels.ensure(&param.display, &theme.font, &mut text);
        }
    }
    let mut portraits = fontelle_ui::portrait::Portraits::default();
    if let Some(path) = header.singer.as_ref().and_then(|s| s.image.as_deref()) {
        portraits.get(path);
    }
    let title = text.layout(
        &format!("{} \u{2014} {}", header.instrument, controls.title),
        &theme.font,
        None,
    );
    let mut scene = fontelle_ui::vello::Scene::new();
    draw_editor_window(
        &mut scene,
        &theme,
        &panel,
        &labels,
        &title,
        &EditorWindowChrome::Singer(SingerChrome {
            layout: layout.clone(),
            view: &header,
            controls: &controls,
            portraits: &portraits,
            hover: None,
            active: None,
            hover_hit: None,
        }),
        None,
        None,
        None,
        None,
    );
    let pixels = headless
        .render(&scene, w, h, theme.palette.window)
        .expect("the panel renders");
    dump(&pixels, name, w, h);
    if header.singer.is_none() {
        return pixels;
    }
    // The portrait is a picture, not a flat square.
    let r = layout.portrait;
    let mut colours = std::collections::BTreeSet::new();
    for y in (r.y as u32..r.bottom() as u32).step_by(3) {
        for x in (r.x as u32..r.right() as u32).step_by(3) {
            let i = ((y * w + x) * 4) as usize;
            colours.insert([pixels[i], pixels[i + 1], pixels[i + 2]]);
        }
    }
    assert!(colours.len() > 3, "{name}: something drawn in the portrait");
    pixels
}

fn set(session: &mut Session, address: &str, value: f32) {
    session.set_instrument_param(&ParamAddress::new(address), value);
    session.end_gesture();
}

#[test]
fn the_singer_panel_with_real_banks_and_an_engine() {
    let Some(banks) = std::env::var_os("FONTELLE_TEST_VOICEBANKS").map(PathBuf::from) else {
        eprintln!("skipping: FONTELLE_TEST_VOICEBANKS is not set");
        return;
    };
    let mut headless = match Headless::new() {
        Ok(h) => h,
        Err(e) => {
            eprintln!("skipping: no usable GPU adapter ({e})");
            return;
        }
    };
    let mut session = common::a_session_in(common::a_clip_project(4), None);
    session.set_singer_folders(vec![engine_folder()]);
    session.set_voicebanks(banks, fontelle_vocal::builtin_backends());
    session.add_singer_channel(None).unwrap();
    let index = (0..session.channels().len())
        .rev()
        .find(|i| session.channel_kind(*i) == Some(InstrumentKind::Vocal))
        .unwrap();
    session.select_channel(index);
    let singers = session.singers();
    let pick = |kind: SingerKind, folder: &str| {
        singers
            .iter()
            .find(|s| s.kind == kind && s.folder.as_deref() == Some(folder))
            .unwrap_or_else(|| panic!("{folder} installed"))
            .clone()
    };

    // Kasane Teto's UTAU bank, a little brighter, with vibrato on.
    let teto = pick(SingerKind::Utau, "KasaneTeto_tougou_110401");
    session.set_singer(
        index,
        Some(fontelle_model::VocalVoice::new(
            &teto.engine,
            &teto.id,
            "ja",
        )),
    );
    set(&mut session, "vocal/lane/gender", 0.6);
    set(&mut session, "vocal/vibrato/on", 1.0);
    set(&mut session, "vocal/glide/on", 1.0);
    shoot(&session, &mut headless, "singer-panel-utau");
    shoot_in(
        &session,
        &mut headless,
        "singer-panel-utau-light",
        Theme::light_default(),
    );

    // Qixuan's DiffSinger bank.
    let qixuan = pick(SingerKind::DiffSinger, "Qixuan_v2.7.0_DiffSinger_OpenUtau");
    session.set_singer(
        index,
        Some(fontelle_model::VocalVoice::new(
            &qixuan.engine,
            &qixuan.id,
            "zh",
        )),
    );
    shoot(&session, &mut headless, "singer-panel-diffsinger");

    // An engine with no window of its own: its panel, not a blank window.
    session.set_singer_instrument(index, Some(ENGINE_NAME));
    let header = session.singer_panel().unwrap();
    assert!(!header.has_editor);
    shoot(&session, &mut headless, "singer-panel-engine");

    // A song from a computer with a bank this one has not got.
    session.set_singer_instrument(index, None);
    session.set_singer(
        index,
        Some(fontelle_model::VocalVoice::new(
            "fontelle-utau",
            "utau/Somebody_Elses_Bank",
            "ja",
        )),
    );
    let message = session.singer_panel().unwrap().message.unwrap_or_default();
    assert!(message.contains("Find singers"), "{message}");
    shoot(&session, &mut headless, "singer-panel-missing");
    shoot_in(
        &session,
        &mut headless,
        "singer-panel-missing-light",
        Theme::light_default(),
    );

    // A first track with no voicebank installed at all.
    let empty = std::env::temp_dir().join(format!("fontelle-panel-empty-{}", std::process::id()));
    std::fs::create_dir_all(&empty).unwrap();
    let mut bare = common::a_session_in(common::a_clip_project(4), None);
    bare.set_singer_folders(vec![empty.join("no-engines")]);
    bare.set_voicebanks(empty.join("voicebanks"), fontelle_vocal::builtin_backends());
    bare.add_singer_channel(None).unwrap();
    let index = (0..bare.channels().len())
        .rev()
        .find(|i| bare.channel_kind(*i) == Some(InstrumentKind::Vocal))
        .unwrap();
    bare.select_channel(index);
    shoot(&bare, &mut headless, "singer-panel-no-voicebank");
}
