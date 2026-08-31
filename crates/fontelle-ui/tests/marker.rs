//! The time marker, and what the transport's buttons mean against it.
//!
//! Reported from using the window: *"the time marker isn't very useful. It
//! should behave more like FL Studio. The last selected time location is your
//! time marker, which is where the playhead will play from, and pressing space
//! to stop brings the playhead back to that position; pressing the stop square
//! brings you to the front of the song."*
//!
//! That is three rules, and none of them was true before:
//!
//! 1. Clicking a ruler — either ruler — **marks** a place, and play starts
//!    there rather than from wherever the playhead was left.
//! 2. Stopping returns to the mark. It is a pause with a memory, not a stop.
//! 3. The stop button is the one that goes back to the front, and it takes the
//!    mark with it — otherwise the next play would jump forwards again.
//!
//! All of it is decided by pure functions, so a window is not needed to know
//! whether the rule holds.

use fontelle_ui::layout::Rect;
use fontelle_ui::transport::{
    TransportAction, TransportHit, TransportHost, TransportView, action, apply, sample_at,
};

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
        view: TransportView {
            available: true,
            length_samples: 480_000,
            sample_rate: 48_000.0,
            ..TransportView::unavailable()
        },
        commands: Vec::new(),
    }
}

#[test]
fn clicking_the_ruler_marks_a_place_and_goes_there() {
    let mut engine = fake();
    let what = action(TransportHit::Scrub(96_000), &engine.view());
    let marker = apply(&mut engine, what.expect("an engine control"), 0);
    assert_eq!(marker, 96_000, "the click is the new mark");
    assert_eq!(engine.commands, ["seek 96000"]);
}

#[test]
fn play_starts_from_the_mark_rather_than_from_wherever_the_playhead_is() {
    let mut engine = fake();
    // Marked at bar 5, then the playhead wandered — a previous take ran on and
    // stopped somewhere else.
    engine.view.position_sample = 400_000;
    let marker = 96_000;

    let what = action(TransportHit::Play, &engine.view());
    let marker = apply(&mut engine, what.expect("an engine control"), marker);
    assert_eq!(marker, 96_000, "playing does not move the mark");
    assert_eq!(
        engine.commands,
        ["seek 96000", "play"],
        "it goes back to the mark first — that is what a mark is for"
    );
}

#[test]
fn stopping_returns_the_playhead_to_the_mark() {
    let mut engine = fake();
    let marker = 96_000;
    let what = action(TransportHit::Play, &engine.view());
    apply(&mut engine, what.expect("an engine control"), marker);
    engine.commands.clear();
    engine.view.position_sample = 300_000;

    // The same button, or the space bar: while it is rolling, it pauses.
    assert_eq!(
        action(TransportHit::Play, &engine.view()),
        Some(TransportAction::Pause)
    );
    let marker = apply(&mut engine, TransportAction::Pause, marker);
    assert_eq!(engine.commands, ["stop", "seek 96000"]);
    assert_eq!(marker, 96_000, "and the mark stays where it was put");
}

#[test]
fn the_stop_button_goes_to_the_front_of_the_song_and_takes_the_mark_with_it() {
    let mut engine = fake();
    let marker = 96_000;
    assert_eq!(
        action(TransportHit::Stop, &engine.view()),
        Some(TransportAction::Rewind)
    );
    let marker = apply(&mut engine, TransportAction::Rewind, marker);
    assert_eq!(engine.commands, ["stop", "seek 0"]);
    assert_eq!(
        marker, 0,
        "the mark comes back to the front too — a stop button that returns the \
         playhead and then plays from bar 5 again is a stop button nobody can use"
    );
}

#[test]
fn the_loop_toggle_still_toggles_and_leaves_the_mark_alone() {
    let mut engine = fake();
    let what = action(TransportHit::ToggleLoop, &engine.view());
    let marker = apply(&mut engine, what.expect("an engine control"), 96_000);
    assert_eq!(engine.commands, ["loop true"]);
    assert_eq!(marker, 96_000);

    engine.view.looping = true;
    let what = action(TransportHit::ToggleLoop, &engine.view());
    apply(&mut engine, what.expect("an engine control"), marker);
    assert_eq!(engine.commands, ["loop true", "loop false"]);
}

#[test]
fn a_mark_can_never_be_before_the_start_of_the_song() {
    let mut engine = fake();
    let marker = apply(&mut engine, TransportAction::Mark(-5_000), 0);
    assert_eq!(marker, 0);
    assert_eq!(engine.commands, ["seek 0"]);
}

#[test]
fn dragging_the_ruler_reaches_both_of_its_ends_exactly() {
    // The complaint this is for: *"I cannot ever get it to just drag to the
    // very start."* A drag is clamped rather than ignored, so the pointer
    // leaving the ruler on either side lands on the end it left by.
    let ruler = Rect::new(200.0, 4.0, 400.0, 20.0);
    let length = 480_000;

    assert_eq!(sample_at(ruler, ruler.x, length), 0);
    assert_eq!(
        sample_at(ruler, ruler.x - 500.0, length),
        0,
        "off the left-hand end is the front of the song, not nothing"
    );
    assert_eq!(sample_at(ruler, ruler.right(), length), length);
    assert_eq!(sample_at(ruler, ruler.right() + 500.0, length), length);
    // And it is monotonic across the middle, which is what makes a drag feel
    // like a drag.
    let mut last = -1;
    for step in 0..=40 {
        let x = ruler.x + ruler.width * step as f32 / 40.0;
        let sample = sample_at(ruler, x, length);
        assert!(sample >= last, "went backwards at x={x}");
        last = sample;
    }
}
