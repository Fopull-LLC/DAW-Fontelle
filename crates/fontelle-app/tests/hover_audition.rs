//! Resting the pointer on a preset hears it (Ty, 2026-11-12: *"hovering over
//! a search result to hear the preset would be good"*).
//!
//! After a 400 ms rest on one row (`docs/flopsynth-next.md` §5.2's figure),
//! once per visit, and never while a button is held — so a pointer on its way
//! somewhere sounds nothing, and a row looked at for a while sounds once.
//! Only where a listen changes nothing: soundfont presets, which play on the
//! preview voice, and Flopsynth's own bank. The Presets tab's listen loads
//! the preset onto the channel, so hovering there would be edits nobody made.

mod common;

use std::path::{Path, PathBuf};
use std::time::Duration;

use fontelle_app::Session;
use fontelle_types::InstrumentKind;
use fontelle_ui::canvas::{BrowserMode, PresetDevice};
use fontelle_ui::document::{LibraryKind, StudioHost};
use fontelle_ui::layout::Rect;

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "fontelle-hover-audition-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::remove_dir_all(&path).ok();
    std::fs::create_dir_all(&path).expect("the scratch folder must be creatable");
    path
}

/// A real soundfont of one preset, as `browsing.rs` makes them.
fn a_soundfont(dir: &Path, name: &str) {
    use fontelle_assets::fixtures::{
        GEN_KEY_RANGE, GEN_OVERRIDING_ROOT_KEY, Sf2Fixture, ZoneSpec, build_sf2, gen_range, gen_val,
    };
    let fixture = Sf2Fixture {
        samples: (0..32).map(|i| (i * 500 - 8_000) as i16).collect(),
        sample_rate: 44_100,
        header_start: 0,
        header_end: 32,
        header_loop_start: 4,
        header_loop_end: 28,
        origpitch: 60,
        pitchadj: 0,
        zone: ZoneSpec {
            generators: vec![
                gen_range(GEN_KEY_RANGE, 0, 127),
                gen_val(GEN_OVERRIDING_ROOT_KEY, 60),
            ],
        },
        extra_zones: Vec::new(),
    };
    std::fs::write(dir.join(name), build_sf2(&fixture)).unwrap();
}

fn middle_of(r: Rect) -> (f32, f32) {
    (r.x + r.width / 2.0, r.y + r.height / 2.0)
}

fn window(session: Session) -> fontelle_ui::Windowless {
    fontelle_ui::Windowless::new(
        Box::new(session),
        fontelle_ui::theme::Theme::dark_default(),
        (1280, 800),
    )
}

/// Whether anything is heard within `ms`, looking as often as a frame.
fn heard_within(w: &mut fontelle_ui::Windowless, ms: u64) -> bool {
    let until = std::time::Instant::now() + Duration::from_millis(ms);
    while std::time::Instant::now() < until {
        w.idle(Duration::from_millis(16));
        if w.auditioning().is_some() {
            return true;
        }
    }
    false
}

fn let_it_end(w: &mut fontelle_ui::Windowless) {
    for _ in 0..40 {
        if w.auditioning().is_none() {
            return;
        }
        w.idle(Duration::from_millis(50));
    }
    panic!("the audition never ended");
}

#[test]
fn resting_on_a_soundfont_preset_hears_it_once() {
    let dir = scratch("sounds");
    a_soundfont(&dir, "Piano.sf2");
    let mut session = common::a_session_for(common::a_clip_project(4));
    session.add_soundfont_dir(&dir);
    session.open_bank();
    session.set_browser_mode(BrowserMode::Sounds);
    let file = session
        .library_files()
        .iter()
        .position(|e| e.name == "Piano")
        .expect("the soundfont is listed");
    session.open_file(file).unwrap();
    let row = session
        .library_presets()
        .iter()
        .position(|e| e.kind != LibraryKind::Group)
        .expect("a preset row");

    let mut w = window(session);
    w.show_browser_mode(BrowserMode::Sounds);
    let (x, y) = middle_of(w.browser_preset_row(row).expect("the row is drawn"));
    w.move_studio(x, y);
    assert!(!heard_within(&mut w, 250), "passing over is not a listen");
    assert!(heard_within(&mut w, 500), "a rest on the row hears it");

    // Once per visit: staying on it does not play it again.
    let_it_end(&mut w);
    assert!(!heard_within(&mut w, 800), "heard once, not over and over");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_presets_tab_hears_nothing_on_a_rest() {
    let dir = scratch("presets");
    let mut session = common::a_session_for(common::a_clip_project(4));
    session.set_browser_mode(BrowserMode::Presets);
    let device = session
        .library_files()
        .iter()
        .position(|e| e.name == "Flopsynth")
        .expect("Flopsynth has presets");
    session.open_file(device).unwrap();
    let rows = session.library_presets();
    let row = rows
        .iter()
        .position(|e| e.kind != LibraryKind::Group)
        .expect("a preset row");

    let mut w = window(session);
    w.show_browser_mode(BrowserMode::Presets);
    let before = w.document().preset_bar(PresetDevice::Instrument).name;
    let (x, y) = middle_of(w.browser_preset_row(row).expect("the row is drawn"));
    w.move_studio(x, y);
    assert!(
        !heard_within(&mut w, 800),
        "a listen here would load it onto the channel"
    );
    assert_eq!(
        w.document().preset_bar(PresetDevice::Instrument).name,
        before,
        "hovering is no edit"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn resting_on_a_flopsynth_bank_row_hears_it_without_loading_it() {
    let mut session = common::a_session_for(common::a_clip_project(4));
    session.set_channel_kind(0, InstrumentKind::Flopsynth);

    let mut w = window(session);
    let (width, height) = fontelle_ui::layout::flopsynth_window_size(1.0);
    w.open_instrument_panel((width as f32, height as f32));
    w.show_flop_presets();
    let loaded = w.document().preset_bar(PresetDevice::Instrument).name;
    let rows = w.flop_preset_rows();
    let (_, rect) = *rows.get(1).expect("the bank is listed");
    let (x, y) = middle_of(rect);
    w.move_instrument_panel(x, y);
    assert!(!heard_within(&mut w, 250), "passing over is not a listen");
    assert!(heard_within(&mut w, 500), "a rest on a bank row hears it");
    assert_eq!(
        w.document().preset_bar(PresetDevice::Instrument).name,
        loaded,
        "a listen is not a load"
    );
}
