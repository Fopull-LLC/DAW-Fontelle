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
    HOLD_SECONDS, METER_FLOOR_DB, Meter, RELEASE_DB_PER_SECOND, TransportAction, TransportHit,
    TransportHost, TransportView, action, apply, format_bars_beats, format_clock, hit, meter_fill,
    playhead_x, sample_at, transport_bar_layout,
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
        armed: false,
        metronome: false,
    }
}

fn bar() -> Rect {
    Rect::new(8.0, 8.0, 1264.0, 36.0)
}

// ---------------------------------------------------------------- layout ---

#[test]
fn every_piece_of_the_bar_is_inside_it_and_none_of_them_overlap() {
    let l = transport_bar_layout(bar(), &Theme::dark_default().metrics);
    let pieces = [
        l.play,
        l.stop,
        l.loop_toggle,
        l.readout,
        l.help,
        l.ruler,
        l.meter,
    ];

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
fn a_meter_lets_go_of_a_hit_within_a_second_of_silence() {
    // *"other times it plays then leaves it hanging too long when nothings
    // on anymore."* The bar's meter is read against the master strip's, which
    // draws the peak since the last frame and nothing more — so the bar's
    // ballistics have to be quick enough that the two agree about when the
    // song went quiet. Full scale to the floor, bar and hold marker both,
    // inside a second.
    let mut meter = Meter::new();
    meter.update(1.0, 1.0 / 60.0);
    for _ in 0..60 {
        meter.update(0.0, 1.0 / 60.0);
    }
    assert_eq!(
        meter.level_db, METER_FLOOR_DB,
        "a second after the last sound the bar was still at {} dB",
        meter.level_db
    );
    assert_eq!(
        meter.hold_db, METER_FLOOR_DB,
        "a second after the last sound the hold marker was still at {} dB",
        meter.hold_db
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
    // Play seeks to the mark and rolls; a scrub moves the mark; stop rewinds.
    // See `tests/marker.rs` for why each of those is two calls rather than one.
    let mut marker = 0;
    for h in [
        TransportHit::Play,
        TransportHit::Scrub(1234),
        TransportHit::ToggleLoop,
        TransportHit::Stop,
    ] {
        let what = action(h, &engine.view()).expect("every hit here is the engine's");
        marker = apply(&mut engine, what, marker);
    }
    assert_eq!(
        engine.commands,
        ["seek 0", "play", "seek 1234", "loop true", "stop", "seek 0"]
    );
    assert_eq!(marker, 0, "the stop button brought the mark back with it");
}

#[test]
fn the_loop_toggle_toggles_rather_than_setting() {
    let mut engine = fake();
    // The bar asks for the opposite of what it last *saw*, so two clicks are
    // a round trip. Writing `true` twice would be a loop button that only
    // ever turns looping on.
    let what = action(TransportHit::ToggleLoop, &engine.view()).expect("an engine control");
    apply(&mut engine, what, 0);
    let what = action(TransportHit::ToggleLoop, &engine.view()).expect("an engine control");
    apply(&mut engine, what, 0);
    assert_eq!(engine.commands, ["loop true", "loop false"]);
}

#[test]
fn play_while_playing_is_a_pause_back_to_the_mark() {
    let mut engine = fake();
    let marker = 1234;
    let what = action(TransportHit::Play, &engine.view()).expect("an engine control");
    apply(&mut engine, what, marker);
    engine.view.position_sample = 90_000;

    // The second press is a pause, not a second play: it never restarts the
    // transport and never stacks a play onto the audio thread every frame the
    // button is held. Where it comes back to is the mark — see `marker.rs`.
    let what = action(TransportHit::Play, &engine.view()).expect("an engine control");
    assert_eq!(what, TransportAction::Pause);
    apply(&mut engine, what, marker);
    assert_eq!(engine.commands, ["seek 1234", "play", "stop", "seek 1234"]);
}

// -------------------------------------------------------- song and clip ---
//
// *"there should also be a way to swap between clip and song mode currently
// its always on song so you cant ONLY focus one instrument."* The chip lives
// on the transport bar beside the signature, because it changes what pressing
// play does.

#[test]
fn the_bar_has_a_mode_chip_and_it_is_a_control() {
    let l = transport_bar_layout(bar(), &Theme::dark_default().metrics);
    assert!(
        !l.mode.is_empty(),
        "nowhere to switch between song and clip"
    );
    assert_eq!(l.mode.intersection(&l.bar), l.mode, "inside the bar");
    for other in [
        l.play,
        l.stop,
        l.loop_toggle,
        l.record,
        l.metronome,
        l.readout,
        l.tempo,
        l.signature,
        l.ruler,
        l.meter,
    ] {
        assert!(
            !l.mode.intersects(&other),
            "the mode chip overlaps {other:?}"
        );
    }
    let v = view();
    let hit_at = hit(
        &l,
        &v,
        l.mode.x + l.mode.width / 2.0,
        l.mode.y + l.mode.height / 2.0,
    );
    assert_eq!(hit_at, Some(TransportHit::Mode));
    assert!(
        TransportHit::Mode.tip().is_some(),
        "a chip with no explanation"
    );
}

#[test]
fn the_mode_chip_is_the_documents_business_not_the_engines() {
    // Like the tempo: `action` has nothing to say, and the window asks the
    // studio instead.
    assert_eq!(action(TransportHit::Mode, &view()), None);
}

#[test]
fn play_mode_steps_between_its_two_values_and_says_which_it_is() {
    use fontelle_ui::document::PlayMode;
    assert_eq!(PlayMode::default(), PlayMode::Song);
    assert_eq!(PlayMode::Song.next(), PlayMode::Clip);
    assert_eq!(PlayMode::Clip.next(), PlayMode::Song);
    assert_ne!(PlayMode::Song.label(), PlayMode::Clip.label());
    assert!(
        PlayMode::Song.label().len() <= 5,
        "a chip's word, not a sentence"
    );
}

// ---------------------------------------------------------- the help button ---

#[test]
fn the_bar_has_a_help_button_beside_the_tempo_cluster_and_it_is_a_control() {
    // *"a new small ? icon next to the tempo indicator and pressing that
    // will pop up the same keybinds menu."* A small square after the three
    // document boxes, before the ruler — with them rather than at the far
    // end, because it explains the controls it sits next to.
    let l = transport_bar_layout(bar(), &Theme::dark_default().metrics);
    assert!(
        !l.help.is_empty(),
        "a wide bar has room for the help button"
    );
    assert!(
        l.help.x >= l.tempo.right(),
        "the help button sits after the tempo box"
    );
    assert!(l.ruler.x >= l.help.right(), "and before the ruler");
    assert!(
        l.help.width <= l.play.width + 0.01,
        "small: no wider than a transport button"
    );
    let (x, y) = (
        l.help.x + l.help.width / 2.0,
        l.help.y + l.help.height / 2.0,
    );
    assert_eq!(hit(&l, &view(), x, y), Some(TransportHit::Help));
    assert_eq!(
        action(TransportHit::Help, &view()),
        None,
        "the help page is the window's to open, not the engine's"
    );
    // The tip names no key of its own: the key is the keymap's to say, since
    // the page can change it — `action()` is what the window appends.
    use fontelle_ui::canvas::Action;
    assert_eq!(TransportHit::Help.action(), Some(Action::Help));
    assert_eq!(TransportHit::Play.action(), Some(Action::Play));
    assert_eq!(
        TransportHit::ToggleMetronome.action(),
        Some(Action::Metronome)
    );
    assert_eq!(TransportHit::Tempo.action(), None);
    for what in [
        TransportHit::Play,
        TransportHit::Stop,
        TransportHit::ToggleMetronome,
        TransportHit::Mode,
        TransportHit::Help,
    ] {
        let tip = what.tip().unwrap_or("");
        assert!(
            !tip.contains("F1") && !tip.contains("Ctrl") && !tip.contains("Space"),
            "{what:?}'s tip {tip:?} hard-codes a key the page can change"
        );
    }
}

#[test]
fn a_narrow_bar_gives_the_help_button_up_before_the_read_outs() {
    // F1 always works, so the button is the least essential thing on the bar
    // and the first to go — before the mode chip, and long before the ruler.
    let m = Theme::dark_default().metrics;
    for width in [520.0, 560.0, 640.0, 800.0, 1264.0] {
        let l = transport_bar_layout(Rect::new(8.0, 8.0, width, 36.0), &m);
        assert!(
            l.ruler.width >= 80.0,
            "at {width} the ruler is {} wide",
            l.ruler.width
        );
        if l.mode.is_empty() {
            assert!(
                l.help.is_empty(),
                "at {width} the help button survived the mode chip"
            );
        }
    }
}

#[test]
fn a_narrow_bar_drops_the_mode_chip_rather_than_squeezing_the_ruler_away() {
    // The ruler is the playhead and the scrub, and it is the one thing on
    // this bar there is no other way to reach. Adding a box ahead of it took
    // it to nothing at 640 pixels — a width the window opens at — and the
    // playhead then drew at the same pixel wherever the song was. So a box
    // that will not fit is left out, the same rule the roll's toolbar
    // follows.
    let m = Theme::dark_default().metrics;
    for width in [520.0, 560.0, 640.0, 800.0, 1264.0] {
        let l = transport_bar_layout(Rect::new(8.0, 8.0, width, 36.0), &m);
        assert!(
            l.ruler.width >= 80.0,
            "at {width} the ruler is {} wide",
            l.ruler.width
        );
        assert!(
            !l.ruler.intersects(&l.mode),
            "at {width} the mode chip is over the ruler"
        );
    }
    // And where there is room it is there.
    let wide = transport_bar_layout(Rect::new(0.0, 0.0, 1264.0, 36.0), &m);
    assert!(!wide.mode.is_empty(), "a wide bar has room for the chip");
}

// ------------------------------------------------ sharing (collab-plan §10.1) ---

/// F46. *Share this song* is a button at the bar's right end, beside the
/// master meter — kept whatever the bar's width, like the meter, because it
/// is the only way into a session from inside the studio.
#[test]
fn the_bar_has_a_share_button_beside_the_meter() {
    let m = Theme::dark_default().metrics;
    for width in [1264.0, 640.0] {
        let l = transport_bar_layout(Rect::new(8.0, 8.0, width, 36.0), &m);
        assert!(!l.share.is_empty(), "no share button at {width}");
        assert_eq!(l.share.intersection(&l.bar), l.share, "inside the bar");
        assert!(l.share.right() <= l.meter.x, "left of the meter");
        assert!(l.meter.x - l.share.right() <= m.panel_padding, "beside it");
        for other in [l.ruler, l.help, l.mode, l.readout] {
            assert!(!l.share.intersects(&other), "overlaps {other:?}");
        }
        let (x, y) = (
            l.share.x + l.share.width / 2.0,
            l.share.y + l.share.height / 2.0,
        );
        assert_eq!(hit(&l, &view(), x, y), Some(TransportHit::Share));
    }
    assert!(TransportHit::Share.tip().is_some());
    // The window's business: it opens a panel, and the engine hears nothing.
    assert_eq!(action(TransportHit::Share, &view()), None);
}
