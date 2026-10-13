//! Pitch drawn on the grid of a vocal roll, the way OpenUtau, Synthesizer V
//! and Vocaloid draw it: the pen writes the pitch-bend lane as the
//! difference between where the hand is and the pitch the notes make, a
//! line with Shift, a reset back to the notes; Smooth and Snap to scale as
//! commands over a range — and the roll framing a part when it opens and
//! following the lyric field as Tab walks it off screen. All pure.

use fontelle_model::{Arena, LaneCurve, Note};
use fontelle_types::{PPQN, Tick};
use fontelle_ui::canvas::{
    PitchBase, PitchStroke, RollView, StrokeMode, drawn_pitch, follow_note, frame_notes,
    nearest_in_scale, note_menu_anchor, pitch_edit_range, pitch_to_y, smooth_bend,
    snap_bend_to_scale, tick_to_x, y_to_pitch,
};
use fontelle_ui::layout::Rect;

/// C major.
const C_MAJOR: u16 = 0b1010_1011_0101;

fn note(start: Tick, length: Tick, key: u8) -> Note {
    Note {
        start,
        length,
        key,
        velocity: 100,
        pan: 0,
        fine_pitch: 0,
        release: 0,
        mod_x: 0,
        mod_y: 0,
        slide: false,
        path: Vec::new(),
        channel: None,
        lyric: "la".into(),
        voice: None,
    }
}

/// A flat line at `key` from 0 to `to`.
fn flat(key: f32, to: Tick) -> PitchBase {
    PitchBase::from_lines(&[vec![(0, key), (to, key)]])
}

fn grid() -> Rect {
    Rect::new(100.0, 50.0, 800.0, 400.0)
}

fn view() -> RollView {
    RollView {
        scroll_tick: 0,
        top_key: 84,
        key_offset: 0.0,
        pixels_per_tick: 0.1,
        key_height: 16.0,
        ..RollView::default()
    }
}

// ------------------------------------------------------------ the base

#[test]
fn heights_and_pitches_turn_into_each_other_through_the_rows_middles() {
    let (v, g) = (view(), grid());
    // A key's own pitch is the middle of its row.
    let row = fontelle_ui::canvas::key_row(&v, g, 72);
    let y = pitch_to_y(&v, g, 72.0);
    assert!((y - (row.y + row.height / 2.0)).abs() < 1.0);
    // Half a semitone up is half a row up.
    assert!((pitch_to_y(&v, g, 72.5) - (y - 8.0)).abs() < 1e-3);
    for p in [60.0, 61.37, 72.0, 83.9] {
        assert!((y_to_pitch(&v, g, pitch_to_y(&v, g, p)) - p).abs() < 1e-3);
    }
}

#[test]
fn the_base_is_the_computed_line_held_past_its_ends_and_falls_back_to_the_notes() {
    let base = PitchBase::from_lines(&[vec![(0, 60.0), (100, 62.0)], vec![(200, 64.0)]]);
    assert_eq!(base.at(-50), 60.0);
    assert!((base.at(50) - 61.0).abs() < 1e-4);
    assert_eq!(base.at(1000), 64.0);
    // With no line yet (nothing sung), the notes are the pitch.
    let mut notes = Arena::default();
    notes.insert(note(0, PPQN, 60));
    notes.insert(note(PPQN * 2, PPQN, 67));
    let base = PitchBase::from_notes(&notes);
    assert_eq!(base.at(PPQN / 2), 60.0);
    assert_eq!(base.at(PPQN * 2 + 10), 67.0);
    // A rest holds the nearer note.
    assert_eq!(base.at(PPQN + PPQN / 4), 60.0);
    assert_eq!(base.at(PPQN * 2 - 10), 67.0);
    assert!(PitchBase::default().is_empty());
}

// ---------------------------------------------------------- the strokes

#[test]
fn the_pen_writes_the_bend_as_the_difference_from_the_notes_line() {
    let base = flat(60.0, 4 * PPQN);
    let mut stroke = PitchStroke::begin(
        base.clone(),
        LaneCurve::default(),
        StrokeMode::Pencil,
        (100, 60.5),
        10,
    );
    stroke.drag((300, 61.0));
    let bend = stroke.curve();
    // Half a semitone over the notes at the press, a semitone at the end.
    assert!((bend.at(100, 0.0) - 50.0).abs() < 0.5);
    assert!((bend.at(300, 0.0) - 100.0).abs() < 0.5);
    assert!((bend.at(200, 0.0) - 75.0).abs() < 1.0);
    // Outside the stroke, nothing.
    assert_eq!(bend.at(0, 0.0), 0.0);
    assert_eq!(bend.at(1000, 0.0), 0.0);
    // Over a moving line (a glide in the notes) the drawn pitch is what
    // sounds: base + bend is where the hand was.
    let glide = PitchBase::from_lines(&[vec![(0, 60.0), (400, 64.0)]]);
    let mut stroke = PitchStroke::begin(
        glide.clone(),
        LaneCurve::default(),
        StrokeMode::Pencil,
        (0, 61.0),
        5,
    );
    stroke.drag((400, 61.0));
    for t in [0, 100, 200, 300, 400] {
        let sounds = glide.at(t) + stroke.curve().at(t, 0.0) / 100.0;
        assert!((sounds - 61.0).abs() < 0.02, "{t}: {sounds}");
    }
}

#[test]
fn a_line_stroke_is_straight_from_the_press_and_redrawn_as_the_hand_moves() {
    let base = flat(60.0, 4 * PPQN);
    let mut stroke =
        PitchStroke::begin(base, LaneCurve::default(), StrokeMode::Line, (0, 60.0), 10);
    stroke.drag((200, 64.0));
    stroke.drag((400, 62.0));
    let bend = stroke.curve();
    // Only the last line is left: 60 -> 62 over 0..400.
    assert!((bend.at(200, 0.0) - 100.0).abs() < 1.0);
    assert!((bend.at(400, 0.0) - 200.0).abs() < 1.0);
}

#[test]
fn reset_returns_a_dragged_range_to_the_notes_and_keeps_the_rest() {
    let base = flat(60.0, 10 * PPQN);
    let mut bend = LaneCurve::default();
    bend.write(0, 1000, 10, |_| 80.0, 0.0);
    let mut stroke = PitchStroke::begin(base, bend, StrokeMode::Erase, (400, 70.0), 10);
    stroke.drag((600, 50.0));
    let after = stroke.curve();
    assert_eq!(after.at(500, 0.0), 0.0);
    assert_eq!(after.at(450, 0.0), 0.0);
    assert!((after.at(200, 0.0) - 80.0).abs() < 1e-3);
    assert!((after.at(800, 0.0) - 80.0).abs() < 1e-3);
}

#[test]
fn a_bend_is_kept_inside_the_lanes_range() {
    let base = flat(60.0, 4 * PPQN);
    let mut stroke = PitchStroke::begin(
        base,
        LaneCurve::default(),
        StrokeMode::Pencil,
        (0, 90.0),
        10,
    );
    stroke.drag((100, 30.0));
    assert_eq!(stroke.curve().at(0, 0.0), 1200.0);
    assert_eq!(stroke.curve().at(100, 0.0), -1200.0);
}

#[test]
fn the_drawn_line_is_shown_only_where_a_bend_is() {
    let base = flat(60.0, 10 * PPQN);
    let mut bend = LaneCurve::default();
    bend.write(200, 400, 10, |_| 50.0, 0.0);
    let segments = drawn_pitch(&base, &bend, 0, 1000, 10);
    assert_eq!(segments.len(), 1);
    let seg = &segments[0];
    assert!(seg.first().unwrap().0 >= 190 && seg.last().unwrap().0 <= 410);
    assert!(
        seg.iter()
            .all(|(_, p)| (p - 60.5).abs() < 0.02 || *p > 60.0)
    );
    assert!(drawn_pitch(&base, &LaneCurve::default(), 0, 1000, 10).is_empty());
}

// ------------------------------------------------------------- smoothing

#[test]
fn smoothing_takes_the_edges_off_a_drawn_line_and_leaves_the_rest() {
    let mut bend = LaneCurve::default();
    // A square bump: 0, then 100 cents for 200 ticks, then 0.
    bend.write(400, 600, 1000, |_| 100.0, 0.0);
    let smooth = smooth_bend(&bend, 300, 700, 5, 60);
    // The corners are rounded: just inside the bump is now under 100, just
    // outside it over 0.
    assert!(smooth.at(410, 0.0) < 95.0);
    assert!(smooth.at(390, 0.0) > 5.0);
    // The middle of a long held stretch keeps its value.
    assert!((smooth.at(500, 0.0) - 100.0).abs() < 1.0);
    // Outside the range, untouched.
    assert_eq!(smooth.at(200, 0.0), 0.0);
    assert_eq!(smooth.at(800, 0.0), 0.0);
    // Nothing drawn stays nothing.
    assert!(smooth_bend(&LaneCurve::default(), 0, 1000, 5, 60).is_empty());
    // A spike is lowered.
    let mut spike = LaneCurve::default();
    spike.write(500, 510, 1, |_| 300.0, 0.0);
    let s = smooth_bend(&spike, 0, 1000, 5, 60);
    assert!(s.at(505, 0.0) < 150.0);
}

// ----------------------------------------------------------- snap to scale

#[test]
fn nearest_in_scale_is_the_closest_degree() {
    assert_eq!(nearest_in_scale(60.3, C_MAJOR), 60.0);
    assert_eq!(nearest_in_scale(61.2, C_MAJOR), 62.0);
    assert_eq!(nearest_in_scale(60.8, C_MAJOR), 60.0);
    assert_eq!(nearest_in_scale(64.6, C_MAJOR), 65.0);
    // No scale: chromatic.
    assert_eq!(nearest_in_scale(61.2, 0), 61.0);
    assert_eq!(nearest_in_scale(61.2, 0xfff), 61.0);
}

/// 1920 ticks a second (120 bpm at 960 PPQN), as the tests elsewhere use.
const TPS: f64 = 1920.0;

#[test]
fn snapping_moves_held_stretches_onto_the_scale_and_keeps_the_move_between() {
    // Notes at C4 throughout; the hand drew 20 cents sharp of C# for a
    // while (a held pitch off the scale), then a quick rise to E-ish.
    let base = flat(60.0, 4 * TPS as Tick);
    let mut bend = LaneCurve::default();
    let t = |s: f64| (s * TPS) as Tick;
    bend.write(0, t(0.5), 10, |_| 120.0, 0.0);
    bend.write(
        t(0.5),
        t(0.6),
        10,
        |x| 120.0 + (x - t(0.5)) as f32 / (t(0.6) - t(0.5)) as f32 * 270.0,
        0.0,
    );
    bend.write(t(0.6), t(1.2), 10, |_| 390.0, 0.0);
    let snapped = snap_bend_to_scale(&base, &bend, 0, t(1.2), 10, C_MAJOR, TPS);
    let sounds = |c: &LaneCurve, s: f64| base.at(t(s)) + c.at(t(s), 0.0) / 100.0;
    // Held parts on the degree: 61.2 -> 62 (D), 63.9 -> 64 (E).
    assert!(
        (sounds(&snapped, 0.25) - 62.0).abs() < 0.03,
        "{}",
        sounds(&snapped, 0.25)
    );
    assert!(
        (sounds(&snapped, 0.9) - 64.0).abs() < 0.03,
        "{}",
        sounds(&snapped, 0.9)
    );
    // The rise between them still rises, from one to the other.
    let mid = sounds(&snapped, 0.55);
    assert!(mid > 62.0 && mid < 64.0, "{mid}");
    // Snapping twice is snapping once.
    let again = snap_bend_to_scale(&base, &snapped, 0, t(1.2), 10, C_MAJOR, TPS);
    for s in [0.1, 0.3, 0.55, 0.8, 1.1] {
        assert!((sounds(&again, s) - sounds(&snapped, s)).abs() < 0.02);
    }
}

#[test]
fn snapping_keeps_vibrato_on_a_held_note_and_leaves_undrawn_notes_alone() {
    let t = |s: f64| (s * TPS) as Tick;
    // A note off the scale on purpose (C#), nothing drawn over it.
    let mut lines = vec![(0, 61.0), (t(0.5), 61.0)];
    lines.push((t(0.5) + 1, 60.0));
    lines.push((t(2.0), 60.0));
    let base = PitchBase::from_lines(&[lines]);
    // Over the C, a drawn line 30 cents sharp with a 40-cent vibrato.
    let mut bend = LaneCurve::default();
    bend.write(
        t(0.6),
        t(1.8),
        5,
        |x| {
            let s = x as f64 / TPS;
            30.0 + 40.0 * (s * 5.5 * std::f64::consts::TAU).sin() as f32
        },
        0.0,
    );
    let snapped = snap_bend_to_scale(&base, &bend, 0, t(2.0), 5, C_MAJOR, TPS);
    // The C# note is not drawn over and stays a C#.
    assert_eq!(snapped.at(t(0.25), 0.0), 0.0);
    // The vibrato is still there, now around 60.
    let mut lo = f32::MAX;
    let mut hi = f32::MIN;
    let mut sum = 0.0;
    let mut n = 0.0;
    let mut x = t(0.9);
    while x < t(1.5) {
        let p = base.at(x) + snapped.at(x, 0.0) / 100.0;
        lo = lo.min(p);
        hi = hi.max(p);
        sum += p;
        n += 1.0;
        x += 5;
    }
    assert!((sum / n - 60.0).abs() < 0.06, "centre {}", sum / n);
    assert!(hi - lo > 0.6, "vibrato kept ({})", hi - lo);
}

// ----------------------------------------------------------------- ranges

#[test]
fn a_pitch_command_works_on_the_selected_notes_then_the_time_selection_then_all() {
    let mut notes = Arena::default();
    notes.insert(note(0, PPQN, 60));
    let b = notes.insert(note(PPQN * 2, PPQN, 62));
    let c = notes.insert(note(PPQN * 4, PPQN, 64));
    assert_eq!(
        pitch_edit_range(&notes, &[c, b], Some((10, 20))),
        Some((PPQN * 2, PPQN * 5))
    );
    assert_eq!(
        pitch_edit_range(&notes, &[], Some((10, 20))),
        Some((10, 20))
    );
    assert_eq!(pitch_edit_range(&notes, &[], None), Some((0, PPQN * 5)));
    assert_eq!(pitch_edit_range(&Arena::default(), &[], None), None);
}

// ------------------------------------------------- framing and following

#[test]
fn opening_a_part_frames_its_notes() {
    let (v, g) = (view(), grid());
    let mut notes = Arena::default();
    notes.insert(note(PPQN * 40, PPQN, 52));
    notes.insert(note(PPQN * 42, PPQN * 2, 59));
    let framed = frame_notes(&v, g, &notes);
    // The first note is on screen, a little in from the left edge.
    let x = tick_to_x(&framed, g, PPQN * 40);
    assert!(x > g.x && x < g.x + g.width * 0.25, "{x}");
    // Both keys are on screen, the middle of them near the middle.
    for key in [52u8, 59] {
        let row = fontelle_ui::canvas::key_row(&framed, g, key);
        assert!(row.y >= g.y && row.bottom() <= g.bottom(), "{key}");
    }
    let mid = pitch_to_y(&framed, g, 55.5);
    assert!((mid - (g.y + g.height / 2.0)).abs() < framed.key_height * 1.5);
    // The zoom is the writer's own.
    assert_eq!(framed.pixels_per_tick, v.pixels_per_tick);
    assert_eq!(framed.key_height, v.key_height);
    // Nothing to frame: nothing moves.
    assert_eq!(frame_notes(&v, g, &Arena::default()), v);
}

#[test]
fn the_lyric_field_is_followed_when_tab_takes_it_off_screen() {
    let (v, g) = (view(), grid());
    // On screen: nothing moves.
    let here = note(PPQN * 2, PPQN, 72);
    assert_eq!(follow_note(&v, g, &here, 72.0), None);
    // Past the right edge (8000 px of grid is 8000 ticks at 0.1): scrolled
    // so it sits a quarter in, with room for what comes next.
    let right = note(9000, PPQN, 72);
    let moved = follow_note(&v, g, &right, 72.0).expect("scrolled");
    let x = tick_to_x(&moved, g, 9000);
    assert!(x >= g.x && x + 72.0 <= g.right());
    assert!(x < g.x + g.width * 0.5);
    // Behind the left edge (Shift+Tab).
    let scrolled = RollView {
        scroll_tick: 20_000,
        ..v
    };
    let left = note(15_000, PPQN, 72);
    let moved = follow_note(&scrolled, g, &left, 72.0).expect("scrolled back");
    let x = tick_to_x(&moved, g, 15_000);
    assert!(x >= g.x && x + 72.0 <= g.right());
    // A field whose note is too wide for the edge still shows its start.
    // Above the top: the key is brought into view.
    let high = note(PPQN * 2, PPQN, 100);
    let moved = follow_note(&v, g, &high, 72.0).expect("scrolled up");
    let row = fontelle_ui::canvas::key_row(&moved, g, 100);
    assert!(row.y >= g.y && row.bottom() <= g.bottom());
    assert_eq!(moved.scroll_tick, v.scroll_tick);
}

#[test]
fn the_note_menu_opens_at_the_selected_note() {
    let (v, g) = (view(), grid());
    let n = note(PPQN, PPQN, 72);
    let (x, y) = note_menu_anchor(&v, g, &n);
    let row = fontelle_ui::canvas::key_row(&v, g, 72);
    assert!((x - tick_to_x(&v, g, PPQN)).abs() < 1.0);
    assert!((y - row.bottom()).abs() < 1.0);
    // A note off screen: the menu stays on the grid.
    let far = note(100_000, PPQN, 10);
    let (x, y) = note_menu_anchor(&v, g, &far);
    assert!(g.contains(x, y) || (x <= g.right() && y <= g.bottom()));
}

#[test]
fn the_note_menu_offers_the_pitch_commands_and_they_are_not_voice_changes() {
    use fontelle_ui::canvas::{PitchCommand, VocalNoteRow, apply_note_row, vocal_note_menu_rows};
    let caps = fontelle_sing::Capabilities {
        lanes: vec![fontelle_sing::Lane::PitchBend],
        ..Default::default()
    };
    let rows = vocal_note_menu_rows(None, &caps, "x");
    for command in [
        PitchCommand::Smooth,
        PitchCommand::Snap,
        PitchCommand::Reset,
    ] {
        let (entry, row) = rows
            .iter()
            .find(|(_, r)| *r == VocalNoteRow::Pitch(command))
            .expect("offered");
        assert!(entry.enabled);
        assert!(entry.label.to_lowercase().contains("pitch"));
        assert_eq!(apply_note_row(row, None, "x"), None);
    }
    // A voice that ignores the bend still offers them, greyed and saying so.
    let rows = vocal_note_menu_rows(None, &fontelle_sing::Capabilities::default(), "x");
    let (entry, _) = rows
        .iter()
        .find(|(_, r)| *r == VocalNoteRow::Pitch(PitchCommand::Smooth))
        .unwrap();
    assert!(!entry.enabled);
}
