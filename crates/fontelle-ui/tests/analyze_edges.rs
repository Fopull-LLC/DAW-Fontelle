//! Analyze Musically's edges, as a demanding musician finds them: a take an
//! hour long, the zoom at either end, a note pushed far past where a voice
//! goes, the ruler clicked outside the loop while it plays, a take with no
//! notes in it, a study with nothing recorded yet.

use fontelle_types::KeyScale;
use fontelle_ui::canvas::{
    AnalyzeClarity, AnalyzeEditOp, AnalyzeKey, AnalyzeLayout, AnalyzeMode, AnalyzeState,
    AnalyzeView, AnalyzedChord, AnalyzedNote, analyze_lane_message, analyze_layout,
    estimated_width, ruler_labels, ruler_step, zoom_to,
};
use fontelle_ui::layout::editor_window_layout;
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
        edit: None,
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
        rendered: false,
        preview_pending: false,
        ..AnalyzeView::default()
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

/// An hour-long take fits the lane when it opens, and Shift+Z shows it
/// all; the ruler counts it in minutes, not a label every pixel.
#[test]
fn an_hour_long_take_fits_and_its_ruler_reads() {
    let mut view = a_view();
    view.duration = 3600.0;
    let mut state = state(1.0);
    let l = laid_out(1180.0, 740.0, &view, &mut state);
    let grid = l.lane.grid;
    let shown = f64::from(grid.width / state.pixels_per_second);
    assert!(shown >= 3600.0 - 1.0, "fitted, {shown} s of 3600 show");
    state.pixels_per_second = 80.0;
    assert!(zoom_to(&mut state, &view, &l, true));
    assert!(f64::from(grid.width / state.pixels_per_second) >= 3600.0 - 1.0);
    let labels = ruler_labels(&l, &view, &state);
    assert!(labels.len() >= 2 && labels.len() < 30, "{labels:?}");
    for pair in labels.windows(2) {
        assert!(pair[1].0 - pair[0].0 >= 60.0, "{labels:?}");
    }
}

/// Zoomed all the way in, the ruler's labels still say where they are
/// (hundredths, not the same tenth twice).
#[test]
fn the_ruler_reads_at_the_closest_zoom() {
    assert!(ruler_step(2000.0) < 0.1);
    let view = a_view();
    let mut state = state(1.0);
    let l = laid_out(1180.0, 740.0, &view, &mut state);
    state.pixels_per_second = 2000.0;
    state.start = 3.0;
    let labels = ruler_labels(&l, &view, &state);
    assert!(labels.len() >= 3);
    for pair in labels.windows(2) {
        assert_ne!(pair[0].1, pair[1].1, "{labels:?}");
    }
}

/// A note can go two octaves either way from where it was sung, and no
/// further, by the keys or by a drag: past that a voice is not a voice and
/// the resynthesis has nothing to make it from.
#[test]
fn a_note_goes_no_further_than_two_octaves() {
    let view = a_view();
    let mut state = state(1.0);
    let l = laid_out(1180.0, 740.0, &view, &mut state);
    state.click_note(3, false);
    let mut moved = view.clone();
    for _ in 0..5 {
        let change = state.edit(&moved, AnalyzeEditOp::Nudge(1200.0)).unwrap();
        moved.melody[3].edit = change[0].edit;
    }
    let shift = moved.melody[3].edit.unwrap().shift_cents;
    assert!(shift <= 2400.0 + 0.01, "{shift}");
    for _ in 0..10 {
        let change = state.edit(&moved, AnalyzeEditOp::Nudge(-1200.0)).unwrap();
        moved.melody[3].edit = change[0].edit;
    }
    assert!(moved.melody[3].edit.unwrap().shift_cents >= -2400.0 - 0.01);
    // A drag far off the top of the lane.
    let blob = l.blob(&view, &state, 3).unwrap();
    let (x, y) = (blob.x + blob.width / 2.0, blob.y + blob.height / 2.0);
    fontelle_ui::canvas::analyze_press(&l, &view, &mut state, x, y, Default::default());
    let changes = state
        .drag_note(&l.lane, &view, (x, y - 5000.0), false)
        .unwrap()
        .unwrap();
    let shift = changes[0].edit.unwrap().shift_cents;
    assert!(shift <= 2400.0 + 0.01 && shift > 2000.0, "{shift}");
    let changes = state
        .drag_note(&l.lane, &view, (x, y + 5000.0), false)
        .unwrap()
        .unwrap();
    assert!(changes[0].edit.unwrap().shift_cents >= -2400.0 - 0.01);
}

/// A click on the ruler while a loop plays: inside the loop, it carries on
/// round from there; outside it, it plays on from there to the end rather
/// than stopping dead or jumping back.
#[test]
fn the_ruler_clicked_while_a_loop_plays() {
    let view = a_view();
    let mut state = state(1.0);
    state.region = Some((2.0, 4.0));
    assert_eq!(state.seek_range(&view, 3.0), (3.0, Some(4.0), true));
    assert_eq!(state.seek_range(&view, 6.0), (6.0, None, false));
    assert_eq!(state.seek_range(&view, 1.0), (1.0, None, false));
    state.region = None;
    assert_eq!(state.seek_range(&view, 5.0), (5.0, None, false));
    // At the very end, from the start.
    assert_eq!(state.seek_range(&view, 12.0), (0.0, None, false));
}

/// The lane says why it is empty: still listening, nothing recorded yet,
/// or nothing with a pitch heard in it — never a blank screen.
#[test]
fn an_empty_lane_says_why() {
    let mut view = a_view();
    assert_eq!(analyze_lane_message(&view), None, "notes: nothing to say");
    view.melody.clear();
    view.notes.clear();
    view.has_audio = true;
    view.analysing = Some(0.3);
    assert!(
        analyze_lane_message(&view)
            .unwrap()
            .starts_with("Listening")
    );
    view.analysing = None;
    let said = analyze_lane_message(&view).unwrap();
    assert!(said.contains("No notes"), "{said}");
    view.has_audio = false;
    let said = analyze_lane_message(&view).unwrap();
    assert!(said.contains("Record"), "{said}");
    view.error = Some("the clip has no audio".to_string());
    assert_eq!(
        analyze_lane_message(&view).as_deref(),
        Some("the clip has no audio")
    );
    // A take under a tenth of a second: fitted, and nothing divides by zero.
    let mut short = a_view();
    short.duration = 0.05;
    short.melody.clear();
    short.notes.clear();
    let mut state = state(1.0);
    let l = laid_out(1180.0, 740.0, &short, &mut state);
    assert!(state.pixels_per_second.is_finite() && state.pixels_per_second > 0.0);
    assert!(!ruler_labels(&l, &short, &state).is_empty());
    assert_eq!(state.space_range(&short), (0.0, None, false));
}

/// A note dragged past the lane's top or bottom edge stays in sight: the
/// lane scrolls with it rather than letting it go off the screen.
#[test]
fn a_note_dragged_off_the_lane_stays_in_sight() {
    let view = a_view();
    for up in [true, false] {
        let mut state = state(1.0);
        let l = laid_out(1180.0, 740.0, &view, &mut state);
        state.row_height = 20.0;
        let rows = l.lane.grid.height / state.row_height;
        state.top = 69.0 + rows / 2.0;
        let blob = l.blob(&view, &state, 3).unwrap();
        let (x, y) = (blob.x + blob.width / 2.0, blob.y + blob.height / 2.0);
        fontelle_ui::canvas::analyze_press(&l, &view, &mut state, x, y, Default::default());
        let to = if up {
            l.lane.grid.y - 400.0
        } else {
            l.lane.grid.bottom() + 400.0
        };
        let changes = state
            .drag_note(&l.lane, &view, (x, to), false)
            .unwrap()
            .unwrap();
        let mut moved = view.clone();
        moved.melody[3].edit = changes[0].edit;
        let now = fontelle_ui::canvas::analyze_pitch(&moved.melody[3]);
        let at = l.lane.y_of(&state, now);
        assert!(
            at > l.lane.grid.y && at < l.lane.grid.bottom(),
            "{} at {at}, the lane {:?}",
            if up { "up" } else { "down" },
            l.lane.grid
        );
    }
}
