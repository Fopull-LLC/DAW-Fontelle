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
use fontelle_types::StudyTake;
use fontelle_ui::canvas::{
    AnalyzeAction, AnalyzeCard, AnalyzeClarity, AnalyzeControl, AnalyzeHit, AnalyzeKey,
    AnalyzeKnob, AnalyzeLayout, AnalyzeMode, AnalyzePage, AnalyzeRecordView, AnalyzeSource,
    AnalyzeState, AnalyzeTool, AnalyzeView, AnalyzedChord, AnalyzedNote, RowShade, SCALES,
    analyze_cents_tag, analyze_hit, analyze_layout, analyze_press, analyze_row_shade,
    analyze_strings, analyze_tip, estimated_width, scale_degrees,
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

/// A view as a recording study has it: two takes, the second in the lane.
fn a_recording_view() -> AnalyzeView {
    let mut view = a_view();
    view.source = AnalyzeSource::Insert {
        track: "Vox".to_string(),
    };
    view.record = Some(AnalyzeRecordView {
        inputs: vec!["Mic".to_string()],
        ..AnalyzeRecordView::default()
    });
    view.takes = (1..=2)
        .map(|id| StudyTake {
            id,
            asset: fontelle_types::AssetRef {
                id: Default::default(),
                path: format!("recordings/Take {id}.wav").into(),
                content_hash: 0,
                size: 0,
                kind: fontelle_types::AssetKind::Sample,
            },
            name: format!("Take {id}"),
            song_sample: None,
            frames: 48_000 * 6,
            sample_rate: 48_000,
            starred: id == 2,
            dropped_frames: 0,
        })
        .collect();
    view.take_peaks = vec![view.peaks.clone(), view.peaks.clone()];
    view.current_take = Some(2);
    view
}

/// Every page — Notes, Clean, Slice, Record with takes and without — fits
/// the window at every scale and at its smallest: its cards under the
/// canopy, its controls inside their cards and apart, and everything the
/// pointer can be over says what it is.
#[test]
fn every_page_fits_and_says_what_everything_is() {
    let mut views = vec![(AnalyzePage::Notes, a_view())];
    for page in [AnalyzePage::Clean, AnalyzePage::Slice, AnalyzePage::Record] {
        let mut view = a_view();
        view.rate = 48_000;
        view.has_audio = true;
        views.push((page, view));
    }
    views.push((AnalyzePage::Record, a_recording_view()));
    for (page, view) in &views {
        for scale in SCALES {
            for (w, h) in [analyze_window_size(scale), analyze_minimum_size(scale)] {
                let mut s = state(scale);
                s.page = *page;
                let l = laid_out(w as f32, h as f32, view, &mut s);
                assert_eq!(l.cards.len(), 3, "{page:?}: three cards");
                for (name, rect) in l.named() {
                    assert!(
                        inside(l.body, rect),
                        "{page:?}: {name} {rect:?} outside {:?} at {scale} {w}x{h}",
                        l.body
                    );
                    if name.starts_with("card.") {
                        assert!(!overlaps(rect, l.canopy), "{page:?}: {name} over the lane");
                    }
                }
                for (control, rect) in &l.controls {
                    let card = l
                        .cards
                        .iter()
                        .find(|(_, c)| c.frame.contains(rect.x + 1.0, rect.y + 1.0))
                        .unwrap_or_else(|| panic!("{page:?}: {control:?} is on no card"));
                    assert!(
                        inside(card.1.frame, *rect),
                        "{page:?}: {control:?} {rect:?} spills out of {:?} at {scale} {w}x{h}",
                        card.0
                    );
                    for (other, r) in &l.controls {
                        if other != control {
                            assert!(
                                !overlaps(*rect, *r),
                                "{page:?}: {control:?} overlaps {other:?} at {scale} {w}x{h}"
                            );
                        }
                    }
                    let (x, y) = centre(*rect);
                    let hit = analyze_hit(&l, view, &s, x, y);
                    assert_eq!(hit, Some(AnalyzeHit::Control(*control)), "{page:?}");
                    assert!(analyze_tip(&hit.unwrap(), view, &s).is_some_and(|t| !t.is_empty()));
                }
                // Nothing but Notes has the coming-soon card.
                assert!(l.later.is_empty());
                let strings = analyze_strings(view, &s, &l);
                assert!(
                    !strings.iter().any(|(t, _)| t.contains("later update")),
                    "{page:?}"
                );
            }
        }
        // And a sweep: every hit has a tip.
        let mut s = state(1.0);
        s.page = *page;
        let (w, h) = analyze_window_size(1.0);
        let l = laid_out(w as f32, h as f32, view, &mut s);
        let mut y = l.body.y;
        while y < l.body.bottom() {
            let mut x = l.body.x;
            while x < l.body.right() {
                if let Some(hit) = analyze_hit(&l, view, &s, x, y) {
                    assert!(
                        analyze_tip(&hit, view, &s).is_some_and(|t| !t.is_empty()),
                        "{page:?}: {hit:?} at ({x}, {y}) has no tip"
                    );
                }
                x += 9.0;
            }
            y += 9.0;
        }
    }
}

/// Each page's cards are its own: Clean is noise, denoise and the trim;
/// Slice the points, the layout, the send; Record the source, the lamp and
/// the takes.
#[test]
fn each_page_has_its_cards_and_its_tools() {
    let view = a_recording_view();
    let (w, h) = analyze_window_size(1.0);
    let expect = [
        (
            AnalyzePage::Notes,
            [AnalyzeCard::Note, AnalyzeCard::Pitch, AnalyzeCard::Output],
            vec![AnalyzeTool::Select, AnalyzeTool::Move],
        ),
        (
            AnalyzePage::Clean,
            [AnalyzeCard::Noise, AnalyzeCard::Denoise, AnalyzeCard::Shape],
            vec![AnalyzeTool::Select, AnalyzeTool::Noise],
        ),
        (
            AnalyzePage::Slice,
            [AnalyzeCard::Points, AnalyzeCard::Layout, AnalyzeCard::Send],
            vec![AnalyzeTool::Select, AnalyzeTool::Marker],
        ),
        (
            AnalyzePage::Record,
            [AnalyzeCard::Source, AnalyzeCard::Record, AnalyzeCard::Takes],
            vec![],
        ),
    ];
    for (page, cards, tools) in expect {
        let mut s = state(1.0);
        s.page = page;
        let l = laid_out(w as f32, h as f32, &view, &mut s);
        let have: Vec<AnalyzeCard> = l.cards.iter().map(|(c, _)| *c).collect();
        assert_eq!(have, cards);
        let have: Vec<AnalyzeTool> = l.tools.iter().map(|(t, _)| *t).collect();
        assert_eq!(have, tools, "{page:?}");
    }
    // The Pitch card has its seven knobs; the Record card its lamp.
    let mut s = state(1.0);
    let l = laid_out(w as f32, h as f32, &view, &mut s);
    for knob in AnalyzeKnob::PITCH {
        assert!(l.control(AnalyzeControl::Knob(knob)).is_some(), "{knob:?}");
    }
    s.page = AnalyzePage::Record;
    let l = laid_out(w as f32, h as f32, &view, &mut s);
    assert!(l.control(AnalyzeControl::Arm).is_some());
    assert_eq!(l.takes.len(), 2);
    assert!(
        !l.lane.grid.is_empty(),
        "the take in the lane, over the takes"
    );
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

/// Melody mode's chord lane is the line's bar-by-bar reading, not the
/// half-second one that named every sung note a chord (the coordinator, on
/// `analyze-real-edited`: *"Csus2", "Dsus2", "E5", "G5"*).
#[test]
fn melody_mode_reads_its_chord_lane_a_bar_at_a_time() {
    let mut view = a_view();
    view.chords = (0..12)
        .map(|k| AnalyzedChord {
            start: k as f64,
            end: k as f64 + 1.0,
            label: format!("E5 {k}"),
        })
        .collect();
    view.melody_chords = vec![AnalyzedChord {
        start: 0.0,
        end: 4.0,
        label: "Am".to_string(),
    }];
    let mut s = state(1.0);
    assert_eq!(s.chord_spans(&view), &view.melody_chords[..]);
    let (w, h) = analyze_window_size(1.0);
    let l = laid_out(w as f32, h as f32, &view, &mut s);
    let strings: Vec<String> = analyze_strings(&view, &s, &l)
        .into_iter()
        .map(|(t, _)| t)
        .collect();
    assert!(strings.contains(&"Am".to_string()));
    assert!(!strings.iter().any(|t| t.starts_with("E5")), "{strings:?}");
    s.mode = Some(AnalyzeMode::Chords);
    assert_eq!(s.chord_spans(&view), &view.chords[..]);
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

// ------------------------------------------------- the pitch picture ---

/// Ty, trying P1: *"i think the pitch picture is more useful to look at than
/// the waveform so lets make that the default"*. Tab still shows the
/// waveform, and another clip keeps whichever was chosen.
#[test]
fn the_pitch_picture_is_the_default_and_the_choice_is_kept() {
    let state = AnalyzeState::default();
    assert!(state.spectrogram, "the pitch picture is what opens");
    let mut waveform = AnalyzeState::default();
    waveform.spectrogram = false;
    assert!(!waveform.for_another_clip().spectrogram);
    assert!(state.for_another_clip().spectrogram);
}

/// The model's contour is never zero: about a tenth of full scale sits in
/// every cell, and drawn as it is that is the "cloudy" Ty saw. The floor is
/// what most of the picture is, and what a note rises out of.
#[test]
fn the_pitch_picture_floor_sits_on_the_noise_and_under_the_notes() {
    use fontelle_ui::canvas::pitch_picture_floor;
    // A floor of 70..90 everywhere, as the model's is, and a few loud cells.
    let mut data: Vec<u8> = (0..20_000u32)
        .map(|i| 70 + ((i.wrapping_mul(2_654_435_761) >> 24) % 21) as u8)
        .collect();
    for i in (0..data.len()).step_by(40) {
        data[i] = 220;
    }
    let floor = pitch_picture_floor(&data);
    assert!(floor >= 85, "{floor} is over most of the noise");
    assert!(floor < 160, "{floor} is well under a note");
    // Nothing but silence: no floor to speak of, nothing to draw.
    assert!(pitch_picture_floor(&[0u8; 100]) < 10);
    assert_eq!(pitch_picture_floor(&[]), 0);
}

// ------------------------------------------------ P2: hear it, move it ---

use fontelle_ui::canvas::{AnalyzeEdit, AnalyzeEditOp, AnalyzeNotePart, nearest_note};

/// Ty: *"theres no playhead ... so i cant preview what im making"*. A click
/// on the ruler puts the cursor there; Space plays from it, round a region
/// dragged out on the ruler when there is one; Enter plays the selection.
#[test]
fn the_ruler_puts_the_cursor_and_drags_out_a_region() {
    let view = a_view();
    let mut state = state(1.0);
    let l = laid_out(1180.0, 740.0, &view, &mut state);
    let ruler = l.lane.ruler;
    let x = l.lane.x_of(&state, 3.0);
    let action = analyze_press(
        &l,
        &view,
        &mut state,
        x,
        ruler.y + ruler.height / 2.0,
        Default::default(),
    );
    match action {
        Some(AnalyzeAction::Seek(t)) => assert!((t - 3.0).abs() < 0.02, "{t}"),
        other => panic!("{other:?}"),
    }
    assert!((state.cursor - 3.0).abs() < 0.02);
    assert_eq!(state.space_range(&view).1, None, "no region: to the end");
    // Dragged on to 5 s: a region, which Space loops.
    assert!(state.drag_ruler(5.0));
    state.end_ruler();
    let (from, to, looped) = state.space_range(&view);
    assert!((from - 3.0).abs() < 0.02 && to.is_some_and(|b| (b - 5.0).abs() < 1e-9) && looped);
    // Enter: the selected notes, first start to last end.
    state.click_note(1, false);
    state.click_note(2, true);
    assert_eq!(state.selection_range(&view), Some((1.5, 3.6)));
}

/// The playhead stays on screen while it plays: past the right edge, the
/// lane pages on.
#[test]
fn the_lane_follows_the_playhead() {
    let view = a_view();
    let mut state = state(1.0);
    let l = laid_out(1180.0, 740.0, &view, &mut state);
    state.pixels_per_second = 300.0;
    state.start = 0.0;
    state.playhead = Some(9.0);
    state.follow(&l.lane, &view);
    let x = l.lane.x_of(&state, 9.0);
    assert!(x > l.lane.grid.x && x < l.lane.grid.right(), "{x}");
}

/// A click on a note, in either tool, asks to hear **that note's audio**
/// (Ty: *"its just playing like a synth wave"*); the key column is the
/// reference tone, and says so.
#[test]
fn a_click_on_a_note_plays_its_audio_and_the_keys_are_a_reference_tone() {
    let view = a_view();
    for tool in AnalyzeTool::ALL {
        let mut state = state(1.0);
        state.tool = tool;
        let l = laid_out(1180.0, 740.0, &view, &mut state);
        let blob = l.blob(&view, &state, 2).unwrap();
        let (x, y) = centre(blob);
        let action = analyze_press(&l, &view, &mut state, x, y, Default::default());
        assert_eq!(action, Some(AnalyzeAction::PlayNote(2)), "{tool:?}");
        assert!(state.is_selected(2));
        assert_eq!(state.dragging_note(), tool == AnalyzeTool::Move);
    }
    let mut state = state(1.0);
    let l = laid_out(1180.0, 740.0, &view, &mut state);
    let key = l.lane.row(&state, 64);
    let tip = analyze_tip(
        &analyze_hit(
            &l,
            &view,
            &state,
            l.lane.keys.x + 4.0,
            key.y + key.height / 2.0,
        )
        .unwrap(),
        &view,
        &state,
    )
    .unwrap();
    assert!(tip.contains("reference tone"), "{tip}");
}

/// Ty: *"i dont have the ability to drag notes around in here"*. Dragged up
/// a row and a bit, a note lands a semitone up (snapped, from its sung
/// centre); with Alt, to the cent; the rest of the selection moves as far.
#[test]
fn dragging_a_note_repitches_it_by_semitones() {
    let view = a_view();
    let mut state = state(1.0);
    let l = laid_out(1180.0, 740.0, &view, &mut state);
    state.click_note(0, false);
    state.click_note(3, true);
    let blob = l.blob(&view, &state, 3).unwrap();
    let (x, y) = centre(blob);
    analyze_press(&l, &view, &mut state, x, y, Default::default());
    let up = y - state.row_height * 1.3;
    let changes = state
        .drag_note(&l.lane, &view, (x, up), false)
        .unwrap()
        .unwrap();
    assert_eq!(changes.len(), 2, "the whole selection");
    // Note 3 was sung 18 cents flat of A4: snapped, it lands on A#4 exactly.
    let moved = changes
        .iter()
        .find(|c| c.start == 3.8)
        .unwrap()
        .edit
        .unwrap();
    assert!((moved.shift_cents - 118.0).abs() < 0.01, "{moved:?}");
    let other = changes
        .iter()
        .find(|c| c.start == 0.5)
        .unwrap()
        .edit
        .unwrap();
    assert!((other.shift_cents - 118.0).abs() < 0.01);
    let free = state
        .drag_note(&l.lane, &view, (x, up), true)
        .unwrap()
        .unwrap();
    let free = free.iter().find(|c| c.start == 3.8).unwrap().edit.unwrap();
    assert!((free.shift_cents - 130.0).abs() < 0.5, "{free:?}");
    assert!(state.end_note_drag(), "it moved");
}

/// A moved note's ends are its glides, in the Move tool.
#[test]
fn a_moved_notes_ends_set_its_glides() {
    let mut view = a_view();
    view.melody[2].edit = Some(AnalyzeEdit {
        shift_cents: 100.0,
        ..AnalyzeEdit::default()
    });
    let mut state = state(1.0);
    let l = laid_out(1180.0, 740.0, &view, &mut state);
    let blob = l.blob(&view, &state, 2).unwrap();
    let y = blob.y + blob.height / 2.0;
    assert_eq!(
        analyze_hit(&l, &view, &state, blob.x + 1.0, y),
        Some(AnalyzeHit::NoteEnd(2, AnalyzeNotePart::Start))
    );
    analyze_press(&l, &view, &mut state, blob.x + 1.0, y, Default::default());
    let to = l.lane.x_of(&state, 2.4 + 0.2);
    let changes = state
        .drag_note(&l.lane, &view, (to, y), false)
        .unwrap()
        .unwrap();
    let glide = changes[0].edit.unwrap().glide_in_ms;
    assert!((glide - 200.0).abs() < 15.0, "{glide}");
}

/// ↑ ↓, Q (half way, then the whole way; in the scale when it is shown), F,
/// V and Del, on the selected notes; nothing selected, or a chord note,
/// says why.
#[test]
fn the_edit_keys() {
    let view = a_view();
    let mut state = state(1.0);
    assert!(
        state.edit(&view, AnalyzeEditOp::Snap).is_err(),
        "nothing selected"
    );
    state.click_note(3, false); // A4, 18 cents flat
    let shift =
        |state: &AnalyzeState, view: &AnalyzeView, op| state.edit(view, op).unwrap()[0].edit;
    assert_eq!(
        shift(&state, &view, AnalyzeEditOp::Nudge(100.0))
            .unwrap()
            .shift_cents,
        100.0
    );
    let half = shift(&state, &view, AnalyzeEditOp::Snap).unwrap();
    assert!((half.shift_cents - 9.0).abs() < 0.01, "{half:?}");
    let mut halfway = view.clone();
    halfway.melody[3].edit = Some(half);
    let full = shift(&state, &halfway, AnalyzeEditOp::Snap).unwrap();
    assert!((full.shift_cents - 18.0).abs() < 0.01, "{full:?}");
    let mut there = view.clone();
    there.melody[3].edit = Some(full);
    assert!(
        (shift(&state, &there, AnalyzeEditOp::Snap)
            .unwrap()
            .shift_cents
            - 18.0)
            .abs()
            < 0.01
    );
    assert_eq!(
        shift(&state, &view, AnalyzeEditOp::Flatten)
            .unwrap()
            .flatten,
        0.7
    );
    assert_eq!(
        shift(&state, &view, AnalyzeEditOp::Vibrato)
            .unwrap()
            .vibrato,
        0.5
    );
    assert_eq!(shift(&state, &view, AnalyzeEditOp::Reset), None);
    // In A minor, G#4 (68) is out: 67.6 snaps to G4, not G#4.
    assert_eq!(nearest_note(6_780.0, None), 6_800.0);
    assert_eq!(
        nearest_note(6_780.0, KeyScale::new(9, "natural-minor").mask()),
        6_700.0
    );
    // A chord note cannot be moved yet, and says so.
    let mut chords = state.clone();
    chords.mode = Some(AnalyzeMode::Chords);
    chords.click_note(0, false);
    let said = chords.edit(&view, AnalyzeEditOp::Nudge(100.0)).unwrap_err();
    assert!(said.contains("Chord notes can't be moved yet"), "{said}");
}

/// The glass has the tools and the transport, and the Output card Render
/// to clip and its ▾; Revert only when the clip plays a render. Every one
/// fits, overlaps nothing, and says what it is.
#[test]
fn the_transport_and_render_are_there_and_fit() {
    let mut view = a_view();
    for rendered in [false, true] {
        view.rendered = rendered;
        for scale in SCALES {
            let mut state = state(scale);
            let (w, h) = analyze_window_size(scale);
            let l = laid_out(w as f32, h as f32, &view, &mut state);
            let named = l.named();
            for name in [
                "transport.play",
                "transport.listen",
                "tool.move",
                "tool.select",
                "card.output.render",
                "card.output.render-menu",
            ] {
                let rect = named
                    .iter()
                    .find(|(n, _)| n == name)
                    .unwrap_or_else(|| panic!("{name}"))
                    .1;
                assert!(!rect.is_empty(), "{name} at {scale}");
                let hit = analyze_hit(&l, &view, &state, centre(rect).0, centre(rect).1).unwrap();
                assert!(analyze_tip(&hit, &view, &state).is_some());
            }
            assert_eq!(
                named.iter().any(|(n, _)| n == "card.output.revert"),
                rendered
            );
            // The glass row: tabs, tools, transport, switches, apart.
            let glass: Vec<Rect> = l
                .tabs
                .iter()
                .map(|(_, r)| *r)
                .chain(l.tools.iter().map(|(_, r)| *r))
                .chain([
                    l.play,
                    l.listen,
                    l.wave_toggle,
                    l.chords_toggle,
                    l.scale_toggle,
                    l.window_scale,
                ])
                .collect();
            for (i, a) in glass.iter().enumerate() {
                for b in glass.iter().skip(i + 1) {
                    assert!(!overlaps(*a, *b), "{a:?} and {b:?} at {scale}");
                }
            }
            assert!(
                l.readout.right() <= l.wave_toggle.x + 0.5,
                "the read-out fits at {scale}"
            );
            assert!(!overlaps(l.render, l.copy_scale), "at {scale}");
        }
    }
}
