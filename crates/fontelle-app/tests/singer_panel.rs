//! The Fontelle Singer as a complete instrument: opening a vocal track's
//! instrument shows its singer (portrait, name, author, kind, languages,
//! terms) and the controls a UTAU, Vocaloid or Synthesizer V user expects —
//! the voice, the expression a note without its own gets, the bank's own
//! settings, the output — every one a command (a knob's drag one undo step)
//! that reaches what the singer is asked to sing. An engine's instrument is
//! not blank either: its own window opens where a plugin's would, and one
//! with no window gets the same panel of what it declares.
//!
//! > *"theres no instrument visuals for each yet, please go and create the
//! > generic vocal plugin instrument fully it should be done after this. it
//! > should have its own visuals and knobs and stuff in the instrument that
//! > people would expect and [the engine instrument] also shouldnt be
//! > blank."* — Ty
//!
//! Engines are `fontelle-testsinger`; the Fontelle Singer's runtime is a
//! fake that hums whatever is in the test's own voicebanks folder.

mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use fontelle_app::Session;
use fontelle_model::{VocalStyle, lane_spec};
use fontelle_sing::{
    Backend, Capabilities, Lane, Phrase, Quality, Session as SingSession, SettingInfo, SingError,
    Sung, VoiceInfo,
};
use fontelle_types::{InstrumentKind, NoteId, PPQN, ParamAddress};
use fontelle_ui::canvas::{InstrumentView, ParamKind, RollEdit};
use fontelle_ui::document::{DocumentHost, FONTELLE_SINGER, SingerKind, StudioHost};
use fontelle_vocal::{BuiltinBackend, SingerKind as Kind};

use fontelle_testsinger::{BRIGHTNESS_SETTING, ENGINE_NAME, ENGINE2_NAME};

fn library_name(stem: &str) -> String {
    if cfg!(target_os = "windows") {
        format!("{stem}.dll")
    } else if cfg!(target_os = "macos") {
        format!("lib{stem}.dylib")
    } else {
        format!("lib{stem}.so")
    }
}

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "fontelle-singer-panel-{name}-{}",
        std::process::id()
    ));
    std::fs::remove_dir_all(&path).ok();
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn singer_folder() -> PathBuf {
    static FOLDER: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    FOLDER
        .get_or_init(|| {
            let mut path = std::env::current_exe().unwrap();
            path.pop();
            path.pop();
            let built = path.join(library_name("fontelle_testsinger"));
            assert!(built.exists(), "run `cargo build -p fontelle-testsinger`");
            let folder = scratch("singers");
            std::fs::copy(&built, folder.join(library_name("fontelle_testsinger"))).unwrap();
            std::fs::copy(
                &built,
                folder.join(library_name("fontelle_testsinger_abi2")),
            )
            .unwrap();
            folder
        })
        .clone()
}

/// The Fontelle Singer's runtime for the test: each installed bank a voice
/// that answers to dynamics, breathiness, vibrato and glides, with a
/// phonemizer choice and a voice colour — and not to gender or tension.
struct Banks(PathBuf);

impl Backend for Banks {
    fn name(&self) -> &str {
        "Test Banks"
    }
    fn version(&self) -> String {
        "1".into()
    }
    fn voices(&self) -> Vec<VoiceInfo> {
        fontelle_vocal::voicebanks::list(&self.0)
            .into_iter()
            .map(|b| VoiceInfo {
                id: b.id.clone(),
                name: b.name.clone(),
                languages: vec!["ja".into(), "en".into()],
                lowest: 48,
                highest: 72,
                research_only: false,
                terms: None,
                image: None,
            })
            .collect()
    }
    fn capabilities(&self, _voice: &str) -> Capabilities {
        Capabilities {
            lanes: vec![Lane::PitchBend, Lane::Dynamics, Lane::Breathiness],
            vibrato: true,
            portamento: true,
            velocity: true,
            settings: vec![SettingInfo {
                id: "phonemizer".into(),
                name: "Phonemizer".into(),
                min: 0.0,
                max: 2.0,
                default: 0.0,
                unit: String::new(),
                choices: vec![
                    "Automatic".into(),
                    "Japanese CV".into(),
                    "Japanese VCV".into(),
                ],
            }],
            note_options: vec![fontelle_sing::NoteOptionInfo {
                id: "colour".into(),
                name: "Voice colour".into(),
                choices: vec![
                    (String::new(), "Default".into()),
                    ("soft".into(), "Soft".into()),
                    ("power".into(), "Power".into()),
                ],
            }],
            ..Capabilities::default()
        }
    }
    fn session(&self) -> Result<Box<dyn SingSession>, String> {
        Ok(Box::new(Hum))
    }
}

struct Hum;

impl SingSession for Hum {
    fn sing(
        &mut self,
        phrase: &Phrase,
        _quality: Quality,
        _seed: u64,
        _cancelled: &dyn Fn() -> bool,
    ) -> Result<Sung, SingError> {
        let end = phrase.notes.last().map_or(0.0, |n| n.end());
        Ok(Sung {
            sample_rate: 8_000,
            samples: vec![0.1; (end * 8_000.0) as usize + 1],
            ..Sung::default()
        })
    }
}

fn banks(root: &Path) -> Arc<dyn Backend> {
    Arc::new(Banks(root.to_path_buf()))
}

fn builtins() -> Vec<BuiltinBackend> {
    vec![BuiltinBackend {
        key: "fontelle-utau",
        kind: Kind::Utau,
        make: banks,
    }]
}

fn install_teto(voicebanks: &Path) {
    let inner = voicebanks
        .join("KasaneTeto_English")
        .join("\u{91cd}\u{97f3}\u{30c6}\u{30c8}");
    std::fs::create_dir_all(&inner).unwrap();
    std::fs::write(inner.join("oto.ini"), b"a.wav=- a,20,100,-200,50,30\n").unwrap();
    std::fs::write(inner.join("teto.bmp"), b"BM").unwrap();
    std::fs::write(inner.join("sample.wav"), b"RIFF").unwrap();
    let text = encoding_rs::SHIFT_JIS
        .encode(
            "name=\u{91cd}\u{97f3}\u{30c6}\u{30c8}\r\nimage=teto.bmp\r\nauthor=TWINDRILL\r\nsample=sample.wav\r\n",
        )
        .0
        .into_owned();
    std::fs::write(inner.join("character.txt"), text).unwrap();
}

fn session(name: &str) -> (Session, PathBuf) {
    let dir = scratch(name);
    let project = common::a_clip_project(4);
    let bundle = dir.join("Song.fontelle");
    fontelle_app::save_project(&project, &bundle).unwrap();
    let mut session = common::a_session_in(project, Some(bundle));
    session.set_singer_folders(vec![singer_folder()]);
    let voicebanks = dir.join("voicebanks");
    install_teto(&voicebanks);
    session.set_voicebanks(voicebanks, builtins());
    session.set_headless_singer_editors(true);
    (session, dir)
}

fn vocal_index(session: &Session) -> usize {
    (0..session.channels().len())
        .rev()
        .find(|i| session.channel_kind(*i) == Some(InstrumentKind::Vocal))
        .expect("a vocal channel")
}

/// A vocal track of `engine`'s instrument, selected, with two long notes.
fn track(session: &mut Session, engine: Option<&str>) -> usize {
    session.add_singer_channel(engine).unwrap();
    let index = vocal_index(session);
    session.select_channel(index);
    draw(session, 0, 64, "la");
    draw(session, 4, 67, "li");
    index
}

fn draw(session: &mut Session, beat: i64, key: u8, lyric: &str) -> NoteId {
    let id = session.edit(RollEdit::Add {
        note: fontelle_model::Note {
            start: beat * PPQN,
            length: PPQN * 3,
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
            lyric: String::new(),
            voice: None,
        },
    })[0];
    session.set_lyrics(&[id], vec![lyric.into()]).unwrap();
    id
}

/// The one vocal channel in these songs.
fn vocal_id(session: &Session, _index: usize) -> fontelle_types::ChannelId {
    let mut vocal = session
        .project()
        .channels
        .iter()
        .filter(|(_, c)| c.instrument == Some(InstrumentKind::Vocal));
    let (id, _) = vocal.next().expect("a vocal channel");
    assert!(vocal.next().is_none(), "one vocal channel");
    id
}

fn style(session: &Session, index: usize) -> VocalStyle {
    let id = vocal_id(session, index);
    session.project().channels[id].vocal_style.clone()
}

fn phrases(session: &Session, index: usize) -> Vec<fontelle_vocal::Phrase> {
    let id = vocal_id(session, index);
    fontelle_vocal::phrases_of(
        session.project(),
        id,
        &fontelle_model::effective_tempo_map(session.project()),
    )
}

fn addresses(view: &InstrumentView, group: &str) -> Vec<String> {
    view.groups
        .iter()
        .find(|g| g.name == group)
        .unwrap_or_else(|| panic!("a {group} section"))
        .params
        .iter()
        .map(|p| p.address.as_str().to_string())
        .collect()
}

fn set(session: &mut Session, address: &str, value: f32) {
    session.set_instrument_param(&ParamAddress::new(address), value);
}

#[test]
fn the_fontelle_singer_shows_its_singer_and_every_section() {
    let (mut session, _) = session("sections");
    let index = track(&mut session, None);
    let header = session.singer_panel().expect("a singer panel");
    assert_eq!(header.instrument, FONTELLE_SINGER);
    assert_eq!(header.engine, None);
    let singer = header.singer.as_ref().expect("a singer");
    assert_eq!(singer.name, "\u{91cd}\u{97f3}\u{30c6}\u{30c8}");
    assert_eq!(singer.kind, SingerKind::Utau);
    assert_eq!(singer.languages, vec!["ja", "en"]);
    assert!(
        singer
            .image
            .as_deref()
            .is_some_and(|p| p.ends_with("teto.bmp"))
    );
    assert_eq!(header.author.as_deref(), Some("TWINDRILL"));
    assert_eq!(header.bank.as_deref(), Some("KasaneTeto_English"));
    assert!(header.sample, "the bank names a sample to play");
    assert!(!header.has_editor);
    assert_eq!(session.vocal_voice(index).unwrap().language, "ja");

    let view = session.instrument().expect("not blank");
    let names: Vec<&str> = view.groups.iter().map(|g| g.name.as_str()).collect();
    assert_eq!(names, ["Voice", "Expression", "Performance", "Output"]);
    let voice = addresses(&view, "Voice");
    for lane in ["gender", "breathiness", "tension", "voicing"] {
        assert!(voice.contains(&format!("vocal/lane/{lane}")), "{voice:?}");
    }
    let expression = addresses(&view, "Expression");
    for a in [
        "vocal/vibrato/on",
        "vocal/vibrato/depth",
        "vocal/vibrato/rate",
        "vocal/vibrato/delay",
        "vocal/vibrato/fade_in",
        "vocal/vibrato/fade_out",
        "vocal/vibrato/min_length",
        "vocal/glide/on",
        "vocal/glide/length",
        "vocal/glide/shape",
        "vocal/consonant_speed",
    ] {
        assert!(expression.contains(&a.to_string()), "{a}: {expression:?}");
    }
    let performance = addresses(&view, "Performance");
    assert!(performance.contains(&"vocal/setting/phonemizer".to_string()));
    assert!(performance.contains(&"vocal/option/colour".to_string()));
    let output = addresses(&view, "Output");
    assert_eq!(output, ["mixer/gain", "mixer/pan"]);
    // A declared list is a list, named.
    let phonemizer = view
        .groups
        .iter()
        .flat_map(|g| &g.params)
        .find(|p| p.address.as_str() == "vocal/setting/phonemizer")
        .unwrap();
    assert_eq!(
        phonemizer.kind,
        ParamKind::Choice(vec![
            "Automatic".into(),
            "Japanese CV".into(),
            "Japanese VCV".into()
        ])
    );
    assert_eq!(phonemizer.display, "Automatic");
}

/// Gender and tension are drawn greyed, with why: this voice ignores them.
#[test]
fn a_control_the_voice_ignores_is_greyed_and_says_why() {
    let (mut session, _) = session("greyed");
    track(&mut session, None);
    let header = session.singer_panel().unwrap();
    let why = |a: &str| {
        header
            .greyed
            .iter()
            .find(|(address, _)| address == a)
            .map(|(_, why)| why.clone())
    };
    let gender = why("vocal/lane/gender").expect("gender is greyed");
    assert!(gender.contains("doesn't use"), "{gender}");
    assert!(why("vocal/lane/tension").is_some());
    assert!(why("vocal/lane/breathiness").is_none(), "it uses breath");
    assert!(why("vocal/vibrato/depth").is_none());
}

#[test]
fn a_knob_drag_is_one_undo_step_and_reaches_the_phrase() {
    let (mut session, _) = session("drag");
    let index = track(&mut session, None);
    let before = session.undo_depth();
    for v in [0.55, 0.65, 0.75, 1.0] {
        set(&mut session, "vocal/lane/breathiness", v);
    }
    session.end_gesture();
    assert_eq!(session.undo_depth(), before + 1, "one step");
    let max = lane_spec(Lane::Breathiness).max;
    assert_eq!(
        style(&session, index).lanes.get(&Lane::Breathiness),
        Some(&max)
    );
    let p = &phrases(&session, index)[0];
    let breath = &p.sing.curves[&Lane::Breathiness];
    assert!(breath.values.iter().all(|v| (v - max).abs() < 1e-4));
    // The panel reads it back.
    let view = session.instrument().unwrap();
    let knob = view
        .groups
        .iter()
        .flat_map(|g| &g.params)
        .find(|p| p.address.as_str() == "vocal/lane/breathiness")
        .unwrap();
    assert!((knob.value - 1.0).abs() < 1e-4);
    session.undo();
    assert!(style(&session, index).is_default());
}

#[test]
fn auto_vibrato_glide_and_consonant_speed_reach_notes_without_their_own() {
    let (mut session, _) = session("expression");
    let index = track(&mut session, None);
    let p = &phrases(&session, index)[0];
    assert!(p.sing.notes.iter().all(|n| n.vibrato.is_none()));
    set(&mut session, "vocal/vibrato/on", 1.0);
    session.end_gesture();
    set(&mut session, "vocal/vibrato/depth", 0.5);
    session.end_gesture();
    set(&mut session, "vocal/glide/on", 1.0);
    session.end_gesture();
    set(&mut session, "vocal/consonant_speed", 1.0);
    session.end_gesture();
    let s = style(&session, index);
    let auto = s.vibrato.expect("auto vibrato set");
    assert!(auto.enabled);
    assert!(s.glide.is_some_and(|g| g.enabled));
    assert!(s.consonant_speed > 1.0);
    let all = phrases(&session, index);
    let notes: Vec<_> = all.iter().flat_map(|p| &p.sing.notes).collect();
    assert!(
        notes.iter().all(|n| n.vibrato == Some(auto.vibrato)),
        "both long notes wobble"
    );
    assert!(
        notes
            .iter()
            .all(|n| (n.velocity - s.consonant_speed).abs() < 1e-4)
    );
    // The switch shows on, the depth its value.
    let view = session.instrument().unwrap();
    let on = view
        .groups
        .iter()
        .flat_map(|g| &g.params)
        .find(|p| p.address.as_str() == "vocal/vibrato/on")
        .unwrap();
    assert_eq!(on.kind, ParamKind::Switch);
    assert_eq!(on.value, 1.0);
}

#[test]
fn the_banks_settings_and_voice_colour_are_the_instruments() {
    let (mut session, _) = session("bank");
    let index = track(&mut session, None);
    set(&mut session, "vocal/setting/phonemizer", 1.0);
    session.end_gesture();
    set(&mut session, "vocal/option/colour", 0.5);
    session.end_gesture();
    let s = style(&session, index);
    assert_eq!(s.settings["fontelle-utau"]["phonemizer"], 2.0);
    assert_eq!(s.options["fontelle-utau"]["colour"], "soft");
    let p = &phrases(&session, index)[0];
    assert_eq!(p.sing.settings["phonemizer"], 2.0);
    assert!(p.sing.notes.iter().all(|n| n.options["colour"] == "soft"));
    let view = session.instrument().unwrap();
    let phonemizer = view
        .groups
        .iter()
        .flat_map(|g| &g.params)
        .find(|p| p.address.as_str() == "vocal/setting/phonemizer")
        .unwrap();
    assert_eq!(phonemizer.display, "Japanese VCV");
}

/// Alt-click puts a control back; typing a number sets it.
#[test]
fn a_control_resets_to_its_default_and_takes_a_typed_value() {
    let (mut session, _) = session("reset");
    track(&mut session, None);
    let gender = ParamAddress::new("vocal/lane/gender");
    let default = session
        .instrument_param_default_value(&gender)
        .expect("a default");
    assert!((default - 0.5).abs() < 1e-4, "no shift is the middle");
    let typed = session
        .instrument_param_from_text(&gender, "300")
        .expect("cents read");
    assert!((typed - 0.75).abs() < 1e-3, "{typed}");
}

#[test]
fn an_engine_with_a_window_opens_it_as_its_instrument() {
    let (mut session, _) = session("engine-window");
    let index = track(&mut session, Some(ENGINE2_NAME));
    let header = session.singer_panel().expect("a panel all the same");
    assert_eq!(header.instrument, ENGINE2_NAME);
    assert_eq!(header.engine.as_deref(), Some(ENGINE2_NAME));
    assert!(header.has_editor);
    // Opening the instrument opens the engine's own window, the way a
    // plugin's own editor opens instead of the knob grid.
    assert!(session.open_plugin_editor_for_channel(index));
    assert!(session.singer_editor_open());
    session.close_singer_editor();
    // Its declared setting is a knob on the panel too, and sings.
    let view = session.instrument().unwrap();
    let performance = addresses(&view, "Performance");
    let address = format!("vocal/setting/{BRIGHTNESS_SETTING}");
    assert!(performance.contains(&address), "{performance:?}");
    set(&mut session, &address, 1.0);
    session.end_gesture();
    let p = &phrases(&session, index)[0];
    assert_eq!(p.sing.settings[BRIGHTNESS_SETTING], 1.0);
}

#[test]
fn an_engine_without_a_window_gets_the_panel_of_what_it_declares() {
    let (mut session, _) = session("engine-plain");
    let index = track(&mut session, Some(ENGINE_NAME));
    let header = session.singer_panel().unwrap();
    assert_eq!(header.instrument, ENGINE_NAME);
    assert!(!header.has_editor);
    assert!(!session.open_plugin_editor_for_channel(index));
    let view = session.instrument().expect("not blank");
    assert!(view.groups.iter().any(|g| g.name == "Output"));
    // A version-1 engine honours nothing beyond notes and words: every
    // voice and expression control is greyed, saying so.
    assert!(
        header
            .greyed
            .iter()
            .any(|(a, why)| a == "vocal/vibrato/on" && why.contains(ENGINE_NAME))
    );
}

/// A track nobody sings yet still has its panel, saying what to do.
#[test]
fn an_engine_track_with_nobody_chosen_says_choose_a_singer() {
    let (mut session, _) = session("nobody");
    let index = track(&mut session, Some(ENGINE2_NAME));
    session.set_singer(
        index,
        Some(fontelle_model::VocalVoice::new(ENGINE2_NAME, "", "en")),
    );
    let header = session.singer_panel().unwrap();
    assert!(header.singer.is_none());
    assert!(
        header
            .message
            .as_deref()
            .is_some_and(|m| m.contains("Choose a singer")),
        "{:?}",
        header.message
    );
}

/// Ty agreed to Teto's terms without being able to read them: a bank whose
/// terms are not in English and that the Find singers index knows comes
/// with the index's English summary, and is marked for the Translate row.
#[test]
fn foreign_terms_come_with_the_index_summary() {
    let (mut session, dir) = session("terms");
    let bank = dir.join("voicebanks").join("KasaneTeto_tougou_110401");
    std::fs::create_dir_all(&bank).unwrap();
    std::fs::write(bank.join("oto.ini"), b"a.wav=- a,20,100,-200,50,30\n").unwrap();
    std::fs::write(
        bank.join("readme.txt"),
        "重音テトの音声ライブラリーは無料でご利用いただけます。再配布・販売は禁止です。",
    )
    .unwrap();
    let english = dir.join("voicebanks").join("My_Own_Bank");
    std::fs::create_dir_all(&english).unwrap();
    std::fs::write(english.join("oto.ini"), b"a.wav=- a,20,100,-200,50,30\n").unwrap();
    std::fs::write(english.join("readme.txt"), "Free to use. Credit me.").unwrap();
    let voicebanks = dir.join("voicebanks");
    session.set_voicebanks(voicebanks, builtins());
    let rows = session.voicebanks();
    let teto = rows
        .iter()
        .find(|r| r.id == "KasaneTeto_tougou_110401")
        .expect("installed");
    assert!(teto.terms_foreign, "Japanese terms are foreign here");
    assert!(
        teto.terms_summary
            .as_deref()
            .is_some_and(|s| s.starts_with("Free to download")),
        "{:?}",
        teto.terms_summary
    );
    let own = rows.iter().find(|r| r.id == "My_Own_Bank").unwrap();
    assert!(!own.terms_foreign);
    assert_eq!(own.terms_summary, None);
    // The picker's card says the same.
    let singers = session.singers();
    let card = singers
        .iter()
        .find(|s| s.folder.as_deref() == Some("KasaneTeto_tougou_110401"))
        .expect("a card for it");
    assert!(card.terms_foreign);
    assert!(card.terms_summary.is_some());
}
