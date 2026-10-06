//! Slicing (plan §3.8, P4).

use fontelle_analysis::slice::{
    CHOP_FIRST_KEY, DrumClass, NoteSpan, Slice, SliceLayout, classify_drum, layout_slices,
    slice_pitch, slices_at, slices_equal, slices_from_notes, slices_on_grid,
};
use fontelle_analysis::testsignals::{drum_loop, noise};

const SR: u32 = 48_000;

fn sine(seconds: f64, midi: f64) -> Vec<f32> {
    let hz = 440.0 * 2f64.powf((midi - 69.0) / 12.0);
    (0..(seconds * f64::from(SR)) as usize)
        .map(|i| (std::f64::consts::TAU * hz * i as f64 / f64::from(SR)).sin() as f32 * 0.5)
        .collect()
}

#[test]
fn n_markers_make_n_plus_1_slices() {
    for n in 0..8usize {
        let markers: Vec<usize> = (1..=n).map(|i| i * 1000).collect();
        let slices = slices_at(&markers, 10_000);
        assert_eq!(slices.len(), n + 1);
        assert_eq!(slices[0].start, 0);
        assert_eq!(slices.last().unwrap().end, 10_000);
        for pair in slices.windows(2) {
            assert_eq!(pair[0].end, pair[1].start, "slices leave no gap");
        }
    }
    // Unsorted, repeated and out-of-range markers cut once each or not at all.
    let slices = slices_at(&[5000, 0, 2000, 5000, 10_000, 99_999], 10_000);
    assert_eq!(
        slices.iter().map(|s| (s.start, s.end)).collect::<Vec<_>>(),
        vec![(0, 2000), (2000, 5000), (5000, 10_000)]
    );
}

#[test]
fn equal_and_grid_slices_cover_the_buffer() {
    let equal = slices_equal(10_001, 4);
    assert_eq!(equal.len(), 4);
    assert_eq!(equal[0].start, 0);
    assert_eq!(equal[3].end, 10_001);
    let grid = slices_on_grid(48_000, 0, 12_000.5);
    assert_eq!(grid.len(), 4);
    assert_eq!(grid[1].start, 12_001);
    // A grid starting after zero keeps the lead-in as its own slice.
    assert_eq!(slices_on_grid(1000, 100, 400.0).len(), 4);
}

#[test]
fn notes_become_slices_with_their_pitch() {
    let notes = [
        NoteSpan {
            start: 100,
            end: 900,
            midi: 60.0,
        },
        NoteSpan {
            start: 1000,
            end: 1800,
            midi: 64.2,
        },
    ];
    let slices = slices_from_notes(&notes, 2000);
    assert_eq!(slices.len(), 2);
    assert_eq!((slices[0].start, slices[0].end), (100, 1000));
    assert_eq!((slices[1].start, slices[1].end), (1000, 1800));
    assert_eq!(slices[1].pitch, Some(64.2));
}

#[test]
fn chop_puts_slice_i_on_key_48_plus_i() {
    let x = noise(SR, 1.0, 0.1);
    let slices = slices_equal(x.len(), 5);
    let patch = layout_slices(&x, SR, &slices, SliceLayout::Chop);
    assert_eq!(patch.zones.len(), 5);
    for (i, zone) in patch.zones.iter().enumerate() {
        let key = CHOP_FIRST_KEY + i as u8;
        assert_eq!(zone.key_range, (key, key));
        assert_eq!(zone.root_key, key);
        assert_eq!(zone.fine_tune_cents, 0.0);
        assert_eq!((zone.start, zone.end), (slices[i].start, slices[i].end));
    }
    // The replay clip plays them back in order, where they were.
    assert_eq!(patch.replay.len(), 5);
    for (i, note) in patch.replay.iter().enumerate() {
        assert_eq!(note.key, CHOP_FIRST_KEY + i as u8);
        assert_eq!(note.start, slices[i].start);
        assert_eq!(note.length, slices[i].len());
    }
    assert!(patch.unmapped.is_empty());
}

#[test]
fn by_pitch_ranges_tile_the_keyboard_without_gaps() {
    // Four notes, one of them 30 cents sharp, out of order in pitch.
    let pitches = [64.0, 57.0, 72.3, 60.0];
    let mut x = Vec::new();
    for p in pitches {
        x.extend(sine(0.5, p));
    }
    let slices = slices_equal(x.len(), 4);
    for (slice, want) in slices.iter().zip(pitches) {
        let found = slice_pitch(&x[slice.start..slice.end], SR).expect("a sine has a pitch");
        assert!((found - want as f32).abs() < 0.1, "{found} for {want}");
    }
    let patch = layout_slices(&x, SR, &slices, SliceLayout::ByPitch);
    assert_eq!(patch.zones.len(), 4);
    let mut ranges: Vec<(u8, u8)> = patch.zones.iter().map(|z| z.key_range).collect();
    ranges.sort();
    assert_eq!(ranges[0].0, 0);
    assert_eq!(ranges.last().unwrap().1, 127);
    for pair in ranges.windows(2) {
        assert_eq!(pair[0].1 + 1, pair[1].0, "a gap or an overlap: {ranges:?}");
    }
    for zone in &patch.zones {
        assert!(zone.key_range.0 <= zone.root_key && zone.root_key <= zone.key_range.1);
        let want = pitches[zone.slice];
        assert_eq!(zone.root_key, want.round() as u8);
        // Tuned so it plays in tune: 30 cents sharp is tuned 30 down.
        let cents = (want - want.round()) * 100.0;
        assert!(
            (zone.fine_tune_cents + cents as f32).abs() < 10.0,
            "{} cents on a slice {cents} sharp",
            zone.fine_tune_cents
        );
    }
}

#[test]
fn drum_map_puts_a_kick_on_36() {
    let x = drum_loop(SR);
    // The loop's first four eighths: kick (with a hat), hat, snare (with a
    // hat), hat — 0.3 s apart.
    let eighth = (0.3 * f64::from(SR)) as usize;
    let slices: Vec<Slice> = (0..4)
        .map(|i| Slice {
            start: i * eighth,
            end: (i + 1) * eighth,
            pitch: None,
        })
        .collect();
    let classes: Vec<DrumClass> = slices.iter().map(|s| classify_drum(&x, s, SR)).collect();
    assert_eq!(classes[0], DrumClass::Kick);
    assert_eq!(classes[1], DrumClass::Hat);
    assert_eq!(classes[2], DrumClass::Snare);
    let patch = layout_slices(&x, SR, &slices, SliceLayout::DrumMap);
    let key_of = |slice: usize| {
        patch
            .zones
            .iter()
            .find(|z| z.slice == slice)
            .map(|z| z.key_range)
    };
    assert_eq!(key_of(0), Some((36, 36)));
    assert_eq!(key_of(1), Some((42, 42)));
    assert_eq!(key_of(2), Some((38, 38)));
    // A second hat takes the next hat key, not the first one again.
    assert_eq!(key_of(3), Some((44, 44)));
}
