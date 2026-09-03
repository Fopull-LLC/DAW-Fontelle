//! The audio clip editor (TDD §15.1).
//!
//! Reported from using the window:
//!
//! > *"double clicking on an audio clip should open a menu that lets me make
//! > changes to that audio like basic changes that would be widely general
//! > useful across all audio editing so for example changing the boost or the
//! > cutoff or resonance of the sound or even changing a fade in or fade out or
//! > stuff like that. just need all the features we would expect or need for a
//! > full audio suite."*
//!
//! Its shape is this window's own: **rows of a name and a value, where a click
//! steps the value forward and a Ctrl+click steps it back**, under headings
//! that say what each group is for. No text field, because there is not one in
//! this window and a value you can reach in a handful of clicks is quicker than
//! one you have to type. Across the top is the clip's own waveform, so what you
//! are changing is in front of you while you change it.
//!
//! Everything here is pure: the panel is geometry, the properties are
//! `AudioClipData`, and *"what does a click on this row do"* is a function from
//! one of those to another. So the whole editor is checkable without a window.

use fontelle_types::{
    AssetKind, AssetRef, AudioClipData, ClipLoopMode, FadeCurve, FilterShape,
};
use fontelle_ui::canvas::{
    AUDIO_ROWS, AudioField, audio_editor_hit, audio_editor_layout, audio_row_label,
    audio_row_value, nudge_audio_row,
};
use fontelle_ui::layout::Rect;
use fontelle_ui::theme::Theme;

fn metrics() -> fontelle_ui::theme::Metrics {
    Theme::dark_default().metrics
}

fn body() -> Rect {
    Rect::new(0.0, 0.0, 460.0, 620.0)
}

fn a_clip() -> AudioClipData {
    AudioClipData::whole(
        AssetRef {
            id: fontelle_types::AssetId::default(),
            path: "take.wav".into(),
            content_hash: 0,
            size: 0,
            kind: AssetKind::Sample,
        },
        48_000,
    )
}

/// Steps `field` `n` times in `direction`.
fn stepped(mut clip: AudioClipData, field: AudioField, direction: i32, n: usize) -> AudioClipData {
    for _ in 0..n {
        nudge_audio_row(&mut clip, field, direction, 48_000);
    }
    clip
}

// -------------------------------------------------------------- the rows ---

#[test]
fn every_row_says_what_it_is_and_what_it_is_at() {
    // The same rule the settings tab and the tool dialogs follow: a row with no
    // name cannot be identified, and a row with no value cannot be read. A
    // heading is the exception — it is all name.
    let clip = a_clip();
    for row in AUDIO_ROWS {
        assert!(!audio_row_label(row).is_empty(), "{row:?} has no name");
        if matches!(row, AudioField::Heading(_)) {
            continue;
        }
        assert!(
            !audio_row_value(&clip, row).is_empty(),
            "{row:?} does not say what it is at"
        );
    }
}

#[test]
fn everything_that_was_asked_for_is_on_it() {
    // *"the boost or the cutoff or resonance of the sound or even changing a
    // fade in or fade out"*, by name.
    for wanted in [
        AudioField::Gain,
        AudioField::Cutoff,
        AudioField::Resonance,
        AudioField::FadeIn,
        AudioField::FadeOut,
    ] {
        assert!(AUDIO_ROWS.contains(&wanted), "{wanted:?} is not on the editor");
    }
}

#[test]
fn a_heading_is_a_row_a_click_does_nothing_to() {
    let clip = a_clip();
    for row in AUDIO_ROWS {
        if matches!(row, AudioField::Heading(_)) {
            assert_eq!(stepped(clip.clone(), row, 1, 3), clip);
        }
    }
}

// ------------------------------------------------------------- stepping ---

#[test]
fn the_boost_steps_in_decibels_and_stops_at_both_ends() {
    // A number stops and a choice wraps — the rule the rest of the window
    // follows. A gain that wrapped from +24 to -60 on one extra click is a
    // control nobody can trust to a held press.
    let clip = a_clip();
    let up = stepped(clip.clone(), AudioField::Gain, 1, 3);
    assert!(up.gain_db > 0.0);
    let down = stepped(clip.clone(), AudioField::Gain, -1, 3);
    assert!(down.gain_db < 0.0);

    let top = stepped(clip.clone(), AudioField::Gain, 1, 500);
    assert_eq!(top.gain_db, fontelle_types::MAX_CLIP_GAIN_DB);
    let bottom = stepped(clip, AudioField::Gain, -1, 5000);
    assert_eq!(bottom.gain_db, fontelle_types::MIN_CLIP_GAIN_DB);
}

#[test]
fn the_cutoff_steps_by_ear_rather_than_by_hertz() {
    // Equal steps in Hz are useless: a hundred hertz is a third of the way up
    // the bass and inaudible at the top. So it steps by a ratio, which is what
    // a filter knob does.
    let clip = a_clip();
    let mut low = clip.clone();
    low.filter.cutoff_hz = 200.0;
    let mut high = clip;
    high.filter.cutoff_hz = 8000.0;

    let low_step = stepped(low.clone(), AudioField::Cutoff, 1, 1).filter.cutoff_hz
        - low.filter.cutoff_hz;
    let high_step = stepped(high.clone(), AudioField::Cutoff, 1, 1).filter.cutoff_hz
        - high.filter.cutoff_hz;
    assert!(
        high_step > low_step * 4.0,
        "the step is nearly constant: {low_step} at 200 Hz, {high_step} at 8 kHz"
    );
}

#[test]
fn the_cutoff_never_leaves_what_the_filter_can_do() {
    let clip = a_clip();
    let top = stepped(clip.clone(), AudioField::Cutoff, 1, 500);
    assert!(top.filter.cutoff_hz <= fontelle_types::MAX_FILTER_HZ);
    let bottom = stepped(clip, AudioField::Cutoff, -1, 500);
    assert!(bottom.filter.cutoff_hz >= fontelle_types::MIN_FILTER_HZ);
}

#[test]
fn the_resonance_runs_from_none_to_all_of_it_and_no_further() {
    let clip = a_clip();
    assert_eq!(clip.filter.resonance, 0.0);
    let up = stepped(clip.clone(), AudioField::Resonance, 1, 500);
    assert_eq!(up.filter.resonance, 1.0);
    let down = stepped(clip, AudioField::Resonance, -1, 500);
    assert_eq!(down.filter.resonance, 0.0);
}

#[test]
fn a_choice_wraps_where_a_number_stops() {
    // Six filter shapes are a set with no ends; stopping at one would mean
    // knowing to go back through the others to reach it.
    let clip = a_clip();
    let mut seen = Vec::new();
    let mut walking = clip.clone();
    for _ in 0..FilterShape::ALL.len() {
        walking = stepped(walking, AudioField::FilterShape, 1, 1);
        seen.push(walking.filter.shape);
    }
    assert_eq!(seen.last(), Some(&clip.filter.shape), "it did not come round");

    let mut curves = Vec::new();
    let mut walking = clip.clone();
    for _ in 0..FadeCurve::ALL.len() {
        walking = stepped(walking, AudioField::FadeInCurve, 1, 1);
        curves.push(walking.fade_in.curve);
    }
    assert_eq!(curves.last(), Some(&clip.fade_in.curve));

    for _ in 0..ClipLoopMode::ALL.len() {
        walking = stepped(walking, AudioField::Loop, 1, 1);
    }
    assert_eq!(walking.loop_mode, clip.loop_mode);
}

#[test]
fn the_switches_are_switches() {
    let clip = a_clip();
    assert!(stepped(clip.clone(), AudioField::Reverse, 1, 1).reverse);
    assert!(!stepped(clip.clone(), AudioField::Reverse, 1, 2).reverse);
    assert!(stepped(clip.clone(), AudioField::Normalize, -1, 1).normalize);
}

#[test]
fn a_fade_is_stepped_in_time_and_can_never_be_longer_than_the_clip() {
    // A fade longer than what it fades is a clip that never reaches full
    // level, which is a thing to be able to ask for by mistake and not a thing
    // to be stuck with.
    let clip = a_clip();
    assert_eq!(clip.fade_in.frames, 0);
    let some = stepped(clip.clone(), AudioField::FadeIn, 1, 2);
    assert!(some.fade_in.frames > 0);
    let all = stepped(clip.clone(), AudioField::FadeIn, 1, 500);
    assert!(
        all.fade_in.frames <= clip.source_frames(),
        "a {}-frame fade over a {}-frame clip",
        all.fade_in.frames,
        clip.source_frames()
    );
    let none = stepped(clip, AudioField::FadeOut, -1, 500);
    assert_eq!(none.fade_out.frames, 0, "a fade cannot be negative");
}

#[test]
fn the_speed_and_the_pitch_stay_inside_what_the_player_will_read() {
    let clip = a_clip();
    let fast = stepped(clip.clone(), AudioField::Speed, 1, 500);
    assert!(fast.speed <= fontelle_types::MAX_CLIP_SPEED);
    assert!(fast.rate().is_finite());
    let slow = stepped(clip.clone(), AudioField::Speed, -1, 500);
    assert!(slow.speed >= fontelle_types::MIN_CLIP_SPEED);

    let up = stepped(clip.clone(), AudioField::Pitch, 1, 500);
    assert!(up.pitch_semitones <= 48.0);
    let down = stepped(clip, AudioField::Pitch, -1, 500);
    assert!(down.pitch_semitones >= -48.0);
}

#[test]
fn nothing_a_click_can_do_makes_a_clip_the_player_would_choke_on() {
    // The blanket claim, swept over every row both ways. Everything here
    // reaches the audio thread.
    let mut clip = a_clip();
    for row in AUDIO_ROWS {
        for direction in [1, -1] {
            clip = stepped(clip, row, direction, 40);
            assert!(clip.gain().is_finite(), "{row:?} made the gain {}", clip.gain());
            assert!(clip.rate().is_finite() && clip.rate() > 0.0);
            assert!(clip.source_frames() >= 0);
            assert!(clip.fade_gain(0.0).is_finite());
            assert!((0.0..=1.0).contains(&clip.fade_gain(10.0)));
        }
    }
}

// ----------------------------------------------------------- the layout ---

#[test]
fn the_editor_shows_the_clip_it_is_editing() {
    // A list of numbers with nothing to look at is a list of numbers. The
    // waveform across the top is what makes a fade something you can aim.
    let l = audio_editor_layout(body(), &metrics(), AUDIO_ROWS.len());
    assert!(!l.waveform.is_empty());
    assert!(l.waveform.y < l.rows[0].1.y, "the waveform is under the rows");
    for (field, rect) in &l.rows {
        assert!(!rect.intersects(&l.waveform), "{field:?} is over the waveform");
    }
}

#[test]
fn a_click_on_a_row_finds_that_row() {
    let l = audio_editor_layout(body(), &metrics(), AUDIO_ROWS.len());
    for (field, rect) in &l.rows {
        if rect.is_empty() {
            continue;
        }
        let (x, y) = (rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
        assert_eq!(audio_editor_hit(&l, x, y), Some(*field));
    }
    assert_eq!(audio_editor_hit(&l, -10.0, -10.0), None);
}

#[test]
fn nothing_in_the_editor_is_drawn_outside_it_or_over_anything_else() {
    let l = audio_editor_layout(body(), &metrics(), AUDIO_ROWS.len());
    for (field, rect) in &l.rows {
        if rect.is_empty() {
            continue;
        }
        assert_eq!(
            rect.intersection(&body()),
            *rect,
            "{field:?} escapes the panel"
        );
    }
    for (i, (a_field, a)) in l.rows.iter().enumerate() {
        for (b_field, b) in l.rows.iter().skip(i + 1) {
            if a.is_empty() || b.is_empty() {
                continue;
            }
            assert!(!a.intersects(b), "{a_field:?} is over {b_field:?}");
        }
    }
}

#[test]
fn a_panel_too_short_for_every_row_still_shows_what_it_can() {
    let short = Rect::new(0.0, 0.0, 400.0, 90.0);
    let l = audio_editor_layout(short, &metrics(), AUDIO_ROWS.len());
    assert_eq!(l.rows.len(), AUDIO_ROWS.len(), "the list changed length");
    for (field, rect) in &l.rows {
        assert!(rect.width >= 0.0 && rect.height >= 0.0, "{field:?} is {rect:?}");
        assert_eq!(rect.intersection(&short), *rect, "{field:?} escapes");
    }
}

// ------------------------------------------------------- opening it ---
//
// *"double clicking on an audio clip should open a menu."* Double, and the
// window had no notion of one — every other panel here opens on a single click,
// because a clip you clicked is a clip you meant. An audio clip is different
// and the difference is the report: clicking one is how you *move* it, and a
// window that appeared every time you nudged a take along the bar would be in
// the way of the thing you were doing.
//
// So it is a pure decision about two clicks and where they were, decided here
// rather than inside an event loop where nothing could test it.

use fontelle_ui::pointer::{DOUBLE_CLICK_SLOP, DOUBLE_CLICK_WINDOW, DoubleClick};
use std::time::{Duration, Instant};

#[test]
fn two_quick_clicks_in_one_place_are_a_double_click() {
    let mut clicks = DoubleClick::default();
    let now = Instant::now();
    assert!(!clicks.press(100.0, 100.0, now), "one click is not two");
    assert!(clicks.press(100.0, 100.0, now + Duration::from_millis(120)));
}

#[test]
fn a_third_click_does_not_make_a_second_double() {
    // Otherwise a triple-click opens two windows, and a held-down mash opens
    // one per click.
    let mut clicks = DoubleClick::default();
    let now = Instant::now();
    clicks.press(10.0, 10.0, now);
    assert!(clicks.press(10.0, 10.0, now + Duration::from_millis(80)));
    assert!(
        !clicks.press(10.0, 10.0, now + Duration::from_millis(160)),
        "the third click counted as a second double"
    );
}

#[test]
fn two_clicks_far_apart_in_time_are_two_clicks() {
    let mut clicks = DoubleClick::default();
    let now = Instant::now();
    clicks.press(10.0, 10.0, now);
    assert!(!clicks.press(10.0, 10.0, now + DOUBLE_CLICK_WINDOW + Duration::from_millis(1)));
}

#[test]
fn two_clicks_far_apart_on_screen_are_two_clicks() {
    // Clicking one clip and then another is two clips opened, not one editor.
    let mut clicks = DoubleClick::default();
    let now = Instant::now();
    clicks.press(10.0, 10.0, now);
    assert!(!clicks.press(10.0 + DOUBLE_CLICK_SLOP + 1.0, 10.0, now + Duration::from_millis(50)));
}

#[test]
fn a_hand_that_wobbles_between_two_clicks_still_double_clicked() {
    let mut clicks = DoubleClick::default();
    let now = Instant::now();
    clicks.press(10.0, 10.0, now);
    assert!(clicks.press(11.0, 11.0, now + Duration::from_millis(50)));
}
