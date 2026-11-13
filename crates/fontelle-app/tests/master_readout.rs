//! The Master's loudness readout, from the session to the window (Ty,
//! 2026-11-12: *"loudness readout on master would be good"*).
//!
//! The session says what its tap reads and what the insert aims at; the
//! window puts four cells along its top, above the knobs, and opens tall
//! enough for both.

mod common;

use fontelle_types::EffectKind;
use fontelle_ui::document::StudioHost;

fn a_session_with(kind: EffectKind) -> fontelle_app::Session {
    let mut session = common::a_session_for(common::a_clip_project(4));
    session.add_insert(0, kind);
    session
}

#[test]
fn a_master_insert_has_a_meter_and_says_what_it_aims_at() {
    let mut session = a_session_with(EffectKind::Master);
    let meter = session
        .master_insert_meter(0, 0)
        .expect("a Master has a meter");
    assert_eq!(meter.target_lufs, Some(-14.0));
    assert!(meter.loudness_lufs <= -60.0, "nothing has played");

    // Leveling off: there is nothing it aims at.
    session.set_insert_param(0, 0, "auto_level", 0.0);
    assert_eq!(session.master_insert_meter(0, 0).unwrap().target_lufs, None);
}

#[test]
fn any_other_insert_has_none() {
    let session = a_session_with(EffectKind::Width);
    assert!(session.master_insert_meter(0, 0).is_none());
    assert!(
        session.master_insert_meter(0, 5).is_none(),
        "nor an empty slot"
    );
}

#[test]
fn the_window_puts_the_meter_above_the_knobs_and_opens_tall_enough() {
    let session = a_session_with(EffectKind::Master);
    let mut w = fontelle_ui::Windowless::new(
        Box::new(session),
        fontelle_ui::theme::Theme::dark_default(),
        (1280, 800),
    );
    let (width, height) = w.effect_window_size(0, 0);
    w.open_effect_window(0, 0, (width as f32, height as f32));
    let strip = w.master_meter_strip().expect("the meter is drawn");
    let knobs = w.effect_first_heading().expect("the knobs are drawn");
    assert!(
        strip.bottom() <= knobs.y,
        "above the knobs: {strip:?} / {knobs:?}"
    );
    for (caption, value) in fontelle_ui::canvas::master_meter_cells(&w.master_meter().unwrap()) {
        assert!(w.shaped_small(caption), "{caption} is not shaped");
        assert!(w.shaped(&value), "{value} is not shaped");
    }
    // Every knob still fits: the strip did not push the last section out.
    let last = w.effect_last_cell().expect("a knob");
    assert!(
        last.bottom() <= height as f32,
        "the last knob at {last:?} in a window {height} tall"
    );
}
