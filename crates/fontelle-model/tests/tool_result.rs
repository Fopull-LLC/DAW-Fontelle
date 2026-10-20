//! The one shape every roll tool answers with (`docs/roll-tools-plan.md`
//! §3.1): what changes, what goes, what is new — and what the roll will look
//! like afterwards.

mod tool_kit;

use fontelle_model::{Note, ToolResult};
use tool_kit::{note, scope};

#[test]
fn rewriting_keeps_only_the_notes_that_changed_and_keeps_their_ids() {
    let s = scope(vec![
        note(0, 240, 60),
        note(240, 240, 62),
        note(480, 240, 64),
    ]);
    let mut after: Vec<Note> = s.iter().map(|(_, n)| n.clone()).collect();
    after[1].key = 63;
    let r = ToolResult::rewriting(&s, after);
    assert_eq!(r.rewrite.len(), 1);
    assert_eq!(r.rewrite[0].0, s[1].0, "the same note, by id");
    assert_eq!(r.rewrite[0].1.key, 63);
    assert!(r.remove.is_empty() && r.insert.is_empty());
}

#[test]
fn rewriting_into_the_same_notes_is_nothing() {
    let s = scope(vec![note(0, 240, 60), note(240, 240, 62)]);
    let r = ToolResult::rewriting(&s, s.iter().map(|(_, n)| n.clone()).collect());
    assert!(r.is_empty());
}

#[test]
fn replacing_takes_every_note_away_and_puts_the_new_ones_in() {
    let s = scope(vec![note(0, 960, 60), note(0, 960, 64)]);
    let made = vec![note(0, 240, 60), note(240, 240, 64)];
    let r = ToolResult::replacing(&s, made.clone());
    assert_eq!(r.remove, vec![s[0].0, s[1].0]);
    assert_eq!(r.insert, made);
    assert!(r.rewrite.is_empty());
    assert!(!r.is_empty());
}

#[test]
fn after_is_the_scope_as_the_tool_leaves_it_in_time_order() {
    let s = scope(vec![
        note(480, 240, 64),
        note(0, 240, 60),
        note(240, 240, 62),
    ]);
    let r = ToolResult {
        rewrite: vec![(s[0].0, note(960, 240, 65))],
        remove: vec![s[2].0],
        insert: vec![note(120, 60, 70)],
    };
    let keys: Vec<(i64, u8)> = r.after(&s).iter().map(|n| (n.start, n.key)).collect();
    assert_eq!(keys, vec![(0, 60), (120, 70), (960, 65)]);
}

#[test]
fn touched_is_what_the_roll_dims_rewritten_and_removed() {
    let s = scope(vec![
        note(0, 240, 60),
        note(240, 240, 62),
        note(480, 240, 64),
    ]);
    let r = ToolResult {
        rewrite: vec![(s[0].0, note(0, 120, 60))],
        remove: vec![s[2].0],
        insert: vec![],
    };
    let mut t = r.touched();
    t.sort();
    let mut want = vec![s[0].0, s[2].0];
    want.sort();
    assert_eq!(t, want);
}
