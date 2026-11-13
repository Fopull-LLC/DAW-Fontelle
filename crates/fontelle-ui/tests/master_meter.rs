//! What a Master's window says about the song going through it (Ty,
//! 2026-11-12: *"loudness readout on master would be good"*): how loud it
//! comes out, what it is aiming at, how far the leveler is riding it, and how
//! hard the ceiling is holding it — four cells along the top of the window.

use fontelle_ui::canvas::{
    MASTER_METER_HEIGHT, MasterMeterView, master_meter_cells, master_meter_rects,
};
use fontelle_ui::layout::Rect;

fn view() -> MasterMeterView {
    MasterMeterView {
        loudness_lufs: -14.24,
        target_lufs: Some(-14.0),
        level_db: 6.06,
        reduction_db: 1.27,
    }
}

#[test]
fn it_says_how_loud_what_it_aims_at_and_what_it_is_doing() {
    let cells = master_meter_cells(&view());
    assert_eq!(
        cells,
        [
            ("Loudness", "-14.2 LUFS".to_string()),
            ("Target", "-14 LUFS".to_string()),
            ("Leveling", "+6.1 dB".to_string()),
            ("Ceiling", "-1.3 dB".to_string()),
        ]
    );
}

#[test]
fn silence_reads_as_a_dash_and_leveling_off_says_off() {
    let cells = master_meter_cells(&MasterMeterView {
        loudness_lufs: -70.0,
        target_lufs: None,
        level_db: 0.0,
        reduction_db: 0.0,
    });
    assert_eq!(cells[0].1, "\u{2014}", "nothing playing is not a number");
    assert_eq!(cells[1].1, "Off");
    assert_eq!(cells[2].1, "+0.0 dB");
    assert_eq!(cells[3].1, "0.0 dB", "a ceiling holding nothing");
}

#[test]
fn the_cells_share_the_strip_and_stay_inside_it() {
    let strip = Rect::new(10.0, 20.0, 600.0, MASTER_METER_HEIGHT);
    let rects = master_meter_rects(strip);
    assert_eq!(rects.len(), 4);
    for pair in rects.windows(2) {
        assert!(pair[0].right() <= pair[1].x, "side by side: {rects:?}");
    }
    for r in rects {
        assert!(r.x >= strip.x && r.right() <= strip.right() + 0.01);
        assert!(r.y >= strip.y && r.bottom() <= strip.bottom() + 0.01);
        assert!(r.width > 100.0, "room for \"-14.2 LUFS\": {r:?}");
    }
}
