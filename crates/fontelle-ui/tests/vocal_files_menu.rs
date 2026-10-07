//! A vocal track's own rows on its rack menu: bring a song file in, write
//! the track out, freeze or unfreeze it, render the roll's selection.

use fontelle_ui::canvas::{VocalTrackChoice, vocal_track_menu_choice, vocal_track_menu_entries};
use fontelle_ui::document::VocalFileFormat;

#[test]
fn the_rows_say_what_they_do_and_freeze_reads_as_its_state() {
    let rows: Vec<String> = vocal_track_menu_entries(false)
        .into_iter()
        .map(|e| e.label)
        .collect();
    assert_eq!(
        rows,
        vec![
            "Import vocal file (UST, USTX, VSQX, MIDI, MusicXML)\u{2026}",
            "Export MIDI with lyrics\u{2026}",
            "Export USTX\u{2026}",
            "Freeze track",
            "Render selection to clip",
        ]
    );
    let frozen = vocal_track_menu_entries(true);
    assert_eq!(frozen[3].label, "Unfreeze track");
    // A frozen track's part is not sung: there is nothing to render.
    assert!(!frozen[4].enabled);
    // The first is ruled off from the rack's own rows above it.
    assert!(vocal_track_menu_entries(false)[0].separator);
}

#[test]
fn each_row_is_its_choice() {
    assert_eq!(
        vocal_track_menu_choice(0, false),
        Some(VocalTrackChoice::Import)
    );
    assert_eq!(
        vocal_track_menu_choice(1, false),
        Some(VocalTrackChoice::Export(VocalFileFormat::Midi))
    );
    assert_eq!(
        vocal_track_menu_choice(2, false),
        Some(VocalTrackChoice::Export(VocalFileFormat::Ustx))
    );
    assert_eq!(
        vocal_track_menu_choice(3, false),
        Some(VocalTrackChoice::Freeze)
    );
    assert_eq!(
        vocal_track_menu_choice(3, true),
        Some(VocalTrackChoice::Unfreeze)
    );
    assert_eq!(
        vocal_track_menu_choice(4, false),
        Some(VocalTrackChoice::RenderSelection)
    );
    assert_eq!(vocal_track_menu_choice(4, true), None);
    assert_eq!(vocal_track_menu_choice(5, false), None);
}
