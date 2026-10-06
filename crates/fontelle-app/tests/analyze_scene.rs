//! Analyze Musically's window drawn from a **real** analysis: synthesised
//! audio (`fontelle_analysis::testsignals`) dropped on a song, analysed by
//! the session's own job, and the view it hands the window rendered the way
//! the window renders it. The UI's own scenes (`render_headless.rs`) use a
//! made-up view; this is the one that shows what basic-pitch and pYIN
//! actually give the lane.
//!
//! Set `FONTELLE_UI_DUMP` to a directory to look at it.

mod common;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use fontelle_analysis::testsignals;
use fontelle_app::Session;
use fontelle_assets::fixtures::build_wav;
use fontelle_ui::canvas::{AnalyzeMode, AnalyzeState, AnalyzeView};
use fontelle_ui::document::{ClipKind, JobPoll, StudioHost};
use fontelle_ui::render::{
    AnalyzeChrome, EditorWindowChrome, Headless, lay_out_analyze, shape_analyze,
};
use fontelle_ui::text::{Labels, TextContext};
use fontelle_ui::theme::Theme;

use common::SR;

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "fontelle-analyze-scene-{name}-{}",
        std::process::id()
    ));
    std::fs::remove_dir_all(&path).ok();
    std::fs::create_dir_all(&path).expect("creatable");
    path
}

fn analysed_view(dir: &Path, samples: &[f32]) -> AnalyzeView {
    let mut session: Session = common::a_session_in(
        common::a_project_with_a_clip(16, 120.0, SR),
        Some(dir.join("Song")),
    );
    let path = dir.join("Vox take 3.wav");
    std::fs::write(&path, build_wav(SR, 1, samples)).expect("writable");
    session
        .drop_file_on(&path, i64::from(SR) * 4, Some(0))
        .expect("the drop lands");
    let clip = session
        .clips()
        .into_iter()
        .find(|c| c.kind == ClipKind::Audio)
        .expect("an audio clip")
        .id;
    session.analyze_musically(clip).expect("analysable");
    let started = Instant::now();
    while !matches!(
        session.poll_analysis(),
        JobPoll::Finished(_) | JobPoll::Idle
    ) {
        assert!(started.elapsed() < Duration::from_secs(120));
        std::thread::sleep(Duration::from_millis(5));
    }
    session.analyze_view().expect("a view")
}

fn dump(name: &str, view: &AnalyzeView, state: &mut AnalyzeState) {
    let Ok(dir) = std::env::var("FONTELLE_UI_DUMP") else {
        return;
    };
    let mut headless = match Headless::new() {
        Ok(h) => h,
        Err(e) => {
            eprintln!("skipping: no usable GPU adapter ({e})");
            return;
        }
    };
    let theme = Theme::dark_default();
    let (w, h) = fontelle_ui::layout::analyze_window_size(state.scale);
    let panel = fontelle_ui::layout::editor_window_layout(w as f32, h as f32, &theme.metrics);
    let mut text = TextContext::new();
    let mut labels = Labels::new();
    let font = theme.font.clone();
    let title = text.layout(
        &format!("Analyze Musically \u{b7} {}", view.name),
        &theme.for_bridge().font,
        None,
    );
    let first = lay_out_analyze(&mut labels, &mut text, &font, panel.body, view, state);
    state.fit(view, &first);
    let layout = lay_out_analyze(&mut labels, &mut text, &font, panel.body, view, state);
    shape_analyze(&mut labels, &mut text, &font, view, state, &layout);
    let mut scene = fontelle_ui::vello::Scene::new();
    fontelle_ui::render::draw_editor_window(
        &mut scene,
        &theme,
        &panel,
        &labels,
        &title,
        &EditorWindowChrome::Analyze(AnalyzeChrome {
            layout,
            view,
            state,
            tooltip: None,
            skin: None,
        }),
        None,
        None,
        None,
        None,
    );
    let pixels = headless
        .render(&scene, w, h, theme.palette.window)
        .expect("the scene renders");
    let path = Path::new(&dir).join(format!("{name}.png"));
    let file = std::fs::File::create(&path).expect("somewhere to write");
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .and_then(|mut writer| writer.write_image_data(&pixels))
        .expect("the frame");
    eprintln!("wrote {}", path.display());
}

#[test]
fn a_sung_line_and_a_chord_part_as_the_window_shows_them() {
    let dir = scratch("real");
    // The sung line: melody mode, pYIN's notes with their vibrato.
    let sung = testsignals::vibrato_melody(SR);
    let view = analysed_view(&dir, &sung.samples);
    assert_eq!(view.detected, Some(AnalyzeMode::Melody));
    let mut state = AnalyzeState::default();
    state.click_note(2, false);
    dump("analyze-real-melody", &view, &mut state);

    // A melody over chords: chords mode, every note, the chord lane.
    let mix = testsignals::melody_and_chords(SR);
    let dir2 = scratch("real-mix");
    let view = analysed_view(&dir2, &mix.samples);
    assert_eq!(view.detected, Some(AnalyzeMode::Chords));
    // The waveform behind them (Tab), and the pitch picture: what opens.
    let mut state = AnalyzeState::default();
    state.spectrogram = false;
    dump("analyze-real-chords", &view, &mut state);
    let mut state = AnalyzeState::default();
    dump("analyze-real-chords-pitch", &view, &mut state);
    std::fs::remove_dir_all(&dir).ok();
    std::fs::remove_dir_all(&dir2).ok();
}
