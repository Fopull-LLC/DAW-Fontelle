//! Strum: a chord's notes spread in time (FL Studio's Strum, Live 12's
//! Strum). Ty, 2026-10-20: *"tools that other daws have that were just
//! missing like strum"*.

mod tool_kit;

use fontelle_model::{StrumDirection, StrumSpec, chords_of, strummed};
use fontelle_types::PPQN;
use tool_kit::{as_left, note, scope};

const BAR: i64 = PPQN * 4;

fn even(direction: StrumDirection) -> StrumSpec {
    StrumSpec {
        time: 60,
        direction,
        tension: 0.0,
        velocity: 0,
        keep_ends: true,
        land: false,
        seed: 1,
    }
}

fn triad(at: i64) -> Vec<fontelle_model::Note> {
    vec![note(at, BAR, 64), note(at, BAR, 60), note(at, BAR, 67)]
}

fn starts_by_key(
    s: &[(fontelle_types::NoteId, fontelle_model::Note)],
    r: &fontelle_model::ToolResult,
) -> Vec<(u8, i64)> {
    let mut v: Vec<(u8, i64)> = s
        .iter()
        .filter_map(|(id, _)| as_left(s, r, *id))
        .map(|n| (n.key, n.start))
        .collect();
    v.sort();
    v
}

#[test]
fn an_upstroke_starts_on_the_lowest_string_and_steps_up() {
    let s = scope(triad(0));
    let r = strummed(&s, even(StrumDirection::Up));
    assert_eq!(starts_by_key(&s, &r), vec![(60, 0), (64, 60), (67, 120)]);
}

#[test]
fn a_downstroke_starts_on_the_highest() {
    let s = scope(triad(0));
    let r = strummed(&s, even(StrumDirection::Down));
    assert_eq!(starts_by_key(&s, &r), vec![(60, 120), (64, 60), (67, 0)]);
}

#[test]
fn the_chord_still_lets_go_together_when_ends_are_kept() {
    let s = scope(triad(0));
    let r = strummed(&s, even(StrumDirection::Up));
    for (id, _) in &s {
        let n = as_left(&s, &r, *id).unwrap();
        assert_eq!(n.start + n.length, BAR, "{n:?}");
    }
    let mut loose = even(StrumDirection::Up);
    loose.keep_ends = false;
    let r = strummed(&s, loose);
    for (id, _) in &s {
        assert_eq!(as_left(&s, &r, *id).unwrap().length, BAR);
    }
}

#[test]
fn alternate_goes_up_then_down_chord_by_chord() {
    let mut notes = triad(0);
    notes.extend(triad(BAR));
    let s = scope(notes);
    let r = strummed(&s, even(StrumDirection::Alternate));
    let after = r.after(&s);
    let first: Vec<u8> = after
        .iter()
        .filter(|n| n.start < BAR)
        .map(|n| n.key)
        .collect();
    let second: Vec<u8> = after
        .iter()
        .filter(|n| n.start >= BAR)
        .map(|n| n.key)
        .collect();
    assert_eq!(first, vec![60, 64, 67], "up");
    assert_eq!(second, vec![67, 64, 60], "then down");
}

#[test]
fn a_single_note_is_not_a_chord_and_is_left_alone() {
    let s = scope(vec![note(0, 240, 60), note(480, 240, 62)]);
    assert!(strummed(&s, even(StrumDirection::Up)).is_empty());
}

#[test]
fn a_chord_played_in_by_hand_is_still_one_chord() {
    let s = scope(vec![note(0, BAR, 60), note(12, BAR, 64), note(25, BAR, 67)]);
    assert_eq!(chords_of(&s).len(), 1);
    let r = strummed(&s, even(StrumDirection::Up));
    assert_eq!(starts_by_key(&s, &r), vec![(60, 0), (64, 60), (67, 120)]);
}

#[test]
fn chords_are_in_time_order_and_low_to_high() {
    let mut notes = triad(BAR);
    notes.extend(triad(0));
    let s = scope(notes);
    let groups = chords_of(&s);
    assert_eq!(groups.len(), 2);
    let keys: Vec<u8> = groups[0].iter().map(|i| s[*i].1.key).collect();
    assert_eq!(keys, vec![60, 64, 67]);
    assert!(groups[0].iter().all(|i| s[*i].1.start == 0));
}

#[test]
fn tension_speeds_it_up_without_changing_its_length() {
    let s = scope(vec![
        note(0, BAR, 60),
        note(0, BAR, 64),
        note(0, BAR, 67),
        note(0, BAR, 72),
    ]);
    let mut tense = even(StrumDirection::Up);
    tense.tension = 0.8;
    let r = strummed(&s, tense);
    let st: Vec<i64> = starts_by_key(&s, &r).into_iter().map(|(_, t)| t).collect();
    assert_eq!(st[0], 0);
    assert_eq!(st[3], 180, "the whole strum is as long as an even one");
    assert!(st[1] - st[0] > st[3] - st[2], "the gaps shrink: {st:?}");
}

#[test]
fn landing_puts_the_last_string_on_the_beat() {
    let s = scope(triad(BAR));
    let mut land = even(StrumDirection::Up);
    land.land = true;
    let r = strummed(&s, land);
    assert_eq!(
        starts_by_key(&s, &r),
        vec![(60, BAR - 120), (64, BAR - 60), (67, BAR)]
    );
}

#[test]
fn nothing_is_strummed_to_before_the_clip() {
    let s = scope(triad(0));
    let mut land = even(StrumDirection::Up);
    land.land = true;
    let r = strummed(&s, land);
    assert!(r.after(&s).iter().all(|n| n.start >= 0));
}

#[test]
fn velocity_changes_string_by_string_and_stays_in_range() {
    let s = scope(triad(0));
    let mut harder = even(StrumDirection::Up);
    harder.velocity = 15;
    let r = strummed(&s, harder);
    let v: Vec<u8> = r.after(&s).iter().map(|n| n.velocity).collect();
    assert_eq!(v, vec![100, 115, 127]);
}

#[test]
fn random_is_a_strum_of_every_string_and_the_same_for_the_same_seed() {
    let s = scope(triad(0));
    let mut random = even(StrumDirection::Random);
    let a = strummed(&s, random);
    assert_eq!(a, strummed(&s, random));
    let mut st: Vec<i64> = a.after(&s).iter().map(|n| n.start).collect();
    st.sort();
    assert_eq!(st, vec![0, 60, 120]);
    random.seed = 2;
    let _ = strummed(&s, random);
}
