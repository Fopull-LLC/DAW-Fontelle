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
//! Its shape is **rows of a name and a control**, under headings that say what
//! each group is for: a slider for anything continuous, a switch for anything
//! that is on or off, a drop-down for anything that is one of a list. Across
//! the top is the clip's own waveform, so what you are changing is in front of
//! you while you change it.
//!
//! It was one gesture for all eighteen — a click that stepped forward and a
//! Ctrl+click that stepped back — and that is what *"the pitch changing should
//! be a knob but instead its a button i click to iteratively go through a list
//! of pre made values"* was about. Stepping stayed, as the wheel.
//!
//! Everything here is pure: the panel is geometry, the properties are
//! `AudioClipData`, and *"what does a click on this row do"* is a function from
//! one of those to another. So the whole editor is checkable without a window.

use fontelle_types::{
    AssetKind, AssetRef, AudioClipData, ClipLoopMode, ClipStretch, FadeCurve, FilterShape,
};
use fontelle_ui::canvas::{
    AUDIO_ROWS, AudioField, MAX_CLIP_PITCH, MIN_CLIP_PITCH, audio_editor_hit, audio_editor_layout,
    audio_row_label, audio_row_value, nudge_audio_row, nudge_route,
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
        assert!(
            AUDIO_ROWS.contains(&wanted),
            "{wanted:?} is not on the editor"
        );
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

    let low_step = stepped(low.clone(), AudioField::Cutoff, 1, 1)
        .filter
        .cutoff_hz
        - low.filter.cutoff_hz;
    let high_step = stepped(high.clone(), AudioField::Cutoff, 1, 1)
        .filter
        .cutoff_hz
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
    assert_eq!(
        seen.last(),
        Some(&clip.filter.shape),
        "it did not come round"
    );

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
    assert!(fast.time_rate().is_finite() && fast.read_rate().is_finite());
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
            assert!(
                clip.gain().is_finite(),
                "{row:?} made the gain {}",
                clip.gain()
            );
            assert!(clip.time_rate().is_finite() && clip.time_rate() > 0.0);
            assert!(clip.read_rate().is_finite() && clip.read_rate() > 0.0);
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
    assert!(
        l.waveform.y < l.rows[0].1.y,
        "the waveform is under the rows"
    );
    for (field, rect) in &l.rows {
        assert!(
            !rect.intersects(&l.waveform),
            "{field:?} is over the waveform"
        );
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
        assert!(
            rect.width >= 0.0 && rect.height >= 0.0,
            "{field:?} is {rect:?}"
        );
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
    assert!(!clicks.press(
        10.0,
        10.0,
        now + DOUBLE_CLICK_WINDOW + Duration::from_millis(1)
    ));
}

#[test]
fn two_clicks_far_apart_on_screen_are_two_clicks() {
    // Clicking one clip and then another is two clips opened, not one editor.
    let mut clicks = DoubleClick::default();
    let now = Instant::now();
    clicks.press(10.0, 10.0, now);
    assert!(!clicks.press(
        10.0 + DOUBLE_CLICK_SLOP + 1.0,
        10.0,
        now + Duration::from_millis(50)
    ));
}

#[test]
fn a_hand_that_wobbles_between_two_clicks_still_double_clicked() {
    let mut clicks = DoubleClick::default();
    let now = Instant::now();
    clicks.press(10.0, 10.0, now);
    assert!(clicks.press(11.0, 11.0, now + Duration::from_millis(50)));
}

// ------------------------------------------- the two rows that were owed ---
//
// > *"soundclips dont have options right now for selecting their mixer track
// > and stretch mode."*

#[test]
fn a_clip_says_which_mixer_track_it_plays_through_and_can_be_pointed_at_another() {
    // The routing is on the clip already — `AudioClipData::mixer_track`, which
    // is what a take is given so that recording through a strip means
    // something. It had no row.
    assert!(
        AUDIO_ROWS.contains(&AudioField::Route),
        "there is nowhere to choose a clip's mixer track"
    );

    // Master, then every track somebody made: the same list, in the same
    // order, that the rack's route chip and the mixer's output row read.
    let tracks = a_few_tracks();
    let mut clip = a_clip();
    assert_eq!(clip.mixer_track, None, "a fresh clip goes to the master");

    nudge_route(&mut clip, 1, &tracks);
    assert_eq!(clip.mixer_track, tracks[1]);
    nudge_route(&mut clip, 1, &tracks);
    assert_eq!(clip.mixer_track, tracks[2]);
    // A choice wraps, the same rule every other choice on this panel follows.
    nudge_route(&mut clip, 1, &tracks);
    assert_eq!(clip.mixer_track, None, "the list did not come round");
    nudge_route(&mut clip, -1, &tracks);
    assert_eq!(clip.mixer_track, tracks[2], "and it did not go back");
}

#[test]
fn a_clip_pointed_at_a_track_that_is_no_longer_there_still_steps() {
    // A project whose bus was deleted since. The row must not be a dead end.
    let tracks = a_few_tracks();
    let mut clip = a_clip();
    clip.mixer_track = Some(an_absent_track());
    nudge_route(&mut clip, 1, &tracks);
    assert!(
        tracks.contains(&clip.mixer_track),
        "it stepped to something that is not in the list"
    );
}

#[test]
fn a_clip_says_whether_it_follows_the_tempo() {
    assert!(
        AUDIO_ROWS.contains(&AudioField::Stretch),
        "there is nowhere to choose a clip's stretch mode"
    );
    let clip = a_clip();
    assert_eq!(
        clip.stretch,
        ClipStretch::Off,
        "off is the only safe default"
    );
    assert_eq!(audio_row_value(&clip, AudioField::Stretch), "Off");

    let on = stepped(a_clip(), AudioField::Stretch, 1, 1);
    assert_eq!(on.stretch, ClipStretch::Resample);
    assert_eq!(audio_row_value(&on, AudioField::Stretch), "Resample");
    // And it wraps, like every other choice here.
    let back = stepped(on, AudioField::Stretch, 1, 1);
    assert_eq!(back.stretch, ClipStretch::Off);
}

#[test]
fn the_two_new_rows_are_under_headings_that_say_what_they_are_for() {
    // A panel of twenty rows is only readable because it is grouped, and a row
    // that lands in the wrong group is one nobody finds — the Tools panel
    // shipped exactly that mistake once.
    let heading_before = |wanted: AudioField| {
        AUDIO_ROWS
            .iter()
            .take_while(|f| **f != wanted)
            .filter_map(|f| match f {
                AudioField::Heading(title) => Some(*title),
                _ => None,
            })
            .last()
    };
    assert_eq!(heading_before(AudioField::Route), Some("Track"));
    assert_eq!(
        heading_before(AudioField::Stretch),
        Some("Time"),
        "the stretch mode belongs with pitch and speed"
    );
}

/// The master and two tracks, in the order the mixer lays them out — which is
/// the order every route list in this window is in.
fn a_few_tracks() -> Vec<Option<fontelle_types::MixerTrackId>> {
    let mut map: slotmap::SlotMap<fontelle_types::MixerTrackId, ()> = slotmap::SlotMap::with_key();
    vec![None, Some(map.insert(())), Some(map.insert(()))]
}

fn an_absent_track() -> fontelle_types::MixerTrackId {
    let mut map: slotmap::SlotMap<fontelle_types::MixerTrackId, ()> = slotmap::SlotMap::with_key();
    map.insert(())
}

// ------------------------------------------------------- the controls (§15.1) ---
//
// > *"right now a lot of options that could be knobs or sliders or dropdowns
// > for some reason are instead shown as buttons you click to toggle through a
// > list of options in order iteratively. this is really annoying ... for
// > example, this is happening in the audio clip editing panel right now the
// > pitch changing should be a knob but instead its a button i click to
// > iteratively go through a list of pre made values."*
//
// So every row now says what kind of control it is, and the three kinds are
// the three shapes a value can have: continuous, on/off, one of a list.

use fontelle_ui::canvas::{
    AudioControl, audio_row_choices, audio_row_chosen, audio_row_control, audio_row_control_rect,
    audio_row_fraction, audio_row_is_on, audio_row_neutral, audio_slider_at, audio_slider_x_of,
    choose_audio_row, set_audio_row_fraction, toggle_audio_row,
};

#[test]
fn every_row_has_a_control_and_a_heading_has_none() {
    for field in AUDIO_ROWS {
        let control = audio_row_control(field);
        match field {
            AudioField::Heading(_) => assert_eq!(control, AudioControl::None, "{field:?}"),
            _ => assert_ne!(control, AudioControl::None, "{field:?} has nothing to set"),
        }
    }
}

#[test]
fn the_continuous_values_are_sliders_and_nothing_steps_through_them() {
    // The row the complaint named, and every row like it.
    for field in [
        AudioField::Gain,
        AudioField::Pan,
        AudioField::Pitch,
        AudioField::Speed,
        AudioField::FadeIn,
        AudioField::FadeOut,
        AudioField::Cutoff,
        AudioField::Resonance,
        AudioField::Drive,
    ] {
        assert_eq!(audio_row_control(field), AudioControl::Slider, "{field:?}");
        assert!(
            audio_row_fraction(&a_clip(), field).is_some(),
            "{field:?} is a slider with nowhere to sit"
        );
    }
}

#[test]
fn a_list_of_values_is_a_drop_down_and_an_on_off_is_a_switch() {
    for field in [
        AudioField::Route,
        AudioField::Stretch,
        AudioField::Loop,
        AudioField::FadeInCurve,
        AudioField::FadeOutCurve,
        AudioField::FilterShape,
    ] {
        assert_eq!(audio_row_control(field), AudioControl::Choice, "{field:?}");
    }
    for field in [AudioField::Normalize, AudioField::Reverse] {
        assert_eq!(audio_row_control(field), AudioControl::Switch, "{field:?}");
    }
}

#[test]
fn a_drop_down_lists_what_the_row_can_be_and_says_which_it_is_on() {
    let mut clip = a_clip();
    for field in [
        AudioField::Stretch,
        AudioField::Loop,
        AudioField::FadeInCurve,
        AudioField::FadeOutCurve,
        AudioField::FilterShape,
    ] {
        let choices = audio_row_choices(field);
        assert!(choices.len() >= 2, "{field:?} listed {choices:?}");
        let at = audio_row_chosen(&clip, field).expect("a row is always on one of its entries");
        assert_eq!(
            choices[at],
            audio_row_value(&clip, field),
            "the ticked entry and the read-out disagree on {field:?}"
        );
        // Straight to the last one, in a single press, which is the whole
        // point: six filter shapes used to be five clicks away.
        let last = choices.len() - 1;
        choose_audio_row(&mut clip, field, last);
        assert_eq!(audio_row_chosen(&clip, field), Some(last), "{field:?}");
        assert_eq!(audio_row_value(&clip, field), choices[last]);
    }
}

#[test]
fn the_route_row_leaves_its_list_to_the_window() {
    // Which mixer tracks exist is the document's, and this crate may not see
    // one — so the route row is a drop-down whose entries come from outside.
    assert_eq!(audio_row_control(AudioField::Route), AudioControl::Choice);
    assert!(audio_row_choices(AudioField::Route).is_empty());
}

#[test]
fn choosing_a_row_out_of_range_leaves_it_alone() {
    let mut clip = a_clip();
    let before = clip.filter.shape;
    choose_audio_row(&mut clip, AudioField::FilterShape, 99);
    assert_eq!(clip.filter.shape, before);
}

#[test]
fn a_switch_flips_and_says_which_way_it_is() {
    let mut clip = a_clip();
    for field in [AudioField::Normalize, AudioField::Reverse] {
        assert!(!audio_row_is_on(&clip, field), "{field:?} starts off");
        toggle_audio_row(&mut clip, field);
        assert!(audio_row_is_on(&clip, field), "{field:?}");
        toggle_audio_row(&mut clip, field);
        assert!(!audio_row_is_on(&clip, field), "{field:?}");
    }
}

#[test]
fn a_slider_reads_back_what_it_was_set_to() {
    // The round trip, which is what makes the handle land under the pointer.
    for field in [
        AudioField::Gain,
        AudioField::Pan,
        AudioField::Speed,
        AudioField::FadeIn,
        AudioField::Cutoff,
        AudioField::Resonance,
        AudioField::Drive,
    ] {
        for t in [0.0f32, 0.13, 0.37, 0.62, 0.88, 1.0] {
            let mut clip = a_clip();
            set_audio_row_fraction(&mut clip, field, t);
            let back = audio_row_fraction(&clip, field).unwrap();
            assert!(
                (back - t).abs() < 0.03,
                "{field:?} was set to {t} and reads back {back}"
            );
        }
    }
}

#[test]
fn the_pitch_slider_covers_the_whole_range_and_lands_on_semitones() {
    // *"the pitch changing should be a knob"* — four octaves each way, reached
    // by dragging rather than by ninety-six clicks, and detented so that the
    // note you are aiming for is the one you get.
    let mut clip = a_clip();
    set_audio_row_fraction(&mut clip, AudioField::Pitch, 0.0);
    assert_eq!(clip.pitch_semitones, MIN_CLIP_PITCH);
    set_audio_row_fraction(&mut clip, AudioField::Pitch, 1.0);
    assert_eq!(clip.pitch_semitones, MAX_CLIP_PITCH);
    set_audio_row_fraction(&mut clip, AudioField::Pitch, 0.5);
    assert_eq!(
        clip.pitch_semitones, 0.0,
        "the middle of the track is unison"
    );

    for t in [0.1f32, 0.25, 0.4, 0.63, 0.9] {
        set_audio_row_fraction(&mut clip, AudioField::Pitch, t);
        assert_eq!(
            clip.pitch_semitones,
            clip.pitch_semitones.round(),
            "{t} gave {} semitones, which is not a note",
            clip.pitch_semitones
        );
    }
}

#[test]
fn the_sliders_that_are_ratios_run_logarithmically() {
    // Ten per cent of half speed and ten per cent of double speed are the same
    // musical distance and different numbers, so the track is spaced the way
    // stepping already was — not linearly.
    let mut clip = a_clip();
    set_audio_row_fraction(&mut clip, AudioField::Speed, 0.5);
    let middle = clip.speed;
    assert!(
        middle < 1.5,
        "the middle of a linear speed track would be {middle:.2}\u{d7}"
    );
    set_audio_row_fraction(&mut clip, AudioField::Cutoff, 0.5);
    assert!(
        clip.filter.cutoff_hz < 4000.0,
        "the middle of the cutoff track is a musical middle, got {} Hz",
        clip.filter.cutoff_hz
    );
}

#[test]
fn the_useful_values_have_detents_because_a_hand_cannot_land_on_them() {
    let mut clip = a_clip();
    // Unity gain, dead centre, normal speed: each reachable from a fraction
    // that is merely near it.
    let unity = audio_row_neutral(AudioField::Gain).unwrap();
    set_audio_row_fraction(&mut clip, AudioField::Gain, unity + 0.004);
    assert_eq!(clip.gain_db, 0.0);
    let centre = audio_row_neutral(AudioField::Pan).unwrap();
    set_audio_row_fraction(&mut clip, AudioField::Pan, centre + 0.01);
    assert_eq!(clip.pan, 0.0);
    let normal = audio_row_neutral(AudioField::Speed).unwrap();
    set_audio_row_fraction(&mut clip, AudioField::Speed, normal - 0.004);
    assert_eq!(clip.speed, 1.0);
}

#[test]
fn a_sliders_fill_grows_from_the_value_that_does_nothing() {
    // A cut and a boost have to look like opposite things, not like the same
    // thing by different amounts.
    let gain = audio_row_neutral(AudioField::Gain).unwrap();
    assert!(
        gain > 0.1 && gain < 0.9,
        "0 dB is not at an end, got {gain}"
    );
    assert_eq!(audio_row_neutral(AudioField::Pan), Some(0.5));
    assert_eq!(audio_row_neutral(AudioField::Pitch), Some(0.5));
    // Wide open does nothing, and that is the top of the track.
    assert_eq!(audio_row_neutral(AudioField::Cutoff), Some(1.0));
    // And these start at nothing, so they grow from the left.
    for field in [AudioField::FadeIn, AudioField::Resonance, AudioField::Drive] {
        assert_eq!(audio_row_neutral(field), Some(0.0), "{field:?}");
    }
    assert_eq!(
        audio_row_neutral(AudioField::Loop),
        None,
        "a choice has no fill"
    );
}

#[test]
fn a_fade_slider_spends_its_travel_where_the_useful_lengths_are() {
    // The useful fades span three orders of magnitude — a five-millisecond
    // click-remover and a four-second swell — so a linear track would spend
    // nine tenths of itself on lengths nobody asks for.
    let mut clip = a_clip();
    set_audio_row_fraction(&mut clip, AudioField::FadeIn, 0.5);
    let quarter = clip.source_frames() / 4;
    assert!(
        (clip.fade_in.frames - quarter).abs() <= 1,
        "half the track is a quarter of the clip, got {} of {}",
        clip.fade_in.frames,
        clip.source_frames()
    );
    // Never longer than what it fades.
    set_audio_row_fraction(&mut clip, AudioField::FadeIn, 1.0);
    assert_eq!(clip.fade_in.frames, clip.source_frames());
}

#[test]
fn a_track_sits_in_the_right_hand_half_of_its_row_and_maps_both_ways() {
    let layout = audio_editor_layout(body(), &metrics(), AUDIO_ROWS.len());
    let (_, row) = layout
        .rows
        .iter()
        .find(|(field, _)| *field == AudioField::Gain)
        .expect("the boost row");
    let track = audio_row_control_rect(*row, &metrics());
    assert!(!track.is_empty());
    assert!(
        track.x > row.x + row.width / 3.0,
        "the name needs its column"
    );
    assert!(track.right() <= row.right() + 0.01);
    assert!(track.y >= row.y - 0.01 && track.bottom() <= row.bottom() + 0.01);

    // A press lands where it looks like it lands, and the far ends are the
    // ends of the range rather than somewhere past them.
    for t in [0.0f32, 0.25, 0.5, 0.75, 1.0] {
        let x = audio_slider_x_of(*row, &metrics(), t);
        assert!(
            (audio_slider_at(*row, &metrics(), x) - t).abs() < 0.01,
            "at {t}"
        );
    }
    assert_eq!(audio_slider_at(*row, &metrics(), row.x - 500.0), 0.0);
    assert_eq!(audio_slider_at(*row, &metrics(), row.right() + 500.0), 1.0);
}

#[test]
fn stepping_still_works_because_that_is_what_a_wheel_over_a_control_does() {
    // The gesture that was the only way in is now the fine one: a wheel moves
    // a value by exactly one of whatever it is measured in.
    let mut clip = a_clip();
    nudge_audio_row(&mut clip, AudioField::Pitch, 1, 48_000);
    assert_eq!(clip.pitch_semitones, 1.0);
    nudge_audio_row(&mut clip, AudioField::Pitch, -1, 48_000);
    assert_eq!(clip.pitch_semitones, 0.0);
}
