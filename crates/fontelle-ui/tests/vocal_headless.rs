//! The vocal track's pixels: the roll with words in its notes, phonemes
//! beneath, phrases being sung hatched, the sung pitch, the singer chip and
//! the research badge — and the singer picker. Rendered through the real
//! pipeline with no device; dumped with `FONTELLE_UI_DUMP=<dir>`.
//!
//! Skips, loudly, on a machine with no usable adapter.

use std::sync::{Mutex, OnceLock};

use fontelle_model::{Arena, Note, VocalVoice};
use fontelle_types::{NoteId, PPQN, Tick};
use fontelle_ui::canvas::{
    DEFAULT_LANE_HEIGHT, LaneProperty, RollView, SnapDivision, Tool, key_row, singer_menu_rows,
    tick_to_x, toolbar_layout,
};
use fontelle_ui::document::{KeyMap, SingerChoice, VocalPhraseView, VocalRenderState, VocalView};
use fontelle_ui::layout::window_layout;
use fontelle_ui::render::vocal::{LYRIC_STYLE, PHONEME_STYLE, RollVocal, caption_rect, strings};
use fontelle_ui::render::{
    Chrome, Headless, RenameMarks, RollChrome, TransportChrome, draw_window,
};
use fontelle_ui::text::{Labels, TextContext};
use fontelle_ui::theme::Theme;
use fontelle_ui::transport::{Meter, TransportView, format_readout, transport_bar_layout};

const RW: u32 = 1000;
const RH: u32 = 620;

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

struct Song {
    notes: Arena<NoteId, Note>,
    ids: Vec<NoteId>,
    view: VocalView,
}

/// "Amazing grace" over two phrases: one sung (phonemes, pitch, a guessed
/// word), one still being sung.
fn song() -> Song {
    let mut notes = Arena::default();
    let line = [
        (0.0, 1.0, 64, "a-"),
        (1.0, 1.0, 67, "ma-"),
        (2.0, 1.5, 69, "zing"),
        (3.5, 0.5, 67, "-"),
        (4.0, 2.0, 64, "grace"),
        (7.0, 1.0, 62, "さ"),
        (8.0, 1.0, 64, "く"),
        (9.0, 2.0, 67, "ら"),
    ];
    let ids: Vec<NoteId> = line
        .iter()
        .map(|(s, l, k, w)| notes.insert(note(*s, *l, *k, w)))
        .collect();
    let phonemes = ["ə", "m eɪ", "z ɪ ŋ", "", "g ɹ eɪ s"];
    // The pitch it sang over the first phrase: each note's key, with a
    // little vibrato and a scoop into it.
    let mut pitch = Vec::new();
    for (start, len, key, _) in &line[..5] {
        let steps = (len * 24.0) as usize;
        for i in 0..steps {
            let t = start + i as f64 / 24.0;
            let scoop = (-(i as f32) / 3.0).exp() * -0.6;
            let vib = ((t * 11.0).sin() as f32) * 0.12 * (i as f32 / steps as f32);
            pitch.push(((t * PPQN as f64) as Tick, f32::from(*key) + scoop + vib));
        }
    }
    let view = VocalView {
        caption: "Test Alto \u{b7} en".into(),
        default_lyric: "la".into(),
        research_only: true,
        phrases: vec![
            VocalPhraseView {
                start: 0,
                end: 6 * PPQN,
                state: VocalRenderState::Final,
            },
            VocalPhraseView {
                start: 7 * PPQN,
                end: 11 * PPQN,
                state: VocalRenderState::Waiting,
            },
        ],
        phonemes: ids[..5]
            .iter()
            .zip(phonemes)
            .filter(|(_, p)| !p.is_empty())
            .map(|(id, p)| (*id, p.to_string()))
            .collect(),
        guessed: vec![ids[4]],
        pitch: vec![pitch],
        message: None,
        ..VocalView::default()
    };
    Song { notes, ids, view }
}

fn base(text: &mut TextContext, theme: &Theme) -> (fontelle_ui::layout::WindowLayout, Labels) {
    let layout = window_layout(RW as f32, RH as f32, &theme.metrics, 0.0);
    let _ = text;
    (layout, Labels::new())
}

/// Renders the roll over `song`, with the lyric field open on `editing`
/// (typed `typed`) when there is one, and `menu` over it all.
fn shoot(
    song: &Song,
    editing: Option<(&[NoteId], &str)>,
    menu: Option<&fontelle_ui::canvas::ContextMenu>,
    name: &str,
) -> Option<(Vec<u8>, Theme, fontelle_ui::canvas::RollLayout, RollView)> {
    let theme = Theme::dark_default();
    let shared = headless()?;
    let mut text = TextContext::new();
    let (layout, mut labels) = base(&mut text, &theme);
    let title = text.layout("Roll", &theme.font, None);
    let view = TransportView::unavailable();
    let readout = text.layout(&format_readout(&view, 4), &theme.font, None);
    let tempo = text.layout("120.00", &theme.font, None);
    let signature = text.layout("4/4", &theme.font, None);
    let key_map = KeyMap::unknown();
    let roll_l = fontelle_ui::canvas::roll_layout_with_keys(
        layout.panel.body,
        &theme.metrics,
        DEFAULT_LANE_HEIGHT,
        fontelle_ui::canvas::keyboard_width(&key_map),
    );
    let roll_view = RollView {
        top_key: 74,
        key_offset: 0.0,
        pixels_per_tick: 0.075,
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
    let (words, small) = strings(&song.view, &song.notes, editing.map(|(_, t)| t));
    for w in &words {
        labels.ensure_styled(w, &theme.font, LYRIC_STYLE, &mut text);
    }
    for w in &small {
        labels.ensure_styled(w, &theme.font, PHONEME_STYLE, &mut text);
    }
    if let Some(menu) = menu {
        for entry in &menu.entries {
            labels.ensure(&entry.label, &theme.font, &mut text);
        }
    }
    let caption = labels
        .get_styled(&song.view.caption, LYRIC_STYLE)
        .map(|t| caption_rect(roll_l.grid, t.width));
    let marks = editing.map(|(_, typed)| RenameMarks {
        caret_x: labels
            .get_styled(typed, LYRIC_STYLE)
            .map_or(0.0, |t| t.width),
        selection: None,
        caret_on: true,
    });
    let selection: Vec<NoteId> = editing.map(|(ids, _)| ids.to_vec()).unwrap_or_default();
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
                selection: &selection,
                playhead_tick: Some(PPQN * 2 + PPQN / 2),
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
                vocal: Some(RollVocal {
                    view: &song.view,
                    editing,
                    marks,
                    caption,
                    strips: Default::default(),
                    tabs: &[],
                    curve: None,
                    lane: None,
                    cells: &[],
                    phoneme_field: None,
                    px_per_second: 150.0,
                    engine_chip: None,
                    note_button: None,
                    portrait: None,
                }),
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
            menu,
            carry: None,
            welcome: None,
            keybinds: None,
            help: None,
            tour: None,
            settings_page: None,
            vocal_overlay: None,
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

#[test]
fn the_roll_writes_each_notes_word_inside_it() {
    let song = song();
    let Some((pixels, theme, roll, view)) = shoot(&song, None, None, "vocal-roll") else {
        return;
    };
    // Inside the first note, along where its word is written, some pixels
    // are not the note's colour: the word.
    let n = &song.notes[song.ids[2]];
    let row = key_row(&view, roll.grid, n.key);
    let x0 = tick_to_x(&view, roll.grid, n.start);
    let y = row.y + row.height / 2.0;
    let note_ink = theme.palette.note.0;
    let differs = (0..30)
        .map(|dx| at(&pixels, x0 + 4.0 + dx as f32, y))
        .filter(|p| p[..3] != note_ink[..3])
        .count();
    assert!(differs > 3, "no word drawn in the note ({differs})");
}

#[test]
fn a_phrase_still_being_sung_is_hatched_and_one_sung_is_not() {
    let song = song();
    let Some((pixels, _, roll, view)) = shoot(&song, None, None, "vocal-roll-bands") else {
        return;
    };
    // Low in the grid, away from every note: the waiting phrase's band has
    // hatching through it, the sung one's is the plain grid.
    let y = roll.grid.bottom() - 6.0;
    let sample = |beat: f64| {
        let x0 = tick_to_x(&view, roll.grid, (beat * PPQN as f64) as Tick);
        (0..24)
            .map(|dx| at(&pixels, x0 + dx as f32, y))
            .collect::<std::collections::HashSet<_>>()
            .len()
    };
    assert!(
        sample(7.3) > sample(1.5),
        "the waiting band is busier than the sung one"
    );
}

#[test]
fn the_lyric_field_opens_over_its_note() {
    let song = song();
    let ids = [song.ids[5]];
    shoot(&song, Some((&ids, "さく")), None, "vocal-roll-editing");
    let line: Vec<NoteId> = song.ids[..5].to_vec();
    shoot(
        &song,
        Some((&line, "a-ma-zing grace")),
        None,
        "vocal-roll-line",
    );
}

#[test]
fn the_singer_picker_lists_singers_with_the_research_badge() {
    let song = song();
    let singers = vec![
        SingerChoice {
            engine: "Fontelle Test Singer".into(),
            id: "test.alto".into(),
            name: "Test Alto".into(),
            languages: vec!["en".into(), "ja".into()],
            research_only: false,
            ..Default::default()
        },
        SingerChoice {
            engine: "Fontelle Test Singer".into(),
            id: "test.research".into(),
            name: "Research Voice".into(),
            languages: vec!["ja".into(), "en".into()],
            research_only: true,
            ..Default::default()
        },
    ];
    let current = VocalVoice::new("Fontelle Test Singer", "test.alto", "en");
    let rows = singer_menu_rows(&singers, Some(&current), &[], "");
    let theme = Theme::dark_default();
    let layout = window_layout(RW as f32, RH as f32, &theme.metrics, 0.0);
    let menu = fontelle_ui::canvas::context_menu_layout(
        (420.0, 160.0),
        layout.window,
        &theme.metrics,
        theme.font.size,
        rows.into_iter().map(|(e, _)| e).collect(),
    );
    shoot(&song, None, Some(&menu), "vocal-singer-picker");
    let empty = fontelle_ui::canvas::context_menu_layout(
        (420.0, 160.0),
        layout.window,
        &theme.metrics,
        theme.font.size,
        singer_menu_rows(&[], None, &[], "/home/you/.local/share/fontelle/singers")
            .into_iter()
            .map(|(e, _)| e)
            .collect(),
    );
    shoot(&song, None, Some(&empty), "vocal-singer-picker-none");
}
