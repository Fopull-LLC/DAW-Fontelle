//! The Analyze Musically window (`docs/analyze-musically-plan.md` §3.1–§3.4,
//! §4 P1): pure geometry and pure answers.
//!
//! What a person can reach — that the whole window fits the size it opens at
//! at every scale, that everything the pointer can be over says what it is,
//! that a click on the scale copies it — and what the lane promises: a note
//! drawn on its row, the ones that are off saying by how much, the rows
//! outside the scale dimmed.

use std::sync::Arc;

use fontelle_types::KeyScale;
use fontelle_ui::canvas::{
    AnalyzeAction, AnalyzeClarity, AnalyzeHit, AnalyzeKey, AnalyzeLayout, AnalyzeMode, AnalyzePage,
    AnalyzeState, AnalyzeView, AnalyzedChord, AnalyzedNote, RowShade, SCALES, analyze_cents_tag,
    analyze_hit, analyze_layout, analyze_press, analyze_row_shade, analyze_strings, analyze_tip,
    estimated_width, scale_degrees,
};
use fontelle_ui::layout::{Rect, analyze_minimum_size, analyze_window_size, editor_window_layout};
use fontelle_ui::theme::Theme;

fn a_note(start: f64, end: f64, midi: u8, cents: f32, poly: bool) -> AnalyzedNote {
    AnalyzedNote {
        start,
        end,
        midi,
        cents,
        amplitude: 0.7,
        confidence: 0.9,
        poly,
        curve: (0..=10)
            .map(|i| {
                let t = start + (end - start) * f64::from(i) / 10.0;
                (t, cents + 8.0 * (f64::from(i) * 0.9).sin() as f32)
            })
            .collect(),
    }
}

/// A sung line in A minor over 12 s, one note 23 cents sharp, and a chord
/// reading of it.
fn a_view() -> AnalyzeView {
    let melody = vec![
        a_note(0.5, 1.4, 69, 4.0, false),
        a_note(1.5, 2.3, 72, -6.0, false),
        a_note(2.4, 3.6, 71, 23.0, false),
        a_note(3.8, 5.0, 69, -18.0, false),
        a_note(5.2, 7.0, 64, 2.0, false),
    ];
    let notes = vec![
        a_note(0.5, 1.4, 69, 4.0, true),
        a_note(0.5, 3.0, 57, 0.0, true),
        a_note(2.4, 3.6, 71, 23.0, true),
        a_note(5.2, 7.0, 64, 2.0, true),
    ];
    AnalyzeView {
        name: "Vox take 3".to_string(),
        duration: 12.0,
        analysing: None,
        error: None,
        detected: Some(AnalyzeMode::Melody),
        melody,
        notes,
        chords: vec![
            AnalyzedChord {
                start: 0.0,
                end: 3.0,
                label: "Am".to_string(),
            },
            AnalyzedChord {
                start: 3.0,
                end: 6.0,
                label: "F".to_string(),
            },
        ],
        key: Some(AnalyzeKey {
            key: KeyScale::new(9, "natural-minor"),
            confidence: 0.82,
            relative: KeyScale::new(0, "major"),
            tonic_confidence: 0.7,
            alternatives: vec![KeyScale::new(0, "major"), KeyScale::new(9, "dorian")],
        }),
        clarity: Some((AnalyzeClarity::Clear, 0.91)),
        clarity_reason: "Clean, pitched audio".to_string(),
        tuning_cents: Some(6.0),
        bpm: None,
        peaks: (0..1200)
            .map(|i| {
                let v = ((i as f32) * 0.05).sin().abs() * 0.6;
                (-v, v)
            })
            .collect::<Vec<_>>()
            .into(),
        peaks_per_second: 100.0,
        spectrogram: None,
    }
}

fn state(scale: f32) -> AnalyzeState {
    let mut state = AnalyzeState::default();
    state.scale = scale;
    state
}

fn measure(text: &str) -> f32 {
    estimated_width(text)
}

fn laid_out(
    width: f32,
    height: f32,
    view: &AnalyzeView,
    state: &mut AnalyzeState,
) -> AnalyzeLayout {
    let metrics = Theme::dark_default().metrics;
    let panel = editor_window_layout(width, height, &metrics);
    let layout = analyze_layout(panel.body, view, state, &measure);
    state.fit(view, &layout);
    analyze_layout(panel.body, view, state, &measure)
}

fn inside(outer: Rect, inner: Rect) -> bool {
    inner.x >= outer.x - 0.5
        && inner.y >= outer.y - 0.5
        && inner.right() <= outer.right() + 0.5
        && inner.bottom() <= outer.bottom() + 0.5
}

fn centre(r: Rect) -> (f32, f32) {
    (r.x + r.width / 2.0, r.y + r.height / 2.0)
}

fn overlaps(a: Rect, b: Rect) -> bool {
    a.x < b.right() - 0.5
        && b.x < a.right() - 0.5
        && a.y < b.bottom() - 0.5
        && b.y < a.bottom() - 0.5
}

// ------------------------------------------------------------ layout ---

#[test]
fn the_window_fits_at_every_scale_and_at_its_smallest() {
    let view = a_view();
    for scale in SCALES {
        for (w, h) in [analyze_window_size(scale), analyze_minimum_size(scale)] {
            let mut s = state(scale);
            let l = laid_out(w as f32, h as f32, &view, &mut s);
            let named = l.named();
            assert!(named.len() >= 18, "{named:?}");
            for (name, rect) in &named {
                assert!(!rect.is_empty(), "{name} is empty at {scale} in {w}x{h}");
                assert!(
                    inside(l.body, *rect),
                    "{name} {rect:?} is outside {:?} at {scale} in {w}x{h}",
                    l.body
                );
            }
            // Nothing in the header strip sits on anything else.
            let header: Vec<_> = named
                .iter()
                .filter(|(n, _)| n.starts_with("header."))
                .collect();
            for (i, (a, ra)) in header.iter().enumerate() {
                for (b, rb) in &header[i + 1..] {
                    assert!(!overlaps(*ra, *rb), "{a} overlaps {b} at {scale}");
                }
            }
            // The lane is the instrument: it gets the room.
            assert!(
                l.lane.grid.height >= l.body.height * 0.4,
                "the lane is {} of {} at {scale} in {w}x{h}",
                l.lane.grid.height,
                l.body.height
            );
            for (name, rect) in named.iter().filter(|(n, _)| n.starts_with("card.")) {
                assert!(!overlaps(*rect, l.canopy), "{name} is over the lane");
            }
        }
    }
}

#[test]
fn every_scale_opens_bigger_than_the_one_before() {
    let sizes: Vec<_> = SCALES.iter().map(|s| analyze_window_size(*s)).collect();
    assert_eq!(analyze_window_size(1.0), (1180, 740));
    assert_eq!(analyze_minimum_size(1.0), (900, 600));
    for pair in sizes.windows(2) {
        assert!(pair[1].0 > pair[0].0 && pair[1].1 > pair[0].1);
    }
}

#[test]
fn the_job_strip_is_there_only_while_analysing() {
    let mut view = a_view();
    let (w, h) = analyze_window_size(1.0);
    let l = laid_out(w as f32, h as f32, &view, &mut state(1.0));
    assert!(l.job.is_empty());
    view.analysing = Some(0.4);
    let l = laid_out(w as f32, h as f32, &view, &mut state(1.0));
    assert!(!l.job.is_empty());
    assert!(inside(l.body, l.job));
    assert!(!overlaps(l.job, l.canopy));
    let strings = analyze_strings(&view, &state(1.0), &l);
    assert!(
        strings.iter().any(|(s, _)| s.contains("40 %")),
        "{strings:?}"
    );
}

#[test]
fn the_other_pages_say_they_are_coming_rather_than_being_empty() {
    let view = a_view();
    let (w, h) = analyze_window_size(1.0);
    for page in [AnalyzePage::Clean, AnalyzePage::Slice, AnalyzePage::Record] {
        let mut s = state(1.0);
        s.page = page;
        let l = laid_out(w as f32, h as f32, &view, &mut s);
        assert!(!l.later.is_empty(), "{page:?} shows its card");
        assert!(l.lane.grid.is_empty(), "and not the lane");
        let strings = analyze_strings(&view, &s, &l);
        assert!(
            strings
                .iter()
                .any(|(text, _)| text.contains("coming in a later update")),
            "{page:?}: {strings:?}"
        );
    }
}

// ------------------------------------------------------- hits, tips ---

#[test]
fn everything_the_pointer_can_be_over_says_what_it_is() {
    let view = a_view();
    let (w, h) = analyze_window_size(1.0);
    let mut s = state(1.0);
    let l = laid_out(w as f32, h as f32, &view, &mut s);
    // Every named control, at its centre.
    for (name, rect) in l.named() {
        let (x, y) = centre(rect);
        if let Some(hit) = analyze_hit(&l, &view, &s, x, y) {
            let tip = analyze_tip(&hit, &view, &s);
            assert!(
                tip.as_deref().is_some_and(|t| !t.trim().is_empty()),
                "{name}: {hit:?} has no tip"
            );
        }
    }
    // And a sweep over the whole window.
    let mut hits = 0;
    let mut y = l.body.y;
    while y < l.body.bottom() {
        let mut x = l.body.x;
        while x < l.body.right() {
            if let Some(hit) = analyze_hit(&l, &view, &s, x, y) {
                hits += 1;
                assert!(
                    analyze_tip(&hit, &view, &s).is_some_and(|t| !t.is_empty()),
                    "{hit:?} at ({x}, {y}) has no tip"
                );
            }
            x += 7.0;
        }
        y += 7.0;
    }
    assert!(hits > 500, "{hits}");
}

#[test]
fn a_chord_note_says_it_cannot_be_moved_yet() {
    let view = a_view();
    let (w, h) = analyze_window_size(1.0);
    let mut s = state(1.0);
    s.mode = Some(AnalyzeMode::Chords);
    let l = laid_out(w as f32, h as f32, &view, &mut s);
    let blob = l
        .blob(&view, &s, 1)
        .expect("the second chord note is on screen");
    let (x, y) = centre(blob);
    let hit = analyze_hit(&l, &view, &s, x, y).unwrap();
    assert_eq!(hit, AnalyzeHit::Note(1));
    let tip = analyze_tip(&hit, &view, &s).unwrap();
    assert!(tip.contains("can't be moved yet"), "{tip}");
}

// ------------------------------------------------------- the scale ---

#[test]
fn a_click_on_the_scale_copies_it() {
    let view = a_view();
    let (w, h) = analyze_window_size(1.0);
    let mut s = state(1.0);
    let l = laid_out(w as f32, h as f32, &view, &mut s);
    let (x, y) = centre(l.scale_name);
    assert_eq!(
        analyze_hit(&l, &view, &s, x, y),
        Some(AnalyzeHit::ScaleName)
    );
    assert_eq!(
        analyze_press(&l, &view, &mut s, x, y, Default::default()),
        Some(AnalyzeAction::CopyScale)
    );
    let (x, y) = centre(l.scale_copy);
    assert_eq!(
        analyze_press(&l, &view, &mut s, x, y, Default::default()),
        Some(AnalyzeAction::CopyScale)
    );
    let (x, y) = centre(l.scale_menu);
    assert_eq!(
        analyze_press(&l, &view, &mut s, x, y, Default::default()),
        Some(AnalyzeAction::ScaleMenu)
    );
    let strings = analyze_strings(&view, &s, &l);
    assert!(
        strings
            .iter()
            .any(|(t, _)| t.contains("A minor") && t.contains("82 %")),
        "{strings:?}"
    );
    assert!(
        strings.iter().any(|(t, _)| t == "Clear 91 %"),
        "{strings:?}"
    );
    assert!(
        strings.iter().any(|(t, _)| t == "Tuning +6 ct"),
        "{strings:?}"
    );
}

#[test]
fn the_scale_reads_as_names_and_degrees() {
    let a_minor = KeyScale::new(9, "natural-minor");
    let (names, degrees) = scale_degrees(&a_minor);
    assert_eq!(names, vec!["A", "B", "C", "D", "E", "F", "G"]);
    assert_eq!(
        degrees,
        vec!["1", "2", "\u{266d}3", "4", "5", "\u{266d}6", "\u{266d}7"]
    );
    let (_, major) = scale_degrees(&KeyScale::new(0, "major"));
    assert_eq!(major, vec!["1", "2", "3", "4", "5", "6", "7"]);
}

#[test]
fn rows_outside_the_scale_are_dimmed_and_the_root_marked() {
    let view = a_view();
    let s = state(1.0);
    assert_eq!(analyze_row_shade(&view, &s, 69), RowShade::Root);
    assert_eq!(analyze_row_shade(&view, &s, 68), RowShade::OutOfScale);
    assert_eq!(analyze_row_shade(&view, &s, 72), RowShade::Plain);
    let mut hidden = state(1.0);
    hidden.show_scale = false;
    assert_ne!(analyze_row_shade(&view, &hidden, 68), RowShade::OutOfScale);
}

// --------------------------------------------------------- the lane ---

#[test]
fn a_note_is_drawn_at_its_time_and_its_pitch() {
    let view = a_view();
    let (w, h) = analyze_window_size(1.0);
    let mut s = state(1.0);
    let l = laid_out(w as f32, h as f32, &view, &mut s);
    let g = &l.lane;
    let note = &view.melody[2];
    let blob = l.blob(&view, &s, 2).unwrap();
    assert!((blob.x - g.x_of(&s, note.start)).abs() < 0.5);
    assert!((blob.right() - g.x_of(&s, note.end)).abs() < 0.5);
    // Centred on its true pitch: 23 cents above B4's row.
    let middle = blob.y + blob.height / 2.0;
    let row = g.y_of(&s, 71.0);
    let true_pitch = g.y_of(&s, 71.23);
    assert!(
        (middle - true_pitch).abs() < 0.5,
        "{middle} vs {true_pitch}"
    );
    assert!(middle < row, "sharp is drawn above the row");
    // And taller when louder, never taller than a row.
    assert!(blob.height <= g.row_height(&s) + 0.01);
    // Time and pitch both come back from a point.
    let t = g.t_of(&s, g.x_of(&s, 4.25));
    assert!((t - 4.25).abs() < 1e-3);
    assert!((g.midi_of(&s, g.y_of(&s, 64.0)) - 64.0).abs() < 1e-3);
}

#[test]
fn only_notes_off_by_more_than_15_cents_carry_a_tag() {
    let view = a_view();
    assert_eq!(analyze_cents_tag(&view.melody[0]), None);
    assert_eq!(
        analyze_cents_tag(&view.melody[2]).as_deref(),
        Some("\u{25b2} 23ct")
    );
    assert_eq!(
        analyze_cents_tag(&view.melody[3]).as_deref(),
        Some("\u{25bc} 18ct")
    );
}

#[test]
fn melody_or_chords_chooses_which_notes_are_drawn() {
    let view = a_view();
    let mut s = state(1.0);
    assert_eq!(s.notes(&view).len(), view.melody.len(), "detected: melody");
    assert_eq!(s.effective_mode(&view), AnalyzeMode::Melody);
    let (w, h) = analyze_window_size(1.0);
    let l = laid_out(w as f32, h as f32, &view, &mut s);
    let (x, y) = centre(l.mode_chords);
    assert_eq!(
        analyze_press(&l, &view, &mut s, x, y, Default::default()),
        Some(AnalyzeAction::Mode(AnalyzeMode::Chords))
    );
    s.mode = Some(AnalyzeMode::Chords);
    assert_eq!(s.notes(&view).len(), view.notes.len());
}

// ---------------------------------------------------- selection ---

#[test]
fn click_shift_click_marquee_and_select_all() {
    let view = a_view();
    let (w, h) = analyze_window_size(1.0);
    let mut s = state(1.0);
    let l = laid_out(w as f32, h as f32, &view, &mut s);
    let (x, y) = centre(l.blob(&view, &s, 1).unwrap());
    analyze_press(&l, &view, &mut s, x, y, Default::default());
    assert_eq!(s.selected(), vec![1]);
    let (x, y) = centre(l.blob(&view, &s, 3).unwrap());
    let shift = fontelle_ui::canvas::Modifiers {
        shift: true,
        ..Default::default()
    };
    analyze_press(&l, &view, &mut s, x, y, shift);
    assert_eq!(s.selected(), vec![1, 3]);
    // Shift on a selected note takes it out.
    analyze_press(&l, &view, &mut s, x, y, shift);
    assert_eq!(s.selected(), vec![1]);

    // A press on empty lane starts a marquee; dragging it over the first two
    // notes selects exactly those.
    let a = l.blob(&view, &s, 0).unwrap();
    let b = l.blob(&view, &s, 1).unwrap();
    let from = (a.x - 4.0, l.lane.grid.y + 2.0);
    assert_eq!(
        analyze_press(&l, &view, &mut s, from.0, from.1, Default::default()),
        Some(AnalyzeAction::Marquee)
    );
    assert!(s.selected().is_empty(), "a press on nothing clears");
    s.drag_marquee(&l, &view, (b.x + 2.0, l.lane.grid.bottom() - 2.0));
    assert_eq!(s.selected(), vec![0, 1]);
    s.end_marquee();

    s.select_all(&view);
    assert_eq!(s.selected(), (0..view.melody.len()).collect::<Vec<_>>());
}

// ------------------------------------------------- zoom and scroll ---

#[test]
fn the_wheel_scrolls_pitch_and_ctrl_zooms_time_about_the_pointer() {
    let view = a_view();
    let (w, h) = analyze_window_size(1.0);
    let mut s = state(1.0);
    let l = laid_out(w as f32, h as f32, &view, &mut s);
    let g = l.lane.clone();
    let x = g.grid.x + g.grid.width * 0.6;
    let y = g.grid.y + g.grid.height * 0.5;
    let at = g.t_of(&s, x);
    let top = s.top;
    s.wheel(&g, &view, x, y, 0.0, 1.0, Default::default());
    assert!(s.top > top, "up the wheel is up the keyboard");
    let ctrl = fontelle_ui::canvas::Modifiers {
        ctrl: true,
        ..Default::default()
    };
    let before = s.pixels_per_second;
    s.wheel(&g, &view, x, y, 0.0, 2.0, ctrl);
    assert!(s.pixels_per_second > before, "Ctrl and up zooms in");
    assert!((g.t_of(&s, x) - at).abs() < 1e-3, "about the pointer");
    let shift = fontelle_ui::canvas::Modifiers {
        shift: true,
        ..Default::default()
    };
    let start = s.start;
    s.wheel(&g, &view, x, y, 0.0, -1.0, shift);
    assert!(s.start > start, "Shift scrolls time");
}

#[test]
fn fitting_puts_every_note_on_screen() {
    let view = a_view();
    let (w, h) = analyze_window_size(1.0);
    let mut s = state(1.0);
    let l = laid_out(w as f32, h as f32, &view, &mut s);
    for index in 0..view.melody.len() {
        let blob = l.blob(&view, &s, index).unwrap();
        assert!(inside(l.lane.grid, blob), "note {index} at {blob:?}");
    }
    let _ = Arc::new(());
}
