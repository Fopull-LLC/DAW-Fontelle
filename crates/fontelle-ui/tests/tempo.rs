//! The tempo and time-signature boxes on the transport bar.
//!
//! Reported from using the window: there was no way to change the tempo at
//! all. `SetNumber(NumberTarget::Tempo)` has existed since Phase 1 item 3 and
//! nothing on screen could reach it — the same shape as the snap control and
//! the arrangement's missing toolbar before them.
//!
//! Two things make this different from the rest of the bar, and both are why
//! it gets its own file:
//!
//! - **The tempo is document state, not engine state.** Every other control on
//!   the bar is one atomic store into the transport; this one is a `Command`
//!   that has to be undoable and has to be saved. So [`action`] cannot turn it
//!   into a [`TransportAction`], and says so by returning `None`.
//! - **It is a drag, not a click**, which means the value has to be arithmetic
//!   on where the gesture *started* rather than on where the pointer is —
//!   exactly like the instrument editor's knobs, and for the same reason.

use fontelle_ui::layout::Rect;
use fontelle_ui::theme::Theme;
use fontelle_ui::transport::{
    MAX_BEATS_PER_BAR, MAX_TEMPO, MIN_BEATS_PER_BAR, MIN_TEMPO, TransportHit, TransportView,
    action, cycle_beats_per_bar, format_signature, format_tempo, hit, nudge_tempo,
    step_beats_per_bar, tempo_at, transport_bar_layout,
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
fn the_bar_has_a_tempo_box_and_a_signature_box_and_neither_overlaps_anything() {
    let l = transport_bar_layout(bar(), &Theme::dark_default().metrics);
    let pieces = [
        l.play,
        l.stop,
        l.loop_toggle,
        l.readout,
        l.tempo,
        l.signature,
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
fn the_tempo_reads_next_to_the_position_because_they_answer_the_same_question() {
    // "Where am I in the song" and "how fast is it going" are read together,
    // and a tempo box at the far end of the bar is one you have to go and find.
    let l = transport_bar_layout(bar(), &Theme::dark_default().metrics);
    assert!(
        l.tempo.x >= l.readout.right(),
        "the tempo sits after the read-out"
    );
    assert!(
        l.signature.x >= l.tempo.right(),
        "and the signature after the tempo"
    );
    assert!(
        l.ruler.x >= l.signature.right(),
        "and the ruler takes what is left"
    );
}

#[test]
fn a_bar_too_narrow_for_the_boxes_yields_empty_rects_never_negative_ones() {
    for width in [0.0, 1.0, 40.0, 200.0, 400.0] {
        let l = transport_bar_layout(
            Rect::new(0.0, 0.0, width, 36.0),
            &Theme::dark_default().metrics,
        );
        for (name, r) in [("tempo", l.tempo), ("signature", l.signature)] {
            assert!(
                r.width >= 0.0 && r.height >= 0.0,
                "{name} went negative at width {width}: {r:?}"
            );
        }
    }
}

// ------------------------------------------------------------ hit-testing ---

#[test]
fn clicking_the_tempo_box_reports_the_tempo_and_not_a_seek() {
    let l = transport_bar_layout(bar(), &Theme::dark_default().metrics);
    let view = view();

    let centre = (
        l.tempo.x + l.tempo.width / 2.0,
        l.tempo.y + l.tempo.height / 2.0,
    );
    assert_eq!(
        hit(&l, &view, centre.0, centre.1),
        Some(TransportHit::Tempo)
    );

    let centre = (
        l.signature.x + l.signature.width / 2.0,
        l.signature.y + l.signature.height / 2.0,
    );
    assert_eq!(
        hit(&l, &view, centre.0, centre.1),
        Some(TransportHit::Signature)
    );
}

#[test]
fn the_tempo_is_not_the_engines_business_so_it_yields_no_transport_action() {
    // Every other hit on the bar is one atomic store into the transport. This
    // one is a command against the document, which this crate may not reach —
    // so the honest answer is that there is no transport action to take.
    let view = view();
    assert_eq!(action(TransportHit::Tempo, &view), None);
    assert_eq!(action(TransportHit::Signature, &view), None);
    assert!(
        action(TransportHit::Play, &view).is_some(),
        "the engine's own controls still answer"
    );
}

// ---------------------------------------------------------- the arithmetic ---

#[test]
fn dragging_up_speeds_the_song_up() {
    // Up is more, which is the way every tempo box and every knob works and
    // the opposite of the screen's y axis.
    let faster = tempo_at(120.0, -40.0, false);
    let slower = tempo_at(120.0, 40.0, false);
    assert!(faster > 120.0, "dragging up gave {faster}");
    assert!(slower < 120.0, "dragging down gave {slower}");
    assert!(
        (faster - 120.0 - (120.0 - slower)).abs() < 1e-9,
        "the two directions travel the same distance"
    );
}

#[test]
fn a_drag_measures_from_where_it_started_not_from_where_the_pointer_is() {
    // The bug this pins: a knob that reads the pointer's absolute position
    // jumps to a new value the moment you grab it.
    let once = tempo_at(120.0, -20.0, false);
    let twice = tempo_at(once, -20.0, false);
    assert!(
        (twice - tempo_at(120.0, -40.0, false)).abs() < 1e-9,
        "two drags of 20 px are one drag of 40"
    );
}

#[test]
fn a_fine_drag_moves_less_for_the_same_travel() {
    let coarse = tempo_at(120.0, -100.0, false) - 120.0;
    let fine = tempo_at(120.0, -100.0, true) - 120.0;
    assert!(fine > 0.0, "a fine drag still moves");
    assert!(
        fine < coarse / 2.0,
        "a fine drag ({fine}) is meaningfully slower than a coarse one ({coarse})"
    );
}

#[test]
fn the_tempo_cannot_be_dragged_to_zero_or_past_the_top() {
    // And the bottom of the range is a tempo, not a stop: a song at zero beats
    // per minute never plays, and every tick-to-sample conversion in the
    // project divides by it.
    assert_eq!(tempo_at(120.0, 100_000.0, false), MIN_TEMPO);
    assert!(tempo_at(120.0, 100_000.0, false) > 0.0);
    assert_eq!(tempo_at(120.0, -100_000.0, false), MAX_TEMPO);
}

#[test]
fn a_wheel_notch_is_one_beat_per_minute_and_a_fine_one_is_a_tenth() {
    assert_eq!(nudge_tempo(120.0, 1.0, false), 121.0);
    assert_eq!(nudge_tempo(120.0, -1.0, false), 119.0);
    assert_eq!(nudge_tempo(120.0, 1.0, true), 120.1);
    assert_eq!(
        nudge_tempo(MAX_TEMPO, 1.0, false),
        MAX_TEMPO,
        "and it clamps like the drag does"
    );
}

#[test]
fn the_tempo_reads_to_two_decimal_places() {
    // Two, because a fine drag can put the value between them and a box that
    // rounds what it shows is a box that lies about what the song is doing.
    assert_eq!(format_tempo(120.0), "120.00");
    assert_eq!(format_tempo(128.5), "128.50");
    // Not a tie: exactly half a hundredth is `{:.2}`'s round-half-to-even
    // business, and this test is about the two places, not about that rule.
    assert_eq!(format_tempo(97.128), "97.13");
}

#[test]
fn a_dragged_tempo_is_a_value_the_box_can_show_exactly() {
    // Whatever the drag produces has to survive being formatted and read back,
    // or the number on screen is not the number in the document.
    for dy in [-137.0, -13.0, -1.0, 1.0, 13.0, 137.0] {
        for fine in [false, true] {
            let bpm = tempo_at(120.0, dy, fine);
            let shown: f64 = format_tempo(bpm).parse().expect("the box shows a number");
            assert!(
                (shown - bpm).abs() < 1e-9,
                "dy {dy} fine {fine} produced {bpm}, which shows as {shown}"
            );
        }
    }
}

// ------------------------------------------------------- the time signature ---

#[test]
fn clicking_the_signature_steps_the_beats_in_a_bar_and_wraps_round() {
    // Wrapping, not clamping: a click is the only gesture on it that needs no
    // aim, and one that stops at the top is one you cannot get back down from.
    assert_eq!(cycle_beats_per_bar(4), 5);
    assert_eq!(cycle_beats_per_bar(MAX_BEATS_PER_BAR), MIN_BEATS_PER_BAR);
}

#[test]
fn the_wheel_steps_the_signature_and_clamps_at_both_ends() {
    assert_eq!(step_beats_per_bar(4, 1), 5);
    assert_eq!(step_beats_per_bar(4, -1), 3);
    assert_eq!(step_beats_per_bar(MIN_BEATS_PER_BAR, -1), MIN_BEATS_PER_BAR);
    assert_eq!(step_beats_per_bar(MAX_BEATS_PER_BAR, 1), MAX_BEATS_PER_BAR);
}

#[test]
fn the_signature_reads_over_a_quarter_note_because_that_is_what_it_is() {
    // The denominator is not settable and the box does not pretend it is:
    // `PPQN` is ticks per *quarter*, so a denominator other than 4 is a change
    // to what a tick means everywhere, not a control.
    assert_eq!(format_signature(4), "4/4");
    assert_eq!(format_signature(3), "3/4");
    assert_eq!(format_signature(7), "7/4");
}
