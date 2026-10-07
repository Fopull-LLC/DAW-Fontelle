//! The vocal roll's editing, in pixels: a lane drawn under the notes with
//! its tabs (one greyed and saying why), the phoneme row, a selected note's
//! vibrato wave and handles, the pitch line, the engine chip, the phoneme
//! field with suggestions — and the grouped singer picker, a voicebank's
//! terms, and the Voicebanks page. Dumped with `FONTELLE_UI_DUMP=<dir>`.
//!
//! Skips, loudly, on a machine with no usable adapter.

use std::sync::{Mutex, OnceLock};

use fontelle_model::{Arena, LaneCurve, Note, NoteVoice, VocalPart, VocalVoice};
use fontelle_sing::{Capabilities, GlideShape, Lane, LaneInfo, Portamento, Vibrato};
use fontelle_types::{NoteId, PPQN, Tick};
use fontelle_ui::canvas::{
    DEFAULT_LANE_HEIGHT, LaneProperty, PhonemeCell, RollLayout, RollView, SnapDivision, Tool,
    VocalCurve, VocalTab, VoicebankTab, carve_vocal_strips, curve_scale, key_row, phoneme_cells,
    singer_menu_rows, terms_menu_rows, tick_to_x, toolbar_layout, vocal_tabs,
    voicebank_page_layout,
};
use fontelle_ui::document::{
    CatalogueRow, KeyMap, SingerChoice, SingerKind, VocalPhraseView, VocalRenderState, VocalView,
    VoicebankRow,
};
use fontelle_ui::layout::{Rect, window_layout};
use fontelle_ui::render::vocal::{
    LYRIC_STYLE, PHONEME_STYLE, RollVocal, VoicebankPage, caption_rect, edit_strings,
    engine_chip_rect, engine_chip_text, page_strings, strings,
};
use fontelle_ui::render::{Chrome, Headless, RollChrome, TransportChrome, draw_window};
use fontelle_ui::text::{Labels, TextContext};
use fontelle_ui::theme::Theme;
use fontelle_ui::transport::{Meter, TransportView, format_readout, transport_bar_layout};

const RW: u32 = 1100;
const PROBLEMS: [String; 0] = [];
const RH: u32 = 700;

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

fn dump(pixels: &[u8], name: &str) {
    let Ok(dir) = std::env::var("FONTELLE_UI_DUMP") else {
        return;
    };
    let path = std::path::Path::new(&dir).join(format!("{name}.png"));
    let file = std::fs::File::create(&path).expect("somewhere to write the frame");
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), RW, RH);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .unwrap()
        .write_image_data(pixels)
        .unwrap();
    eprintln!("wrote {}", path.display());
}

fn note(start_beats: f64, beats: f64, key: u8, lyric: &str) -> Note {
    Note {
        start: (start_beats * PPQN as f64) as Tick,
        length: (beats * PPQN as f64) as Tick,
        key,
        velocity: 100,
        pan: 0,
        fine_pitch: 0,
        release: 0,
        mod_x: 0,
        mod_y: 0,
        slide: false,
        path: Vec::new(),
        channel: None,
        lyric: lyric.to_string(),
        voice: None,
    }
}

fn caps() -> Capabilities {
    Capabilities {
        lanes: vec![Lane::PitchBend, Lane::Dynamics, Lane::Tension],
        vibrato: true,
        portamento: true,
        velocity: true,
        phoneme_timing: true,
        phonemes: ["a", "e", "i", "o", "u", "k", "s", "sh", "t", "n", "m", "r"]
            .iter()
            .map(|s| s.to_string())
            .collect(),
        extra_lanes: vec![LaneInfo {
            id: "growl".into(),
            name: "Growl".into(),
            min: 0.0,
            max: 1.0,
            default: 0.0,
            unit: "%".into(),
        }],
        ..Capabilities::default()
    }
}

struct Song {
    notes: Arena<NoteId, Note>,
    ids: Vec<NoteId>,
    view: VocalView,
}

/// "sa-ku-ra" and a held note with vibrato, a glide into it, sung with
/// timed phonemes; a dynamics swell drawn under them.
fn song() -> Song {
    let mut notes = Arena::default();
    let line = [
        (0.0, 1.0, 62, "さ"),
        (1.0, 1.0, 64, "く"),
        (2.0, 3.0, 67, "ら"),
        (5.5, 1.0, 64, "-"),
    ];
    let ids: Vec<NoteId> = line
        .iter()
        .map(|(s, l, k, w)| notes.insert(note(*s, *l, *k, w)))
        .collect();
    notes[ids[2]].voice = Some(Box::new(NoteVoice {
        vibrato: Some(Vibrato {
            depth: 60.0,
            rate: 5.5,
            delay: 0.3,
            fade_in: 0.25,
            fade_out: 0.15,
        }),
        portamento: Some(Portamento {
            length: 0.15,
            position: 0.4,
            shape: GlideShape::S,
        }),
        ..NoteVoice::default()
    }));
    let mut part = VocalPart::default();
    let mut dynamics = LaneCurve::default();
    dynamics.write(
        0,
        PPQN * 6,
        20,
        |t| {
            let x = t as f32 / (PPQN * 6) as f32;
            -12.0 + 18.0 * (x * std::f32::consts::PI).sin()
        },
        0.0,
    );
    part.lanes.insert(Lane::Dynamics, dynamics);
    // The line the notes, glide and vibrato make.
    let mut pitch_line = Vec::new();
    for (i, (start, len, key, _)) in line.iter().enumerate() {
        let steps = (len * 40.0) as usize;
        for s in 0..steps {
            let b = start + s as f64 / 40.0;
            let mut k = f32::from(*key);
            if i == 2 && s as f64 / 40.0 > 0.9 {
                k += 0.6 * ((b * 15.0).sin() as f32);
            }
            pitch_line.push(((b * PPQN as f64) as Tick, k));
        }
    }
    let view = VocalView {
        caption: "Test Alto \u{b7} ja".into(),
        default_lyric: "ら".into(),
        phrases: vec![VocalPhraseView {
            start: 0,
            end: 6 * PPQN + PPQN / 2,
            state: VocalRenderState::Final,
        }],
        phonemes: Vec::new(),
        timed: vec![
            (
                ids[0],
                vec![("s".into(), Some(-0.06)), ("a".into(), Some(0.0))],
            ),
            (
                ids[1],
                vec![("k".into(), Some(-0.05)), ("u".into(), Some(0.0))],
            ),
            (
                ids[2],
                vec![("r".into(), Some(-0.03)), ("a".into(), Some(0.0))],
            ),
            (ids[3], vec![("a".into(), None)]),
        ],
        ticks_per_second: 1920.0,
        backend: "Fontelle Test Singer 2".into(),
        capabilities: caps(),
        part,
        pitch_line: vec![pitch_line],
        editor: Some("Fontelle Test Singer 2".into()),
        ..VocalView::default()
    };
    Song { notes, ids, view }
}

struct Shot<'a> {
    curve: Option<VocalCurve>,
    selection: Vec<NoteId>,
    field: Option<(NoteId, &'a str, Vec<String>)>,
    menu: Option<fontelle_ui::canvas::ContextMenu>,
    page: Option<(VoicebankTab, Vec<VoicebankRow>, Vec<CatalogueRow>)>,
    /// The singer picker: who is offered, what is typed, the focused card.
    picker: Option<(Vec<SingerChoice>, String, Option<usize>)>,
    /// The roll corner's portrait.
    portrait: Option<String>,
}

impl Default for Shot<'_> {
    fn default() -> Self {
        Self {
            curve: Some(VocalCurve::Standard(Lane::Dynamics)),
            selection: Vec::new(),
            field: None,
            menu: None,
            page: None,
            picker: None,
            portrait: None,
        }
    }
}

fn shoot(song: &Song, shot: Shot, name: &str) -> Option<(Vec<u8>, Theme, RollLayout, RollView)> {
    let theme = Theme::dark_default();
    let shared = headless()?;
    let mut text = TextContext::new();
    let layout = window_layout(RW as f32, RH as f32, &theme.metrics, 0.0);
    let mut labels = Labels::new();
    let title = text.layout("Roll", &theme.font, None);
    let view = TransportView::unavailable();
    let readout = text.layout(&format_readout(&view, 4), &theme.font, None);
    let tempo = text.layout("120.00", &theme.font, None);
    let signature = text.layout("4/4", &theme.font, None);
    let key_map = KeyMap::unknown();
    let mut roll_l = fontelle_ui::canvas::roll_layout_with_keys(
        layout.panel.body,
        &theme.metrics,
        DEFAULT_LANE_HEIGHT + 40.0,
        fontelle_ui::canvas::keyboard_width(&key_map),
    );
    let strips = carve_vocal_strips(&mut roll_l);
    let roll_view = RollView {
        top_key: 72,
        key_offset: 0.0,
        pixels_per_tick: 0.11,
        key_height: 18.0,
        snap: SnapDivision::Beat,
        ..RollView::default()
    };
    for key in fontelle_ui::canvas::visible_keys(&roll_view, roll_l.grid) {
        if key % 12 == 0 {
            labels.ensure(&fontelle_ui::render::key_name(key), &theme.font, &mut text);
        }
    }
    labels.ensure(
        &fontelle_ui::canvas::tools_caption(),
        &theme.font,
        &mut text,
    );
    let (words, small) = strings(&song.view, &song.notes, None);
    for w in &words {
        labels.ensure_styled(w, &theme.font, LYRIC_STYLE, &mut text);
    }
    for w in &small {
        labels.ensure_styled(w, &theme.font, PHONEME_STYLE, &mut text);
    }
    // Tabs, measured.
    let mut tab_words = vec!["notes".to_string(), "Growl".to_string()];
    for lane in Lane::ALL {
        tab_words.push(fontelle_model::lane_spec(lane).short.to_string());
    }
    for w in &tab_words {
        labels.ensure_styled(w, &theme.font, PHONEME_STYLE, &mut text);
    }
    let tabs: Vec<VocalTab> = vocal_tabs(strips.tabs, &song.view.capabilities, &|s| {
        labels
            .get_styled(s, PHONEME_STYLE)
            .map_or(30.0, |t| t.width)
    });
    let cells: Vec<PhonemeCell> = phoneme_cells(
        strips.phonemes,
        &roll_view,
        roll_l.grid,
        &song.notes,
        &song.view.timed,
        song.view.ticks_per_second,
    );
    let face = if shot.portrait.is_some() {
        fontelle_ui::render::vocal::CAPTION_PORTRAIT
    } else {
        0.0
    };
    let caption = labels
        .get_styled(&song.view.caption, LYRIC_STYLE)
        .map(|t| caption_rect(roll_l.grid, t.width + face));
    let chip_text = engine_chip_text("Fontelle Test Singer 2", false);
    labels.ensure_styled(&chip_text, &theme.font, PHONEME_STYLE, &mut text);
    let engine_chip = caption.map(|c| {
        engine_chip_rect(
            c,
            labels
                .get_styled(&chip_text, PHONEME_STYLE)
                .map_or(100.0, |t| t.width),
        )
    });
    let lane_curve;
    let lane = match &shot.curve {
        Some(curve) => {
            let key = curve.key(&song.view.backend);
            lane_curve = song.view.part.curve(&key).cloned().unwrap_or_default();
            Some((curve_scale(curve, &song.view.capabilities), &lane_curve))
        }
        None => None,
    };
    let page_layout;
    let page = match &shot.page {
        Some((tab, installed, catalogue)) => {
            let count = match tab {
                VoicebankTab::Installed => installed.len(),
                VoicebankTab::Find => catalogue.len(),
            };
            page_layout = voicebank_page_layout(layout.window, *tab, count, 0.0);
            Some(VoicebankPage {
                layout: &page_layout,
                tab: *tab,
                installed,
                catalogue,
                folder: "/home/you/.local/share/fontelle/voicebanks",
            })
        }
        None => None,
    };
    let mut portraits = fontelle_ui::portrait::Portraits::default();
    let mut faces: Vec<String> = shot.portrait.iter().cloned().collect();
    if let Some((_, installed, _)) = &shot.page {
        faces.extend(installed.iter().filter_map(|r| r.portrait.clone()));
    }
    if let Some((singers, _, _)) = &shot.picker {
        faces.extend(singers.iter().filter_map(|s| s.image.clone()));
    }
    for face in &faces {
        portraits.get(face);
    }
    let picker_layout;
    let picker = match &shot.picker {
        Some((singers, query, focus)) => {
            let shown = fontelle_ui::canvas::picker_filter(singers, query);
            picker_layout =
                fontelle_ui::canvas::singer_picker_layout(layout.window, singers, &shown, 0.0, 1);
            Some(fontelle_ui::render::vocal::SingerPickerView {
                layout: &picker_layout,
                singers,
                query,
                focus: *focus,
                language: 0,
                current: None,
                problems: &PROBLEMS,
            })
        }
        None => None,
    };
    if let Some(p) = &picker {
        for w in fontelle_ui::render::vocal::picker_strings(p) {
            labels.ensure_styled(&w, &theme.font, PHONEME_STYLE, &mut text);
        }
        labels.ensure_styled(
            fontelle_ui::render::vocal::PICKER_TITLE,
            &theme.font,
            LYRIC_STYLE,
            &mut text,
        );
    }
    let rv = RollVocal {
        view: &song.view,
        editing: None,
        marks: None,
        caption,
        strips,
        tabs: &tabs,
        curve: shot.curve.as_ref(),
        lane,
        cells: &cells,
        phoneme_field: shot.field.as_ref().map(|(n, t, s)| (*n, *t, s.as_slice())),
        px_per_second: (0.11 * song.view.ticks_per_second) as f32,
        engine_chip,
        note_button: Some(Rect::new(
            strips.tabs.right() - 60.0,
            strips.tabs.y + 1.0,
            56.0,
            16.0,
        )),
        portrait: shot.portrait.as_deref().and_then(|p| portraits.peek(p)),
    };
    for w in edit_strings(&rv) {
        labels.ensure_styled(&w, &theme.font, PHONEME_STYLE, &mut text);
    }
    if let Some(p) = &page {
        for w in page_strings(p) {
            labels.ensure_styled(&w, &theme.font, PHONEME_STYLE, &mut text);
        }
        labels.ensure_styled("Voicebanks", &theme.font, LYRIC_STYLE, &mut text);
    }
    if let Some(menu) = &shot.menu {
        for entry in &menu.entries {
            labels.ensure(&entry.label, &theme.font, &mut text);
        }
    }
    let tools = fontelle_ui::canvas::Tools::default();
    let mut scene = vello::Scene::new();
    draw_window(
        &mut scene,
        &theme,
        &layout,
        &Chrome {
            field: None,
            panel_title: &title,
            backdrops: Default::default(),
            transport: TransportChrome {
                layout: transport_bar_layout(layout.transport, &theme.metrics),
                view,
                meters: [Meter::new(); 2],
                readout: &readout,
                tempo: &tempo,
                signature: &signature,
                mode: &tempo,
                hover: None,
                marker_sample: 0,
                clip_mode: false,
                tempo_field: None,
            },
            roll: Some(RollChrome {
                focused: true,
                layout: roll_l,
                toolbar: toolbar_layout(roll_l.toolbar, &theme.metrics),
                view: roll_view,
                notes: &song.notes,
                selection: &shot.selection,
                playhead_tick: Some(PPQN * 3),
                beats_per_bar: 4,
                tool: Tool::Draw,
                snap: SnapDivision::Beat,
                lane_property: LaneProperty::Velocity,
                key_map: &key_map,
                ghosts: &[],
                ghost_filter: fontelle_ui::document::GhostFilter::Off,
                ghost_caption: String::new(),
                marker_tick: None,
                clip_length: Some(PPQN * 16),
                loop_range: None,
                marquee: None,
                hover: None,
                lane_menu: None,
                tools_panel: None,
                tools: &tools,
                slice: None,
                key_style: fontelle_ui::canvas::KeyStyle::Piano,
                live_keys: 0,
                recording: &[],
                scale: None,
                key: None,
                key_root: 0,
                vocal: Some(rv),
            }),
            rack: None,
            prefabs: None,
            browser: None,
            timeline: None,
            mixer: None,
            tabs: fontelle_ui::layout::editor_tabs(layout.panel.header, &theme.metrics),
            tab: fontelle_ui::layout::EditorTab::Roll,
            hover_tab: None,
            browser_title: "Soundfonts",
            labels: &labels,
            status: "",
            toast: None,
            confirm: None,
            notices: Default::default(),
            tooltip: None,
            menu: shot.menu.as_ref(),
            carry: None,
            welcome: None,
            keybinds: None,
            help: None,
            tour: None,
            settings_page: None,
            vocal_overlay: (page.is_some() || picker.is_some()).then_some(
                fontelle_ui::render::vocal::VocalOverlay {
                    page,
                    picker,
                    portraits: &portraits,
                },
            ),
        },
    );
    let pixels = shared
        .lock()
        .expect("the shared renderer")
        .render(&scene, RW, RH, theme.palette.window)
        .expect("rendering");
    dump(&pixels, name);
    Some((pixels, theme, roll_l, roll_view))
}

fn at(pixels: &[u8], x: f32, y: f32) -> [u8; 4] {
    let i = ((y as u32 * RW + x as u32) * 4) as usize;
    [pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3]]
}

fn distinct(pixels: &[u8], r: Rect) -> usize {
    let mut seen = std::collections::HashSet::new();
    let mut y = r.y;
    while y < r.bottom() {
        let mut x = r.x;
        while x < r.right() {
            seen.insert(at(pixels, x, y));
            x += 2.0;
        }
        y += 2.0;
    }
    seen.len()
}

#[test]
fn a_drawn_lane_shows_under_the_notes_with_its_tabs_and_phonemes() {
    let song = song();
    let Some((pixels, theme, roll, view)) = shoot(
        &song,
        Shot {
            selection: vec![song.ids[2]],
            ..Shot::default()
        },
        "vocal2-lanes",
    ) else {
        return;
    };
    // The swell's curve is in the accent colour somewhere across the lane.
    let accent = theme.palette.accent.0;
    let lane = roll.velocity;
    let mut found = 0;
    let mut x = lane.x + 4.0;
    while x < lane.right() - 4.0 {
        for dy in 0..lane.height as u32 {
            let p = at(&pixels, x, lane.y + dy as f32);
            if (p[0] as i32 - accent[0] as i32).abs() < 20
                && (p[1] as i32 - accent[1] as i32).abs() < 20
                && (p[2] as i32 - accent[2] as i32).abs() < 20
            {
                found += 1;
                break;
            }
        }
        x += 8.0;
    }
    assert!(found > 10, "the curve is drawn ({found} columns)");
    // The held note's vibrato wave runs through it.
    let n = &song.notes[song.ids[2]];
    let row = key_row(&view, roll.grid, n.key);
    let x1 = tick_to_x(&view, roll.grid, n.start + n.length);
    let band = Rect::new(x1 - 80.0, row.y - 4.0, 70.0, row.height + 8.0);
    assert!(distinct(&pixels, band) > 3);
}

#[test]
fn a_lane_the_voice_ignores_is_greyed_and_says_why() {
    let song = song();
    shoot(
        &song,
        Shot {
            curve: Some(VocalCurve::Standard(Lane::Breathiness)),
            ..Shot::default()
        },
        "vocal2-lane-ignored",
    );
    shoot(
        &song,
        Shot {
            curve: Some(VocalCurve::Extra("growl".into())),
            ..Shot::default()
        },
        "vocal2-lane-own",
    );
}

#[test]
fn the_phoneme_field_offers_the_voices_own_phonemes() {
    let song = song();
    shoot(
        &song,
        Shot {
            selection: vec![song.ids[1]],
            field: Some((song.ids[1], "k s", vec!["s".into(), "sh".into()])),
            ..Shot::default()
        },
        "vocal2-phoneme-field",
    );
}

fn picker_menu(
    theme: &Theme,
    rows: Vec<fontelle_ui::canvas::MenuEntry>,
) -> fontelle_ui::canvas::ContextMenu {
    let layout = window_layout(RW as f32, RH as f32, &theme.metrics, 0.0);
    fontelle_ui::canvas::context_menu_layout(
        (420.0, 60.0),
        layout.window,
        &theme.metrics,
        theme.font.size,
        rows,
    )
}

#[test]
fn the_picker_groups_free_voicebanks_and_engines() {
    let song = song();
    let theme = Theme::dark_default();
    let bank = |id: &str, name: &str, kind, langs: &[&str], agreed| SingerChoice {
        engine: "fontelle-utau".into(),
        engine_name: "UTAU".into(),
        id: id.into(),
        name: name.into(),
        languages: langs.iter().map(|l| l.to_string()).collect(),
        kind,
        terms: Some("terms".into()),
        terms_agreed: agreed,
        ..Default::default()
    };
    let singers = vec![
        bank("teto", "Kasane Teto", SingerKind::Utau, &["ja"], true),
        bank("qx", "Qixuan", SingerKind::DiffSinger, &["zh"], false),
        SingerChoice {
            engine: "Fontelle Test Singer 2".into(),
            engine_name: "Fontelle Test Singer 2".into(),
            id: "test.alto".into(),
            name: "Test Alto".into(),
            languages: vec!["en".into()],
            has_editor: true,
            ..Default::default()
        },
        SingerChoice {
            engine: "Fontelle Test Singer 2".into(),
            engine_name: "Fontelle Test Singer 2".into(),
            id: "test.research".into(),
            name: "Research Voice".into(),
            languages: vec!["ja".into()],
            has_editor: true,
            research_only: true,
            ..Default::default()
        },
    ];
    let current = VocalVoice::new("fontelle-utau", "teto", "ja");
    let rows = singer_menu_rows(&singers, Some(&current), &[], "");
    let menu = picker_menu(&theme, rows.into_iter().map(|(e, _)| e).collect());
    shoot(
        &song,
        Shot {
            menu: Some(menu),
            ..Shot::default()
        },
        "vocal2-picker",
    );
    let terms = terms_menu_rows(
        "Qixuan",
        "Songs made with it may be shared and released, commercially too; label them as AI-synthesised and credit \"Qixuan\" and \"DiffSinger\". The voicebank may not be sold, and its output may not be used to train other voice models.",
    );
    let menu = picker_menu(&theme, terms.into_iter().map(|(e, _)| e).collect());
    shoot(
        &song,
        Shot {
            menu: Some(menu),
            ..Shot::default()
        },
        "vocal2-terms",
    );
}

#[test]
fn the_voicebanks_page_lists_what_is_installed_and_what_can_be_found() {
    let song = song();
    let installed = vec![
        VoicebankRow {
            id: "teto".into(),
            name: "重音テト".into(),
            kind: "UTAU".into(),
            author: Some("線の人".into()),
            terms: Some("利用規約".into()),
            terms_agreed: true,
            portrait: real_portrait("KasaneTeto_tougou_110401/重音テト音声ライブラリー/teto.bmp")
                .or_else(|| Some(portrait_file("teto-page", 200, true))),
            sample: Some("sample.wav".into()),
            languages: vec!["ja".into()],
            ..Default::default()
        },
        VoicebankRow {
            id: "qixuan".into(),
            name: "Qixuan".into(),
            kind: "DiffSinger".into(),
            terms: Some("terms".into()),
            terms_agreed: false,
            portrait: real_portrait("Qixuan_v2.7.0_DiffSinger_OpenUtau/qixuan256x256.png")
                .or_else(|| Some(portrait_file("qixuan-page", 120, false))),
            ..Default::default()
        },
        VoicebankRow {
            id: "Dependencies/nsf_hifigan".into(),
            name: "nsf_hifigan".into(),
            kind: "Dependency".into(),
            ..Default::default()
        },
    ];
    let catalogue = vec![
        CatalogueRow {
            id: "kasane-teto.utau".into(),
            name: "Kasane Teto (UTAU)".into(),
            kind: "UTAU".into(),
            languages: vec!["ja".into(), "en".into()],
            terms_summary: "Free to download and use; commercial use needs permission first."
                .into(),
            commercial: "ask".into(),
            credit: "no".into(),
            ai_training: "?".into(),
            ..Default::default()
        },
        CatalogueRow {
            id: "qixuan.diffsinger".into(),
            name: "Qixuan (DiffSinger)".into(),
            kind: "DiffSinger".into(),
            languages: vec!["zh".into()],
            terms_summary: "Credit \"Qixuan\" and \"DiffSinger\"; no training other models.".into(),
            commercial: "yes".into(),
            credit: "yes".into(),
            ai_training: "no".into(),
            waiting: true,
            ..Default::default()
        },
    ];
    let Some((pixels, theme, _, _)) = shoot(
        &song,
        Shot {
            page: Some((
                VoicebankTab::Installed,
                installed.clone(),
                catalogue.clone(),
            )),
            ..Shot::default()
        },
        "vocal2-page-installed",
    ) else {
        return;
    };
    let layout = window_layout(RW as f32, RH as f32, &theme.metrics, 0.0);
    let page = voicebank_page_layout(layout.window, VoicebankTab::Installed, 3, 0.0);
    // The page is over the studio: its own panel colour across its middle.
    let p = at(
        &pixels,
        page.frame.x + page.frame.width / 2.0,
        page.frame.bottom() - 6.0,
    );
    assert_eq!(p[..3], theme.palette.panel.0[..3]);
    shoot(
        &song,
        Shot {
            page: Some((VoicebankTab::Find, installed, catalogue)),
            ..Shot::default()
        },
        "vocal2-page-find",
    );
}

/// A portrait file for the picker's cards: generated (a gradient with a
/// face-ish disc), or, with `FONTELLE_TEST_VOICEBANKS` set, a real bank's
/// picture — for looking at, never in git.
fn portrait_file(name: &str, hue: u8, bmp: bool) -> String {
    let dir = std::env::temp_dir().join(format!("fontelle-picker-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (w, h) = (96u32, 96u32);
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let dx = x as f32 - 48.0;
            let dy = y as f32 - 40.0;
            let face = dx * dx + dy * dy < 26.0 * 26.0;
            if face {
                rgba.extend_from_slice(&[250, 220, 200, 255]);
            } else {
                rgba.extend_from_slice(&[hue, (x * 2) as u8, (y * 2) as u8, 255]);
            }
        }
    }
    if bmp {
        let path = dir.join(format!("{name}.bmp"));
        let mut out = Vec::new();
        let row = w * 3;
        let size = 54 + row * h;
        out.extend_from_slice(b"BM");
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&54u32.to_le_bytes());
        out.extend_from_slice(&40u32.to_le_bytes());
        out.extend_from_slice(&(w as i32).to_le_bytes());
        out.extend_from_slice(&(h as i32).to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&24u16.to_le_bytes());
        out.extend_from_slice(&[0; 24]);
        for y in (0..h).rev() {
            for x in 0..w {
                let i = ((y * w + x) * 4) as usize;
                out.extend_from_slice(&[rgba[i + 2], rgba[i + 1], rgba[i]]);
            }
        }
        std::fs::write(&path, out).unwrap();
        path.to_string_lossy().into_owned()
    } else {
        let path = dir.join(format!("{name}.png"));
        let file = std::fs::File::create(&path).unwrap();
        let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w, h);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header().unwrap().write_image_data(&rgba).unwrap();
        path.to_string_lossy().into_owned()
    }
}

/// A real bank's portrait, when the test voicebanks are on this machine.
fn real_portrait(relative: &str) -> Option<String> {
    let root = std::env::var("FONTELLE_TEST_VOICEBANKS").ok()?;
    let path = std::path::Path::new(&root).join(relative);
    path.is_file().then(|| path.to_string_lossy().into_owned())
}

fn picker_singers() -> Vec<SingerChoice> {
    let card = |name: &str, kind: SingerKind, langs: &[&str], image: String| SingerChoice {
        engine: match kind {
            SingerKind::Utau => "fontelle-utau".into(),
            SingerKind::DiffSinger => "fontelle-diffsinger".into(),
            SingerKind::Engine => "Fontelle Test Singer 2".into(),
        },
        engine_name: "Fontelle Test Singer 2".into(),
        id: name.to_lowercase(),
        name: name.into(),
        languages: langs.iter().map(|l| l.to_string()).collect(),
        kind,
        image: Some(image),
        ..Default::default()
    };
    let teto = real_portrait("KasaneTeto_tougou_110401/重音テト音声ライブラリー/teto.bmp")
        .unwrap_or_else(|| portrait_file("teto", 200, true));
    let qixuan = real_portrait("Qixuan_v2.7.0_DiffSinger_OpenUtau/qixuan256x256.png")
        .unwrap_or_else(|| portrait_file("qixuan", 120, false));
    let lang = real_portrait("DiffSinger_lang/lang.jpg")
        .unwrap_or_else(|| portrait_file("lang", 60, false));
    let mut teto = card("重音テト", SingerKind::Utau, &["ja"], teto);
    teto.terms = Some("利用規約".into());
    teto.terms_agreed = true;
    let mut qx = card(
        "Qixuan",
        SingerKind::DiffSinger,
        &["zh", "ja", "en"],
        qixuan,
    );
    qx.terms = Some("terms".into());
    let canary = card("Canary ARPAsing", SingerKind::Utau, &["en"], String::new());
    let lang = card("琅", SingerKind::DiffSinger, &["ja", "zh"], lang);
    let mut engine = card(
        "Test Alto",
        SingerKind::Engine,
        &["en", "ja"],
        String::new(),
    );
    engine.research_only = true;
    vec![teto, qx, canary, lang, engine]
}

#[test]
fn the_singer_picker_is_a_grid_of_cards_with_portraits() {
    let song = song();
    let singers = picker_singers();
    let Some((pixels, theme, _, _)) = shoot(
        &song,
        Shot {
            picker: Some((singers.clone(), String::new(), Some(1))),
            portrait: singers[0].image.clone(),
            ..Shot::default()
        },
        "vocal3-picker",
    ) else {
        return;
    };
    let layout = window_layout(RW as f32, RH as f32, &theme.metrics, 0.0);
    let shown = fontelle_ui::canvas::picker_filter(&singers, "");
    let picker = fontelle_ui::canvas::singer_picker_layout(layout.window, &singers, &shown, 0.0, 0);
    // A portrait is drawn, not a flat colour: many colours in the first card's.
    let first = &picker.cards[0];
    assert!(
        distinct(&pixels, first.portrait) > 20,
        "the portrait is a picture"
    );
    // Searching narrows it.
    shoot(
        &song,
        Shot {
            picker: Some((singers.clone(), "chinese".into(), Some(0))),
            ..Shot::default()
        },
        "vocal3-picker-search",
    );
    // The roll's corner: the singer's face in the chip.
    shoot(
        &song,
        Shot {
            portrait: singers[1].image.clone(),
            ..Shot::default()
        },
        "vocal3-corner",
    );
}
