//! Record-arm and the metronome, from the transport bar's side.
//!
//! The last two pieces of item 9 of `docs/first-usable-plan.md`, and they go
//! together: playing a part in time to one you have not written yet needs
//! something to play in time *to*.
//!
//! # Arming is not a transport state
//!
//! `TransportState` has three values and none of them is *"stopped, but the
//! next play records"*. Arming is a decision you make **before** you press
//! play; modelling it as a fourth state would mean every `is_processing()`
//! check in the engine had to learn about it. So the bar has an `armed` flag
//! of its own, and `play` is what turns it into `Recording`.

use fontelle_ui::layout::Rect;
use fontelle_ui::theme::Theme;
use fontelle_ui::transport::{
    TransportAction, TransportHit, TransportHost, TransportView, action, apply, hit,
    start_counted_take, transport_bar_layout,
};

const RATE: f64 = 48_000.0;

fn view() -> TransportView {
    TransportView {
        available: true,
        length_samples: (RATE * 8.0) as i64,
        sample_rate: RATE,
        ..TransportView::unavailable()
    }
}

fn bar() -> Rect {
    Rect::new(8.0, 8.0, 1264.0, 36.0)
}

#[derive(Default)]
struct Fake {
    view: TransportView,
    commands: Vec<String>,
}

impl TransportHost for Fake {
    fn view(&mut self) -> TransportView {
        self.view
    }
    fn play(&mut self) {
        // What `EngineHost` does: armed means the tape rolls with the
        // transport.
        self.commands
            .push(if self.view.armed { "record" } else { "play" }.to_string());
    }
    fn stop(&mut self) {
        self.commands.push("stop".to_string());
    }
    fn seek(&mut self, sample: i64) {
        self.commands.push(format!("seek {sample}"));
    }
    fn set_looping(&mut self, on: bool) {
        self.commands.push(format!("loop {on}"));
    }
    fn set_armed(&mut self, on: bool) {
        self.view.armed = on;
        self.commands.push(format!("arm {on}"));
    }
    fn set_metronome(&mut self, on: bool) {
        self.view.metronome = on;
        self.commands.push(format!("click {on}"));
    }
    fn count_in(&mut self, frames: i64) {
        self.commands.push(format!("count {frames}"));
    }
}

fn fake() -> Fake {
    Fake {
        view: view(),
        commands: Vec::new(),
    }
}

// ---------------------------------------------------------------- layout ---

#[test]
fn the_bar_has_a_record_button_and_a_metronome_and_nothing_overlaps() {
    let l = transport_bar_layout(bar(), &Theme::dark_default().metrics);
    let pieces = [
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
    ];
    for (i, a) in pieces.iter().enumerate() {
        assert!(!a.is_empty(), "piece {i} has no room");
        for (j, b) in pieces.iter().enumerate().skip(i + 1) {
            assert!(!a.intersects(b), "pieces {i} and {j} overlap: {a:?} {b:?}");
        }
    }
}

#[test]
fn the_two_settings_sit_with_the_transport_they_belong_to() {
    // Arm and the click are the things you set *before* you press play, so
    // they read with the transport rather than after the position.
    let l = transport_bar_layout(bar(), &Theme::dark_default().metrics);
    assert!(l.record.x >= l.loop_toggle.right());
    assert!(l.metronome.x >= l.record.right());
    assert!(l.readout.x >= l.metronome.right());
}

#[test]
fn each_button_reports_itself() {
    let l = transport_bar_layout(bar(), &Theme::dark_default().metrics);
    let v = view();
    let mid = |r: Rect| (r.x + r.width / 2.0, r.y + r.height / 2.0);

    let (x, y) = mid(l.record);
    assert_eq!(hit(&l, &v, x, y), Some(TransportHit::ToggleRecord));
    let (x, y) = mid(l.metronome);
    assert_eq!(hit(&l, &v, x, y), Some(TransportHit::ToggleMetronome));
}

// ------------------------------------------------------------- behaviour ---

#[test]
fn arming_toggles_rather_than_setting() {
    // The same reason the loop button does: it asks for the opposite of what
    // it last *saw*, so two presses are a round trip.
    let mut engine = fake();
    for _ in 0..2 {
        let what =
            action(TransportHit::ToggleRecord, &engine.view()).expect("arming is the engine's");
        apply(&mut engine, what, 0);
    }
    assert_eq!(engine.commands, ["arm true", "arm false"]);
}

#[test]
fn arming_starts_nothing_on_its_own() {
    // A record button that begins recording the moment it is pressed records
    // the silence before you were ready.
    let mut engine = fake();
    let what = action(TransportHit::ToggleRecord, &engine.view()).unwrap();
    apply(&mut engine, what, 0);
    assert_eq!(engine.commands, ["arm true"], "no play, no seek");
}

#[test]
fn play_while_armed_records() {
    let mut engine = fake();
    let what = action(TransportHit::ToggleRecord, &engine.view()).unwrap();
    apply(&mut engine, what, 0);
    let what = action(TransportHit::Play, &engine.view()).unwrap();
    apply(&mut engine, what, 0);
    assert_eq!(engine.commands, ["arm true", "seek 0", "record"]);
}

#[test]
fn play_while_not_armed_plays() {
    let mut engine = fake();
    let what = action(TransportHit::Play, &engine.view()).unwrap();
    apply(&mut engine, what, 0);
    assert_eq!(engine.commands, ["seek 0", "play"]);
}

#[test]
fn the_metronome_toggles_and_starts_nothing() {
    let mut engine = fake();
    for _ in 0..2 {
        let what = action(TransportHit::ToggleMetronome, &engine.view()).unwrap();
        apply(&mut engine, what, 0);
    }
    assert_eq!(engine.commands, ["click true", "click false"]);
}

#[test]
fn a_window_with_no_engine_behind_it_arms_nothing() {
    let l = transport_bar_layout(bar(), &Theme::dark_default().metrics);
    let dead = TransportView::unavailable();
    let x = l.record.x + l.record.width / 2.0;
    let y = l.record.y + l.record.height / 2.0;
    assert_eq!(hit(&l, &dead, x, y), None);
}

#[test]
fn arming_and_the_click_are_both_the_engines_business() {
    // Unlike the tempo and the signature, which are the document's — see
    // `tests/tempo.rs`. A click is a session setting and arming is a transport
    // one; neither is saved with the piece.
    let v = view();
    assert_eq!(
        action(TransportHit::ToggleRecord, &v),
        Some(TransportAction::SetArmed(true))
    );
    assert_eq!(
        action(TransportHit::ToggleMetronome, &v),
        Some(TransportAction::SetMetronome(true))
    );
}

// ------------------------------------------------------------- count-in ---
//
// > *"instead of playing 4 bars before or whatever just put the playhead on
// > the same spot frozen and count in, then play it from there ... thats
// > causing it so when you are recording past the first section that all of
// > your recordings will be offset by like a bar."*
//
// The count-in used to **move the marker** a bar back and roll from there.
// The marker is where play returns to, so every take after the first counted
// in from a bar earlier than the last — and landed there.

const BEAT: i64 = 24_000;

#[test]
fn a_counted_take_starts_from_the_marker_and_leaves_it_where_it_was() {
    let mut host = fake();
    host.view.armed = true;
    let marker = BEAT * 16;
    let returned = start_counted_take(&mut host, marker, BEAT * 4);
    assert_eq!(returned, marker, "the count-in moved the marker");
    assert_eq!(
        host.commands,
        vec![
            format!("count {}", BEAT * 4),
            format!("seek {marker}"),
            "record".to_string(),
        ],
        "count armed first, then the seek to the marker itself, then roll"
    );
}

#[test]
fn two_takes_in_a_row_count_in_from_the_same_place() {
    let mut host = fake();
    host.view.armed = true;
    let marker = BEAT * 16;
    let first = start_counted_take(&mut host, marker, BEAT * 4);
    host.commands.clear();
    let second = start_counted_take(&mut host, first, BEAT * 4);
    assert_eq!(second, marker);
    assert!(host.commands.contains(&format!("seek {marker}")));
}

#[test]
fn a_counted_take_does_not_switch_the_metronome_on() {
    // The count clicks whatever the switch says; the switch is the person's,
    // and a take that turned it on for good is a setting that changed itself.
    let mut host = fake();
    host.view.armed = true;
    start_counted_take(&mut host, 0, BEAT * 4);
    assert!(!host.commands.iter().any(|c| c.starts_with("click")));
}
