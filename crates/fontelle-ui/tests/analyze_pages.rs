//! Analyze Musically's Clean, Slice and Record pages and the Pitch card
//! (`docs/analyze-musically-plan.md` §2.7, §3.1, §3.5, §3.8, P3–P5): what a
//! press, a drag and a key do, as pure answers the window hands the host.

use fontelle_types::{StudyCompSpan, StudyMarker, StudyTake};
use fontelle_ui::canvas::{
    AnalyzeAction, AnalyzeControl, AnalyzeEdit, AnalyzeHit, AnalyzeKnob, AnalyzeKnobChange,
    AnalyzeLaneChange, AnalyzeLaneEnd, AnalyzeLayout, AnalyzePage, AnalyzeRecordView,
    AnalyzeSource, AnalyzeState, AnalyzeTool, AnalyzeView, AnalyzedNote, AutoSlice, Precision,
    analyze_cuts, analyze_hit, analyze_knob_change, analyze_knob_value, analyze_layout,
    analyze_press, comp_with, estimated_width, markers_from_cuts, with_marker_at, zoom_to,
};
use fontelle_ui::layout::{Rect, analyze_window_size, editor_window_layout};
use fontelle_ui::theme::Theme;

const RATE: u32 = 48_000;

fn a_note(start: f64, end: f64, midi: u8, cents: f32) -> AnalyzedNote {
    AnalyzedNote {
        start,
        end,
        midi,
        cents,
        amplitude: 0.7,
        confidence: 0.9,
        poly: false,
        curve: Vec::new(),
        edit: None,
    }
}

/// Twelve seconds of audio, five sung notes, transients at every half
/// second (the first strong), a beat of half a second.
fn a_view() -> AnalyzeView {
    AnalyzeView {
        name: "Vox".to_string(),
        duration: 12.0,
        detected: Some(fontelle_ui::canvas::AnalyzeMode::Melody),
        melody: vec![
            a_note(0.5, 1.4, 69, 4.0),
            a_note(1.5, 2.3, 72, -6.0),
            a_note(2.4, 3.6, 71, 23.0),
            a_note(3.8, 5.0, 69, -18.0),
            a_note(5.2, 7.0, 64, 2.0),
        ],
        peaks: (0..1200)
            .map(|i| {
                let v = ((i as f32) * 0.05).sin().abs() * 0.6;
                (-v, v)
            })
            .collect::<Vec<_>>()
            .into(),
        peaks_per_second: 100.0,
        rate: RATE,
        offset: 0,
        onsets: (1..24)
            .map(|k| (k as f64 * 0.5, if k % 4 == 0 { 0.9 } else { 0.3 }))
            .collect(),
        beat_seconds: Some(0.5),
        has_audio: true,
        ..AnalyzeView::default()
    }
}

fn laid_out(view: &AnalyzeView, state: &mut AnalyzeState) -> AnalyzeLayout {
    let (w, h) = analyze_window_size(state.scale);
    let panel = editor_window_layout(w as f32, h as f32, &Theme::dark_default().metrics);
    let measure = |s: &str| estimated_width(s);
    let layout = analyze_layout(panel.body, view, state, &measure);
    state.fit(view, &layout);
    analyze_layout(panel.body, view, state, &measure)
}

fn on(page: AnalyzePage, tool: AnalyzeTool) -> AnalyzeState {
    let mut state = AnalyzeState::default();
    state.page = page;
    state.tool = tool;
    state
}

fn centre(r: Rect) -> (f32, f32) {
    (r.x + r.width / 2.0, r.y + r.height / 2.0)
}

// ------------------------------------------------------------ Clean ---

/// The Noise tool: a drag over a quiet stretch is a span, and letting go
/// asks for it to be captured.
#[test]
fn the_noise_tool_drags_a_span_that_is_captured_on_release() {
    let view = a_view();
    let mut s = on(AnalyzePage::Clean, AnalyzeTool::Noise);
    let l = laid_out(&view, &mut s);
    let y = l.lane.grid.y + l.lane.grid.height * 0.6;
    let (x0, x1) = (l.lane.x_of(&s, 2.0), l.lane.x_of(&s, 3.0));
    assert_eq!(
        analyze_press(&l, &view, &mut s, x0, y, Default::default()),
        Some(AnalyzeAction::LaneGrab)
    );
    assert_eq!(s.drag_lane(&l, &view, x1), Some(AnalyzeLaneChange::Span));
    let (a, b) = s.span.expect("a span");
    assert!((a - 2.0).abs() < 0.05 && (b - 3.0).abs() < 0.05, "{a} {b}");
    match s.end_lane_drag() {
        AnalyzeLaneEnd::Span(a2, b2) => assert_eq!((a2, b2), (a, b)),
        other => panic!("{other:?}"),
    }
    // Capture is offered for a selection, and greyed without one.
    assert!(fontelle_ui::canvas::control_enabled(
        AnalyzeControl::CaptureNoise,
        &view,
        &s
    ));
    s.span = None;
    assert!(!fontelle_ui::canvas::control_enabled(
        AnalyzeControl::CaptureNoise,
        &view,
        &s
    ));
}

/// The lane's ends are the trim's handles, and the fades' handles sit along
/// the top: dragging them changes the study's clean, never the file.
#[test]
fn trim_ends_and_fades_drag_on_the_clean_page() {
    let mut view = a_view();
    let mut s = on(AnalyzePage::Clean, AnalyzeTool::Select);
    let l = laid_out(&view, &mut s);
    let mid = l.lane.grid.y + l.lane.grid.height / 2.0;
    let start = l.lane.x_of(&s, 0.0);
    assert_eq!(
        analyze_hit(&l, &view, &s, start + 1.0, mid),
        Some(AnalyzeHit::TrimEnd(true))
    );
    analyze_press(&l, &view, &mut s, start + 1.0, mid, Default::default());
    let Some(AnalyzeLaneChange::Clean(clean)) = s.drag_lane(&l, &view, l.lane.x_of(&s, 1.5)) else {
        panic!("a trim");
    };
    let (a, b) = clean.trim.expect("trimmed");
    assert!((a as f64 / f64::from(RATE) - 1.5).abs() < 0.05, "{a}");
    assert_eq!(b, view.frame_of(12.0), "the end stays");
    assert_eq!(s.end_lane_drag(), AnalyzeLaneEnd::Changed);
    view.clean = clean;
    // The fade in's handle, at the trim's start along the top.
    let l = laid_out(&view, &mut s);
    let x = l.lane.x_of(&s, 1.5);
    let top = l.lane.grid.y + 9.0;
    assert_eq!(
        analyze_hit(&l, &view, &s, x, top),
        Some(AnalyzeHit::FadeEnd(true))
    );
    analyze_press(&l, &view, &mut s, x, top, Default::default());
    let Some(AnalyzeLaneChange::Clean(faded)) = s.drag_lane(&l, &view, l.lane.x_of(&s, 1.75))
    else {
        panic!("a fade");
    };
    let ms = faded.fade_in as f64 * 1000.0 / f64::from(RATE);
    assert!((ms - 250.0).abs() < 20.0, "{ms}");
    assert_eq!(faded.trim, view.clean.trim, "the trim kept");
}

/// The Clean page's knobs are the study's clean: Reduce, Amount and
/// Sensitivity the denoiser (on, once there is noise to take out), the
/// fades in milliseconds, the gain in dB.
#[test]
fn the_clean_knobs_set_the_studys_clean() {
    let mut view = a_view();
    let mut s = on(AnalyzePage::Clean, AnalyzeTool::Select);
    let AnalyzeKnobChange::Clean(clean) =
        analyze_knob_change(AnalyzeKnob::FadeOut, 120.0, &view, &mut s)
    else {
        panic!("clean");
    };
    assert_eq!(clean.fade_out, 5_760);
    let AnalyzeKnobChange::Clean(clean) =
        analyze_knob_change(AnalyzeKnob::Gain, -3.0, &view, &mut s)
    else {
        panic!("clean");
    };
    assert_eq!(clean.gain_db, -3.0);
    // No noise captured: the knob sets the value and leaves it off.
    let AnalyzeKnobChange::Clean(clean) =
        analyze_knob_change(AnalyzeKnob::Reduce, 20.0, &view, &mut s)
    else {
        panic!("clean");
    };
    assert!(!clean.denoise.on);
    view.clean.denoise.noise = Some(fontelle_types::StudyNoise {
        magnitudes: vec![0.0; 4],
        sample_rate: RATE,
        level_db: -50.0,
    });
    let AnalyzeKnobChange::Clean(clean) =
        analyze_knob_change(AnalyzeKnob::Amount, 80.0, &view, &mut s)
    else {
        panic!("clean");
    };
    assert!(clean.denoise.on, "turning it means wanting it");
    assert!((clean.denoise.amount - 0.8).abs() < 1e-6);
    view.clean = clean;
    assert_eq!(
        analyze_knob_value(AnalyzeKnob::Amount, &view, &s),
        Some(80.0)
    );
}

// ------------------------------------------------------------ Slice ---

/// The Marker tool: a click adds a marker where it was, a drag moves one,
/// Del removes the one in hand.
#[test]
fn the_marker_tool_adds_moves_and_removes() {
    let mut view = a_view();
    let mut s = on(AnalyzePage::Slice, AnalyzeTool::Marker);
    let l = laid_out(&view, &mut s);
    let y = l.lane.grid.y + l.lane.grid.height * 0.7;
    let x = l.lane.x_of(&s, 4.0);
    let Some(AnalyzeAction::AddMarker(t)) =
        analyze_press(&l, &view, &mut s, x, y, Default::default())
    else {
        panic!("a marker");
    };
    assert!((t - 4.0).abs() < 0.05);
    let (markers, id) = with_marker_at(&view, t);
    assert_eq!(markers.len(), 1);
    view.markers = markers;
    s.grab_marker(id);
    // The same press drags it.
    let Some(AnalyzeLaneChange::Markers(moved)) = s.drag_lane(&l, &view, l.lane.x_of(&s, 5.0))
    else {
        panic!("moved");
    };
    assert!((view.seconds_of(moved[0].at) - 5.0).abs() < 0.05);
    assert_eq!(s.end_lane_drag(), AnalyzeLaneEnd::Changed);
    view.markers = moved;
    // A second marker; then pressing the first selects it, and Del takes
    // that one out.
    let (markers, _) = with_marker_at(&view, 8.0);
    view.markers = markers;
    let l = laid_out(&view, &mut s);
    let x = l.lane.x_of(&s, view.seconds_of(view.markers[0].at));
    assert_eq!(
        analyze_hit(&l, &view, &s, x, y),
        Some(AnalyzeHit::Marker(id))
    );
    analyze_press(&l, &view, &mut s, x, y, Default::default());
    assert_eq!(
        s.end_lane_drag(),
        AnalyzeLaneEnd::Nothing,
        "pressed, not moved"
    );
    let left = s.without_selected_marker(&view).expect("one removed");
    assert_eq!(left.len(), 1);
    assert_ne!(left[0].id, id);
    assert!(
        s.without_selected_marker(&view).is_none(),
        "and nothing in hand now"
    );
}

/// Auto-slice previews its cuts live: more sensitivity, more transients;
/// notes, beats and equal pieces; all inside the trim. Use as markers makes
/// them markers.
#[test]
fn auto_slice_previews_its_cuts_live() {
    let mut view = a_view();
    let mut s = on(AnalyzePage::Slice, AnalyzeTool::Select);
    assert!(analyze_cuts(&view, &s).is_empty(), "no markers yet");
    s.auto = AutoSlice::Transients { sensitivity: 0.2 };
    let few = analyze_cuts(&view, &s).len();
    s.auto = AutoSlice::Transients { sensitivity: 0.9 };
    let many = analyze_cuts(&view, &s).len();
    assert!(few > 0 && many > few, "{few} then {many}");
    // The knob is what moves it.
    assert_eq!(
        analyze_knob_change(AnalyzeKnob::SliceSensitivity, 20.0, &view, &mut s),
        AnalyzeKnobChange::State
    );
    assert_eq!(analyze_cuts(&view, &s).len(), few);
    s.auto = AutoSlice::Notes;
    assert_eq!(analyze_cuts(&view, &s).len(), 5);
    s.auto = AutoSlice::Beats;
    assert_eq!(analyze_cuts(&view, &s).len(), 23);
    s.auto = AutoSlice::Equal { pieces: 4 };
    let cuts = analyze_cuts(&view, &s);
    assert_eq!(cuts, vec![3.0, 6.0, 9.0]);
    // Inside the trim only.
    view.clean.trim = Some((view.frame_of(2.0), view.frame_of(8.0)));
    let cuts = analyze_cuts(&view, &s);
    assert!((cuts[0] - 3.5).abs() < 1e-6 && cuts.len() == 3, "{cuts:?}");
    let markers = markers_from_cuts(&view, &cuts);
    assert_eq!(markers.len(), 3);
    assert_eq!(markers[0].at, view.frame_of(3.5));
}

// ------------------------------------------------------------ Pitch ---

/// The Pitch card's knobs act on the selected notes: CENTRE moves them all
/// as far as the one it shows, the others set each alike; with nothing
/// selected they say so.
#[test]
fn the_pitch_knobs_set_the_selected_notes() {
    let mut view = a_view();
    let mut s = on(AnalyzePage::Notes, AnalyzeTool::Move);
    assert_eq!(analyze_knob_value(AnalyzeKnob::Centre, &view, &s), None);
    assert!(matches!(
        analyze_knob_change(AnalyzeKnob::Centre, 10.0, &view, &mut s),
        AnalyzeKnobChange::Nothing(_)
    ));
    s.click_note(0, false);
    s.click_note(2, true);
    // The focus is the first selected: sung 4 ct sharp.
    assert_eq!(
        analyze_knob_value(AnalyzeKnob::Centre, &view, &s),
        Some(4.0)
    );
    let AnalyzeKnobChange::Edits(changes) =
        analyze_knob_change(AnalyzeKnob::Centre, 0.0, &view, &mut s)
    else {
        panic!("edits");
    };
    assert_eq!(changes.len(), 2);
    for change in &changes {
        assert_eq!(
            change.edit.map(|e| e.shift_cents),
            Some(-4.0),
            "moved as far"
        );
    }
    let AnalyzeKnobChange::Edits(changes) =
        analyze_knob_change(AnalyzeKnob::Drift, 70.0, &view, &mut s)
    else {
        panic!("edits");
    };
    assert!(
        changes
            .iter()
            .all(|c| c.edit.is_some_and(|e| (e.flatten - 0.7).abs() < 1e-6))
    );
    // Formant and gain are edits too.
    view.melody[0].edit = Some(AnalyzeEdit {
        formant_cents: 150.0,
        gain_db: -2.0,
        ..AnalyzeEdit::default()
    });
    assert_eq!(
        analyze_knob_value(AnalyzeKnob::Formant, &view, &s),
        Some(150.0)
    );
    assert_eq!(
        analyze_knob_value(AnalyzeKnob::NoteGain, &view, &s),
        Some(-2.0)
    );
}

/// Flopsynth's knob: a drag at three precisions, Alt-click to default,
/// a typed value in the knob's unit.
#[test]
fn a_knob_drags_resets_and_takes_a_typed_value() {
    let mut view = a_view();
    view.clean.gain_db = 0.0;
    let mut s = on(AnalyzePage::Clean, AnalyzeTool::Select);
    let l = laid_out(&view, &mut s);
    let knob = l
        .control(AnalyzeControl::Knob(AnalyzeKnob::Gain))
        .expect("the gain knob");
    let (x, y) = centre(knob);
    assert_eq!(
        analyze_press(&l, &view, &mut s, x, y, Default::default()),
        Some(AnalyzeAction::KnobGrab(AnalyzeKnob::Gain))
    );
    let (_, coarse) = s.drag_knob(y - 30.0, Precision::Coarse).unwrap();
    let (_, fine) = s.drag_knob(y - 30.0, Precision::Fine).unwrap();
    assert!(coarse > fine && fine > 0.0, "{coarse} {fine}");
    assert_eq!(s.end_knob_drag(), Some(AnalyzeKnob::Gain));
    let alt = fontelle_ui::canvas::Modifiers {
        alt: true,
        ..Default::default()
    };
    assert_eq!(
        analyze_press(&l, &view, &mut s, x, y, alt),
        Some(AnalyzeAction::KnobReset(AnalyzeKnob::Gain))
    );
    assert_eq!(AnalyzeKnob::Gain.parse("-6 dB"), Some(-6.0));
    assert_eq!(AnalyzeKnob::FadeIn.parse("1.5 s"), Some(1500.0));
    assert_eq!(AnalyzeKnob::Centre.parse("+30"), Some(30.0));
    assert_eq!(AnalyzeKnob::Centre.parse("2 st"), Some(200.0));
    assert_eq!(AnalyzeKnob::Drift.parse("70%"), Some(70.0));
    assert_eq!(AnalyzeKnob::Pieces.parse("12"), Some(12.0));
    assert_eq!(AnalyzeKnob::Gain.parse("loud"), None);
    for knob in [
        AnalyzeKnob::FadeIn,
        AnalyzeKnob::Centre,
        AnalyzeKnob::Threshold,
    ] {
        let (lo, hi) = knob.range();
        for v in [lo, (lo + hi) / 2.0, hi] {
            assert!(
                (knob.from_unit(knob.to_unit(v)) - v).abs() < (hi - lo) * 0.01,
                "{knob:?} {v}"
            );
        }
    }
}

// ----------------------------------------------------------- Record ---

fn with_takes(mut view: AnalyzeView) -> AnalyzeView {
    view.source = AnalyzeSource::Insert {
        track: "Vox".to_string(),
    };
    view.record = Some(AnalyzeRecordView::default());
    view.takes = (1..=2)
        .map(|id| StudyTake {
            id,
            asset: fontelle_types::AssetRef {
                id: Default::default(),
                path: format!("Take {id}.wav").into(),
                content_hash: 0,
                size: 0,
                kind: fontelle_types::AssetKind::Sample,
            },
            name: format!("Take {id}"),
            song_sample: None,
            frames: i64::from(RATE) * 4,
            sample_rate: RATE,
            starred: false,
            dropped_frames: 0,
        })
        .collect();
    view.current_take = Some(1);
    view
}

/// The takes list: a click on a name loads it, the star and the × are
/// theirs, and a drag across a take's audio chooses that part for the comp
/// — the rest of the comp kept.
#[test]
fn takes_load_star_discard_and_comp_by_dragging() {
    let view = with_takes(a_view());
    let mut s = on(AnalyzePage::Record, AnalyzeTool::Select);
    let l = laid_out(&view, &mut s);
    let second = l.takes[1];
    let (x, y) = centre(second.name);
    assert_eq!(
        analyze_press(&l, &view, &mut s, x, y, Default::default()),
        Some(AnalyzeAction::TakeLoad(2))
    );
    let (x, y) = centre(second.star);
    assert_eq!(
        analyze_press(&l, &view, &mut s, x, y, Default::default()),
        Some(AnalyzeAction::TakeStar(2))
    );
    let (x, y) = centre(second.discard);
    assert_eq!(
        analyze_press(&l, &view, &mut s, x, y, Default::default()),
        Some(AnalyzeAction::TakeDiscard(2))
    );
    // A comp: the first second of take 2.
    let lane = second.lane;
    let x0 = fontelle_ui::canvas::take_x_of(&view, lane, 1.0);
    let x1 = fontelle_ui::canvas::take_x_of(&view, lane, 2.0);
    analyze_press(&l, &view, &mut s, x0, lane.y + 4.0, Default::default());
    let Some(AnalyzeLaneChange::Comp(comp)) = s.drag_lane(&l, &view, x1) else {
        panic!("a comp span");
    };
    assert_eq!(comp.len(), 1);
    assert_eq!(comp[0].take, 2);
    assert!((comp[0].start - i64::from(RATE)).abs() < 600, "{comp:?}");
    assert!((comp[0].end - 2 * i64::from(RATE)).abs() < 600, "{comp:?}");
    // Chosen over take 1's span, take 1 keeps what is either side.
    let whole = vec![StudyCompSpan {
        take: 1,
        start: 0,
        end: 4 * i64::from(RATE),
    }];
    let merged = comp_with(&whole, comp[0]);
    assert_eq!(merged.len(), 3);
    assert_eq!((merged[0].take, merged[1].take, merged[2].take), (1, 2, 1));
    assert_eq!(merged[0].end, merged[1].start);
    assert_eq!(merged[1].end, merged[2].start);
}

/// Send to arrangement is offered once there is a take; Use comp once there
/// is a comp; the lamp always.
#[test]
fn the_record_pages_buttons_say_when_they_can_act() {
    let mut view = with_takes(a_view());
    let s = on(AnalyzePage::Record, AnalyzeTool::Select);
    use fontelle_ui::canvas::control_enabled;
    assert!(control_enabled(
        AnalyzeControl::SendToArrangement,
        &view,
        &s
    ));
    assert!(control_enabled(AnalyzeControl::Arm, &view, &s));
    assert!(!control_enabled(AnalyzeControl::UseComp, &view, &s));
    view.comp = vec![StudyCompSpan {
        take: 1,
        start: 0,
        end: 10,
    }];
    assert!(control_enabled(AnalyzeControl::UseComp, &view, &s));
    view.has_audio = false;
    assert!(!control_enabled(
        AnalyzeControl::SendToArrangement,
        &view,
        &s
    ));
    // The insert's track is a source only for an insert.
    view.record = Some(AnalyzeRecordView {
        input: None,
        ..AnalyzeRecordView::default()
    });
    assert!(control_enabled(AnalyzeControl::PostFader, &view, &s));
    assert_eq!(fontelle_ui::canvas::source_text(&view), "This track (Vox)");
}

// ------------------------------------------------------------- zoom ---

/// Z: the lane on the selected notes (or the dragged span); Shift+Z all of
/// it again.
#[test]
fn z_zooms_to_the_selection_and_shift_z_to_all() {
    let view = a_view();
    let mut s = on(AnalyzePage::Notes, AnalyzeTool::Move);
    let l = laid_out(&view, &mut s);
    let all = s.pixels_per_second;
    assert!(!zoom_to(&mut s, &view, &l, false), "nothing selected");
    s.click_note(2, false);
    assert!(zoom_to(&mut s, &view, &l, false));
    assert!(
        s.pixels_per_second > all * 4.0,
        "{} vs {all}",
        s.pixels_per_second
    );
    let left = l.lane.x_of(&s, 2.4);
    let right = l.lane.x_of(&s, 3.6);
    assert!(left >= l.lane.grid.x && right <= l.lane.grid.right());
    assert!(zoom_to(&mut s, &view, &l, true));
    assert!((s.pixels_per_second - all).abs() < 1e-3);
    // On a wave page, the span dragged out.
    let mut s = on(AnalyzePage::Slice, AnalyzeTool::Select);
    let l = laid_out(&view, &mut s);
    s.span = Some((6.0, 7.0));
    assert!(zoom_to(&mut s, &view, &l, false));
    assert!(l.lane.x_of(&s, 6.0) >= l.lane.grid.x);
    let _ = StudyMarker {
        id: 0,
        at: 0,
        name: String::new(),
    };
}

/// A trim pulled in past where the fades reach takes the fades with it:
/// neither is ever longer than half of what is left, so the two never
/// cross into a dip in the middle of a short take.
#[test]
fn a_narrowing_trim_takes_the_fades_with_it() {
    let mut view = a_view();
    view.clean.fade_in = RATE as i64 * 2;
    view.clean.fade_out = RATE as i64 * 2;
    let mut s = on(AnalyzePage::Clean, AnalyzeTool::Select);
    let l = laid_out(&view, &mut s);
    let mid = l.lane.grid.y + l.lane.grid.height / 2.0;
    let start = l.lane.x_of(&s, 0.0);
    analyze_press(&l, &view, &mut s, start + 1.0, mid, Default::default());
    let Some(AnalyzeLaneChange::Clean(clean)) = s.drag_lane(&l, &view, l.lane.x_of(&s, 10.0))
    else {
        panic!("a trim");
    };
    let (a, b) = clean.trim.unwrap();
    let half = (b - a) / 2;
    assert!(clean.fade_in <= half && clean.fade_out <= half, "{clean:?}");
}

/// A fade typed or turned longer than half the take stops at half: the
/// knob says what will be heard.
#[test]
fn a_fade_is_never_longer_than_half_the_take() {
    let view = a_view();
    let mut s = on(AnalyzePage::Clean, AnalyzeTool::Select);
    let (a, b) = view.trim_seconds();
    let half = ((b - a) / 2.0 * f64::from(RATE)).round() as i64;
    for knob in [AnalyzeKnob::FadeIn, AnalyzeKnob::FadeOut] {
        let AnalyzeKnobChange::Clean(clean) = analyze_knob_change(knob, 600_000.0, &view, &mut s)
        else {
            panic!("clean");
        };
        assert!(clean.fade_in <= half && clean.fade_out <= half, "{clean:?}");
    }
}
