//! The strip along the top of a Master's window: how loud the song comes out,
//! what it is aimed at, how far the leveler is riding it, and how hard the
//! ceiling is holding it (Ty, 2026-11-12: *"loudness readout on master would
//! be good"*).
//!
//! Four numbers and no picture, because the knob most people touch is the
//! target, and the question it answers is "how far from it am I" — a number
//! beside a number.

use crate::layout::Rect;

/// How tall the strip is: a caption over a value.
pub const MASTER_METER_HEIGHT: f32 = 40.0;

/// Below this the song is silent and the loudness is a dash, not a number.
const SILENT_LUFS: f32 = -60.0;

/// What a Master's window is told each frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MasterMeterView {
    pub loudness_lufs: f32,
    /// `None` when leveling is off: there is nothing it aims at.
    pub target_lufs: Option<f32>,
    pub level_db: f32,
    /// The most the ceiling held it down lately, in positive dB.
    pub reduction_db: f32,
}

/// The four cells' captions and values, in order. The same function the
/// window shapes from and draws from.
pub fn master_meter_cells(view: &MasterMeterView) -> [(&'static str, String); 4] {
    let loudness = if view.loudness_lufs <= SILENT_LUFS {
        "\u{2014}".to_string()
    } else {
        format!("{:.1} LUFS", view.loudness_lufs)
    };
    let target = match view.target_lufs {
        Some(target) => format!("{target:.0} LUFS"),
        None => "Off".to_string(),
    };
    // A ceiling holding nothing is no cut at all, not a "-0.0".
    let ceiling = if view.reduction_db < 0.05 {
        "0.0 dB".to_string()
    } else {
        format!("{:.1} dB", -view.reduction_db)
    };
    [
        ("Loudness", loudness),
        ("Target", target),
        ("Leveling", format!("{:+.1} dB", view.level_db)),
        ("Ceiling", ceiling),
    ]
}

/// The four cells, side by side across `strip`.
pub fn master_meter_rects(strip: Rect) -> [Rect; 4] {
    const GAP: f32 = 6.0;
    let width = ((strip.width - GAP * 3.0) / 4.0).max(0.0);
    std::array::from_fn(|i| {
        Rect::new(
            strip.x + i as f32 * (width + GAP),
            strip.y,
            width,
            strip.height,
        )
    })
}

/// Makes room for the strip under `layout`'s name field: everything below the
/// name moves down by the strip and a gap, and the strip's rect is returned.
pub fn master_meter_room(layout: &mut crate::canvas::InstrumentLayout) -> Rect {
    const GAP: f32 = 6.0;
    let strip = Rect::new(
        layout.body.x,
        layout.name.bottom() + GAP,
        layout.body.width,
        MASTER_METER_HEIGHT,
    );
    let down = MASTER_METER_HEIGHT + GAP;
    for (_, rect) in &mut layout.keys {
        rect.y += down;
    }
    for (_, rect) in &mut layout.headings {
        rect.y += down;
    }
    for (_, _, rect) in &mut layout.cells {
        rect.y += down;
    }
    layout.content_height += down;
    strip
}
