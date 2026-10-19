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
    MAX_TEMPO, MIN_TEMPO, TransportHit, TransportView, action, format_tempo, hit, nudge_tempo,
    parse_tempo, tempo_at, transport_bar_layout,
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
        audio_load: None,
        audio_dropouts: 0,
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

// ---------------------------------------------------------- typing a tempo ---

#[test]
fn a_typed_tempo_is_read_as_a_number_and_rounded_to_what_the_box_can_show() {
    // *"i cant click and type in the field like an input field to input my
    // tempo."* A click on the box opens it for typing; this is what the typed
    // text becomes on Enter.
    assert_eq!(parse_tempo("128"), Some(128.0));
    assert_eq!(parse_tempo("128.5"), Some(128.5));
    assert_eq!(
        parse_tempo(" 90 "),
        Some(90.0),
        "surrounding space is not a typo"
    );
    assert_eq!(
        parse_tempo("120.004"),
        Some(120.0),
        "rounded to the two places the box shows, like a drag is"
    );
    assert_eq!(parse_tempo("174.995"), Some(175.0));
}

#[test]
fn a_typed_tempo_is_kept_inside_the_same_limits_a_drag_has() {
    assert_eq!(parse_tempo("0"), Some(MIN_TEMPO));
    assert_eq!(parse_tempo("5"), Some(MIN_TEMPO));
    assert_eq!(parse_tempo("100000"), Some(MAX_TEMPO));
    assert_eq!(parse_tempo("-40"), Some(MIN_TEMPO));
}

#[test]
fn text_that_is_not_a_tempo_is_refused_rather_than_guessed_at() {
    // Enter on any of these leaves the tempo where it was.
    for text in ["", "   ", "fast", "12O", "1.2.3", "nan", "inf", "-inf"] {
        assert_eq!(parse_tempo(text), None, "{text:?} was read as a tempo");
    }
}

// ------------------------------------------------------- the time signature ---

/// Ty, 2026-10-19: *"signatures NEED to be flexible enough to let you chose
/// whatever top or division number you want so youre not just locked to
/// x/4."* The box drops the common signatures in quarters, halves, eighths
/// and sixteenths — the one in force greyed — and a row to type any other.
#[test]
fn the_signature_menu_offers_the_common_ones_and_any_other_typed() {
    use fontelle_types::Signature;
    use fontelle_ui::transport::{MeterRow, MeterTarget, TYPE_A_SIGNATURE, meter_menu};
    let (entries, rows) = meter_menu(MeterTarget::Song, &Signature::new(4, 4), 9, false);
    assert_eq!(entries.len(), rows.len());
    assert!(entries[0].label.contains("song"), "{}", entries[0].label);
    let labels: Vec<&str> = entries.iter().map(|e| e.label.as_str()).collect();
    for wanted in ["3/4", "4/4", "2/2", "6/8", "7/8", "12/8", "5/16"] {
        assert!(
            labels.contains(&wanted),
            "{wanted} is not offered: {labels:?}"
        );
    }
    let four = labels.iter().position(|l| *l == "4/4").unwrap();
    assert!(!entries[four].enabled, "the one in force is greyed");
    let typed = rows
        .iter()
        .position(|r| *r == MeterRow::Type)
        .expect("a typed row");
    assert_eq!(entries[typed].label, TYPE_A_SIGNATURE);
    let add = rows
        .iter()
        .position(|r| *r == MeterRow::AddAt(9))
        .expect("a change offered at the marker's bar");
    assert!(
        entries[add].label.contains("bar 10"),
        "{}",
        entries[add].label
    );
}

/// Editing a change: the same signatures, and a way to take it away.
#[test]
fn the_signature_menu_edits_a_change() {
    use fontelle_types::Signature;
    use fontelle_ui::transport::{MeterRow, MeterTarget, meter_menu};
    let (entries, rows) = meter_menu(MeterTarget::Change(9), &Signature::new(7, 8), 9, true);
    assert!(entries[0].label.contains("bar 10"), "{}", entries[0].label);
    assert!(rows.contains(&MeterRow::Remove(9)));
    assert!(rows.contains(&MeterRow::Type));
    assert!(!rows.iter().any(|r| matches!(r, MeterRow::AddAt(_))));
    let at = entries.iter().position(|e| e.label == "7/8").unwrap();
    assert!(!entries[at].enabled);
    // A grouped one is said as it is grouped, in the heading.
    let (entries, _) = meter_menu(
        MeterTarget::Change(9),
        &Signature::new(7, 8).with_groups(vec![3, 2, 2]),
        9,
        true,
    );
    assert!(entries[0].label.contains("3+2+2/8"), "{}", entries[0].label);
}

/// At bar 1 there is nothing to add: that is the song's own.
#[test]
fn no_change_is_offered_at_bar_one() {
    use fontelle_ui::transport::{MeterRow, MeterTarget, meter_menu};
    let (_, rows) = meter_menu(
        MeterTarget::Song,
        &fontelle_types::Signature::new(4, 4),
        0,
        false,
    );
    assert!(!rows.iter().any(|r| matches!(r, MeterRow::AddAt(_))));
}

/// The read-out counts bars and beats by the meter: past a change to 7/8,
/// a beat is an eighth.
#[test]
fn the_read_out_counts_by_the_meter() {
    use fontelle_types::{Meter, MeterChange};
    use fontelle_ui::transport::format_bars_beats;
    let meter = Meter::new(4, &[MeterChange::new(1, 7, 8)]);
    assert_eq!(format_bars_beats(0.0, &meter), "1.1.000");
    assert_eq!(format_bars_beats(4.0, &meter), "2.1.000");
    // Half a quarter into bar 2: its second eighth.
    assert_eq!(format_bars_beats(4.5, &meter), "2.2.000");
    assert_eq!(format_bars_beats(4.0 + 3.5, &meter), "3.1.000");
}

/// The signature box is as wide as what it says: "3+2+2/8" is not cut to
/// "3+2+2/", and the boxes after it move along to make room.
#[test]
fn the_signature_box_widens_for_a_long_signature() {
    use fontelle_ui::transport::transport_bar_layout_with;
    let bar = Rect::new(0.0, 0.0, 1400.0, 40.0);
    let metrics = Theme::dark_default().metrics;
    let plain = transport_bar_layout(bar, &metrics);
    let wide = transport_bar_layout_with(bar, &metrics, plain.signature.width + 40.0);
    assert!((wide.signature.width - (plain.signature.width + 40.0)).abs() < 0.5);
    assert!(
        wide.mode.x >= plain.mode.x + 39.5,
        "the next box moves along"
    );
    // Never narrower than it always was.
    let narrow = transport_bar_layout_with(bar, &metrics, 5.0);
    assert_eq!(narrow.signature.width, plain.signature.width);
}
