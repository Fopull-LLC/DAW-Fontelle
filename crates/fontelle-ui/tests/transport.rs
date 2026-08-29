//! The transport bar's view-model (item 7 of `docs/first-usable-plan.md`).
//!
//! This is the first feature where the window and the audio thread coexist,
//! and the plan puts it before the piano roll on purpose: prove the threading
//! shape on the simplest thing. The shape is TDD §2.2's — **commands down,
//! atomics up** — and here it is one trait, `TransportHost`, with a fake on
//! this side of it. Everything below runs with no window and no sound card.

use fontelle_ui::layout::Rect;
use fontelle_ui::theme::Theme;
use fontelle_ui::transport::{
    HOLD_SECONDS, METER_FLOOR_DB, Meter, RELEASE_DB_PER_SECOND, TransportHit, TransportHost,
    TransportView, apply, format_bars_beats, format_clock, hit, meter_fill, playhead_x, sample_at,
    transport_bar_layout,
};

const RATE: f64 = 48_000.0;

fn view() -> TransportView {
    TransportView {
        available: true,
        playing: false,
        recording: false,
        position_sample: 0,
        position_beats: 0.0,
        length_samples: (RATE * 8.0) as i64,
        sample_rate: RATE,
        looping: false,
        loop_range_samples: (0, 0),
        peaks: [0.0, 0.0],
        reduction_db: 0.0,
    }
}

fn bar() -> Rect {
    Rect::new(8.0, 8.0, 1264.0, 36.0)
}

// ---------------------------------------------------------------- layout ---

#[test]
fn every_piece_of_the_bar_is_inside_it_and_none_of_them_overlap() {
    let l = transport_bar_layout(bar(), &Theme::dark_default().metrics);
    let pieces = [l.play, l.stop, l.loop_toggle, l.readout, l.ruler, l.meter];

    for (i, a) in pieces.iter().enumerate() {
        assert!(!a.is_empty(), "piece {i} has no room");
        assert_eq!(
            a.intersection(&l.bar),
            *a,
            "piece {i} at {a:?} is not inside the bar {:?}",
            l.bar
        );
        for (j, b) in pieces.iter().enumerate().skip(i + 1) {
            assert!(!a.intersects(b), "pieces {i} and {j} overlap: {a:?} {b:?}");
        }
    }
}

#[test]
fn the_ruler_takes_the_space_the_fixed_pieces_do_not() {
    let m = Theme::dark_default().metrics;
    let narrow = transport_bar_layout(Rect::new(0.0, 0.0, 800.0, 36.0), &m);
    let wide = transport_bar_layout(Rect::new(0.0, 0.0, 1600.0, 36.0), &m);

    // The buttons, the readout and the meter are fixed; only the ruler grows.
    assert_eq!(narrow.play.width, wide.play.width);
    assert_eq!(narrow.meter.width, wide.meter.width);
    assert_eq!(wide.ruler.width, narrow.ruler.width + 800.0);
}

#[test]
fn a_bar_too_narrow_for_its_controls_yields_empty_rects_never_negative_ones() {
    let m = Theme::dark_default().metrics;
    for width in [0.0, 1.0, 40.0, 120.0] {
        let l = transport_bar_layout(Rect::new(0.0, 0.0, width, 36.0), &m);
        for r in [l.play, l.stop, l.loop_toggle, l.readout, l.ruler, l.meter] {
            assert!(
                r.width >= 0.0 && r.height >= 0.0,
                "width {width} produced {r:?}"
            );
        }
    }
}

// ------------------------------------------------------------ hit-testing ---

#[test]
fn clicking_a_button_reports_that_button() {
    let l = transport_bar_layout(bar(), &Theme::dark_default().metrics);
    let v = view();
    let centre = |r: Rect| (r.x + r.width / 2.0, r.y + r.height / 2.0);

    for (rect, expected) in [
        (l.play, TransportHit::Play),
        (l.stop, TransportHit::Stop),
        (l.loop_toggle, TransportHit::ToggleLoop),
    ] {
        let (x, y) = centre(rect);
        assert_eq!(hit(&l, &v, x, y), Some(expected));
    }
}

#[test]
fn clicking_the_ruler_asks_to_seek_to_where_you_clicked() {
    let l = transport_bar_layout(bar(), &Theme::dark_default().metrics);
    let v = view();

    // The very first pixel of the ruler is the start of the song. Half a
    // pixel in is already two hundred samples at this zoom, which is why the
    // edge is the assertion and not "near zero".
    let left = hit(&l, &v, l.ruler.x, l.ruler.y + 2.0);
    assert_eq!(left, Some(TransportHit::Scrub(0)));

    let right = hit(&l, &v, l.ruler.right() - 0.001, l.ruler.y + 2.0);
    match right {
        Some(TransportHit::Scrub(sample)) => assert!(
            sample > v.length_samples - v.length_samples / 500,
            "the far end of the ruler asked for {sample}, not the end of the song"
        ),
        other => panic!("expected a scrub, got {other:?}"),
    }

    let middle = hit(&l, &v, l.ruler.x + l.ruler.width / 2.0, l.ruler.y + 2.0);
    match middle {
        Some(TransportHit::Scrub(sample)) => {
            let half = v.length_samples / 2;
            assert!(
                (sample - half).abs() < v.length_samples / 100,
                "clicking the middle of the ruler asked for {sample}, not about {half}"
            );
        }
        other => panic!("expected a scrub, got {other:?}"),
    }
}

#[test]
fn clicking_the_gaps_does_nothing() {
    let l = transport_bar_layout(bar(), &Theme::dark_default().metrics);
    let v = view();
    // Above the bar, below it, and on the readout — which is a label, not a
    // control. A click that lands nowhere must not be a seek to zero.
    assert_eq!(hit(&l, &v, l.play.x + 1.0, l.bar.y - 5.0), None);
    assert_eq!(hit(&l, &v, l.play.x + 1.0, l.bar.bottom() + 5.0), None);
    assert_eq!(
        hit(
            &l,
            &v,
            l.readout.x + l.readout.width / 2.0,
            l.readout.y + 4.0
        ),
        None
    );
}

#[test]
fn a_window_with_no_engine_behind_it_ignores_every_click() {
    let l = transport_bar_layout(bar(), &Theme::dark_default().metrics);
    let v = TransportView::unavailable();
    // A transport bar with no audio device is drawn, so the window is not a
    // different shape without one — but pressing play on nothing is a lie.
    for rect in [l.play, l.stop, l.loop_toggle, l.ruler] {
        let (x, y) = (rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
        assert_eq!(hit(&l, &v, x, y), None, "a dead transport answered a click");
    }
}

// -------------------------------------------------------------- playhead ---

#[test]
fn the_playhead_starts_at_the_left_and_ends_at_the_right() {
    let ruler = Rect::new(100.0, 0.0, 400.0, 20.0);
    let length = 96_000;
    assert_eq!(playhead_x(ruler, 0, length), 100.0);
    assert_eq!(playhead_x(ruler, length, length), 500.0);
    assert_eq!(playhead_x(ruler, length / 2, length), 300.0);
}

#[test]
fn a_playhead_past_either_end_stays_on_the_ruler() {
    let ruler = Rect::new(100.0, 0.0, 400.0, 20.0);
    // A song that has run past its own end (the release tail) must not draw
    // its playhead into the meter.
    assert_eq!(playhead_x(ruler, -5000, 96_000), 100.0);
    assert_eq!(playhead_x(ruler, 500_000, 96_000), 500.0);
}

#[test]
fn an_empty_song_puts_the_playhead_at_the_start_rather_than_dividing_by_zero() {
    let ruler = Rect::new(100.0, 0.0, 400.0, 20.0);
    assert_eq!(playhead_x(ruler, 0, 0), 100.0);
    assert_eq!(playhead_x(ruler, 1000, 0), 100.0);
    assert_eq!(sample_at(ruler, 300.0, 0), 0);
}

#[test]
fn clicking_where_the_playhead_is_seeks_to_where_it_is() {
    let ruler = Rect::new(100.0, 0.0, 400.0, 20.0);
    let length = 96_000;
    // The round trip is what makes a scrub land under the cursor. One pixel
    // is 240 samples here, so the tolerance is one pixel's worth.
    for position in [0, 1, 12_345, 48_000, 95_999, length] {
        let back = sample_at(ruler, playhead_x(ruler, position, length), length);
        assert!(
            (back - position).abs() <= length / ruler.width as i64,
            "{position} came back as {back}"
        );
    }
}

#[test]
fn a_scrub_outside_the_ruler_clamps_to_the_song() {
    let ruler = Rect::new(100.0, 0.0, 400.0, 20.0);
    assert_eq!(sample_at(ruler, 0.0, 96_000), 0);
    assert_eq!(sample_at(ruler, 9999.0, 96_000), 96_000);
}

// ----------------------------------------------------------------- meter ---

#[test]
fn a_meter_rises_instantly_and_falls_slowly() {
    let mut meter = Meter::new();
    meter.update(1.0, 1.0 / 60.0);
    assert!(
        (meter.level_db - 0.0).abs() < 0.01,
        "full scale read {} dB",
        meter.level_db
    );

    // Silence from here. A peak meter that fell instantly would flicker at
    // every zero crossing and read nothing useful.
    meter.update(0.0, 0.5);
    let expected = -RELEASE_DB_PER_SECOND * 0.5;
    assert!(
        (meter.level_db - expected).abs() < 0.01,
        "after half a second of silence the meter read {} dB, expected {expected}",
        meter.level_db
    );
}

#[test]
fn a_meter_stops_falling_at_the_floor() {
    let mut meter = Meter::new();
    meter.update(1.0, 0.01);
    for _ in 0..1000 {
        meter.update(0.0, 0.1);
    }
    assert_eq!(meter.level_db, METER_FLOOR_DB);
    assert_eq!(meter_fill(meter.level_db), 0.0);
}

#[test]
fn the_peak_hold_stays_put_and_then_lets_go() {
    let mut meter = Meter::new();
    meter.update(1.0, 1.0 / 60.0);
    let held = meter.hold_db;

    // Still held most of the way through the hold time...
    meter.update(0.0, HOLD_SECONDS * 0.5);
    assert!(
        (meter.hold_db - held).abs() < 0.01,
        "the hold let go after {}s",
        HOLD_SECONDS * 0.5
    );
    // ...and released well after it.
    meter.update(0.0, HOLD_SECONDS * 2.0);
    assert!(
        meter.hold_db < held - 1.0,
        "the hold never let go: {} vs {held}",
        meter.hold_db
    );
}

#[test]
fn the_hold_never_falls_below_the_level_it_is_holding_for() {
    let mut meter = Meter::new();
    meter.update(1.0, 0.01);
    for _ in 0..200 {
        meter.update(0.25, 0.05);
        assert!(
            meter.hold_db >= meter.level_db - 0.001,
            "hold {} fell under level {}",
            meter.hold_db,
            meter.level_db
        );
    }
}

#[test]
fn the_meter_fill_spans_the_floor_to_full_scale() {
    assert_eq!(meter_fill(0.0), 1.0);
    assert_eq!(meter_fill(METER_FLOOR_DB), 0.0);
    assert_eq!(meter_fill(METER_FLOOR_DB / 2.0), 0.5);
    // Over full scale is still full — the bar cannot grow past its box.
    assert_eq!(meter_fill(6.0), 1.0);
    assert_eq!(meter_fill(-200.0), 0.0);
}

// ------------------------------------------------------------- read-outs ---

#[test]
fn the_clock_reads_minutes_seconds_and_milliseconds() {
    assert_eq!(format_clock(0.0), "0:00.000");
    assert_eq!(format_clock(3.25), "0:03.250");
    assert_eq!(format_clock(61.5), "1:01.500");
    assert_eq!(format_clock(3600.0), "60:00.000");
    // Before the start of the song is a place the playhead can be asked for.
    assert_eq!(format_clock(-1.0), "0:00.000");
}

#[test]
fn the_position_reads_as_bars_beats_and_ticks() {
    // One-based, like every DAW: the song starts at bar 1, beat 1.
    assert_eq!(format_bars_beats(0.0, 4), "1.1.000");
    assert_eq!(format_bars_beats(1.0, 4), "1.2.000");
    assert_eq!(format_bars_beats(4.0, 4), "2.1.000");
    assert_eq!(format_bars_beats(9.5, 4), "3.2.480");
    assert_eq!(format_bars_beats(-1.0, 4), "1.1.000");
}

// ----------------------------------------------------- commands down ------

/// Stands in for the engine. Records what the bar asked for, so the test can
/// assert on the *command* rather than on a sound.
struct FakeEngine {
    view: TransportView,
    commands: Vec<String>,
}

impl TransportHost for FakeEngine {
    fn view(&mut self) -> TransportView {
        self.view
    }
    fn play(&mut self) {
        self.commands.push("play".to_string());
        self.view.playing = true;
    }
    fn stop(&mut self) {
        self.commands.push("stop".to_string());
        self.view.playing = false;
    }
    fn seek(&mut self, sample: i64) {
        self.commands.push(format!("seek {sample}"));
        self.view.position_sample = sample;
    }
    fn set_looping(&mut self, on: bool) {
        self.commands.push(format!("loop {on}"));
        self.view.looping = on;
    }
}

fn fake() -> FakeEngine {
    FakeEngine {
        view: view(),
        commands: Vec::new(),
    }
}

#[test]
fn a_hit_becomes_exactly_one_command() {
    let mut engine = fake();
    for h in [
        TransportHit::Play,
        TransportHit::Scrub(1234),
        TransportHit::ToggleLoop,
        TransportHit::Stop,
    ] {
        apply(&mut engine, h);
    }
    assert_eq!(engine.commands, ["play", "seek 1234", "loop true", "stop"]);
}

#[test]
fn the_loop_toggle_toggles_rather_than_setting() {
    let mut engine = fake();
    // The bar asks for the opposite of what it last *saw*, so two clicks are
    // a round trip. Writing `true` twice would be a loop button that only
    // ever turns looping on.
    apply(&mut engine, TransportHit::ToggleLoop);
    apply(&mut engine, TransportHit::ToggleLoop);
    assert_eq!(engine.commands, ["loop true", "loop false"]);
}

#[test]
fn play_while_playing_is_not_a_second_play() {
    let mut engine = fake();
    apply(&mut engine, TransportHit::Play);
    apply(&mut engine, TransportHit::Play);
    // Pressing play on a transport that is already rolling should leave it
    // rolling from where it is, not restart it and not stack a second command
    // onto the audio thread every frame the button is held.
    assert_eq!(engine.commands, ["play"]);
}
