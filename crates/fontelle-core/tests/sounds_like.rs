//! Sound match (`docs/flopsynth-next.md` §5.2): a preset's preview — the
//! shape of 1.5 s of C3 — and the nearest others by it. The Grand Piano's
//! neighbours are keys, not growls.

use fontelle_core::flopsynth::presets::FACTORY;
use fontelle_core::preview::{Preview, nearest, preset_preview, sound_distance};

#[test]
fn a_preview_is_the_shape_of_a_note_and_a_vector_to_compare_by() {
    let grand = FACTORY.iter().find(|r| r.name == "Grand Piano").unwrap();
    let preview = preset_preview(&(grand.build)());
    // The peaks: the note's envelope over the preview's length, 0..1,
    // enough columns to draw.
    assert_eq!(preview.peaks.len(), Preview::COLUMNS);
    assert!(preview.peaks.iter().all(|p| (0.0..=1.0).contains(p)));
    let loudest = preview.peaks.iter().cloned().fold(0.0f32, f32::max);
    assert!((loudest - 1.0).abs() < 1e-3, "normalised to its own peak");
    // A piano starts loud and dies: the last column is well under the first.
    assert!(preview.peaks[Preview::COLUMNS - 1] < preview.peaks[1] * 0.5);
    // The vector's axes are finite, and the same preset is at no distance
    // from itself.
    assert!(preview.vector.scalar.iter().all(|v| v.is_finite()));
    assert!((preview.vector.shape.iter().sum::<f32>() - 1.0).abs() < 1e-3);
    assert_eq!(sound_distance(&preview.vector, &preview.vector), 0.0);
}

#[test]
fn the_grand_pianos_neighbours_are_keys_not_growls() {
    let previews: Vec<Preview> = FACTORY
        .iter()
        .map(|row| preset_preview(&(row.build)()))
        .collect();
    let vectors: Vec<_> = previews.iter().map(|p| p.vector.clone()).collect();
    let grand = FACTORY
        .iter()
        .position(|r| r.name == "Grand Piano")
        .unwrap();
    let near = nearest(&vectors, grand, 5);
    assert_eq!(near.len(), 5);
    assert!(!near.contains(&grand), "not itself");
    let shelves: Vec<&str> = near.iter().map(|i| FACTORY[*i].category.label()).collect();
    let names: Vec<&str> = near.iter().map(|i| FACTORY[*i].name).collect();
    for shelf in &shelves {
        assert!(
            !matches!(
                *shelf,
                "Growls & Screams" | "Bass Music" | "Synth Drums" | "Kits & Hits"
            ),
            "{names:?} on {shelves:?}"
        );
    }
    // The nearest of all is the other piano; the rest are sustained,
    // struck-or-blown things — organs, a sax, a lead — which is what a
    // grand held for six tenths reads as on these axes. Measured: Felt
    // Piano 0.21 away, then Retro Lead, Reed Organ, Drawbar 888, Fifths.
    assert_eq!(
        FACTORY[near[0]].category.label(),
        "Sampled Keys",
        "the nearest is the other piano: {names:?}"
    );
    // And the order is by distance.
    let d: Vec<f32> = near
        .iter()
        .map(|i| sound_distance(&vectors[grand], &vectors[*i]))
        .collect();
    assert!(d.windows(2).all(|w| w[0] <= w[1]), "{d:?}");
}
