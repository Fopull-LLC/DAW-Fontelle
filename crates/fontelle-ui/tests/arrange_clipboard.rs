//! Copying, cutting, pasting and repeating clips on the arrangement.
//!
//! Reported from using the window: *"we need better arrangement controls like
//! looping, duplicating clips, copying and pasting arrangement clips, cutting,
//! etc. Right now I can only change the length and move them around — like I
//! made this drum loop but I can't repeat it."*
//!
//! `Timeline::duplicate` existed and was reachable only from `Ctrl+B` with the
//! arrangement focused, which is to say: not reachable. Cut, copy and paste
//! did not exist at all — `Ctrl+C` on the arrangement copied *notes*, because
//! the window's clipboard keys went straight to the piano roll without asking
//! which canvas the keyboard belonged to.
//!
//! **The clipboard itself is deliberately not in this canvas.** A `ClipInfo`
//! is a flattened view for drawing — it has no notes and no channel — so a
//! canvas that tried to hold copied clips would be holding something it is not
//! allowed to see (INVARIANT 2). It says *what it wants*, the same shape
//! `RollEdit` and the audition already have, and `fontelle-app` keeps the
//! clips. That is also what makes cut work: paste must still put something
//! down after the clip it copied from has been deleted.

use fontelle_model::Arena;
use fontelle_types::{ClipId, PPQN, Tick};
use fontelle_ui::canvas::{ArrangeEdit, MouseButton, Timeline, TimelineView, timeline_layout};
use fontelle_ui::document::{ClipInfo, ClipKind};
use fontelle_ui::layout::Rect;
use fontelle_ui::theme::Theme;

fn layout() -> fontelle_ui::canvas::TimelineLayout {
    timeline_layout(
        Rect::new(0.0, 0.0, 900.0, 300.0),
        &Theme::dark_default().metrics,
    )
}

fn clips(items: &[(Tick, Tick, usize)]) -> Vec<ClipInfo> {
    let mut arena: Arena<ClipId, ()> = Arena::default();
    items
        .iter()
        .map(|(start, length, lane)| ClipInfo {
            id: arena.insert(()),
            lane: *lane,
            start: *start,
            length: *length,
            name: "Part".to_string(),
            muted: false,
            open: false,
            color: [0x4f, 0x8f, 0xd0, 0xff],
            loop_length: None,
            kind: ClipKind::Notes,
            curve: Vec::new(),
            notes: Vec::new(),
            audio: Default::default(),
        })
        .collect()
}

fn select_first(timeline: &mut Timeline, items: &[ClipInfo]) {
    let l = layout();
    let x = fontelle_ui::canvas::timeline_tick_to_x(&timeline.view, l.grid, items[0].start) + 4.0;
    let y = fontelle_ui::canvas::lane_to_y(&timeline.view, l.grid, items[0].lane)
        + timeline.view.lane_height / 2.0;
    timeline.press(MouseButton::Left, x, y, &l, items, 4);
    timeline.release();
}

#[test]
fn copying_a_clip_asks_the_host_to_keep_it() {
    let mut timeline = Timeline::new(TimelineView::default());
    let items = clips(&[(0, PPQN * 4, 0)]);
    select_first(&mut timeline, &items);

    let edits = timeline.copy();
    assert_eq!(edits, vec![ArrangeEdit::Copy(vec![items[0].id])]);
}

#[test]
fn copying_nothing_asks_for_nothing() {
    let mut timeline = Timeline::new(TimelineView::default());
    assert!(
        timeline.copy().is_empty(),
        "an empty selection is not a copy"
    );
    assert!(timeline.cut().is_empty());
}

/// Cut is copy *and then* remove, in that order — the other way round copies a
/// clip that is already gone.
#[test]
fn cutting_copies_before_it_removes() {
    let mut timeline = Timeline::new(TimelineView::default());
    let items = clips(&[(0, PPQN * 4, 0)]);
    select_first(&mut timeline, &items);

    let edits = timeline.cut();
    assert_eq!(
        edits,
        vec![
            ArrangeEdit::Copy(vec![items[0].id]),
            ArrangeEdit::Remove(vec![items[0].id]),
        ]
    );
    assert!(
        timeline.selection().is_empty(),
        "what was cut is not still selected"
    );
}

#[test]
fn pasting_puts_the_clipboard_down_at_the_tick_it_is_given() {
    let mut timeline = Timeline::new(TimelineView::default());
    let edits = timeline.paste(PPQN * 16, 4);
    assert_eq!(edits, vec![ArrangeEdit::Paste { at: PPQN * 16 }]);
}

/// The arrangement snaps too, and for the same reason the roll does: the tick
/// comes from a playhead or a pointer and neither is ever on a bar line.
#[test]
fn a_paste_is_snapped_to_the_arrangements_own_grid() {
    let mut timeline = Timeline::new(TimelineView {
        snap: fontelle_ui::canvas::SnapDivision::Bar,
        ..TimelineView::default()
    });
    let edits = timeline.paste(PPQN * 5, 4);
    assert_eq!(
        edits,
        vec![ArrangeEdit::Paste { at: PPQN * 4 }],
        "bar snap puts a pasted clip on the bar"
    );
}

#[test]
fn a_paste_before_the_start_of_the_song_lands_at_the_start() {
    let mut timeline = Timeline::new(TimelineView::default());
    assert_eq!(
        timeline.paste(-PPQN * 4, 4),
        vec![ArrangeEdit::Paste { at: 0 }]
    );
}

/// The report's actual sentence: *"I made this drum loop but I can't repeat
/// it."* Repeat is duplicate applied `n` times, each copy landing after the
/// last — one edit per copy so the offsets are unambiguous, and one undo entry
/// because the host breaks the gesture once at the end.
#[test]
fn repeating_a_clip_lays_copies_end_to_end() {
    let mut timeline = Timeline::new(TimelineView::default());
    let items = clips(&[(0, PPQN * 4, 0)]);
    select_first(&mut timeline, &items);

    let edits = timeline.repeat(&items, 3, 4);
    assert_eq!(edits.len(), 3, "three more of it");
    let offsets: Vec<Tick> = edits
        .iter()
        .map(|e| match e {
            ArrangeEdit::Duplicate { tick_offset, .. } => *tick_offset,
            other => panic!("expected a duplicate, got {other:?}"),
        })
        .collect();
    assert_eq!(
        offsets,
        vec![PPQN * 4, PPQN * 8, PPQN * 12],
        "each copy starts where the previous one ends, not all on top of each other"
    );
}

#[test]
fn repeating_nothing_asks_for_nothing() {
    let mut timeline = Timeline::new(TimelineView::default());
    let items = clips(&[(0, PPQN * 4, 0)]);
    assert!(timeline.repeat(&items, 3, 4).is_empty());
}

/// What makes "repeat" repeat rather than pile copies in one spot.
///
/// A duplicate is measured from the selection, so if the selection stays on
/// the original clip then pressing the key twice puts two copies at the *same*
/// offset, on top of each other. The roll already solved this — `AddNotes`
/// hands its ids back and `PianoRoll::notes_inserted` adopts them — and this
/// is the same handshake for clips, which is why `StudioHost::arrange` now
/// returns what it created.
#[test]
fn a_duplicated_clip_becomes_the_selection_so_the_next_one_lands_after_it() {
    let mut timeline = Timeline::new(TimelineView::default());
    // Both clips out of one arena: two `clips()` calls mint colliding ids,
    // and a selection that accidentally matches both is not the thing under
    // test.
    let after = clips(&[(0, PPQN * 4, 0), (PPQN * 4, PPQN * 4, 0)]);
    let (original, copy) = (after[0].clone(), after[1].clone());
    select_first(&mut timeline, std::slice::from_ref(&original));
    assert_eq!(timeline.selection(), &[original.id]);

    // The host made a clip and says so.
    timeline.clips_inserted(vec![copy.id]);
    assert_eq!(
        timeline.selection(),
        &[copy.id],
        "the copy is what is selected now"
    );

    // So the next repeat is measured from the copy, not from the original.
    let edits = timeline.repeat(&after, 1, 4);
    assert_eq!(
        edits,
        vec![ArrangeEdit::Duplicate {
            ids: vec![copy.id],
            tick_offset: PPQN * 4,
        }],
        "one bar on from the copy, which is bar 3 — not bar 2 again"
    );
}

#[test]
fn being_handed_no_new_clips_leaves_the_selection_alone() {
    let mut timeline = Timeline::new(TimelineView::default());
    let items = clips(&[(0, PPQN * 4, 0)]);
    select_first(&mut timeline, &items);

    timeline.clips_inserted(Vec::new());

    assert_eq!(
        timeline.selection(),
        &[items[0].id],
        "a duplicate that created nothing must not clear what was selected"
    );
}
