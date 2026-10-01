//! **MPE out**: how a note path's slides reach a plugin that hears only MIDI
//! (`docs/note-paths-plan.md` §6). Each note on a member channel of its own,
//! so a pitch bend on that channel moves that note alone — the one way a
//! chord can slide apart through MIDI.
//!
//! `MpeZone` is the bookkeeping, pure and allocation-free, so the audio
//! thread can run it: what it says to send is checked here byte for byte.

use fontelle_host::MpeZone;

fn sent(zone: &mut MpeZone, f: impl FnOnce(&mut MpeZone, &mut dyn FnMut([u8; 3]))) -> Vec<[u8; 3]> {
    let mut out = Vec::new();
    f(zone, &mut |bytes| out.push(bytes));
    out
}

fn bend_bytes(channel: u8, semitones: f32) -> [u8; 3] {
    let raw = ((semitones / 48.0 * 8192.0).round() as i32 + 8192).clamp(0, 16383) as u16;
    [0xE0 | channel, (raw & 0x7F) as u8, (raw >> 7) as u8]
}

/// The member channel a note-on went out on.
fn channel_of_on(bytes: &[[u8; 3]]) -> u8 {
    bytes
        .iter()
        .find(|b| b[0] & 0xF0 == 0x90)
        .map(|b| b[0] & 0x0F)
        .expect("a note-on")
}

#[test]
fn the_first_note_says_the_zone_then_plays_on_a_member_channel() {
    let mut zone = MpeZone::default();
    let out = sent(&mut zone, |z, o| z.note_on(60, 100, o));
    // RPN 6 = 15 on the manager channel: a lower zone of fifteen members.
    assert_eq!(&out[..3], &[[0xB0, 101, 0], [0xB0, 100, 6], [0xB0, 6, 15]]);
    // Each member told its bend range is 48 semitones.
    for member in 1..=15u8 {
        let rpn = [
            [0xB0 | member, 101, 0],
            [0xB0 | member, 100, 0],
            [0xB0 | member, 6, 48],
            [0xB0 | member, 38, 0],
        ];
        assert!(out.windows(4).any(|w| w == rpn), "member {member}");
    }
    let member = channel_of_on(&out);
    assert!((1..=15).contains(&member));
    let tail = &out[out.len() - 2..];
    assert_eq!(
        tail,
        &[bend_bytes(member, 0.0), [0x90 | member, 60, 100]],
        "the member's bend put back to centre, then the note"
    );
    // Said once: the second note goes straight to its channel.
    let second = sent(&mut zone, |z, o| z.note_on(64, 90, o));
    assert_eq!(second.len(), 2);
}

#[test]
fn a_chord_takes_a_channel_a_note_and_each_bends_alone() {
    let mut zone = MpeZone::default();
    let low = channel_of_on(&sent(&mut zone, |z, o| z.note_on(60, 100, o)));
    let high = channel_of_on(&sent(&mut zone, |z, o| z.note_on(64, 100, o)));
    assert_ne!(low, high);

    assert_eq!(
        sent(&mut zone, |z, o| z.note_bend(60, 5.0, o)),
        vec![bend_bytes(low, 5.0)]
    );
    assert_eq!(
        sent(&mut zone, |z, o| z.note_bend(64, -2.0, o)),
        vec![bend_bytes(high, -2.0)]
    );
    assert_eq!(
        sent(&mut zone, |z, o| z.note_off(60, o)),
        vec![[0x80 | low, 60, 0]]
    );
}

#[test]
fn a_bend_for_a_key_not_sounding_sends_nothing() {
    let mut zone = MpeZone::default();
    sent(&mut zone, |z, o| z.note_on(60, 100, o));
    assert!(sent(&mut zone, |z, o| z.note_bend(61, 3.0, o)).is_empty());
    assert!(sent(&mut zone, |z, o| z.note_off(61, o)).is_empty());
}

#[test]
fn a_new_note_takes_the_channel_quiet_longest() {
    // So the release of the note that just ended still rings on its own
    // channel, unbent by the next note's slide.
    let mut zone = MpeZone::default();
    let first = channel_of_on(&sent(&mut zone, |z, o| z.note_on(60, 100, o)));
    sent(&mut zone, |z, o| z.note_off(60, o));
    let second = channel_of_on(&sent(&mut zone, |z, o| z.note_on(62, 100, o)));
    assert_ne!(first, second);
}

#[test]
fn a_bend_past_the_range_stops_at_its_end() {
    let mut zone = MpeZone::default();
    let member = channel_of_on(&sent(&mut zone, |z, o| z.note_on(60, 100, o)));
    assert_eq!(
        sent(&mut zone, |z, o| z.note_bend(60, 60.0, o)),
        vec![[0xE0 | member, 0x7F, 0x7F]]
    );
}

#[test]
fn a_reset_forgets_the_notes_and_says_the_zone_again() {
    let mut zone = MpeZone::default();
    sent(&mut zone, |z, o| z.note_on(60, 100, o));
    zone.reset();
    assert!(sent(&mut zone, |z, o| z.note_bend(60, 3.0, o)).is_empty());
    let again = sent(&mut zone, |z, o| z.note_on(60, 100, o));
    assert_eq!(
        &again[..3],
        &[[0xB0, 101, 0], [0xB0, 100, 6], [0xB0, 6, 15]]
    );
}
