//! The flatten handle (Ty, trying the preview build):
//!
//! > *"it would be nice if there was a handle i could drag from the top of a
//! > note to flatten it from there instead of having to go back and forth
//! > between the notes and the flatten knob this would make it easy since
//! > its the most common thing to do when pitching things to sound better."*
//!
//! A small handle on top of a note — shown on the hovered and selected
//! notes — dragged up for more flatten and down for less, 0..100 %, the
//! selection together and relative, one undo a drag (the host merges what
//! a drag sends until the gesture ends, as a note's move does), its hit a
//! size on screen that scales with the window, double-click back to none;
//! the curve drawn as the render will make it.

use fontelle_types::KeyScale;
use fontelle_ui::canvas::{
    AnalyzeAction, AnalyzeClarity, AnalyzeEdit, AnalyzeHit, AnalyzeKey, AnalyzeLayout, AnalyzeMode,
    AnalyzeNotePart, AnalyzePage, AnalyzeState, AnalyzeTool, AnalyzeView, AnalyzedChord,
    AnalyzedNote, SCALES, analyze_edited_curve, analyze_hit, analyze_layout, analyze_press,
    analyze_tip, estimated_width,
};
use fontelle_ui::layout::{Rect, editor_window_layout};
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

fn centre(r: Rect) -> (f32, f32) {
    (r.x + r.width / 2.0, r.y + r.height / 2.0)
}

/// The handle's hit, at the top of note `index`, once it is shown.
fn handle(l: &AnalyzeLayout, view: &AnalyzeView, state: &AnalyzeState, index: usize) -> Rect {
    l.flatten_handle(view, state, index)
        .expect("a shown note has a handle")
}

fn flatten_of(changes: &[fontelle_ui::canvas::AnalyzeEditChange], start: f64) -> f32 {
    changes
        .iter()
        .find(|c| c.start == start)
        .unwrap()
        .edit
        .map_or(0.0, |e| e.flatten)
}

// -------------------------------------------------------------- shown ---

/// Only a hovered or selected note shows its handle, a chord note never
/// does, and only on the Notes page. Over it the pointer reads the handle,
/// not the note's body or its ends.
#[test]
fn the_handle_shows_on_hovered_and_selected_notes() {
    let view = a_view();
    let mut state = state(1.0);
    let l = laid_out(1180.0, 740.0, &view, &mut state);
    assert!(
        l.flatten_handle(&view, &state, 2).is_none(),
        "not until shown"
    );
    state.hover = Some(AnalyzeHit::Note(2));
    let h = handle(&l, &view, &state, 2);
    let blob = l.blob(&view, &state, 2).unwrap();
    assert!(
        h.y < blob.y + 3.0,
        "on top of the note: {h:?} over {blob:?}"
    );
    assert!((centre(h).0 - centre(blob).0).abs() < 1.0, "centred");
    assert_eq!(
        analyze_hit(&l, &view, &state, centre(h).0, centre(h).1),
        Some(AnalyzeHit::FlattenHandle(2))
    );
    // The body below it is still the body.
    assert_eq!(
        analyze_hit(&l, &view, &state, centre(blob).0, blob.bottom() - 1.0),
        Some(AnalyzeHit::Note(2))
    );
    // Selected, in either tool.
    for tool in [AnalyzeTool::Move, AnalyzeTool::Select] {
        let mut state = state.clone();
        state.hover = None;
        state.tool = tool;
        state.click_note(3, false);
        assert!(l.flatten_handle(&view, &state, 3).is_some(), "{tool:?}");
        assert!(l.flatten_handle(&view, &state, 2).is_none());
    }
    // A chord note has none.
    let mut chords = state.clone();
    chords.mode = Some(AnalyzeMode::Chords);
    let l = laid_out(1180.0, 740.0, &view, &mut chords);
    chords.click_note(0, false);
    chords.hover = Some(AnalyzeHit::Note(0));
    assert!(l.flatten_handle(&view, &chords, 0).is_none());
    // Nor on another page.
    let mut clean = state.clone();
    clean.page = AnalyzePage::Clean;
    clean.hover = Some(AnalyzeHit::Note(2));
    let l = laid_out(1180.0, 740.0, &view, &mut clean);
    assert!(l.flatten_handle(&view, &clean, 2).is_none());
}

/// The hit is a size on screen, whatever the zoom: at least 10 px tall
/// and 12 px wide at 100 %, growing with the window's scale; a note's
/// glide ends keep their zones beside it.
#[test]
fn the_handle_is_a_size_on_screen_at_any_zoom_and_scale() {
    let mut view = a_view();
    view.melody[2].edit = Some(AnalyzeEdit {
        shift_cents: 100.0,
        ..AnalyzeEdit::default()
    });
    for scale in SCALES {
        for (pps, row) in [(2.0f32, 5.0f32), (80.0, 14.0), (2000.0, 28.0)] {
            let mut state = state(scale);
            let l = laid_out(1180.0 * scale, 740.0 * scale, &view, &mut state);
            state.pixels_per_second = pps;
            state.row_height = row;
            state.start = (2.4 + 1.2 / 2.0) - f64::from(l.lane.grid.width / pps) / 2.0;
            state.start = state.start.max(0.0);
            state.top = 72.0 + l.lane.grid.height / row / 2.0;
            state.click_note(2, false);
            let Some(h) = l.flatten_handle(&view, &state, 2) else {
                panic!("scale {scale} pps {pps}: no handle");
            };
            assert!(
                h.height >= 10.0 * scale - 0.01 && h.width >= 12.0 * scale - 0.01,
                "scale {scale} pps {pps}: {h:?}"
            );
            let (x, y) = centre(h);
            assert!(l.lane.grid.contains(x, y), "in sight: {h:?}");
            assert_eq!(
                analyze_hit(&l, &view, &state, x, y),
                Some(AnalyzeHit::FlattenHandle(2)),
                "scale {scale} pps {pps}"
            );
            // On a note long enough to have room, the ends are still ends.
            let blob = l.blob(&view, &state, 2).unwrap();
            let grid = l.lane.grid;
            if blob.width > 60.0 * scale && blob.x >= grid.x && blob.right() <= grid.right() {
                let mid = blob.y + blob.height / 2.0;
                assert_eq!(
                    analyze_hit(&l, &view, &state, blob.x + 1.0, mid),
                    Some(AnalyzeHit::NoteEnd(2, AnalyzeNotePart::Start)),
                    "scale {scale} pps {pps}"
                );
                assert_eq!(
                    analyze_hit(&l, &view, &state, blob.right() - 1.0, mid),
                    Some(AnalyzeHit::NoteEnd(2, AnalyzeNotePart::End)),
                    "scale {scale} pps {pps}"
                );
            }
        }
    }
}

// ------------------------------------------------------------- dragged ---

/// Up is more flatten, down is less, 0..100 %; the note is selected by the
/// press, and the drag is its own (not a move of the pitch).
#[test]
fn dragging_the_handle_up_flattens_and_down_puts_it_back() {
    let view = a_view();
    let mut state = state(1.0);
    let l = laid_out(1180.0, 740.0, &view, &mut state);
    state.hover = Some(AnalyzeHit::Note(3));
    let (x, y) = centre(handle(&l, &view, &state, 3));
    let action = analyze_press(&l, &view, &mut state, x, y, Default::default());
    assert_eq!(
        action,
        Some(AnalyzeAction::Selected),
        "a press on it plays nothing"
    );
    assert!(state.is_selected(3));
    assert!(state.dragging_flatten());
    // A wobble of the hand is not a drag.
    assert!(
        state
            .drag_note(&l.lane, &view, (x, y - 1.0), false)
            .is_none()
    );
    let up = state
        .drag_note(&l.lane, &view, (x, y - 60.0), false)
        .unwrap()
        .unwrap();
    assert_eq!(up.len(), 1);
    let edit = up[0].edit.unwrap();
    assert!(edit.flatten > 0.3 && edit.flatten < 0.8, "{edit:?}");
    assert_eq!(edit.shift_cents, 0.0, "the pitch is not moved");
    assert_eq!(state.flatten_readout(), Some((3, edit.flatten)));
    // All the way up stops at 100 %, all the way down at none (which is no
    // edit at all on a note with nothing else done to it).
    let top = state
        .drag_note(&l.lane, &view, (x, y - 2000.0), false)
        .unwrap()
        .unwrap();
    assert_eq!(top[0].edit.unwrap().flatten, 1.0);
    let bottom = state
        .drag_note(&l.lane, &view, (x, y + 2000.0), false)
        .unwrap()
        .unwrap();
    assert_eq!(bottom[0].edit, None);
    // The fine drag (Shift) moves a tenth as far.
    let coarse = state
        .drag_note(&l.lane, &view, (x, y - 20.0), false)
        .unwrap()
        .unwrap()[0]
        .edit
        .unwrap()
        .flatten;
    let fine = state
        .drag_note(&l.lane, &view, (x, y - 20.0), true)
        .unwrap()
        .unwrap()[0]
        .edit
        .unwrap()
        .flatten;
    assert!((fine * 10.0 - coarse).abs() < 0.011, "{fine} {coarse}");
    assert!(state.end_note_drag(), "it moved: one undo");
    assert!(!state.dragging_flatten());
    assert_eq!(state.flatten_readout(), None);
}

/// Part of a selection, the handle flattens every selected note by as
/// much, each from where it was (relative), each held to 0..100 %; the
/// other edits each note has are kept.
#[test]
fn the_handle_carries_the_selection_relatively() {
    let mut view = a_view();
    view.melody[0].edit = Some(AnalyzeEdit {
        flatten: 0.9,
        shift_cents: 50.0,
        ..AnalyzeEdit::default()
    });
    let mut state = state(1.0);
    let l = laid_out(1180.0, 740.0, &view, &mut state);
    state.click_note(0, false);
    state.click_note(3, true);
    let (x, y) = centre(handle(&l, &view, &state, 3));
    analyze_press(&l, &view, &mut state, x, y, Default::default());
    assert!(
        state.is_selected(0) && state.is_selected(3),
        "the selection is kept"
    );
    let changes = state
        .drag_note(&l.lane, &view, (x, y - 30.0), false)
        .unwrap()
        .unwrap();
    assert_eq!(changes.len(), 2);
    let held = flatten_of(&changes, 3.8);
    assert!(held > 0.1, "{held}");
    assert_eq!(
        flatten_of(&changes, 0.5),
        1.0,
        "0.9 + as much, held at 100 %"
    );
    let first = changes
        .iter()
        .find(|c| c.start == 0.5)
        .unwrap()
        .edit
        .unwrap();
    assert_eq!(first.shift_cents, 50.0, "its move is kept");
    // A note outside the selection takes the selection with it no more.
    state.end_note_drag();
    state.click_note(1, false);
    state.hover = Some(AnalyzeHit::Note(4));
    let (x, y) = centre(handle(&l, &view, &state, 4));
    analyze_press(&l, &view, &mut state, x, y, Default::default());
    let changes = state
        .drag_note(&l.lane, &view, (x, y - 30.0), false)
        .unwrap()
        .unwrap();
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].start, 5.2);
    assert!(state.is_selected(4) && !state.is_selected(1));
}

/// Double-click: none again, on the note (and the selection it is in).
#[test]
fn reset_takes_the_flatten_off() {
    let mut view = a_view();
    view.melody[3].edit = Some(AnalyzeEdit {
        flatten: 0.7,
        ..AnalyzeEdit::default()
    });
    view.melody[2].edit = Some(AnalyzeEdit {
        flatten: 0.4,
        shift_cents: 100.0,
        ..AnalyzeEdit::default()
    });
    let mut state = state(1.0);
    state.click_note(2, false);
    state.click_note(3, true);
    let changes = state.reset_flatten(&view, 3);
    assert_eq!(changes.len(), 2);
    assert_eq!(changes.iter().find(|c| c.start == 3.8).unwrap().edit, None);
    let kept = changes
        .iter()
        .find(|c| c.start == 2.4)
        .unwrap()
        .edit
        .unwrap();
    assert_eq!((kept.flatten, kept.shift_cents), (0.0, 100.0));
    // Off the selection: that note alone.
    state.click_note(0, false);
    assert_eq!(state.reset_flatten(&view, 3).len(), 1);
}

/// The handle says what it does and how much it is now, and wears the
/// sizing cursor's direction (the window maps the hit to its pointer).
#[test]
fn the_handle_says_what_it_does() {
    let mut view = a_view();
    view.melody[2].edit = Some(AnalyzeEdit {
        flatten: 0.64,
        ..AnalyzeEdit::default()
    });
    let state = state(1.0);
    let tip = analyze_tip(&AnalyzeHit::FlattenHandle(2), &view, &state).unwrap();
    assert!(tip.contains("64"), "{tip}");
    assert!(tip.to_lowercase().contains("flatten"), "{tip}");
    assert!(tip.contains("double-click"), "{tip}");
}

// --------------------------------------------------------------- drawn ---

/// The curve drawn through a note is the one the render makes: the slow
/// drift taken out by the flatten, the vibrato kept, the move eased in and
/// out over the glides — so the handle's drag is seen as it is heard.
#[test]
fn the_curve_is_drawn_as_the_render_will_make_it() {
    // A note drifting 40 cents up over a second, with a 6 Hz, 20-cent
    // vibrato on top.
    let start = 1.0;
    let curve: Vec<(f64, f32)> = (0..=200)
        .map(|i| {
            let t = start + f64::from(i) * 0.005;
            let drift = -20.0 + 40.0 * (t - start) as f32;
            let vib = 20.0 * (std::f32::consts::TAU * 6.0 * (t - start) as f32).sin();
            (t, drift + vib)
        })
        .collect();
    let mut note = AnalyzedNote {
        start,
        end: start + 1.0,
        midi: 69,
        cents: 0.0,
        amplitude: 0.7,
        confidence: 0.9,
        poly: false,
        curve: curve.clone(),
        edit: None,
    };
    assert_eq!(analyze_edited_curve(&note), curve, "unedited is as sung");
    note.edit = Some(AnalyzeEdit {
        flatten: 1.0,
        glide_in_ms: 0.0,
        glide_out_ms: 0.0,
        ..AnalyzeEdit::default()
    });
    let flat = analyze_edited_curve(&note);
    // Over the middle, the drift is gone (its first and last fifths are
    // where a lowpass's ends are least sure) and the vibrato is still there.
    let middle = &flat[40..160];
    let mean_first = middle[..30].iter().map(|p| p.1).sum::<f32>() / 30.0;
    let mean_last = middle[90..].iter().map(|p| p.1).sum::<f32>() / 30.0;
    assert!(
        (mean_last - mean_first).abs() < 8.0,
        "the drift is flattened: {mean_first} .. {mean_last}"
    );
    let swing = middle.iter().map(|p| p.1).fold(f32::MIN, f32::max)
        - middle.iter().map(|p| p.1).fold(f32::MAX, f32::min);
    assert!(swing > 30.0, "the vibrato is kept: {swing}");
    // Halfway is halfway.
    note.edit = Some(AnalyzeEdit {
        flatten: 0.5,
        glide_in_ms: 0.0,
        glide_out_ms: 0.0,
        ..AnalyzeEdit::default()
    });
    let half = analyze_edited_curve(&note);
    let rise = |c: &[(f64, f32)]| {
        c[130..160].iter().map(|p| p.1).sum::<f32>() / 30.0
            - c[40..70].iter().map(|p| p.1).sum::<f32>() / 30.0
    };
    let sung_rise = rise(&curve);
    assert!(
        (rise(&half) - sung_rise * 0.5).abs() < sung_rise * 0.2,
        "{} of {sung_rise}",
        rise(&half)
    );
    // A move is the move, eased over the glides.
    note.edit = Some(AnalyzeEdit {
        shift_cents: 100.0,
        glide_in_ms: 100.0,
        glide_out_ms: 100.0,
        ..AnalyzeEdit::default()
    });
    let moved = analyze_edited_curve(&note);
    assert_eq!(moved[0].1, curve[0].1, "it eases in from where it was sung");
    assert!((moved[100].1 - curve[100].1 - 100.0).abs() < 0.01);
}
